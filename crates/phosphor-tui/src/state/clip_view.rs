//! Clip view state — ClipViewState, focus, tabs, piano roll.

/// Which sub-panel of the clip view has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipViewFocus {
    FxPanel,
    PianoRoll,
}

/// Tab in the FX panel (left side of clip view).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxPanelTab {
    TrackFx,
    Synth,
}

impl FxPanelTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::TrackFx => "trk fx",
            Self::Synth => "synth",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::TrackFx => Self::Synth,
            Self::Synth => Self::TrackFx,
        }
    }
}

/// Tab in the piano roll / clip area (right side of clip view).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipTab {
    PianoRoll,
    Automation,
}

impl ClipTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::PianoRoll => "piano",
            Self::Automation => "auto",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::PianoRoll => Self::Automation,
            Self::Automation => Self::PianoRoll,
        }
    }
}

#[derive(Debug)]
pub struct ClipViewState {
    pub focus: ClipViewFocus,
    pub fx_panel_tab: FxPanelTab,
    pub clip_tab: ClipTab,
    pub piano_roll: PianoRollState,
    pub fx_cursor: usize,
    pub synth_param_cursor: usize,
}

impl Default for ClipViewState {
    fn default() -> Self { Self::new() }
}

impl ClipViewState {
    pub fn new() -> Self {
        Self {
            focus: ClipViewFocus::PianoRoll,
            fx_panel_tab: FxPanelTab::TrackFx,
            clip_tab: ClipTab::PianoRoll,
            piano_roll: PianoRollState::new(),
            fx_cursor: 0,
            synth_param_cursor: 0,
        }
    }
}

// ── Piano Roll Navigation ──
//
// Focus hierarchy (Enter goes deeper, Esc goes back):
//   Browsing → Column selected → Row selected
//
// Browsing: j/k scrolls notes, h/l scrolls horizontally
// Column selected: h/l moves between columns, j/k moves rows within column
//   h/l (no shift) = adjust left edge of all notes in column
//   H/L (shift)    = adjust right edge of all notes in column
// Row selected: same h/l/H/L but affects only the single note

/// What level of the piano roll is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PianoRollFocus {
    /// Browsing — j/k scrolls note rows, no column selected.
    Browsing,
    /// A column is highlighted. j/k navigates rows, h/l navigates columns.
    /// Enter goes to row editing.
    Column,
    /// A specific note row within a column is selected for editing.
    /// h/l adjusts left edge, H/L adjusts right edge.
    Row,
}

#[derive(Debug)]
pub struct PianoRollState {
    pub cursor_note: u8,
    pub scroll_x: usize,
    pub view_bottom_note: u8,
    pub view_height: u8,
    /// Current focus level.
    pub focus: PianoRollFocus,
    /// Currently selected column (0-based). Columns map to time subdivisions.
    pub column: usize,
    /// Total number of columns in the grid (set by renderer).
    pub column_count: usize,
    /// Number input buffer for typing column numbers.
    column_digits: String,
}

impl Default for PianoRollState {
    fn default() -> Self { Self::new() }
}

impl PianoRollState {
    pub fn new() -> Self {
        Self {
            cursor_note: 60,
            scroll_x: 0,
            view_bottom_note: 48,
            view_height: 24,
            focus: PianoRollFocus::Browsing,
            column: 0,
            column_count: 16,
            column_digits: String::new(),
        }
    }

    // ── Focus transitions ──

    pub fn enter(&mut self) {
        match self.focus {
            PianoRollFocus::Browsing => {
                self.focus = PianoRollFocus::Column;
            }
            PianoRollFocus::Column => {
                // Column is already selected — Enter does nothing.
                // Use j/k to navigate to a note within the column (enters Row mode).
            }
            PianoRollFocus::Row => {
                // Already at deepest level — no-op
            }
        }
    }

    /// Enter row mode for the current cursor note (called when j/k finds a note).
    pub fn enter_row(&mut self) {
        self.focus = PianoRollFocus::Row;
    }

    pub fn escape(&mut self) {
        match self.focus {
            PianoRollFocus::Row => {
                self.focus = PianoRollFocus::Column;
            }
            PianoRollFocus::Column => {
                self.focus = PianoRollFocus::Browsing;
                self.column_digits.clear();
            }
            PianoRollFocus::Browsing => {
                // Handled by parent (exits clip view)
            }
        }
    }

    /// Returns true if escape was handled internally.
    pub fn can_escape(&self) -> bool {
        self.focus != PianoRollFocus::Browsing
    }

    // ── Note scrolling (browsing + column mode) ──

