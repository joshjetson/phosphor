//! The cut points, and the player's hand on them.
//!
//! A marker is where a slice starts; a slice runs to the next marker, and
//! the last one to the end of the region. Detection proposes markers, and
//! the player's edits *pin* them: adding one, moving one. Re-running the
//! detection at another sensitivity replaces every marker it proposed and
//! none of the ones the player touched — the thing Ableton gets wrong and
//! players hate, because the hand-work is the part that took the time.

use phosphor_plugin::sample::SamplePcm;

use crate::sampler::trim::zero_crossing;

/// No two cuts closer than this: a flam is one hit, and a slice shorter
/// than a hop is not a sound anyone can play.
pub const MIN_GAP_MS: f32 = 40.0;

/// How far a cut may move to land on a zero crossing. Short, because every
/// transient cut is placed just before its attack and a snap must not carry
/// it past; the engine's 2 ms edge fade covers the rest.
const SNAP_MS: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marker {
    /// The first frame of the slice, in buffer frames.
    pub frame: u64,
    /// How hard the hit it marks is, in dB — what "keep the strongest"
    /// ranks by, and what a replayed note's velocity is read from. Equal
    /// for markers that mark no hit (the grid's, the player's own).
    pub strength: f32,
    /// Placed or moved by the player: survives re-detection.
    pub pinned: bool,
}

impl Marker {
    /// A cut at `frame`, moved onto the nearest zero crossing within reach.
    pub fn at(pcm: &SamplePcm, frame: u64, strength: f32, pinned: bool) -> Self {
        let cap = ms_to_frames(SNAP_MS, pcm.sample_rate);
        Self { frame: zero_crossing(pcm, frame, cap), strength, pinned }
    }
}

pub fn ms_to_frames(ms: f32, rate: f32) -> u64 {
    (ms / 1000.0 * rate.max(1.0)) as u64
}

/// Every cut in one region of one buffer, kept in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Markers {
    start: u64,
    end: u64,
    min_gap: u64,
    list: Vec<Marker>,
}

impl Markers {
    /// No cuts yet, over `start..end` of a buffer at `rate`.
    pub fn new(start: u64, end: u64, rate: f32) -> Self {
        Self { start, end: end.max(start), min_gap: ms_to_frames(MIN_GAP_MS, rate).max(1), list: Vec::new() }
    }

    pub fn list(&self) -> &[Marker] {
        &self.list
    }

