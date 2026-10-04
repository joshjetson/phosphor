//! Per-track audio mixer with MIDI recording and clip playback.
//!
//! The mixer owns all audio tracks and processes the track graph:
//! routing MIDI to the active track, recording armed tracks,
//! playing back clips, applying mute/solo/volume, and mixing to master.

use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use phosphor_midi::message::MidiMessage;
use phosphor_plugin::{MidiEvent, Plugin};

use crate::clip::{ClipEvent, ClipSnapshot, MidiClip, RecordBuffer};
use crate::engine::VuLevels;
use crate::fx::{
    pan_gains, Effect, FxChain, FxContext, FxScratch, FxTarget, GrBallistics, GrMeter, SendSlot,
};
use crate::metronome::Metronome;
use crate::parallel::{EachMut, Workers};
use crate::pattern::{EventSink, PatternBlock, PatternEvent, PatternPlayer, PlaybackWindow};
use crate::project::{TrackHandle, TrackKind};
use crate::transport::Transport;

// ── Commands ──

// Clippy would have `SetPattern` box its block, and boxing it is exactly the
// thing this design exists to avoid: a `Box` arriving on the audio thread is a
// `free` on the audio thread when the command is dropped. The command queue is
// short and the memory is nothing; the deadline is not.
#[allow(clippy::large_enum_variant)]
pub enum MixerCommand {
    AddTrack {
        kind: TrackKind,
        handle: Arc<TrackHandle>,
    },
    SetInstrument {
        track_id: usize,
        instrument: Box<dyn Plugin + Send>,
    },
    RemoveTrack {
        track_id: usize,
    },
    SetParameter {
        track_id: usize,
        param_index: usize,
        value: f32,
    },
    /// Create a new empty clip on a track.
    CreateClip {
        track_id: usize,
        start_tick: i64,
        length_ticks: i64,
    },
    /// Replace a clip's events with edited data from the UI.
    UpdateClip {
        track_id: usize,
        clip_index: usize,
        events: Vec<ClipEvent>,
    },
    /// Update a clip's timeline position and length on the audio thread.
    UpdateClipPosition {
        track_id: usize,
        clip_index: usize,
        start_tick: i64,
        length_ticks: i64,
    },
    /// Remove a clip from a track on the audio thread.
    RemoveClip {
        track_id: usize,
        clip_index: usize,
    },
    /// Throw away whatever the recorder is holding, but keep recording.
    ///
    /// This is undo's reach into the pass that has not committed yet: the
    /// player is still recording, dislikes what they just played, and wants
    /// it gone *without* the transport stopping. The buffer empties and
    /// restarts — at the loop start when looping, at the playhead when not —
    /// so the next thing played lands exactly as if the pass had just begun.
    /// Committed takes are not touched; those live in clips and are undone
    /// from the UI side.
    DiscardRecording,
    /// Give one of a sequencer track's eight pattern slots new contents, and
    /// with it the UI's current word on the track-level settings that ride on
    /// a block — see [`PatternBlock`].
    ///
    /// The block travels by value. It is [`Copy`] and about two and a half
    /// kilobytes, so receiving one is a memcpy into memory that already
    /// exists: no `Vec` to free, no `Box` to drop, nothing for the audio
    /// thread to hand back to the allocator. The first pattern a track is
    /// given allocates its player, exactly as `SetInstrument` allocates a
    /// voice array; every one after it does not.
    SetPattern {
        track_id: usize,
        slot: u8,
        block: PatternBlock,
    },

    // ── Inserts ──
    //
    // Every one of these addresses a chain by [`FxTarget`] rather than by
    // track id, because three of the four chains are not on tracks: the two
    // send buses and the master are strips of their own, and a `usize` that
    // sometimes means a track and sometimes means the master is an addressing
    // bug waiting for its first bus effect.
    /// Put an effect into a slot, sliding the ones after it along.
    ///
    /// The box travels like `SetInstrument`'s does, and for the same reason:
    /// the effect has to be built somewhere, the UI thread is the only place
    /// that can allocate, and the audio thread's cost is a pointer move plus
    /// the `init` this command charges [`HEAVY_COMMAND`] for. A chain that is
    /// already full drops the effect rather than growing — the cap is
    /// enforced where the memory is.
    AddFx {
        target: FxTarget,
        slot: usize,
        effect: Box<dyn Effect>,
    },
    /// Put a MIDI effect in a track's pre-instrument slot. Tracks only —
    /// buses and the master have no MIDI, so the address is a track id
    /// rather than an [`FxTarget`] with two impossible variants.
    AddMidiFx {
        track_id: usize,
        slot: usize,
        fx: Box<dyn crate::midi_fx::MidiEffect>,
    },
    /// Take a MIDI effect out. The slot flushes its note-offs first, so a
    /// generated chord does not hang under the instrument.
    RemoveMidiFx {
        track_id: usize,
        slot: usize,
    },
    SetMidiFxParam {
        track_id: usize,
        slot: usize,
        param_index: usize,
        value: f32,
    },
    SetMidiFxBypass {
        track_id: usize,
        slot: usize,
        bypassed: bool,
    },
    /// The practice room's click: a free metronome at `bpm`, transport
    /// ignored. `bpm` 0.0 turns it off. `pattern` 0 = every beat with a
    /// downbeat accent, 1 = beats 2 and 4 only.
    SetPracticeClick { bpm: f64, pattern: u8 },
    /// Hand a MIDI effect a user progression. The Vec travels in and is
    /// copied into the effect's own storage; dropping it here frees it,
    /// the same road `UpdateClip` events already take.
    SetMidiFxProgression {
        track_id: usize,
        slot: usize,
        chords: Vec<crate::midi_fx::UserChord>,
    },
    /// Hand the sampler one pad, whole: config plus layers.
    ///
    /// The `Arc`s inside the layers are refcount handles — the UI side
    /// keeps a reference to every buffer it has ever sent, so the clones
    /// and drops here never touch the allocator's free path. The `Vec`
    /// itself is copied into slots the engine already owns and freed on
    /// this thread, the road `UpdateClip` events already take.
    SetSamplerPad {
        track_id: usize,
        pad: u8,
        config: phosphor_plugin::sample::PadConfig,
        layers: Vec<phosphor_plugin::sample::PadLayer>,
    },
    /// Hand the sampler a stretch of pads at once — each one's config, its
    /// layers and its phrases together.
    ///
    /// One command instead of two per key, because in keys mode a *single*
    /// keypress can change every key on the bed: one nudge of a trim marker
    /// under a bed-wide zone used to queue 88 `SetSamplerPad` and 88
    /// `SetSamplerPhrases`, 176 allocating commands against a drain of four
    /// per callback. Held at a terminal's auto-repeat that is a queue that
    /// grows faster than it empties, and the player hears a trim they set
    /// seconds ago.
    ///
    /// Measured, release, on a full bed (88 pads × 8 layers × 4 phrases):
    /// applying the whole range is **8.6 µs**, or 18 µs counting the `Vec`s
    /// being built and freed. One callback's own rendering with 32 voices on
    /// it is 48 µs, and the shortest deadline the budget is sized against is
    /// 726 µs. So the range is charged by its length — see [`command_cost`] —
    /// rather than being made the one heavy item a callback may take, which
    /// would have spread a single keypress over three callbacks for no
    /// measurable gain.
    ///
    /// Entries past [`phosphor_plugin::sample::NUM_PADS`] are ignored: a
    /// range can never honestly name more keys than the bed has, and the
    /// work one command can ask for has to be bounded by something the audio
    /// thread can see.
    ///
    /// The `Arc`s travel exactly as `SetSamplerPad`'s do.
    SetSamplerRange {
        track_id: usize,
        pads: Vec<(
            u8,
            phosphor_plugin::sample::PadConfig,
            Vec<phosphor_plugin::sample::PadLayer>,
            Vec<phosphor_plugin::sample::PadPhrase>,
        )>,
    },
    /// Audition one sampler layer, or `None` to stop auditioning.
    ///
    /// The layer travels whole for the reason
    /// [`PreviewLayer`](phosphor_plugin::sample::PreviewLayer) gives: an
    /// index into the pad table would name the wrong sound the moment a file
    /// above it went missing. The `Arc` is a refcount handle like every other
    /// one on this road.
    SetSamplerPreview {
        track_id: usize,
        preview: Option<phosphor_plugin::sample::PreviewLayer>,
    },
    /// Give the sampler the one child instrument its phrase layers play
    /// through, or `None` to take it away.
    ///
    /// The box travels exactly as `SetInstrument`'s does, and the note there
    /// applies whole: the UI thread builds it because it is the only thread
    /// allowed to allocate, the audio thread calls `init` on it, and the
    /// child it replaces is freed here. That free is what this command is
    /// charged [`HEAVY_COMMAND`] for.
    SetSamplerChild {
        track_id: usize,
        child: Option<Box<dyn Plugin + Send>>,
    },
    /// One control on the sampler's child instrument.
    ///
    /// A child arrives at its own defaults, and a phrase has to sound like
    /// the thing it was played on, so the panel follows it across one of
    /// these per control — `SetInstrument` and its parameter block, in
    /// miniature. There is no panel for the child on the screen yet; this
    /// is what the landing of a phrase sends.
    SetSamplerChildParam {
        track_id: usize,
        param_index: usize,
        value: f32,
    },
    /// Hand the sampler one pad's phrase layers, whole.
    ///
    /// The `Arc` inside each phrase is a refcount handle on the road
    /// `SetSamplerPad`'s buffers already take: the UI keeps a reference to
    /// every event list it has sent, so the clones and drops on the far side
    /// never reach the allocator's free path. The `Vec` itself is copied into
    /// slots the engine already owns and freed on this thread.
    SetSamplerPhrases {
        track_id: usize,
        pad: u8,
        phrases: Vec<phosphor_plugin::sample::PadPhrase>,
    },
    /// Take the effect out of a slot. Frees on the audio thread, as
    /// `RemoveTrack` and `UpdateClip` already do.
    RemoveFx {
        target: FxTarget,
        slot: usize,
    },
    /// Reorder one slot. Chain order is the chain's meaning, so this is an
    /// explicit move rather than anything that could be mistaken for a sort.
    MoveFx {
        target: FxTarget,
        from: usize,
        to: usize,
    },
    /// One control on one effect, in the control's own unit.
    SetFxParam {
        target: FxTarget,
        slot: usize,
        param: usize,
        value: f32,
    },
    /// Throw a slot's bypass switch. The audio thread crossfades it.
    SetFxBypass {
        target: FxTarget,
        slot: usize,
        bypass: bool,
    },

