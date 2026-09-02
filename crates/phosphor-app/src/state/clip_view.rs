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
    /// The step grid. Only reachable on a track that has a sequencer on it —
    /// [`ClipTab::next`] steps over it everywhere else, and the tab strip
    /// leaves it out.
    Sequencer,
    /// One effect's panel. Reachable by opening a slot from the chain list,
    /// and left out of the strip until there is a slot open — a tab for a
    /// panel that has no effect behind it is a tab that shows nothing.
    Fx,
}

impl ClipTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::InstConfig => "inst",
            Self::PianoRoll => "piano",
            Self::Settings => "settings",
            Self::Sequencer => "seq",
            Self::Fx => "fx",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::InstConfig => Self::PianoRoll,
            Self::PianoRoll => Self::Settings,
            Self::Settings => Self::InstConfig,
            Self::Sequencer | Self::Fx => Self::InstConfig,
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
    /// Notes selected. Plain h/l/j/k = move. Shift+h/l = stretch right edge. Shift+j/k = stretch left edge.
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
    /// Where the cursor is standing in the step grid, and whether a control
    /// is locked. Only ever cursors: what a sequencer *contains* lives in
    /// [`crate::sequencer::SequencerState`] and is edited through its ops.
    pub sequencer: SequencerView,
    /// Which effect's panel is open, and where the cursor is in it.
    pub fx: FxView,
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
            sequencer: SequencerView::new(),
            fx: FxView::new(),
        }
    }
}

// ── The effect panel ──

/// Which effect's panel is open, and where the cursor is inside it.
///
/// Cursors only. What an effect *is* lives in its slot's parameter vector and
/// is edited through the one path that also tells the audio thread — see
/// `App::set_fx_param`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FxView {
    /// The slot whose panel is open, if one is.
    pub slot: Option<usize>,
    /// The band under the cursor, `0..8`; [`FxView::TRIM`] is the output trim.
    pub band: usize,
    /// The control under the cursor inside that band, in the EQ's own order:
    /// type, freq, gain, q, slope, on.
    pub control: usize,
    /// Enter was pressed on it: `h`/`l` now adjust it and nothing else gets
    /// a look. The fader's contract, applied to an EQ band.
    pub locked: bool,
    /// Whether the panel has room for the wide layout — bands as columns,
    /// with the response curve over them. Set from the terminal each frame,
    /// because it decides which way `h`/`l` and `j`/`k` point: the cursor
    /// moves the way the screen looks.
    pub wide: bool,
}

impl FxView {
    /// The band index that addresses the output trim rather than a band.
    pub const TRIM: usize = 8;
    /// How many controls a band has.
    pub const CONTROLS: usize = 6;

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a slot's panel, with the cursor on the first band's frequency —
    /// the control a player reaches for first, and never on the type, which
    /// would make the first stray keypress rewrite the band.
    pub fn open(&mut self, slot: usize) {
        self.slot = Some(slot);
        self.band = 0;
        self.control = 1;
        self.locked = false;
    }

    /// Shut the panel, answering whether one was open.
    pub fn close(&mut self) -> bool {
        self.locked = false;
        self.slot.take().is_some()
    }

    /// Move between bands, the trim included, stopping at both ends.
    pub fn move_band(&mut self, delta: i32) {
        if self.locked {
            return;
        }
        self.band = (self.band as i32 + delta).clamp(0, Self::TRIM as i32) as usize;
    }

    /// Move between the controls of a band. The trim has one control, so the
    /// cursor stays on it.
    pub fn move_control(&mut self, delta: i32, count: usize) {
        if self.locked || count == 0 {
            return;
        }
        self.control = (self.control as i32 + delta).clamp(0, count as i32 - 1) as usize;
    }

    /// Move along a flat list of controls, stopping at both ends.
    ///
    /// What a panel that is a *column of knobs* rather than a grid of bands
    /// uses: the reverb's twelve controls are addressed by [`FxView::band`]
    /// directly, because there is nothing inside them to be the `control` of.
    pub fn move_cursor(&mut self, delta: i32, count: usize) {
        if self.locked || count == 0 {
            return;
        }
        self.band = (self.band as i32 + delta).clamp(0, count as i32 - 1) as usize;
    }