    pub fn region(&self) -> (u64, u64) {
        (self.start, self.end)
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Whether `frame` is far enough from every cut except `skip` to be one.
    fn clear_of_others(&self, frame: u64, skip: Option<usize>) -> bool {
        self.list
            .iter()
            .enumerate()
            .all(|(i, m)| Some(i) == skip || m.frame.abs_diff(frame) >= self.min_gap)
    }

    fn inside(&self, frame: u64) -> bool {
        (self.start..self.end).contains(&frame)
    }

    /// Replace every proposed cut with `proposed`, keeping the pinned ones.
    ///
    /// A proposal too close to a pinned cut yields to it — the player put
    /// that one there on purpose.
    pub fn propose(&mut self, proposed: Vec<Marker>) {
        self.list.retain(|m| m.pinned);
        for m in proposed {
            if self.inside(m.frame) && self.clear_of_others(m.frame, None) {
                self.list.push(Marker { pinned: false, ..m });
            }
        }
        self.list.sort_by_key(|m| m.frame);
    }

    /// The player's own cut. `None` when it would land outside the region
    /// or on top of another cut; otherwise where it went in the list.
    pub fn add(&mut self, pcm: &SamplePcm, frame: u64) -> Option<usize> {
        let marker = Marker::at(pcm, frame, 0.0, true);
        if !self.inside(marker.frame) || !self.clear_of_others(marker.frame, None) {
            return None;
        }
        let index = self.list.partition_point(|m| m.frame < marker.frame);
        self.list.insert(index, marker);
        Some(index)
    }

    pub fn remove(&mut self, index: usize) -> Option<Marker> {
        (index < self.list.len()).then(|| self.list.remove(index))
    }

    /// Move one cut by `delta` frames, pinning it. It stops short of its
    /// neighbours and the region's edges rather than passing them, so the
    /// list's order is the slices' order and the keys' order, always.
    ///
    /// `snap` pulls it onto a zero crossing. Off for a player moving a cut a
    /// sample at a time: the snap reaches a millisecond, and would undo the
    /// very step they asked for.
    ///
    /// Returns where it landed.
    pub fn nudge(&mut self, pcm: &SamplePcm, index: usize, delta: i64, snap: bool) -> Option<u64> {
        let current = self.list.get(index)?;
        let low = match index.checked_sub(1) {
            Some(prev) => self.list[prev].frame + self.min_gap,
            None => self.start,
        };
        let high = match self.list.get(index + 1) {
            Some(next) => next.frame.saturating_sub(self.min_gap),
            None => self.end.saturating_sub(1),
        };
        if low > high {
            return Some(current.frame);
        }
        let wanted = current.frame.saturating_add_signed(delta).clamp(low, high);
        // The snap may not undo the clamp: a crossing past a neighbour is a
        // crossing the player did not ask for.
        let strength = current.strength;
        let snapped = if snap { Marker::at(pcm, wanted, strength, true).frame } else { wanted };
        let frame = if (low..=high).contains(&snapped) { snapped } else { wanted };
        self.list[index] = Marker { frame, strength, pinned: true };
        Some(frame)
    }

    /// Keep at most `count` cuts: every pinned one first (the earliest, if
    /// even those are too many), then the strongest of the rest. Order in
    /// time is kept, because it is the order the keys will be in.
    pub fn fit(&mut self, count: usize) {
        if self.list.len() <= count {
            return;
        }
        let mut ranked: Vec<usize> = (0..self.list.len()).collect();
        // Pinned before proposed, then loudest, then earliest: the sort is
        // total, so the same list always keeps the same cuts.
        ranked.sort_by(|&a, &b| {
            let (ma, mb) = (&self.list[a], &self.list[b]);
            mb.pinned
                .cmp(&ma.pinned)
                .then(mb.strength.total_cmp(&ma.strength))
                .then(ma.frame.cmp(&mb.frame))
        });
        let mut keep = vec![false; self.list.len()];
        for &i in ranked.iter().take(count) {
            keep[i] = true;
        }
        let mut index = 0;
        self.list.retain(|_| {
            index += 1;
            keep[index - 1]
        });
    }

    /// The slices the cuts make: each from its cut to the next, the last to
    /// the region's end. Sound before the first cut is in no slice — a cut
    /// is where a slice starts, and nobody asked for one there.
    pub fn slices(&self) -> Vec<(u64, u64)> {
        self.list
            .iter()
            .enumerate()
            .map(|(i, m)| (m.frame, self.list.get(i + 1).map_or(self.end, |n| n.frame)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 1_000.0; // one frame a millisecond: gaps read as numbers

    /// A buffer that crosses zero nowhere, so snapping never moves a cut
    /// and the arithmetic under test is the only thing that does.
    fn flat() -> SamplePcm {
        SamplePcm { data: vec![0.5; 2_000], channels: 1, sample_rate: RATE }
    }

    fn proposed(frames: &[(u64, f32)]) -> Vec<Marker> {
        frames.iter().map(|&(frame, strength)| Marker { frame, strength, pinned: false }).collect()
    }

    fn frames(m: &Markers) -> Vec<u64> {
        m.list().iter().map(|m| m.frame).collect()
    }

    #[test]
    fn slices_tile_the_region_from_the_first_cut() {
        let mut m = Markers::new(100, 1_000, RATE);
        m.propose(proposed(&[(200, 0.0), (500, 0.0), (800, 0.0)]));
        assert_eq!(m.slices(), vec![(200, 500), (500, 800), (800, 1_000)]);
        assert!(Markers::new(0, 10, RATE).slices().is_empty());
    }

    #[test]
    fn a_proposal_outside_the_region_or_on_top_of_another_is_dropped() {
        let mut m = Markers::new(100, 1_000, RATE);
        m.propose(proposed(&[(50, 0.0), (200, 0.0), (220, 0.0), (1_000, 0.0)]));
        assert_eq!(frames(&m), vec![200], "the gap is {MIN_GAP_MS} ms");
    }

    /// The whole point of pinning.
    #[test]
    fn re_detection_keeps_the_players_cuts_and_yields_to_them() {
        let pcm = flat();
        let mut m = Markers::new(0, 2_000, RATE);
        m.propose(proposed(&[(100, 0.0), (400, 0.0)]));
        let added = m.add(&pcm, 700).unwrap();
        m.nudge(&pcm, 0, 50, true);
        m.propose(proposed(&[(300, 0.0), (710, 0.0), (1_200, 0.0)]));
        assert_eq!(frames(&m), vec![150, 300, 700, 1_200], "the moved and the added cut must survive");
        assert!(m.list()[0].pinned && m.list()[2].pinned);
        assert_eq!(added, 2);
    }

    #[test]
    fn the_player_cannot_cut_on_top_of_a_cut_or_outside_the_region() {
        let pcm = flat();
        let mut m = Markers::new(100, 1_000, RATE);
        m.propose(proposed(&[(500, 0.0)]));
        assert_eq!(m.add(&pcm, 520), None);
        assert_eq!(m.add(&pcm, 50), None);
        assert_eq!(m.add(&pcm, 1_000), None);
        assert_eq!(m.add(&pcm, 300), Some(0));
        assert_eq!(m.remove(0).map(|c| c.frame), Some(300));
        assert_eq!(m.remove(5), None);
    }

    #[test]
    fn a_nudged_cut_stops_short_of_its_neighbours_and_the_edges() {
        let pcm = flat();
        let mut m = Markers::new(100, 1_000, RATE);
        m.propose(proposed(&[(200, 0.0), (500, 0.0), (800, 0.0)]));
        assert_eq!(m.nudge(&pcm, 1, -1_000, true), Some(240), "passed the cut before it");
        assert_eq!(m.nudge(&pcm, 1, 1_000, true), Some(760), "passed the cut after it");
        assert_eq!(m.nudge(&pcm, 0, -1_000, true), Some(100), "left the region");
        assert_eq!(m.nudge(&pcm, 2, 1_000, true), Some(999), "left the region");
        assert_eq!(m.nudge(&pcm, 9, 1, true), None);
        assert!(m.list().iter().all(|c| c.pinned), "a moved cut is the player's");
    }

    #[test]
    fn a_nudge_lands_on_a_zero_crossing_near_where_it_was_sent() {
        // Sign flips every 7 frames.
        let data = (0..2_000).map(|i| if (i / 7) % 2 == 0 { 0.5 } else { -0.5 }).collect();
        let pcm = SamplePcm { data, channels: 1, sample_rate: RATE };
        let mut m = Markers::new(0, 2_000, RATE);
        m.propose(proposed(&[(500, 0.0)]));
        // Crossings sit on the multiples of seven, and 1 ms of reach at
        // 1 kHz is one frame: 502 has none within one and stays, 505 has
        // one at 504 and goes there.
        assert_eq!(m.nudge(&pcm, 0, 2, true), Some(502));
        assert_eq!(m.nudge(&pcm, 0, 3, true), Some(504));
        // Unsnapped, a step of one is a step of one.
        assert_eq!(m.nudge(&pcm, 0, 1, false), Some(505));
    }

    #[test]
    fn fit_keeps_the_pinned_then_the_strongest_in_time_order() {
        let pcm = flat();
        let mut m = Markers::new(0, 2_000, RATE);
        m.propose(proposed(&[(100, -30.0), (300, -6.0), (500, -20.0), (700, -3.0), (900, -12.0)]));
        m.add(&pcm, 1_500);
        m.fit(3);
        assert_eq!(frames(&m), vec![300, 700, 1_500]);
        m.fit(10);
        assert_eq!(m.len(), 3, "fitting to more keys than cuts invents nothing");
        m.fit(0);
        assert!(m.is_empty());
    }

    #[test]
    fn fit_is_deterministic_when_strengths_tie() {
        let mut m = Markers::new(0, 2_000, RATE);
        m.propose(proposed(&[(100, 0.0), (300, 0.0), (500, 0.0)]));
        m.fit(2);
        assert_eq!(frames(&m), vec![100, 300], "a tie keeps the earlier cut");
    }
}
