//! Transport state: play, pause, stop, record, loop.
//!
//! The transport is the single source of truth for playback position.
//! The audio thread reads it via atomics — no locks.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};

/// Relaxed ordering — sufficient for single-producer (UI) single-consumer (audio)
/// where we don't need happens-before guarantees across variables.
const ORD: Ordering = Ordering::Relaxed;

/// Playback state readable from any thread without locking.
#[derive(Debug)]
pub struct Transport {
    playing: AtomicBool,
    recording: AtomicBool,
    looping: AtomicBool,
    /// Current position in ticks (960 PPQ).
    position_ticks: AtomicI64,
    /// Tempo in BPM × 100 (e.g., 12000 = 120.00 BPM). Integer atomics avoid f64 issues.
    tempo_centibpm: AtomicU32,
    /// Loop start in ticks.
    loop_start_ticks: AtomicI64,
    /// Loop end in ticks.
    loop_end_ticks: AtomicI64,
}

/// Snapshot of transport state for the UI to display. Cheap to copy.
#[derive(Debug, Clone, Copy)]
pub struct TransportSnapshot {
    pub playing: bool,
    pub recording: bool,
    pub looping: bool,
    pub position_ticks: i64,
    pub tempo_bpm: f64,
    pub loop_start_ticks: i64,
    pub loop_end_ticks: i64,
}

impl Transport {
    /// Ticks per quarter note.
    pub const PPQ: i64 = 960;

    pub fn new(bpm: f64) -> Self {
        Self {
            playing: AtomicBool::new(false),
            recording: AtomicBool::new(false),
            looping: AtomicBool::new(false),
            position_ticks: AtomicI64::new(0),
            tempo_centibpm: AtomicU32::new((bpm * 100.0) as u32),
            loop_start_ticks: AtomicI64::new(0),
            loop_end_ticks: AtomicI64::new(Self::PPQ * 16), // default 4 bars in 4/4
        }
    }

    // -- Controls (called from UI thread) --

    pub fn play(&self) {
        self.playing.store(true, ORD);
    }

    pub fn pause(&self) {
        self.playing.store(false, ORD);
    }

    pub fn stop(&self) {
        self.playing.store(false, ORD);
        self.position_ticks.store(0, ORD);
    }

    pub fn toggle_record(&self) {
        self.recording.fetch_xor(true, ORD);
    }

    pub fn toggle_loop(&self) {
        self.looping.fetch_xor(true, ORD);
    }

    pub fn set_tempo(&self, bpm: f64) {
        self.tempo_centibpm.store((bpm * 100.0) as u32, ORD);
    }

    pub fn set_position(&self, ticks: i64) {
        self.position_ticks.store(ticks, ORD);
    }

    pub fn set_loop_range(&self, start_ticks: i64, end_ticks: i64) {
        self.loop_start_ticks.store(start_ticks, ORD);
        self.loop_end_ticks.store(end_ticks, ORD);
    }

    /// Set loop range by bar numbers (1-based, in 4/4 time).
    /// E.g., bars 1-4 = ticks 0..3840.
    pub fn set_loop_bars(&self, start_bar: u32, end_bar: u32) {
        let ticks_per_bar = Self::PPQ * 4; // 4/4 time
        self.set_loop_range(
            (start_bar.saturating_sub(1) as i64) * ticks_per_bar,
            (end_bar as i64) * ticks_per_bar,
        );
    }

    pub fn loop_start(&self) -> i64 { self.loop_start_ticks.load(ORD) }
    pub fn loop_end(&self) -> i64 { self.loop_end_ticks.load(ORD) }

    /// Start recording within the loop range.
    /// Sets up loop, rewinds to loop start, enables record + play.
    pub fn start_loop_record(&self) {
        self.looping.store(true, ORD);
        self.position_ticks.store(self.loop_start_ticks.load(ORD), ORD);
        self.recording.store(true, ORD);
        self.playing.store(true, ORD);
    }

    /// Stop loop recording. Disables record, stops playback.
    pub fn stop_loop_record(&self) {
        self.recording.store(false, ORD);
        self.playing.store(false, ORD);
    }

    // -- Reads (called from audio thread — lock-free) --

    pub fn is_playing(&self) -> bool {
        self.playing.load(ORD)
    }

    pub fn is_recording(&self) -> bool {
        self.recording.load(ORD)
    }

    pub fn is_looping(&self) -> bool {
        self.looping.load(ORD)
    }

    pub fn position_ticks(&self) -> i64 {
        self.position_ticks.load(ORD)
    }

    pub fn tempo_bpm(&self) -> f64 {
        self.tempo_centibpm.load(ORD) as f64 / 100.0
    }

    /// Advance position by the given number of samples. Handles loop wrapping.
    /// Called from the audio thread each buffer cycle.
    pub fn advance(&self, num_samples: u32, sample_rate: u32) {
        if !self.is_playing() {
            return;
        }

        let bpm = self.tempo_bpm();
        let ticks_per_sample = (bpm * Self::PPQ as f64) / (60.0 * sample_rate as f64);
        let delta = (num_samples as f64 * ticks_per_sample) as i64;

        let mut new_pos = self.position_ticks.load(ORD) + delta;

        if self.is_looping() {
            let loop_end = self.loop_end_ticks.load(ORD);
            let loop_start = self.loop_start_ticks.load(ORD);
            if new_pos >= loop_end && loop_end > loop_start {
                new_pos = loop_start + (new_pos - loop_end) % (loop_end - loop_start);
            }
        }

        self.position_ticks.store(new_pos, ORD);
    }

