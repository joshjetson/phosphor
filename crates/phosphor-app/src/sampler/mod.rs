//! The sampler's UI-side truth: what is on every pad, before and after
//! the engine hears about it.
//!
//! The engine (phosphor-dsp) keeps a real-time copy delivered through
//! `MixerCommand::SetSamplerPad`; this is the editable original, the way
//! [`crate::sequencer::SequencerState`] is the original of a pattern. The
//! split matters for ownership: every PCM buffer the engine might hold is
//! also held here (or by undo history), so the audio thread's drops are
//! refcount decrements and the actual frees happen on this side.

pub mod capture;
pub mod knobs;
pub mod phrase;
pub mod render;
pub mod root;
pub mod session;
pub mod sidecar;
pub mod trim;
pub mod wav;
pub mod zones;

use std::path::PathBuf;
use std::sync::Arc;

use phosphor_plugin::sample::{PadConfig, PadLayer, PadPhrase, SamplePcm};

use crate::state::InstrumentType;

pub use phrase::{PadRow, PhraseState, RowKind, TakeKind, MAX_PHRASES};
pub use zones::{MapMode, Zone, ZoneEdge};

/// The pad geometry, shared with the engine rather than mirrored.
///
/// Re-exported from the interface crate both sides already depend on, so the
/// link is the compiler's: two constants that agree today are two constants
/// that disagree the day one of them is edited, and the symptom is the top or
/// the bottom of the bed going quiet. [`MAX_LAYERS`] is enforced here first
/// all the same, so the player hears "pad full" instead of a silently dropped
/// ninth layer.
pub use phosphor_plugin::sample::{MAX_LAYERS, NUM_PADS, PAD_BASE_NOTE};

/// Where a layer's audio came from.
///
/// The difference is what a session save has to do about it: a file layer
/// is a reference to something the player already owns, and a take is
/// audio that exists nowhere else until the save writes it out. It is also
/// what the list calls the row, so a player can tell a loaded kick from
/// one they played.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayerSource {
    /// A file on disk the player named. The default, so that every
    /// session written before takes existed reads back unchanged.
    #[default]
    File,
    /// A performance recorded through one of our own instruments and
    /// rendered here. Its `path` is empty until a save gives it one.
    Take,
}

/// A sound stacked on a pad, plus everything the UI knows that the
/// engine does not need: where it came from, what to call it, and
/// whether the file behind it was actually there on load.
#[derive(Debug, Clone)]
pub struct LayerState {
    /// The path as the session stores it — what the player typed, not
    /// what it resolved to, so a project that moves keeps its meaning.
    /// Empty on a take that has never been saved.
    pub path: PathBuf,
    pub source: LayerSource,
    /// Display name: the file stem.
    pub name: String,
    /// The decoded audio, or `None` when the file was missing on load.
    /// A missing layer keeps its seat and its settings — dropping it
    /// would punish the player for moving a folder.
    pub pcm: Option<Arc<SamplePcm>>,
    pub gain: f32,
    pub pan: f32,
    pub tune_st: i8,
    pub tune_cents: i8,
    pub start_frame: u64,
    pub end_frame: u64,
    pub reverse: bool,
    pub mute: bool,
    pub vel_lo: u8,
    pub vel_hi: u8,
}

impl LayerState {
    /// A fresh layer over the whole of `pcm`, at unity.
    pub fn from_wav(path: PathBuf, pcm: Arc<SamplePcm>) -> Self {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sample".into());
        let end_frame = pcm.frames();
        Self {
            path,
            source: LayerSource::File,
            name,
            pcm: Some(pcm),
            gain: 1.0,
            pan: 0.0,
            tune_st: 0,
            tune_cents: 0,
            start_frame: 0,
            end_frame,
            reverse: false,
            mute: false,
            vel_lo: 0,
            vel_hi: 127,
        }
    }

    /// A layer holding a take that was just rendered, trimmed where the
    /// render said and named for the pad it landed on.
    ///
    /// No path: a take exists in memory and nowhere else until a session
    /// save writes it into the sidecar beside the file. See
    /// [`super::sidecar`].
    pub fn from_take(name: String, take: &render::RenderedTake) -> Self {
        Self {
            path: PathBuf::new(),
            source: LayerSource::Take,
            name,
            pcm: Some(Arc::clone(&take.pcm)),
            gain: 1.0,
            pan: 0.0,
            tune_st: 0,
            tune_cents: 0,
            start_frame: take.start_frame,
            end_frame: take.end_frame,
            reverse: false,
            mute: false,
            vel_lo: 0,
            vel_hi: 127,
        }
    }

