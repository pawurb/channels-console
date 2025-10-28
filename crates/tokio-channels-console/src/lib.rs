use tokio::sync::mpsc::{Receiver, Sender, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;

use crossbeam_channel::{unbounded, Sender as CbSender};
use prettytable::{Cell, Row, Table};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;
use tiny_http::{Response, Server};

mod wrappers;
use wrappers::{wrap_channel, wrap_oneshot, wrap_unbounded};

/// Type of a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelType {
    Bounded(usize),
    Unbounded,
    Oneshot,
}

impl std::fmt::Display for ChannelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelType::Bounded(size) => write!(f, "bounded[{}]", size),
            ChannelType::Unbounded => write!(f, "unbounded"),
            ChannelType::Oneshot => write!(f, "oneshot"),
        }
    }
}

impl Serialize for ChannelType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ChannelType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        match s.as_str() {
            "unbounded" => Ok(ChannelType::Unbounded),
            "oneshot" => Ok(ChannelType::Oneshot),
            _ => {
                // try: bounded[123]
                if let Some(inner) = s.strip_prefix("bounded[").and_then(|x| x.strip_suffix(']')) {
                    let size = inner
                        .parse()
                        .map_err(|_| serde::de::Error::custom("invalid bounded size"))?;
                    Ok(ChannelType::Bounded(size))
                } else {
                    Err(serde::de::Error::custom("invalid channel type"))
                }
            }
        }
    }
}

/// Format of the output produced by ChannelsGuard on drop.
#[derive(Clone, Copy, Debug, Default)]
pub enum Format {
    #[default]
    Table,
    Json,
    JsonPretty,
}

/// State of a instrumented channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelState {
    #[default]
    Active,
    Closed,
    Full,
    Notified,
}

impl std::fmt::Display for ChannelState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl ChannelState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChannelState::Active => "active",
            ChannelState::Closed => "closed",
            ChannelState::Full => "full",
            ChannelState::Notified => "notified",
        }
    }
}

impl Serialize for ChannelState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ChannelState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "active" => Ok(ChannelState::Active),
            "closed" => Ok(ChannelState::Closed),
            "full" => Ok(ChannelState::Full),
            "notified" => Ok(ChannelState::Notified),
            _ => Err(serde::de::Error::custom("invalid channel state")),
        }
    }
}

/// Statistics for a single instrumented channel.
#[derive(Debug, Clone)]
pub(crate) struct ChannelStats {
    /// ID of the channel (full path used as HashMap key).
    pub(crate) id: &'static str,
    /// Optional user label; if None, display derives from `id`.
    pub(crate) label: Option<&'static str>,
    /// Type of channel.
    pub(crate) channel_type: ChannelType,
    /// Current state of the channel.
    pub(crate) state: ChannelState,
    /// Number of messages sent through this channel.
    pub(crate) sent_count: u64,
    /// Number of messages received from this channel.
    pub(crate) received_count: u64,
    /// Type name of messages in this channel.
    pub(crate) type_name: &'static str,
    /// Size in bytes of the message type.
    pub(crate) type_size: usize,
}

impl ChannelStats {
    pub fn queued(&self) -> u64 {
        self.sent_count.saturating_sub(self.received_count)
    }

    /// Calculate total bytes sent through this channel.
    pub fn total_bytes(&self) -> u64 {
        self.sent_count * self.type_size as u64
    }

    /// Calculate bytes currently queued in this channel.
    pub fn queued_bytes(&self) -> u64 {
        self.queued() * self.type_size as u64
    }
}

/// Serializable version of channel statistics for JSON responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableChannelStats {
    /// ID of the channel.
    pub id: String,
    /// Optional user label; if None, display derives from `id`.
    pub label: String,
    /// Type of channel (includes capacity for bounded channels).
    pub channel_type: ChannelType,
    /// Current state of the channel.
    pub state: ChannelState,
    /// Number of messages sent through this channel.
    pub sent_count: u64,
    /// Number of messages received from this channel.
    pub received_count: u64,
    /// Current queue size (sent - received).
    pub queued: u64,
    /// Type name of messages in this channel.
    pub type_name: String,
    /// Size in bytes of the message type.
    pub type_size: usize,
    /// Total bytes sent through this channel.
    pub total_bytes: u64,
    /// Bytes currently queued in this channel.
    pub queued_bytes: u64,
}

impl From<&ChannelStats> for SerializableChannelStats {
    fn from(stats: &ChannelStats) -> Self {
        let label = resolve_label(stats.id, stats.label);
        Self {
            id: stats.id.to_string(),
            label,
            channel_type: stats.channel_type,
            state: stats.state,
            sent_count: stats.sent_count,
            received_count: stats.received_count,
            queued: stats.queued(),
            type_name: stats.type_name.to_string(),
            type_size: stats.type_size,
            total_bytes: stats.total_bytes(),
            queued_bytes: stats.queued_bytes(),
        }
    }
}

