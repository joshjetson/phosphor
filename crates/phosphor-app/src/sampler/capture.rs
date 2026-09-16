//! Source mode: the track plays a synth so that a pad can be recorded
//! from it, and the capture that turns a performance into a plan to
//! render.
//!
//! Nothing here makes a sound or touches the engine. What it does is
//! remember what was played and *when*, and turn those arrival stamps into
//! sample offsets — which is the whole of the timing story, and the half
//! of it that can be tested without a device.
//!
//! # Two shapes of take
//!
//! Stopped, a take is free: time zero is the first note-on and the take
//! ends when the player disarms, with the instrument's tail rendered out
//! to silence after it. That is what a one-shot wants — a hit begins when
//! you hit it.
//!
//! Rolling, a take is bars. The window opens at the first bar line at or
//! after arming and closes at the end of the bar the disarm lands in, so a
//! take is always a whole number of bars and therefore loops. This is the
//! Elektron lesson and it is worth stating plainly: a loop that is a
//! musically exact length is useful forever, and one that is 1.87 bars
//! long is a file nobody can use twice. Anything still ringing past the
//! close is cut, and the cut is what makes the seam meet.
//!
//! A key already held when a bar window opens is a key that was sounding
//! at the top of the bar, so it goes into the take at offset zero. A key
//! still held at the close is lifted there.

use phosphor_core::transport::Transport;

use crate::state::InstrumentType;

/// Longest free take, in seconds. A held note with nobody in the room
/// cannot turn into a ten-minute file: the window closes here and says so.
pub const MAX_FREE_SECONDS: f64 = 60.0;

/// Longest bar-quantised take. Sixty-four bars is four times the longest
/// loop anybody sequences into a pad.
pub const MAX_BARS: u32 = 64;

/// Ticks in one bar of 4/4 — the grid a rolling take is quantised to.
const BAR_TICKS: i64 = Transport::PPQ * 4;

/// One note event as the UI's MIDI tap saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturedEvent {
    /// Microseconds on the input clock, as the MIDI callback stamped it.
    pub micros: u64,
    pub note: u8,
    pub velocity: u8,
    /// A key going down; `false` is the lift.
    pub on: bool,
}

/// One event in the plan, at its offset in frames from the take's start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderEvent {
    pub frame: u64,
    /// A channel-1 status byte: `0x90` or `0x80`.
    pub status: u8,
    pub data1: u8,
    pub data2: u8,
}

/// What the renderer does with what is still ringing when the performance
/// ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tail {
    /// Keep rendering until the instrument falls silent, up to this many
    /// frames. A free take's release is part of the sound.
    ToSilence { max_frames: u64 },
    /// Stop dead at the end of the window. A bar take's seam is the point:
    /// the ring past the loop point belongs to the next pass, not to the
    /// end of this one.
    Cut,
}

/// Everything the renderer needs about time, decided by the capture.
#[derive(Debug, Clone, PartialEq)]
pub struct TakePlan {
    /// The performance, sorted, at frame offsets from the take's start.
    pub events: Vec<RenderEvent>,
    /// Frames the performance itself covers. The rendered buffer is this
    /// plus whatever [`Tail`] allows.
    pub frames: u64,
    pub tail: Tail,
    /// The transport tick the window opened on, when the transport was
    /// rolling — an arpeggiator locks its grid to it. `None` for a free
    /// take, where there is no grid to lock to.
    pub start_tick: Option<i64>,
    pub tempo_bpm: f64,
    pub sample_rate: f32,
    /// True when the window was closed by its own cap rather than by the
    /// player, so the flash can say so.
    pub capped: bool,
}

/// How a window's start and end are decided, fixed when the arm is taken.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    /// Transport stopped: zero is the first note-on.
    Free,
    /// Transport rolling: whole bars from `opens`.
    Bars {
        /// Input-clock micros of the bar line the window opens on.
        opens: u64,
        /// One bar in microseconds at the tempo the arm was taken at.
        bar_micros: f64,
        /// The transport tick that bar line sits on.
        start_tick: i64,
    },
}

/// A capture in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct TakeCapture {
    shape: Shape,
    tempo_bpm: f64,
    sample_rate: f32,
    events: Vec<CapturedEvent>,
}

impl TakeCapture {
    /// Arm with the transport stopped.
    #[must_use]
    pub fn free(tempo_bpm: f64, sample_rate: f32) -> Self {
        Self { shape: Shape::Free, tempo_bpm, sample_rate, events: Vec::new() }
    }