    /// The flat parameter index the cursor addresses, in the EQ's own
    /// numbering: `band * 6 + control`, and 48 for the trim.
    #[must_use]
    pub fn param_index(&self) -> usize {
        if self.band >= Self::TRIM {
            Self::TRIM * Self::CONTROLS
        } else {
            self.band * Self::CONTROLS + self.control.min(Self::CONTROLS - 1)
        }
    }
}

// ── Sequencer view ──

/// Which horizontal band of the step grid view has the cursor.
///
/// `j`/`k` walk this list; `h`/`l` move inside whichever band is on. The step
/// cursor, the lane and the selected slot are not here — those are edits, and
/// live in the sequencer itself so that a controller changing one moves the
/// same cursor a key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SeqBand {
    /// The step row. `h`/`l` walks steps, `n` writes one.
    #[default]
    Grid,
    /// The controls belonging to the step under the cursor.
    Step,
    /// The controls belonging to the pattern.
    Pattern,
    /// The eight slots and the chain.
    Slots,
}

impl SeqBand {
    pub const ALL: [SeqBand; 4] = [Self::Grid, Self::Step, Self::Pattern, Self::Slots];

    pub fn index(self) -> usize {
        match self {
            Self::Grid => 0,
            Self::Step => 1,
            Self::Pattern => 2,
            Self::Slots => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::Step => "step",
            Self::Pattern => "pattern",
            Self::Slots => "slots",
        }
    }

    /// One band along, stopping at the ends rather than wrapping: a list that
    /// wraps makes `j` at the bottom jump back to the top, which reads as the
    /// cursor having been lost.
    pub fn stepped(self, delta: i32) -> Self {
        let target = (self.index() as i32 + delta).clamp(0, Self::ALL.len() as i32 - 1);
        Self::ALL[target as usize]
    }
}

/// One control on the step grid's panels.
///
/// Named here rather than in either of the two places that use it, because
/// both have to agree: the key handler turns a press on one of these into an
/// op, and the renderer draws the same list in the same order. A knob the
/// keys know about and the panel does not is a knob nobody can reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqKnob {
    // ── The step under the cursor ──
    /// The one pitch control: semitones, or scale degrees in a mode.
    Pitch,
    Chord,
    Voicing,
    /// Double the root an octave down.
    RootBelow,
    Gate,
    // ── The lane, when it is pinned to a drum voice ──
    /// Which kit sound this lane plays.
    Voice,
    Mute,
    Solo,
    // ── The pattern ──
    Length,
    Rate,
    Swing,
    /// What a newly written step's gate starts at.
    DefaultGate,
    BaseVelocity,
    AccentVelocity,
    Mode,
    Tonic,
    /// When a queued switch happens.
    Switch,
    /// Which instrument the sequencer drives.
    Child,
}

impl SeqKnob {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pitch => "pitch",
            Self::Chord => "chord",
            Self::Voicing => "voicing",
            Self::RootBelow => "root\u{2193}",
            Self::Gate | Self::DefaultGate => "gate",
            Self::Voice => "sound",
            Self::Mute => "mute",
            Self::Solo => "solo",
            Self::Length => "steps",
            Self::Rate => "rate",
            Self::Swing => "swing",
            Self::BaseVelocity => "base",
            Self::AccentVelocity => "accent",
            Self::Mode => "mode",
            Self::Tonic => "key",
            Self::Switch => "switch",
            Self::Child => "child",
        }
    }
}