impl ChannelStats {
    fn new(
        id: &'static str,
        label: Option<&'static str>,
        channel_type: ChannelType,
        type_name: &'static str,
        type_size: usize,
    ) -> Self {
        Self {
            id,
            label,
            channel_type,
            state: ChannelState::default(),
            sent_count: 0,
            received_count: 0,
            type_name,
            type_size,
        }
    }

    /// Update the channel state based on sent/received counts.
    /// Sets state to Full if sent > received, otherwise Active (unless explicitly closed).
    fn update_state(&mut self) {
        if self.state == ChannelState::Closed || self.state == ChannelState::Notified {
            return;
        }

        if self.sent_count > self.received_count {
            self.state = ChannelState::Full;
        } else {
            self.state = ChannelState::Active;
        }
    }
}

/// Events sent to the background statistics collection thread.
#[derive(Debug)]
pub(crate) enum StatsEvent {
    Created {
        id: &'static str,
        display_label: Option<&'static str>,
        channel_type: ChannelType,
        type_name: &'static str,
        type_size: usize,
    },
    MessageSent {
        id: &'static str,
    },
    MessageReceived {
        id: &'static str,
    },
    Closed {
        id: &'static str,
    },
    Notified {
        id: &'static str,
    },
}

type StatsState = (
    CbSender<StatsEvent>,
    Arc<RwLock<HashMap<&'static str, ChannelStats>>>,
);

/// Global state for statistics collection.
static STATS_STATE: OnceLock<StatsState> = OnceLock::new();

/// Initialize the statistics collection system (called on first instrumented channel).
/// Returns a reference to the global state.
fn init_stats_state() -> &'static StatsState {
    STATS_STATE.get_or_init(|| {
        let (tx, rx) = unbounded::<StatsEvent>();
        let stats_map = Arc::new(RwLock::new(HashMap::<&'static str, ChannelStats>::new()));
        let stats_map_clone = Arc::clone(&stats_map);

        std::thread::Builder::new()
            .name("channel-stats-collector".into())
            .spawn(move || {
                while let Ok(event) = rx.recv() {
                    let mut stats = stats_map_clone.write().unwrap();
                    match event {
                        StatsEvent::Created {
                            id: key,
                            display_label,
                            channel_type,
                            type_name,
                            type_size,
                        } => {
                            stats.insert(
                                key,
                                ChannelStats::new(
                                    key,
                                    display_label,
                                    channel_type,
                                    type_name,
                                    type_size,
                                ),
                            );
                        }
                        StatsEvent::MessageSent { id } => {
                            if let Some(channel_stats) = stats.get_mut(id) {
                                channel_stats.sent_count += 1;
                                channel_stats.update_state();
                            }
                        }
                        StatsEvent::MessageReceived { id } => {
                            if let Some(channel_stats) = stats.get_mut(id) {
                                channel_stats.received_count += 1;
                                channel_stats.update_state();
                            }
                        }
                        StatsEvent::Closed { id } => {
                            if let Some(channel_stats) = stats.get_mut(id) {
                                channel_stats.state = ChannelState::Closed;
                            }
                        }
                        StatsEvent::Notified { id } => {
                            if let Some(channel_stats) = stats.get_mut(id) {
                                channel_stats.state = ChannelState::Notified;
                            }
                        }
                    }
                }
            })
            .expect("Failed to spawn channel-stats-collector thread");

        // Spawn the metrics HTTP server in the background
        // Check environment variable for custom port, default to 6770
        let port = std::env::var("TOKIO_CHANNELS_CONSOLE_METRICS_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(6770);
        let addr = format!("127.0.0.1:{}", port);

        std::thread::spawn(move || {
            start_metrics_server(&addr);
        });

        (tx, stats_map)
    })
}

fn resolve_label(id: &'static str, provided: Option<&'static str>) -> String {
    if let Some(l) = provided {
        return l.to_string();
    }
    if let Some(pos) = id.rfind(':') {
        let (path, line_part) = id.split_at(pos);
        let line = &line_part[1..];
        format!("{}:{}", extract_filename(path), line)
    } else {
        extract_filename(id)
    }
}

fn extract_filename(path: &str) -> String {
    let components: Vec<&str> = path.split('/').collect();
    if components.len() >= 2 {
        format!(
            "{}/{}",
            components[components.len() - 2],
            components[components.len() - 1]
        )
    } else {
        path.to_string()
    }
}

/// Format bytes into human-readable units (B, KB, MB, GB, TB).
pub fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }

    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[unit_idx])
    } else {
        format!("{:.1} {}", size, UNITS[unit_idx])
    }
}