    // ── Sends and pan ──
    /// How much of this track goes to a send bus, as a linear gain. Zero is
    /// off, which is where every send starts.
    SetSendLevel {
        track_id: usize,
        send: SendSlot,
        gain: f32,
    },
    /// Where this track sits in the image, −1 hard left to +1 hard right.
    SetPan {
        track_id: usize,
        pan: f32,
    },
    /// Which track's signal this track's chain keys off, by track identity —
    /// not by position, which changes whenever a track is added or removed.
    /// `None` is the internal key.
    SetKeySource {
        track_id: usize,
        source: Option<usize>,
    },
    /// Put one track's output on the monitor path in place of its own signal
    /// — what a compressor's sidechain is keyed off, heard on its own.
    ///
    /// Transient by construction. One track at a time, because it is an
    /// `Option` and not a flag per track; never written to a session; and
    /// cleared by the audio thread itself the moment the transport stops, so
    /// a front end that forgets cannot leave a mix with a hole in it.
    SetKeyListen {
        track: Option<usize>,
    },
}

// ── Command budget ──
//
// The audio callback has a hard deadline — 1.45 ms at the default 64 frames,
// 0.73 ms if the device asks for 32 — and applying commands is the one thing
// in it whose size the audio thread does not control. Loading a preset queues
// one command per control, 59 of them on the Odyssey; opening a session
// queues an AddTrack, a SetInstrument and a full parameter block per track,
// plus two commands per clip. Draining all of that in one callback is an
// unbounded amount of work behind a fixed deadline, which is a dropout.
//
// So each callback spends a fixed budget and stops. Nothing is dropped and
// nothing is reordered: what is left stays queued, in order, and the next
// callback continues from there. A burst that does not fit is spread over
// consecutive callbacks — for a session load that is a few milliseconds with
// the transport stopped, and for a preset it is at worst one buffer rendered
// with part of the old panel, which is 1.45 ms.

/// The cost of a command that goes to the allocator. See [`command_cost`].
const HEAVY_COMMAND: u32 = 16;

/// How many sampler pads one [`HEAVY_COMMAND`]'s worth of budget buys.
///
/// Measured, release, on a full bed: delivering one pad — a config copy,
/// eight `Arc`s re-pointed, four phrase slots filled — is **0.10 µs**, and
/// `SetInstrument`, which is what `HEAVY_COMMAND` is priced against, is
/// **7.5 µs**. So thirty-two pads (3.2 µs, or about 6 µs once the two `Vec`s
/// per pad are freed) sit comfortably inside one heavy command's bill, with
/// margin for the allocator having a bad day. A whole bed is three of these:
/// 48 of the 64 units, so the worst range a player can produce still lands
/// inside a single callback.
const PADS_PER_HEAVY: usize = 32;

/// The most pads one [`MixerCommand::SetSamplerRange`] will apply.
///
/// A range can never honestly name more keys than the bed has, and the work
/// one command can ask of the audio thread has to be bounded by something
/// that thread can check for itself.
const MAX_RANGE_PADS: usize = phosphor_plugin::sample::NUM_PADS;

/// What one command costs, in the units [`COMMAND_BUDGET`] is denominated in.
///
/// Two tiers, and the line between them is the allocator:
///
/// * **1** — writes into memory that already exists. Setting a parameter is a
///   clamp and a store; moving a clip writes two integers.
/// * **[`HEAVY_COMMAND`]** — allocates, frees, or both. `SetInstrument` calls
///   `Plugin::init`, which builds a voice array and, on the Juno, a chorus
///   delay line; `AddTrack` allocates two audio buffers; `RemoveTrack` and
///   `UpdateClip` free what they replace.
///
/// Measured in release on a 64-frame callback: four instrument loads take
/// 30 µs against 1.4 µs for four `AddTrack` and 6.8 µs for sixty-four
/// parameter changes, and the callback's own rendering with one instrument on
/// it is 15 µs. So a flat count would be wrong in both directions: sixty-four
/// parameter changes belong in one callback, and sixty-four instrument loads
/// would be half a millisecond of it.
fn command_cost(cmd: &MixerCommand) -> u32 {
    match cmd {
        // The one command whose bill depends on what is inside it: a range
        // is between one pad and a whole bed, and charging a bed the same as
        // a pad would let one keypress do eighty-eight pads' work under a
        // one-pad budget. Charged in whole heavy units so the currency stays
        // the same one everything else is denominated in.
        MixerCommand::SetSamplerRange { pads, .. } => {
            HEAVY_COMMAND * (pads.len().min(MAX_RANGE_PADS).div_ceil(PADS_PER_HEAVY).max(1)) as u32
        }
        MixerCommand::SetParameter { .. }
        | MixerCommand::UpdateClipPosition { .. }
        // The insert layer's cheap half: a parameter is a store inside an
        // effect, a bypass is a bool, and a send level, a pan position and a
        // key source are one field each on a track that already exists.
        | MixerCommand::SetFxParam { .. }
        | MixerCommand::SetFxBypass { .. }
        | MixerCommand::SetSendLevel { .. }
        | MixerCommand::SetPan { .. }
        | MixerCommand::SetKeySource { .. }
        | MixerCommand::SetKeyListen { .. }
        // A MIDI-FX parameter is a store; a bypass flips a bool and pushes
        // a handful of note-offs into a buffer that already exists.
        | MixerCommand::SetMidiFxParam { .. }
        | MixerCommand::SetMidiFxBypass { .. }
        // A control on the sampler's child is a clamp and a store inside an
        // instrument that is already built — the same bill a track's own
        // parameter pays, under a different name.
        | MixerCommand::SetSamplerChildParam { .. }
        | MixerCommand::SetPracticeClick { .. }
        // Clearing a record buffer keeps its capacity: a store and a length
        // reset, nothing for the allocator.
        | MixerCommand::DiscardRecording => 1,
        MixerCommand::AddTrack { .. }
        | MixerCommand::SetInstrument { .. }
        | MixerCommand::RemoveTrack { .. }
        | MixerCommand::CreateClip { .. }
        | MixerCommand::UpdateClip { .. }
        | MixerCommand::RemoveClip { .. }
        // `AddFx` calls `Effect::init`, which builds delay lines; `RemoveFx`
        // frees one; `MoveFx` shifts the slot list. All three are the
        // allocator's business.
        | MixerCommand::AddFx { .. }
        | MixerCommand::RemoveFx { .. }
        | MixerCommand::AddMidiFx { .. }
        | MixerCommand::RemoveMidiFx { .. }
        | MixerCommand::SetMidiFxProgression { .. }
        // A pad delivery is Arc traffic plus a Vec freed here — allocator
        // business on the drop side even when the copy itself is cheap. An
        // audition is the same traffic and starts a voice with it, and the
        // UI sends one per press of a trim key, which is exactly the burst
        // the budget exists to spread.
        | MixerCommand::SetSamplerPad { .. }
        | MixerCommand::SetSamplerPreview { .. }
        // A child is a whole instrument arriving and, usually, another one
        // leaving: an `init` and a free, which is `SetInstrument`'s bill
        // under a different name. Phrases are the pad road exactly.
        | MixerCommand::SetSamplerChild { .. }
        | MixerCommand::SetSamplerPhrases { .. }
        | MixerCommand::MoveFx { .. }
        // Only the first pattern a track receives allocates — it builds the
        // player — and the cost is charged before the command is opened, so
        // it cannot be told apart from the ones that only copy. Charging all
        // of them the allocating rate makes the bound hold for the one that
        // does; the copy itself is 2.4 kB, which is nothing next to a
        // `Plugin::init`.
        | MixerCommand::SetPattern { .. } => HEAVY_COMMAND,
    }
}

/// How much command work one callback will do.
///
/// 64 units: a whole parameter block in one callback — the widest panel in the
/// project is the Odyssey's 59 controls — or four allocating commands.
///
/// A panel wider than this is not a fault, only a preset load spread over two
/// callbacks, which shows up as one buffer rendered with part of the old panel
/// and is 1.45 ms long.
///
/// Sized against the shortest callback the application can be given, 32 frames
/// at 44.1 kHz, which is 726 µs: a full budget of the expensive kind measures
/// 30 µs, or four percent of that deadline, and the cheap kind 7 µs.
///
/// The bound this buys is `COMMAND_BUDGET - 1 + HEAVY_COMMAND` units of work
/// per callback, not `COMMAND_BUDGET`: the budget is checked before a command
/// is taken and its cost is known only after. Tightening that would need a
/// `peek` the channel does not offer, and the overshoot is one command.
const COMMAND_BUDGET: u32 = 64;

/// How many tracks a mixer has room for before its track list has to grow.
///
/// Growing it is a reallocation on the audio thread, so the list is built with
/// room for more tracks than a session is going to hold. It is not a limit:
/// `AddTrack` past this still works, at the cost of one reallocation, and the
/// next 64 are free again. 64 `AudioTrack` headers are a few kilobytes, which
/// is nothing next to the two audio buffers each one already owns.
const TRACK_CAPACITY: usize = 64;

// ── Master limiter ──

