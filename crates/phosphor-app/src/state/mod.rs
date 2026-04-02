//! TUI navigation state — focus, cursors, selection, leader keys, FX.
//!
//! Navigation:
//!   Space+N  → jump to component (1=Tracks, 2=ClipView)
//!   Tab      → cycle focus between components
//!   j/k      → vertical nav
//!   h/l      → horizontal nav
//!   Enter    → select / activate / open menus
//!   Esc      → back out one level

mod clip_view;
mod input;
mod loop_editor;
mod menu;
mod track;
mod transport_ui;
pub mod undo;

pub use clip_view::*;
pub use input::*;
pub use loop_editor::*;
pub use menu::*;
pub use track::*;
pub use transport_ui::*;
mod navigation;
mod params;
mod track_ops;
pub use track_ops::initial_tracks;

use phosphor_core::project::TrackKind;

// ── Panes ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Transport,
    Tracks,
    ClipView,
}

impl Pane {
    pub fn number(self) -> u8 {
        match self {
            Self::Transport => 1,
            Self::Tracks => 2,
            Self::ClipView => 3,
        }
    }

    pub fn from_number(n: u8) -> Option<Self> {
        match n {
            1 => Some(Self::Transport),
            2 => Some(Self::Tracks),
            3 => Some(Self::ClipView),
            _ => None,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Transport => Self::Tracks,
            Self::Tracks => Self::ClipView,
            Self::ClipView => Self::Transport,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Transport => Self::ClipView,
            Self::Tracks => Self::Transport,
            Self::ClipView => Self::Tracks,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Transport => "transport",
            Self::Tracks => "tracks",
            Self::ClipView => "clip",
        }
    }
}

// ── Full Nav State ──

pub const MAX_VISIBLE_TRACKS: usize = 5;
/// Total number of parameters in the inst config panel (LFO:4 + Filter:4 + Envelope:4 + Pitch:3).
pub const INST_CONFIG_PARAM_COUNT: usize = 15;

#[derive(Debug)]
pub struct NavState {
    pub focused_pane: Pane,
    pub track_cursor: usize,
    pub track_scroll: usize,
    pub track_selected: bool,
    pub track_element: TrackElement,
    pub number_buf: NumberBuffer,
    pub space_menu: SpaceMenu,
    pub clip_view: ClipViewState,
    pub clip_view_visible: bool,
    /// (track_idx, clip_idx) shown in clip view.
    pub clip_view_target: Option<(usize, usize)>,
    /// FX menu state (per-track fx button).
    pub fx_menu: FxMenu,
    pub instrument_modal: InstrumentModal,
    pub loop_editor: LoopEditor,
    pub transport_ui: TransportUiState,
    pub tracks: Vec<TrackState>,
    /// Text input modal (for save/open file paths).
    pub input_modal: InputModal,
    /// Confirmation modal (for delete actions).
    pub confirm_modal: ConfirmModal,
    /// Undo/redo stack.
    pub undo_stack: undo::UndoStack,
    /// Whether a clip is "locked" for editing (Enter locks, Esc unlocks).
    /// When locked, h/l moves the clip instead of navigating between elements.
    pub clip_locked: bool,
    /// Grace counter: set to the number of armed tracks when recording stops.
    /// Decremented as each valid snapshot is accepted. Prevents stale snapshots
    /// while allowing final recording commits from all tracks to come through.
    pub recording_grace: usize,
}

impl NavState {
    pub fn new(tracks: Vec<TrackState>) -> Self {
        Self {
            focused_pane: Pane::Tracks,
            track_cursor: 0,
            track_scroll: 0,
            track_selected: false,
            track_element: TrackElement::Label,
            number_buf: NumberBuffer::new(),
            space_menu: SpaceMenu::new(),
            clip_view: ClipViewState::new(),
            clip_view_visible: false,
            clip_view_target: None,
            fx_menu: FxMenu::new(),
            instrument_modal: InstrumentModal::new(),
            loop_editor: LoopEditor::new(),
            transport_ui: TransportUiState::new(),
            tracks,
            input_modal: InputModal::new(),
            confirm_modal: ConfirmModal::new(),
            undo_stack: undo::UndoStack::new(),
            clip_locked: false,
            recording_grace: 0,
        }
    }
    pub fn visible_tracks(&self) -> &[TrackState] {
        let end = (self.track_scroll + MAX_VISIBLE_TRACKS).min(self.tracks.len());
        &self.tracks[self.track_scroll..end]
    }

    pub fn can_scroll_up(&self) -> bool { self.track_scroll > 0 }

    pub fn can_scroll_down(&self) -> bool {
        self.track_scroll + MAX_VISIBLE_TRACKS < self.tracks.len()
    }

    pub fn current_track(&self) -> Option<&TrackState> { self.tracks.get(self.track_cursor) }

