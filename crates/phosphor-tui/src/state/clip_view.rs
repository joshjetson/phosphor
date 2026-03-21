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

#[derive(Debug)]
pub struct PianoRollState {
    pub cursor_note: u8,
    pub scroll_x: usize,
    pub view_bottom_note: u8,
    pub view_height: u8,
}

impl Default for PianoRollState {
    fn default() -> Self { Self::new() }
}

impl PianoRollState {
    pub fn new() -> Self {
        Self { cursor_note: 60, scroll_x: 0, view_bottom_note: 48, view_height: 24 }
    }

    pub fn move_up(&mut self) {
        if self.cursor_note < 127 {
            self.cursor_note += 1;
            // Scroll up if cursor goes above visible area
            let top = self.view_bottom_note.saturating_add(self.view_height);
            if self.cursor_note >= top {
                self.view_bottom_note = self.cursor_note - self.view_height + 1;
            }
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_note > 0 {
            self.cursor_note -= 1;
            // Scroll down if cursor goes below visible area
            if self.cursor_note < self.view_bottom_note {
                self.view_bottom_note = self.cursor_note;
            }
        }
    }

    /// Set the actual visible height (called from the renderer).
    pub fn set_view_height(&mut self, h: u8) {
        self.view_height = h.max(1);
    }
}