    /// Arm with the transport rolling. `now` and `position_ticks` are read
    /// in the same breath, so the bar line can be named in both clocks.
    #[must_use]
    pub fn bars(now: u64, position_ticks: i64, tempo_bpm: f64, sample_rate: f32) -> Self {
        let bpm = tempo_bpm.max(1.0);
        let micros_per_tick = 60_000_000.0 / (bpm * Transport::PPQ as f64);
        // The first bar line at *or after* the arm: arming exactly on the
        // downbeat starts there rather than waiting a whole bar.
        let into_bar = position_ticks.rem_euclid(BAR_TICKS);
        let to_go = if into_bar == 0 { 0 } else { BAR_TICKS - into_bar };
        Self {
            shape: Shape::Bars {
                opens: now + (to_go as f64 * micros_per_tick) as u64,
                bar_micros: BAR_TICKS as f64 * micros_per_tick,
                start_tick: position_ticks + to_go,
            },
            tempo_bpm: bpm,
            sample_rate,
            events: Vec::new(),
        }
    }

    /// Whether the window has opened yet — a bar-quantised arm waits for
    /// the downbeat, and the banner says so while it does.
    #[must_use]
    pub fn open_at(&self, now: u64) -> bool {
        match self.shape {
            Shape::Free => true,
            Shape::Bars { opens, .. } => now >= opens,
        }
    }

    /// Whether this take will be cut to bars.
    #[must_use]
    pub fn is_bars(&self) -> bool {
        matches!(self.shape, Shape::Bars { .. })
    }

    /// Note-ons the player has landed inside the window — what the banner
    /// counts, and what tells an empty take from a played one.
    #[must_use]
    pub fn note_count(&self) -> usize {
        let from = match self.shape {
            Shape::Free => 0,
            Shape::Bars { opens, .. } => opens,
        };
        self.events
            .iter()
            .filter(|e| e.on && e.velocity > 0 && e.micros >= from)
            .count()
    }

    /// The performance as it was played, stamps and all.
    #[must_use]
    pub fn played(&self) -> &[CapturedEvent] {
        &self.events
    }

    /// One note event from the tap.
    ///
    /// Everything is kept, including the notes that arrive before a bar
    /// window opens: a key held across the downbeat was sounding at the
    /// top of the bar, and [`TakeCapture::close`] is where that is turned
    /// into a note-on at offset zero.
    pub fn note(&mut self, event: CapturedEvent) {
        // A velocity-zero note-on is a note-off; every controller that runs
        // notes together sends them that way.
        let on = event.on && event.velocity > 0;
        self.events.push(CapturedEvent { on, ..event });
    }

    /// Close the window at `now` and say what to render.
    ///
    /// `None` when nothing was played inside it: an armed take with no
    /// notes in it lands nothing at all, rather than a layer of silence
    /// with a name.
    #[must_use]
    pub fn close(&self, now: u64) -> Option<TakePlan> {
        let (zero, window, capped) = self.bounds(now)?;
        let frames_per_micro = f64::from(self.sample_rate) / 1_000_000.0;
        let window_frames = (window as f64 * frames_per_micro).round() as u64;

        let mut events: Vec<RenderEvent> = Vec::with_capacity(self.events.len() + 8);
        // Keys already down when the window opened were sounding at its
        // first sample, so the take starts with them.
        let held = self.held_at(zero);
        for &(note, velocity) in &held {
            events.push(RenderEvent { frame: 0, status: 0x90, data1: note, data2: velocity });
        }
        let mut open: Vec<u8> = held.iter().map(|h| h.0).collect();
        for event in self.events.iter().filter(|e| e.micros >= zero) {
            let frame = ((event.micros - zero) as f64 * frames_per_micro).round() as u64;
            if frame >= window_frames {
                continue;
            }
            if event.on {
                if !open.contains(&event.note) {
                    open.push(event.note);
                }
                events.push(RenderEvent {
                    frame,
                    status: 0x90,
                    data1: event.note,
                    data2: event.velocity,
                });
            } else {
                open.retain(|&n| n != event.note);
                events.push(RenderEvent { frame, status: 0x80, data1: event.note, data2: 0 });
            }
        }
        if events.iter().all(|e| e.status == 0x80) {
            return None; // nothing was played in the window
        }
        // Whatever is still down at the close is lifted there — a note with
        // no off is a voice that never releases.
        let last = window_frames.saturating_sub(1);
        for note in open {
            events.push(RenderEvent { frame: last, status: 0x80, data1: note, data2: 0 });
        }
        events.sort_by_key(|e| (e.frame, e.status));

        let tail = match self.shape {
            // Two seconds is a long reverb tail and a short pad release;
            // past it the take is being padded rather than finished.
            Shape::Free => Tail::ToSilence {
                max_frames: (2.0 * f64::from(self.sample_rate)) as u64,
            },
            Shape::Bars { .. } => Tail::Cut,
        };
        Some(TakePlan {
            events,
            frames: window_frames.max(1),
            tail,
            start_tick: match self.shape {
                Shape::Free => None,
                Shape::Bars { start_tick, .. } => Some(start_tick),
            },
            tempo_bpm: self.tempo_bpm,
            sample_rate: self.sample_rate,
            capped,
        })
    }

