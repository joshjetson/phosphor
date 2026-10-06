//! The stretch of the song the track lanes show.
//!
//! The lanes used to be a fixed sixteen bars from the top of the song, so a
//! clip past bar sixteen was off the edge with no way to reach it, and a loop
//! brace one beat wide was half a cell — a section too small to see is a
//! section too small to copy. The view is a window: where it starts, and how
//! many bars it holds. Zooming changes the second, scrolling the first.
//!
//! The view is where the player is looking, not part of the song, so it is
//! never on the undo stack and never saved.

use crate::timeline::TICKS_PER_BAR;

use super::{NavState, TrackElement};

/// How many bars the lanes can hold, closest first.
pub const ZOOMS: [i64; 8] = [1, 2, 4, 8, 16, 32, 64, 128];

/// Where a fresh session looks: the first sixteen bars, as it always has.
pub const DEFAULT_BARS: i64 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineView {
    /// The first bar on screen, from zero.
    pub first_bar: i64,
    /// How many bars fit across the lanes. Always one of [`ZOOMS`].
    pub bars: i64,
}

impl Default for TimelineView {
    fn default() -> Self {
        Self { first_bar: 0, bars: DEFAULT_BARS }
    }
}

impl TimelineView {
    #[must_use]
    pub fn first_tick(&self) -> i64 {
        self.first_bar * TICKS_PER_BAR
    }

    #[must_use]
    pub fn end_tick(&self) -> i64 {
        (self.first_bar + self.bars) * TICKS_PER_BAR
    }

    #[must_use]
    pub fn ticks(&self) -> i64 {
        self.bars * TICKS_PER_BAR
    }

    /// The column a tick lands in across `width` cells. Off the left edge is
    /// negative and off the right is `width` or more, so a clip that starts
    /// before the view can still be clipped to it rather than dropped.
    #[must_use]
    pub fn column(&self, tick: i64, width: usize) -> i64 {
        let offset = i128::from(tick - self.first_tick());
        (offset * width as i128).div_euclid(i128::from(self.ticks())) as i64
    }

    /// The first tick a column covers — the inverse of [`Self::column`].
    #[must_use]
    pub fn tick_at(&self, column: usize, width: usize) -> i64 {
        if width == 0 {
            return self.first_tick();
        }
        self.first_tick() + (self.ticks() as i128 * column as i128 / width as i128) as i64
    }

    /// Whether any of `start..end` is on screen.
    #[must_use]
    pub fn shows(&self, start: i64, end: i64) -> bool {
        end > self.first_tick() && start < self.end_tick()
    }

    /// One zoom level closer, keeping `anchor` where it was on screen as
    /// nearly as whole bars allow.
    pub fn zoom_in(&mut self, anchor: i64) {
        if let Some(&closer) = ZOOMS.iter().rev().find(|&&z| z < self.bars) {
            self.rezoom(closer, anchor);
        }
    }

    /// One zoom level further out, about `anchor`.
    pub fn zoom_out(&mut self, anchor: i64) {
        if let Some(&further) = ZOOMS.iter().find(|&&z| z > self.bars) {
            self.rezoom(further, anchor);
        }
    }

    fn rezoom(&mut self, bars: i64, anchor: i64) {
        // Where the anchor sat, as a fraction of the old window, is where it
        // sits in the new one.
        let old = self.ticks();
        let into = (anchor - self.first_tick()).clamp(0, old);
        let new = bars * TICKS_PER_BAR;
        let first = anchor - (i128::from(into) * i128::from(new) / i128::from(old)) as i64;
        self.bars = bars;
        self.first_bar = first.div_euclid(TICKS_PER_BAR).max(0);
        // Whole bars can round the anchor off the edge; never lose it.
        let anchor_bar = anchor.div_euclid(TICKS_PER_BAR).max(0);
        self.first_bar = self.first_bar.clamp((anchor_bar + 1 - bars).max(0), anchor_bar);
    }

    /// Scroll by half a window at a time, never before bar 1.
    pub fn scroll_halves(&mut self, halves: i64) {
        let step = (self.bars / 2).max(1);
        self.first_bar = (self.first_bar + halves * step).max(0);
    }

    /// Scroll as little as possible to put `start..end` on screen. A span
    /// wider than the window shows its start. Zoom is left alone: following
    /// something must not change how big everything else looks.
    pub fn reveal(&mut self, start: i64, end: i64) {
        let end = end.max(start + 1);
        if start >= self.first_tick() && end <= self.end_tick() {
            return;
        }
        if start < self.first_tick() || end - start > self.ticks() {
            self.first_bar = start.div_euclid(TICKS_PER_BAR).max(0);
        } else {
            // Off the right: bring the end in, with the bar it ends in.
            let last_bar = (end - 1).div_euclid(TICKS_PER_BAR);
            self.first_bar = (last_bar + 1 - self.bars).max(0);
        }
    }

    /// The closest zoom that holds `start..end` whole, scrolled to it.
    pub fn fit(&mut self, start: i64, end: i64) {
        let end = end.max(start + 1);
        let first = start.div_euclid(TICKS_PER_BAR).max(0);
        let last = (end - 1).div_euclid(TICKS_PER_BAR);
        let needed = last - first + 1;
        self.bars = ZOOMS.iter().copied().find(|&z| z >= needed).unwrap_or(ZOOMS[ZOOMS.len() - 1]);
        self.first_bar = first;
    }

