//! Menu state — SpaceMenu, FxMenu, InstrumentModal, FX types.

// ── FX System ──

/// The effects a player can put in an insert slot.
///
/// Five, and only the five that exist. The list used to carry a gate and a
/// limiter that were never built; a menu entry for a thing that does nothing
/// is worse than no entry, because the player spends the next minute
/// wondering what they did wrong. The safety limiter on the master is not in
/// here either: it is not a slot, it cannot be moved, and framing it as an
/// effect invites someone to try to delete it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxType {
    Eq,
    Compressor,
    Tape,
    Delay,
    Reverb,
}

impl FxType {
    /// What the menu calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Compressor => "comp",
            Self::Tape => "tape",
            Self::Delay => "delay",
            Self::Reverb => "reverb",
        }
    }

    /// Three characters, for a track strip that has no room for more — a bus
    /// carrying a reverb reads `rvb` rather than `snd a`.
    pub fn short(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Compressor => "cmp",
            Self::Tape => "tap",
            Self::Delay => "dly",
            Self::Reverb => "rvb",
        }
    }

    /// The stable name this effect is stored under in a session file.
    ///
    /// An identifier rather than a label: renaming a label is a cosmetic
    /// change, renaming this orphans every saved chain that contains one.
    /// It is the same string the audio-thread effect answers to
    /// [`phosphor_core::fx::Effect::name`] with.
    pub fn key(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Compressor => "comp",
            Self::Tape => "tape",
            Self::Delay => "delay",
            Self::Reverb => "reverb",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.key() == key)
    }

    /// Where this effect belongs in a chain, low numbers first.
    ///
    /// The canonical order an engineer would build by hand: tone-shaping
    /// before dynamics before saturation before time. Adding an effect drops
    /// it at its canonical position among the slots that are already there —
    /// but nothing already in the chain is ever moved by it, because a chain
    /// the player arranged is a decision and not a mistake to be corrected.
    pub fn canonical_rank(self) -> u8 {
        match self {
            Self::Eq => 1,
            Self::Compressor => 2,
            Self::Tape => 3,
            Self::Delay => 4,
            Self::Reverb => 5,
        }
    }

    pub const ALL: &[FxType] = &[
        Self::Eq,
        Self::Compressor,
        Self::Tape,
        Self::Delay,
        Self::Reverb,
    ];
}

/// The UI's copy of one effect in a chain.
///
/// A mirror, not the effect. The effect itself lives on the audio thread
/// inside an [`phosphor_core::fx::FxChain`]; this is what the screen is drawn
/// from and what a session is written from, and every edit to it goes to the
/// audio thread as a command. Two copies of the same state is a thing to be
/// suspicious of, and the alternative — reading the audio thread's chain to
/// draw a frame — is a lock in the callback.
///
/// `params` are in the effect's own units, decibels and hertz and
/// milliseconds, in the order the effect declares them.
#[derive(Debug, Clone)]
pub struct FxInstance {
    pub fx_type: FxType,
    /// Whether the slot's bypass switch is thrown. Bypassed is the exception,
    /// so the field reads the way the switch does.
    pub bypass: bool,
    pub params: Vec<f32>,
    /// The gain-reduction meter this slot's effect publishes to, when it has
    /// one — the compressor does, and nothing else does yet.
    ///
    /// A window onto the audio thread rather than state, exactly like a
    /// track's `TrackHandle`: the effect on the far side writes two atomics
    /// and the panel reads them. Attached when the effect is built, and
    /// re-attached whenever the chain is reinstalled, so a slot that has been
    /// copied, pasted or reloaded points at the effect that is actually in the
    /// signal path rather than at the one it was cloned from.
    pub gr: Option<std::sync::Arc<phosphor_core::fx::GrMeter>>,
}

