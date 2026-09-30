//! Cuts that ignore the audio: N equal slices, or a loop's beat grid.
//!
//! The grid is the equal cut with the count worked out for the player. A
//! loop that is `bars` bars long, cut every `division`, is `bars × per bar`
//! equal slices — so the one piece of arithmetic here is the equal cut, and
//! a grid is never a second copy of it. Bars are four beats, as they are
//! everywhere else in the app.

use phosphor_plugin::sample::SamplePcm;

use super::markers::Marker;

/// Where a grid cuts, as slices per four-beat bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Division {
    Bar,
    Half,
    #[default]
    Beat,
    Eighth,
    Sixteenth,
}

impl Division {
    pub fn per_bar(self) -> u32 {
        match self {
            Division::Bar => 1,
            Division::Half => 2,
            Division::Beat => 4,
            Division::Eighth => 8,
            Division::Sixteenth => 16,
        }
    }
}

/// `count` equal slices of `start..end`, one cut at the start of each.
///
/// Every cut is a zero-crossing snap of the exact division, so a slice can
/// come out a frame or two off equal; that is the price of not clicking.
/// A region with fewer frames than cuts gets one cut per frame at most.
pub fn equal(pcm: &SamplePcm, start: u64, end: u64, count: u32) -> Vec<Marker> {
    let span = end.saturating_sub(start);
    let count = u64::from(count).min(span);
    (0..count)
        .map(|i| {
            // Multiplied before divided: `span / count * i` drifts a frame
            // per slice and a sixteen-slice bar ends sixteen frames short.
            let exact = start + span * i / count;
            // The first cut is the region's own start: snapping it could
            // only move it outside, or later than the player's trim.
            if i == 0 {
                Marker { frame: exact, strength: 0.0, pinned: false }
            } else {
                Marker::at(pcm, exact, 0.0, false)
            }
        })
        .collect()
}

/// A loop `bars` bars long, cut every `division`.
pub fn beats(pcm: &SamplePcm, start: u64, end: u64, bars: u32, division: Division) -> Vec<Marker> {
    equal(pcm, start, end, bars.saturating_mul(division.per_bar()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(frames: usize) -> SamplePcm {
        SamplePcm { data: vec![0.5; frames], channels: 1, sample_rate: 48_000.0 }
    }

    fn frames(m: &[Marker]) -> Vec<u64> {
        m.iter().map(|m| m.frame).collect()
    }

    #[test]
    fn equal_slices_start_at_the_region_and_do_not_drift() {
        let pcm = flat(10_000);
        assert_eq!(frames(&equal(&pcm, 1_000, 2_000, 4)), vec![1_000, 1_250, 1_500, 1_750]);
        // 1000 / 3 does not divide: each cut is the exact one, floored.
        assert_eq!(frames(&equal(&pcm, 0, 1_000, 3)), vec![0, 333, 666]);
        let sixteen = equal(&pcm, 0, 9_999, 16);
        assert_eq!(sixteen.last().unwrap().frame, 9_999 * 15 / 16, "the grid drifted");
    }

    #[test]
    fn a_grid_is_bars_times_the_division() {
        let pcm = flat(10_000);
        assert_eq!(beats(&pcm, 0, 9_600, 2, Division::Beat).len(), 8);
        assert_eq!(beats(&pcm, 0, 9_600, 1, Division::Sixteenth).len(), 16);
        assert_eq!(beats(&pcm, 0, 9_600, 4, Division::Bar).len(), 4);
    }

    #[test]
    fn nothing_to_cut_makes_no_cuts() {
        let pcm = flat(100);
        assert!(equal(&pcm, 0, 100, 0).is_empty());
        assert!(equal(&pcm, 50, 50, 8).is_empty());
        assert_eq!(equal(&pcm, 0, 3, 8).len(), 3, "more cuts than frames");
    }

    #[test]
    fn an_inner_cut_lands_on_a_zero_crossing() {
        // Sign flips every 50 frames; 1 ms at 48 kHz reaches 48.
        let data = (0..1_000).map(|i| if (i / 50) % 2 == 0 { 0.5 } else { -0.5 }).collect();
        let pcm = SamplePcm { data, channels: 1, sample_rate: 48_000.0 };
        let cuts = frames(&equal(&pcm, 0, 1_000, 3));
        assert_eq!(cuts, vec![0, 350, 650]);
    }
}
