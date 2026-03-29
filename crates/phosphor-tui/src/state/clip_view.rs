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
    InstConfig,
    PianoRoll,
    Settings,
}

impl ClipTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::InstConfig => "inst",
            Self::PianoRoll => "piano",
            Self::Settings => "settings",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::InstConfig => Self::PianoRoll,
            Self::PianoRoll => Self::Settings,
            Self::Settings => Self::InstConfig,
        }
    }

    pub const ALL: &[ClipTab] = &[Self::InstConfig, Self::PianoRoll, Self::Settings];
}

// ── Grid Resolution ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridResolution {
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
    QuarterT,
    EighthT,
    SixteenthT,
}

impl GridResolution {
    /// Fraction of a bar (4/4 time, 1 bar = column_count columns).
    /// This returns fraction relative to the full clip (0.0..1.0) when multiplied
    /// by (beats_per_bar / total_beats).
    pub fn subdivisions_per_beat(self) -> f64 {
        match self {
            Self::Quarter => 1.0,
            Self::Eighth => 2.0,
            Self::Sixteenth => 4.0,
            Self::ThirtySecond => 8.0,
            Self::QuarterT => 1.5,    // 3 in the space of 2
            Self::EighthT => 3.0,
            Self::SixteenthT => 6.0,
        }
    }

    /// Grid step as a fraction of the total clip, given total beats.
    pub fn step_frac(self, total_beats: usize) -> f64 {
        if total_beats == 0 { return 0.25; }
        1.0 / (total_beats as f64 * self.subdivisions_per_beat())
    }

    /// Snap a fractional position to the nearest grid line.
    pub fn snap(self, frac: f64, total_beats: usize) -> f64 {
        let step = self.step_frac(total_beats);
        if step <= 0.0 { return frac; }
        (frac / step).round() * step
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Quarter => "1/4",
            Self::Eighth => "1/8",
            Self::Sixteenth => "1/16",
            Self::ThirtySecond => "1/32",
            Self::QuarterT => "1/4T",
            Self::EighthT => "1/8T",
            Self::SixteenthT => "1/16T",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Quarter => Self::Eighth,
            Self::Eighth => Self::Sixteenth,
            Self::Sixteenth => Self::ThirtySecond,
            Self::ThirtySecond => Self::QuarterT,
            Self::QuarterT => Self::EighthT,
            Self::EighthT => Self::SixteenthT,
            Self::SixteenthT => Self::Quarter,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Quarter => Self::SixteenthT,
            Self::Eighth => Self::Quarter,
            Self::Sixteenth => Self::Eighth,
            Self::ThirtySecond => Self::Sixteenth,
            Self::QuarterT => Self::ThirtySecond,
            Self::EighthT => Self::QuarterT,
            Self::SixteenthT => Self::EighthT,
        }
    }
}

// ── Edit Mode Sub-States ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditSubMode {
    /// Navigating between notes by proximity.
    Navigate,
    /// Shift held: extending selection.
    Selecting,
    /// Notes selected, now moving them as a group.
    Moving,
}

