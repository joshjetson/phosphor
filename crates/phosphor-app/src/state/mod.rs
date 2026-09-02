//! TUI navigation state — focus, cursors, selection, leader keys, FX.
//!
//! Navigation:
//!   Space+N  → jump to component (1=Tracks, 2=ClipView)
//!   Tab      → cycle focus between components
//!   j/k      → vertical nav
//!   h/l      → horizontal nav
//!   Enter    → select / activate / open menus
//!   Esc      → back out one level

mod automation;
mod midi_fx;
mod clip_view;
mod input;
mod loop_editor;
mod menu;
mod track;
mod transport_ui;
pub mod undo;

pub use automation::*;
pub use midi_fx::*;
pub use clip_view::*;
pub use input::*;
pub use loop_editor::*;
pub use menu::*;
pub use track::*;
pub use transport_ui::*;
mod navigation;
mod params;
mod track_ops;
pub use track_ops::{initial_tracks, FxAdd};

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
    /// Quantize modal state.
    pub quantize_modal: QuantizeModal,
    /// User preset browser for the track under the cursor.
    pub preset_modal: PresetModal,
    /// Whether the selected track element is "locked" for editing — Enter
    /// locks, Esc releases. While locked, h/l edits that element instead of
    /// navigating between elements, which is the same shape as the
    /// transport's BPM field and the loop editor.
    ///
    /// One flag rather than one per element: `track_element` already says
    /// *which* element the keys go to, so a second flag would only make it
    /// possible to have two things locked at once.
    pub element_locked: bool,
    /// Grace counter: set to the number of armed tracks when recording stops.
    /// Decremented as each valid snapshot is accepted. Prevents stale snapshots
    /// while allowing final recording commits from all tracks to come through.
    pub recording_grace: usize,
    /// Takes committed since this recording started — the "pass 3" the top
    /// of the roll reads out, so the layer stack has a visible depth.
    pub take_count: usize,
    /// What R does to the loop range it is about to record over: `false`
    /// layers onto what is there (overdub, the default), `true` clears the
    /// range first so the take starts clean (re-record).
    pub record_replace: bool,
    /// The viewed clip as the MIDI rack will play it — dim "ghost" notes
    /// behind the real ones, so the roll tells the truth when a chord or
    /// arp device is transforming playback. Recomputed when the clip, the
    /// rack, or the view changes; empty when no device is active.
    pub ghost_notes: Vec<phosphor_core::clip::NoteSnapshot>,
    /// Which (track, clip) the ghosts were rendered for.
    pub ghost_for: Option<(usize, usize)>,
    /// Set by anything that may change what the rack would play; the main
    /// loop re-renders and clears it.
    pub ghost_dirty: bool,
    /// The chord device's progression editor, when it is open.
    pub prog_editor: ProgEditor,
    /// What the master limiter is taking off, ready to draw.
    ///
    /// The audio thread's end of this is in `Mixer`; the ballistics have
    /// already been applied there, because only the audio thread sees every
    /// sample — see `phosphor_core::fx::GrBallistics`. Held here rather than
    /// passed to the renderer because it is exactly the same kind of thing as
    /// a track's `TrackHandle`: a window onto the audio thread that the UI
    /// reads whenever it happens to draw.
    ///
    /// A `NavState` with no mixer behind it — every headless test — reads
    /// zero, which is the truth for a mixer that is not running.
    pub limiter_gr: std::sync::Arc<phosphor_core::fx::GrMeter>,
    /// The rate the engine is running at.
    ///
    /// Here because a panel that draws a filter's response has to design it
    /// at the rate the audio thread designed it at: the same EQ drawn at
    /// 44.1 and rendered at 48 is a different curve, most visibly in the top
    /// octave where every matched design bends. Set once, from the device.
    pub sample_rate: u32,
    /// The transport's tempo, mirrored for the panels that have to *say* what
    /// a setting means rather than only send it.
    ///
    /// The audio thread reads the tempo out of the transport itself, once a
    /// block, and never from here — this is the copy the delay's panel uses to
    /// answer "a dotted eighth is how many milliseconds?", which is a question
    /// only a person asks. Refreshed every frame from the same snapshot the
    /// top bar draws its BPM from, so the two can never disagree.
    pub tempo_bpm: f32,
    /// The track whose sidechain key is being monitored in place of its own
    /// output, by mixer id.
    ///
    /// **One, and the type is what says so.** An `Option` cannot hold two, so
    /// "only one key listen at a time" is not a rule anybody has to remember.
    /// The audio thread keeps the same field and clears it on a transport
    /// stop; this is the mirror the status bar and the track strip blink from.
    /// Never written to a session — it is a monitoring switch, not a setting.
    pub key_listen: Option<usize>,
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
            quantize_modal: QuantizeModal::new(),
            preset_modal: PresetModal::new(),
            element_locked: false,
            recording_grace: 0,
            take_count: 0,
            record_replace: false,
            ghost_notes: Vec::new(),
            ghost_for: None,
            ghost_dirty: true,
            prog_editor: ProgEditor::default(),
            limiter_gr: std::sync::Arc::new(phosphor_core::fx::GrMeter::new()),
            sample_rate: 48_000,
            tempo_bpm: 120.0,
            key_listen: None,
        }
    }

    /// The kind of effect whose panel is open, if one is.
    ///
    /// Here rather than only on the app so that the renderer can ask it too:
    /// the hint bar has to say `jk knob` over a column of knobs and `hl band`
    /// over the EQ's grid, and those are the same key doing two different
    /// things.
    #[must_use]
    pub fn open_fx_type(&self) -> Option<FxType> {
        let slot = self.clip_view.fx.slot?;
        Some(self.current_track()?.fx_chain.get(slot)?.fx_type)
    }

    /// Whether this track is the one whose key is being monitored.
    #[must_use]
    pub fn is_key_listening(&self, track_index: usize) -> bool {
        self.key_listen.is_some()
            && self.tracks.get(track_index).and_then(|t| t.mixer_id) == self.key_listen
    }

    /// The name of the track whose key is being monitored, for the status bar.
    #[must_use]
    pub fn key_listen_track_name(&self) -> Option<&str> {
        let id = self.key_listen?;
        self.tracks
            .iter()
            .find(|t| t.mixer_id == Some(id))
            .map(|t| t.name.as_str())
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
        // The routing cells come between the switches and the clips.
        assert_eq!(TrackElement::RecordArm.move_right(3), TrackElement::Pan);
        assert_eq!(TrackElement::Pan.move_right(3), TrackElement::SendA);
        assert_eq!(TrackElement::SendA.move_right(3), TrackElement::SendB);
        assert_eq!(TrackElement::SendB.move_right(3), TrackElement::Clip(0));
        assert_eq!(TrackElement::Clip(2).move_right(3), TrackElement::Clip(2));
        // ...and a track with no clips stops on the last send rather than
        // walking off the end of the strip.
        assert_eq!(TrackElement::SendB.move_right(0), TrackElement::SendB);
    }

    #[test]
    fn track_element_left_full() {
        assert_eq!(TrackElement::Clip(0).move_left(), TrackElement::SendB);
        assert_eq!(TrackElement::SendB.move_left(), TrackElement::SendA);
        assert_eq!(TrackElement::SendA.move_left(), TrackElement::Pan);
        assert_eq!(TrackElement::Pan.move_left(), TrackElement::RecordArm);
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

    /// The menu used to add a placeholder: an entry in the chain with three
    /// made-up parameters and nothing behind it. Now it either produces a
    /// real effect or says why it did not — a slot that does nothing is worse
    /// than no slot, because the player spends the next minute wondering what
    /// they did wrong.
    #[test]
    fn fx_menu_refuses_an_effect_this_build_cannot_make() {
        let mut nav = NavState::new(initial_tracks());
        let initial_count = nav.tracks[0].fx_chain.len();
        nav.enter();
        nav.move_right(); // -> Fx
        nav.enter(); // open menu
        let outcome = nav.fx_menu_select(); // first item
        assert!(!nav.fx_menu.open);
        match outcome {
            crate::state::FxAdd::NotBuilt(fx_type) => {
                assert_eq!(fx_type, FxType::ALL[0]);
            }
            crate::state::FxAdd::Added { fx_type, slot, .. } => {
                // Once the effect exists this is the branch that runs, and
                // the mirror has to have it in the slot it announced.
                assert_eq!(nav.tracks[0].fx_chain.len(), initial_count + 1);
                assert_eq!(nav.tracks[0].fx_chain[slot].fx_type, fx_type);
                return;
            }
            _ => panic!("the menu neither added an effect nor said why not"),
        }
        assert_eq!(
            nav.tracks[0].fx_chain.len(),
            initial_count,
            "a refused effect still took a slot"
        );
    }

    /// Six slots, and the cap is reported rather than enforced by silence.
    #[test]
    fn fx_menu_reports_a_full_chain() {
        let mut nav = NavState::new(initial_tracks());
        nav.tracks[0].fx_chain = (0..phosphor_core::fx::MAX_FX_SLOTS)
            .map(|_| FxInstance::new(FxType::Eq, vec![]))
            .collect();
        assert!(matches!(nav.add_fx(FxType::Reverb), crate::state::FxAdd::ChainFull));
        assert_eq!(nav.tracks[0].fx_chain.len(), phosphor_core::fx::MAX_FX_SLOTS);
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

    // ── Fader ──

    use phosphor_core::project::{TrackConfig, TrackHandle};

    /// A track wired to an audio-thread handle, so the tests can check the
    /// fader reaches it rather than only the UI mirror.
    fn live_track() -> TrackState {
        let mut t = TrackState::new("t", 0, false, TrackKind::Instrument, vec![]);
        t.handle = Some(std::sync::Arc::new(TrackHandle::new(0, TrackKind::Instrument)));
        t.mixer_id = Some(0);
        t
    }

    fn handle_volume(t: &TrackState) -> f32 {
        t.handle.as_ref().unwrap().config.get_volume()
    }

    /// Every press moves the readout by exactly one dB. This is the property
    /// the dB-stepping exists for: a linear step would round to the same
    /// displayed number several presses in a row.
    #[test]
    fn fader_steps_one_db_per_press() {
        let mut t = live_track();
        // The default is -2.5 dB, off the grid; the first press snaps onto it.
        t.adjust_volume(1);
        let start = t.volume_db().unwrap().round();
        for i in 1..=6 {
            t.adjust_volume(1);
            let db = t.volume_db().unwrap();
            assert!(
                (db - (start + i as f32)).abs() < 0.01,
                "press {i} landed at {db:.3} dB, expected {:.3}",
                start + i as f32
            );
        }
    }

    /// The fader reaches unity exactly, so "no gain change" is a position the
    /// user can actually select rather than one they can only get near.
    #[test]
    fn fader_lands_exactly_on_unity() {
        let mut t = live_track();
        for _ in 0..40 {
            t.adjust_volume(1);
        }
        // At the top; walk back down to 0 dB.
        while t.volume_db().unwrap() > 0.5 {
            t.adjust_volume(-1);
        }
        assert!(
            (t.volume - TrackConfig::UNITY_VOLUME).abs() < 1.0e-3,
            "fader stopped at {} instead of unity",
            t.volume
        );
    }

    /// The travel has ends. Holding `l` cannot push the track past +6 dB, and
    /// holding `h` reaches silence rather than an ever-smaller number.
    #[test]
    fn fader_travel_is_bounded_at_both_ends() {
        let mut t = live_track();
        for _ in 0..200 {
            t.adjust_volume(1);
        }
        assert_eq!(t.volume, TrackConfig::MAX_VOLUME);
        assert_eq!(handle_volume(&t), TrackConfig::MAX_VOLUME);

        for _ in 0..200 {
            t.adjust_volume(-1);
        }
        assert_eq!(t.volume, TrackConfig::MIN_VOLUME);
        assert_eq!(handle_volume(&t), TrackConfig::MIN_VOLUME);

        // And it comes back off the bottom rather than sticking there.
        t.adjust_volume(1);
        assert!(t.volume > 0.0, "fader stuck at silence");
    }

    /// Every press pushes the new position to the audio thread. Without this
    /// the fader moves on screen and nothing happens in the speakers, which
    /// is the state this control was in before.
    #[test]
    fn fader_syncs_to_the_audio_thread() {
        let mut t = live_track();
        for steps in [1, 1, -1, 3, -7] {
            t.adjust_volume(steps);
            assert_eq!(
                handle_volume(&t),
                t.volume,
                "audio thread has {} while the UI shows {}",
                handle_volume(&t),
                t.volume
            );
        }
    }

    /// A position loaded from a session that is not on the dB grid snaps onto
    /// it on the first press instead of carrying the offset forever.
    #[test]
    fn fader_snaps_a_loaded_position_onto_the_grid() {
        let mut t = live_track();
        t.volume = 0.6234; // -4.1 dB, as if hand-edited into a .phos file
        t.adjust_volume(1);
        let db = t.volume_db().unwrap();
        assert!((db - db.round()).abs() < 0.01, "off the grid at {db:.3} dB");
    }

    /// Enter locks the fader so h/l edits it, and only on tracks that have
    /// one — a bus track's header does not draw a fader.
    #[test]
    fn enter_locks_the_fader_only_on_tracks_that_have_one() {
        let mut nav = NavState::new(initial_tracks()); // bus tracks only
        nav.enter();
        nav.move_right(); // Label -> Fx
        nav.move_right(); // Fx -> Volume
        assert_eq!(nav.track_element, TrackElement::Volume);
        nav.enter();
        assert!(!nav.element_locked, "locked the fader on a bus track");

        nav.tracks.push(live_track());
        nav.track_cursor = nav.tracks.len() - 1;
        nav.enter();
        assert!(nav.element_locked, "did not lock the fader on an instrument track");

        // Esc releases, leaving the element selected.
        nav.escape();
        assert!(!nav.element_locked);
        assert_eq!(nav.track_element, TrackElement::Volume);
    }

    /// The fader is undoable as a *gesture*: a ride of any length is one
    /// step, and `u` puts the fader back where the ride began. Mute is a
    /// step of its own. Solo and arm stay off the stack — solo is audition
    /// state, like a selection, and arm is the transport's business.
    #[test]
    fn a_fader_ride_is_one_undo_step_and_solo_is_none() {
        let mut nav = NavState::new(vec![live_track()]);
        let origin = nav.tracks[0].volume;
        assert!(!nav.undo_stack.can_undo());

        nav.adjust_volume(3);
        nav.adjust_volume(-1);
        nav.adjust_volume(-1);
        let step = nav.undo_stack.pop_undo().expect("the ride left no step");
        assert!(
            !nav.undo_stack.can_undo(),
            "three presses of one ride left more than one step"
        );
        let undo::StateSlice::TrackMix { volume, .. } = step.before else {
            panic!("wrong slice kind");
        };
        assert_eq!(volume, origin, "the step does not remember where the ride began");

        nav.toggle_mute();
        assert!(nav.undo_stack.can_undo(), "mute left no step");
        let _ = nav.undo_stack.pop_undo();

        nav.toggle_solo();
        nav.toggle_arm();
        assert!(
            !nav.undo_stack.can_undo(),
            "solo or arm pushed an undo entry; both are transient state"
        );
    }
}