    /// The window to draw while the transport is at `playhead`: this one, or
    /// — while playing past its edge — the page the playhead is on, so the
    /// lanes turn like the pages of a score instead of losing the music.
    #[must_use]
    pub fn following(&self, playhead: i64, playing: bool) -> Self {
        if !playing || (playhead >= self.first_tick() && playhead < self.end_tick()) {
            return *self;
        }
        let bar = playhead.div_euclid(TICKS_PER_BAR).max(0);
        Self { first_bar: bar - bar.rem_euclid(self.bars), bars: self.bars }
    }
}

impl NavState {
    /// Keep what the player is working on in the lanes: the brace while the
    /// loop editor has the keys, the clip under the cursor otherwise. Called
    /// after every input, so a brace walked off the edge, or a clip stepped
    /// to past bar sixteen, scrolls into view on the same key.
    pub fn follow_focus(&mut self) {
        if self.loop_editor.active {
            let (start, end) = (self.loop_editor.start, self.loop_editor.end);
            self.timeline.reveal(start, end);
            return;
        }
        if !self.track_selected {
            return;
        }
        let TrackElement::Clip(i) = self.track_element else { return };
        let span = self
            .tracks
            .get(self.track_cursor)
            .and_then(|t| t.clips.get(i))
            .map(|c| (c.start_tick, c.start_tick + c.length_ticks));
        if let Some((start, end)) = span {
            self.timeline.reveal(start, end);
        }
    }

    /// Where a zoom key zooms about: the clip under the cursor, the brace,
    /// or the middle of the lanes.
    #[must_use]
    pub fn zoom_anchor(&self) -> i64 {
        if self.loop_editor.active {
            return self.loop_editor.start;
        }
        if let (true, TrackElement::Clip(i)) = (self.track_selected, self.track_element) {
            if let Some(clip) = self.tracks.get(self.track_cursor).and_then(|t| t.clips.get(i)) {
                return clip.start_tick;
            }
        }
        self.timeline.first_tick() + self.timeline.ticks() / 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: i64 = TICKS_PER_BAR;

    #[test]
    fn a_fresh_view_is_the_first_sixteen_bars() {
        let v = TimelineView::default();
        assert_eq!((v.first_tick(), v.end_tick()), (0, 16 * BAR));
        assert_eq!(v.column(0, 160), 0);
        assert_eq!(v.column(BAR, 160), 10);
        assert_eq!(v.column(16 * BAR, 160), 160);
    }

    #[test]
    fn columns_off_either_edge_are_out_of_range_not_wrapped() {
        let v = TimelineView { first_bar: 4, bars: 4 };
        assert!(v.column(0, 80) < 0);
        assert!(v.column(9 * BAR, 80) >= 80);
        assert_eq!(v.tick_at(0, 80), 4 * BAR);
        assert_eq!(v.tick_at(20, 80), 5 * BAR);
    }

    /// Zooming in on a spot keeps it on screen, at every level, both ways.
    #[test]
    fn zoom_keeps_the_anchor_in_view() {
        let mut v = TimelineView::default();
        let anchor = 11 * BAR + BAR / 2;
        for _ in 0..ZOOMS.len() {
            v.zoom_in(anchor);
            assert!(v.shows(anchor, anchor + 1), "lost the anchor at {} bars", v.bars);
        }
        assert_eq!(v.bars, 1);
        for _ in 0..ZOOMS.len() {
            v.zoom_out(anchor);
            assert!(v.shows(anchor, anchor + 1), "lost the anchor at {} bars", v.bars);
        }
        assert_eq!(v.bars, 128);
    }

    #[test]
    fn reveal_scrolls_the_least_it_can() {
        let mut v = TimelineView::default();
        v.reveal(2 * BAR, 3 * BAR);
        assert_eq!(v.first_bar, 0, "already on screen, nothing moves");
        v.reveal(20 * BAR, 21 * BAR);
        assert_eq!(v.first_bar, 5, "bar 21 brought in at the right edge");
        v.reveal(BAR, 2 * BAR);
        assert_eq!(v.first_bar, 1, "back to the left edge");
        v.reveal(40 * BAR, 80 * BAR);
        assert_eq!(v.first_bar, 40, "too wide to fit shows its start");
        assert_eq!(v.bars, 16, "following never zooms");
    }

    #[test]
    fn fit_picks_the_closest_zoom_that_holds_the_span() {
        let mut v = TimelineView::default();
        v.fit(BAR + BAR / 4, BAR + BAR / 2);
        assert_eq!((v.first_bar, v.bars), (1, 1), "a beat fits in one bar");
        v.fit(3 * BAR, 8 * BAR);
        assert_eq!((v.first_bar, v.bars), (3, 8));
        v.fit(0, 100 * BAR);
        assert_eq!(v.bars, 128);
    }

    #[test]
    fn playback_turns_the_page() {
        let v = TimelineView::default();
        assert_eq!(v.following(3 * BAR, true), v);
        assert_eq!(v.following(17 * BAR, true).first_bar, 16);
        assert_eq!(v.following(17 * BAR, false), v, "stopped, the view stays put");
    }
}
