//! Loop region editor — the brace between the loop markers.
//!
//! Tick-native. The markers used to live in whole bars, which made the
//! brace a song-form tool and nothing else; a player who wants to chop a
//! quarter of a bar and run it needs the markers to move on a finer grid.
//! The grid is a setting the editor carries (`g` cycles it): bar, beat,
//! eighth, sixteenth. Whatever the grid, the region can never shrink
//! below one sixteenth — below that a "loop" is a buzz.
//!
//! When active, h/l move the left (start) marker and shift+h/l (or H/L)
//! move the right (end) marker, by one grid step.

use phosphor_core::transport::Transport;

/// One bar in ticks (4/4 time).
const TICKS_PER_BAR: i64 = Transport::PPQ * 4;

/// The smallest region the editor will make: one sixteenth.
pub const MIN_LOOP_TICKS: i64 = Transport::PPQ / 4;

/// The grid the markers move on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopStep {
    Bar,
    Beat,
    Eighth,
    Sixteenth,
}

impl LoopStep {
    pub const ALL: [LoopStep; 4] = [LoopStep::Bar, LoopStep::Beat, LoopStep::Eighth, LoopStep::Sixteenth];

    #[must_use]
    pub fn ticks(self) -> i64 {
        match self {
            Self::Bar => TICKS_PER_BAR,
            Self::Beat => Transport::PPQ,
            Self::Eighth => Transport::PPQ / 2,
            Self::Sixteenth => Transport::PPQ / 4,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::Beat => "beat",
            Self::Eighth => "1/8",
            Self::Sixteenth => "1/16",
        }
    }

    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Bar => Self::Beat,
            Self::Beat => Self::Eighth,
            Self::Eighth => Self::Sixteenth,
            Self::Sixteenth => Self::Bar,
        }
    }
}

#[derive(Debug)]
pub struct LoopEditor {
    /// Whether the loop editor is focused (controls locked to markers).
    pub active: bool,
    /// Whether the loop is enabled (playhead loops within the region).
    pub enabled: bool,
    /// Region start in ticks.
    pub start: i64,
    /// Region end in ticks, exclusive.
    pub end: i64,
    /// The grid the markers move on.
    pub step: LoopStep,
}

impl Default for LoopEditor {
    fn default() -> Self { Self::new() }
}

impl LoopEditor {
    pub fn new() -> Self {
        Self {
            active: false,
            enabled: false,
            start: 0,
            end: 4 * TICKS_PER_BAR,
            step: LoopStep::Bar,
        }
    }

    /// Focus the editor (lock controls to loop markers).
    pub fn focus(&mut self) {
        self.active = true;
    }

    /// Unfocus the editor (release controls).
    pub fn unfocus(&mut self) {
        self.active = false;
    }

    /// Toggle the loop on/off. Called when user presses Enter on the loop.
    pub fn toggle_enabled(&mut self) {
        self.enabled = !self.enabled;
    }

    /// Cycle the marker grid. The markers stay where they are — a brace
    /// set on a fine grid is not yanked to bar lines by looking at the
    /// knob — but every move after this lands on the new grid.
    pub fn cycle_step(&mut self) {
        self.step = self.step.next();
    }

    /// Move a tick onto the current grid in the direction of travel.
    fn snap(&self, tick: i64, toward_right: bool) -> i64 {
        let step = self.step.ticks();
        if tick % step == 0 {
            return tick;
        }
        if toward_right {
            (tick / step + 1) * step
        } else {
            (tick / step) * step
        }
    }

    /// Move the left (start) marker left by one grid step.
    pub fn move_start_left(&mut self) {
        let target = if self.start % self.step.ticks() == 0 {
            self.start - self.step.ticks()
        } else {
            self.snap(self.start, false)
        };
        self.start = target.max(0);
    }

    /// Move the left (start) marker right; it never closes the region
    /// below one sixteenth.
    pub fn move_start_right(&mut self) {
        let target = if self.start % self.step.ticks() == 0 {
            self.start + self.step.ticks()
        } else {
            self.snap(self.start, true)
        };
        if target <= self.end - MIN_LOOP_TICKS {
            self.start = target;
        } else if self.end - MIN_LOOP_TICKS > self.start {
            self.start = self.end - MIN_LOOP_TICKS;
        }
    }

    /// Move the right (end) marker left; same floor.
    pub fn move_end_left(&mut self) {
        let target = if self.end % self.step.ticks() == 0 {
            self.end - self.step.ticks()
        } else {
            self.snap(self.end, false)
        };
        if target >= self.start + MIN_LOOP_TICKS {
            self.end = target;
        } else if self.start + MIN_LOOP_TICKS < self.end {
            self.end = self.start + MIN_LOOP_TICKS;
        }
    }

    /// Move the right (end) marker right by one grid step.
    pub fn move_end_right(&mut self) {
        self.end = if self.end % self.step.ticks() == 0 {
            self.end + self.step.ticks()
        } else {
            self.snap(self.end, true)
        };
    }

