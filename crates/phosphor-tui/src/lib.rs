//! Terminal UI frontend for Phosphor.

mod app;
pub mod state;
mod theme;
mod ui;

use anyhow::Result;
use phosphor_core::EngineConfig;

/// Run the TUI application.
pub fn run(config: EngineConfig, enable_audio: bool, enable_midi: bool) -> Result<()> {
    let mut app = app::App::new(config, enable_audio, enable_midi);
    app.run()
}