/// Trait for instrumenting channels.
///
/// This trait is not intended for direct use. Use the `instrument!` macro instead.
#[doc(hidden)]
pub trait Instrument {
    type Output;
    fn instrument(self, channel_id: &'static str, label: Option<&'static str>) -> Self::Output;
}

impl<T: Send + 'static> Instrument for (Sender<T>, Receiver<T>) {
    type Output = (Sender<T>, Receiver<T>);
    fn instrument(self, channel_id: &'static str, label: Option<&'static str>) -> Self::Output {
        wrap_channel(self, channel_id, label)
    }
}

impl<T: Send + 'static> Instrument for (UnboundedSender<T>, UnboundedReceiver<T>) {
    type Output = (UnboundedSender<T>, UnboundedReceiver<T>);
    fn instrument(self, channel_id: &'static str, label: Option<&'static str>) -> Self::Output {
        wrap_unbounded(self, channel_id, label)
    }
}

impl<T: Send + 'static> Instrument for (oneshot::Sender<T>, oneshot::Receiver<T>) {
    type Output = (oneshot::Sender<T>, oneshot::Receiver<T>);
    fn instrument(self, channel_id: &'static str, label: Option<&'static str>) -> Self::Output {
        wrap_oneshot(self, channel_id, label)
    }
}

/// Instrument a channel creation to wrap it with debugging proxies.
/// Currently only supports bounded, unbounded and oneshot channels.
///
/// # Examples
///
/// ```
/// use tokio::sync::mpsc;
/// use tokio_channels_console::instrument;
///
/// #[tokio::main]
/// async fn main() {
///
///    // Create channels normally
///    let (tx, rx) = mpsc::channel::<String>(100);
///
///    // Instrument them only when the feature is enabled
///    #[cfg(feature = "tokio-channels-console")]
///    let (tx, rx) = tokio_channels_console::instrument!((tx, rx));
///
///    // The channel works exactly the same way
///    tx.send("Hello".to_string()).await.unwrap();
/// }
/// ```
///
/// By default, channels are labeled with their file location and line number (e.g., `src/worker.rs:25`). You can provide custom labels for easier identification:
///
/// ```rust,no_run
/// use tokio::sync::mpsc;
/// use tokio_channels_console::instrument;
/// let (tx, rx) = mpsc::channel::<String>(10);
/// #[cfg(feature = "tokio-channels-console")]
/// let (tx, rx) = tokio_channels_console::instrument!((tx, rx), label = "task-queue");
/// ```
///
#[macro_export]
macro_rules! instrument {
    ($expr:expr) => {{
        const CHANNEL_ID: &'static str = concat!(file!(), ":", line!());
        $crate::Instrument::instrument($expr, CHANNEL_ID, None)
    }};

    ($expr:expr, label = $label:literal) => {{
        const CHANNEL_ID: &'static str = concat!(file!(), ":", line!());
        $crate::Instrument::instrument($expr, CHANNEL_ID, Some($label))
    }};
}

fn get_channel_stats() -> HashMap<&'static str, ChannelStats> {
    if let Some((_, stats_map)) = STATS_STATE.get() {
        stats_map.read().unwrap().clone()
    } else {
        HashMap::new()
    }
}

fn get_serializable_stats() -> Vec<SerializableChannelStats> {
    let mut stats: Vec<SerializableChannelStats> = get_channel_stats()
        .values()
        .map(SerializableChannelStats::from)
        .collect();

    stats.sort_by(|a, b| a.id.cmp(&b.id));
    stats
}

fn start_metrics_server(addr: &str) {
    let server = match Server::http(addr) {
        Ok(s) => s,
        Err(e) => {
            panic!("Failed to bind metrics server to {}: {}. Customize the port using the TOKIO_CHANNELS_CONSOLE_METRICS_PORT environment variable.", addr, e);
        }
    };

    println!("Channel metrics server listening on http://{}", addr);

    for request in server.incoming_requests() {
        if request.url() == "/metrics" {
            let stats = get_serializable_stats();
            match serde_json::to_string(&stats) {
                Ok(json) => {
                    let response = Response::from_string(json).with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/json"[..],
                        )
                        .unwrap(),
                    );
                    let _ = request.respond(response);
                }
                Err(e) => {
                    eprintln!("Failed to serialize metrics: {}", e);
                    let response = Response::from_string(format!("Internal server error: {}", e))
                        .with_status_code(500);
                    let _ = request.respond(response);
                }
            }
        } else {
            let response = Response::from_string("Not found").with_status_code(404);
            let _ = request.respond(response);
        }
    }
}

