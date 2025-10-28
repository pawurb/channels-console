# Real-time monitoring and metrics for your Tokio channels
[![Latest Version](https://img.shields.io/crates/v/tokio-channels-console.svg)](https://crates.io/crates/tokio-channels-console) [![GH Actions](https://github.com/pawurb/tokio-channels-console/actions/workflows/ci.yml/badge.svg)](https://github.com/pawurb/tokio-channels-console/actions)

![Console TUI Example](console-tui2.gif)

This crate provides an easy-to-configure way to monitor Tokio channels. Track per-channel metrics such as queue depth, send/receive rates, and memory usage, directly from your terminal.

[tokio-console](https://github.com/tokio-rs/console) is an awesome project, but it currently provides insights only into tasks and resources. [There are plans](https://github.com/tokio-rs/console/issues/278) to add support for `mpsc` channels, but it's still in progress. This lib tries to fill this gap, but offering a _quick & easy_ way to instrument Tokio channels in any Rust application.

## Features

- **Zero-cost when disabled** — fully gated by a feature flag
- **Minimal configuration** - just one `instrument!` macro to start collecting metrics
- **Detailed stats** - per channel status, sent/received messages, queue capacity, and memory usage 
- **Background processing** - minimal profiling impact
- **Live monitoring** - view metrics in a clear, real-time TUI dashboard (built with [ratatui.rs](https://ratatui.rs/))

## Getting started

`Cargo.toml`

```toml
tokio-channels-console = { version = "0.1", optional = true }

[features]
tokio-channels-console = ["tokio-channels-console/tokio-channels-console"]
```

This config ensures that the lib has **zero** overhead unless explicitly enabled via a `tokio-channels-console` feature.

Next use `instrument!` macro to monitor selected Tokio channels:

```rust
let (tx1, rx1) = mpsc::channel::<i32>(10);
#[cfg(feature = "tokio-channels-console")]
let (tx1, rx1) = tokio_channels_console::instrument!((tx1, rx1));

let (tx2, rx2) = mpsc::unbounded_channel::<UserData>();
#[cfg(feature = "tokio-channels-console")]
let (tx2, rx2) = tokio_channels_console::instrument!((tx2, rx2));
```

This is the only change you have to do in your codebase. `instrument!` macro returns exactly the same channel types so it remains 100% compatible.

Currently the library supports: 
- `sync::mpsc::channel`
- `sync::mpsc::unbounded_channel` 
- `sync::oneshot` 

Now, install `tokio-channels-console` TUI:

```bash
cargo install tokio-channels-console --features=tui
```

Execute your program with `--features=tokio-channels-console`:

```bash
cargo run --features=tokio-channels-console
```

In a different terminal run `tokio-channels-console` CLI to start the TUI and see live usage metrics:

```bash
tokio-channels-console
```

![Console Dashboard](console-dashboard2.png)

### Quickstart demo guide

1. Install CLI:

```bash
cargo install tokio-channels-console --features=tui
```

2. Clone this repo:

```bash
git clone git@github.com:pawurb/tokio-channels-console.git
```

3. Run `console_feed` example:

```bash
cd tokio-channels-console
cargo run --example console_feed --features=tokio-channels-console
```

4. Run TUI (in a different terminal):

```bash
tokio-channels-console
```

## How it works?

`instrument!` wraps Tokio channels with lightweight proxies that transparently forward all messages while collecting real-time statistics. Each `send` and `recv` operation passes through a monitored proxy channel that emits updates to a background metrics system.

In the background a HTTP server process exposes gathered metrics in a JSON format, allowing TUI process to display them in the interface.

### There be bugs 🐛

This library has just been released. I've tested it with several nontrivial apps, and it consistently produced reliable metrics. However, please note that enabling monitoring can subtly affect channel behavior in some cases. For example, using `try_send` may not return an error as expected, since the proxy layers effectively increase total capacity. I'm actively improving the library, so any feedback, issues, bug reports are very welcome.

## API

### `instrument!` Macro

The `instrument!` macro is the primary way to monitor channels. It wraps channel creation expressions and returns instrumented versions that collect metrics.

**Supported Channel Types:**
- `tokio::sync::mpsc::channel` - Bounded MPSC channels
- `tokio::sync::mpsc::unbounded_channel` - Unbounded MPSC channels
- `tokio::sync::oneshot::channel` - Oneshot channels

**Basic Usage:**

```rust
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    // Create channels normally
    let (tx, rx) = mpsc::channel::<String>(100);

    // Instrument them only when the feature is enabled
    #[cfg(feature = "tokio-channels-console")]
    let (tx, rx) = tokio_channels_console::instrument!((tx, rx));

    // The channel works exactly the same way
    tx.send("Hello".to_string()).await.unwrap();
}
```

**Zero-Cost Abstraction:** When the `tokio-channels-console` feature is disabled, the `#[cfg]` attribute ensures the instrumentation code is completely removed at compile time - there's absolutely zero runtime overhead.

**Note:** The first invocation of `instrument!` automatically starts:
- A background thread for metrics collection
- An HTTP server on `http://127.0.0.1:6770` (default port) exposing metrics in JSON format

This initialization happens only once and is shared across all instrumented channels.

**Channel Labels:**

By default, channels are labeled with their file location and line number (e.g., `src/worker.rs:25`). You can provide custom labels for easier identification:

```rust
let (tx, rx) = mpsc::channel::<Task>(10);
#[cfg(feature = "tokio-channels-console")]
let (tx, rx) = tokio_channels_console::instrument!((tx, rx), label = "task-queue");
```

### `ChannelsGuard` - Printing Statistics on Drop

Similar to the [hotpath API](https://github.com/pawurb/hotpath) the `ChannelsGuard` is a RAII guard that automatically prints channel statistics when dropped (typically at program end). This is useful for debugging and getting a summary of channel usage.

**Basic Usage:**

```rust
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    // Create guard at the start of your program (only when feature is enabled)
    #[cfg(feature = "tokio-channels-console")]
    let _guard = tokio_channels_console::ChannelsGuard::new();

    // Your code with instrumented channels...
    let (tx, rx) = mpsc::channel::<i32>(10);
    #[cfg(feature = "tokio-channels-console")]
    let (tx, rx) = tokio_channels_console::instrument!((tx, rx));

    // ... use your channels ...

    // Statistics will be printed when _guard is dropped (at program end)
}
```

**Output Formats:**

You can customize the output format using `ChannelsGuardBuilder`:

```rust
#[tokio::main]
async fn main() {
    #[cfg(feature = "tokio-channels-console")]
    let _guard = tokio_channels_console::ChannelsGuardBuilder::new()
        .format(tokio_channels_console::Format::Json)
        .build();
}
```

**Output Example (Table Format):**

```
=== Channel Statistics (runtime: 5.23s) ===

+------------------+-------------+--------+------+-------+----------+--------+-------+
| Channel          | Type        | State  | Sent | Mem   | Received | Queued | Mem   |
+------------------+-------------+--------+------+-------+----------+--------+-------+
| task-queue       | bounded[10] | active | 1543 | 12 KB | 1543     | 0      | 0 B   |
| http-responses   | unbounded   | active | 892  | 89 KB | 890      | 2      | 200 B |
| shutdown-signal  | oneshot     | closed | 1    | 8 B   | 1        | 0      | 0 B   |
+------------------+-------------+--------+------+-------+----------+--------+-------+
```

## Configuration

### Metrics Server Port

The HTTP metrics server runs on port `6770` by default. You can customize this using the `TOKIO_CHANNELS_CONSOLE_METRICS_PORT` environment variable:

```bash
TOKIO_CHANNELS_CONSOLE_METRICS_PORT=8080 cargo run --features tokio-channels-console
```

When using the TUI console, specify the matching port with the `--metrics-port` flag:

```bash
tokio-channels-console --metrics-port 8080
```