/// The step grid's cursor: which band, which control inside it, and whether
/// that control has been locked with Enter.
///
/// Nothing here is part of a pattern. It is the same separation the piano
/// roll keeps — [`PianoRollState`] holds a column and a focus level, the clip
/// holds the notes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SequencerView {
    pub band: SeqBand,
    /// Which control inside [`SequencerView::band`] the cursor is on.
    pub knob: usize,
    /// Enter was pressed on a control: `h`/`l` now adjust it and nothing else
    /// gets a look at the key. The fader's contract, applied to a knob.
    pub locked: bool,
    /// The slot `y` picked up, for `p` to paste.
    pub copy_from: Option<u8>,
    /// Digits typed towards a step or slot number.
    pub digits: String,
}

impl SequencerView {
    pub fn new() -> Self {
        Self::default()
    }

    /// Move to another band, giving up the lock and the digit buffer with it.
    pub fn move_band(&mut self, delta: i32) {
        if self.locked {
            return;
        }
        let next = self.band.stepped(delta);
        if next != self.band {
            self.band = next;
            self.knob = 0;
            self.digits.clear();
        }
    }

    /// Put the cursor on a band directly, as opening the view does.
    pub fn focus_band(&mut self, band: SeqBand) {
        self.band = band;
        self.knob = 0;
        self.locked = false;
        self.digits.clear();
    }

    /// Move the cursor between the controls of the current band. Clamped:
    /// walking off the end of a knob row and reappearing at the other end is
    /// how a value gets changed by accident.
    pub fn move_knob(&mut self, delta: i32, count: usize) {
        if count == 0 {
            self.knob = 0;
            return;
        }
        self.knob = (self.knob as i32 + delta).clamp(0, count as i32 - 1) as usize;
    }

    /// Type a digit towards a number in `1..=max`, and say which one was
    /// named once it can no longer grow.
    ///
    /// The piano roll's rule, because a step grid has the same problem: `1`
    /// on a 16-step pattern might be step 1 or the front of step 12.
    pub fn type_digit(&mut self, ch: char, max: usize) -> Option<usize> {
        self.digits.push(ch);
        let Ok(number) = self.digits.parse::<usize>() else {
            self.digits.clear();
            return None;
        };
        if number == 0 || number > max {
            self.digits.clear();
            return None;
        }
        if number * 10 > max || self.digits.len() >= 2 {
            self.digits.clear();
            return Some(number);
        }
        None
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
    /// Total beats in the clip (e.g. 4 for a 1-bar clip).
    pub total_beats: usize,
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
    /// Whether highlights are locked for stretching (Enter while highlights exist).
    pub highlight_locked: bool,
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
    // ── Automation lane ──
    /// Whether the controller lane is showing under the note grid.
    pub automation_open: bool,
    /// Whether the lane has the keys: j/k draw values at the column cursor,
    /// h/l walk columns, rather than the note grid taking them.
    pub automation_focus: bool,
    /// Which of the clip's controller streams the lane shows, as an index
    /// into [`Clip::control_streams`]. Clamped to what the clip offers when
    /// the lane is drawn.
    pub automation_lane: usize,
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
            total_beats: 4,
            selected_note_indices: Vec::new(),
            column_digits: String::new(),
            highlight_start: None,
            highlight_end: None,
            visible_columns: 16,
            row_highlight_low: None,
            row_highlight_high: None,
            yank_buffer: Vec::new(),
            yank_columns: 0,
            highlight_locked: false,
            edit_mode: false,
            edit_cursor: 0,
            edit_selected: Vec::new(),
            edit_sub: EditSubMode::Navigate,
            grid: GridResolution::Eighth,
            snap_enabled: true,
            default_velocity: 100,
            settings_cursor: 0,
            automation_open: false,
            automation_focus: false,
            automation_lane: 0,
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
        self.highlight_locked = false;
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

    /// Recalculate column_count from total_beats and grid resolution.
    pub fn update_column_count(&mut self) {
        let cols = (self.total_beats as f64 * self.grid.subdivisions_per_beat()).round() as usize;
        self.column_count = cols.max(1);
        if self.column >= self.column_count {
            self.column = self.column_count.saturating_sub(1);
        }
    }

    /// Returns true if any column or row highlights are active.
    pub fn has_highlights(&self) -> bool {
        self.highlight_start.is_some() || self.row_highlight_low.is_some()
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
