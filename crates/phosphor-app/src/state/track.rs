//! Track state — TrackState, TrackElement, Clip, MidiNote.

use phosphor_core::project::TrackKind;

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

// ── Data Models ──

#[derive(Debug, Clone)]
pub struct Clip {
    pub number: usize,
    pub width: u16,
    pub has_content: bool,
    /// Start position on the timeline (ticks).
    pub start_tick: i64,
    /// Length in ticks.
    pub length_ticks: i64,
    /// Notes for piano roll display (from ClipSnapshot).
    pub notes: Vec<phosphor_core::clip::NoteSnapshot>,
    /// Notes hidden by shrinking the clip. Stored with start_frac and
    /// duration_frac as absolute tick ratios (tick / original_length_when_hidden)
    /// converted to tick offsets for stable restore.
    /// Format: (tick_offset_from_clip_start, duration_ticks, note, velocity)
    pub hidden_notes: Vec<(i64, i64, u8, u8)>,
}

#[derive(Debug, Clone)]
pub struct TrackState {
    pub name: String,
    pub muted: bool,
    pub soloed: bool,
    pub armed: bool,
    pub color_index: usize,
    pub kind: TrackKind,
    pub clips: Vec<Clip>,
    /// Track-level FX chain.
    pub fx_chain: Vec<super::FxInstance>,
    /// Track volume (0.0..1.0).
    pub volume: f32,
    /// Unique ID for this track (matches the mixer's track ID).
    pub mixer_id: Option<usize>,
    /// Handle to the audio engine's track state. When present, mute/solo/arm/volume
    /// writes go directly to the audio thread via atomics.
    pub handle: Option<std::sync::Arc<phosphor_core::project::TrackHandle>>,
    /// What type of instrument this track has.
    pub instrument_type: Option<super::InstrumentType>,
    /// Parameter values (mirrors the audio thread's plugin params).
    pub synth_params: Vec<f32>,
}

impl TrackState {
    pub fn new(name: &str, color_index: usize, armed: bool, kind: TrackKind, clips: Vec<Clip>) -> Self {
        Self {
            name: name.to_string(),
            muted: false,
            soloed: false,
            armed,
            color_index,
            kind,
            clips,
            fx_chain: Vec::new(),
            volume: 0.75,
            mixer_id: None,
            handle: None,
            instrument_type: None,
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
