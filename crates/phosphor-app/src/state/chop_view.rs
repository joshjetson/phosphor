//! The chop screen: a chop being set up over the pad map.
//!
//! The plan is what will land; this is only where the screen's cursors are.
//! The screen names its track by the mixer's id for it rather than by its
//! place in the list, because a track deleted or moved above it would put a
//! different sampler at that place — and a chop set up on one kit must never
//! land on another.

use crate::sampler::chop::controls::{rows, ChopRow};
use crate::sampler::chop::plan::ChopPlan;
use crate::sampler::trim::NudgeUnit;

#[derive(Debug, Clone)]
pub struct ChopScreen {
    /// The mixer's id for the sampler track the chop is for.
    pub track_id: usize,
    pub plan: ChopPlan,
    /// Which row the cursor is on, an index into the mode's
    /// [`rows`](crate::sampler::chop::controls::rows).
    pub row: usize,
    /// Enter was pressed on the cuts row: `h`/`l` move the cut under the
    /// cursor, and nothing else gets a look at them.
    pub held: bool,
    /// How far one press moves a held cut — `j`/`k` walk it, the trim
    /// strip's ladder.
    pub unit: NudgeUnit,
}

impl ChopScreen {
    #[must_use]
    pub fn new(track_id: usize, plan: ChopPlan) -> Self {
        Self { track_id, plan, row: 0, held: false, unit: NudgeUnit::default() }
    }

    /// The rows the mode on screen shows.
    pub fn rows(&self) -> &'static [ChopRow] {
        rows(self.plan.mode)
    }

    /// The row under the cursor. The list can shrink under it — a grid has
    /// fewer rows than a transient chop — so it is read clamped.
    pub fn current(&self) -> ChopRow {
        let rows = self.rows();
        rows[self.row.min(rows.len() - 1)]
    }

    /// Move between rows, stopping at both ends.
    pub fn move_row(&mut self, delta: i32) {
        let last = self.rows().len() as i32 - 1;
        self.row = (self.row as i32 + delta).clamp(0, last) as usize;
    }

    /// Pull the cursor back inside the rows the mode has.
    pub fn clamp(&mut self) {
        self.row = self.row.min(self.rows().len() - 1);
        if self.plan.cuts().is_empty() {
            self.held = false;
        }
    }
}

impl super::NavState {
    /// The chop screen, when it is open for the track under the cursor.
    ///
    /// The one answer to "is the chop on screen here": the pad map draws
    /// it, the keys go to it and the hint bar names it by this, so the three
    /// can never disagree about a chop left open on another track.
    pub fn chop_here(&self) -> Option<&ChopScreen> {
        let id = self.tracks.get(self.track_cursor)?.mixer_id?;
        self.sampler_chop.as_deref().filter(|chop| chop.track_id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::chop::plan::ChopMode;
    use crate::sampler::LayerState;
    use phosphor_plugin::sample::SamplePcm;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn screen() -> ChopScreen {
        let data = (0..48_000).map(|i| if i % 12_000 < 200 { 0.8 } else { 0.0 }).collect();
        let pcm = Arc::new(SamplePcm { data, channels: 1, sample_rate: 48_000.0 });
        let plan = ChopPlan::new(LayerState::from_wav(PathBuf::from("clicks.wav"), pcm)).unwrap();
        ChopScreen::new(3, plan)
    }

    #[test]
    fn the_cursor_stops_at_both_ends_of_the_rows() {
        let mut s = screen();
        s.move_row(-1);
        assert_eq!(s.current(), ChopRow::Cuts);
        s.move_row(100);
        assert_eq!(s.current(), ChopRow::Clip);
    }

    /// A transient chop has more rows than an equal one: the cursor on the
    /// last of them has to land on a row that is still there.
    #[test]
    fn a_mode_with_fewer_rows_pulls_the_cursor_in() {
        let mut s = screen();
        s.move_row(100);
        s.plan.mode = ChopMode::Equal;
        s.plan.redetect();
        assert_eq!(s.current(), ChopRow::Clip, "read past the end of the rows");
        s.clamp();
        assert_eq!(s.row, s.rows().len() - 1);
    }

    #[test]
    fn nothing_to_hold_lets_go() {
        let mut s = screen();
        s.held = true;
        while s.plan.remove_cut() {}
        s.clamp();
        assert!(!s.held);
    }
}
