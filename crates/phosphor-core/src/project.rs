//! Shared domain models for the audio engine and UI.
//!
//! These types live in phosphor-core so both the audio thread (mixer)
//! and the UI thread (TUI/GUI) can reference the same data without
//! duplicating definitions. Audio-thread-safe state uses atomics.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

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
    /// Fader position as a linear gain, stored as f32 bits in an AtomicU32.
    ///
    /// Written by the UI thread, read once per buffer by the audio thread.
    /// Constrained to [`TrackConfig::MIN_VOLUME`]..=[`TrackConfig::MAX_VOLUME`]
    /// by [`TrackConfig::set_volume`], which is the only way to write it.
    pub volume: AtomicU32,
}

impl TrackConfig {
    /// Bottom of the fader: silence.
    pub const MIN_VOLUME: f32 = 0.0;

    /// Unity gain — the track reaches the master bus at the level the
    /// instrument produced it.
    pub const UNITY_VOLUME: f32 = 1.0;

    /// Top of the fader, +6 dB.
    ///
    /// Makeup gain above unity, not decoration. The instruments are trimmed
    /// so that ordinary playing peaks near −12 dBFS, which leaves room for
    /// several tracks to sum; a user who is playing one quiet pad on its own
    /// needs somewhere to get that back, and the alternative is the operating
    /// system's volume control, which raises everything else on the machine
    /// too. The master limiter is what makes the top of the range safe.
    pub const MAX_VOLUME: f32 = 2.0;

    /// Where a new track's fader starts, −2.5 dB.
    ///
    /// Below unity so that adding a second and third track does not
    /// immediately need the limiter, and so the fader has visible travel in
    /// both directions before it is touched.
    pub const DEFAULT_VOLUME: f32 = 0.75;

    pub fn new() -> Self {
        Self {
            muted: AtomicBool::new(false),
            soloed: AtomicBool::new(false),
            armed: AtomicBool::new(false),
            midi_active: AtomicBool::new(false),
            volume: AtomicU32::new(Self::DEFAULT_VOLUME.to_bits()),
        }
    }

    pub fn get_volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    /// Set the fader position, clamped to the fader's travel.
    ///
    /// The clamp is here rather than at the call sites because this value is
    /// read on the audio thread and multiplied into every sample of the
    /// track: a caller that computes a position wrongly would otherwise turn
    /// a UI arithmetic slip into a full-scale burst. A NaN is not a fader
    /// position at all, so it is ignored rather than stored — storing it
    /// would multiply the track to NaN, which the master limiter turns into
    /// silence, and a silent track with no visible cause is worse than a
    /// dropped keystroke.
    pub fn set_volume(&self, v: f32) {
        if v.is_nan() {
            return;
        }
        let clamped = v.clamp(Self::MIN_VOLUME, Self::MAX_VOLUME);
        self.volume.store(clamped.to_bits(), Ordering::Relaxed);
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

/// What a sequencer track's pattern player is doing, for the UI to draw.
///
/// Four small atomics rather than a channel, and for the same reason as
/// [`VuLevels`]: the UI redraws on a timer and wants the state *now* rather
/// than a history of it, and publishing must not make the audio thread
/// allocate, block, or care whether anyone is listening.
///
/// The step and the queued slot are things the UI could work out for itself —
/// both are functions of the transport position — but only by reimplementing
/// the audio thread's arithmetic and hoping the two never disagree. Reading
/// what actually played is one store per callback and cannot drift.
#[derive(Debug, Default)]
pub struct PatternStatus {
    /// Which of the eight slots is sounding.
    live_slot: AtomicU8,
    /// The queued slot plus one, or zero when nothing is queued — so that
    /// "nothing" and "slot 0" are different values in one byte.
    queued_slot: AtomicU8,
    /// The step the playhead was over on the last callback.
    step: AtomicU8,
    /// Whether the pattern is running at all.
    running: AtomicBool,
}

impl PatternStatus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called once per callback from the audio thread.
    pub fn publish(&self, live_slot: u8, queued_slot: Option<u8>, step: u8, running: bool) {
        self.live_slot.store(live_slot, Ordering::Relaxed);
        self.queued_slot
            .store(queued_slot.map_or(0, |s| s.saturating_add(1)), Ordering::Relaxed);
        self.step.store(step, Ordering::Relaxed);
        self.running.store(running, Ordering::Relaxed);
    }

    pub fn live_slot(&self) -> u8 {
        self.live_slot.load(Ordering::Relaxed)
    }

    pub fn queued_slot(&self) -> Option<u8> {
        match self.queued_slot.load(Ordering::Relaxed) {
            0 => None,
            n => Some(n - 1),
        }
    }

    pub fn step(&self) -> u8 {
        self.step.load(Ordering::Relaxed)
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
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
    /// Where the step sequencer on this track is, when it has one. Left at
    /// its defaults on every other track, which costs four bytes.
    pub pattern: PatternStatus,
}

impl TrackHandle {
    pub fn new(id: usize, kind: TrackKind) -> Self {
        Self {
            id,
            kind,
            config: TrackConfig::new(),
            vu: VuLevels::new(),
            pattern: PatternStatus::new(),
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
        assert!((cfg.get_volume() - TrackConfig::DEFAULT_VOLUME).abs() < 0.001);
    }

    #[test]
    fn track_config_volume_round_trip() {
        let cfg = TrackConfig::new();
        cfg.set_volume(0.42);
        assert!((cfg.get_volume() - 0.42).abs() < 0.001);
    }

    /// The whole fader travel round-trips, including the makeup gain above
    /// unity that the range was widened for.
    #[test]
    fn track_config_volume_spans_the_fader() {
        let cfg = TrackConfig::new();
        for v in [0.0f32, 0.25, TrackConfig::DEFAULT_VOLUME, TrackConfig::UNITY_VOLUME, 1.5, 2.0] {
            cfg.set_volume(v);
            assert_eq!(cfg.get_volume().to_bits(), v.to_bits(), "fader lost {v}");
        }
    }

    /// The clamp is the reason `volume` has no other writer: whatever the UI
    /// computes, the audio thread only ever sees a value inside the travel.
    #[test]
    fn track_config_volume_is_clamped() {
        let cfg = TrackConfig::new();
        cfg.set_volume(-1.0);
        assert_eq!(cfg.get_volume(), TrackConfig::MIN_VOLUME);
        cfg.set_volume(50.0);
        assert_eq!(cfg.get_volume(), TrackConfig::MAX_VOLUME);
        cfg.set_volume(f32::INFINITY);
        assert_eq!(cfg.get_volume(), TrackConfig::MAX_VOLUME);
        cfg.set_volume(f32::NEG_INFINITY);
        assert_eq!(cfg.get_volume(), TrackConfig::MIN_VOLUME);
    }

    /// A NaN leaves the fader where it was. Storing it would multiply the
    /// track to NaN and the master limiter would render it as silence.
    #[test]
    fn track_config_volume_ignores_nan() {
        let cfg = TrackConfig::new();
        cfg.set_volume(1.25);
        cfg.set_volume(f32::NAN);
        assert_eq!(cfg.get_volume(), 1.25);
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
