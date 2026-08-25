//! Bounce: a pattern, or a whole chain, written out as a clip.
//!
//! # Why it is one function call
//!
//! The bounce does not re-implement the sequencer. It calls
//! [`phosphor_core::pattern::compile_cycle`], which is the generator the audio
//! thread runs, with a `Vec` for a sink instead of a track's event queue.
//! Swing, gates, ties, accents, chords and lane mutes are therefore not "the
//! same as" live playback — they *are* live playback, and there is no second
//! copy of the arithmetic to drift.
//!
//! # Where it lands
//!
//! At the next free bar at or after the playhead. Two clips overlapping on
//! one track is a position the rest of the application has no meaning for, so
//! the bounce looks for a gap rather than making one; the caller is told which
//! bar it chose so the status line can say so.
//!
//! # And it stops the pattern
//!
//! A bounced clip and the pattern that produced it play the same notes at the
//! same ticks, so leaving both running is a doubled part — every note a flam
//! against itself, which sounds wrong in a way that is hard to attribute.
//! [`Bounce::stops_playback`] says so, and the caller acts on it.

use phosphor_core::clip::{ClipEvent, MidiClip, NoteSnapshot};
use phosphor_core::pattern::compile_cycle;
use phosphor_core::transport::Transport;

use super::SequencerState;
use crate::state::Clip;

/// One bar in ticks, 4/4 — the grid a bounce lands on.
pub const TICKS_PER_BAR: i64 = Transport::PPQ * 4;

/// `ticks` rounded up to a whole number of bars.
///
/// Written out rather than `i64::div_ceil`, which is still unstable at this
/// project's minimum supported Rust version.
fn bars_covering(ticks: i64) -> i64 {
    (ticks + TICKS_PER_BAR - 1).div_euclid(TICKS_PER_BAR)
}

/// The first bar line at or after `tick`.
fn bar_at_or_after(tick: i64) -> i64 {
    bars_covering(tick.max(0)) * TICKS_PER_BAR
}

/// What a bounce produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Bounce {
    /// Where on the timeline it goes.
    pub start_tick: i64,
    /// How long it is.
    pub length_ticks: i64,
    /// The notes, in tick order, relative to `start_tick`.
    pub events: Vec<ClipEvent>,
    /// Whether the sequencer on this track was running and now has to stop.
    pub stops_playback: bool,
}

impl Bounce {
    /// The bar number a player would call this, counting from one.
    #[must_use]
    pub fn bar(&self) -> i64 {
        self.start_tick / TICKS_PER_BAR + 1
    }

    /// How many bars long it is, rounded up — a 12-step pattern is not a
    /// whole number of them.
    #[must_use]
    pub fn bars(&self) -> i64 {
        bars_covering(self.length_ticks).max(1)
    }

    /// The notes as the piano roll holds them.
    #[must_use]
    pub fn notes(&self) -> Vec<NoteSnapshot> {
        let clip = MidiClip::new(self.start_tick, self.length_ticks, self.events.clone());
        phosphor_core::clip::ClipSnapshot::from_clip(0, 0, &clip).notes
    }
}

/// Compile the pattern under the editor, one time through.
#[must_use]
pub fn bounce_pattern(state: &SequencerState, playhead: i64, clips: &[Clip]) -> Option<Bounce> {
    let block = state.block(state.selected_slot() as usize);
    let mut events = Vec::new();
    compile_cycle(&block, 0, &mut events);
    finish(state, events, block.length_ticks(), playhead, clips)
}

/// Compile the whole chain, repeats expanded, one time through.
///
/// Falls back to the pattern under the editor when there is no chain, so that
/// the command means something on every track.
#[must_use]
pub fn bounce_chain(state: &SequencerState, playhead: i64, clips: &[Clip]) -> Option<Bounce> {
    if !state.is_chained() {
        return bounce_pattern(state, playhead, clips);
    }

    let mut events = Vec::new();
    let mut origin = 0i64;
    for entry in state.chain() {
        let block = state.block(entry.slot as usize);
        // Each repeat is compiled at its own origin rather than the whole
        // entry at once, because that is what the audio thread does with it:
        // a pattern that repeats starts again from step zero.
        for _ in 0..entry.repeats.max(1) {
            compile_cycle(&block, origin, &mut events);
            origin += block.length_ticks();
        }
    }
    events.sort_by_key(|e| e.tick);
    finish(state, events, origin, playhead, clips)
}

