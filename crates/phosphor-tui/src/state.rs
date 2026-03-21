//! TUI navigation state — focus, cursors, selection, leader keys, FX.
//!
//! Navigation:
//!   Space+N  → jump to component (1=Tracks, 2=ClipView)
//!   Tab      → cycle focus between components
//!   j/k      → vertical nav
//!   h/l      → horizontal nav
//!   Enter    → select / activate / open menus
//!   Esc      → back out one level

use std::time::Instant;

/// Actions that can be triggered from the space menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceAction {
    PlayPause,
    ToggleRecord,
    ToggleLoop,
    Panic,
    Save,
    AddInstrument,
    NewTrack,
}

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

// ── Track Element Navigation ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackElement {
    Label,
    Fx,
    Volume,
    Mute,
    Solo,
    RecordArm,
    Clip(usize),
}

impl TrackElement {
    pub fn move_right(self, num_clips: usize) -> Self {
        match self {
            Self::Label => Self::Fx,
            Self::Fx => Self::Volume,
            Self::Volume => Self::Mute,
            Self::Mute => Self::Solo,
            Self::Solo => Self::RecordArm,
            Self::RecordArm => {
                if num_clips > 0 { Self::Clip(0) } else { Self::RecordArm }
            }
            Self::Clip(i) => {
                if i + 1 < num_clips { Self::Clip(i + 1) } else { Self::Clip(i) }
            }
        }
    }

    pub fn move_left(self) -> Self {
        match self {
            Self::Label => Self::Label,
            Self::Fx => Self::Label,
            Self::Volume => Self::Fx,
            Self::Mute => Self::Volume,
            Self::Solo => Self::Mute,
            Self::RecordArm => Self::Solo,
            Self::Clip(0) => Self::RecordArm,
            Self::Clip(i) => Self::Clip(i - 1),
        }
    }
}

// ── FX System ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxType {
    Reverb,
    Delay,
    Gate,
    Eq,
    Limiter,
    Compressor,
}

impl FxType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Reverb => "reverb",
            Self::Delay => "delay",
            Self::Gate => "gate",
            Self::Eq => "eq",
            Self::Limiter => "limiter",
            Self::Compressor => "comp",
        }
    }

    pub const ALL: &[FxType] = &[
        Self::Reverb, Self::Delay, Self::Gate, Self::Eq, Self::Limiter, Self::Compressor,
    ];
}

/// An FX instance on a track or clip.
#[derive(Debug, Clone)]
pub struct FxInstance {
    pub fx_type: FxType,
    pub enabled: bool,
    /// Placeholder parameter values (0.0..1.0).
    pub params: Vec<(String, f32)>,
}

impl FxInstance {
    pub fn new(fx_type: FxType) -> Self {
        let params = match fx_type {
            FxType::Reverb => vec![
                ("mix".into(), 0.3), ("decay".into(), 0.5), ("size".into(), 0.6),
            ],
            FxType::Delay => vec![
                ("time".into(), 0.4), ("feedback".into(), 0.3), ("mix".into(), 0.25),
            ],
            FxType::Gate => vec![
                ("thresh".into(), 0.5), ("attack".into(), 0.1), ("release".into(), 0.3),
            ],
            FxType::Eq => vec![
                ("low".into(), 0.5), ("mid".into(), 0.5), ("high".into(), 0.5),
            ],
            FxType::Limiter => vec![
                ("thresh".into(), 0.8), ("release".into(), 0.2),
            ],
            FxType::Compressor => vec![
                ("thresh".into(), 0.6), ("ratio".into(), 0.4), ("attack".into(), 0.1),
                ("release".into(), 0.3),
            ],
        };
        Self { fx_type, enabled: true, params }
    }
}

/// Where audio is routed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioRoute {
    Master,
    SendA,
    SendB,
}

impl AudioRoute {
    pub fn label(self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::SendA => "send A",
            Self::SendB => "send B",
        }
    }
}

/// FX menu state (opened when pressing Enter on fx button).
#[derive(Debug)]
pub struct FxMenu {
    pub open: bool,
    pub cursor: usize,
    /// Which tab: 0=add fx, 1=routing
    pub tab: usize,
}

