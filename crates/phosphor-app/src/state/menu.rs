//! Menu state — SpaceMenu, FxMenu, InstrumentModal, FX types.

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

/// An FX instance on a track.
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

/// FX menu state (opened when pressing Enter on fx button).
#[derive(Debug)]
pub struct FxMenu {
    pub open: bool,
    pub cursor: usize,
}

impl Default for FxMenu {
    fn default() -> Self { Self::new() }
}

impl FxMenu {
    pub fn new() -> Self {
        Self { open: false, cursor: 0 }
    }

    pub fn item_count(&self) -> usize {
        FxType::ALL.len()
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 { self.cursor -= 1; }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.item_count() { self.cursor += 1; }
    }
}

// ── Instrument Selection Modal ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentType {
    Synth,
    DrumRack,
    DX7,
    Jupiter8,
    Odyssey,
    Juno60,
    Rhodes,
    Sampler,
    LittlePhatty,
    Prophet6,
    /// The step sequencer. Not an instrument: it drives one.
    Sequencer,
}

impl InstrumentType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Synth => "Phosphor Synth",
            Self::DrumRack => "Drum Rack",
            Self::DX7 => "DX7",
            Self::Jupiter8 => "Jupiter-8",
            Self::Odyssey => "Odyssey",
            Self::Juno60 => "Juno-60",
            Self::Rhodes => "Rhodes",
            Self::Sampler => "Sampler",
            Self::LittlePhatty => "Little Phatty",
            Self::Prophet6 => "Prophet-6",
            Self::Sequencer => "Step Sequencer",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Synth => "polyphonic subtractive synthesizer",
            Self::DrumRack => "drum machine with sample pads",
            Self::DX7 => "6-operator FM synthesizer",
            Self::Jupiter8 => "dual-VCO analog poly synthesizer",
            Self::Odyssey => "duophonic synth with 3 filter types",
            Self::Juno60 => "single-DCO poly with BBD chorus",
            Self::Rhodes => "modelled tine electric piano",
            Self::Sampler => "sample-based instrument",
            Self::LittlePhatty => "monophonic Moog with morphing waves",
            Self::Prophet6 => "six-voice analog poly with poly mod",
            Self::Sequencer => "pattern sequencer driving any instrument",
        }
    }

    /// Appended to, never reordered: a session stores an instrument by its
    /// key rather than by position, but the menu's own order is what a player
    /// has learned, and the preset browser walks this list.
    pub const ALL: &[InstrumentType] = &[Self::Synth, Self::DrumRack, Self::DX7, Self::Jupiter8, Self::Odyssey, Self::Juno60, Self::Rhodes, Self::Sampler, Self::LittlePhatty, Self::Prophet6, Self::Sequencer];

    /// Whether picking this from the add-track menu builds a step sequencer
    /// rather than an instrument.
    ///
    /// A sequencer track's `instrument_type` is its *child* — the thing in
    /// the plugin slot making the sound — so this entry never ends up stored
    /// on a track. It is a choice in a menu, and the track it produces is an
    /// ordinary instrument track with a pattern player in front of it.
    #[must_use]
    pub const fn is_sequencer(self) -> bool {
        matches!(self, Self::Sequencer)
    }
}

#[derive(Debug)]
pub struct InstrumentModal {
    pub open: bool,
    pub cursor: usize,
}

impl Default for InstrumentModal {
    fn default() -> Self { Self::new() }
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

// ── Preset Browser Modal ──

/// The user-preset browser for the track under the cursor.
///
/// A modal rather than extra entries on the patch knob: the patch selector
/// stores a normalised fraction, so lengthening the bank it indexes would
/// remap every value already saved in a session. Browsing presets in their own
/// list moves nothing.
#[derive(Debug)]
pub struct PresetModal {
    pub open: bool,
    /// Whose bank this is. `None` until the modal is opened on a track.
    pub instrument: Option<InstrumentType>,
    /// The track the bank was opened for. Held so a load lands on that track
    /// even if something moved the cursor while the modal was up.
    pub track_idx: usize,
    pub cursor: usize,
    /// Preset names in bank order, read when the modal opened.
    pub entries: Vec<String>,
    /// Why the bank could not be read, when it could not.
    pub error: Option<String>,
    /// Name waiting on an overwrite confirmation.
    pub pending_name: String,
}

impl Default for PresetModal {
    fn default() -> Self { Self::new() }
}

impl PresetModal {
    /// Row 0 is always "save the current panel"; the presets follow it.
    pub const SAVE_ROW: usize = 0;

