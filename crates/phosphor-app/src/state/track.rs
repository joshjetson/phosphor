//! Track state — TrackState, TrackElement, Clip, MidiNote.

use phosphor_core::project::{TrackConfig, TrackKind};

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
    /// Fader position as a linear gain, mirroring the audio thread's
    /// `TrackConfig::volume`. Travel is
    /// [`TrackConfig::MIN_VOLUME`]..=[`TrackConfig::MAX_VOLUME`]; the audio
    /// thread's copy is clamped to it by `TrackConfig::set_volume`, so this
    /// mirror is the only place an out-of-range value could survive.
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
    /// One press of the fader, in dB.
    ///
    /// The fader is stepped in dB rather than in linear gain so that every
    /// press moves the readout by exactly one. A linear step small enough to
    /// be useful near the top of the travel is a 6 dB jump near the bottom,
    /// and three consecutive linear detents around unity all round to the
    /// same displayed number — a control that does not appear to respond.
    pub const VOLUME_STEP_DB: f32 = 1.0;

    /// Bottom of the fader's travel, below which it goes to silence.
    ///
    /// −40 dB rather than the −60 a drawn fader usually bottoms out at: this
    /// one is stepped a keypress at a time, and 20 extra presses to reach a
    /// level that is inaudible anyway is travel nobody wants. Muting a track
    /// outright is `m`.
    pub const VOLUME_FLOOR_DB: f32 = -40.0;

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
            volume: TrackConfig::DEFAULT_VOLUME,
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

    /// The fader position in dB relative to unity. `None` at the bottom of
    /// the travel, where the answer is negative infinity.
    pub fn volume_db(&self) -> Option<f32> {
        (self.volume > 0.0).then(|| 20.0 * self.volume.log10())
    }

    /// Move the fader by `steps` presses and push the result to the audio
    /// thread. Returns the new linear gain.
    ///
    /// The current position is rounded onto the dB grid before stepping, so
    /// the fader self-corrects: the default of 0.75 is −2.5 dB, off the grid,
    /// and the first press lands it on −2 or −3 and every press after that
    /// moves exactly one. A session saved with a hand-edited volume snaps the
    /// same way.
    pub fn adjust_volume(&mut self, steps: i32) -> f32 {
        let top_db = 20.0 * TrackConfig::MAX_VOLUME.log10();
        // One step below the floor is the silent position, so stepping down
        // from the bottom of the travel reaches it and stepping up leaves it.
        let silent_db = Self::VOLUME_FLOOR_DB - Self::VOLUME_STEP_DB;
        let current_db = self.volume_db().map_or(silent_db, |db| db.round());

        let target_db =
            (current_db + steps as f32 * Self::VOLUME_STEP_DB).clamp(silent_db, top_db);
        self.volume = if target_db < Self::VOLUME_FLOOR_DB {
            TrackConfig::MIN_VOLUME
        } else {
            10.0f32
                .powf(target_db / 20.0)
                .clamp(TrackConfig::MIN_VOLUME, TrackConfig::MAX_VOLUME)
        };

        self.sync_to_audio();
        self.volume
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