impl FxMenu {
    pub fn new() -> Self {
        Self { open: false, cursor: 0, tab: 0 }
    }

    pub fn item_count(&self) -> usize {
        match self.tab {
            0 => FxType::ALL.len(),
            1 => 3, // master, send A, send B
            _ => 0,
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 { self.cursor -= 1; }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.item_count() { self.cursor += 1; }
    }

    pub fn next_tab(&mut self) {
        self.tab = (self.tab + 1) % 2;
        self.cursor = 0;
    }
}

// ── Instrument Selection Modal ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentType {
    Synth,
    DrumRack,
    Sampler,
}

impl InstrumentType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Synth => "Phosphor Synth",
            Self::DrumRack => "Drum Rack",
            Self::Sampler => "Sampler",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Synth => "polyphonic subtractive synthesizer",
            Self::DrumRack => "drum machine with sample pads",
            Self::Sampler => "sample-based instrument",
        }
    }

    pub const ALL: &[InstrumentType] = &[Self::Synth, Self::DrumRack, Self::Sampler];
}

#[derive(Debug)]
pub struct InstrumentModal {
    pub open: bool,
    pub cursor: usize,
}

impl InstrumentModal {
    pub fn new() -> Self {
        Self { open: false, cursor: 0 }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 { self.cursor -= 1; }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < InstrumentType::ALL.len() { self.cursor += 1; }
    }

    pub fn selected(&self) -> InstrumentType {
        InstrumentType::ALL[self.cursor]
    }
}

// ── Track Types ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackType {
    Audio,
    SendA,
    SendB,
    Master,
}

// ── Data Models ──

#[derive(Debug, Clone)]
pub struct Clip {
    pub number: usize,
    pub width: u16,
    pub has_content: bool,
    pub midi_notes: Vec<MidiNote>,
    /// FX chain on this specific clip.
    pub fx_chain: Vec<FxInstance>,
    /// Volume envelope (placeholder).
    pub volume: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct MidiNote {
    pub note: u8,
    pub start: f64,
    pub duration: f64,
    pub velocity: u8,
}

#[derive(Debug, Clone)]
pub struct TrackState {
    pub name: String,
    pub muted: bool,
    pub soloed: bool,
    pub armed: bool,
    pub color_index: usize,
    pub track_type: TrackType,
    pub clips: Vec<Clip>,
    /// Track-level FX chain.
    pub fx_chain: Vec<FxInstance>,
    /// Audio routing destination.
    pub route: AudioRoute,
    /// Track volume (0.0..1.0).
    pub volume: f32,
    /// Unique ID for this track (matches the mixer's track ID).
    pub mixer_id: Option<usize>,
    /// Handle to the audio engine's track state. When present, mute/solo/arm/volume
    /// writes go directly to the audio thread via atomics.
    pub handle: Option<std::sync::Arc<phosphor_core::project::TrackHandle>>,
    /// Synth parameter values (mirrors the audio thread's plugin params).
    /// Index matches phosphor_dsp::synth::P_* constants.
    pub synth_params: Vec<f32>,
}

impl TrackState {
    pub fn new(name: &str, color_index: usize, armed: bool, track_type: TrackType, clips: Vec<Clip>) -> Self {
        Self {
            name: name.to_string(),
            muted: false,
            soloed: false,
            armed,
            color_index,
            track_type,
            clips,
            fx_chain: Vec::new(),
            route: AudioRoute::Master,
            volume: 0.75,
            mixer_id: None,
            handle: None,
            synth_params: Vec::new(),
        }
    }

    /// Sync mute/solo/arm/volume to the audio thread handle (if wired up).
    pub fn sync_to_audio(&self) {
        if let Some(ref h) = self.handle {
            h.config.muted.store(self.muted, std::sync::atomic::Ordering::Relaxed);
            h.config.soloed.store(self.soloed, std::sync::atomic::Ordering::Relaxed);
            h.config.armed.store(self.armed, std::sync::atomic::Ordering::Relaxed);
            h.config.set_volume(self.volume);
        }
    }

