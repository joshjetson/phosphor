//! GUI frontend for Phosphor (egui/eframe).
//!
//! Stub — will be implemented in Phase 6.

use anyhow::Result;
use phosphor_core::EngineConfig;

pub fn run(_config: EngineConfig) -> Result<()> {
    anyhow::bail!("GUI frontend not yet implemented. Use --tui for now.")
}