/// Peak ceiling the limiter holds the master bus to, −1 dBFS.
///
/// Not 1.0: the samples we write are points on a waveform the converter
/// reconstructs between, and that reconstruction can overshoot the samples
/// themselves. A dB of margin is the usual allowance for it.
const LIMITER_CEILING: f32 = 0.891;

/// Release time constant, 50 ms.
///
/// Long enough not to modulate the waveform of a low note — a 40 Hz cycle is
/// 25 ms, and a release near that period distorts the fundamental instead of
/// riding it. Short enough that a single loud transient does not duck the
/// following bar. Attack is not a time constant at all: see [`MasterLimiter`].
const LIMITER_RELEASE_SECONDS: f32 = 0.050;

/// Stereo-linked peak limiter on the master bus.
///
/// The last stage before the audio device, and the only hard guarantee that
/// nothing leaves at more than full scale. Gain staging in the instruments
/// and the soft saturator on their outputs are what keep this idle; this is
/// what catches everything they cannot — many loud tracks at once, a plugin
/// with no output bound, a NaN out of a diverging filter.
///
/// Design notes:
///
/// * **Stereo-linked.** One gain, computed from `max(|L|, |R|)` and applied
///   to both channels, so a peak in one channel does not pull the image
///   across to the other.
/// * **Instant attack.** The gain that a sample needs is applied to that
///   same sample, not `n` samples later, so there is no overshoot to clean
///   up afterwards and no lookahead buffer to pay for. The alternative — a
///   millisecond attack — would let a millisecond of overshoot through, and
///   the only thing left to catch it would be a hard clip.
/// * **Smooth release.** One-pole, so the gain walks back to unity rather
///   than stepping.
///
/// Real-time safe: three floats of state, no allocation, no locks, no
/// branches that can panic.
struct MasterLimiter {
    /// Current gain, 0..=1. Never above unity: this only ever attenuates.
    gain: f32,
    /// One-pole coefficient for the release ramp.
    release_coeff: f32,
    /// The lowest gain reached anywhere in the block just processed — what
    /// the meter is drawn from. The worst moment rather than the average,
    /// because a limiter's whole job is the worst moment.
    block_min_gain: f32,
}

impl MasterLimiter {
    fn new(sample_rate: u32) -> Self {
        let sr = (sample_rate as f32).max(1.0);
        Self {
            gain: 1.0,
            release_coeff: 1.0 - (-1.0 / (LIMITER_RELEASE_SECONDS * sr)).exp(),
            block_min_gain: 1.0,
        }
    }

    fn reset(&mut self) {
        self.gain = 1.0;
        self.block_min_gain = 1.0;
    }

    /// Limit an interleaved stereo buffer in place.
    ///
    /// On return every sample is finite and within ±1.0. Any frame that was
    /// not finite on the way in leaves as silence.
    fn process(&mut self, output: &mut [f32]) {
        self.block_min_gain = 1.0;
        let mut frames = output.chunks_exact_mut(2);
        for frame in frames.by_ref() {
            // A NaN or infinity reaching the device is a full-scale noise
            // burst, so it is turned into silence here — and, just as
            // important, before it can be fed into the detector below, where
            // it would poison the gain state for every sample after it.
            let l = if frame[0].is_finite() { frame[0] } else { 0.0 };
            let r = if frame[1].is_finite() { frame[1] } else { 0.0 };

            let peak = l.abs().max(r.abs());
            // The backoff is not a fudge factor. `CEILING / peak` rounds to
            // nearest, and so does the multiply that applies it, so the
            // product can land up to three rounding steps above the ceiling.
            // Two epsilons of headroom covers that with margin and makes "at
            // or below the ceiling" exact rather than approximate.
            let target = if peak > LIMITER_CEILING {
                (LIMITER_CEILING / peak) * (1.0 - 2.0 * f32::EPSILON)
            } else {
                1.0
            };

            if target < self.gain {
                self.gain = target;
            } else {
                self.gain += (target - self.gain) * self.release_coeff;
            }
            if self.gain < self.block_min_gain {
                self.block_min_gain = self.gain;
            }

            // Belt and braces. `gain <= CEILING / peak` holds by
            // construction, so the product cannot exceed the ceiling and this
            // clamp cannot fire — it is here because it is the last line
            // before the audio device and the cost of being wrong is a
            // speaker.
            frame[0] = (l * self.gain).clamp(-1.0, 1.0);
            frame[1] = (r * self.gain).clamp(-1.0, 1.0);
        }

        // An interleaved stereo buffer with an odd sample count is malformed
        // and no device produces one, but the guarantee is unconditional: a
        // trailing sample gets the same treatment rather than going out
        // unchecked.
        for tail in frames.into_remainder() {
            let s = if tail.is_finite() { *tail } else { 0.0 };
            *tail = (s * self.gain).clamp(-LIMITER_CEILING, LIMITER_CEILING);
        }
    }
}

// ── AudioTrack ──

/// How many events one track's plugin queue holds before it would have to
/// grow.
///
/// It never grows: the pattern player is handed the queue's remaining room as
/// its budget and stops when it runs out, and clip playback has always fitted
/// inside it. Sized for the densest thing the sequencer can ask for — eight
/// lanes of five-note chords, each with the note-off of whatever it replaced,
/// across the two or three steps a callback can span — plus room for live
/// MIDI on top.
const PLUGIN_EVENT_CAPACITY: usize = 512;

pub struct AudioTrack {
    pub id: usize,
    pub kind: TrackKind,
    pub handle: Arc<TrackHandle>,
    pub instrument: Option<Box<dyn Plugin>>,
    /// The six insert slots, run between the instrument and the fader.
    pub chain: FxChain,
    /// Where the track sits in the image, −1..=1. See [`pan_gains`].
    pan: f32,
    /// Post-fader send levels, as linear gains. Zero — off — is where both
    /// start, so a session that has never opened a send mixes exactly as it
    /// did before sends existed.
    send: [f32; 2],
    /// Which track's pre-insert signal this track's chain keys off, by track
    /// id. Resolved to a position once per block; `None` is the internal key.
    key_source: Option<usize>,
    /// Recorded clips on this track's timeline.
    pub clips: Vec<MidiClip>,
    /// The step sequencer on this track, when it has one.
    ///
    /// Boxed because it carries all eight pattern slots — around 19 kB — and
    /// a track without a sequencer should not pay for them, least of all
    /// inside the `Vec<AudioTrack>` that is memcpy'd when a track is added.
    pattern: Option<Box<PatternPlayer>>,
    /// Active recording buffer (when armed + transport recording).
    record_buf: RecordBuffer,
    /// Whether we were recording last buffer (to detect stop).
    was_recording: bool,
    /// Last tick position seen during recording (to detect loop wraps).
    last_record_tick: i64,
    /// Pass one's output: the instrument's sound, and the sidechain tap
    /// every keyed chain reads. Left as the instrument made it.
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    /// Pass two's working copy: the inserts, then the fader, run here so
    /// that `buf_l`/`buf_r` stay a clean tap. See [`inserts`].
    work_l: Vec<f32>,
    work_r: Vec<f32>,
    /// The dry copy a bypass crossfade in this track's chain needs.
    fx_scratch: FxScratch,
    /// Where this block's key comes from: a position in the track list,
    /// resolved from `key_source` before the inserts run.
    key_from: Option<usize>,
    plugin_events: Vec<MidiEvent>,
    /// Pre-instrument MIDI effects, run over the assembled event list.
    midi_fx: Vec<crate::midi_fx::MidiFxSlot>,
    /// Note-offs owed by a slot that was just bypassed or removed, played
    /// into the instrument on the next block so nothing hangs.
    midi_fx_owed: Vec<MidiEvent>,
}

impl AudioTrack {
    /// Pass one's expensive half: this track's instrument, playing the
    /// events assembled for it, into its own buffers. Touches nothing but
    /// this track, so every track's can run at once.
    fn render_instrument(&mut self, frames: usize) {
        if let Some(ref mut instrument) = self.instrument {
            let out_l = &mut self.buf_l[..frames];
            let out_r = &mut self.buf_r[..frames];
            let mut out_slices: [&mut [f32]; 2] = [out_l, out_r];
            instrument.process(&[], &mut out_slices, &self.plugin_events);
        }
    }

    pub fn new(handle: Arc<TrackHandle>, sample_rate: u32, max_buffer_size: usize) -> Self {
        Self {
            id: handle.id,
            kind: handle.kind,
            handle,
            instrument: None,
            chain: FxChain::new(sample_rate),
            pan: 0.0,
            send: [0.0; 2],
            key_source: None,
            clips: Vec::new(),
            pattern: None,
            record_buf: RecordBuffer::new(),
            was_recording: false,
            last_record_tick: -1,
            buf_l: vec![0.0; max_buffer_size],
            buf_r: vec![0.0; max_buffer_size],
            work_l: vec![0.0; max_buffer_size],
            work_r: vec![0.0; max_buffer_size],
            fx_scratch: FxScratch::new(max_buffer_size),
            key_from: None,
            plugin_events: Vec::with_capacity(PLUGIN_EVENT_CAPACITY),
            midi_fx: Vec::with_capacity(crate::midi_fx::MAX_MIDI_FX_SLOTS),
            midi_fx_owed: Vec::with_capacity(64),
        }
    }
}

/// Writes pattern events straight into a track's plugin queue.
///
/// The conversion from song time to buffer position happens here, through
/// [`PlaybackWindow::sample_offset`] — the same call clip playback makes a few
/// lines further down, which is what "a pattern step and a clip note on the
/// same beat land on the same sample" rests on.
///
/// The queue is never grown. When it is full the sink refuses, and the
/// generator stops rather than dropping events out of the middle of a step.
struct TrackEventSink<'a> {
    events: &'a mut Vec<MidiEvent>,
    window: &'a PlaybackWindow,
}

