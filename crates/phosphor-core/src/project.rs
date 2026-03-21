//! Shared domain models for the audio engine and UI.
//!
//! These types live in phosphor-core so both the audio thread (mixer)
//! and the UI thread (TUI/GUI) can reference the same data without
//! duplicating definitions. Audio-thread-safe state uses atomics.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::engine::VuLevels;

/// Identifies a track by index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrackId(pub usize);

/// What kind of track this is — determines routing and capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    /// Has a synth/plugin, receives MIDI.
    Instrument,
    /// Plays back audio clips.
    Audio,
    /// Send bus A.
    SendA,
    /// Send bus B.
    SendB,
    /// Master output bus.
    Master,
}

/// Audio-thread-safe track configuration.
///
/// Written by the UI thread, read by the audio thread — all fields
/// are atomic so no locks are needed.
#[derive(Debug)]
pub struct TrackConfig {
    pub muted: AtomicBool,
    pub soloed: AtomicBool,
    pub armed: AtomicBool,
    /// Whether this track is currently selected for MIDI input.
    /// Only one track should be selected at a time.
    pub midi_active: AtomicBool,
    /// Volume stored as f32 bits in an AtomicU32.
    pub volume: AtomicU32,
}

impl TrackConfig {
    pub fn new() -> Self {
        Self {
            muted: AtomicBool::new(false),
            soloed: AtomicBool::new(false),
            armed: AtomicBool::new(false),
            midi_active: AtomicBool::new(false),
            volume: AtomicU32::new(0.75f32.to_bits()),
        }
    }

    pub fn get_volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    pub fn set_volume(&self, v: f32) {
        self.volume.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    pub fn is_soloed(&self) -> bool {
        self.soloed.load(Ordering::Relaxed)
    }

    pub fn is_armed(&self) -> bool {
        self.armed.load(Ordering::Relaxed)
    }

    pub fn is_midi_active(&self) -> bool {
        self.midi_active.load(Ordering::Relaxed)
    }
}

impl Default for TrackConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared handle for a track — the UI holds an `Arc<TrackHandle>` to
/// read VU levels and write mute/solo/arm/volume.
#[derive(Debug)]
pub struct TrackHandle {
    pub id: usize,
    pub kind: TrackKind,
    pub config: TrackConfig,
    pub vu: VuLevels,
}

impl TrackHandle {
    pub fn new(id: usize, kind: TrackKind) -> Self {
        Self {
            id,
            kind,
            config: TrackConfig::new(),
            vu: VuLevels::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_config_defaults() {
        let cfg = TrackConfig::new();
        assert!(!cfg.is_muted());
        assert!(!cfg.is_soloed());
        assert!(!cfg.is_armed());
        assert!((cfg.get_volume() - 0.75).abs() < 0.001);
    }

    #[test]
    fn track_config_volume_round_trip() {
        let cfg = TrackConfig::new();
        cfg.set_volume(0.42);
        assert!((cfg.get_volume() - 0.42).abs() < 0.001);
    }

    #[test]
    fn track_config_atomics() {
        let cfg = TrackConfig::new();
        cfg.muted.store(true, Ordering::Relaxed);
        assert!(cfg.is_muted());
        cfg.soloed.store(true, Ordering::Relaxed);
        assert!(cfg.is_soloed());
        cfg.armed.store(true, Ordering::Relaxed);
        assert!(cfg.is_armed());
    }

    #[test]
    fn track_handle_new() {
        let h = TrackHandle::new(0, TrackKind::Instrument);
        assert_eq!(h.id, 0);
        assert_eq!(h.kind, TrackKind::Instrument);
        assert!(!h.config.is_muted());
    }

    #[test]
    fn track_kind_variants() {
        assert_ne!(TrackKind::Instrument, TrackKind::Audio);
        assert_ne!(TrackKind::SendA, TrackKind::SendB);
        assert_ne!(TrackKind::Master, TrackKind::Audio);
    }
}
