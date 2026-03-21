//! Terminal UI frontend for Phosphor.

mod app;
pub mod debug_log;
pub mod state;
mod theme;
mod ui;

use anyhow::Result;
use phosphor_core::EngineConfig;

/// Run the TUI application.
pub fn run(config: EngineConfig, enable_audio: bool, enable_midi: bool) -> Result<()> {
    debug_log::init();
    let mut app = app::App::new(config, enable_audio, enable_midi);
    app.run()
}
