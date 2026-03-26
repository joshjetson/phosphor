//! Terminal UI frontend for Phosphor.

pub mod actions;
mod app;
pub mod debug_log;
pub mod session;
pub mod state;
#[cfg(test)]
mod test_harness;
mod theme;
mod ui;

use anyhow::Result;
use phosphor_core::EngineConfig;

/// Run the TUI application.
pub fn run(config: EngineConfig, enable_audio: bool, enable_midi: bool) -> Result<()> {
    debug_log::init();
    theme::load_preference();

    // Install panic handler that logs to our debug file before crashing
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        let bt = std::backtrace::Backtrace::force_capture();
        debug_log::log("PANIC", &format!("{msg} at {location}"));
        debug_log::log("PANIC", &format!("backtrace:\n{bt}"));
        default_hook(info);
    }));

    let mut app = app::App::new(config, enable_audio, enable_midi);
    app.run()
}