fn finish(
    state: &SequencerState,
    events: Vec<phosphor_core::pattern::PatternEvent>,
    length_ticks: i64,
    playhead: i64,
    clips: &[Clip],
) -> Option<Bounce> {
    if events.is_empty() || length_ticks <= 0 {
        return None;
    }
    Some(Bounce {
        start_tick: next_free_bar(clips, playhead, length_ticks),
        length_ticks,
        events: events
            .into_iter()
            .map(|e| ClipEvent { tick: e.tick, status: e.status, data1: e.data1, data2: e.data2 })
            .collect(),
        stops_playback: state.is_playing(),
    })
}

/// The first bar line at or after `playhead` where a clip `length` ticks long
/// fits between the clips already on the track.
///
/// Bar-aligned because a bounce is a bar of music and a player is going to
/// want it lined up with the rest of them; searched rather than assumed
/// because writing a clip on top of another one produces a track state
/// nothing else in the application knows how to draw or play.
#[must_use]
pub fn next_free_bar(clips: &[Clip], playhead: i64, length: i64) -> i64 {
    let length = length.max(1);
    let mut start = bar_at_or_after(playhead);

    // Bounded: each step past an occupied bar moves the candidate to the end
    // of the clip that blocked it, so the search visits each clip once at
    // most, and the `+1` guarantees forward progress even on a clip of no
    // length.
    for _ in 0..=clips.len() {
        let end = start + length;
        let blocker = clips
            .iter()
            .filter(|c| c.start_tick < end && c.start_tick + c.length_ticks.max(1) > start)
            .map(|c| c.start_tick + c.length_ticks.max(1))
            .max();
        match blocker {
            Some(after) => start = bar_at_or_after(after.max(start + 1)),
            None => return start,
        }
    }
    start
}

#[cfg(test)]
mod tests {
    use super::super::ops::{dispatch, SeqOp};
    use super::super::tests::drum_track;
    use super::*;
    use crate::state::TrackState;

    fn clip(start_tick: i64, length_ticks: i64) -> Clip {
        Clip {
            number: 1,
            width: 4,
            has_content: true,
            start_tick,
            length_ticks,
            notes: Vec::new(),
            hidden_notes: Vec::new(),
        }
    }

    fn four_on_the_floor() -> TrackState {
        let mut track = drum_track();
        for step in [0usize, 4, 8, 12] {
            dispatch(&mut track, SeqOp::SelectStep(step as u8));
            dispatch(&mut track, SeqOp::ToggleStep);
        }
        track
    }

    /// One time through, and every hit accounted for at the tick the pattern
    /// would have played it.
    #[test]
    fn a_bounce_is_one_cycle_of_the_pattern() {
        let track = four_on_the_floor();
        let bounce = bounce_pattern(track.sequencer.as_ref().unwrap(), 0, &[]).unwrap();

        assert_eq!(bounce.length_ticks, 3840);
        assert_eq!(bounce.bars(), 1);
        assert_eq!(bounce.bar(), 1);
        let ons: Vec<i64> = bounce
            .events
            .iter()
            .filter(|e| e.status == 0x90 && e.data2 > 0)
            .map(|e| e.tick)
            .collect();
        assert_eq!(ons, vec![0, 960, 1920, 2880]);
        // Every note ends.
        assert_eq!(bounce.events.iter().filter(|e| e.status == 0x80).count(), 4);
        assert_eq!(bounce.notes().len(), 4);
    }

    /// Swing is not applied again by the bounce; it comes out of the same
    /// generator, so the offsets are the pattern's own.
    #[test]
    fn a_bounce_carries_the_patterns_swing() {
        let mut track = drum_track();
        for step in 0..8u8 {
            dispatch(&mut track, SeqOp::SelectStep(step));
            dispatch(&mut track, SeqOp::ToggleStep);
        }
        dispatch(&mut track, SeqOp::NudgeSwing(12)); // 62%

        let bounce = bounce_pattern(track.sequencer.as_ref().unwrap(), 0, &[]).unwrap();
        let ons: Vec<i64> = bounce
            .events
            .iter()
            .filter(|e| e.status == 0x90 && e.data2 > 0)
            .map(|e| e.tick)
            .collect();
        // Odd steps 57 ticks late: (62 - 50) * 2 * 240 / 100.
        assert_eq!(ons, vec![0, 297, 480, 777, 960, 1257, 1440, 1737]);
    }