    /// Start tick for the transport.
    #[must_use]
    pub fn start_ticks(&self) -> i64 {
        self.start
    }

    /// End tick for the transport (exclusive).
    #[must_use]
    pub fn end_ticks(&self) -> i64 {
        self.end
    }

    /// The region's length in ticks.
    #[must_use]
    pub fn len_ticks(&self) -> i64 {
        self.end - self.start
    }

    /// Set the region directly (session load, undo).
    pub fn set_region(&mut self, start: i64, end: i64) {
        self.start = start.max(0);
        self.end = end.max(self.start + MIN_LOOP_TICKS);
    }

    /// Whole bars covered, rounded up — for displays that think in bars.
    #[must_use]
    pub fn bar_count(&self) -> u32 {
        ((self.len_ticks() + TICKS_PER_BAR - 1) / TICKS_PER_BAR) as u32
    }

    /// A musical position: "3" on a bar line, "3.2" on a beat, "3.2.4"
    /// on a sixteenth. Bars, beats and sixteenths all read 1-based, the
    /// way a musician counts them.
    fn position(tick: i64) -> String {
        let bar = tick / TICKS_PER_BAR + 1;
        let in_bar = tick % TICKS_PER_BAR;
        let beat = in_bar / Transport::PPQ + 1;
        let in_beat = in_bar % Transport::PPQ;
        let sixteenth = in_beat / (Transport::PPQ / 4) + 1;
        if in_bar == 0 {
            format!("{bar}")
        } else if in_beat == 0 {
            format!("{bar}.{beat}")
        } else {
            format!("{bar}.{beat}.{sixteenth}")
        }
    }

    /// Display string: "1-4" for whole bars (inclusive, as before), a
    /// position range like "1.2-1.3" otherwise.
    #[must_use]
    pub fn display(&self) -> String {
        if self.start % TICKS_PER_BAR == 0 && self.end % TICKS_PER_BAR == 0 {
            let first = self.start / TICKS_PER_BAR + 1;
            let last = self.end / TICKS_PER_BAR; // inclusive last bar
            format!("{first}-{last}")
        } else {
            format!("{}-{}", Self::position(self.start), Self::position(self.end))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_4_bars() {
        let le = LoopEditor::new();
        assert_eq!(le.start, 0);
        assert_eq!(le.end, 4 * TICKS_PER_BAR);
        assert_eq!(le.bar_count(), 4);
        assert_eq!(le.display(), "1-4");
    }

    #[test]
    fn start_cant_go_below_zero() {
        let mut le = LoopEditor::new();
        le.move_start_left();
        le.move_start_left();
        assert_eq!(le.start, 0);
    }

    /// The brace shrinks below a bar on the finer grids, down to a
    /// sixteenth and no further — a smaller loop is a buzz, not a loop.
    #[test]
    fn the_brace_shrinks_to_a_sixteenth_and_stops() {
        let mut le = LoopEditor::new();
        le.step = LoopStep::Sixteenth;
        // Close the region from four bars to the floor.
        for _ in 0..1000 {
            le.move_end_left();
        }
        assert_eq!(le.len_ticks(), MIN_LOOP_TICKS, "the floor did not hold");
        // And the start cannot climb over it either.
        for _ in 0..10 {
            le.move_start_right();
        }
        assert_eq!(le.len_ticks(), MIN_LOOP_TICKS);
        assert!(le.end > le.start, "the region inverted");
    }

    /// Off-grid markers snap in the direction of travel rather than
    /// jumping a whole step past where the player is aiming.
    #[test]
    fn off_grid_markers_snap_toward_the_move() {
        let mut le = LoopEditor::new();
        le.set_region(100, TICKS_PER_BAR * 2 + 100);
        le.step = LoopStep::Beat;
        le.move_start_left();
        assert_eq!(le.start, 0, "left move should land on the beat below");
        le.set_region(100, TICKS_PER_BAR * 2 + 100);
        le.move_start_right();
        assert_eq!(le.start, Transport::PPQ, "right move should land on the beat above");
    }

    /// The display reads bars for whole bars and positions for the rest.
    #[test]
    fn the_display_speaks_both_dialects() {
        let mut le = LoopEditor::new();
        assert_eq!(le.display(), "1-4");
        le.set_region(Transport::PPQ, Transport::PPQ * 2);
        assert_eq!(le.display(), "1.2-1.3");
        le.set_region(Transport::PPQ / 4, Transport::PPQ / 2);
        assert_eq!(le.display(), "1.1.2-1.1.3");
        le.set_region(0, MIN_LOOP_TICKS);
        assert_eq!(le.display(), "1-1.1.2");
    }

    /// Cycling the grid never moves the markers.
    #[test]
    fn changing_the_grid_leaves_the_brace_alone() {
        let mut le = LoopEditor::new();
        le.step = LoopStep::Sixteenth;
        le.move_end_left(); // end now off the bar grid
        let (s, e) = (le.start, le.end);
        le.cycle_step();
        le.cycle_step();
        assert_eq!((le.start, le.end), (s, e));
    }
}