/// Two slots are the same slot when they hold the same effect at the same
/// settings.
///
/// The meter is deliberately not part of it: it is a window onto the audio
/// thread, not state, and two chains that differ only in which running
/// compressor they are watching are the same chain as far as a session, an
/// undo step or a comparison is concerned.
impl PartialEq for FxInstance {
    fn eq(&self, other: &Self) -> bool {
        self.fx_type == other.fx_type
            && self.bypass == other.bypass
            && self.params == other.params
    }
}

impl FxInstance {
    #[must_use]
    pub fn new(fx_type: FxType, params: Vec<f32>) -> Self {
        Self { fx_type, bypass: false, params, gr: None }
    }

    /// The same slot, watching a meter.
    #[must_use]
    pub fn with_meter(
        mut self,
        meter: Option<std::sync::Arc<phosphor_core::fx::GrMeter>>,
    ) -> Self {
        self.gr = meter;
        self
    }

    /// What the strip shows for this slot: `eq`, or `eq \u{00b7}` when it is
    /// bypassed.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.bypass
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
    Teo5,
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
            Self::Teo5 => "TEO-5",
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
            Self::Teo5 => "five-voice analog with the SEM filter",
            Self::Sequencer => "pattern sequencer driving any instrument",
        }
    }

    /// Order is presentation only — safe to rearrange. Sessions and presets
    /// store an instrument by its key (`session::instrument_key`), never by
    /// position in this list, and every use of `ALL` in the workspace is
    /// iteration. Instruments stay grouped together and the sequencer stays
    /// last, because it is not an instrument: it drives one.
    pub const ALL: &[InstrumentType] = &[Self::Synth, Self::DrumRack, Self::DX7, Self::Jupiter8, Self::Odyssey, Self::Juno60, Self::Rhodes, Self::Sampler, Self::LittlePhatty, Self::Prophet6, Self::Teo5, Self::Sequencer];

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
    /// Taking an effect out of a chain. Undoable, but still asked about:
    /// a chain is work, and a `d` that lands one row off should have to say
    /// what it is about to take.
    DeleteFx,
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
    /// The name Enter uses when nothing has been typed after the directory.
    ///
    /// Shown dim, after the cursor, rather than sitting in the field: a
    /// prompt that opens with `sessions/untitled.phos` already in it and the
    /// cursor at the end means every character typed lands *after* the
    /// extension. A player typing the name of their song got
    /// `sessions/untitled.phosneon_causeway`, and the file that appeared was
    /// called untitled.
    placeholder: String,
}

impl Default for InputModal {
    fn default() -> Self { Self::new() }
}

