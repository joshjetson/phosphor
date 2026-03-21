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

pub use clip_view::*;
pub use input::*;
pub use loop_editor::*;
pub use menu::*;
pub use track::*;

use phosphor_core::project::TrackKind;

// ── Panes ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Tracks,
    ClipView, // combined: FX panel (left) + piano roll/clip (right)
}

impl Pane {
    pub fn number(self) -> u8 {
        match self {
            Self::Tracks => 1,
            Self::ClipView => 2,
        }
    }

    pub fn from_number(n: u8) -> Option<Self> {
        match n {
            1 => Some(Self::Tracks),
            2 => Some(Self::ClipView),
            _ => None,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Tracks => Self::ClipView,
            Self::ClipView => Self::Tracks,
        }
    }

    pub fn prev(self) -> Self {
        self.next()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Tracks => "tracks",
            Self::ClipView => "clip",
        }
    }
}

// ── Full Nav State ──

pub const MAX_VISIBLE_TRACKS: usize = 5;

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
    pub tracks: Vec<TrackState>,
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
            tracks,
        }
    }

    // ── Space menu ──

    /// Toggle the space menu open/closed.
    pub fn toggle_space_menu(&mut self) {
        self.space_menu.toggle();
    }

    /// Handle a key press while the space menu is open.
    /// Returns a SpaceAction if an action should be performed.
    pub fn space_menu_handle(&mut self, ch: char) -> Option<SpaceAction> {
        self.space_menu.open = false;
        match ch {
            '1' => { self.focus_pane(Pane::Tracks); None }
            '2' => { self.focus_pane(Pane::ClipView); None }
            'p' => Some(SpaceAction::PlayPause),
            'r' => Some(SpaceAction::ToggleRecord),
            'l' => Some(SpaceAction::ToggleLoop),
            '!' => Some(SpaceAction::Panic),
            'a' => Some(SpaceAction::AddInstrument),
            's' => Some(SpaceAction::Save),
            'n' => Some(SpaceAction::NewTrack),
            'h' => {
                self.space_menu.open = true;
                self.space_menu.section = SpaceMenuSection::Help;
                self.space_menu.cursor = 0;
                None
            }
            _ => None,
        }
    }

    // ── Pane focus ──

    pub fn focus_pane(&mut self, pane: Pane) {
        if self.focused_pane == Pane::Tracks { self.track_selected = false; }
        self.focused_pane = pane;
    }

    pub fn focus_next_pane(&mut self) {
        self.focus_pane(self.focused_pane.next());
    }

    // ── Navigation ──

    pub fn move_up(&mut self) {
        if self.instrument_modal.open { self.instrument_modal.move_up(); return; }
        if self.space_menu.open { self.space_menu.move_up(); return; }
        if self.fx_menu.open { self.fx_menu.move_up(); return; }
        match self.focused_pane {
            Pane::Tracks => {
                if self.track_cursor > 0 {
                    self.track_cursor -= 1;
                    if self.track_cursor < self.track_scroll {
                        self.track_scroll = self.track_cursor;
                    }
                    if self.track_selected { self.show_current_track_controls(); }
                }
            }
            Pane::ClipView => {
                match self.clip_view.focus {
                    ClipViewFocus::PianoRoll => self.clip_view.piano_roll.move_up(),
                    ClipViewFocus::FxPanel => {
                        if self.clip_view.fx_panel_tab == FxPanelTab::Synth {
                            if self.clip_view.synth_param_cursor > 0 {
                                self.clip_view.synth_param_cursor -= 1;
                            }
                        } else if self.clip_view.fx_cursor > 0 {
                            self.clip_view.fx_cursor -= 1;
                        }
                    }
                }
            }
        }
    }

    pub fn move_down(&mut self) {
        if self.instrument_modal.open { self.instrument_modal.move_down(); return; }
        if self.space_menu.open { self.space_menu.move_down(); return; }
        if self.fx_menu.open { self.fx_menu.move_down(); return; }
        match self.focused_pane {
            Pane::Tracks => {
                if self.track_cursor + 1 < self.tracks.len() {
                    self.track_cursor += 1;
                    if self.track_cursor >= self.track_scroll + MAX_VISIBLE_TRACKS {
                        self.track_scroll = self.track_cursor + 1 - MAX_VISIBLE_TRACKS;
                    }
                    if self.track_selected { self.show_current_track_controls(); }
                }
            }
            Pane::ClipView => {
                match self.clip_view.focus {
                    ClipViewFocus::PianoRoll => self.clip_view.piano_roll.move_down(),
                    ClipViewFocus::FxPanel => {
                        if self.clip_view.fx_panel_tab == FxPanelTab::Synth {
                            let max = self.current_track().map(|t| t.synth_params.len()).unwrap_or(0);
                            if self.clip_view.synth_param_cursor + 1 < max {
                                self.clip_view.synth_param_cursor += 1;
                            }
                        } else {
                            let max = self.active_fx_chain_len();
                            if self.clip_view.fx_cursor + 1 < max {
                                self.clip_view.fx_cursor += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn move_left(&mut self) {
        if self.focused_pane == Pane::Tracks && self.track_selected {
            self.track_element = self.track_element.move_left();
        } else if self.focused_pane == Pane::ClipView {
            match self.clip_view.focus {
                ClipViewFocus::PianoRoll => {
                    self.clip_view.focus = ClipViewFocus::FxPanel;
                }
                ClipViewFocus::FxPanel if self.clip_view.fx_panel_tab == FxPanelTab::Synth => {
                    // h = decrease parameter value
                    self.adjust_synth_param(-0.05);
                }
                _ => {}
            }
        }
    }

    pub fn move_right(&mut self) {
        if self.focused_pane == Pane::Tracks && self.track_selected {
            let num_clips = self.current_track().map(|t| t.clips.len()).unwrap_or(0);
            self.track_element = self.track_element.move_right(num_clips);
        } else if self.focused_pane == Pane::ClipView {
            match self.clip_view.focus {
                ClipViewFocus::FxPanel if self.clip_view.fx_panel_tab == FxPanelTab::Synth => {
                    // l = increase parameter value
                    self.adjust_synth_param(0.05);
                }
                ClipViewFocus::FxPanel => {
                    self.clip_view.focus = ClipViewFocus::PianoRoll;
                }
                _ => {}
            }
        }
    }

    /// Adjust the currently selected synth parameter by delta.
    /// Returns the (mixer_id, param_index, new_value) if changed, for sending to audio.
    pub fn adjust_synth_param(&mut self, delta: f32) -> Option<(usize, usize, f32)> {
        let idx = self.clip_view.synth_param_cursor;
        if let Some(track) = self.tracks.get_mut(self.track_cursor) {
            if idx < track.synth_params.len() {
                let new_val = (track.synth_params[idx] + delta).clamp(0.0, 1.0);
                track.synth_params[idx] = new_val;
                if let Some(mixer_id) = track.mixer_id {
                    return Some((mixer_id, idx, new_val));
                }
            }
        }
        None
    }

    pub fn enter(&mut self) -> Option<SpaceAction> {
        // Space menu open → select item via space_menu_handle using the key from cursor position
        if self.space_menu.open {
            match self.space_menu.section {
                SpaceMenuSection::Actions => {
                    if let Some((key, _, _)) = SPACE_ACTIONS.get(self.space_menu.cursor) {
                        // Extract the char after "spc+"
                        if let Some(ch) = key.strip_prefix("spc+").and_then(|s| s.chars().next()) {
                            return self.space_menu_handle(ch);
                        }
                    }
                    self.space_menu.open = false;
                    return None;
                }
                SpaceMenuSection::Help => {
                    // Help topics just show info — no action
                    return None;
                }
            }
        }
        // FX menu open → select item
        if self.fx_menu.open {
            self.fx_menu_select();
            return None;
        }

        match self.focused_pane {
            Pane::Tracks => {
                if !self.track_selected {
                    self.track_selected = true;
                    self.track_element = TrackElement::Label;
                    // If this is a live instrument track, show its synth controls
                    self.show_current_track_controls();
                } else {
                    self.activate_element();
                }
            }
            Pane::ClipView => {}
        }
        None
    }

    pub fn escape(&mut self) {
        if self.instrument_modal.open {
            self.instrument_modal.open = false;
            return;
        }
        if self.space_menu.open {
            self.space_menu.open = false;
            return;
        }
        if self.fx_menu.open {
            self.fx_menu.open = false;
            return;
        }
        match self.focused_pane {
            Pane::Tracks => {
                if self.track_selected {
                    self.track_selected = false;
                    self.track_element = TrackElement::Label;
                    self.clip_view_visible = false;
                    self.clip_view_target = None;
                }
            }
            Pane::ClipView => self.focus_pane(Pane::Tracks),
        }
    }

    /// Cycle tabs in the clip view (FX panel or piano roll side).
    pub fn cycle_tab(&mut self) {
        if self.focused_pane == Pane::ClipView {
            match self.clip_view.focus {
                ClipViewFocus::FxPanel => {
                    self.clip_view.fx_panel_tab = self.clip_view.fx_panel_tab.next();
                    self.clip_view.fx_cursor = 0;
                }
                ClipViewFocus::PianoRoll => {
                    self.clip_view.clip_tab = self.clip_view.clip_tab.next();
                }
            }
        }
    }

    pub fn toggle_mute(&mut self) {
        if let Some(t) = self.current_track_mut() {
            t.muted = !t.muted;
            t.sync_to_audio();
        }
    }

    pub fn toggle_solo(&mut self) {
        if let Some(t) = self.current_track_mut() {
            t.soloed = !t.soloed;
            t.sync_to_audio();
        }
    }

    pub fn toggle_arm(&mut self) {
        if let Some(t) = self.current_track_mut() {
            t.armed = !t.armed;
            t.sync_to_audio();
        }
    }

    pub fn digit_input(&mut self, ch: char) {
        if self.focused_pane == Pane::Tracks && self.track_selected {
            self.number_buf.push_digit(ch);
        }
    }

    pub fn tick(&mut self) {
        if let Some(clip_num) = self.number_buf.check_timeout() {
            self.jump_to_clip(clip_num);
        }
    }

    pub fn jump_to_clip(&mut self, clip_number: usize) {
        if let Some(track) = self.current_track() {
            if let Some(idx) = track.clips.iter().position(|c| c.number == clip_number) {
                self.track_element = TrackElement::Clip(idx);
                self.open_clip_view(self.track_cursor, idx);
            }
        }
    }

    fn activate_element(&mut self) {
        match self.track_element {
            TrackElement::Mute => self.toggle_mute(),
            TrackElement::Solo => self.toggle_solo(),
            TrackElement::RecordArm => self.toggle_arm(),
            TrackElement::Fx => {
                self.fx_menu.open = true;
                self.fx_menu.cursor = 0;
            }
            TrackElement::Volume => { /* future: volume slider */ }
            TrackElement::Clip(idx) => {
                self.open_clip_view(self.track_cursor, idx);
            }
            _ => {}
        }
    }

    /// Add a new instrument track. Inserts before the send/master tracks.
    /// `handle` is the shared audio-thread handle for this track.
    /// `mixer_id` is the track's ID in the mixer.
    pub fn add_instrument_track(
        &mut self,
        instrument: InstrumentType,
        mixer_id: usize,
        handle: std::sync::Arc<phosphor_core::project::TrackHandle>,
    ) {
        let name = match instrument {
            InstrumentType::Synth => "synth",
            InstrumentType::DrumRack => "drums",
            InstrumentType::Sampler => "smplr",
        };

        // Find insert position: before sends/master
        let insert_pos = self.tracks.iter().position(|t| {
            matches!(t.kind, TrackKind::SendA | TrackKind::SendB | TrackKind::Master)
        }).unwrap_or(self.tracks.len());

        let color = insert_pos % 8;
        let mut track = TrackState::new(name, color, true, TrackKind::Instrument, vec![]);
        track.mixer_id = Some(mixer_id);
        track.handle = Some(handle);
        track.synth_params = phosphor_dsp::synth::PARAM_DEFAULTS.to_vec();
        // Sync the initial armed state to audio
        track.sync_to_audio();
        self.tracks.insert(insert_pos, track);

        // Move cursor to the new track and open clip view with synth controls
        self.track_cursor = insert_pos;
        if self.track_cursor >= self.track_scroll + MAX_VISIBLE_TRACKS {
            self.track_scroll = self.track_cursor + 1 - MAX_VISIBLE_TRACKS;
        }

        // Select the track, show synth controls, and route MIDI to it
        self.track_selected = true;
        self.track_element = TrackElement::Label;
        self.show_current_track_controls();
    }

    fn open_clip_view(&mut self, track_idx: usize, clip_idx: usize) {
        self.clip_view_visible = true;
        self.clip_view_target = Some((track_idx, clip_idx));
        self.clip_view.fx_cursor = 0;
    }

    /// Show controls for the currently selected track and route MIDI to it.
    /// For instrument tracks: opens clip view with Synth tab, activates MIDI input.
    /// For bus tracks: no clip view, deactivates MIDI.
    pub fn show_current_track_controls(&mut self) {
        // Deactivate MIDI on ALL tracks first
        for track in &self.tracks {
            if let Some(ref h) = track.handle {
                h.config.midi_active.store(false, std::sync::atomic::Ordering::Relaxed);
            }
        }

        if let Some(track) = self.tracks.get(self.track_cursor) {
            if track.is_live() {
                // Activate MIDI on this track
                if let Some(ref h) = track.handle {
                    h.config.midi_active.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                self.clip_view_visible = true;
                self.clip_view_target = Some((self.track_cursor, 0));
                self.clip_view.fx_panel_tab = FxPanelTab::Synth;
                self.clip_view.focus = ClipViewFocus::FxPanel;
                self.clip_view.synth_param_cursor = 0;
            } else {
                // Bus track — hide clip view
                self.clip_view_visible = false;
                self.clip_view_target = None;
            }
        }
    }

    fn fx_menu_select(&mut self) {
        // Add FX
        if let Some(fx_type) = FxType::ALL.get(self.fx_menu.cursor) {
            let inst = FxInstance::new(*fx_type);
            if let Some(t) = self.current_track_mut() {
                t.fx_chain.push(inst);
            }
        }
        self.fx_menu.open = false;
    }

    fn active_fx_chain_len(&self) -> usize {
        match self.clip_view.fx_panel_tab {
            FxPanelTab::TrackFx | FxPanelTab::Synth => {
                self.current_track().map(|t| t.fx_chain.len().max(1)).unwrap_or(1)
            }
        }
    }

    // ── Accessors ──

    pub fn visible_tracks(&self) -> &[TrackState] {
        let end = (self.track_scroll + MAX_VISIBLE_TRACKS).min(self.tracks.len());
        &self.tracks[self.track_scroll..end]
    }

    pub fn can_scroll_up(&self) -> bool { self.track_scroll > 0 }

    pub fn can_scroll_down(&self) -> bool {
        self.track_scroll + MAX_VISIBLE_TRACKS < self.tracks.len()
    }

    pub fn current_track(&self) -> Option<&TrackState> { self.tracks.get(self.track_cursor) }

    fn current_track_mut(&mut self) -> Option<&mut TrackState> {
        self.tracks.get_mut(self.track_cursor)
    }

    pub fn active_clip(&self) -> Option<&Clip> {
        let (ti, ci) = self.clip_view_target?;
        self.tracks.get(ti)?.clips.get(ci)
    }

    pub fn active_clip_track(&self) -> Option<&TrackState> {
        let (ti, _) = self.clip_view_target?;
        self.tracks.get(ti)
    }

    /// Receive a clip snapshot from the audio thread and add it to the
    /// corresponding TUI track's clip list.
    pub fn receive_clip_snapshot(&mut self, snap: phosphor_core::clip::ClipSnapshot) {
        // Find the TUI track with matching mixer_id
        if let Some(track) = self.tracks.iter_mut().find(|t| t.mixer_id == Some(snap.track_id)) {
            let ppq = phosphor_core::transport::Transport::PPQ;
            // Width in cells: roughly 1 cell per beat (PPQ ticks)
            let beats = (snap.length_ticks as f64 / ppq as f64).ceil() as u16;
            let width = beats.max(2); // minimum 2 cells wide

            let clip_number = track.clips.len() + 1;
            track.clips.push(Clip {
                number: clip_number,
                width,
                has_content: true,
                start_tick: snap.start_tick,
                length_ticks: snap.length_ticks,
                notes: snap.notes,
            });
        }
    }
}

// ── Initial Data ──

/// Initial tracks: just the bus tracks. Instruments are added by the user via Space+A.
pub fn initial_tracks() -> Vec<TrackState> {
    vec![
        TrackState::new("snd a", 5, false, TrackKind::SendA, vec![]),
        TrackState::new("snd b", 6, false, TrackKind::SendB, vec![]),
        TrackState::new("mstr", 7, false, TrackKind::Master, vec![]),
    ]
}


// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_numbers() {
        assert_eq!(Pane::Tracks.number(), 1);
        assert_eq!(Pane::ClipView.number(), 2);
        assert_eq!(Pane::from_number(1), Some(Pane::Tracks));
        assert_eq!(Pane::from_number(2), Some(Pane::ClipView));
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

        assert_eq!(nav.clip_view.fx_panel_tab, FxPanelTab::TrackFx);
        nav.cycle_tab();
        assert_eq!(nav.clip_view.fx_panel_tab, FxPanelTab::Synth);
        nav.cycle_tab();
        assert_eq!(nav.clip_view.fx_panel_tab, FxPanelTab::TrackFx);

        nav.clip_view.focus = ClipViewFocus::PianoRoll;
        assert_eq!(nav.clip_view.clip_tab, ClipTab::PianoRoll);
        nav.cycle_tab();
        assert_eq!(nav.clip_view.clip_tab, ClipTab::Automation);
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
        assert_eq!(nav.focused_pane, Pane::ClipView);
        assert!(action.is_none()); // pane jump, no transport action
        assert!(!nav.space_menu.open);
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
