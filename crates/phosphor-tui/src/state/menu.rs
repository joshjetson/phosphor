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
    Panic,
    Save,
    AddInstrument,
    NewTrack,
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