impl EventSink for TrackEventSink<'_> {
    fn accept(&mut self, event: PatternEvent) -> bool {
        if self.events.len() >= self.events.capacity() {
            return false;
        }
        self.events.push(MidiEvent {
            sample_offset: self.window.sample_offset(event.tick),
            status: event.status,
            data1: event.data1,
            data2: event.data2,
        });
        true
    }
}

/// Put a track's events in the order the instrument will read them.
///
/// A hand-written insertion sort, and not for speed: `slice::sort_by_key` is
/// a merge sort that allocates a scratch buffer past twenty elements, which
/// on the audio thread is exactly the thing this whole crate is arranged to
/// avoid. These lists are short and arrive nearly sorted — clips are stored
/// in tick order and a pattern generates step by step — so the insertion sort
/// is linear in practice as well as allocation-free.
///
/// Stable, which is load-bearing: a note-off written before a note-on at the
/// same offset has to stay before it, or a pattern switch kills the voice it
/// just started.
fn sort_events_by_offset(events: &mut [MidiEvent]) {
    for i in 1..events.len() {
        let mut j = i;
        while j > 0 && events[j - 1].sample_offset > events[j].sample_offset {
            events.swap(j - 1, j);
            j -= 1;
        }
    }
}

// ── Send buses ──

/// One of the two send buses: what the tracks feed, what it runs, and what it
/// returns to the master.
///
/// Not an [`AudioTrack`]. A bus has no instrument, no clips, no pattern, no
/// recording and no sends of its own — modelling it as a track would mean six
/// fields that are permanently `None` on two of the three strips in every
/// session, and an `audible` rule that has to remember to exempt it from
/// solo. Being a different type means the solo exemption is structural: the
/// bus is not in the list solo is computed over.
struct BusStrip {
    /// The UI's handle: mute, return level and the bus meter. `None` until
    /// the front end attaches one, which is what an `AddTrack` carrying a bus
    /// kind does.
    handle: Option<Arc<TrackHandle>>,
    /// The bus's own six inserts — the reverb, the delay.
    chain: FxChain,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    /// The dry copy a bypass crossfade in the bus's chain needs.
    fx_scratch: FxScratch,
    /// Whether any track sent to the bus this block. A bus with nothing in it
    /// and nothing fed to it is skipped entirely, which is what keeps a
    /// session with no sends bit-identical to one from before they existed.
    fed: bool,
}

impl BusStrip {
    fn new(sample_rate: u32, max_buffer_size: usize) -> Self {
        Self {
            handle: None,
            chain: FxChain::new(sample_rate),
            buf_l: vec![0.0; max_buffer_size],
            buf_r: vec![0.0; max_buffer_size],
            fx_scratch: FxScratch::new(max_buffer_size),
            fed: false,
        }
    }

    /// The bus's return level into the master, and whether it is muted.
    ///
    /// Both come from the same [`TrackHandle`] a track's fader does, so the
    /// return is the strip's fader rather than a control of its own. A bus
    /// with no handle attached returns at unity: the audio has to go
    /// somewhere, and silence would be a worse default than loud.
    fn return_gain(&self) -> f32 {
        match &self.handle {
            Some(h) if h.config.is_muted() => 0.0,
            Some(h) => h.config.get_volume(),
            None => 1.0,
        }
    }
}

// ── Mixer ──

pub struct Mixer {
    tracks: Vec<AudioTrack>,
    master_vu: Arc<VuLevels>,
    command_rx: Receiver<MixerCommand>,
    clip_tx: Sender<ClipSnapshot>,
    metronome: Metronome,
    /// The practice click, when the practice room asked for one:
    /// (bpm, pattern), plus its own beat phase.
    practice_click: Option<(f64, u8)>,
    practice_beat_phase: f64,
    sample_rate: u32,
    max_buffer_size: usize,
    /// Pre-allocated scratch buffers for mix — avoids allocation in process().
    scratch_l: Vec<f32>,
    scratch_r: Vec<f32>,
    /// Pre-allocated buffer for live MIDI conversion.
    live_events: Vec<(MidiEvent, i64)>,
    /// One scratch buffer for the whole MIDI-FX layer. Pass 1 is
    /// sequential, so one is enough for every track's chain.
    midi_fx_scratch: Vec<MidiEvent>,
    /// The window the previous callback rendered, when playback was running.
    ///
    /// One per mixer rather than one per track: the window is a fact about
    /// the transport and the block, so every track's is the same window, and
    /// two tracks that computed it separately could disagree. `None` whenever
    /// the transport is not rolling, which is what makes the first block
    /// after a start discontinuous — see [`PlaybackWindow::is_continuous`].
    last_window: Option<PlaybackWindow>,
    /// Final stage before the audio device — see [`MasterLimiter`].
    limiter: MasterLimiter,
    /// The limiter's gain reduction, as the UI reads it. The ballistics are
    /// here rather than in the UI because only this side sees every sample:
    /// see [`crate::fx::GrBallistics`].
    limiter_gr: GrBallistics,
    limiter_gr_meter: Arc<GrMeter>,
    /// The two send buses, A and B.
    bus_a: BusStrip,
    bus_b: BusStrip,
    /// The master's own six inserts, between the mix and the limiter.
    master_chain: FxChain,
    /// The master row's handle, for its meter. See the `AddTrack` arm.
    master_handle: Option<Arc<TrackHandle>>,
    /// The dry copy a bypass crossfade in the master's chain needs. Tracks
    /// and buses carry their own, so that their chains share nothing.
    fx_scratch: FxScratch,
    /// The track whose sidechain key is being monitored in place of its own
    /// output, if any.
    ///
    /// **One, and it is the type that says so.** An `Option<usize>` cannot
    /// hold two, so "only one key listen at a time" is not a rule anybody has
    /// to remember — setting a second one puts the first back by itself.
    key_listen: Option<usize>,
    /// Whether the transport was rolling on the previous block, so that a
    /// stop can clear the key listen without the front end having to.
    was_playing: bool,
    /// A [`MixerCommand::DiscardRecording`] waiting for the recording pass.
    ///
    /// A flag rather than work done in `apply_command`, because emptying a
    /// record buffer needs the transport — loop start or playhead — and the
    /// transport arrives with `process`, not with the command.
    discard_recording: bool,
    /// Whether the previous block was inside a count-in, so the metronome's
    /// beat tracker can be reset on the way in rather than swallowing the
    /// countdown's first click.
    was_counting: bool,
    /// The threads the tracks' instruments and inserts are shared out to.
    /// See [`crate::parallel`].
    workers: Workers,
}

impl Mixer {
    pub fn new(
        command_rx: Receiver<MixerCommand>,
        master_vu: Arc<VuLevels>,
        clip_tx: Sender<ClipSnapshot>,
        sample_rate: u32,
        max_buffer_size: usize,
    ) -> Self {
        Self {
            tracks: Vec::with_capacity(TRACK_CAPACITY),
            master_vu,
            command_rx,
            clip_tx,
            metronome: Metronome::new(sample_rate as f64),
            practice_click: None,
            practice_beat_phase: 0.0,
            sample_rate,
            max_buffer_size,
            scratch_l: vec![0.0; max_buffer_size],
            scratch_r: vec![0.0; max_buffer_size],
            live_events: Vec::with_capacity(256),
            midi_fx_scratch: Vec::with_capacity(crate::midi_fx::MIDI_FX_EVENT_CAPACITY),
            last_window: None,
            limiter: MasterLimiter::new(sample_rate),
            limiter_gr: GrBallistics::new(),
            limiter_gr_meter: Arc::new(GrMeter::new()),
            bus_a: BusStrip::new(sample_rate, max_buffer_size),
            bus_b: BusStrip::new(sample_rate, max_buffer_size),
            master_chain: FxChain::new(sample_rate),
            master_handle: None,
            fx_scratch: FxScratch::new(max_buffer_size),
            key_listen: None,
            was_playing: false,
            discard_recording: false,
            was_counting: false,
            workers: Workers::new(
                crate::parallel::audio_threads(),
                Some((max_buffer_size as u32, sample_rate)),
            ),
        }
    }

    /// Spread the audio over `threads` threads in all, the audio thread
    /// included; one runs everything on the audio thread. Starts threads,
    /// so never call it on the audio thread.
    pub fn set_threads(&mut self, threads: usize) {
        self.workers = Workers::new(threads, Some((self.max_buffer_size as u32, self.sample_rate)));
    }

    /// How many threads the audio is spread over, the audio thread included.
    #[must_use]
    pub fn threads(&self) -> usize {
        self.workers.threads()
    }

    /// The track whose key is being monitored, if any.
    #[must_use]
    pub fn key_listen(&self) -> Option<usize> {
        self.key_listen
    }

    /// The meter the master limiter publishes its gain reduction to.
    ///
    /// Handed out before the mixer moves to the audio thread; the UI holds
    /// the other end of the `Arc` and reads two atomics to draw it.
    #[must_use]
    pub fn limiter_gr_meter(&self) -> Arc<GrMeter> {
        self.limiter_gr_meter.clone()
    }

    /// Point the master limiter's meter at one the front end already holds.
    ///
    /// Called once, before the mixer reaches the audio thread. The engine
    /// creates the meter early — before it knows whether there is a device to
    /// build a mixer for at all — so this is how the two are joined.
    pub fn set_limiter_gr_meter(&mut self, meter: Arc<GrMeter>) {
        self.limiter_gr_meter = meter;
    }

    /// The insert chain a command is addressed to, if it exists.
    fn chain_mut(&mut self, target: FxTarget) -> Option<&mut FxChain> {
        match target {
            FxTarget::Track(id) => self
                .tracks
                .iter_mut()
                .find(|t| t.id == id)
                .map(|t| &mut t.chain),
            FxTarget::BusA => Some(&mut self.bus_a.chain),
            FxTarget::BusB => Some(&mut self.bus_b.chain),
            FxTarget::Master => Some(&mut self.master_chain),
        }
    }