/// Builder for creating a ChannelsGuard with custom configuration.
///
/// # Examples
///
/// ```no_run
/// use tokio_channels_console::{ChannelsGuardBuilder, Format};
///
/// let _guard = ChannelsGuardBuilder::new()
///     .format(Format::JsonPretty)
///     .build();
/// // Statistics will be printed as pretty JSON when _guard is dropped
/// ```
pub struct ChannelsGuardBuilder {
    format: Format,
}

impl ChannelsGuardBuilder {
    /// Create a new channels guard builder.
    pub fn new() -> Self {
        Self {
            format: Format::default(),
        }
    }

    /// Set the output format for statistics.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tokio_channels_console::{ChannelsGuardBuilder, Format};
    ///
    /// let _guard = ChannelsGuardBuilder::new()
    ///     .format(Format::Json)
    ///     .build();
    /// ```
    pub fn format(mut self, format: Format) -> Self {
        self.format = format;
        self
    }

    /// Build and return the ChannelsGuard.
    /// Statistics will be printed when the guard is dropped.
    pub fn build(self) -> ChannelsGuard {
        ChannelsGuard {
            start_time: Instant::now(),
            format: self.format,
        }
    }
}

impl Default for ChannelsGuardBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Guard for channel statistics collection.
/// When dropped, prints a summary of all instrumented channels and their statistics.
///
/// Use `ChannelsGuardBuilder` to create a guard with custom configuration.
///
/// # Examples
///
/// ```no_run
/// use tokio_channels_console::ChannelsGuard;
///
/// let _guard = ChannelsGuard::new();
/// // Your code with instrumented channels here
/// // Statistics will be printed when _guard is dropped
/// ```
pub struct ChannelsGuard {
    start_time: Instant,
    format: Format,
}

impl ChannelsGuard {
    /// Create a new channels guard with default settings (table format).
    /// Statistics will be printed when this guard is dropped.
    ///
    /// For custom configuration, use `ChannelsGuardBuilder::new()` instead.
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            format: Format::default(),
        }
    }

    /// Set the output format for statistics.
    /// This is a convenience method for backward compatibility.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tokio_channels_console::{ChannelsGuard, Format};
    ///
    /// let _guard = ChannelsGuard::new().format(Format::Json);
    /// ```
    pub fn format(mut self, format: Format) -> Self {
        self.format = format;
        self
    }
}

impl Default for ChannelsGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ChannelsGuard {
    fn drop(&mut self) {
        let elapsed = self.start_time.elapsed();
        let stats = get_channel_stats();

        if stats.is_empty() {
            println!("\nNo instrumented channels found.");
            return;
        }

        match self.format {
            Format::Table => {
                let mut table = Table::new();

                table.add_row(Row::new(vec![
                    Cell::new("Channel"),
                    Cell::new("Type"),
                    Cell::new("State"),
                    Cell::new("Sent"),
                    Cell::new("Mem"),
                    Cell::new("Received"),
                    Cell::new("Queued"),
                    Cell::new("Mem"),
                ]));

                let mut sorted_stats: Vec<_> = stats.into_iter().collect();
                sorted_stats.sort_by(|a, b| {
                    let la = resolve_label(a.1.id, a.1.label);
                    let lb = resolve_label(b.1.id, b.1.label);
                    la.cmp(&lb)
                });

                for (_key, channel_stats) in sorted_stats {
                    let label = resolve_label(channel_stats.id, channel_stats.label);
                    table.add_row(Row::new(vec![
                        Cell::new(&label),
                        Cell::new(&channel_stats.channel_type.to_string()),
                        Cell::new(channel_stats.state.as_str()),
                        Cell::new(&channel_stats.sent_count.to_string()),
                        Cell::new(&format_bytes(channel_stats.total_bytes())),
                        Cell::new(&channel_stats.received_count.to_string()),
                        Cell::new(&channel_stats.queued().to_string()),
                        Cell::new(&format_bytes(channel_stats.queued_bytes())),
                    ]));
                }

                println!(
                    "\n=== Channel Statistics (runtime: {:.2}s) ===",
                    elapsed.as_secs_f64()
                );
                table.printstd();
            }
            Format::Json => {
                let serializable_stats = get_serializable_stats();
                match serde_json::to_string(&serializable_stats) {
                    Ok(json) => println!("{}", json),
                    Err(e) => eprintln!("Failed to serialize statistics to JSON: {}", e),
                }
            }
            Format::JsonPretty => {
                let serializable_stats = get_serializable_stats();
                match serde_json::to_string_pretty(&serializable_stats) {
                    Ok(json) => println!("{}", json),
                    Err(e) => eprintln!("Failed to serialize statistics to pretty JSON: {}", e),
                }
            }
        }
    }
}