    /// Read VU levels from the audio thread handle.
    pub fn vu_levels(&self) -> (f32, f32) {
        self.handle.as_ref().map(|h| h.vu.get()).unwrap_or((0.0, 0.0))
    }

    /// Whether this track is wired to the audio engine.
    pub fn is_live(&self) -> bool {
        self.handle.is_some()
    }
}

// ── Clip View State ──

/// Which sub-panel of the clip view has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipViewFocus {
    /// FX panel on the left.
    FxPanel,
    /// Piano roll / clip content on the right.
    PianoRoll,
}

/// Tab in the FX panel (left side of clip view).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxPanelTab {
    /// Track-level FX chain.
    TrackFx,
    /// Synth / instrument controls.
    Synth,
    /// Clip-level FX.
    ClipFx,
}

impl FxPanelTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::TrackFx => "trk fx",
            Self::Synth => "synth",
            Self::ClipFx => "clip fx",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::TrackFx => Self::Synth,
            Self::Synth => Self::ClipFx,
            Self::ClipFx => Self::TrackFx,
        }
    }
}

/// Tab in the piano roll / clip area (right side of clip view).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipTab {
    PianoRoll,
    ClipFx,
    Automation,
}

impl ClipTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::PianoRoll => "piano",
            Self::ClipFx => "clip fx",
            Self::Automation => "auto",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::PianoRoll => Self::ClipFx,
            Self::ClipFx => Self::Automation,
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
    /// FX panel cursor (which fx in the chain).
    pub fx_cursor: usize,
    /// Synth parameter cursor (which param to adjust).
    pub synth_param_cursor: usize,
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

// ── Space Menu ──

/// The space menu: press Space to open, Space again to close.
/// Shows all Space+key shortcuts, actions, and help topics.
#[derive(Debug)]
pub struct SpaceMenu {
    pub open: bool,
    pub cursor: usize,
    /// Which section is active.
    pub section: SpaceMenuSection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceMenuSection {
    /// Main shortcuts list.
    Actions,
    /// Help topics.
    Help,
}

impl SpaceMenu {
    pub fn new() -> Self {
        Self { open: false, cursor: 0, section: SpaceMenuSection::Actions }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open { self.cursor = 0; self.section = SpaceMenuSection::Actions; }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 { self.cursor -= 1; }
    }

    pub fn move_down(&mut self) {
        let max = self.item_count();
        if self.cursor + 1 < max { self.cursor += 1; }
    }

    pub fn switch_section(&mut self) {
        self.section = match self.section {
            SpaceMenuSection::Actions => SpaceMenuSection::Help,
            SpaceMenuSection::Help => SpaceMenuSection::Actions,
        };
        self.cursor = 0;
    }

    fn item_count(&self) -> usize {
        match self.section {
            SpaceMenuSection::Actions => SPACE_ACTIONS.len(),
            SpaceMenuSection::Help => HELP_TOPICS.len(),
        }
    }
}

/// Space menu action entries: (key, label, description).
pub const SPACE_ACTIONS: &[(&str, &str, &str)] = &[
    ("spc+1", "tracks",    "focus the tracks panel"),
    ("spc+2", "clip view", "focus the clip / piano roll panel"),
    ("spc+p", "play/pause","toggle transport playback"),
    ("spc+r", "record",    "toggle global recording"),
    ("spc+l", "loop",      "toggle loop mode"),
    ("spc+!", "panic",     "kill all sound immediately"),
    ("spc+a", "add instr", "add instrument track (synth, drums)"),
    ("spc+s", "save",      "save project"),
    ("spc+n", "new track", "add a new audio track"),
    ("spc+h", "help",      "open help topics"),
];

/// Help topic entries: (title, short description).
pub const HELP_TOPICS: &[(&str, &str)] = &[
    ("navigation",  "moving between tracks, clips, and panes"),
    ("transport",   "play, pause, stop, record, loop, BPM"),
    ("tracks",      "mute, solo, arm, fx, volume, routing"),
    ("clips",       "selecting, jumping, clip-level fx"),
    ("piano roll",  "editing MIDI notes, velocity, quantize"),
    ("fx & mixing", "adding effects, sends, master bus"),
    ("shortcuts",   "full keyboard shortcut reference"),
    ("plugins",     "loading and managing plugins"),
];

// ── Number Buffer ──

#[derive(Debug)]
pub struct NumberBuffer {
    digits: String,
    last_input: Option<Instant>,
    timeout_ms: u128,
}

impl NumberBuffer {
    pub fn new() -> Self { Self { digits: String::new(), last_input: None, timeout_ms: 500 } }

