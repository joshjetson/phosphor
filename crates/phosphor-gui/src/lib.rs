//! GUI frontend for Phosphor (egui/eframe).
//!
//! Stub — will be implemented in Phase 6.

use anyhow::Result;
use phosphor_core::AudioRequest;

/// `request` is what the command line asked for — usually nothing — not what
/// the engine will run at. When this grows an audio path it must do what
/// `phosphor_tui::app` does: open the `CpalBackend` first, take
/// `CpalBackend::format()`, and build the engine and mixer from that.
/// Building from the request directly is how the whole application ended up
/// running 8.8% sharp.
pub fn run(_request: AudioRequest) -> Result<()> {
    anyhow::bail!("GUI frontend not yet implemented. Use --tui for now.")
}