    pub fn new() -> Self {
        Self {
            open: false,
            instrument: None,
            track_idx: 0,
            cursor: 0,
            entries: Vec::new(),
            error: None,
            pending_name: String::new(),
        }
    }

    pub fn show(&mut self, instrument: InstrumentType, track_idx: usize, entries: Vec<String>) {
        self.open = true;
        self.instrument = Some(instrument);
        self.track_idx = track_idx;
        self.cursor = 0;
        self.entries = entries;
        self.error = None;
        self.pending_name.clear();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.entries.clear();
        self.error = None;
        self.pending_name.clear();
        self.cursor = 0;
    }

    /// Rows in the list: the save row plus one per preset.
    pub fn item_count(&self) -> usize { self.entries.len() + 1 }

    pub fn move_up(&mut self) {
        if self.cursor > 0 { self.cursor -= 1; }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.item_count() { self.cursor += 1; }
    }

    /// Index into the bank for the selected row, or `None` on the save row.
    pub fn selected_preset(&self) -> Option<usize> {
        self.cursor.checked_sub(1).filter(|i| *i < self.entries.len())
    }

    /// Name of the selected preset, or `None` on the save row.
    pub fn selected_name(&self) -> Option<&str> {
        self.selected_preset().map(|i| self.entries[i].as_str())
    }

    /// Replace the list after a save or delete, keeping the cursor on
    /// something that exists.
    pub fn set_entries(&mut self, entries: Vec<String>) {
        self.entries = entries;
        let max = self.item_count() - 1;
        if self.cursor > max { self.cursor = max; }
    }
}

// ── Space Menu ──

/// Actions that can be triggered from the space menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceAction {
    PlayPause,
    /// Stop and return the playhead to the top of the song.
    Stop,
    ToggleRecord,
    ToggleLoop,
    ToggleMetronome,
    Panic,
    Save,
    Open,
    AddInstrument,
    Delete,
    CycleTheme,
    NewTrack,
    EditMode,
    Quantize,
    Presets,
}

// ── Confirmation Modal ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    DeleteTrack,
    DeleteClip,
    DeletePreset,
    /// Saving over a preset name the bank already holds.
    OverwritePreset,
}

#[derive(Debug)]
pub struct ConfirmModal {
    pub open: bool,
    pub kind: ConfirmKind,
    pub message: String,
}

impl Default for ConfirmModal {
    fn default() -> Self { Self::new() }
}

impl ConfirmModal {
    pub fn new() -> Self {
        Self { open: false, kind: ConfirmKind::DeleteTrack, message: String::new() }
    }

    pub fn show(&mut self, kind: ConfirmKind, message: &str) {
        self.open = true;
        self.kind = kind;
        self.message = message.to_string();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.message.clear();
    }
}

// ── Input Modal (for file path entry) ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputModalKind {
    SaveAs,
    Open,
    /// Naming a user preset from the preset browser.
    PresetName,
}

#[derive(Debug)]
pub struct InputModal {
    pub open: bool,
    pub kind: InputModalKind,
    pub buffer: String,
    pub cursor: usize,
}

impl Default for InputModal {
    fn default() -> Self { Self::new() }
}

impl InputModal {
    pub fn new() -> Self {
        Self { open: false, kind: InputModalKind::SaveAs, buffer: String::new(), cursor: 0 }
    }

    /// Ask for a filename to save under.
    ///
    /// The field starts in `sessions/` when the working directory has one —
    /// a checkout being run from its own root, which is where every session
    /// on disk already is — and in the absolute `<app dir>/sessions/`
    /// otherwise. A bare `sessions/` resolves against wherever the process was
    /// started, so a shortcut, an alias or a desktop launcher would write the
    /// file successfully into a directory nobody is going to look in again.
    /// See [`crate::paths::session_prompt_dir`].
    pub fn open_save(&mut self, default_name: &str) {
        self.open = true;
        self.kind = InputModalKind::SaveAs;
        self.buffer = format!("{}{default_name}", crate::paths::session_prompt_dir());
        self.cursor = self.buffer.len();
    }