    /// The loudest sample in the region that plays, linear — what a
    /// normalize measures itself against. Zero for a layer with no audio
    /// behind it.
    pub fn region_peak(&self) -> f32 {
        let (Some((start, end)), Some(pcm)) = (self.region(), self.pcm.as_ref()) else {
            return 0.0;
        };
        let channels = usize::from(pcm.channels.max(1));
        let (from, to) = (start as usize * channels, (end as usize * channels).min(pcm.data.len()));
        render::peak_of(&pcm.data[from.min(to)..to])
    }

    /// How long the playable region lasts, in seconds — what the layer
    /// list prints beside its name. Zero while the file is missing: there
    /// is nothing to time.
    pub fn seconds(&self) -> f32 {
        let (Some((start, end)), Some(pcm)) = (self.region(), self.pcm.as_ref()) else {
            return 0.0;
        };
        (end - start) as f32 / pcm.sample_rate.max(1.0)
    }

    /// The engine's view of this layer, or `None` while the file behind
    /// it is missing.
    ///
    /// Clamped on the way out, through the table the engine clamps with. The
    /// app's one door to a [`PadLayer`] is also the one place it can promise
    /// a number the pad will not play, so it promises nothing the engine
    /// would quietly change its mind about.
    pub fn engine_layer(&self) -> Option<PadLayer> {
        let pcm = self.pcm.as_ref()?;
        Some(phosphor_plugin::sample::clamp_layer(PadLayer {
            pcm: Arc::clone(pcm),
            gain: self.gain,
            pan: self.pan,
            tune_st: self.tune_st,
            tune_cents: self.tune_cents,
            start_frame: self.start_frame,
            end_frame: self.end_frame,
            reverse: self.reverse,
            mute: self.mute,
            vel_lo: self.vel_lo,
            vel_hi: self.vel_hi,
        }))
    }
}

/// Two layers are the same layer when they point at the same buffer and
/// carry the same settings.
///
/// Identity on the PCM, never contents: undo compares slices of this state
/// on every commit, and comparing two takes sample by sample would walk
/// megabytes to answer a question the pointer already answers. Two distinct
/// decodes of one file are "different" under this rule, which costs one
/// harmless undo step and never a missed one.
impl PartialEq for LayerState {
    fn eq(&self, other: &Self) -> bool {
        let same_pcm = match (&self.pcm, &other.pcm) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        same_pcm
            && self.path == other.path
            && self.source == other.source
            && self.name == other.name
            && self.gain == other.gain
            && self.pan == other.pan
            && self.tune_st == other.tune_st
            && self.tune_cents == other.tune_cents
            && self.start_frame == other.start_frame
            && self.end_frame == other.end_frame
            && self.reverse == other.reverse
            && self.mute == other.mute
            && self.vel_lo == other.vel_lo
            && self.vel_hi == other.vel_hi
    }
}

/// What a pad was last recorded from: the instrument, and the panel it
/// was played with.
///
/// Remembered per pad so that a second take of the same sound is one key
/// press away — `i` reopens the picker already standing on it, and the
/// render replays exactly these numbers into a fresh instance. It goes
/// into the session with the pad, because "record another one like that"
/// is a thing a player wants a week later.
#[derive(Debug, Clone, PartialEq)]
pub struct PadSource {
    pub instrument: InstrumentType,
    /// The instrument's whole panel, in its own order.
    pub params: Vec<f32>,
}