    /// Process one buffer cycle.
    ///
    /// # Two passes
    ///
    /// ```text
    /// pass 1  every track: MIDI → instrument → buf_l/buf_r    ← the key tap
    /// pass 2  every track: buf → inserts → fader → pan → meter
    ///                                            ├→ send A ─┐
    ///                                            └→ send B ─┤
    /// buses   send sum → inserts → return → bus meter ───────┤
    /// master  mix ─────────────────────────────────────────→ inserts
    ///                                → metronome → limiter → out
    /// ```
    ///
    /// The split into two passes is what makes sidechaining honest. A single
    /// pass would let a compressor on track 3 key off track 1's *current*
    /// block and track 5's *previous* one, because track 5 has not rendered
    /// yet — a one-block error that depends on the order tracks happen to sit
    /// in and moves when the user reorders them. With every instrument
    /// rendered before any insert runs, every key is the same block, whatever
    /// the order.
    ///
    /// It also means pass 2 must not write over what pass 1 produced: a
    /// track's own buffers *are* the key tap, so the inserts run on the
    /// track's work pair (`work_l`/`work_r`) instead of in place. That is one
    /// memcpy per track per block, and it is what buys order-independence.
    ///
    /// Pass 2 is itself two halves. Every strip's inserts run first, each
    /// strip on its own ([`inserts`]); then, in track order, each strip's
    /// fader, pan, meter and sends add it into the mix. The first half is
    /// the expensive one and has no order; the second is cheap and keeps the
    /// summing order, which keeps the mix bit-identical however the first
    /// half was run.
    pub fn process(&mut self, output: &mut [f32], midi_messages: &[MidiMessage], transport: &Transport) {
        // Bounded: whatever does not fit in this callback's budget is applied
        // by the next one, in order. See `drain_commands`.
        let _ = self.drain_commands();

        let num_frames = output.len() / 2;
        let playing = transport.is_playing();

        // ── Key listen clears itself on a stop ──
        //
        // The audio thread's own safety net, on the edge rather than on the
        // level: a front end that crashed, or a panel that was closed by
        // something that forgot, cannot leave a track monitoring its
        // sidechain for the rest of the session. Setting it while the
        // transport is already stopped is left alone, because auditioning a
        // key against live playing is a thing people do.
        if self.was_playing && !playing {
            self.key_listen = None;
            // A stop cuts clip playback mid-note, and the note-offs that
            // would have closed those notes never render. Anything in the
            // MIDI-FX layer that tracks held notes — the arpeggiator's
            // pool, a chord device's release book — would keep them
            // forever and fold them into whatever the player does next.
            // Flush on the edge: generated notes get their offs, the pools
            // empty, and the next play starts from what the hands say.
            for track in &mut self.tracks {
                for slot in &mut track.midi_fx {
                    slot.fx.flush(&mut track.midi_fx_owed);
                }
            }
        }
        self.was_playing = playing;

        let recording = transport.is_recording();
        let looping = transport.is_looping();
        let current_tick = transport.position_ticks();
        let bpm = transport.tempo_bpm();
        let ticks_per_sample = (bpm * Transport::PPQ as f64) / (60.0 * self.sample_rate as f64);
        let loop_end = transport.loop_end();

        // ── The window ──
        //
        // The span of song time this callback renders, computed once and read
        // by everything that turns song time into notes. Clip playback and
        // pattern playback both take their events from this one value, which
        // is what makes them sample-identical on the same beat rather than
        // two implementations that have to be kept in agreement.
        let window = PlaybackWindow::for_block(
            current_tick,
            num_frames as u32,
            ticks_per_sample,
            looping.then(|| (transport.loop_start(), loop_end)),
            self.last_window,
        );
        self.last_window = playing.then_some(window);

        // Convert live MIDI to plugin events (reuse pre-allocated buffer).
        // Each event carries its age in ticks: how far into the past, in
        // song time, the note was actually struck. Messages arrive between
        // callbacks and are drained at the top of the next one, so with no
        // correction every note lands on a block edge — a 10ms-plus grid at
        // large buffers. The age puts it back where the player put it.
        self.live_events.clear();
        let drain_micros = phosphor_midi::clock::now_micros();
        for msg in midi_messages {
            if let Some(ev) = midi_to_plugin_event(msg) {
                let age = event_age_ticks(
                    msg.received_micros,
                    drain_micros,
                    ticks_per_sample,
                    self.sample_rate,
                );
                self.live_events.push((ev, age));
            }
        }

        let live_events = std::mem::take(&mut self.live_events);
        let clip_tx = &self.clip_tx;
        // Taken, not read: a discard applies to exactly one block, whether or
        // not anything was recording when it landed.
        let discard = std::mem::take(&mut self.discard_recording);

        // ── Pass 1: every instrument into its own buffers ──
        //
        // Nothing downstream of the instrument happens here. What each track
        // leaves behind is the sidechain key tap: post-instrument,
        // pre-insert, and still there when pass 2 reads it. The events each
        // instrument plays are gathered first, in track order; the
        // instruments then run together.
        for track in &mut self.tracks {
            if track.buf_l.len() < num_frames {
                track.buf_l.resize(num_frames, 0.0);
                track.buf_r.resize(num_frames, 0.0);
            }
            track.buf_l[..num_frames].fill(0.0);
            track.buf_r[..num_frames].fill(0.0);
            track.plugin_events.clear();

            let is_midi_active = track.kind == TrackKind::Instrument
                && track.handle.config.is_midi_active();
            let is_armed = track.handle.config.is_armed();
            let should_record = playing && recording && is_armed && is_midi_active;

            // ── Recording ──
            if should_record && !track.was_recording {
                // Start recording at the loop start, not the current position,
                // so the clip spans the full loop region
                let rec_start = if looping { transport.loop_start() } else { current_tick };
                track.record_buf.start(rec_start);
                tracing::debug!("rec start track={} tick={}", track.id, current_tick);
            }

            // Detect loop wrap: current tick jumped backward means transport looped.
            if should_record && track.was_recording && looping
                && track.record_buf.is_active() && track.last_record_tick >= 0
                && current_tick < track.last_record_tick
            {
                // A downbeat played a hair early belongs to the pass it was
                // aimed at. Notes still held within a 32nd of the wrap come
                // out of this take and re-strike at the top of the next —
                // without this they commit as a stray sliver at the far
                // right of the previous bar.
                let window = Transport::PPQ / 8;
                let end_rel = loop_end - track.record_buf.start_tick();
                let (anticipated, carried) = track.record_buf.take_anticipated(end_rel, window);
                commit_recording(track, loop_end, clip_tx);
                // Start new recording at loop start, not current_tick
                // (current_tick may be a few ticks past 0 due to buffer boundaries)
                track.record_buf.start(transport.loop_start());
                for &(note, velocity) in anticipated.iter().take(carried) {
                    track.record_buf.record(transport.loop_start(), 0x90, note, velocity);
                }
            }
            if should_record {
                track.last_record_tick = current_tick;
            }

            // ── A scrapped pass ──
            //
            // Undo, while the recorder is holding uncommitted notes. The
            // buffer restarts where a fresh pass would — loop start, or the
            // playhead when not looping — and stays active, so the player
            // replays the part without the transport so much as flinching.
            //
            // Before the stop-commit below on purpose: a discard racing a
            // stop must not have the stop commit the very notes the discard
            // was sent to remove. Emptied first, the stop then commits
            // nothing at all.
            if discard && track.was_recording && track.record_buf.is_active() {
                let rec_start = if looping { transport.loop_start() } else { current_tick };
                track.record_buf.start(rec_start);
                tracing::debug!("rec discard track={} restart at tick={}", track.id, rec_start);
            }

            // Commit when recording stops (user pressed stop)
            if !should_record && track.was_recording {
                commit_recording(track, current_tick, clip_tx);
            }
            track.was_recording = should_record;

            // Record live MIDI events (and pass through for monitoring).
            // Monitoring renders at the block edge — an already-played note
            // should sound as soon as possible. The recorded tick walks back
            // by the event's age, clamped to the take's start so a note
            // struck just before the loop wrapped lands on the downbeat
            // instead of before the take exists.
            if is_midi_active {
                for (ev, age_ticks) in &live_events {
                    track.plugin_events.push(*ev);
                    if should_record {
                        let played_tick =
                            (current_tick - age_ticks).max(track.record_buf.start_tick());
                        track.record_buf.record(played_tick, ev.status, ev.data1, ev.data2);
                    }
                }
            }

            // ── Pattern playback ──
            //
            // Before the clips, and unconditionally: a player that has just
            // been stopped still has note-offs to write, and the transport
            // being stopped is exactly when it has to write them.
            if let Some(ref mut player) = track.pattern {
                let mut sink = TrackEventSink { events: &mut track.plugin_events, window: &window };
                player.render(&window, playing, &mut sink);
                track.handle.pattern.publish(
                    player.live_slot(),
                    player.queued_slot(),
                    player.current_step(),
                    playing && player.is_playing(),
                );
            }

            // ── Clip playback ──
            //
            // Same window, same `sample_offset`. The loop wrap needs no
            // branch of its own any more: the window already starts at the
            // loop point when the transport has just gone round.
            if playing && !track.clips.is_empty() {
                for clip in &track.clips {
                    for (tick, event) in clip.events_between(window.from(), window.to()) {
                        if track.plugin_events.len() >= track.plugin_events.capacity() {
                            break;
                        }
                        track.plugin_events.push(MidiEvent {
                            sample_offset: window.sample_offset(tick),
                            status: event.status,
                            data1: event.data1,
                            data2: event.data2,
                        });
                    }
                }
            }

            if !track.plugin_events.is_empty() {
                sort_events_by_offset(&mut track.plugin_events);
            }

            // ── MIDI effects: between the assembled events and the ears ──
            //
            // After the sort so every effect sees a correctly ordered list;
            // after the recorder so what lands in a clip is the played
            // input, never the generated output.
            if track.midi_fx.iter().any(|s| !s.bypassed) {
                let ctx = crate::midi_fx::MidiFxContext {
                    sample_rate: self.sample_rate as f32,
                    tempo_bpm: bpm,
                    playing,
                    num_frames: num_frames as u32,
                    block_start_tick: current_tick,
                    ticks_per_sample,
                };
                for slot in track.midi_fx.iter_mut().filter(|s| !s.bypassed) {
                    self.midi_fx_scratch.clear();
                    slot.fx.process(&track.plugin_events, &mut self.midi_fx_scratch, &ctx);
                    track.plugin_events.clear();
                    for ev in &self.midi_fx_scratch {
                        if track.plugin_events.len() >= track.plugin_events.capacity() {
                            break;
                        }
                        track.plugin_events.push(*ev);
                    }
                }
                if !track.plugin_events.is_empty() {
                    sort_events_by_offset(&mut track.plugin_events);
                }
            }
            // Offs owed by a flushed slot go in AFTER the chain, straight
            // to the instrument. Before it, an active arp would swallow
            // them — it consumes note events for its pool — and the very
            // notes the flush exists to close would hang forever.
            if !track.midi_fx_owed.is_empty() {
                for ev in track.midi_fx_owed.drain(..) {
                    if track.plugin_events.len() < track.plugin_events.capacity() {
                        track.plugin_events.push(ev);
                    }
                }
                sort_events_by_offset(&mut track.plugin_events);
            }

            // Track position for wrap detection (used by both recording and playback)
            if playing {
                track.last_record_tick = current_tick;
            }
        }
        self.live_events = live_events;

        // ── Every instrument, spread over the cores ──
        //
        // The events are assembled; what is left is the sound, and each
        // track's touches only that track.
        {
            let each = EachMut::new(&mut self.tracks);
            self.workers.for_each(each.len(), &|index| {
                // SAFETY: the job's own index.
                unsafe { each.get(index) }.render_instrument(num_frames);
            });
        }

        // ── Pass 2: inserts, fader, pan, meters, sends ──

        // Buses and the master are exempt from solo. Without that, soloing
        // one track takes the reverb return with it and the mix goes dry —
        // and since the buses are not in the track list at all, the exemption
        // is structural rather than a rule someone has to remember.
        let any_solo = self
            .tracks
            .iter()
            .any(|t| !is_bus(t.kind) && t.handle.config.is_soloed());

        // Reuse pre-allocated scratch buffers for master mix.
        // Swap out of self to avoid borrow conflicts in the track loop.
        let mut master_l = std::mem::take(&mut self.scratch_l);
        let mut master_r = std::mem::take(&mut self.scratch_r);
        // Dead code in practice, and deliberately kept. `max_buffer_size` is
        // the largest block the device said it could deliver, so a block that
        // does not fit means a driver exceeded its own stated maximum. One
        // allocation is a glitch; the alternative here is wrong output or a
        // panic on the audio thread.
        if master_l.len() < num_frames {
            master_l.resize(num_frames, 0.0);
            master_r.resize(num_frames, 0.0);
        }
        master_l[..num_frames].fill(0.0);
        master_r[..num_frames].fill(0.0);

        let context = FxContext {
            sample_rate: self.sample_rate as f32,
            tempo_bpm: bpm as f32,
            playing,
            key: None,
        };

        {
            // Disjoint fields, borrowed at once: the track list is split
            // around the track being rendered so its key can be read out of
            // one of the halves, while the work buffers and the crossfade
            // scratch come from the mixer itself.
            let Self { tracks, bus_a, bus_b, key_listen, workers, .. } = self;
            let key_listen = *key_listen;

            for bus in [&mut *bus_a, &mut *bus_b] {
                if bus.buf_l.len() < num_frames {
                    bus.buf_l.resize(num_frames, 0.0);
                    bus.buf_r.resize(num_frames, 0.0);
                }
                bus.buf_l[..num_frames].fill(0.0);
                bus.buf_r[..num_frames].fill(0.0);
                bus.fed = false;
            }

            // ── The sidechain keys ──
            //
            // Resolved from the stored identity to a position every block,
            // never cached: a track that has been deleted must fall back to
            // the internal key *this* block rather than reading whatever now
            // sits where it used to. A key that names this same track is no
            // key at all. Monitoring the key needs it resolved whether or not
            // anything in the chain asked for one — the point of the switch
            // is to hear what a compressor *would* be keying off, including
            // on a track that has not got one yet.
            for index in 0..tracks.len() {
                let track = &tracks[index];
                let listening = key_listen == Some(track.id);
                let from = track
                    .key_source
                    .filter(|_| track.chain.wants_key() || listening)
                    .and_then(|id| tracks.iter().position(|t| t.id == id))
                    .filter(|&position| position != index);
                let track = &mut tracks[index];
                track.key_from = from;
                if track.work_l.len() < num_frames {
                    track.work_l.resize(num_frames, 0.0);
                    track.work_r.resize(num_frames, 0.0);
                }
            }

            // ── Every strip's inserts, spread over the cores ──
            {
                let strips = inserts::Strips::new(tracks, num_frames, key_listen);
                workers.for_each(strips.len(), &|index| {
                    // SAFETY: each index is one job.
                    unsafe { strips.run(index, &context) };
                });
            }

            for track in tracks.iter_mut() {
                let work_l = &mut track.work_l;
                let work_r = &mut track.work_r;
                // ── Fader, pan, meter ──
                //
                // Mute and solo are applied here, at the fader, which is what
                // makes them kill the sends as well: everything downstream
                // reads the post-fader signal.
                let muted = track.handle.config.is_muted();
                let soloed = track.handle.config.is_soloed();
                let volume = if is_audible(track.kind, muted, soloed, any_solo) {
                    track.handle.config.get_volume()
                } else {
                    0.0
                };
                let (pan_l, pan_r) = pan_gains(track.pan);
                let gain_l = volume * pan_l;
                let gain_r = volume * pan_r;

                // A silent strip is skipped rather than multiplied by zero.
                // Not for the cycles: `NaN * 0.0` is `NaN`, so a muted track
                // whose instrument has diverged would otherwise take the
                // whole mix with it — the limiter would render the *master*
                // as silence rather than the one track the user muted.
                if volume == 0.0 {
                    publish_vu(&track.handle.vu, 0.0, 0.0);
                    continue;
                }

                let mut peak_l = 0.0f32;
                let mut peak_r = 0.0f32;
                for i in 0..num_frames {
                    let l = work_l[i] * gain_l;
                    let r = work_r[i] * gain_r;
                    work_l[i] = l;
                    work_r[i] = r;
                    peak_l = peak_l.max(l.abs());
                    peak_r = peak_r.max(r.abs());
                    master_l[i] += l;
                    master_r[i] += r;
                }

                // The channel meter reads here — after the inserts, the fader
                // and the pan — so that pulling the fader down moves it. It
                // used to read the instrument's raw output, which meant the
                // meter showed what the track *would* have been.
                publish_vu(&track.handle.vu, peak_l, peak_r);

                // ── Sends ──
                //
                // Post-fader and post-pan, so a track that is muted, soloed
                // out or faded down goes quiet in the reverb too.
                for (level, bus) in [
                    (track.send[0], &mut *bus_a),
                    (track.send[1], &mut *bus_b),
                ] {
                    if level <= 0.0 {
                        continue;
                    }
                    bus.fed = true;
                    for i in 0..num_frames {
                        bus.buf_l[i] += work_l[i] * level;
                        bus.buf_r[i] += work_r[i] * level;
                    }
                }
            }

            // ── The buses ──
            //
            // Their own inserts, then their returns into the mix: A before
            // B, as always.
            {
                let mut buses = [&mut *bus_a, &mut *bus_b];
                let each = EachMut::new(&mut buses);
                workers.for_each(each.len(), &|index| {
                    // SAFETY: the job's own index.
                    inserts::run_bus(unsafe { each.get(index) }, num_frames, &context);
                });
            }
            for bus in [&mut *bus_a, &mut *bus_b] {
                return_bus(bus, &mut master_l[..num_frames], &mut master_r[..num_frames]);
            }
        }

        // ── The master's inserts ──
        //
        // Ahead of the metronome and the limiter: the click is a monitoring
        // aid rather than part of the mix, and the limiter is a safety device
        // rather than a slot.
        if !self.master_chain.is_empty() {
            self.master_chain.process(
                &mut master_l[..num_frames],
                &mut master_r[..num_frames],
                &context,
                &mut self.fx_scratch,
            );
        }

        // Write tracks to interleaved output
        for i in 0..num_frames {
            output[i * 2] = master_l[i];
            output[i * 2 + 1] = master_r[i];
        }

        // Return scratch buffers to self (no allocation, just moves)
        self.scratch_l = master_l;
        self.scratch_r = master_r;

        // Mix metronome click into output (after tracks, so it's always audible)
        //
        // The count-in owns this spot while it runs: the transport is still
        // stopped — no clips, no recording — and only the countdown's click
        // sounds. The countdown is consumed here on the audio thread, and
        // the block it reaches zero on is the block that fires the
        // transport, which is what lands the take's first downbeat exactly
        // on the bar rather than a UI frame late.
        if transport.is_counting_in() {
            if !self.was_counting {
                self.metronome.reset();
            }
            let elapsed = transport.count_in_total_ticks() - transport.count_in_remaining();
            self.metronome.count_in(output, elapsed, transport);
            let fired = transport.consume_count_in(num_frames as u32, self.sample_rate);
            self.was_counting = !fired;
        } else if let Some((bpm, pattern)) = self.practice_click {
            // The practice room's click owns the metronome while it is set:
            // free-running on its own beat phase, transport ignored, so a
            // player can drill with the song stopped — or with it playing,
            // where the ordinary metronome would double it.
            self.was_counting = false;
            let mut phase = self.practice_beat_phase;
            self.metronome.practice_click(output, bpm, pattern, &mut phase);
            self.practice_beat_phase = phase;
        } else {
            self.was_counting = false;
            self.metronome.process(output, transport);
        }

        // ── Master limiter ──
        // Everything that reaches the device passes through here, the
        // metronome included: it is summed on top of the track mix, so
        // limiting before it would leave a gap in the guarantee.
        self.limiter.process(output);

        // What it took off, as a number a meter can draw. Computed here
        // rather than in the UI because only this side sees every sample —
        // see `fx::GrBallistics`.
        self.limiter_gr.publish(
            &self.limiter_gr_meter,
            self.limiter.block_min_gain,
            num_frames,
            self.sample_rate as f32,
        );

        // Master VU (includes metronome), read after limiting so the meter
        // shows what actually left rather than what would have.
        let mut mp_l = 0.0f32;
        let mut mp_r = 0.0f32;
        for i in 0..num_frames {
            mp_l = mp_l.max(output[i * 2].abs());
            mp_r = mp_r.max(output[i * 2 + 1].abs());
        }

        publish_vu(&self.master_vu, mp_l, mp_r);
        if let Some(handle) = &self.master_handle {
            publish_vu(&handle.vu, mp_l, mp_r);
        }
    }

