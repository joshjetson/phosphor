//! Where things go on the timeline: bars, and the next place a new clip fits.
//!
//! Shared by everything that writes a clip it made itself — a sequencer
//! bounce, a chop's replay clip — and by the loop editor's grid, so there is
//! one bar length and one answer to "where does this go" in the crate.

use phosphor_core::transport::Transport;

use crate::state::Clip;

/// One bar in ticks, 4/4 — the transport has no time signature to ask.
pub const TICKS_PER_BAR: i64 = Transport::PPQ * 4;

/// `ticks` rounded up to a whole number of bars.
///
/// Written out rather than `i64::div_ceil`, which is still unstable at this
/// project's minimum supported Rust version.
pub fn bars_covering(ticks: i64) -> i64 {
    (ticks + TICKS_PER_BAR - 1).div_euclid(TICKS_PER_BAR)
}

/// The first bar line at or after `tick`.
pub fn bar_at_or_after(tick: i64) -> i64 {
    bars_covering(tick.max(0)) * TICKS_PER_BAR
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
    use super::*;

    fn clip(start_tick: i64, length_ticks: i64) -> Clip {
        Clip {
            number: 1,
            width: 4,
            has_content: true,
            start_tick,
            length_ticks,
            notes: Vec::new(),
            hidden_notes: Vec::new(),
            controls: Vec::new(),
        }
    }

    #[test]
    fn a_new_clip_lands_on_a_bar_line_at_or_after_the_playhead() {
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
    fn a_new_clip_never_lands_on_a_clip_that_is_already_there() {
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
