//! Terminal UI frontend for Phosphor.

pub mod actions;
mod app;
pub mod debug_log;
pub mod session;
mod splash;
pub mod state;
#[cfg(test)]
mod test_harness;
#[cfg(test)]
mod test_clips;
#[cfg(test)]
mod test_fader;
#[cfg(test)]
mod test_keys;
#[cfg(test)]
mod test_panels;
#[cfg(test)]
mod test_presets;
#[cfg(test)]
mod test_sequencer;
#[cfg(test)]
mod test_session;
mod theme;
mod ui;

use anyhow::Result;
use phosphor_core::AudioRequest;

/// Run the TUI application.
///
/// `request` is what the command line asked for, which is usually nothing at
/// all — see [`AudioRequest`]. What the engine ends up running at is decided
/// by the device, inside [`app::App::new`].
pub fn run(request: AudioRequest, enable_audio: bool, enable_midi: bool) -> Result<()> {
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

    let mut app = app::App::new_with_splash(request, enable_audio, enable_midi)?;
    app.run()
}
