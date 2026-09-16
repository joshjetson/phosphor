//! The trim strip's arithmetic: how far one press moves an edge, where a
//! snap puts it, and what a region is not allowed to become.
//!
//! Here rather than in the strip that draws it, for
//! [`knobs`](super::knobs)' reason: the keys and the header have to agree
//! about what "one press of `l` at unit `beat`" means, and a step size
//! computed in two places is two step sizes.
//!
//! # The units are the layer's, not the engine's
//!
//! Every frame count below is in frames *of the recording*. A bar of a
//! 48 kHz loop is 96 000 frames at 120 BPM however fast the device happens
//! to be running, because the trim indexes the file. Reading the engine's
//! rate here would land a bar 8.8% short on a 44.1 kHz device — and the
//! error would only show up on other people's machines.

use phosphor_plugin::sample::SamplePcm;

use super::LayerState;

/// The shortest region a trim may leave, in milliseconds.
///
/// One frame is a legal region in the engine — the pad table clamps to
/// `start < end` and the voice trusts it — and a useless one to a player:
/// two millimetres of waveform that the 2 ms edge fades cancel between
/// them. So the edges stop a millisecond apart, and say so.
pub const MIN_REGION_MS: f32 = 1.0;

/// How far a zero-crossing snap will look, in milliseconds either side.
///
/// Five is about a full cycle of 200 Hz: far enough to find the crossing in
/// anything with pitch in it, short enough that the snap never quietly moves
/// an edge somewhere the player did not put it. Material with no crossing
/// inside the cap — a sustained offset, a fade-out tail — keeps the nudge's
/// own position rather than being dragged to the nearest one anywhere.
pub const SNAP_MS: f32 = 5.0;

/// How far one press of `h`/`l` moves a trim edge.
///
/// The ladder runs from the musical to the microscopic and `j`/`k` walk it:
/// a player squaring up a two-bar loop works in bars, and a player chasing
/// the click off the front of a kick works in single samples. There is no
/// in-between unit worth a rung — between 10 ms and 1 ms is where a hand
/// stops hearing the difference and starts looking at the waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NudgeUnit {
    Bar,
    Beat,
    Sixteenth,
    TenMs,
    OneMs,
    Sample,
}

/// Beats to a bar. Four, because the transport has no time signature to ask
/// — when it grows one, this is the line that reads it.
const BEATS_PER_BAR: f64 = 4.0;

impl Default for NudgeUnit {
    /// Ten milliseconds: the middle of the ladder and the rung a strip is
    /// usually opened for. A bar would slam a drum hit's start into the far
    /// edge on the first press, and a single sample would need a thousand
    /// presses to cross the same ground.
    fn default() -> Self {
        Self::TenMs
    }
}

impl NudgeUnit {
    /// The ladder, shallowest first — the order `k` walks back up.
    pub const ALL: [NudgeUnit; 6] = [
        Self::Bar,
        Self::Beat,
        Self::Sixteenth,
        Self::TenMs,
        Self::OneMs,
        Self::Sample,
    ];

    /// The word on the header.
    pub fn label(self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::Beat => "beat",
            Self::Sixteenth => "1/16",
            Self::TenMs => "10ms",
            Self::OneMs => "1ms",
            Self::Sample => "1smp",
        }
    }

    /// One rung along the ladder: positive goes deeper, toward the single
    /// sample. Both ends are walls — walking off the bottom and reappearing
    /// at bars would turn one press too many into a trim nobody meant.
    pub fn stepped(self, delta: i32) -> Self {
        let here = Self::ALL.iter().position(|u| *u == self).unwrap_or(0) as i32;
        let there = (here + delta).clamp(0, Self::ALL.len() as i32 - 1) as usize;
        Self::ALL[there]
    }

    /// How many frames of the layer's own recording one press moves.
    /// Never zero: a unit that rounds to nothing is a key that does nothing.
    pub fn frames(self, bpm: f64, sample_rate: f32) -> u64 {
        let sr = f64::from(sample_rate.max(1.0));
        let beat = 60.0 / bpm.clamp(1.0, 1_000.0) * sr;
        let frames = match self {
            Self::Bar => beat * BEATS_PER_BAR,
            Self::Beat => beat,
            Self::Sixteenth => beat / 4.0,
            Self::TenMs => sr * 0.010,
            Self::OneMs => sr * 0.001,
            Self::Sample => 1.0,
        };
        (frames.round().max(1.0)) as u64
    }
}

