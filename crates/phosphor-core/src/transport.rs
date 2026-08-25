//! Transport state: play, pause, stop, record, loop.
//!
//! The transport is the single source of truth for playback position.
//! The audio thread reads it via atomics — no locks.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};

/// Relaxed ordering — sufficient for single-producer (UI) single-consumer (audio)
/// where we don't need happens-before guarantees across variables.
const ORD: Ordering = Ordering::Relaxed;

/// Playback state readable from any thread without locking.
#[derive(Debug)]
pub struct Transport {
    playing: AtomicBool,
    recording: AtomicBool,
    looping: AtomicBool,
    metronome: AtomicBool,
    /// Current position in ticks (960 PPQ).
    position_ticks: AtomicI64,
    /// Tempo in BPM × 100 (e.g., 12000 = 120.00 BPM). Integer atomics avoid f64 issues.
    tempo_centibpm: AtomicU32,
    /// Loop start in ticks.
    loop_start_ticks: AtomicI64,
    /// Loop end in ticks.
    loop_end_ticks: AtomicI64,
    /// Sub-tick remainder carried from one `advance` call to the next, held as
    /// a numerator over `6000 * sample_rate` (see [`Transport::advance`]).
    ///
    /// Written only by the audio thread in `advance`; the UI thread only ever
    /// zeroes it, and only when it moves the playhead.
    ///
    /// Integer rather than f64 bits on purpose: the per-block delta is an exact
    /// rational, so an integer carry makes the position *exactly*
    /// `floor(total_samples * ticks_per_sample)` for any block sequence, for as
    /// long as the session runs. An f64 carry would still round twice per block
    /// and can land a few parts in 10^15 under a whole tick — enough to put the
    /// end of bar 4 at 15359 ticks instead of 15360, at every block size at
    /// 44.1 kHz.
    tick_residual: AtomicU64,
}