/// What pointing the sampler's child at an instrument actually changed.
///
/// Three answers rather than a bool, because the player is owed different
/// words for each: a new instrument changes how every phrase on the kit
/// sounds, a new panel on the same instrument changes it more quietly, and
/// nothing changed is worth no words and no command at all — a `SetInstrument`
/// on the audio thread for a child that is already right would cut whatever
/// was playing through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildChange {
    Same,
    /// The same instrument, carrying the panel this phrase was played on.
    Panel,
    Instrument,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PadState {
    pub config: PadConfig,
    pub layers: Vec<LayerState>,
    /// Performances kept as notes, played through the sampler's one child
    /// instrument. They stack after the layers in the sound list — see
    /// [`PadState::rows`].
    pub phrases: Vec<PhraseState>,
    /// The instrument this pad was last recorded from, when it has been.
    pub source: Option<PadSource>,
    /// What `r` lands here: audio, or the performance itself. Remembered
    /// with the source, because "record another one like that" means the
    /// shape of the take as well as the instrument.
    pub take: TakeKind,
}

impl PadState {
    /// A seat with nothing in it: what a fresh pad is, what a zone starts
    /// as, and what a key no zone covers plays.
    pub fn empty(note: u8) -> Self {
        Self {
            config: PadConfig::for_key(note),
            layers: Vec::new(),
            phrases: Vec::new(),
            source: None,
            take: TakeKind::Audio,
        }
    }

    /// Whether anything on this pad was asked for and is not here — the
    /// red mark on the bed and in the list.
    pub fn has_missing(&self) -> bool {
        self.layers.iter().any(|l| l.pcm.is_none())
    }

    /// Takes already on this pad — what the next one is numbered after.
    /// Counted rather than stored: a take removed and undone back onto the
    /// pad would otherwise reuse a number that is already in the list.
    pub fn take_count(&self) -> usize {
        self.layers.iter().filter(|l| l.source == LayerSource::Take).count()
    }

    /// Stack one more sound, if the bed has room for it.
    ///
    /// `title` is what the refusal calls this place — "pad C3", "zone
    /// C2-B3" — because a pad and a zone are full in the same words and
    /// named in different ones.
    fn push_layer(&mut self, layer: LayerState, title: &str) -> Result<usize, String> {
        if self.layers.len() >= MAX_LAYERS {
            return Err(SamplerState::full_message(title));
        }
        self.layers.push(layer);
        Ok(self.layers.len() - 1)
    }

    /// Put a decoded WAV here. `Err` is a status-bar sentence and nothing
    /// is touched.
    pub fn add_wav(
        &mut self,
        path: PathBuf,
        pcm: Arc<SamplePcm>,
        title: &str,
    ) -> Result<usize, String> {
        self.push_layer(LayerState::from_wav(path, pcm), title)
    }

    /// Put a rendered take here, named for its place in the stack.
    ///
    /// The root a one-pitch performance teaches is *not* set here: in keys
    /// mode it belongs to the zone, and one door for all three ways a root
    /// arrives is [`SamplerState::set_edit_root`].
    pub fn add_take(
        &mut self,
        take: &render::RenderedTake,
        title: &str,
    ) -> Result<usize, String> {
        let name = format!("take {}", self.take_count() + 1);
        self.push_layer(LayerState::from_take(name, take), title)
    }
}

/// Where a layer sits.
///
/// Two places, because a sound can be on a pad or in a zone, and the things
/// that walk every layer in the kit — the sidecar writing takes out, a
/// count of what is missing — have to be able to name either and come back
/// to it. See [`SamplerState::layer_at`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerAddr {
    Pad { pad: usize, layer: usize },
    Zone { zone: usize, layer: usize },
}

impl LayerAddr {
    /// Its place in the stack it sits in.
    pub fn layer(self) -> usize {
        match self {
            Self::Pad { layer, .. } | Self::Zone { layer, .. } => layer,
        }
    }
}