/// Which end of the region a nudge moves. `h`/`l` take the left, `H`/`L`
/// the right — the loop brace's grammar, on a waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimEdge {
    Start,
    End,
}

impl TrimEdge {
    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
        }
    }
}

/// What a nudge did, beyond moving the edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nudge {
    /// Where the edge ended up.
    pub frame: u64,
    /// The press asked for more than [`MIN_REGION_MS`] allows and the edge
    /// stopped against its neighbour. Worth a sentence: running into the
    /// ends of the file is obvious on the waveform, running into an edge
    /// that is already as close as it may get is not.
    pub floored: bool,
}

/// The shortest region this recording may be trimmed to, in its own frames.
pub fn min_region_frames(sample_rate: f32) -> u64 {
    let frames = f64::from(sample_rate.max(1.0)) * f64::from(MIN_REGION_MS) * 1e-3;
    (frames.round() as u64).max(1)
}

/// The nearest frame either side of `frame` where the waveform changes
/// sign, or `frame` itself when there is none within `cap`.
///
/// Read on the first channel alone. A stereo recording's two channels
/// almost never cross together, and a rule that waited for both would find
/// nothing on most material — which is worse than snapping to the channel
/// the eye is looking at, since the 2 ms edge fade covers what is left
/// either way.
pub fn zero_crossing(pcm: &SamplePcm, frame: u64, cap: u64) -> u64 {
    let frames = pcm.frames();
    if frames < 2 {
        return frame;
    }
    let channels = usize::from(pcm.channels.max(1));
    let negative = |f: u64| pcm.data.get(f as usize * channels).is_some_and(|s| *s < 0.0);
    // A crossing *at* f means the sample before it sat on the other side.
    // Frame 0 has nothing before it, so it is never one.
    let crosses = |f: u64| f > 0 && f < frames && negative(f - 1) != negative(f);

    for step in 0..=cap {
        // Outward from the nudge's own answer, nearest first, and the
        // earlier frame wins a tie so that a snap is deterministic.
        if let Some(down) = frame.checked_sub(step) {
            if crosses(down) {
                return down;
            }
        }
        let up = frame.saturating_add(step);
        if step > 0 && crosses(up) {
            return up;
        }
    }
    frame
}

impl LayerState {
    /// The playable region, pulled inside the buffer.
    ///
    /// What the engine will actually play, which is what the strip has to
    /// draw and nudge: the pad table applies exactly this clamp on delivery,
    /// so a session hand-edited to `end_frame: 999999` must not be drawn as
    /// if it meant it. `None` while the file behind the layer is missing —
    /// there is no region without audio.
    pub fn region(&self) -> Option<(u64, u64)> {
        let pcm = self.pcm.as_ref()?;
        let frames = pcm.frames();
        if frames == 0 {
            return None;
        }
        let start = self.start_frame.min(frames - 1);
        Some((start, self.end_frame.clamp(start + 1, frames)))
    }

    /// Move one edge of the region by `delta` presses of `unit`.
    ///
    /// `None` when there is no audio behind the layer — a missing file is
    /// refused rather than trimmed, because a region over nothing is a
    /// number that will mean something else when the file comes back.
    pub fn nudge_trim(
        &mut self,
        edge: TrimEdge,
        delta: i32,
        unit: NudgeUnit,
        bpm: f64,
        snap: bool,
    ) -> Option<Nudge> {
        let (start, end) = self.region()?;
        let pcm = self.pcm.as_ref()?;
        let frames = pcm.frames() as i64;
        let floor = min_region_frames(pcm.sample_rate) as i64;
        let step = i64::from(delta) * unit.frames(bpm, pcm.sample_rate) as i64;

        // Each edge's travel is bounded by the file at one end and by the
        // minimum region at the other. Both bounds are pulled inside the
        // file first, so a recording shorter than the minimum region — half
        // a millisecond of audio — collapses to "the whole of it" instead of
        // to an inverted range.
        let (lo, hi, here) = match edge {
            TrimEdge::Start => (0, (end as i64 - floor).max(0), start as i64),
            TrimEdge::End => ((start as i64 + floor).min(frames), frames, end as i64),
        };
        let asked = here + step;
        let floored = match edge {
            TrimEdge::Start => asked > hi,
            TrimEdge::End => asked < lo,
        };
        let landed = asked.clamp(lo, hi);
        // The snap moves the edge the player just placed, never the other
        // one, and never outside the travel the clamp just decided.
        let frame = if snap {
            let cap = (f64::from(pcm.sample_rate) * f64::from(SNAP_MS) * 1e-3) as u64;
            zero_crossing(pcm, landed as u64, cap).clamp(lo as u64, hi as u64)
        } else {
            landed as u64
        };

        match edge {
            TrimEdge::Start => self.start_frame = frame,
            TrimEdge::End => self.end_frame = frame,
        }
        Some(Nudge { frame, floored })
    }