    /// Take a snapshot for the UI to display.
    pub fn snapshot(&self) -> TransportSnapshot {
        TransportSnapshot {
            playing: self.playing.load(ORD),
            recording: self.recording.load(ORD),
            looping: self.looping.load(ORD),
            position_ticks: self.position_ticks.load(ORD),
            tempo_bpm: self.tempo_bpm(),
            loop_start_ticks: self.loop_start_ticks.load(ORD),
            loop_end_ticks: self.loop_end_ticks.load(ORD),
        }
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new(120.0)
    }
}

/// Convert ticks to bar.beat.tick string (assumes 4/4 time).
pub fn ticks_to_position_string(ticks: i64, ppq: i64) -> String {
    let ticks_per_beat = ppq;
    let ticks_per_bar = ppq * 4; // 4/4 time

    let bar = ticks / ticks_per_bar + 1;
    let beat = (ticks % ticks_per_bar) / ticks_per_beat + 1;
    let tick = ticks % ticks_per_beat;

    format!("{bar}.{beat}.{tick:03}")
}

/// Convert ticks to samples at a given tempo and sample rate.
pub fn ticks_to_samples(ticks: i64, bpm: f64, sample_rate: f64) -> i64 {
    let seconds = ticks as f64 * 60.0 / (bpm * Transport::PPQ as f64);
    (seconds * sample_rate) as i64
}

/// Convert samples to ticks at a given tempo and sample rate.
pub fn samples_to_ticks(samples: i64, bpm: f64, sample_rate: f64) -> i64 {
    let seconds = samples as f64 / sample_rate;
    (seconds * bpm * Transport::PPQ as f64 / 60.0) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_starts_stopped() {
        let t = Transport::default();
        assert!(!t.is_playing());
        assert!(!t.is_recording());
        assert_eq!(t.position_ticks(), 0);
    }

    #[test]
    fn play_pause_stop() {
        let t = Transport::default();
        t.play();
        assert!(t.is_playing());
        t.pause();
        assert!(!t.is_playing());

        // Pause preserves position
        t.set_position(1000);
        t.play();
        t.pause();
        assert_eq!(t.position_ticks(), 1000);

        t.stop();
        assert!(!t.is_playing());
        assert_eq!(t.position_ticks(), 0); // position reset on stop
    }

    #[test]
    fn tempo_set_and_read() {
        let t = Transport::new(140.0);
        assert!((t.tempo_bpm() - 140.0).abs() < 0.01);
        t.set_tempo(95.5);
        assert!((t.tempo_bpm() - 95.5).abs() < 0.01);
    }

    #[test]
    fn advance_moves_position() {
        let t = Transport::new(120.0);
        t.play();
        // At 120 BPM, 960 PPQ, 44100 Hz:
        // ticks_per_sample = 120 * 960 / (60 * 44100) = 0.04354
        // 64 samples = ~2.79 ticks
        t.advance(44100, 44100); // advance 1 second
        let pos = t.position_ticks();
        // 1 second at 120 BPM = 2 beats = 1920 ticks
        assert!(
            (pos - 1920).abs() <= 1,
            "Expected ~1920 ticks after 1s at 120bpm, got {pos}"
        );
    }

    #[test]
    fn advance_does_nothing_when_stopped() {
        let t = Transport::new(120.0);
        t.advance(44100, 44100);
        assert_eq!(t.position_ticks(), 0);
    }

    #[test]
    fn loop_wraps_position() {
        let t = Transport::new(120.0);
        t.set_loop_range(0, 1920); // loop 2 beats
        t.toggle_loop();
        t.play();

        // Advance 3 seconds (= 5760 ticks at 120bpm)
        t.advance(44100 * 3, 44100);
        let pos = t.position_ticks();
        // 5760 % 1920 = 0, so should wrap to 0
        assert!(
            pos < 1920,
            "Position should have wrapped within loop, got {pos}"
        );
    }

    #[test]
    fn position_string_formatting() {
        assert_eq!(ticks_to_position_string(0, 960), "1.1.000");
        assert_eq!(ticks_to_position_string(960, 960), "1.2.000");
        assert_eq!(ticks_to_position_string(3840, 960), "2.1.000");
        assert_eq!(ticks_to_position_string(4000, 960), "2.1.160");
    }

    #[test]
    fn tick_sample_conversion_round_trip() {
        let bpm = 120.0;
        let sr = 44100.0;
        for tick in [0, 480, 960, 1920, 3840, 96000] {
            let samples = ticks_to_samples(tick, bpm, sr);
            let back = samples_to_ticks(samples, bpm, sr);
            assert!(
                (back - tick).abs() <= 1,
                "Round trip failed: {tick} → {samples} → {back}"
            );
        }
    }

    #[test]
    fn snapshot_reflects_current_state() {
        let t = Transport::new(130.0);
        t.play();
        t.toggle_record();
        t.set_position(500);
        let snap = t.snapshot();
        assert!(snap.playing);
        assert!(snap.recording);
        assert_eq!(snap.position_ticks, 500);
        assert!((snap.tempo_bpm - 130.0).abs() < 0.01);
    }
}