    /// A chain bounces as it plays: entries in order, repeats expanded, each
    /// time through starting from step zero.
    #[test]
    fn a_chain_bounces_with_its_repeats_expanded() {
        let mut track = four_on_the_floor();
        dispatch(&mut track, SeqOp::SelectSlot(1));
        dispatch(&mut track, SeqOp::SelectStep(2));
        dispatch(&mut track, SeqOp::ToggleStep);
        dispatch(&mut track, SeqOp::PushChainEntry { slot: 0, repeats: 2 });
        dispatch(&mut track, SeqOp::PushChainEntry { slot: 1, repeats: 1 });

        let state = track.sequencer.as_ref().unwrap();
        let bounce = bounce_chain(state, 0, &[]).unwrap();
        assert_eq!(bounce.length_ticks, 3840 * 3);
        assert_eq!(bounce.bars(), 3);

        let ons: Vec<i64> = bounce
            .events
            .iter()
            .filter(|e| e.status == 0x90 && e.data2 > 0)
            .map(|e| e.tick)
            .collect();
        assert_eq!(
            ons,
            vec![0, 960, 1920, 2880, 3840, 4800, 5760, 6720, 7680 + 480],
            "two times through A, then one of B"
        );
    }

    /// With no chain, the chain bounce is the pattern bounce — the command
    /// has to mean something on every track.
    #[test]
    fn bouncing_a_chain_that_is_not_there_bounces_the_pattern() {
        let track = four_on_the_floor();
        let state = track.sequencer.as_ref().unwrap();
        assert_eq!(bounce_chain(state, 0, &[]), bounce_pattern(state, 0, &[]));
    }

    /// An empty pattern produces nothing rather than an empty clip nobody
    /// asked for.
    #[test]
    fn an_empty_pattern_bounces_to_nothing() {
        let track = drum_track();
        assert!(bounce_pattern(track.sequencer.as_ref().unwrap(), 0, &[]).is_none());
    }

    /// A running sequencer has to stop, because the clip and the pattern
    /// would otherwise play the same notes at the same ticks.
    #[test]
    fn a_bounce_says_when_it_has_to_stop_the_pattern() {
        // A fresh sequencer runs by default, so bouncing it stops playback.
        let mut track = four_on_the_floor();
        let state = track.sequencer.as_ref().unwrap();
        assert!(bounce_pattern(state, 0, &[]).unwrap().stops_playback);

        dispatch(&mut track, SeqOp::SetPlaying(false));
        let state = track.sequencer.as_ref().unwrap();
        assert!(!bounce_pattern(state, 0, &[]).unwrap().stops_playback);
    }

    // ── Placement ──

    #[test]
    fn a_bounce_lands_on_a_bar_line_at_or_after_the_playhead() {
        assert_eq!(next_free_bar(&[], 0, 3840), 0);
        assert_eq!(next_free_bar(&[], 1, 3840), 3840);
        assert_eq!(next_free_bar(&[], 3840, 3840), 3840);
        assert_eq!(next_free_bar(&[], 3841, 3840), 7680);
        assert_eq!(next_free_bar(&[], -500, 3840), 0);
    }

    /// Never on top of a clip that is already there: two overlapping clips on
    /// one track is a position the rest of the application has no meaning
    /// for.
    #[test]
    fn a_bounce_never_lands_on_a_clip_that_is_already_there() {
        let occupied = [clip(0, 3840), clip(3840, 3840)];
        assert_eq!(next_free_bar(&occupied, 0, 3840), 7680);

        // A gap that is big enough gets used.
        let gap = [clip(0, 3840), clip(7680, 3840)];
        assert_eq!(next_free_bar(&gap, 0, 3840), 3840);

        // A gap that is not big enough does not.
        assert_eq!(next_free_bar(&gap, 0, 3840 * 2), 11_520);
    }

    /// The search terminates whatever it is given, including clips of no
    /// length and clips out of order.
    #[test]
    fn the_search_for_a_free_bar_terminates() {
        let awkward = [clip(7680, 0), clip(0, 1), clip(3840, 100_000), clip(0, 3840)];
        let found = next_free_bar(&awkward, 0, 3840);
        assert_eq!(found % TICKS_PER_BAR, 0);
        assert!(found >= 103_840);
    }
}