/// One key as the engine plays it: everything
/// [`SamplerState::sync`](crate::sampler::SamplerState) has to hand over
/// for one pad, materialized once.
///
/// One struct rather than three calls, because in keys mode every one of
/// them is built by stacking the zones over the key, and asking three times
/// would stack them three times a keystroke.
#[derive(Debug, Clone)]
pub struct EnginePad {
    pub config: PadConfig,
    pub layers: Vec<PadLayer>,
    pub phrases: Vec<PadPhrase>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SamplerState {
    /// Indexed by pad — `note - PAD_BASE_NOTE`.
    pub pads: Vec<PadState>,
    /// The one instrument every phrase on this sampler plays through, and
    /// the panel it plays with.
    ///
    /// At sampler level rather than per pad because that is what it is: one
    /// child, rendered once a block however many phrases are running. It is
    /// set by the landing of a phrase, from the pad's remembered source —
    /// the player never picks it twice.
    pub child: Option<PadSource>,
    /// The pad being edited. Follows the keys: playing a note on the
    /// track moves it, which on an 88-key controller is the fastest
    /// pad selector there is.
    pub cursor: usize,
    /// What the eighty-eight keys mean: a pad each, or zones. Both truths
    /// live here at once — see [`zones`].
    pub mode: MapMode,
    /// The zones, in edge order. Empty in pads mode, and empty in keys
    /// mode until the player makes one.
    pub zones: Vec<Zone>,
}

impl Default for SamplerState {
    fn default() -> Self {
        Self::new()
    }
}

impl SamplerState {
    pub fn new() -> Self {
        Self {
            pads: (0..NUM_PADS).map(|i| PadState::empty(PAD_BASE_NOTE + i as u8)).collect(),
            // C3 — the middle of the bed, where a hand falls.
            cursor: (60 - PAD_BASE_NOTE) as usize,
            mode: MapMode::Pads,
            zones: Vec::new(),
            child: None,
        }
    }

    /// Point the sampler's one child at an instrument and its panel, and
    /// say what that changed.
    ///
    /// The whole answer in one door, because every caller has to tell the
    /// player the same thing: there is one child, and recording a phrase
    /// from something else replaces it for every phrase on the kit.
    pub fn set_child(&mut self, source: PadSource) -> ChildChange {
        let change = match self.child.as_ref() {
            Some(child) if child.instrument != source.instrument => ChildChange::Instrument,
            Some(child) if child.params != source.params => ChildChange::Panel,
            Some(_) => ChildChange::Same,
            None => ChildChange::Instrument,
        };
        if change != ChildChange::Same {
            self.child = Some(source);
        }
        change
    }

    /// The pad a note addresses, if it is on the bed.
    ///
    /// The engine's own map, reached by the name the app already calls it by
    /// — this was the same arithmetic written twice, and nothing linked the
    /// two.
    pub fn pad_of_note(note: u8) -> Option<usize> {
        phosphor_plugin::sample::pad_index(note)
    }

    /// The MIDI note of a pad.
    pub fn note_of_pad(pad: usize) -> u8 {
        PAD_BASE_NOTE + pad.min(NUM_PADS - 1) as u8
    }

    /// The pad's name on a keyboard — "C3", "A#1".
    pub fn pad_label(pad: usize) -> String {
        crate::format::note_name(Self::note_of_pad(pad))
    }

    /// The pad under the cursor. Always a pad: the bed is 88 seats that
    /// exist whether or not anything is sitting on them, and a cursor that
    /// somehow walked off the end is pulled back to the top key rather than
    /// answering `None` to every caller.
    pub fn current(&self) -> &PadState {
        &self.pads[self.cursor.min(NUM_PADS - 1)]
    }

    pub fn current_mut(&mut self) -> &mut PadState {
        &mut self.pads[self.cursor.min(NUM_PADS - 1)]
    }

    /// Walk the cursor along the bed, stopping at both ends. Wrapping would
    /// turn one press too many at the top of the keyboard into a jump to
    /// the bottom, which reads as the cursor having been lost.
    pub fn move_cursor(&mut self, delta: i32) -> usize {
        self.cursor =
            (self.cursor as i32 + delta).clamp(0, NUM_PADS as i32 - 1) as usize;
        self.cursor
    }

    /// Put a decoded WAV on a pad. `Err` is a status-bar sentence.
    pub fn add_wav_layer(
        &mut self,
        pad: usize,
        path: PathBuf,
        pcm: Arc<SamplePcm>,
    ) -> Result<(), String> {
        let title = Self::pad_title(pad);
        let Some(state) = self.pads.get_mut(pad) else {
            return Err("no such pad".into());
        };
        state.add_wav(path, pcm, &title).map(|_| ())
    }

