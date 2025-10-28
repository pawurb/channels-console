use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use eyre::Result;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols::border,
    text::Line,
    widgets::{Block, Cell, Row, Table, Widget},
    DefaultTerminal, Frame,
};
use std::io;
use std::time::{Duration, Instant, SystemTime};
use tokio_channels_console::{format_bytes, ChannelState, ChannelType, SerializableChannelStats};

#[derive(Debug, Parser)]
pub struct ConsoleArgs {
    /// Port for the metrics server
    #[arg(long, default_value = "6770")]
    pub metrics_port: u16,
}

#[derive(Debug)]
pub struct App {
    stats: Vec<SerializableChannelStats>,
    error: Option<String>,
    exit: bool,
    last_refresh: Instant,
    last_successful_fetch: Option<SystemTime>,
    metrics_port: u16,
    last_render_duration: Duration,
}

impl ConsoleArgs {
    pub fn run(&self) -> Result<()> {
        let mut app = App {
            stats: Vec::new(),
            error: None,
            exit: false,
            last_refresh: Instant::now(),
            last_successful_fetch: None,
            metrics_port: self.metrics_port,
            last_render_duration: Duration::from_millis(0),
        };

        let mut terminal = ratatui::init();
        let app_result = app.run(&mut terminal);
        ratatui::restore();
        app_result.map_err(|e| eyre::eyre!("TUI error: {}", e))
    }
}

fn fetch_metrics(port: u16) -> Result<Vec<SerializableChannelStats>> {
    let url = format!("http://127.0.0.1:{}/metrics", port);
    let response = ureq::get(&url).call()?;
    let stats: Vec<SerializableChannelStats> = response.into_json()?;
    Ok(stats)
}

fn format_timestamp(time: std::time::SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Local> = time.into();
    datetime.format("%H:%M:%S").to_string()
}

fn truncate_left(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncated_len = max_len.saturating_sub(3);
        let start_idx = s.len().saturating_sub(truncated_len);
        format!("...{}", &s[start_idx..])
    }
}

fn usage_bar(queued: u64, channel_type: &ChannelType, width: usize) -> Cell<'static> {
    let capacity = match channel_type {
        ChannelType::Bounded(cap) => Some(*cap),
        ChannelType::Oneshot => Some(1),
        ChannelType::Unbounded => None,
    };

    match capacity {
        Some(cap) if cap > 0 => {
            let percentage = (queued as f64 / cap as f64 * 100.0).min(100.0);
            let filled = ((queued as f64 / cap as f64) * width as f64).round() as usize;
            let filled = filled.min(width);
            let empty = width.saturating_sub(filled);

            let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
            let text = format!("{} {:>3.0}%", bar, percentage);

            let color = if percentage >= 100.0 {
                Color::Red
            } else if percentage >= 50.0 {
                Color::Yellow
            } else {
                Color::Green
            };

            Cell::from(text).style(Style::default().fg(color))
        }
        _ => Cell::from(format!("{} N/A", "░".repeat(width))),
    }
}

impl App {
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        const REFRESH_INTERVAL: Duration = Duration::from_millis(200);

        self.refresh_data();

        while !self.exit {
            if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
                self.refresh_data();
            }

            let render_start = Instant::now();
            terminal.draw(|frame| self.draw(frame))?;
            self.last_render_duration = render_start.elapsed();