    /// Where the window starts, how long it is, and whether the cap closed
    /// it. `None` when there is no window at all.
    fn bounds(&self, now: u64) -> Option<(u64, u64, bool)> {
        match self.shape {
            Shape::Free => {
                let first = self.events.iter().find(|e| e.on)?.micros;
                let last_off = self.events.iter().filter(|e| !e.on).map(|e| e.micros).max();
                // The take ends at the disarm, or at the last lift if the
                // player let go and then reached for the key that ends it.
                let end = now.max(last_off.unwrap_or(first)).max(first + 1);
                let cap = (MAX_FREE_SECONDS * 1_000_000.0) as u64;
                let length = end - first;
                Some((first, length.min(cap), length > cap))
            }
            Shape::Bars { opens, bar_micros, .. } => {
                if now <= opens {
                    return None; // disarmed before the downbeat
                }
                let played = (now - opens) as f64;
                let bars = (played / bar_micros).ceil().max(1.0) as u32;
                let capped = bars > MAX_BARS;
                let bars = bars.min(MAX_BARS);
                Some((opens, (f64::from(bars) * bar_micros) as u64, capped))
            }
        }
    }

    /// The keys that were down at `at`, with the velocity they went down
    /// at, in the order they were pressed.
    fn held_at(&self, at: u64) -> Vec<(u8, u8)> {
        let mut held: Vec<(u8, u8)> = Vec::new();
        for event in self.events.iter().filter(|e| e.micros < at) {
            if event.on {
                if !held.iter().any(|h| h.0 == event.note) {
                    held.push((event.note, event.velocity));
                }
            } else {
                held.retain(|h| h.0 != event.note);
            }
        }
        held
    }
}

/// The track is playing an instrument so that a pad can be recorded from
/// it.
///
/// A mode, not state of record: what the *pad* remembers about its source
/// lives on the pad ([`super::PadSource`]) and goes into the session. This
/// is the copy the mode is running on, so that an undo of the pad's memory
/// cannot change the instrument under the player's hands mid-performance.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceMode {
    /// The track whose plugin slot is on loan.
    pub track_idx: usize,
    /// The pad the take will land on. Fixed for the life of the mode: the
    /// remembered instrument and its panel belong to *this* pad.
    pub pad: usize,
    pub instrument: InstrumentType,
    pub params: Vec<f32>,
    /// The capture, while one is armed.
    pub capture: Option<TakeCapture>,
}

impl SourceMode {
    #[must_use]
    pub fn new(
        track_idx: usize,
        pad: usize,
        instrument: InstrumentType,
        params: Vec<f32>,
    ) -> Self {
        Self { track_idx, pad, instrument, params, capture: None }
    }

    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.capture.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn on(micros: u64, note: u8) -> CapturedEvent {
        CapturedEvent { micros, note, velocity: 100, on: true }
    }

    fn off(micros: u64, note: u8) -> CapturedEvent {
        CapturedEvent { micros, note, velocity: 0, on: false }
    }

    #[test]
    fn a_free_take_starts_at_the_first_note_not_at_the_arm() {
        let mut capture = TakeCapture::free(120.0, SR);
        // Armed at zero, played half a second later.
        capture.note(on(500_000, 60));
        capture.note(off(1_000_000, 60));
        let plan = capture.close(1_200_000).expect("a played take");
        assert_eq!(plan.events[0].frame, 0, "the take did not start at the first note");
        assert_eq!(plan.events[0].status, 0x90);
        // The lift is half a second in, in frames of the engine's rate.
        assert_eq!(plan.events[1].frame, 24_000);
        // And the window runs to the disarm, not to the last lift.
        assert_eq!(plan.frames, 33_600);
        assert!(matches!(plan.tail, Tail::ToSilence { .. }));
        assert_eq!(plan.start_tick, None);
    }

    #[test]
    fn an_armed_take_with_no_notes_in_it_plans_nothing() {
        let capture = TakeCapture::free(120.0, SR);
        assert!(capture.close(5_000_000).is_none());
        // ...and neither does one that only ever saw lifts.
        let mut lifts = TakeCapture::free(120.0, SR);
        lifts.note(off(100, 60));
        assert!(lifts.close(1_000_000).is_none());
    }

    #[test]
    fn a_held_key_is_lifted_at_the_close() {
        let mut capture = TakeCapture::free(120.0, SR);
        capture.note(on(0, 48));
        let plan = capture.close(1_000_000).unwrap();
        let last = plan.events.last().unwrap();
        assert_eq!(last.status, 0x80);
        assert_eq!(last.frame, plan.frames - 1, "the held key was never lifted");
    }