#[derive(Debug)]
pub struct ClipViewState {
    pub focus: ClipViewFocus,
    pub fx_panel_tab: FxPanelTab,
    pub clip_tab: ClipTab,
    pub piano_roll: PianoRollState,
    pub fx_cursor: usize,
    pub synth_param_cursor: usize,
    /// Cursor position within the inst config panel.
    pub inst_config_cursor: usize,
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
            inst_config_cursor: 0,
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
/// Follows the Right Left Trick Controls pattern:
///   Navigation → Selected (column) → Row (individual note)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PianoRollFocus {
    /// h/l navigates columns, number keys jump, j/k scrolls view.
    /// Enter selects the current column.
    Navigation,
    /// Column selected. h/l = left edge, H/L = right edge of ALL notes.
    /// j/k drops to Row mode. Esc back to Navigation.
    Selected,
    /// Single note. h/l = left edge, H/L = right edge of ONE note.
    /// j/k moves between notes. Esc back to Selected.
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
    /// Indices of notes that belong to the selected column (set on Enter).
    /// Edits operate on these indices so notes don't "escape" the column.
    pub selected_note_indices: Vec<usize>,
    /// Number input buffer for typing column numbers.
    column_digits: String,
    /// Highlight range for bulk selection (Shift+h/l in Navigation mode).
    /// When set, columns from highlight_start..=highlight_end are selected.
    pub highlight_start: Option<usize>,
    pub highlight_end: Option<usize>,
    /// Number of columns visible on screen (set by renderer each frame).
    pub visible_columns: usize,
    /// Yanked (copied) notes buffer. Notes stored with start_frac relative to
    /// the yank origin (leftmost yanked column), so they can be pasted at any position.
    pub yank_buffer: Vec<phosphor_core::clip::NoteSnapshot>,
    /// Width of the yanked region in columns, so paste knows the source span.
    pub yank_columns: usize,
    /// Row highlight range (Shift+j/k). Stores MIDI note numbers (low..=high).
    pub row_highlight_low: Option<u8>,
    pub row_highlight_high: Option<u8>,
    // ── Edit mode ──
    pub edit_mode: bool,
    /// Index into the clip's notes vec — the "cursor" note.
    pub edit_cursor: usize,
    /// Indices of selected notes (for multi-select + move).
    pub edit_selected: Vec<usize>,
    pub edit_sub: EditSubMode,
    // ── Grid / snap ──
    pub grid: GridResolution,
    pub snap_enabled: bool,
    pub default_velocity: u8,
    /// Settings panel cursor (for the Settings tab).
    pub settings_cursor: usize,
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
            focus: PianoRollFocus::Navigation,
            column: 0,
            column_count: 16,
            selected_note_indices: Vec::new(),
            column_digits: String::new(),
            highlight_start: None,
            highlight_end: None,
            visible_columns: 16,
            row_highlight_low: None,
            row_highlight_high: None,
            yank_buffer: Vec::new(),
            yank_columns: 0,
            edit_mode: false,
            edit_cursor: 0,
            edit_selected: Vec::new(),
            edit_sub: EditSubMode::Navigate,
            grid: GridResolution::Eighth,
            snap_enabled: true,
            default_velocity: 100,
            settings_cursor: 0,
        }
    }

    // ── Focus transitions ──

    /// Enter the next focus level. `note_indices` are the indices of notes
    /// in the current column (captured at selection time so they don't drift).
    pub fn enter(&mut self, note_indices: Vec<usize>) {
        match self.focus {
            PianoRollFocus::Navigation => {
                self.focus = PianoRollFocus::Selected;
                self.selected_note_indices = note_indices;
            }
            PianoRollFocus::Selected | PianoRollFocus::Row => {}
        }
    }

    /// Enter row mode for the current cursor note (called when j/k finds a note).
    pub fn enter_row(&mut self) {
        self.focus = PianoRollFocus::Row;
    }

    pub fn escape(&mut self) {
        match self.focus {
            PianoRollFocus::Row => {
                self.focus = PianoRollFocus::Selected;
            }
            PianoRollFocus::Selected => {
                self.focus = PianoRollFocus::Navigation;
                self.column_digits.clear();
            }
            PianoRollFocus::Navigation => {
                // Handled by parent (exits clip view)
            }
        }
    }

    /// Returns true if escape was handled internally.
    pub fn can_escape(&self) -> bool {
        self.focus != PianoRollFocus::Navigation
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
            // Auto-scroll left
            if self.column < self.scroll_x {
                self.scroll_x = self.column;
            }
        }
    }

    pub fn move_column_right(&mut self) {
        if self.column + 1 < self.column_count {
            self.column += 1;
            // Auto-scroll right (visible_columns is set by renderer)
            if self.column >= self.scroll_x + self.visible_columns && self.visible_columns > 0 {
                self.scroll_x = self.column + 1 - self.visible_columns;
            }
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
                    // Auto-scroll to show the jumped-to column
                    self.ensure_column_visible();
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
                self.ensure_column_visible();
                return true;
            }
        }
        self.column_digits.clear();
        false
    }

    /// Scroll to make the current column visible.
    pub fn ensure_column_visible(&mut self) {
        if self.visible_columns == 0 { return; }
        if self.column < self.scroll_x {
            self.scroll_x = self.column;
        } else if self.column >= self.scroll_x + self.visible_columns {
            self.scroll_x = self.column + 1 - self.visible_columns;
        }
    }

    pub fn column_digits_display(&self) -> &str {
        &self.column_digits
    }

    // ── Highlight (Shift+h/l range selection) ──

    /// Begin or cancel highlighting at the current column.
    /// If already highlighting and range is just the anchor column, cancel.
    pub fn start_highlight(&mut self) {
        if let (Some(s), Some(e)) = (self.highlight_start, self.highlight_end) {
            if s == e && s == self.column {
                // Pressing shift on the same single column again = cancel
                self.clear_highlight();
                return;
            }
        }
        if self.highlight_start.is_none() {
            self.highlight_start = Some(self.column);
            self.highlight_end = Some(self.column);
        }
    }

    /// Expand highlight left (Shift+h while highlighting).
    pub fn highlight_left(&mut self) {
        if let (Some(start), Some(end)) = (self.highlight_start, self.highlight_end) {
            if self.column > 0 {
                self.column -= 1;
            }
            // Adjust range to include current column
            let new_start = self.column.min(start);
            let new_end = self.column.max(end);
            self.highlight_start = Some(new_start);
            self.highlight_end = Some(new_end);
            // If we moved back past our anchor, shrink from the other side
            if self.column >= start {
                self.highlight_end = Some(self.column);
            } else {
                self.highlight_start = Some(self.column);
            }
        }
    }

    /// Expand highlight right (Shift+l while highlighting).
    pub fn highlight_right(&mut self) {
        if let (Some(start), Some(end)) = (self.highlight_start, self.highlight_end) {
            if self.column + 1 < self.column_count {
                self.column += 1;
            }
            let new_start = self.column.min(start);
            let new_end = self.column.max(end);
            self.highlight_start = Some(new_start);
            self.highlight_end = Some(new_end);
            if self.column <= end {
                self.highlight_start = Some(self.column);
            } else {
                self.highlight_end = Some(self.column);
            }
        }
    }

    /// Clear the column highlight.
    pub fn clear_highlight(&mut self) {
        self.highlight_start = None;
        self.highlight_end = None;
    }

    // ── Row highlight (Shift+j/k) ──

    /// Begin or cancel row highlighting at the current cursor note.
    pub fn start_row_highlight(&mut self) {
        if let (Some(lo), Some(hi)) = (self.row_highlight_low, self.row_highlight_high) {
            if lo == hi && lo == self.cursor_note {
                self.clear_row_highlight();
                return;
            }
        }
        if self.row_highlight_low.is_none() {
            self.row_highlight_low = Some(self.cursor_note);
            self.row_highlight_high = Some(self.cursor_note);
        }
    }

    /// Expand row highlight downward (Shift+j).
    pub fn highlight_down(&mut self) {
        self.start_row_highlight();
        if self.cursor_note > 0 {
            self.cursor_note -= 1;
            if self.cursor_note < self.view_bottom_note {
                self.view_bottom_note = self.cursor_note;
            }
        }
        if let Some(lo) = self.row_highlight_low {
            self.row_highlight_low = Some(self.cursor_note.min(lo));
        }
        if let Some(hi) = self.row_highlight_high {
            self.row_highlight_high = Some(self.cursor_note.max(hi));
        }
    }

    /// Expand row highlight upward (Shift+k).
    pub fn highlight_up(&mut self) {
        self.start_row_highlight();
        if self.cursor_note < 127 {
            self.cursor_note += 1;
            let top = self.view_bottom_note.saturating_add(self.view_height);
            if self.cursor_note >= top {
                self.view_bottom_note = self.cursor_note - self.view_height + 1;
            }
        }
        if let Some(lo) = self.row_highlight_low {
            self.row_highlight_low = Some(self.cursor_note.min(lo));
        }
        if let Some(hi) = self.row_highlight_high {
            self.row_highlight_high = Some(self.cursor_note.max(hi));
        }
    }

    pub fn clear_row_highlight(&mut self) {
        self.row_highlight_low = None;
        self.row_highlight_high = None;
    }

    /// Check if a MIDI note is within the row highlight range.
    pub fn is_row_highlighted(&self, note: u8) -> bool {
        if let (Some(lo), Some(hi)) = (self.row_highlight_low, self.row_highlight_high) {
            note >= lo && note <= hi
        } else {
            false
        }
    }

    /// Get the highlighted row range as (low_note, high_note).
    pub fn row_highlight_range(&self) -> Option<(u8, u8)> {
        match (self.row_highlight_low, self.row_highlight_high) {
            (Some(lo), Some(hi)) => Some((lo, hi)),
            _ => None,
        }
    }

    /// Clear both column and row highlights.
    pub fn clear_all_highlights(&mut self) {
        self.clear_highlight();
        self.clear_row_highlight();
    }

    /// Check if a column is within the highlight range.
    pub fn is_highlighted(&self, col: usize) -> bool {
        if let (Some(start), Some(end)) = (self.highlight_start, self.highlight_end) {
            col >= start && col <= end
        } else {
            false
        }
    }

    /// Get the highlighted column range, if any.
    pub fn highlight_range(&self) -> Option<(usize, usize)> {
        match (self.highlight_start, self.highlight_end) {
            (Some(s), Some(e)) => Some((s.min(e), s.max(e))),
            _ => None,
        }
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
        assert_eq!(pr.focus, PianoRollFocus::Navigation);

        pr.enter(vec![]);
        assert_eq!(pr.focus, PianoRollFocus::Selected);

        // Enter in column mode does nothing — j/k finds notes and enters row mode
        pr.enter(vec![]);
        assert_eq!(pr.focus, PianoRollFocus::Selected);

        // Manually enter row mode (simulating finding a note)
        pr.enter_row();
        assert_eq!(pr.focus, PianoRollFocus::Row);

        pr.escape();
        assert_eq!(pr.focus, PianoRollFocus::Selected);

        pr.escape();
        assert_eq!(pr.focus, PianoRollFocus::Navigation);
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

        pr.enter(vec![]);
        assert!(pr.can_escape()); // column mode — internal

        pr.enter(vec![]);
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
