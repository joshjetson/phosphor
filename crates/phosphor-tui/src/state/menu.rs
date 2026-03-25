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
    Sampler,
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
            Self::Sampler => "Sampler",
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
            Self::Sampler => "sample-based instrument",
        }
    }

    pub const ALL: &[InstrumentType] = &[Self::Synth, Self::DrumRack, Self::DX7, Self::Jupiter8, Self::Odyssey, Self::Juno60, Self::Sampler];
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

// ── Space Menu ──

/// Actions that can be triggered from the space menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceAction {
    PlayPause,
    ToggleRecord,
    ToggleLoop,
    ToggleMetronome,
    Panic,
    Save,
    Open,
    AddInstrument,
    Delete,
    NewTrack,
}

// ── Confirmation Modal ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    DeleteTrack,
    DeleteClip,
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

    pub fn open_save(&mut self, default_name: &str) {
        self.open = true;
        self.kind = InputModalKind::SaveAs;
        self.buffer = format!("sessions/{default_name}");
        self.cursor = self.buffer.len();
    }

    pub fn open_load(&mut self) {
        self.open = true;
        self.kind = InputModalKind::Open;
        self.buffer = "sessions/".to_string();
        self.cursor = self.buffer.len();
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
    ("spc+r", "record",    "toggle global recording"),
    ("spc+l", "loop",      "edit loop region"),
    ("spc+m", "metronome", "toggle click track"),
    ("spc+!", "panic",     "kill all sound immediately"),
    ("spc+a", "add instr", "add instrument track"),
    ("spc+s", "save",      "save project"),
    ("spc+o", "open",      "open project"),
    ("spc+d", "delete",    "delete selected track/clip"),
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