    /// Test-only: read a MIDI effect's parameter as the audio thread holds
    /// it, so a test can prove a command landed rather than assume it.
    #[cfg(any(test, feature = "test-introspection"))]
    pub fn midi_fx_param(&self, track_id: usize, slot: usize, index: usize) -> Option<f32> {
        let track = self.tracks.iter().find(|t| t.id == track_id)?;
        Some(track.midi_fx.get(slot)?.fx.get_parameter(index))
    }

    pub fn reset_all(&mut self) {
        let clip_tx = &self.clip_tx;
        for track in &mut self.tracks {
            if let Some(ref mut inst) = track.instrument {
                inst.reset();
            }
            // A panic kills the tails too. A reverb still ringing after the
            // instruments have been silenced is exactly the sound the panic
            // key exists to stop.
            track.chain.reset();
            // And the MIDI effects drop their state silently — a latched arp
            // still firing note-ons a millisecond after the panic is the
            // worst possible version of a panic that does not panic.
            for slot in &mut track.midi_fx {
                slot.fx.reset();
            }
            track.midi_fx_owed.clear();
            track.handle.vu.set(0.0, 0.0);
            // Commit any active recording before resetting (don't lose overdubs)
            if track.record_buf.is_active() && track.was_recording {
                let end_tick = track.last_record_tick.max(0);
                commit_recording(track, end_tick, clip_tx);
            } else if track.record_buf.is_active() {
                track.record_buf.discard();
            }
            track.was_recording = false;
            // A panic resets the instruments underneath the sequencer, so the
            // notes it is holding are already gone: the table is dropped
            // rather than sounded, which would only send offs to voices that
            // no longer exist.
            if let Some(ref mut player) = track.pattern {
                player.silence();
            }
        }
        for bus in [&mut self.bus_a, &mut self.bus_b] {
            bus.chain.reset();
            bus.buf_l.fill(0.0);
            bus.buf_r.fill(0.0);
            if let Some(handle) = &bus.handle {
                handle.vu.set(0.0, 0.0);
            }
        }
        self.master_chain.reset();
        if let Some(handle) = &self.master_handle {
            handle.vu.set(0.0, 0.0);
        }
        self.last_window = None;
        self.key_listen = None;
        self.metronome.reset();
        self.limiter.reset();
        self.limiter_gr.reset();
        self.limiter_gr_meter.reset();
    }