    pub fn current_track_mut(&mut self) -> Option<&mut TrackState> {
        self.tracks.get_mut(self.track_cursor)
    }

    pub fn active_clip(&self) -> Option<&Clip> {
        let (ti, ci) = self.clip_view_target?;
        self.tracks.get(ti)?.clips.get(ci)
    }

    pub fn active_clip_mut(&mut self) -> Option<&mut Clip> {
        let (ti, ci) = self.clip_view_target?;
        self.tracks.get_mut(ti)?.clips.get_mut(ci)
    }

    pub fn active_clip_track(&self) -> Option<&TrackState> {
        let (ti, _) = self.clip_view_target?;
        self.tracks.get(ti)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_numbers() {
        assert_eq!(Pane::Transport.number(), 1);
        assert_eq!(Pane::Tracks.number(), 2);
        assert_eq!(Pane::ClipView.number(), 3);
        assert_eq!(Pane::from_number(1), Some(Pane::Transport));
        assert_eq!(Pane::from_number(2), Some(Pane::Tracks));
        assert_eq!(Pane::from_number(3), Some(Pane::ClipView));
        assert_eq!(Pane::from_number(9), None);
    }

    #[test]
    fn track_element_navigation_full() {
        let e = TrackElement::Label;
        assert_eq!(e.move_right(3), TrackElement::Fx);
        assert_eq!(TrackElement::Fx.move_right(3), TrackElement::Volume);
        assert_eq!(TrackElement::Volume.move_right(3), TrackElement::Mute);
        assert_eq!(TrackElement::Mute.move_right(3), TrackElement::Solo);
        assert_eq!(TrackElement::Solo.move_right(3), TrackElement::RecordArm);
        assert_eq!(TrackElement::RecordArm.move_right(3), TrackElement::Clip(0));
        assert_eq!(TrackElement::Clip(2).move_right(3), TrackElement::Clip(2));
    }

    #[test]
    fn track_element_left_full() {
        assert_eq!(TrackElement::Clip(0).move_left(), TrackElement::RecordArm);
        assert_eq!(TrackElement::RecordArm.move_left(), TrackElement::Solo);
        assert_eq!(TrackElement::Solo.move_left(), TrackElement::Mute);
        assert_eq!(TrackElement::Mute.move_left(), TrackElement::Volume);
        assert_eq!(TrackElement::Volume.move_left(), TrackElement::Fx);
        assert_eq!(TrackElement::Fx.move_left(), TrackElement::Label);
        assert_eq!(TrackElement::Label.move_left(), TrackElement::Label);
    }

    #[test]
    fn initial_tracks_has_sends_and_master() {
        let tracks = initial_tracks();
        assert_eq!(tracks.len(), 3); // send A + send B + master
        assert_eq!(tracks[0].kind, TrackKind::SendA);
        assert_eq!(tracks[1].kind, TrackKind::SendB);
        assert_eq!(tracks[2].kind, TrackKind::Master);
    }

    #[test]
    fn sends_are_at_end() {
        let mut nav = NavState::new(initial_tracks());
        nav.move_down();
        nav.move_down();
        assert_eq!(nav.track_cursor, 2);
        assert_eq!(nav.tracks[nav.track_cursor].kind, TrackKind::Master);
    }

    #[test]
    fn fx_menu_opens_and_closes() {
        let mut nav = NavState::new(initial_tracks());
        nav.enter(); // select track
        // Navigate to FX
        nav.move_right(); // -> Fx
        assert_eq!(nav.track_element, TrackElement::Fx);
        nav.enter(); // open FX menu
        assert!(nav.fx_menu.open);

        nav.escape(); // close menu
        assert!(!nav.fx_menu.open);
    }

    #[test]
    fn fx_menu_add_effect() {
        let mut nav = NavState::new(initial_tracks());
        let initial_count = nav.tracks[0].fx_chain.len();
        nav.enter();
        nav.move_right(); // -> Fx
        nav.enter(); // open menu
        nav.enter(); // select first item (Reverb)
        assert!(!nav.fx_menu.open);
        assert_eq!(nav.tracks[0].fx_chain.len(), initial_count + 1);
        assert_eq!(nav.tracks[0].fx_chain.last().unwrap().fx_type, FxType::Reverb);
    }

    #[test]
    fn clip_view_focus_toggle() {
        let mut nav = NavState::new(initial_tracks());
        // Manually set up clip view (simulating an instrument track being selected)
        nav.clip_view_visible = true;
        nav.clip_view_target = Some((0, 0));

        nav.focus_pane(Pane::ClipView);
        assert_eq!(nav.clip_view.focus, ClipViewFocus::PianoRoll);

        nav.move_left(); // -> FxPanel
        assert_eq!(nav.clip_view.focus, ClipViewFocus::FxPanel);
    }

    #[test]
    fn clip_view_tabs_cycle() {
        let mut nav = NavState::new(initial_tracks());
        nav.focused_pane = Pane::ClipView;
        nav.clip_view.focus = ClipViewFocus::FxPanel;

        // Tab cycles: trk fx → synth → inst config → piano → auto → trk fx
        assert_eq!(nav.clip_view.fx_panel_tab, FxPanelTab::TrackFx);
        nav.cycle_tab();
        assert_eq!(nav.clip_view.fx_panel_tab, FxPanelTab::Synth);
        nav.cycle_tab();
        // Now switches to inst config
        assert_eq!(nav.clip_view.focus, ClipViewFocus::PianoRoll);
        assert_eq!(nav.clip_view.clip_tab, ClipTab::InstConfig);
        nav.cycle_tab();
        // Now switches to piano roll
        assert_eq!(nav.clip_view.clip_tab, ClipTab::PianoRoll);
        nav.cycle_tab();
        assert_eq!(nav.clip_view.clip_tab, ClipTab::Settings);
        nav.cycle_tab();
        // Back to FX panel
        assert_eq!(nav.clip_view.focus, ClipViewFocus::FxPanel);
        assert_eq!(nav.clip_view.fx_panel_tab, FxPanelTab::TrackFx);
    }

    #[test]
    fn arm_toggle() {
        let mut nav = NavState::new(initial_tracks());
        assert!(!nav.tracks[0].armed); // bus tracks start unarmed
        nav.toggle_arm();
        assert!(nav.tracks[0].armed);
        nav.toggle_arm();
        assert!(!nav.tracks[0].armed);
    }

    #[test]
    fn space_menu_toggle() {
        let mut nav = NavState::new(initial_tracks());
        assert!(!nav.space_menu.open);
        nav.toggle_space_menu();
        assert!(nav.space_menu.open);
        nav.toggle_space_menu();
        assert!(!nav.space_menu.open);
    }

    #[test]
    fn space_menu_handle_pane_jump() {
        let mut nav = NavState::new(initial_tracks());
        nav.toggle_space_menu();
        let action = nav.space_menu_handle('2');
        assert_eq!(nav.focused_pane, Pane::Tracks);
        assert!(action.is_none());
        assert!(!nav.space_menu.open);

        nav.toggle_space_menu();
        let action = nav.space_menu_handle('1');
        assert_eq!(nav.focused_pane, Pane::Transport);
        assert!(action.is_none());
    }

    #[test]
    fn space_menu_handle_play_pause() {
        let mut nav = NavState::new(initial_tracks());
        nav.toggle_space_menu();
        let action = nav.space_menu_handle('p');
        assert_eq!(action, Some(SpaceAction::PlayPause));
        assert!(!nav.space_menu.open);
    }

    #[test]
    fn space_menu_enter_select() {
        let mut nav = NavState::new(initial_tracks());
        nav.toggle_space_menu();
        // cursor at 0 = "spc+1" = tracks
        let action = nav.enter();
        assert!(action.is_none()); // pane jump
        assert!(!nav.space_menu.open);
    }

    #[test]
    fn space_menu_nav_and_help() {
        let mut nav = NavState::new(initial_tracks());
        nav.toggle_space_menu();
        assert_eq!(nav.space_menu.section, SpaceMenuSection::Actions);
        nav.space_menu.switch_section();
        assert_eq!(nav.space_menu.section, SpaceMenuSection::Help);
        assert_eq!(nav.space_menu.cursor, 0);
    }

    #[test]
    fn number_buffer_commit() {
        let mut buf = NumberBuffer::new();
        buf.push_digit('1');
        assert_eq!(buf.commit(), Some(1));
        buf.push_digit('1');
        buf.push_digit('2');
        assert_eq!(buf.commit(), Some(12));
    }

    #[test]
    fn number_buffer_empty_commit() {
        assert_eq!(NumberBuffer::new().commit(), None);
    }

    #[test]
    fn nav_cursor_bounds() {
        let mut nav = NavState::new(initial_tracks());
        for _ in 0..20 { nav.move_down(); }
        assert_eq!(nav.track_cursor, 2); // 3 bus tracks
    }

    #[test]
    fn enter_escape_track() {
        let mut nav = NavState::new(initial_tracks());
        nav.enter();
        assert!(nav.track_selected);
        nav.escape();
        assert!(!nav.track_selected);
    }

    #[test]
    fn mute_solo_toggle() {
        let mut nav = NavState::new(initial_tracks());
        nav.toggle_mute();
        assert!(nav.tracks[0].muted);
        nav.toggle_solo();
        assert!(nav.tracks[0].soloed);
    }

    #[test]
    fn volume_element_in_chain() {
        // Ensure volume is navigable
        let e = TrackElement::Fx;
        assert_eq!(e.move_right(1), TrackElement::Volume);
        assert_eq!(TrackElement::Volume.move_left(), TrackElement::Fx);
    }
}
