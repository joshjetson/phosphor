//! Track state — TrackState, TrackElement, Clip, MidiNote.

use phosphor_core::fx::{FxTarget, SendSlot};
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
    /// Where the track sits in the stereo field.
    Pan,
    /// How much of it goes to send bus A, and to B.
    SendA,
    SendB,
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
            // The routing comes after the switches and before the clips: the
            // order the switches are in is muscle memory, and the three new
            // cells are a group of their own on the row below them.
            Self::RecordArm => Self::Pan,
            Self::Pan => Self::SendA,
            Self::SendA => Self::SendB,
            Self::SendB => {
                if num_clips > 0 { Self::Clip(0) } else { Self::SendB }
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
            Self::Pan => Self::RecordArm,
            Self::SendA => Self::Pan,
            Self::SendB => Self::SendA,
            Self::Clip(0) => Self::SendB,
            Self::Clip(i) => Self::Clip(i - 1),
        }
    }
}

// ── Data Models ──

#[derive(Debug, Clone, PartialEq)]
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

/// Which half of the `sends` pair a bus is.
fn send_index(slot: SendSlot) -> usize {
    match slot {
        SendSlot::A => 0,
        SendSlot::B => 1,
    }
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
    /// The UI's mirror of this strip's six insert slots. The effects
    /// themselves live on the audio thread; see [`super::FxInstance`].
    pub fx_chain: Vec<super::FxInstance>,
    /// Where the track sits in the image, −1 hard left to +1 hard right.
    ///
    /// Centre is unity in both channels, so a session written before pan
    /// existed loads at 0.0 and renders exactly as it did — see
    /// `phosphor_core::fx::pan_gains`.
    pub pan: f32,
    /// Post-fader send levels into buses A and B, as linear gains. Both
    /// start at zero, which is −inf: a send is a thing the player opens, not
    /// a thing they have to remember to close.
    pub sends: [f32; 2],
    /// The track this one's sidechain keys off, by mixer id — an identity
    /// rather than a position, so adding or deleting a track above it does
    /// not silently re-point the key at something else.
    pub key_source: Option<usize>,
    /// What that track was called when the key was set.
    ///
    /// Kept only so that a key whose track has been deleted can say *which*
    /// track — `Kick (missing)` rather than a bare `(missing)`, which tells
    /// the player nothing they can act on. Not persisted and not authoritative:
    /// the id is the key, this is a label, and a track that has been renamed
    /// re-labels itself the moment it is found.
    pub key_source_name: Option<String>,
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
    ///
    /// On a sequencer track this is the *child*: the thing in the plugin slot
    /// making the sound. There is no separate instrument type for a
    /// sequencer, which is what lets the child's panel, preset bank and saved
    /// parameters all keep working untouched. What marks the track is
    /// [`TrackState::sequencer`].
    pub instrument_type: Option<super::InstrumentType>,
    /// Parameter values (mirrors the audio thread's plugin params).
    pub synth_params: Vec<f32>,
    /// The step sequencer driving this track, when it has one.
    ///
    /// Edited only through [`crate::sequencer::ops::dispatch`] — see that
    /// module for why there is exactly one way in.
    ///
    /// Boxed because eight patterns are nineteen kilobytes, and a
    /// `TrackState` is cloned whole for every undo entry and moved whenever
    /// the track list grows. A track with no sequencer should pay a pointer
    /// for the possibility, not a pattern bank.
    pub sequencer: Option<Box<crate::sequencer::SequencerState>>,
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

    /// The middle of the pan control.
    pub const CENTRE_PAN: f32 = 0.0;

    /// One press of the pan control: twenty steps from centre to either end,
    /// which is fine enough to place a sound and coarse enough to reach the
    /// end of the travel without holding the key down.
    pub const PAN_STEP: f32 = 0.05;

    /// The top of a send, unity. A send cannot be pushed past the level the
    /// track is already at — that is what the bus return is for.
    pub const MAX_SEND_DB: f32 = 0.0;

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
            pan: Self::CENTRE_PAN,
            sends: [0.0; 2],
            key_source: None,
            key_source_name: None,
            volume: TrackConfig::DEFAULT_VOLUME,
            mixer_id: None,
            handle: None,
            instrument_type: None,
            synth_params: Vec::new(),
            sequencer: None,
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

    /// Whether this track is an instrument track wired to the audio engine.
    ///
    /// The bus strips have handles too — that is how their meters and their
    /// return levels work — so "has a handle" stopped being the same question
    /// as "is a track the player can arm, record and load a patch on" the day
    /// the buses became real. Every caller of this means the second one.
    pub fn is_live(&self) -> bool {
        self.handle.is_some() && !self.is_bus()
    }

    /// Whether this strip is one of the two send buses or the master.
    pub fn is_bus(&self) -> bool {
        matches!(
            self.kind,
            TrackKind::SendA | TrackKind::SendB | TrackKind::Master
        )
    }

    /// Which send bus this strip is, if it is one.
    pub fn send_slot(&self) -> Option<SendSlot> {
        match self.kind {
            TrackKind::SendA => Some(SendSlot::A),
            TrackKind::SendB => Some(SendSlot::B),
            _ => None,
        }
    }

    /// Which insert chain on the audio thread this strip's `fx_chain`
    /// mirrors.
    ///
    /// `None` for an instrument track that has not been given to the mixer
    /// yet: there is nothing to address.
    pub fn fx_target(&self) -> Option<FxTarget> {
        match self.kind {
            TrackKind::SendA => Some(FxTarget::BusA),
            TrackKind::SendB => Some(FxTarget::BusB),
            TrackKind::Master => Some(FxTarget::Master),
            TrackKind::Instrument | TrackKind::Audio => self.mixer_id.map(FxTarget::Track),
        }
    }

    /// Move the pan control by `steps` presses, and report where it landed.
    ///
    /// Snaps onto the centre when it passes it: the centre is a position the
    /// player has to be able to get back to exactly, since it is the one that
    /// leaves the track untouched.
    pub fn adjust_pan(&mut self, steps: i32) -> f32 {
        let target = self.pan + steps as f32 * Self::PAN_STEP;
        self.pan = if target.abs() < Self::PAN_STEP * 0.5 {
            Self::CENTRE_PAN
        } else {
            target.clamp(-1.0, 1.0)
        };
        self.pan
    }

    /// A send level in decibels, or `None` when the send is closed.
    pub fn send_db(&self, slot: SendSlot) -> Option<f32> {
        let gain = self.sends[send_index(slot)];
        (gain > 0.0).then(|| 20.0 * gain.log10())
    }

    /// Set a send from a level in decibels. Below the floor, the send closes.
    pub fn set_send_db(&mut self, slot: SendSlot, db: f32) -> f32 {
        let gain = if db <= phosphor_core::fx::SILENT_DB {
            0.0
        } else {
            phosphor_core::fx::db_to_gain(db.min(Self::MAX_SEND_DB))
        };
        self.sends[send_index(slot)] = gain;
        gain
    }

    /// A send's linear gain.
    pub fn send(&self, slot: SendSlot) -> f32 {
        self.sends[send_index(slot)]
    }
}