            self.handle_events()?;
        }
        Ok(())
    }

    fn refresh_data(&mut self) {
        match fetch_metrics(self.metrics_port) {
            Ok(stats) => {
                self.stats = stats;
                self.error = None;
                self.last_successful_fetch = Some(SystemTime::now());
            }
            Err(e) => {
                self.error = Some(format!("Failed to fetch metrics: {}", e));
            }
        }
        self.last_refresh = Instant::now();
    }

    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    fn handle_events(&mut self) -> io::Result<()> {
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key_event) = event::read()? {
                if key_event.kind == KeyEventKind::Press {
                    self.handle_key_event(key_event);
                }
            }
        }
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if let KeyCode::Char('q') = key_event.code {
            self.exit()
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = Line::from(" Tokio Channels Console ".bold());

        let mut status_parts = vec![];

        if let Some(last_fetch) = self.last_successful_fetch {
            let timestamp = format_timestamp(last_fetch);
            status_parts.push(format!("Last refresh: {} ", timestamp));
        }

        if self.error.is_some() && !self.stats.is_empty() {
            status_parts.push("⚠ No fresh metrics available ".to_string());
        }

        let bottom_line = if !status_parts.is_empty() {
            Line::from(vec![
                " Quit ".into(),
                "<Q> ".blue().bold(),
                " | ".into(),
                status_parts.join(" | ").yellow(),
            ])
        } else {
            Line::from(vec![" Quit ".into(), "<Q> ".blue().bold()])
        };

        #[cfg(feature = "dev")]
        let block = {
            let render_time_ms = self.last_render_duration.as_millis();
            let render_time_text = if render_time_ms < 10 {
                format!("  {}ms ", render_time_ms)
            } else {
                format!(" {}ms ", render_time_ms)
            };

            Block::bordered()
                .title(title.centered())
                .title_bottom(bottom_line.centered())
                .title_bottom(Line::from(render_time_text).cyan().right_aligned())
                .border_set(border::THICK)
        };

        #[cfg(not(feature = "dev"))]
        let block = Block::bordered()
            .title(title.centered())
            .title_bottom(bottom_line.centered())
            .border_set(border::THICK);

        if let Some(ref error_msg) = self.error {
            if self.stats.is_empty() {
                let error_text = vec![
                    Line::from(""),
                    Line::from("Error").red().bold().centered(),
                    Line::from(""),
                    Line::from(error_msg.as_str()).red().centered(),
                    Line::from(""),
                    Line::from(format!(
                        "Make sure the metrics server is running on http://127.0.0.1:{}",
                        self.metrics_port
                    ))
                    .yellow()
                    .centered(),
                ];

                ratatui::widgets::Paragraph::new(error_text)
                    .block(block)
                    .render(area, buf);
                return;
            }
        }

        if self.stats.is_empty() {
            let empty_text = vec![
                Line::from(""),
                Line::from("No channel statistics found")
                    .yellow()
                    .centered(),
                Line::from(""),
                Line::from("Make sure channels are instrumented and the server is running")
                    .centered(),
            ];

            ratatui::widgets::Paragraph::new(empty_text)
                .block(block)
                .render(area, buf);
            return;
        }

        let available_width = area.width.saturating_sub(10);
        let channel_width = ((available_width as f32 * 0.22) as usize).max(15);

        let header_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);

        let header = Row::new(vec![
            Cell::from("Channel"),
            Cell::from("Type"),
            Cell::from("State"),
            Cell::from("Sent"),
            Cell::from("Mem"),
            Cell::from("Received"),
            Cell::from("Queued"),
            Cell::from("Mem"),
            Cell::from("Usage"),
        ])
        .style(header_style)
        .height(1);

        let rows: Vec<Row> = self
            .stats
            .iter()
            .map(|stat| {
                let (state_text, state_style) = match stat.state {
                    ChannelState::Active => {
                        (stat.state.to_string(), Style::default().fg(Color::Green))
                    }
                    ChannelState::Closed => {
                        (stat.state.to_string(), Style::default().fg(Color::Yellow))
                    }
                    ChannelState::Full => {
                        (format!("⚠ {}", stat.state), Style::default().fg(Color::Red))
                    }
                    ChannelState::Notified => {
                        (stat.state.to_string(), Style::default().fg(Color::Blue))
                    }
                };

                Row::new(vec![
                    Cell::from(truncate_left(&stat.label, channel_width)),
                    Cell::from(stat.channel_type.to_string()),
                    Cell::from(state_text).style(state_style),
                    Cell::from(stat.sent_count.to_string()),
                    Cell::from(format_bytes(stat.total_bytes)),
                    Cell::from(stat.received_count.to_string()),
                    Cell::from(stat.queued.to_string()),
                    Cell::from(format_bytes(stat.queued_bytes)),
                    usage_bar(stat.queued, &stat.channel_type, 10),
                ])
            })
            .collect();

        let widths = [
            Constraint::Percentage(22), // Channel
            Constraint::Percentage(11), // Type
            Constraint::Percentage(9),  // State
            Constraint::Percentage(7),  // Sent
            Constraint::Percentage(9),  // Mem
            Constraint::Percentage(8),  // Received
            Constraint::Percentage(7),  // Queued
            Constraint::Percentage(9),  // Mem
            Constraint::Percentage(14), // Capacity
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .block(block)
            .column_spacing(1);

        table.render(area, buf);
    }
}