    pub fn move_up(&mut self) {
        if self.cursor_note < 127 {
            self.cursor_note += 1;
            let top = self.view_bottom_note.saturating_add(self.view_height);
            if self.cursor_note >= top {
                self.view_bottom_note = self.cursor_note - self.view_height + 1;
            }
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_note > 0 {
            self.cursor_note -= 1;
            if self.cursor_note < self.view_bottom_note {
                self.view_bottom_note = self.cursor_note;
            }
        }
    }

    // ── Column navigation ──

    pub fn move_column_left(&mut self) {
        if self.column > 0 {
            self.column -= 1;
        }
    }

    pub fn move_column_right(&mut self) {
        if self.column + 1 < self.column_count {
            self.column += 1;
        }
    }

    /// Type a digit for column number jump. Returns true if the column was set.
    pub fn type_digit(&mut self, ch: char) -> bool {
        self.column_digits.push(ch);
        if let Ok(num) = self.column_digits.parse::<usize>() {
            if num >= 1 && num <= self.column_count {
                // If no further digit could make a valid larger number, resolve now
                let could_grow = num * 10 <= self.column_count;
                if !could_grow || self.column_digits.len() >= 2 {
                    self.column = num - 1;
                    self.column_digits.clear();
                    return true;
                }
                // Single digit but could be prefix of larger number — wait
                return false;
            }
        }
        // Invalid — clear
        self.column_digits.clear();
        false
    }

    /// Force-resolve whatever is in the digit buffer.
    pub fn commit_digits(&mut self) -> bool {
        if let Ok(num) = self.column_digits.parse::<usize>() {
            if num >= 1 && num <= self.column_count {
                self.column = num - 1;
                self.column_digits.clear();
                return true;
            }
        }
        self.column_digits.clear();
        false
    }

    pub fn column_digits_display(&self) -> &str {
        &self.column_digits
    }

    pub fn set_view_height(&mut self, h: u8) {
        self.view_height = h.max(1);
    }

    pub fn set_column_count(&mut self, count: usize) {
        self.column_count = count.max(1);
        if self.column >= self.column_count {
            self.column = self.column_count - 1;
        }
    }

    /// The 1-based column number for display.
    pub fn column_display(&self) -> usize {
        self.column + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_hierarchy() {
        let mut pr = PianoRollState::new();
        assert_eq!(pr.focus, PianoRollFocus::Browsing);

        pr.enter();
        assert_eq!(pr.focus, PianoRollFocus::Column);

        // Enter in column mode does nothing — j/k finds notes and enters row mode
        pr.enter();
        assert_eq!(pr.focus, PianoRollFocus::Column);

        // Manually enter row mode (simulating finding a note)
        pr.enter_row();
        assert_eq!(pr.focus, PianoRollFocus::Row);

        pr.escape();
        assert_eq!(pr.focus, PianoRollFocus::Column);

        pr.escape();
        assert_eq!(pr.focus, PianoRollFocus::Browsing);
    }

    #[test]
    fn column_navigation() {
        let mut pr = PianoRollState::new();
        pr.column_count = 16;
        pr.column = 0;

        pr.move_column_right();
        assert_eq!(pr.column, 1);

        pr.move_column_left();
        assert_eq!(pr.column, 0);

        pr.move_column_left();
        assert_eq!(pr.column, 0); // can't go below 0

        pr.column = 15;
        pr.move_column_right();
        assert_eq!(pr.column, 15); // can't go past last
    }

    #[test]
    fn digit_jump() {
        let mut pr = PianoRollState::new();
        pr.column_count = 16;

        // Single digit > max prefix: resolves immediately
        // '5' could be prefix of nothing valid (50 > 16), so resolves
        assert!(pr.type_digit('5'));
        assert_eq!(pr.column, 4); // 0-based

        // '1' could be prefix of 10-16, so it waits
        assert!(!pr.type_digit('1'));
        // '2' makes it 12, resolves
        assert!(pr.type_digit('2'));
        assert_eq!(pr.column, 11); // column 12 = index 11

        // Single '9' — 9*10=90 > 16, resolves immediately
        assert!(pr.type_digit('9'));
        assert_eq!(pr.column, 8);

        // Single '1' then commit
        pr.type_digit('1');
        assert!(pr.commit_digits());
        assert_eq!(pr.column, 0);
    }

    #[test]
    fn can_escape() {
        let mut pr = PianoRollState::new();
        assert!(!pr.can_escape()); // browsing — parent handles esc

        pr.enter();
        assert!(pr.can_escape()); // column mode — internal

        pr.enter();
        assert!(pr.can_escape()); // row mode — internal
    }

    #[test]
    fn note_scroll() {
        let mut pr = PianoRollState::new();
        pr.view_height = 10;
        pr.view_bottom_note = 50;
        pr.cursor_note = 55;

        // Move up past visible area
        for _ in 0..10 {
            pr.move_up();
        }
        // Cursor should have scrolled the view
        assert!(pr.cursor_note >= pr.view_bottom_note);
        assert!(pr.cursor_note < pr.view_bottom_note + pr.view_height);
    }
}