    /// Where an edge sits in seconds of the recording — what the header
    /// reads out, and the one number a player checks a nudge against.
    pub fn edge_seconds(&self, edge: TrimEdge) -> f32 {
        let (Some((start, end)), Some(pcm)) = (self.region(), self.pcm.as_ref()) else {
            return 0.0;
        };
        let frame = match edge {
            TrimEdge::Start => start,
            TrimEdge::End => end,
        };
        frame as f32 / pcm.sample_rate.max(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// A layer over `frames` frames of `rate`-hertz audio, all of it above
    /// zero so that a snap has nothing to find unless a test plants one.
    fn layer(frames: usize, rate: f32) -> LayerState {
        let pcm = Arc::new(SamplePcm {
            data: vec![0.5; frames],
            channels: 1,
            sample_rate: rate,
        });
        LayerState::from_wav(PathBuf::from("a.wav"), pcm)
    }

    #[test]
    fn a_musical_unit_is_measured_in_the_layers_own_frames() {
        // 120 BPM: a beat is half a second, a bar is two.
        assert_eq!(NudgeUnit::Beat.frames(120.0, 48_000.0), 24_000);
        assert_eq!(NudgeUnit::Bar.frames(120.0, 48_000.0), 96_000);
        assert_eq!(NudgeUnit::Sixteenth.frames(120.0, 48_000.0), 6_000);
        // The same tempo against a 44.1 kHz recording is a different number
        // of frames for the same music. Reading the engine's rate instead
        // would land a bar of this loop 8.8% out.
        assert_eq!(NudgeUnit::Beat.frames(120.0, 44_100.0), 22_050);

        // An odd tempo rounds rather than truncating: at 137 BPM a beat of a
        // 48 kHz recording is 21 021.9 frames, and a press that dropped the
        // fraction would drift a frame every press.
        assert_eq!(NudgeUnit::Beat.frames(137.0, 48_000.0), 21_022);
        assert_eq!(NudgeUnit::Beat.frames(90.5, 44_100.0), 29_238);

        // The fixed units ignore tempo entirely.
        for bpm in [20.0, 120.0, 300.0] {
            assert_eq!(NudgeUnit::TenMs.frames(bpm, 48_000.0), 480);
            assert_eq!(NudgeUnit::OneMs.frames(bpm, 48_000.0), 48);
            assert_eq!(NudgeUnit::Sample.frames(bpm, 48_000.0), 1);
        }
    }

    /// No unit is ever a key that does nothing, however absurd the tempo or
    /// the rate — a 1/16 at 1000 BPM of an 8 kHz recording still moves.
    #[test]
    fn no_unit_ever_rounds_to_nothing() {
        for bpm in [0.0, 1.0, 20.0, 999.0, 100_000.0, f64::NAN] {
            for rate in [0.0, 1.0, 8_000.0, 44_100.0, 192_000.0] {
                for unit in NudgeUnit::ALL {
                    assert!(unit.frames(bpm, rate) >= 1, "{unit:?} at {bpm} BPM, {rate} Hz");
                }
            }
        }
    }

    #[test]
    fn the_unit_ladder_runs_from_bars_to_samples_and_stops_at_both_ends() {
        let mut unit = NudgeUnit::default();
        assert_eq!(unit.label(), "10ms");
        // Down to the bottom and no further.
        for _ in 0..20 {
            unit = unit.stepped(1);
        }
        assert_eq!(unit, NudgeUnit::Sample);
        // ...and back up to the top and no further.
        for _ in 0..20 {
            unit = unit.stepped(-1);
        }
        assert_eq!(unit, NudgeUnit::Bar);
        // One rung at a time, in the order the doc promises.
        let walked: Vec<&str> = (0..6)
            .scan(NudgeUnit::Bar, |u, i| {
                if i > 0 {
                    *u = u.stepped(1);
                }
                Some(u.label())
            })
            .collect();
        assert_eq!(walked, ["bar", "beat", "1/16", "10ms", "1ms", "1smp"]);
    }

    #[test]
    fn a_nudge_moves_the_edge_it_is_given_and_leaves_the_other_alone() {
        let mut l = layer(44_100, 44_100.0);
        l.nudge_trim(TrimEdge::Start, 3, NudgeUnit::TenMs, 120.0, false).unwrap();
        assert_eq!(l.start_frame, 1_323);
        assert_eq!(l.end_frame, 44_100, "a start nudge moved the end");

        l.nudge_trim(TrimEdge::End, -2, NudgeUnit::TenMs, 120.0, false).unwrap();
        assert_eq!(l.end_frame, 43_218);
        assert_eq!(l.start_frame, 1_323, "an end nudge moved the start");

        // And a press back is a press back: the ladder is not a ratchet.
        l.nudge_trim(TrimEdge::Start, -3, NudgeUnit::TenMs, 120.0, false).unwrap();
        assert_eq!(l.start_frame, 0);
    }

    /// The edges may not cross, may not meet, and may not leave the file —
    /// however long a key is held down.
    #[test]
    fn the_edges_never_cross_and_never_leave_the_file() {
        let mut l = layer(44_100, 44_100.0);
        let floor = min_region_frames(44_100.0);
        for _ in 0..500 {
            l.nudge_trim(TrimEdge::Start, 1, NudgeUnit::Bar, 120.0, false).unwrap();
            l.nudge_trim(TrimEdge::End, -1, NudgeUnit::Bar, 120.0, false).unwrap();
        }
        assert!(l.start_frame < l.end_frame, "{}..{}", l.start_frame, l.end_frame);
        assert!(l.end_frame - l.start_frame >= floor, "the region went under a millisecond");
        assert!(l.end_frame <= 44_100);

        // The other direction: out to both walls.
        for _ in 0..500 {
            l.nudge_trim(TrimEdge::Start, -1, NudgeUnit::Bar, 120.0, false).unwrap();
            l.nudge_trim(TrimEdge::End, 1, NudgeUnit::Bar, 120.0, false).unwrap();
        }
        assert_eq!((l.start_frame, l.end_frame), (0, 44_100));
    }

    /// Hitting the minimum region says so, and hitting the end of the file
    /// does not — one is invisible on the waveform and the other is not.
    #[test]
    fn only_the_minimum_region_is_worth_a_word() {
        let mut l = layer(44_100, 44_100.0);
        let far = l.nudge_trim(TrimEdge::Start, 100, NudgeUnit::Bar, 120.0, false).unwrap();
        assert!(far.floored, "the start ran into the end without saying so");
        assert_eq!(l.end_frame - l.start_frame, min_region_frames(44_100.0));

        let mut l = layer(44_100, 44_100.0);
        let wall = l.nudge_trim(TrimEdge::Start, -10, NudgeUnit::Bar, 120.0, false).unwrap();
        assert!(!wall.floored, "the front of the file was reported as a floor");
        assert_eq!(wall.frame, 0);
    }

    /// A recording shorter than the minimum region is a degenerate case, not
    /// a panic: the edges collapse onto the whole of it.
    #[test]
    fn a_recording_shorter_than_the_minimum_region_does_not_invert() {
        let mut l = layer(20, 44_100.0); // half a millisecond
        for delta in [-5, 5] {
            for edge in [TrimEdge::Start, TrimEdge::End] {
                l.nudge_trim(edge, delta, NudgeUnit::OneMs, 120.0, true).unwrap();
            }
        }
        assert!(l.start_frame < l.end_frame);
        assert!(l.end_frame <= 20);
    }

    /// The snap finds the nearest crossing, and stays inside its cap.
    #[test]
    fn a_snap_takes_the_nearest_crossing_inside_the_cap() {
        // One press of 10 ms at 44.1 kHz lands on frame 441, and the cap is
        // 5 ms — 220 frames either side. Plant a crossing 50 frames above it
        // and another 341 below: the far one is outside the cap and must not
        // win however much nearer to zero it is.
        let mut data = vec![0.5f32; 44_100];
        for s in data.iter_mut().take(100) {
            *s = -0.5;
        }
        for s in data.iter_mut().skip(491) {
            *s = -0.5;
        }
        let pcm = Arc::new(SamplePcm { data, channels: 1, sample_rate: 44_100.0 });
        let mut l = LayerState::from_wav(PathBuf::from("a.wav"), pcm);
        let nudge = l.nudge_trim(TrimEdge::Start, 1, NudgeUnit::TenMs, 120.0, true).unwrap();
        assert_eq!(nudge.frame, 491, "the snap missed the nearby crossing");

        // Without the snap the edge stays exactly where the press put it.
        let mut l = layer(44_100, 44_100.0);
        let plain = l.nudge_trim(TrimEdge::Start, 1, NudgeUnit::TenMs, 120.0, false).unwrap();
        assert_eq!(plain.frame, 441);
    }

    /// Material with no crossing inside the cap keeps the nudge's own
    /// position rather than being dragged somewhere it can find one.
    #[test]
    fn a_snap_with_nothing_to_find_leaves_the_edge_alone() {
        // All positive except one crossing 10 000 frames away — miles
        // outside the 220-frame cap.
        let mut data = vec![0.5f32; 44_100];
        for s in data.iter_mut().skip(10_000) {
            *s = -0.5;
        }
        let pcm = Arc::new(SamplePcm { data, channels: 1, sample_rate: 44_100.0 });
        let mut l = LayerState::from_wav(PathBuf::from("a.wav"), pcm);
        let nudge = l.nudge_trim(TrimEdge::Start, 1, NudgeUnit::TenMs, 120.0, true).unwrap();
        assert_eq!(nudge.frame, 441, "the snap dragged the edge out of its cap");
    }

    /// The cap is the layer's own rate: a 96 kHz recording gets twice as
    /// many frames of search for the same five milliseconds of sound.
    #[test]
    fn the_snap_cap_is_five_milliseconds_of_the_layers_rate() {
        for rate in [22_050.0f32, 44_100.0, 96_000.0] {
            let cap = (rate * SNAP_MS * 1e-3) as u64;
            let start = 8_000usize;
            let mut data = vec![0.5f32; 44_100];
            // A crossing exactly one frame past the cap, and nothing else.
            for s in data.iter_mut().skip(start + cap as usize + 1) {
                *s = -0.5;
            }
            let pcm = Arc::new(SamplePcm { data, channels: 1, sample_rate: rate });
            let found = zero_crossing(&pcm, start as u64, cap);
            assert_eq!(found, start as u64, "at {rate} Hz the cap leaked by a frame");
            // ...and one frame inside it is found.
            let found = zero_crossing(&pcm, start as u64, cap + 1);
            assert_eq!(found, start as u64 + cap + 1);
        }
    }

    #[test]
    fn a_layer_with_no_audio_refuses_every_trim() {
        let mut l = layer(44_100, 44_100.0);
        l.pcm = None;
        for edge in [TrimEdge::Start, TrimEdge::End] {
            assert!(l.nudge_trim(edge, 1, NudgeUnit::OneMs, 120.0, true).is_none());
        }
        assert!(l.region().is_none());
        assert_eq!(l.edge_seconds(TrimEdge::Start), 0.0);
    }

    /// A session hand-edited to nonsense is drawn and nudged as what the
    /// engine will actually play, not as what the file claims.
    #[test]
    fn a_region_past_the_buffer_is_pulled_back_in_before_anything_reads_it() {
        let mut l = layer(1_000, 44_100.0);
        l.start_frame = 5_000;
        l.end_frame = 9_000;
        assert_eq!(l.region(), Some((999, 1_000)));
        assert_eq!(l.edge_seconds(TrimEdge::End), 1_000.0 / 44_100.0);

        // And a backwards region, which the engine repairs the same way.
        l.start_frame = 800;
        l.end_frame = 20;
        assert_eq!(l.region(), Some((800, 801)));
    }

    /// The header's numbers are the layer's own seconds, whatever rate it
    /// was recorded at.
    #[test]
    fn the_edge_readouts_are_seconds_of_the_recording() {
        let mut l = layer(96_000, 48_000.0);
        l.start_frame = 24_000;
        l.end_frame = 72_000;
        assert!((l.edge_seconds(TrimEdge::Start) - 0.5).abs() < 1e-6);
        assert!((l.edge_seconds(TrimEdge::End) - 1.5).abs() < 1e-6);
    }
}