/// Snapshot of transport state for the UI to display. Cheap to copy.
#[derive(Debug, Clone, Copy)]
pub struct TransportSnapshot {
    pub playing: bool,
    pub recording: bool,
    pub looping: bool,
    pub metronome: bool,
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
            metronome: AtomicBool::new(false),
            position_ticks: AtomicI64::new(0),
            tempo_centibpm: AtomicU32::new((bpm * 100.0) as u32),
            loop_start_ticks: AtomicI64::new(0),
            loop_end_ticks: AtomicI64::new(Self::PPQ * 4 * 4), // default 4 bars
            tick_residual: AtomicU64::new(0),
        }
    }

    /// Drop the carried sub-tick remainder. Called wherever the playhead jumps:
    /// the fraction describes where we were inside the tick we just left, so it
    /// is meaningless at the new position. Tempo changes do *not* clear it — the
    /// remainder is a fraction of a tick, not of a sample, so it stays true when
    /// ticks-per-sample changes underneath it.
    fn clear_residual(&self) {
        self.tick_residual.store(0, ORD);
    }

    // -- Controls (called from UI thread) --

    pub fn play(&self) {
        self.playing.store(true, ORD);
    }

    /// Pause in place. Keeps the sub-tick remainder: the playhead does not
    /// move, so resuming continues the same tick we were partway through.
    pub fn pause(&self) {
        self.playing.store(false, ORD);
    }

    pub fn stop(&self) {
        self.playing.store(false, ORD);
        self.position_ticks.store(0, ORD);
        self.clear_residual();
    }

    pub fn toggle_record(&self) {
        self.recording.fetch_xor(true, ORD);
    }

    pub fn toggle_loop(&self) {
        self.looping.fetch_xor(true, ORD);
    }

    pub fn toggle_metronome(&self) {
        self.metronome.fetch_xor(true, ORD);
    }

    pub fn is_metronome_on(&self) -> bool {
        self.metronome.load(ORD)
    }

    pub fn set_tempo(&self, bpm: f64) {
        self.tempo_centibpm.store((bpm * 100.0) as u32, ORD);
    }

    pub fn set_position(&self, ticks: i64) {
        self.position_ticks.store(ticks, ORD);
        self.clear_residual();
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
        self.clear_residual(); // playhead jumps to the loop start
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
    ///
    /// A block is almost never a whole number of ticks: at 120 BPM / 44.1 kHz a
    /// 470-frame block (what a real device hands us) is 20.46 ticks. Dropping
    /// that fraction every block ran the whole session 2.3% slow against the
    /// wall clock — 28% slow at 64 frames, and at 96 kHz with 47-frame blocks
    /// the playhead never moved at all. Every consumer reads this one counter,
    /// so nothing looked wrong internally; it was only wrong against real time.
    ///
    /// So the fraction is carried. The exact delta for a block is the rational
    ///
    /// ```text
    ///     num_samples * centibpm * PPQ / (6000 * sample_rate)
    /// ```
    ///
    /// (tempo is stored as BPM×100, and ticks/second = BPM × PPQ / 60). We keep
    /// the division's remainder in `tick_residual` and feed it back in on the
    /// next block, which makes the position exactly
    /// `floor(total_samples × ticks_per_sample)` after any sequence of blocks —
    /// exact, not merely closer. Cost is one 128-bit divmod per *buffer*, and
    /// no allocation or locking.
    pub fn advance(&self, num_samples: u32, sample_rate: u32) {
        if !self.is_playing() || sample_rate == 0 {
            return;
        }

        let den = 6000u128 * u128::from(sample_rate);
        // `min` only bites if the sample rate dropped since the last block, in
        // which case the carried fraction is over the wrong denominator; the
        // one-time error is under a tick and the alternative is storing the
        // denominator alongside it, which is a second atomic for a case that
        // only happens on a device switch.
        let carried = u128::from(self.tick_residual.load(ORD)).min(den - 1);
        let numer = u128::from(num_samples)
            * u128::from(self.tempo_centibpm.load(ORD))
            * Self::PPQ as u128
            + carried;
        let delta = (numer / den).min(i64::MAX as u128) as i64;
        self.tick_residual.store((numer % den) as u64, ORD);

        let mut new_pos = self.position_ticks.load(ORD).saturating_add(delta);

        // The wrap below is integer modulo on whole ticks and the residual is
        // strictly sub-tick, so the carry survives a loop wrap untouched.
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
            metronome: self.metronome.load(ORD),
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
    use std::fmt::Write as _;

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
        // 1 second at 120 BPM = 2 beats = 1920 ticks, on the nose
        assert_eq!(pos, 1920, "Expected 1920 ticks after 1s at 120bpm, got {pos}");
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


    #[test]
    fn loop_wraps_at_boundary() {
        let t = Transport::new(120.0);
        t.set_loop_range(0, 7680); // 2 bars
        t.toggle_loop();
        t.play();

        // At 120bpm, 44100Hz, 256 samples/buffer: ~11 ticks/buffer
        // 7680 / 11 ≈ 698 buffers needed
        let mut wrapped = false;
        for _ in 0..800 {
            let before = t.position_ticks();
            t.advance(256, 44100);
            if t.position_ticks() < before {
                wrapped = true;
                break;
            }
        }
        assert!(wrapped, "Loop should have wrapped. pos={}", t.position_ticks());
    }

    // ---- Wall-clock exactness ----
    //
    // Every consumer (clips, metronome, recording, the step sequencer) reads
    // the same `position_ticks`, so a per-block rounding error is invisible to
    // any test that compares ticks against ticks — the whole session is
    // consistently wrong together. These tests compare ticks against *samples*.

    /// Ticks elapsed over `samples` samples, computed exactly with integer
    /// arithmetic: `samples * centibpm * PPQ / (6000 * sample_rate)`, floored.
    /// Same rational `advance` evaluates, evaluated without any rounding.
    /// `bpm` is quantised to centi-BPM first because that is what the
    /// transport actually stores.
    fn exact_ticks(samples: u64, bpm: f64, sample_rate: u32) -> i64 {
        let centibpm = (bpm * 100.0) as u128;
        ((u128::from(samples) * centibpm * Transport::PPQ as u128)
            / (6000u128 * u128::from(sample_rate))) as i64
    }

    /// Feed exactly `total_samples` to `advance` in `block`-sized chunks, the
    /// last one short so the sample total lands on the nose.
    fn feed(t: &Transport, total_samples: u64, block: u32, sr: u32) {
        let mut fed = 0u64;
        while fed < total_samples {
            let n = block.min((total_samples - fed) as u32);
            t.advance(n, sr);
            fed += u64::from(n);
        }
    }

    const RATES: [u32; 3] = [44_100, 48_000, 96_000];
    /// 470 is what this machine's audio device actually hands us.
    const BLOCKS: [u32; 5] = [64, 128, 470, 512, 1024];

    #[test]
    fn four_bars_take_exactly_eight_seconds() {
        // 4 bars of 4/4 at 120 BPM = 4 * 4 * 960 ticks, and 8.000 s of samples.
        const WANT: i64 = 4 * 4 * Transport::PPQ;
        let mut table = String::new();
        let mut bad = 0usize;
        for sr in RATES {
            for block in BLOCKS {
                let t = Transport::new(120.0);
                t.play();
                feed(&t, u64::from(sr) * 8, block, sr);
                let pos = t.position_ticks();
                let err = pos - WANT;
                if err != 0 {
                    bad += 1;
                }
                let _ = writeln!(
                    table,
                    "  {sr} Hz / {block:>4} frames: {pos:>5} ticks (want {WANT}), \
                     off {err:>5} = {:>6.3}% slow",
                    -100.0 * err as f64 / f64::from(WANT as i32)
                );
            }
        }
        assert_eq!(bad, 0, "transport drifts against the wall clock:\n{table}");
    }

    #[test]
    fn every_tempo_block_size_and_rate_is_exact() {
        for bpm in [60.0, 118.0, 120.0, 174.3, 300.0] {
            for sr in RATES {
                for block in BLOCKS {
                    let t = Transport::new(bpm);
                    t.play();
                    let total = u64::from(sr) * 11 + 337; // odd tail on purpose
                    feed(&t, total, block, sr);
                    assert_eq!(
                        t.position_ticks(),
                        exact_ticks(total, bpm, sr),
                        "{bpm} BPM, {sr} Hz, {block}-frame blocks drifted"
                    );
                }
            }
        }
    }

    #[test]
    fn an_hour_of_odd_blocks_stays_exact() {
        // 47-sample blocks for an hour: ~3.4M chances to drop a fraction.
        for sr in RATES {
            let t = Transport::new(120.0);
            t.play();
            let total = u64::from(sr) * 3600;
            feed(&t, total, 47, sr);
            let want = exact_ticks(total, 120.0, sr);
            let got = t.position_ticks();
            assert_eq!(
                got, want,
                "an hour of 47-frame blocks at {sr} Hz drifted by {} ticks",
                got - want
            );
        }
    }

    #[test]
    fn tempo_change_mid_run_keeps_the_residual_valid() {
        // The residual is a fraction of a *tick*, not of a sample, so it stays
        // meaningful when ticks-per-sample changes underneath it.
        for (a, b) in [(120.0, 174.3), (174.3, 60.0), (60.0, 300.0)] {
            for sr in RATES {
                let t = Transport::new(a);
                t.play();
                let first = u64::from(sr) * 5 + 13; // neither leg is block-aligned
                let second = u64::from(sr) * 7 + 401;
                feed(&t, first, 470, sr);
                assert_eq!(
                    t.position_ticks(),
                    exact_ticks(first, a, sr),
                    "{a} BPM leg drifted at {sr} Hz"
                );
                t.set_tempo(b);
                feed(&t, second, 470, sr);
                // Exact total: floor((first*centi_a + second*centi_b) * PPQ / (6000*sr))
                let want = ((u128::from(first) * (a * 100.0) as u128
                    + u128::from(second) * (b * 100.0) as u128)
                    * Transport::PPQ as u128
                    / (6000u128 * u128::from(sr))) as i64;
                assert_eq!(
                    t.position_ticks(),
                    want,
                    "{a} -> {b} BPM at {sr} Hz lost the carry across the change"
                );
            }
        }
    }

    #[test]
    fn loop_wrap_keeps_the_sub_tick_residual() {
        // The wrap is integer modulo on whole ticks and the residual is
        // strictly sub-tick, so it must survive a wrap untouched. Compare a
        // looping transport against a free-running one folded into the loop.
        let sr = 44_100u32;
        let (start, end) = (Transport::PPQ, Transport::PPQ * 3); // beats 2-3, not at 0
        let looped = Transport::new(174.3);
        looped.set_loop_range(start, end);
        looped.toggle_loop();
        looped.set_position(start);
        looped.play();

        let free = Transport::new(174.3);
        free.set_position(start);
        free.play();

        let total = u64::from(sr) * 30;
        let mut fed = 0u64;
        while fed < total {
            let n = 470.min((total - fed) as u32);
            looped.advance(n, sr);
            free.advance(n, sr);
            fed += u64::from(n);
        }

        let len = end - start;
        let want = start + (free.position_ticks() - start).rem_euclid(len);
        assert!(free.position_ticks() > end + len * 20, "test should wrap many times");
        assert_eq!(
            looped.position_ticks(),
            want,
            "looping lost time against free-running playback"
        );
    }

    #[test]
    fn stop_clears_the_residual() {
        // stop() rewinds to 0; a fraction of a tick from the old timeline must
        // not tip the first tick of the new one.
        let sr = 44_100u32;
        let t = Transport::new(120.0);
        t.play();
        t.advance(22, sr); // 0.958 of a tick, position still 0
        assert_eq!(t.position_ticks(), 0);
        t.stop();
        t.play();
        t.advance(1, sr); // 0.044 of a tick on its own
        assert_eq!(t.position_ticks(), 0, "stop() left a residual behind");
    }

    #[test]
    fn set_position_clears_the_residual() {
        let sr = 44_100u32;
        let t = Transport::new(120.0);
        t.play();
        t.advance(22, sr);
        t.set_position(500);
        t.advance(1, sr);
        assert_eq!(t.position_ticks(), 500, "set_position() left a residual behind");
    }

    #[test]
    fn start_loop_record_clears_the_residual() {
        let sr = 44_100u32;
        let t = Transport::new(120.0);
        t.play();
        t.advance(22, sr);
        t.set_loop_bars(2, 2);
        t.start_loop_record();
        let start = t.loop_start();
        t.advance(1, sr);
        assert_eq!(t.position_ticks(), start, "start_loop_record() left a residual behind");
    }

    #[test]
    fn pause_keeps_the_residual() {
        // Pause does not move the playhead, so the sub-tick phase is still
        // true when we resume; dropping it would lose up to a tick per pause.
        let sr = 44_100u32;
        let t = Transport::new(120.0);
        t.play();
        t.advance(22, sr); // 0.958 of a tick
        assert_eq!(t.position_ticks(), 0);
        t.pause();
        t.advance(44_100, sr); // ignored while paused
        assert_eq!(t.position_ticks(), 0);
        t.play();
        t.advance(1, sr); // 0.958 + 0.044 crosses the tick boundary
        assert_eq!(t.position_ticks(), 1, "pause dropped the sub-tick residual");
    }

    #[test]
    fn zero_sample_rate_does_not_move_or_panic() {
        let t = Transport::new(120.0);
        t.play();
        t.advance(512, 0);
        assert_eq!(t.position_ticks(), 0);
    }
}