    /// Apply queued commands until the callback's budget is spent.
    ///
    /// Returns the units spent, which is what the tests assert the bound on.
    ///
    /// Anything left in the channel stays there, in the order it was sent, and
    /// the next callback continues from it. That is the whole of the ordering
    /// guarantee: commands are taken one at a time from a FIFO and applied
    /// immediately, so `AddTrack` before `SetInstrument` for the same track
    /// cannot be seen the other way round even when the two land in different
    /// callbacks.
    fn drain_commands(&mut self) -> u32 {
        let mut spent = 0;
        while spent < COMMAND_BUDGET {
            let Ok(cmd) = self.command_rx.try_recv() else { break };
            spent += command_cost(&cmd);
            self.apply_command(cmd);
        }
        spent
    }

    fn apply_command(&mut self, cmd: MixerCommand) {
        match cmd {
            // The `kind` finally decides something: a bus handle attaches to
            // the strip it names instead of becoming a track. The two send
            // buses and the master are single, permanent strips — a second
            // `AddTrack` for one of them replaces the handle rather than
            // making a second bus.
            MixerCommand::AddTrack { kind, handle } => match kind {
                TrackKind::SendA => self.bus_a.handle = Some(handle),
                TrackKind::SendB => self.bus_b.handle = Some(handle),
                // The master's level already goes to the engine-wide
                // `master_vu`; the handle is so the master's own row on the
                // track strip can draw the same meter. Its fader is not read:
                // the only gain between the mix and the device is the
                // limiter, which is a safety device and not a control.
                TrackKind::Master => self.master_handle = Some(handle),
                TrackKind::Instrument | TrackKind::Audio => {
                    let track = AudioTrack::new(handle, self.sample_rate, self.max_buffer_size);
                    self.tracks.push(track);
                }
            },
            MixerCommand::SetInstrument { track_id, mut instrument } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    instrument.init(self.sample_rate as f64, self.max_buffer_size);
                    track.instrument = Some(instrument);
                }
            }
            MixerCommand::RemoveTrack { track_id } => {
                self.tracks.retain(|t| t.id != track_id);
            }
            MixerCommand::SetParameter { track_id, param_index, value } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(ref mut inst) = track.instrument {
                        inst.set_parameter(param_index, value);
                    }
                }
            }
            MixerCommand::CreateClip { track_id, start_tick, length_ticks } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    track.clips.push(MidiClip::new(start_tick, length_ticks, Vec::new()));
                }
            }
            MixerCommand::UpdateClip { track_id, clip_index, events } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(clip) = track.clips.get_mut(clip_index) {
                        clip.events = events;
                        // Offs first, controllers between, ons last — see
                        // `clip::same_tick_order`.
                        clip.events.sort_by_key(|e| (e.tick, crate::clip::same_tick_order(e.status)));
                    }
                }
            }
            MixerCommand::UpdateClipPosition { track_id, clip_index, start_tick, length_ticks } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(clip) = track.clips.get_mut(clip_index) {
                        clip.start_tick = start_tick;
                        clip.length_ticks = length_ticks;
                    }
                }
            }
            MixerCommand::RemoveClip { track_id, clip_index } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if clip_index < track.clips.len() {
                        track.clips.remove(clip_index);
                    }
                }
            }
            MixerCommand::DiscardRecording => {
                self.discard_recording = true;
            }
            MixerCommand::SetPattern { track_id, slot, block } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    let player = track.pattern.get_or_insert_with(|| Box::new(PatternPlayer::new()));
                    player.apply(slot, block);
                }
            }

            // ── Inserts ──
            MixerCommand::AddFx { target, slot, mut effect } => {
                let (sample_rate, max_buffer_size) = (self.sample_rate, self.max_buffer_size);
                if let Some(chain) = self.chain_mut(target) {
                    effect.init(f64::from(sample_rate), max_buffer_size);
                    // A full chain hands the effect back, and it is dropped
                    // here. The UI is expected to have refused already — this
                    // is the audio thread declining to grow a `Vec` for a
                    // seventh effect it was told it would never be sent.
                    drop(chain.insert(slot, effect));
                }
            }
            MixerCommand::RemoveFx { target, slot } => {
                if let Some(chain) = self.chain_mut(target) {
                    drop(chain.remove(slot));
                }
            }
            MixerCommand::AddMidiFx { track_id, slot, mut fx } => {
                let sr = self.sample_rate;
                let max_buffer_size = self.max_buffer_size;
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    fx.init(f64::from(sr), max_buffer_size);
                    let slot = slot.min(track.midi_fx.len());
                    if track.midi_fx.len() < crate::midi_fx::MAX_MIDI_FX_SLOTS {
                        track
                            .midi_fx
                            .insert(slot, crate::midi_fx::MidiFxSlot { fx, bypassed: false });
                    }
                    // A full rack drops the newcomer — the UI is expected to
                    // have refused already.
                }
            }
            MixerCommand::RemoveMidiFx { track_id, slot } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if slot < track.midi_fx.len() {
                        let mut gone = track.midi_fx.remove(slot);
                        gone.fx.flush(&mut track.midi_fx_owed);
                        drop(gone);
                    }
                }
            }
            MixerCommand::SetMidiFxParam { track_id, slot, param_index, value } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(s) = track.midi_fx.get_mut(slot) {
                        s.fx.set_parameter(param_index, value);
                    }
                }
            }
            MixerCommand::SetPracticeClick { bpm, pattern } => {
                if bpm > 0.0 {
                    self.practice_click = Some((bpm, pattern));
                    self.practice_beat_phase = 0.0;
                    self.metronome.reset();
                } else {
                    self.practice_click = None;
                }
            }
            MixerCommand::SetMidiFxProgression { track_id, slot, chords } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(s) = track.midi_fx.get_mut(slot) {
                        s.fx.set_progression(&chords);
                    }
                }
            }
            MixerCommand::SetSamplerPad { track_id, pad, config, layers } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(instrument) = track.instrument.as_mut() {
                        instrument.set_sampler_pad(pad, &config, &layers);
                    }
                }
            }
            MixerCommand::SetSamplerRange { track_id, pads } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(instrument) = track.instrument.as_mut() {
                        // Bounded by the bed, whatever the sender claims —
                        // see [`MAX_RANGE_PADS`]. Both halves of each pad go
                        // together: a range that shipped the layers and not
                        // the phrases would leave a performance playing on a
                        // key the player has just emptied.
                        for (pad, config, layers, phrases) in pads.into_iter().take(MAX_RANGE_PADS)
                        {
                            instrument.set_sampler_pad(pad, &config, &layers);
                            instrument.set_sampler_phrases(pad, &phrases);
                        }
                    }
                }
            }
            MixerCommand::SetSamplerPreview { track_id, preview } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(instrument) = track.instrument.as_mut() {
                        instrument.set_sampler_preview(preview.as_ref());
                    }
                }
            }
            MixerCommand::SetSamplerChild { track_id, child } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(instrument) = track.instrument.as_mut() {
                        // The sampler starts it, because the sampler is what
                        // knows the rate and the block size it was given.
                        instrument.set_sampler_child(child.map(|c| c as Box<dyn Plugin>));
                    }
                }
            }
            MixerCommand::SetSamplerChildParam { track_id, param_index, value } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(instrument) = track.instrument.as_mut() {
                        instrument.set_sampler_child_param(param_index, value);
                    }
                }
            }
            MixerCommand::SetSamplerPhrases { track_id, pad, phrases } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(instrument) = track.instrument.as_mut() {
                        instrument.set_sampler_phrases(pad, &phrases);
                    }
                }
            }
            MixerCommand::SetMidiFxBypass { track_id, slot, bypassed } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(s) = track.midi_fx.get_mut(slot) {
                        if bypassed && !s.bypassed {
                            s.fx.flush(&mut track.midi_fx_owed);
                        }
                        s.bypassed = bypassed;
                    }
                }
            }
            MixerCommand::MoveFx { target, from, to } => {
                if let Some(chain) = self.chain_mut(target) {
                    chain.move_slot(from, to);
                }
            }
            MixerCommand::SetFxParam { target, slot, param, value } => {
                if let Some(chain) = self.chain_mut(target) {
                    chain.set_parameter(slot, param, value);
                }
            }
            MixerCommand::SetFxBypass { target, slot, bypass } => {
                if let Some(chain) = self.chain_mut(target) {
                    chain.set_bypass(slot, bypass);
                }
            }

            // ── Sends, pan, key ──
            MixerCommand::SetSendLevel { track_id, send, gain } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    // Clamped here rather than at the call site, for the same
                    // reason `TrackConfig::set_volume` is: this multiplies
                    // every sample of a bus feed, and a UI arithmetic slip
                    // would otherwise be a full-scale burst.
                    let gain = if gain.is_nan() { 0.0 } else { gain.clamp(0.0, 1.0) };
                    track.send[match send {
                        SendSlot::A => 0,
                        SendSlot::B => 1,
                    }] = gain;
                }
            }
            MixerCommand::SetPan { track_id, pan } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    track.pan = if pan.is_nan() { 0.0 } else { pan.clamp(-1.0, 1.0) };
                }
            }
            // Setting one clears the other by construction — see the field.
            // A track the mixer has never heard of is refused rather than
            // stored, so the flag cannot outlive the track it names.
            MixerCommand::SetKeyListen { track } => {
                self.key_listen =
                    track.filter(|id| self.tracks.iter().any(|t| t.id == *id && !is_bus(t.kind)));
            }
            MixerCommand::SetKeySource { track_id, source } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    // A track keyed to itself is keyed to nothing: the tap it
                    // would read is its own pre-insert signal, which is the
                    // internal key by another name.
                    track.key_source = source.filter(|id| *id != track_id);
                }
            }
        }
    }
}