    /// Put a rendered take on a pad, named for its place in the stack.
    /// `Err` is a status-bar sentence, and the take is untouched.
    pub fn add_take_layer(
        &mut self,
        pad: usize,
        take: &render::RenderedTake,
    ) -> Result<usize, String> {
        let title = Self::pad_title(pad);
        let Some(state) = self.pads.get_mut(pad) else {
            return Err("no such pad".into());
        };
        let index = state.add_take(take, &title)?;
        // A take teaches the pad its root when the performance was one
        // pitch. Keytrack is left alone: a root is a fact about the
        // recording, and whether the pad should transpose is a decision.
        if let Some(root) = take.root {
            state.config.root = root;
        }
        Ok(index)
    }

    /// Put a decoded WAV on whatever the cursor is editing — the pad in
    /// pads mode, the zone's own sound in keys mode.
    pub fn add_wav_here(
        &mut self,
        path: PathBuf,
        pcm: Arc<SamplePcm>,
    ) -> Result<usize, String> {
        let title = self.edit_title();
        self.edited_mut().ok_or_else(Self::no_zone_message)?.add_wav(path, pcm, &title)
    }

    /// Put a rendered take on whatever the cursor is editing, and let it
    /// teach its root when the performance was one pitch.
    pub fn add_take_here(&mut self, take: &render::RenderedTake) -> Result<usize, String> {
        let title = self.edit_title();
        let index = self.edited_mut().ok_or_else(Self::no_zone_message)?.add_take(take, &title)?;
        if let Some(root) = take.root {
            self.set_edit_root(root);
        }
        Ok(index)
    }

    /// Teach a root to whatever the cursor is editing.
    ///
    /// One door, because a root arrives three ways — a capture, a file
    /// name, a key played — and all three have to land in the same place or
    /// two of them are silently wrong in one of the two modes. Answers
    /// whether it moved, which is what the flash is for.
    pub fn set_edit_root(&mut self, root: u8) -> bool {
        match self.edited_mut() {
            Some(state) if state.config.root != root => {
                state.config.root = root;
                true
            }
            _ => false,
        }
    }

    /// Whether a pad has room for one more sound — the refusal both
    /// doors give, in the same words.
    pub fn room_on(&self, pad: usize) -> Result<(), String> {
        let Some(state) = self.pads.get(pad) else {
            return Err("no such pad".into());
        };
        if state.layers.len() >= MAX_LAYERS {
            return Err(Self::full_message(&Self::pad_title(pad)));
        }
        Ok(())
    }

    /// The same question about whatever the cursor is editing.
    pub fn room_here(&self) -> Result<(), String> {
        match self.edited() {
            None => Err(Self::no_zone_message()),
            Some(state) if state.layers.len() >= MAX_LAYERS => {
                Err(Self::full_message(&self.edit_title()))
            }
            Some(_) => Ok(()),
        }
    }

    /// "pad C3" — what a flash and a refusal call a pad.
    pub fn pad_title(pad: usize) -> String {
        format!("pad {}", Self::pad_label(pad))
    }

    /// What every door says when there is no room left, whether it is a
    /// pad or a zone that is full.
    pub fn full_message(title: &str) -> String {
        Self::bed_is_full(title, "eight layers")
    }

    /// The shape of every "no room" sentence in the sampler. One place, so
    /// that a pad, a zone and a phrase bed all refuse in the same words and
    /// only the count changes.
    pub(crate) fn bed_is_full(title: &str, bed: &str) -> String {
        format!("{title} is full \u{2014} {bed} is the bed")
    }

    /// What every door says when keys mode has no zone under the cursor —
    /// with the three keys that make one, because a refusal that does not
    /// say what to press instead is a dead end.
    pub fn no_zone_message() -> String {
        "no zone on this key \u{00b7} w covers the bed \u{00b7} o the octave \u{00b7} s splits"
            .into()
    }

    /// The engine's copy of one key: config, every playable layer, and
    /// every phrase.
    ///
    /// [`Self::voice`] is what decides *what* the key plays, so keys mode
    /// arrives here materialized and the engine is handed eighty-eight
    /// pads either way — it has never heard of a zone.
    pub fn engine_pad(&self, pad: usize) -> Option<EnginePad> {
        if pad >= NUM_PADS {
            return None;
        }
        let state = self.voice(pad);
        Some(EnginePad {
            config: state.config,
            layers: state.layers.iter().filter_map(LayerState::engine_layer).collect(),
            phrases: state.phrases.iter().map(PhraseState::engine_phrase).collect(),
        })
    }

