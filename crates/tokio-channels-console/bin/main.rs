mod cmd;
use clap::{Parser, Subcommand};
use cmd::console::ConsoleArgs;
use eyre::Result;

#[derive(Subcommand, Debug)]
pub enum TCSubcommand {
    #[command(about = "Start the console TUI")]
    Console(ConsoleArgs),
}

#[derive(Parser, Debug)]
#[command(
    version,
    about,
    long_about = "tokio-channels-console - real-time monitoring and metrics for your Tokio channels"
)]
pub struct TCArgs {
    #[command(subcommand)]
    pub cmd: Option<TCSubcommand>,

    /// Port for the metrics server (used when no subcommand is provided)
    #[arg(long, default_value = "6770", global = true)]
    pub metrics_port: u16,
}

fn main() -> Result<()> {
    let root_args = TCArgs::parse();

    match root_args.cmd {
        Some(TCSubcommand::Console(args)) => {
            args.run()?;
        }
        None => {
            let args = ConsoleArgs {
                metrics_port: root_args.metrics_port,
            };
            args.run()?;
        }
    }

    Ok(())
}