    /// Ask for a file to open. Same starting directory as [`Self::open_save`];
    /// a relative path typed here is also looked for under the application
    /// directory, so the way a checkout spells a session keeps working from
    /// anywhere. See [`crate::paths::find_session`].
    pub fn open_load(&mut self) {
        self.open = true;
        self.kind = InputModalKind::Open;
        self.buffer = crate::paths::session_prompt_dir();
        self.cursor = self.buffer.len();
    }

    /// Name a user preset. Starts empty rather than on a suggestion, because
    /// a suggestion the player accepts by reflex is how a bank fills up with
    /// eight sounds called "juno".
    pub fn open_preset_name(&mut self) {
        self.open = true;
        self.kind = InputModalKind::PresetName;
        self.buffer.clear();
        self.cursor = 0;
    }

    pub fn type_char(&mut self, ch: char) {
        self.buffer.insert(self.cursor, ch);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.buffer.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.buffer.len() {
            self.buffer.remove(self.cursor);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 { self.cursor -= 1; }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.buffer.len() { self.cursor += 1; }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.buffer.clear();
        self.cursor = 0;
    }

    pub fn value(&self) -> &str {
        &self.buffer
    }
}

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

impl Default for SpaceMenu {
    fn default() -> Self { Self::new() }
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
    ("spc+1", "transport", "focus transport controls"),
    ("spc+2", "tracks",    "focus the tracks panel"),
    ("spc+3", "clip view", "focus clip / piano roll panel"),
    ("spc+p", "play/pause","toggle transport playback"),
    ("spc+0", "stop",      "stop and return to bar 1"),
    ("spc+r", "record",    "toggle global recording"),
    ("spc+l", "loop",      "edit loop region"),
    ("spc+m", "metronome", "toggle click track"),
    ("spc+!", "panic",     "kill all sound immediately"),
    ("spc+a", "add instr", "add instrument track"),
    ("spc+s", "save",      "save project"),
    ("spc+o", "open",      "open project"),
    ("spc+d", "delete",    "delete selected track/clip"),
    ("spc+e", "edit mode", "note-level piano roll editing"),
    ("spc+q", "quantize",  "snap notes to grid"),
    ("spc+w", "presets",   "save / load instrument presets"),
    ("spc+v", "vibe",      "cycle color theme"),
    ("spc+h", "help",      "open help topics"),
];

// ── Quantize Modal ──

use super::clip_view::GridResolution;

#[derive(Debug)]
pub struct QuantizeModal {
    pub open: bool,
    pub grid: GridResolution,
    pub strength: u8,
    pub cursor: usize,
}

impl Default for QuantizeModal {
    fn default() -> Self { Self::new() }
}

impl QuantizeModal {
    pub fn new() -> Self {
        Self { open: false, grid: GridResolution::Eighth, strength: 100, cursor: 0 }
    }
    pub fn open_with(&mut self, grid: GridResolution) {
        self.open = true;
        self.grid = grid;
        self.strength = 100;
        self.cursor = 0;
    }
    pub fn close(&mut self) { self.open = false; }
    pub fn move_up(&mut self) { if self.cursor > 0 { self.cursor -= 1; } }
    pub fn move_down(&mut self) { if self.cursor < 2 { self.cursor += 1; } }
    pub fn adjust(&mut self, direction: i32) {
        match self.cursor {
            0 => { if direction > 0 { self.grid = self.grid.next(); } else { self.grid = self.grid.prev(); } }
            1 => { self.strength = (self.strength as i32 + direction * 25).clamp(25, 100) as u8; }
            _ => {}
        }
    }
}

/// Help topic entries: (title, short description).
pub const HELP_TOPICS: &[(&str, &str)] = &[
    ("navigation",  "moving between tracks, clips, and panes"),
    ("transport",   "play, pause, stop, record, loop, BPM"),
    ("tracks",      "mute, solo, arm, fx, volume, routing"),
    ("clips",       "selecting, jumping, clip-level fx"),
    ("piano roll",  "editing MIDI notes, velocity, quantize"),
    ("step grid",   "n hit \u{00B7} a accent \u{00B7} [ ] lane \u{00B7} jk band \u{00B7} b bounce \u{00B7} t run"),
    ("fx & mixing", "adding effects, sends, master bus"),
    ("shortcuts",   "full keyboard shortcut reference"),
    ("plugins",     "loading and managing plugins"),
];