    /// Pads that differ from a fresh sampler — what a session stores and
    /// what a load must replay to the engine.
    ///
    /// A pad that remembers an instrument counts even with nothing on it:
    /// the player picked a source and walked away, and losing that on save
    /// would lose the setup for the take they were about to record.
    pub fn occupied_pads(&self) -> impl Iterator<Item = usize> + '_ {
        self.pads.iter().enumerate().filter_map(|(i, p)| {
            let fresh = PadConfig::for_key(Self::note_of_pad(i));
            let touched = !p.layers.is_empty()
                || !p.phrases.is_empty()
                || p.config != fresh
                || p.source.is_some()
                || p.take != TakeKind::Audio;
            touched.then_some(i)
        })
    }

    /// Every take in the kit — what a session save has to write out before
    /// it writes the file that names them.
    ///
    /// The zones' sounds as well as the pads', because a take recorded into
    /// a zone is a performance that exists nowhere else either.
    pub fn takes(&self) -> impl Iterator<Item = LayerAddr> + '_ {
        let pads = self.pads.iter().enumerate().flat_map(|(pad, state)| {
            takes_in(state).map(move |layer| LayerAddr::Pad { pad, layer })
        });
        let zones = self.zones.iter().enumerate().flat_map(|(zone, state)| {
            takes_in(&state.pad).map(move |layer| LayerAddr::Zone { zone, layer })
        });
        pads.chain(zones)
    }

    /// One layer by address, or `None` when the address is stale.
    pub fn layer_at(&self, addr: LayerAddr) -> Option<&LayerState> {
        match addr {
            LayerAddr::Pad { pad, layer } => self.pads.get(pad)?.layers.get(layer),
            LayerAddr::Zone { zone, layer } => self.zones.get(zone)?.pad.layers.get(layer),
        }
    }

    pub fn layer_at_mut(&mut self, addr: LayerAddr) -> Option<&mut LayerState> {
        match addr {
            LayerAddr::Pad { pad, layer } => self.pads.get_mut(pad)?.layers.get_mut(layer),
            LayerAddr::Zone { zone, layer } => {
                self.zones.get_mut(zone)?.pad.layers.get_mut(layer)
            }
        }
    }

    /// The key a layer's address is named after, which is what a take's
    /// file on disk is called: the pad's own key, or the zone's first one.
    pub fn addr_label(&self, addr: LayerAddr) -> String {
        match addr {
            LayerAddr::Pad { pad, .. } => Self::pad_label(pad),
            LayerAddr::Zone { zone, .. } => {
                Self::pad_label(self.zones.get(zone).map_or(0, |z| z.lo))
            }
        }
    }

    /// Every sound in the kit, wherever it sits — the pads' and the zones'
    /// both, because a zone's layers are as real as a pad's.
    pub fn all_layers(&self) -> impl Iterator<Item = &LayerState> + '_ {
        self.pads
            .iter()
            .chain(self.zones.iter().map(|z| &z.pad))
            .flat_map(|state| state.layers.iter())
    }

    /// Layers whose file was not found on load.
    pub fn missing_layers(&self) -> usize {
        self.all_layers().filter(|l| l.pcm.is_none()).count()
    }

    /// Bytes of PCM held, counting a buffer shared by several layers
    /// once. The count is for the player's memory line, not the
    /// allocator's truth, so pointer identity is enough.
    pub fn pcm_bytes(&self) -> usize {
        let mut seen: Vec<*const SamplePcm> = Vec::new();
        let mut total = 0usize;
        for layer in self.all_layers() {
            if let Some(pcm) = layer.pcm.as_ref() {
                let ptr = Arc::as_ptr(pcm);
                if !seen.contains(&ptr) {
                    seen.push(ptr);
                    total += pcm.data.len() * core::mem::size_of::<f32>();
                }
            }
        }
        total
    }
}