    pub fn push_digit(&mut self, ch: char) -> Option<usize> {
        if self.is_timed_out() { self.digits.clear(); }
        self.digits.push(ch);
        self.last_input = Some(Instant::now());
        None
    }

    pub fn check_timeout(&mut self) -> Option<usize> {
        if self.digits.is_empty() { return None; }
        if self.is_timed_out() {
            let num = self.digits.parse::<usize>().ok();
            self.digits.clear();
            self.last_input = None;
            num
        } else { None }
    }

    fn is_timed_out(&self) -> bool {
        self.last_input.map(|t| t.elapsed().as_millis() >= self.timeout_ms).unwrap_or(true)
    }

    pub fn display(&self) -> &str {
        if self.is_timed_out() { "" } else { &self.digits }
    }

    pub fn commit(&mut self) -> Option<usize> {
        if self.digits.is_empty() { return None; }
        let num = self.digits.parse::<usize>().ok();
        self.digits.clear();
        self.last_input = None;
        num
    }
}

// ── Piano Roll State ──

#[derive(Debug)]
pub struct PianoRollState {
    pub cursor_note: u8,
    pub scroll_x: usize,
    pub view_bottom_note: u8,
    pub view_height: u8,
}

impl PianoRollState {
    pub fn new() -> Self {
        Self { cursor_note: 60, scroll_x: 0, view_bottom_note: 48, view_height: 24 }
    }