/// Whether a track kind is one of the strips the mix returns *into*, rather
/// than one that feeds it.
fn is_bus(kind: TrackKind) -> bool {
    matches!(kind, TrackKind::SendA | TrackKind::SendB | TrackKind::Master)
}

/// Whether a strip's fader passes signal this block.
///
/// Solo is a statement about the tracks, not about the mix bus: a soloed
/// track still wants its reverb, and a master that solo could silence would
/// make the whole feature a mute button. So the buses and the master answer
/// to mute alone.
fn is_audible(kind: TrackKind, muted: bool, soloed: bool, any_solo: bool) -> bool {
    if muted {
        return false;
    }
    is_bus(kind) || !any_solo || soloed
}

/// One VU update: fast attack, slow decay. The decay is per callback rather
/// than per second — it always has been — so a meter falls at a rate that
/// depends on the block size. Left as it was; changing it would move every
/// meter in the application in a milestone about routing.
///
/// The comparison is `>=` rather than `>`, and that one character is a fix.
/// A steady signal produces the same peak every block; with a strict `>` the
/// meter took the decay branch on every one of them, so it alternated
/// between the true peak and 85% of it forever — a needle that flickers on a
/// tone that is not moving. Holding on equality cannot make a meter read
/// high: it only ever holds a level the signal is still producing.
fn publish_vu(vu: &VuLevels, peak_l: f32, peak_r: f32) {
    let (old_l, old_r) = vu.get();
    let decay = 0.85f32;
    vu.set(
        if peak_l >= old_l { peak_l } else { old_l * decay },
        if peak_r >= old_r { peak_r } else { old_r * decay },
    );
}

/// Return a send bus into the master: its return level, its meter, and the
/// sum. Its inserts have already run — see [`inserts::run_bus`].
///
/// Skipped whole when nothing was sent to it and it has no effects in it. An
/// empty bus that still ran would add `+0.0` to every sample of the master —
/// harmless arithmetically, but it is the difference between "a session with
/// no sends renders exactly as it did before sends existed" being a
/// guarantee and being a claim about floating-point zero.
fn return_bus(bus: &mut BusStrip, master_l: &mut [f32], master_r: &mut [f32]) {
    if !bus.fed && bus.chain.is_empty() {
        if let Some(handle) = &bus.handle {
            publish_vu(&handle.vu, 0.0, 0.0);
        }
        return;
    }

    let frames = master_l.len();

    let gain = bus.return_gain();
    let mut peak_l = 0.0f32;
    let mut peak_r = 0.0f32;
    for i in 0..frames {
        let l = bus.buf_l[i] * gain;
        let r = bus.buf_r[i] * gain;
        peak_l = peak_l.max(l.abs());
        peak_r = peak_r.max(r.abs());
        master_l[i] += l;
        master_r[i] += r;
    }

    // The bus meter reads after its own chain and its return level, which is
    // the level it actually contributes to the mix.
    if let Some(handle) = &bus.handle {
        publish_vu(&handle.vu, peak_l, peak_r);
    }
}

/// Commit a recording buffer into a clip and send snapshot to UI.
fn commit_recording(track: &mut AudioTrack, end_tick: i64, clip_tx: &Sender<ClipSnapshot>) {
    if let Some(clip) = track.record_buf.commit(end_tick) {
        let idx = track.clips.len();
        tracing::debug!(
            "rec commit track={}: {} events, ticks {}..{}",
            track.id, clip.events.len(), clip.start_tick, clip.end_tick()
        );
        let snapshot = ClipSnapshot::from_clip(track.id, idx, &clip);
        track.clips.push(clip);
        let _ = clip_tx.send(snapshot);
    }
}

/// Which live MIDI messages reach a plugin.
///
/// Channel pressure is here because instruments route it: the Prophet-6 has
/// an aftertouch section with six destinations and an amount that reads as
/// bipolar, and every one of its 500 factory programs stores a setting for
/// it. It is a two-byte message, so `raw[2]` is whatever the parser left
/// there and a plugin reads the pressure from `data1`, as the MIDI
/// specification puts it.
///
/// The oldest arrival stamp the recorder will believe, in microseconds.
/// Two blocks at the largest common buffer size (2048 frames at 44.1k is
/// about 46ms) plus headroom. A stamp older than this is not a note that
/// waited — it is a clock from another domain, a stale ring, or a hand-built
/// message, and the honest fallback is the block edge, exactly as before.
const MAX_EVENT_AGE_MICROS: u64 = 120_000;

/// How many ticks into the past an event actually happened, from its
/// arrival stamp. Zero whenever the stamp is missing or unbelievable, which
/// makes the correction strictly opt-in: only messages stamped by the
/// receipt site move off the block edge.
fn event_age_ticks(
    received_micros: Option<u64>,
    drain_micros: u64,
    ticks_per_sample: f64,
    sample_rate: u32,
) -> i64 {
    let Some(stamp) = received_micros else { return 0 };
    let age_micros = drain_micros.saturating_sub(stamp);
    if age_micros > MAX_EVENT_AGE_MICROS {
        return 0;
    }
    let frames = age_micros as f64 * sample_rate as f64 / 1_000_000.0;
    (frames * ticks_per_sample) as i64
}

/// Polyphonic key pressure is *not* here, and that is the instruments rather
/// than an oversight — the Prophet-6 provides "monophonic (or 'channel')
/// aftertouch" and nothing in the rack has a per-key pressure destination.
/// `phosphor-midi` does not parse it into a variant of its own either.
pub fn midi_to_plugin_event(msg: &MidiMessage) -> Option<MidiEvent> {
    use phosphor_midi::message::MidiMessageType;
    match msg.message_type {
        MidiMessageType::NoteOn { .. }
        | MidiMessageType::NoteOff { .. }
        | MidiMessageType::ControlChange { .. }
        | MidiMessageType::PitchBend { .. }
        | MidiMessageType::ChannelPressure { .. } => Some(MidiEvent {
            sample_offset: 0,
            status: msg.raw[0],
            data1: msg.raw[1],
            data2: msg.raw[2],
        }),
        _ => None,
    }
}

pub fn mixer_command_channel() -> (Sender<MixerCommand>, Receiver<MixerCommand>) {
    crossbeam_channel::unbounded()
}

/// Create a channel for clip snapshots (audio → UI).
pub fn clip_snapshot_channel() -> (Sender<ClipSnapshot>, Receiver<ClipSnapshot>) {
    crossbeam_channel::unbounded()
}

mod inserts;

#[cfg(test)]
mod tests;
