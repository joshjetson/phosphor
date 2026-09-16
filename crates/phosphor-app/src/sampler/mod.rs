//! The sampler's UI-side truth: what is on every pad, before and after
//! the engine hears about it.
//!
//! The engine (phosphor-dsp) keeps a real-time copy delivered through
//! `MixerCommand::SetSamplerPad`; this is the editable original, the way
//! [`crate::sequencer::SequencerState`] is the original of a pattern. The
//! split matters for ownership: every PCM buffer the engine might hold is
//! also held here (or by undo history), so the audio thread's drops are
//! refcount decrements and the actual frees happen on this side.

pub mod knobs;
pub mod session;
pub mod wav;

use std::path::PathBuf;
use std::sync::Arc;

use phosphor_plugin::sample::{PadConfig, PadLayer, SamplePcm};

/// One pad per piano key, mirroring the engine.
pub const NUM_PADS: usize = 88;

/// MIDI note of the lowest pad (A0).
pub const PAD_BASE_NOTE: u8 = 21;

/// Layer slots per pad — the engine's cap, enforced here first so the
/// player hears "pad full" instead of a silently dropped ninth layer.
pub const MAX_LAYERS: usize = 8;

/// A sound stacked on a pad, plus everything the UI knows that the
/// engine does not need: where it came from, what to call it, and
/// whether the file behind it was actually there on load.
#[derive(Debug, Clone)]
pub struct LayerState {
    /// The path as the session stores it — what the player typed, not
    /// what it resolved to, so a project that moves keeps its meaning.
    pub path: PathBuf,
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

    /// How long the playable region lasts, in seconds — what the layer
    /// list prints beside its name. Zero while the file is missing: there
    /// is nothing to time.
    pub fn seconds(&self) -> f32 {
        let Some(pcm) = self.pcm.as_ref() else { return 0.0 };
        let end = self.end_frame.min(pcm.frames());
        let frames = end.saturating_sub(self.start_frame) as f32;
        frames / pcm.sample_rate.max(1.0)
    }

    /// The engine's view of this layer, or `None` while the file behind
    /// it is missing.
    pub fn engine_layer(&self) -> Option<PadLayer> {
        let pcm = self.pcm.as_ref()?;
        Some(PadLayer {
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
        })
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

#[derive(Debug, Clone, PartialEq)]
pub struct PadState {
    pub config: PadConfig,
    pub layers: Vec<LayerState>,
}

impl PadState {
    /// Whether anything on this pad was asked for and is not here — the
    /// red mark on the bed and in the list.
    pub fn has_missing(&self) -> bool {
        self.layers.iter().any(|l| l.pcm.is_none())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SamplerState {
    /// Indexed by pad — `note - PAD_BASE_NOTE`.
    pub pads: Vec<PadState>,
    /// The pad being edited. Follows the keys: playing a note on the
    /// track moves it, which on an 88-key controller is the fastest
    /// pad selector there is.
    pub cursor: usize,
}

impl Default for SamplerState {
    fn default() -> Self {
        Self::new()
    }
}

impl SamplerState {
    pub fn new() -> Self {
        Self {
            pads: (0..NUM_PADS)
                .map(|i| PadState {
                    config: PadConfig::for_key(PAD_BASE_NOTE + i as u8),
                    layers: Vec::new(),
                })
                .collect(),
            // C3 — the middle of the bed, where a hand falls.
            cursor: (60 - PAD_BASE_NOTE) as usize,
        }
    }

    /// The pad a note addresses, if it is on the bed.
    pub fn pad_of_note(note: u8) -> Option<usize> {
        let last = PAD_BASE_NOTE + (NUM_PADS as u8 - 1);
        (PAD_BASE_NOTE..=last).contains(&note).then(|| usize::from(note - PAD_BASE_NOTE))
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
        let Some(state) = self.pads.get_mut(pad) else {
            return Err("no such pad".into());
        };
        if state.layers.len() >= MAX_LAYERS {
            return Err(format!(
                "pad {} is full — eight layers is the bed",
                Self::pad_label(pad)
            ));
        }
        state.layers.push(LayerState::from_wav(path, pcm));
        Ok(())
    }

    /// The engine's copy of one pad: config plus every playable layer.
    pub fn engine_pad(&self, pad: usize) -> Option<(PadConfig, Vec<PadLayer>)> {
        let state = self.pads.get(pad)?;
        let layers = state.layers.iter().filter_map(LayerState::engine_layer).collect();
        Some((state.config, layers))
    }

    /// Pads that differ from a fresh sampler — what a session stores and
    /// what a load must replay to the engine.
    pub fn occupied_pads(&self) -> impl Iterator<Item = usize> + '_ {
        self.pads.iter().enumerate().filter_map(|(i, p)| {
            let fresh = PadConfig::for_key(Self::note_of_pad(i));
            (!p.layers.is_empty() || p.config != fresh).then_some(i)
        })
    }

    /// Layers whose file was not found on load.
    pub fn missing_layers(&self) -> usize {
        self.pads
            .iter()
            .flat_map(|p| p.layers.iter())
            .filter(|l| l.pcm.is_none())
            .count()
    }

    /// Bytes of PCM held, counting a buffer shared by several layers
    /// once. The count is for the player's memory line, not the
    /// allocator's truth, so pointer identity is enough.
    pub fn pcm_bytes(&self) -> usize {
        let mut seen: Vec<*const SamplePcm> = Vec::new();
        let mut total = 0usize;
        for layer in self.pads.iter().flat_map(|p| p.layers.iter()) {
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
        let (_, layers) = s.engine_pad(5).unwrap();
        assert_eq!(layers.len(), 1, "a missing file must not reach the engine");
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

    #[test]
    fn an_edited_config_counts_as_occupied_even_with_no_layers() {
        let mut s = SamplerState::new();
        s.pads[10].config.choke = 3;
        let occupied: Vec<usize> = s.occupied_pads().collect();
        assert_eq!(occupied, vec![10]);
    }
}