    pub fn move_up(&mut self) {
        if self.cursor_note < 127 {
            self.cursor_note += 1;
            if self.cursor_note >= self.view_bottom_note + self.view_height {
                self.view_bottom_note = self.view_bottom_note.saturating_add(1);
            }
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_note > 0 {
            self.cursor_note -= 1;
            if self.cursor_note < self.view_bottom_note {
                self.view_bottom_note = self.view_bottom_note.saturating_sub(1);
            }
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
        // Space menu open → select item
        if self.space_menu.open {
            return self.space_menu_select();
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

    fn space_menu_select(&mut self) -> Option<SpaceAction> {
        match self.space_menu.section {
            SpaceMenuSection::Actions => {
                let action = SPACE_ACTIONS.get(self.space_menu.cursor);
                self.space_menu.open = false;
                if let Some((key, _, _)) = action {
                    // Parse the key and dispatch
                    match *key {
                        "spc+1" => { self.focus_pane(Pane::Tracks); None }
                        "spc+2" => { self.focus_pane(Pane::ClipView); None }
                        "spc+p" => Some(SpaceAction::PlayPause),
                        "spc+r" => Some(SpaceAction::ToggleRecord),
                        "spc+l" => Some(SpaceAction::ToggleLoop),
                        "spc+!" => Some(SpaceAction::Panic),
                        "spc+a" => Some(SpaceAction::AddInstrument),
                        "spc+s" => Some(SpaceAction::Save),
                        "spc+n" => Some(SpaceAction::NewTrack),
                        "spc+h" => {
                            self.space_menu.open = true;
                            self.space_menu.section = SpaceMenuSection::Help;
                            self.space_menu.cursor = 0;
                            None
                        }
                        _ => None,
                    }
                } else { None }
            }
            SpaceMenuSection::Help => {
                // For now, help topics just show info — no action
                // Future: could open a detailed help pane
                None
            }
        }
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
        } else if self.fx_menu.open {
            self.fx_menu.next_tab();
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
                self.fx_menu.tab = 0;
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
            matches!(t.track_type, TrackType::SendA | TrackType::SendB | TrackType::Master)
        }).unwrap_or(self.tracks.len());

        let color = insert_pos % 8;
        let mut track = TrackState::new(name, color, true, TrackType::Audio, vec![]);
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
    fn show_current_track_controls(&mut self) {
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
        match self.fx_menu.tab {
            0 => {
                // Add FX
                if let Some(fx_type) = FxType::ALL.get(self.fx_menu.cursor) {
                    let inst = FxInstance::new(*fx_type);
                    if let Some(t) = self.current_track_mut() {
                        t.fx_chain.push(inst);
                    }
                }
                self.fx_menu.open = false;
            }
            1 => {
                // Routing
                let route = match self.fx_menu.cursor {
                    0 => AudioRoute::Master,
                    1 => AudioRoute::SendA,
                    2 => AudioRoute::SendB,
                    _ => return,
                };
                if let Some(t) = self.current_track_mut() {
                    t.route = route;
                }
                self.fx_menu.open = false;
            }
            _ => {}
        }
    }

    fn active_fx_chain_len(&self) -> usize {
        match self.clip_view.fx_panel_tab {
            FxPanelTab::TrackFx | FxPanelTab::Synth => {
                self.current_track().map(|t| t.fx_chain.len().max(1)).unwrap_or(1)
            }
            FxPanelTab::ClipFx => {
                self.active_clip().map(|c| c.fx_chain.len().max(1)).unwrap_or(1)
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
}

// ── Placeholder Data ──

/// Initial tracks: just the bus tracks. Instruments are added by the user via Space+A.
pub fn initial_tracks() -> Vec<TrackState> {
    vec![
        TrackState::new("snd a", 5, false, TrackType::SendA, vec![]),
        TrackState::new("snd b", 6, false, TrackType::SendB, vec![]),
        TrackState::new("mstr", 7, false, TrackType::Master, vec![]),
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
        assert_eq!(tracks[0].track_type, TrackType::SendA);
        assert_eq!(tracks[1].track_type, TrackType::SendB);
        assert_eq!(tracks[2].track_type, TrackType::Master);
    }

    #[test]
    fn sends_are_at_end() {
        let mut nav = NavState::new(initial_tracks());
        nav.move_down();
        nav.move_down();
        assert_eq!(nav.track_cursor, 2);
        assert_eq!(nav.tracks[nav.track_cursor].track_type, TrackType::Master);
    }

    #[test]
    fn fx_menu_opens_and_closes() {
        let mut nav = NavState::new(initial_tracks());
        nav.enter(); // select track
        // Navigate to FX
        nav.move_right(); // → Fx
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
        nav.move_right(); // → Fx
        nav.enter(); // open menu
        nav.enter(); // select first item (Reverb)
        assert!(!nav.fx_menu.open);
        assert_eq!(nav.tracks[0].fx_chain.len(), initial_count + 1);
        assert_eq!(nav.tracks[0].fx_chain.last().unwrap().fx_type, FxType::Reverb);
    }

    #[test]
    fn fx_menu_routing() {
        let mut nav = NavState::new(initial_tracks());
        nav.enter();
        nav.move_right(); // → Fx
        nav.enter(); // open menu
        nav.cycle_tab(); // switch to routing tab
        assert_eq!(nav.fx_menu.tab, 1);
        nav.move_down(); // → Send A
        nav.enter(); // select
        assert_eq!(nav.tracks[0].route, AudioRoute::SendA);
    }

    #[test]
    fn clip_view_focus_toggle() {
        let mut nav = NavState::new(initial_tracks());
        // Manually set up clip view (simulating an instrument track being selected)
        nav.clip_view_visible = true;
        nav.clip_view_target = Some((0, 0));

        nav.focus_pane(Pane::ClipView);
        assert_eq!(nav.clip_view.focus, ClipViewFocus::PianoRoll);

        nav.move_left(); // → FxPanel
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
        assert_eq!(nav.clip_view.fx_panel_tab, FxPanelTab::ClipFx);

        nav.clip_view.focus = ClipViewFocus::PianoRoll;
        assert_eq!(nav.clip_view.clip_tab, ClipTab::PianoRoll);
        nav.cycle_tab();
        assert_eq!(nav.clip_view.clip_tab, ClipTab::ClipFx);
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