impl InputModal {
    pub fn new() -> Self {
        Self {
            open: false,
            kind: InputModalKind::SaveAs,
            buffer: String::new(),
            cursor: 0,
            placeholder: String::new(),
        }
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
    /// The field holds the directory and nothing else, so the first key
    /// pressed is the first letter of the name. `default_name` is what Enter
    /// falls back to on an untouched prompt, and is shown dim where the name
    /// would go — a suggestion rather than text to delete.
    pub fn open_save(&mut self, default_name: &str) {
        self.open = true;
        self.kind = InputModalKind::SaveAs;
        self.buffer = crate::paths::session_prompt_dir();
        self.cursor = self.len_chars();
        self.placeholder = default_name.to_string();
    }

    /// Ask for a file to open. Same starting directory as [`Self::open_save`];
    /// a relative path typed here is also looked for under the application
    /// directory, so the way a checkout spells a session keeps working from
    /// anywhere. See [`crate::paths::find_session`].
    pub fn open_load(&mut self) {
        self.open = true;
        self.kind = InputModalKind::Open;
        self.buffer = crate::paths::session_prompt_dir();
        self.cursor = self.len_chars();
        self.placeholder.clear();
    }

    /// Name a user preset. Starts empty rather than on a suggestion, because
    /// a suggestion the player accepts by reflex is how a bank fills up with
    /// eight sounds called "juno".
    pub fn open_preset_name(&mut self) {
        self.open = true;
        self.kind = InputModalKind::PresetName;
        self.buffer.clear();
        self.cursor = 0;
        self.placeholder.clear();
    }

    /// How many characters are in the field.
    ///
    /// The cursor counts characters, not bytes. It has to: it is drawn as a
    /// column on a screen and moved by an arrow key, both of which are
    /// character-shaped. `String` is indexed in bytes, so every method here
    /// converts — and the ones that did not used to panic the whole
    /// application the first time an accented letter was typed into a
    /// filename, which is a perfectly ordinary thing to do.
    fn len_chars(&self) -> usize {
        self.buffer.chars().count()
    }

    /// The byte offset of character `index`, or the end of the string.
    fn byte_at(&self, index: usize) -> usize {
        self.buffer
            .char_indices()
            .nth(index)
            .map_or(self.buffer.len(), |(offset, _)| offset)
    }

    pub fn type_char(&mut self, ch: char) {
        let at = self.byte_at(self.cursor);
        self.buffer.insert(at, ch);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let at = self.byte_at(self.cursor);
            self.buffer.remove(at);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.len_chars() {
            let at = self.byte_at(self.cursor);
            self.buffer.remove(at);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 { self.cursor -= 1; }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.len_chars() { self.cursor += 1; }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.len_chars();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.buffer.clear();
        self.cursor = 0;
        self.placeholder.clear();
    }

    pub fn value(&self) -> &str {
        &self.buffer
    }

    /// Whether the field names a file yet, or is still just a directory.
    fn names_a_file(&self) -> bool {
        !self
            .buffer
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .is_empty()
    }

    /// The dim suggestion drawn where the name would go, or nothing once
    /// there is a name there.
    #[must_use]
    pub fn placeholder(&self) -> &str {
        if self.placeholder.is_empty() || self.names_a_file() {
            ""
        } else {
            &self.placeholder
        }
    }

    /// What Enter means: exactly what was typed, or the suggestion when
    /// nothing was typed after the directory.
    ///
    /// The distinction matters because the two used to be the same string in
    /// the field, and "what was typed" then included a filename the player
    /// never chose.
    #[must_use]
    pub fn resolved(&self) -> String {
        if self.names_a_file() {
            self.buffer.clone()
        } else {
            format!("{}{}", self.buffer, self.placeholder)
        }
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
    /// The help topic whose card is open, if one is.
    ///
    /// The list used to be the whole of it: Enter resolved a row by looking
    /// up its shortcut key, help topics have no shortcut, and so pressing
    /// Enter on one did nothing at all. This is what Enter opens.
    pub topic: Option<usize>,
    /// First line of that card on the screen.
    pub scroll: usize,
    /// How many of its lines fit, set from the terminal each frame so that
    /// scrolling stops where the drawing does.
    pub page_rows: usize,
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
        Self {
            open: false,
            cursor: 0,
            section: SpaceMenuSection::Actions,
            topic: None,
            scroll: 0,
            page_rows: 12,
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.cursor = 0;
            self.section = SpaceMenuSection::Actions;
        }
        self.close_topic();
    }

    pub fn move_up(&mut self) {
        if self.topic.is_some() {
            self.scroll_body(-1);
        } else if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.topic.is_some() {
            self.scroll_body(1);
            return;
        }
        let max = self.item_count();
        if self.cursor + 1 < max { self.cursor += 1; }
    }

    pub fn switch_section(&mut self) {
        if self.topic.is_some() {
            return;
        }
        self.section = match self.section {
            SpaceMenuSection::Actions => SpaceMenuSection::Help,
            SpaceMenuSection::Help => SpaceMenuSection::Actions,
        };
        self.cursor = 0;
    }

    // ── The help card ──

    /// Open the topic under the cursor. Only the help section has any.
    pub fn open_topic(&mut self) {
        if self.section != SpaceMenuSection::Help {
            return;
        }
        if self.cursor < HELP_TOPICS.len() {
            self.topic = Some(self.cursor);
            self.scroll = 0;
        }
    }

    /// Shut the card, answering whether one was open — which is what tells
    /// Esc whether it has closed the card or should close the menu.
    pub fn close_topic(&mut self) -> bool {
        self.scroll = 0;
        self.topic.take().is_some()
    }

    /// The topic being read, if any.
    #[must_use]
    pub fn open_help(&self) -> Option<&'static HelpTopic> {
        HELP_TOPICS.get(self.topic?)
    }

    /// How much of the card is off the bottom of the screen.
    #[must_use]
    pub fn scroll_max(&self) -> usize {
        self.open_help()
            .map_or(0, |topic| topic.body.len().saturating_sub(self.page_rows.max(1)))
    }

    /// Move the card under the window, stopping at both ends: a page of text
    /// that scrolls past its own last line reads as a page that has been
    /// lost.
    pub fn scroll_body(&mut self, delta: i32) {
        let max = self.scroll_max();
        self.scroll = (self.scroll as i32 + delta).clamp(0, max as i32) as usize;
    }

    /// Told the terminal's height each frame, so that "the bottom" means the
    /// same thing to the keys and to the drawing.
    pub fn set_terminal_rows(&mut self, rows: u16) {
        let body = self.open_help().map_or(0, |topic| topic.body.len());
        self.page_rows = help_page_rows(rows, body).max(1);
        let max = self.scroll_max();
        self.scroll = self.scroll.min(max);
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

// ── Help ──

/// One line of a help topic.
///
/// A reference card rather than prose: most of a topic is key-and-action
/// pairs, and the few sentences that are worth writing are the ones a key
/// table cannot say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpLine {
    /// A section inside the topic.
    Heading(&'static str),
    /// A key, and what it does.
    Key(&'static str, &'static str),
    /// A sentence.
    Note(&'static str),
    /// A blank line.
    Gap,
}

/// A help topic: what it is called, what it covers, and the card itself.
#[derive(Debug, Clone, Copy)]
pub struct HelpTopic {
    pub title: &'static str,
    pub summary: &'static str,
    pub body: &'static [HelpLine],
}

use HelpLine::{Gap, Heading, Key, Note};

/// The help topics, in the order the list shows them.
///
/// Every binding here is one that exists. They were read off the key
/// handlers, the bottom bar's own hint tables and the manual in the README
/// rather than remembered, because a help page that is confidently wrong
/// costs more than no help page at all — which is what this was: nine
/// summaries with nothing behind them and an Enter key that did nothing.
pub const HELP_TOPICS: &[HelpTopic] = &[
    HelpTopic {
        title: "navigation",
        summary: "panes, tabs, and getting back out",
        body: &[
            Heading("panes"),
            Key("spc+1 / 2 / 3", "transport \u{00B7} tracks \u{00B7} clip view"),
            Key("tab", "next pane, or next tab inside the clip view"),
            Key("shift+tab", "previous pane"),
            Key("esc", "back one level: release, deselect, leave the pane"),
            Gap,
            Heading("inside a pane"),
            Key("j / k", "up and down: tracks, notes, parameters, rows"),
            Key("h / l", "left and right: elements, steps, values"),
            Key("enter", "open or lock what the cursor is on"),
            Key("q", "quit, from the tracks and transport panes"),
            Gap,
            Note("The bottom bar always lists the keys that are live"),
            Note("where the cursor is standing."),
        ],
    },
    HelpTopic {
        title: "transport",
        summary: "play, stop, record, loop, tempo",
        body: &[
            Heading("from anywhere"),
            Key("spc+p", "play / pause"),
            Key("spc+0", "stop, and return the playhead to bar 1"),
            Key("spc+r", "arm recording (record onto armed tracks)"),
            Key("spc+m", "metronome on / off"),
            Key("spc+l", "loop region editor"),
            Key("spc+!", "panic \u{2014} kill every sounding note"),
            Key("+ / -", "tempo, one BPM at a time"),
            Gap,
            Heading("transport pane (spc+1)"),
            Key("h / l", "move between bpm, record, loop, met, count"),
            Key("enter", "bpm: hold it \u{00b7} the switches: toggle"),
            Key("esc", "release"),
            Gap,
            Heading("count-in"),
            Key("enter on count", "off \u{2192} 1 bar \u{2192} 2 bars"),
            Note("With count-in set, record armed (spc+r) and play"),
            Note("pressed \u{2014} all three \u{2014} the bars click down first."),
            Note("R counts in too. Any transport key backs out."),
            Gap,
            Heading("loop editor"),
            Key("h / l", "move the loop start"),
            Key("H / L", "move the loop end"),
            Key("enter", "loop on / off"),
            Key("esc", "done"),
            Gap,
            Heading("while recording"),
            Key("u", "scrap what you just played, keep rolling"),
            Note("Each pass round the loop is a take; u peels the"),
            Note("newest layer \u{2014} the pass under your fingers first,"),
            Note("then committed takes, one per press. ctrl+r puts a"),
            Note("peeled take back. The transport never stops."),
            Gap,
            Note("The mod wheel, pitch bend and aftertouch are"),
            Note("recorded with the notes and play back with them."),
            Note("X in the piano roll clears a clip's controller data."),
            Gap,
            Heading("automation lane"),
            Key("A", "open the lane \u{00b7} A again hands keys back"),
            Key("k / j", "draw the value up / down at the cursor"),
            Key("K / J", "the same, in bigger steps"),
            Key("h / l", "walk columns \u{00b7} the value carries along"),
            Key("[ / ]", "the controller the lane shows"),
            Key("d", "clear the point under the cursor"),
            Note("An empty clip offers mod, bend and aftertouch to"),
            Note("draw from scratch; a recorded sweep opens on its own."),
        ],
    },
    HelpTopic {
        title: "tracks",
        summary: "add, arm, mute, solo, fader, fx",
        body: &[
            Heading("the list"),
            Key("j / k", "move between tracks"),
            Key("enter", "select the track \u{2014} its instrument panel opens"),
            Key("h / l", "move between fx, fader, mute, solo, arm, clips"),
            Key("spc+a", "add an instrument track"),
            Key("spc+d", "delete the selected track or clip"),
            Gap,
            Heading("switches"),
            Key("m / s", "mute / solo"),
            Key("r", "arm for recording"),
            Key("R", "loop record"),
            Gap,
            Heading("the fader"),
            Key("enter", "hold it (on the dB reading)"),
            Key("h / l", "down and up, one dB a press"),
            Key("esc", "let go"),
            Gap,
            Heading("track fx"),
            Key("enter", "on the fx cell: choose an effect to add"),
            Key("j / k", "walk the chain in the [trk fx] tab"),
        ],
    },
    HelpTopic {
        title: "clips",
        summary: "move, stretch, trim, copy",
        body: &[
            Note("With a track selected, h/l walks its clips; the clip"),
            Note("view follows whichever one the cursor is on."),
            Gap,
            Heading("locked to a clip (enter)"),
            Key("h / l", "move it, one beat a press"),
            Key("H / L", "the right edge \u{2014} shrink and stretch"),
            Key("ctrl+h / ctrl+l", "the left edge \u{2014} trim and extend"),
            Key("y / p", "yank \u{00b7} paste after this clip"),
            Key("P", "paste onto another track, same position"),
            Key("d", "duplicate it, straight after itself"),
            Key("esc", "release"),
            Gap,
            Heading("layering"),
            Key("y on the label", "yank the whole arrangement"),
            Key("P elsewhere", "lay it down on the same bars"),
            Note("Record a part, yank the lot, put it under a second"),
            Note("instrument \u{2014} two keys, and every clip keeps its bars."),
            Gap,
            Heading("elsewhere"),
            Key("1-9", "jump to a clip by number"),
            Key("spc+d", "delete the selected clip"),
            Key("u / ctrl+r", "undo \u{00b7} redo"),
            Gap,
            Note("Clips cannot overlap: moving, stretching, trimming and"),
            Note("pasting all stop at the neighbour. Notes keep their"),
            Note("timeline positions when a clip is stretched."),
        ],
    },
    HelpTopic {
        title: "piano roll",
        summary: "write notes, select, stretch, quantize",
        body: &[
            Heading("browsing"),
            Key("h / l", "move between columns"),
            Key("j / k", "move up and down the keyboard"),
            Key("{ / }", "down / up a whole octave at a time"),
            Key("[ / ]", "snap to the nearest note below / above"),
            Key("1-9", "jump to a column"),
            Key("n", "write or erase a note at the cursor"),
            Key("enter", "select the column under the cursor"),
            Note("A clip opens framed on its notes, so you start"),
            Note("looking at the music."),
            Gap,
            Heading("selecting"),
            Key("H / L", "highlight columns left and right"),
            Key("shift+j / k", "highlight rows down and up"),
            Key("d / y / p", "delete \u{00b7} yank \u{00b7} paste the highlight"),
            Key("enter", "lock the highlight, then h/l and H/L stretch it"),
            Gap,
            Heading("one column, one note"),
            Key("h / l", "the left edge of every note in the column"),
            Key("H / L", "the right edge"),
            Key("j / k", "go deeper, to a single note"),
            Gap,
            Heading("note editing (spc+e)"),
            Key("h j k l", "move between notes by proximity"),
            Key("shift+dir", "select as you go"),
            Key("enter", "select the note under the cursor"),
            Key("h/l, j/k", "with a selection: move it"),
            Key("shift+h/l", "stretch its right edge"),
            Key(", / .", "velocity down / up \u{00b7} < > in strides"),
            Key("d", "delete \u{00b7} esc: drop the selection"),
            Key("e / esc", "leave note editing"),
            Note("Notes draw brighter the harder they were hit; the"),
            Note("header reads the cursor note's velocity out."),
            Gap,
            Key("spc+q", "quantize the clip to a grid"),
            Note("Writing the first note on an empty track makes the"),
            Note("clip; the status bar says how long it is."),
        ],
    },
    HelpTopic {
        title: "step sequencer",
        summary: "the step grid, band by band",
        body: &[
            Note("A track type that makes no sound of its own: it drives"),
            Note("a child instrument. A new one runs from birth, so"),
            Note("write steps and press play."),
            Gap,
            Heading("the grid"),
            Key("j / k", "the rows \u{2014} a kit's sounds, a synth's voices"),
            Key("h / l", "along the steps"),
            Key("n", "write or erase the step under the cursor"),
            Key("a", "accent it \u{00b7} x: clear it"),
            Key("enter", "open the panel for what the cursor is on"),
            Key("[ / ]", "previous / next row, from any depth"),
            Key("1-9", "jump to a step"),
            Gap,
            Heading("the panels (j from the last row)"),
            Key("h / l", "move between knobs"),
            Key("enter", "hold the knob \u{00b7} h/l turns it, H/L strides"),
            Key("esc", "let go, then leave the panel"),
            Note("step: pitch, chord, voicing, gate \u{00b7} lane: sound,"),
            Note("mute, solo \u{00b7} pattern: child, length, rate, swing,"),
            Note("velocities, mode, key."),
            Gap,
            Heading("patterns"),
            Key("h / l", "on the slots row: choose one of the eight"),
            Key("enter", "queue it \u{2014} the header counts it down"),
            Key("c / C", "chain the slot (again for \u{00d7}2) \u{00b7} C clears it"),
            Key("y / p", "copy a pattern to another slot"),
            Key("X", "clear the whole pattern"),
            Gap,
            Heading("playing and printing"),
            Key("t", "run / stop this pattern, and the transport"),
            Key("m / s", "mute / solo the row"),
            Key("r", "step record: a played key writes and moves on"),
            Key(". / _", "recording: a rest \u{00b7} tie the step before"),
            Key("b", "bounce the pattern or chain to a clip"),
        ],
    },
    HelpTopic {
        title: "effects",
        summary: "chains, the panels, pan and sends",
        body: &[
            Note("Six insert slots on every track, every bus and the"),
            Note("master. The chain is the [trk fx] tab; Enter on a slot"),
            Note("opens its panel beside it."),
            Gap,
            Heading("the chain"),
            Key("j / k", "move between slots"),
            Key("a", "add an effect \u{00b7} d: take one out"),
            Key("enter", "open its panel"),
            Key("b", "bypass \u{2014} the slot stays, the effect steps aside"),
            Key("[ / ]", "move the slot earlier / later in the chain"),
            Note("Order is the sound: an EQ before a compressor is not"),
            Note("the same as one after it, so nothing is ever sorted"),
            Note("for you."),
            Gap,
            Heading("the eq"),
            Key("h / l", "the eight bands (rows on a narrow terminal)"),
            Key("j / k", "the band's controls: type, freq, gain, q, slope"),
            Key("1-8", "jump to a band \u{00b7} n: switch it on or off"),
            Key("enter", "hold a control \u{00b7} h/l turns it, H/L strides"),
            Key("esc", "let go, then leave the panel"),
            Note("Frequencies walk the ISO centres, so a band reads"),
            Note("2.5k rather than 2487. Gain moves half a decibel at a"),
            Note("time and three with a stride."),
            Note("A control the band type does not use is greyed and"),
            Note("will not move \u{2014} a bell has no slope, a shelf no Q."),
            Note("The curve over the bands is drawn from the filter's"),
            Note("own response, at the rate the engine is running."),
            Gap,
            Heading("the reverb and the delay"),
            Key("j / k", "pick a knob \u{00b7} h/l turns it, H/L strides"),
            Key("enter", "hold it, so j/k stop moving \u{00b7} esc lets go"),
            Note("The delay has two axes: mode is what the repeats"),
            Note("sound like (digital, bbd, tape) and route is where"),
            Note("they go (stereo, ping-pong, mono). Either with"),
            Note("either \u{2014} a tape ping-pong is a real setting."),
            Note("Sync on follows the tempo; turning it off carries"),
            Note("the time over in milliseconds rather than jumping."),
            Note("Feedback goes past 100%: the loop is bounded by a"),
            Note("saturator, so it sings instead of running away."),
            Note("heads belongs to the tape and wander to the bbd;"),
            Note("either one greyed will not move, and says why."),
            Gap,
            Heading("pan and sends"),
            Key("h / l", "on the track row: pan, send A, send B"),
            Key("enter", "hold one \u{00b7} h/l moves it, esc lets go"),
            Note("Sends are post-fader and open from silence. The top"),
            Note("bar shows the safety limiter's reduction when the"),
            Note("mix is loud enough to need it."),
        ],
    },
    HelpTopic {
        title: "instruments",
        summary: "the [inst] panel, patches, MIDI",
        body: &[
            Note("Selecting a track opens its instrument. The [inst]"),
            Note("tab in the clip view is the whole panel, in columns;"),
            Note("the narrow [synth] strip on the left is the same"),
            Note("controls and the same cursor."),
            Gap,
            Heading("the panel"),
            Key("tab", "cycle the clip view's tabs, in this order:"),
            Note("[trk fx] [synth] \u{00b7} [inst] [piano] [settings]"),
            Note("\u{2014} and [seq] first, on a sequencer track."),
            Key("j / k", "move between controls, down each column"),
            Key("h / l", "turn the one under the cursor"),
            Key("esc", "back to the tracks pane"),
            Gap,
            Note("The first control is always the patch selector, and"),
            Note("moving it reloads the whole panel. Selectors step by"),
            Note("position; knobs move by a fraction of their travel."),
            Gap,
            Heading("playing it"),
            Note("MIDI input goes to the selected track, so choosing a"),
            Note("track is how you choose what your keyboard plays."),
            Key("spc+!", "panic, if a note ever hangs"),
        ],
    },
    HelpTopic {
        title: "presets & sessions",
        summary: "saving sounds and saving songs",
        body: &[
            Heading("instrument presets (spc+w)"),
            Key("j / k", "walk the bank"),
            Key("enter", "load the one under the cursor"),
            Key("enter", "on <save new>: name and store the panel"),
            Key("d", "delete it (it asks first)"),
            Key("esc", "close"),
            Gap,
            Heading("sessions"),
            Key("ctrl+s", "save \u{2014} straight back to the open file"),
            Key("spc+s", "save, naming the file the first time"),
            Key("spc+o", "open one"),
            Gap,
            Note("The save prompt starts on the folder with the name"),
            Note("dimmed after it: type and it is yours, or press enter"),
            Note("to take the suggestion."),
            Gap,
            Heading("where they live"),
            Note("sessions/ in a checkout, and otherwise the"),
            Note("application folder: ~/.phosphor on macOS and Linux,"),
            Note("%APPDATA%\\phosphor on Windows. Presets and the theme"),
            Note("preference live there too."),
        ],
    },
    HelpTopic {
        title: "themes",
        summary: "nine palettes, and where the choice is kept",
        body: &[
            Key("spc+v", "cycle to the next theme"),
            Gap,
            Note("Nine palettes, in this order:"),
            Note("Phosphor \u{00b7} SpaceVim \u{00b7} Gruvbox \u{00b7} Midnight"),
            Note("Dracula \u{00b7} Nord \u{00b7} Jellybean \u{00b7} Catppuccin"),
            Note("SpaceVim2"),
            Gap,
            Note("The name of the one you land on is shown on the"),
            Note("bottom bar as you cycle."),
            Gap,
            Note("The choice is written to config.json in the"),
            Note("application folder \u{2014} ~/.phosphor on macOS and"),
            Note("Linux, %APPDATA%\\phosphor on Windows \u{2014} and is read"),
            Note("back the next time Phosphor starts."),
            Gap,
            Note("Every colour on the screen comes from the palette,"),
            Note("this help page included."),
        ],
    },
    HelpTopic {
        title: "shortcuts",
        summary: "the keys that work everywhere",
        body: &[
            Heading("global"),
            Key("spc", "this menu \u{00b7} spc again, or esc, closes it"),
            Key("spc+h", "these help topics"),
            Key("tab / shift+tab", "next / previous pane"),
            Key("esc", "back one level, anywhere"),
            Key("u", "undo \u{00b7} ctrl+r: redo"),
            Key("ctrl+s", "save the session"),
            Key("ctrl+c", "quit \u{00b7} q from the tracks pane"),
            Key("+ / -", "tempo"),
            Key("spc+!", "panic"),
            Gap,
            Heading("the space menu"),
            Key("j / k", "move \u{00b7} enter: choose"),
            Key("tab", "switch between actions and help"),
            Key("<key>", "any shortcut fires straight from the menu"),
            Gap,
            Note("A held control \u{2014} a fader, a knob, a step grid knob"),
            Note("\u{2014} takes every key it is given until esc lets go."),
        ],
    },
];

/// How tall a help card is: as tall as the topic needs, capped by the
/// terminal and by [`HELP_BOX_MAX`].
///
/// Shared by the overlay that draws it and the loop that tells the menu how
/// much of a topic is on the screen, so scrolling and drawing cannot come to
/// different answers about where the bottom is.
#[must_use]
pub fn help_box_height(rows: u16, body_len: usize) -> u16 {
    let wanted = u16::try_from(body_len.saturating_add(3)).unwrap_or(HELP_BOX_MAX);
    wanted.min(HELP_BOX_MAX).min(rows.saturating_sub(2)).max(4)
}

/// The lines of a topic that fit in that card: the borders and the footer
/// are not topic.
#[must_use]
pub fn help_page_rows(rows: u16, body_len: usize) -> usize {
    (help_box_height(rows, body_len) as usize).saturating_sub(3)
}

/// The tallest the help body is allowed to be, however tall the terminal is.
const HELP_BOX_MAX: u16 = 24;
