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

    /// Request an audio block size in samples (lower = less latency, more
    /// CPU). Left alone, the device chooses. Clamped to what it will accept.
    #[arg(long)]
    buffer_size: Option<u32>,

    /// Request a sample rate in Hz. Left alone, phosphor runs at whatever the
    /// device is already set to rather than changing it. If the device does
    /// not offer the rate asked for, its own is adopted and reported.
    #[arg(long)]
    sample_rate: Option<u32>,

    /// Disable audio output (useful for UI development)
    #[arg(long)]
    no_audio: bool,

    /// Disable MIDI input
    #[arg(long)]
    no_midi: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // A request, not a configuration: both halves are optional and both are
    // usually empty. What the engine runs at is settled against the device,
    // not here — see `phosphor_core::AudioRequest`.
    let request = phosphor_core::AudioRequest {
        buffer_size: cli.buffer_size,
        sample_rate: cli.sample_rate,
    };

    if cli.gui {
        // GUI mode: tracing goes to stderr as usual
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .init();

        tracing::info!(
            "Phosphor v{} starting (requested buffer_size={:?}, sample_rate={:?})",
            env!("CARGO_PKG_VERSION"),
            cli.buffer_size,
            cli.sample_rate,
        );

        phosphor_gui::run(request)
    } else {
        // TUI mode: suppress all tracing output to stderr so it doesn't
        // bleed into the splash screen or the terminal UI.
        // Debug logging goes to phosphor_debug.log via the debug_log module.
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("off"))
            .init();

        phosphor_tui::run(request, !cli.no_audio, !cli.no_midi)
    }
}