    /// The Elektron rule: armed mid-bar at 120 BPM, the window opens on the
    /// next bar line and closes at the end of the bar the disarm lands in.
    #[test]
    fn a_rolling_take_is_whole_bars_from_the_next_bar_line() {
        // Half a bar in: 2 beats of 960 ticks. At 120 BPM a bar is 2 s.
        let pos = Transport::PPQ * 2;
        let mut capture = TakeCapture::bars(0, pos, 120.0, SR);
        assert!(!capture.open_at(500_000), "the window opened before the bar line");
        assert!(capture.open_at(1_000_000), "the window never opened");
        // The downbeat is one second away. Play a note on it and one a bar
        // and a half in, then disarm a bar and a half in.
        capture.note(on(1_000_000, 60));
        capture.note(off(1_500_000, 60));
        let plan = capture.close(1_000_000 + 3_000_000).unwrap();
        assert_eq!(plan.tail, Tail::Cut);
        assert_eq!(plan.start_tick, Some(Transport::PPQ * 4));
        // Two bars of 48 kHz at 120 BPM: 4 seconds.
        assert_eq!(plan.frames, 192_000);
        assert_eq!(plan.events[0].frame, 0, "the downbeat note did not land at zero");
    }

    #[test]
    fn a_key_held_across_the_downbeat_is_in_the_take() {
        let mut capture = TakeCapture::bars(0, Transport::PPQ * 2, 120.0, SR);
        capture.note(on(200_000, 55)); // pressed before the window opened
        capture.note(off(1_500_000, 55));
        let plan = capture.close(2_500_000).unwrap();
        assert_eq!(plan.events[0], RenderEvent {
            frame: 0,
            status: 0x90,
            data1: 55,
            data2: 100,
        });
        assert_eq!(plan.events[1].status, 0x80, "the lift went missing");
        assert_eq!(plan.events[1].frame, 24_000);
    }

    #[test]
    fn disarming_before_the_downbeat_plans_nothing() {
        let mut capture = TakeCapture::bars(0, Transport::PPQ * 2, 120.0, SR);
        capture.note(on(200_000, 55));
        assert!(capture.close(900_000).is_none());
    }

    #[test]
    fn arming_on_the_downbeat_opens_there_rather_than_a_bar_later() {
        let capture = TakeCapture::bars(7_000, Transport::PPQ * 8, 120.0, SR);
        assert!(capture.open_at(7_000), "an arm on the bar line waited a whole bar");
    }

    /// A key left down cannot make a ten-minute file: a free take stops at
    /// its cap, and says it did.
    #[test]
    fn a_runaway_take_stops_at_its_cap() {
        let mut capture = TakeCapture::free(120.0, SR);
        capture.note(on(0, 60));
        let plan = capture.close(600_000_000).unwrap(); // ten minutes
        assert!(plan.capped);
        assert_eq!(plan.frames, (MAX_FREE_SECONDS * f64::from(SR)) as u64);

        // And a rolling one stops at sixty-four bars.
        let mut bars = TakeCapture::bars(0, 0, 120.0, SR);
        bars.note(on(0, 60));
        let plan = bars.close(600_000_000).unwrap();
        assert!(plan.capped);
        assert_eq!(plan.frames, (f64::from(MAX_BARS) * 2.0 * f64::from(SR)) as u64);
    }

    /// Notes played past a bar window's close are not in the take: the
    /// window is a number of bars, not "everything I played".
    #[test]
    fn notes_past_the_close_are_not_in_the_take() {
        let mut capture = TakeCapture::bars(0, 0, 120.0, SR);
        capture.note(on(100_000, 60));
        capture.note(off(200_000, 60));
        // Disarm at 2.1 s: two bars. A note at 4.5 s could only arrive from
        // a tap that kept feeding after the close, which is a bug this
        // guards rather than a gesture.
        capture.note(on(4_500_000, 72));
        let plan = capture.close(2_100_000).unwrap();
        assert!(plan.events.iter().all(|e| e.data1 == 60), "a late note got in");
        assert_eq!(plan.frames, 192_000);
    }

    #[test]
    fn the_note_count_ignores_lifts_and_anything_before_the_window() {
        let mut capture = TakeCapture::bars(0, Transport::PPQ * 2, 120.0, SR);
        capture.note(on(100_000, 60)); // before the downbeat
        capture.note(off(200_000, 60));
        assert_eq!(capture.note_count(), 0);
        capture.note(on(1_100_000, 62));
        capture.note(off(1_200_000, 62));
        assert_eq!(capture.note_count(), 1);
    }
}
