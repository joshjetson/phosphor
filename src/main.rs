use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "phosphor", about = "A terminal & graphical DAW", version)]
struct Cli {
    /// Launch the TUI frontend (default)
    #[arg(long, conflicts_with = "gui")]
    tui: bool,

    /// Launch the GUI frontend
    #[arg(long, conflicts_with = "tui")]
    gui: bool,

    /// Audio buffer size in samples (lower = less latency, more CPU)
    #[arg(long, default_value = "64")]
    buffer_size: u32,

    /// Sample rate in Hz
    #[arg(long, default_value = "44100")]
    sample_rate: u32,

    /// Disable audio output (useful for UI development)
    #[arg(long)]
    no_audio: bool,

    /// Disable MIDI input
    #[arg(long)]
    no_midi: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!(
        "Phosphor v{} starting (buffer_size={}, sample_rate={})",
        env!("CARGO_PKG_VERSION"),
        cli.buffer_size,
        cli.sample_rate,
    );

    let config = phosphor_core::EngineConfig {
        buffer_size: cli.buffer_size,
        sample_rate: cli.sample_rate,
    };

    if cli.gui {
        phosphor_gui::run(config)
    } else {
        phosphor_tui::run(config, !cli.no_audio, !cli.no_midi)
    }
}