/// Which of a sound's layers are takes, by index.
fn takes_in(state: &PadState) -> impl Iterator<Item = usize> + '_ {
    state
        .layers
        .iter()
        .enumerate()
        .filter(|(_, l)| l.source == LayerSource::Take)
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(frames: usize) -> Arc<SamplePcm> {
        Arc::new(SamplePcm { data: vec![0.1; frames], channels: 1, sample_rate: 44_100.0 })
    }

    #[test]
    fn a_fresh_sampler_sits_at_c3_with_empty_pads() {
        let s = SamplerState::new();
        assert_eq!(s.pads.len(), NUM_PADS);
        assert_eq!(SamplerState::note_of_pad(s.cursor), 60);
        assert_eq!(s.occupied_pads().count(), 0);
        assert_eq!(s.pcm_bytes(), 0);
    }

    #[test]
    fn pad_labels_read_like_a_keyboard() {
        assert_eq!(SamplerState::pad_label(0), "A-1"); // note 21, C3 = 60
        assert_eq!(SamplerState::pad_label((60 - 21) as usize), "C3");
        assert_eq!(SamplerState::pad_label(87), "C7"); // note 108
    }

    #[test]
    fn the_ninth_layer_is_refused_in_words() {
        let mut s = SamplerState::new();
        for _ in 0..MAX_LAYERS {
            s.add_wav_layer(0, PathBuf::from("k.wav"), pcm(10)).unwrap();
        }
        let err = s.add_wav_layer(0, PathBuf::from("k.wav"), pcm(10)).unwrap_err();
        assert!(err.contains("full"), "{err}");
        assert_eq!(s.pads[0].layers.len(), MAX_LAYERS);
    }

    #[test]
    fn a_missing_layer_keeps_its_seat_but_stays_out_of_the_engine() {
        let mut s = SamplerState::new();
        s.add_wav_layer(5, PathBuf::from("a.wav"), pcm(100)).unwrap();
        s.add_wav_layer(5, PathBuf::from("gone.wav"), pcm(100)).unwrap();
        s.pads[5].layers[1].pcm = None;
        assert_eq!(
            s.engine_pad(5).unwrap().layers.len(),
            1,
            "a missing file must not reach the engine",
        );
        assert_eq!(s.pads[5].layers.len(), 2, "and must not lose its seat");
        assert_eq!(s.missing_layers(), 1);
    }

    #[test]
    fn shared_buffers_count_once_in_the_memory_line() {
        let mut s = SamplerState::new();
        let shared = pcm(1_000); // 4 kB
        s.add_wav_layer(0, PathBuf::from("a.wav"), Arc::clone(&shared)).unwrap();
        s.add_wav_layer(1, PathBuf::from("a.wav"), shared).unwrap();
        assert_eq!(s.pcm_bytes(), 4_000);
    }

    fn take(peak: f32, root: Option<u8>) -> render::RenderedTake {
        let pcm = Arc::new(SamplePcm {
            data: vec![peak, -peak, 0.0, 0.0],
            channels: 2,
            sample_rate: 44_100.0,
        });
        render::RenderedTake { start_frame: 0, end_frame: 2, peak, root, pcm }
    }

    /// Takes are numbered by what is on the pad, not by a counter that
    /// forgets: a take removed and a new one recorded must not both be
    /// called "take 2".
    #[test]
    fn a_take_is_named_for_its_place_in_the_stack() {
        let mut s = SamplerState::new();
        assert_eq!(s.add_take_layer(0, &take(0.5, None)).unwrap(), 0);
        s.add_take_layer(0, &take(0.5, None)).unwrap();
        let names: Vec<&str> = s.pads[0].layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["take 1", "take 2"]);
        s.pads[0].layers.remove(0);
        s.add_take_layer(0, &take(0.5, None)).unwrap();
        assert_eq!(s.pads[0].layers[1].name, "take 2");
        assert_eq!(s.takes().count(), 2);
    }

    /// A one-pitch performance teaches the pad its root; keytrack is left
    /// alone, because a root is a fact about the recording and
    /// keytracking is a decision about the pad.
    #[test]
    fn a_take_teaches_its_root_and_nothing_else() {
        let mut s = SamplerState::new();
        let before = s.pads[0].config;
        s.add_take_layer(0, &take(0.5, Some(64))).unwrap();
        assert_eq!(s.pads[0].config.root, 64);
        assert_eq!(s.pads[0].config.keytrack, before.keytrack);
        // A chord has no root to teach, and leaves the last one standing.
        s.add_take_layer(0, &take(0.5, None)).unwrap();
        assert_eq!(s.pads[0].config.root, 64);
    }

    #[test]
    fn a_full_pad_refuses_a_take_in_the_same_words_as_a_file() {
        let mut s = SamplerState::new();
        for _ in 0..MAX_LAYERS {
            s.add_wav_layer(3, PathBuf::from("k.wav"), pcm(10)).unwrap();
        }
        let err = s.add_take_layer(3, &take(0.5, None)).unwrap_err();
        assert!(err.contains("full"), "{err}");
        assert_eq!(s.pads[3].layers.len(), MAX_LAYERS);
    }

    /// A pad that remembers an instrument is not a fresh pad, even with
    /// nothing on it: the session has to keep the setup.
    #[test]
    fn a_remembered_source_counts_as_occupied() {
        let mut s = SamplerState::new();
        s.pads[7].source = Some(PadSource {
            instrument: InstrumentType::DX7,
            params: vec![0.5; 4],
        });
        assert_eq!(s.occupied_pads().collect::<Vec<_>>(), vec![7]);
    }

    /// A pad carrying nothing but a performance is an occupied pad: a
    /// session that left it out would lose the performance, and a resync
    /// that skipped it would leave the engine playing the old one.
    #[test]
    fn a_pad_with_only_a_phrase_counts_as_occupied() {
        let mut s = SamplerState::new();
        let events: Arc<[phosphor_plugin::sample::PhraseEvent]> =
            Arc::from(vec![phosphor_plugin::sample::PhraseEvent {
                frame: 0,
                status: 0x90,
                data1: 60,
                data2: 100,
            }]);
        s.pads[4].add_phrase(Arc::clone(&events), 500, 0.0, "pad").unwrap();
        assert_eq!(s.occupied_pads().collect::<Vec<_>>(), vec![4]);
        assert!(s.sounding_pads().contains(&4));

        // ...and the engine is handed it beside the layers, pointing at the
        // same event list rather than a copy of it.
        let engine = s.engine_pad(4).unwrap();
        assert_eq!(engine.phrases.len(), 1);
        assert!(Arc::ptr_eq(&engine.phrases[0].events, &events));
        assert!(engine.layers.is_empty());
        assert!(s.engine_pad(5).unwrap().phrases.is_empty());
    }

    /// A pad that is only *armed* for phrases counts too: the player chose
    /// it and walked away, and losing that on save loses the setup.
    #[test]
    fn a_pad_armed_for_phrases_counts_as_occupied() {
        let mut s = SamplerState::new();
        s.pads[9].take = TakeKind::Phrase;
        assert_eq!(s.occupied_pads().collect::<Vec<_>>(), vec![9]);
    }

    /// One child per sampler, and the player is told what changed: a new
    /// instrument, a new panel on the same one, or nothing at all — which
    /// must not send a rebuild, because a rebuild cuts what is playing.
    #[test]
    fn the_child_says_what_pointing_it_somewhere_changed() {
        let mut s = SamplerState::new();
        assert!(s.child.is_none());
        let dx7 = PadSource { instrument: InstrumentType::DX7, params: vec![0.5, 0.5] };
        assert_eq!(s.set_child(dx7.clone()), ChildChange::Instrument);
        assert_eq!(s.child.as_ref().unwrap().instrument, InstrumentType::DX7);

        assert_eq!(s.set_child(dx7.clone()), ChildChange::Same, "an unchanged child rebuilt");
        let tweaked = PadSource { params: vec![0.5, 0.9], ..dx7 };
        assert_eq!(s.set_child(tweaked.clone()), ChildChange::Panel);
        assert_eq!(s.child.as_ref().unwrap().params, vec![0.5, 0.9]);

        let rhodes = PadSource { instrument: InstrumentType::Rhodes, params: vec![0.1] };
        assert_eq!(s.set_child(rhodes), ChildChange::Instrument);
        assert_eq!(s.child.as_ref().unwrap().instrument, InstrumentType::Rhodes);
        assert_eq!(s.set_child(tweaked), ChildChange::Instrument, "the swap back was silent");
    }

    #[test]
    fn an_edited_config_counts_as_occupied_even_with_no_layers() {
        let mut s = SamplerState::new();
        s.pads[10].config.choke = 3;
        let occupied: Vec<usize> = s.occupied_pads().collect();
        assert_eq!(occupied, vec![10]);
    }
}
