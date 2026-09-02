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
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    plugin_events: Vec<MidiEvent>,
}

impl AudioTrack {
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
            plugin_events: Vec::with_capacity(PLUGIN_EVENT_CAPACITY),
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
    sample_rate: u32,
    max_buffer_size: usize,
    /// Pre-allocated scratch buffers for mix — avoids allocation in process().
    scratch_l: Vec<f32>,
    scratch_r: Vec<f32>,
    /// Pre-allocated buffer for live MIDI conversion.
    live_events: Vec<MidiEvent>,
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
    /// Where a track's signal is worked on in pass two, so that its own
    /// buffers stay as the instrument left them — which is what makes every
    /// sidechain key a same-block, order-independent read. See
    /// [`Mixer::process`].
    work_l: Vec<f32>,
    work_r: Vec<f32>,
    /// The dry copy a bypass crossfade needs, borrowed by whichever chain is
    /// running. One per mixer: only one slot is ever mid-fade at a time
    /// inside a single call.
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
            sample_rate,
            max_buffer_size,
            scratch_l: vec![0.0; max_buffer_size],
            scratch_r: vec![0.0; max_buffer_size],
            live_events: Vec::with_capacity(256),
            last_window: None,
            limiter: MasterLimiter::new(sample_rate),
            limiter_gr: GrBallistics::new(),
            limiter_gr_meter: Arc::new(GrMeter::new()),
            bus_a: BusStrip::new(sample_rate, max_buffer_size),
            bus_b: BusStrip::new(sample_rate, max_buffer_size),
            master_chain: FxChain::new(sample_rate),
            master_handle: None,
            work_l: vec![0.0; max_buffer_size],
            work_r: vec![0.0; max_buffer_size],
            fx_scratch: FxScratch::new(max_buffer_size),
            key_listen: None,
            was_playing: false,
            discard_recording: false,
        }
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
    /// track's own buffers *are* the key tap, so the inserts run on a
    /// scratch pair (`work_l`/`work_r`) instead of in place. That is one
    /// memcpy per track per block, and it is what buys order-independence.
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

        // Convert live MIDI to plugin events (reuse pre-allocated buffer)
        self.live_events.clear();
        for msg in midi_messages {
            if let Some(ev) = midi_to_plugin_event(msg) {
                self.live_events.push(ev);
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
        // pre-insert, and still there when pass 2 reads it.
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

            // Record live MIDI events (and pass through for monitoring)
            if is_midi_active {
                for ev in &live_events {
                    track.plugin_events.push(*ev);
                    if should_record {
                        let event_tick = current_tick
                            + (ev.sample_offset as f64 * ticks_per_sample) as i64;
                        track.record_buf.record(event_tick, ev.status, ev.data1, ev.data2);
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

            // Track position for wrap detection (used by both recording and playback)
            if playing {
                track.last_record_tick = current_tick;
            }

            // ── Process instrument (allocation-free) ──
            if let Some(ref mut instrument) = track.instrument {
                let out_l = &mut track.buf_l[..num_frames];
                let out_r = &mut track.buf_r[..num_frames];
                let mut out_slices: [&mut [f32]; 2] = [out_l, out_r];
                instrument.process(&[], &mut out_slices, &track.plugin_events);
            }
        }
        self.live_events = live_events;

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
            let Self { tracks, bus_a, bus_b, work_l, work_r, fx_scratch, key_listen, .. } = self;
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
            if work_l.len() < num_frames {
                work_l.resize(num_frames, 0.0);
                work_r.resize(num_frames, 0.0);
            }

            for index in 0..tracks.len() {
                let (before, from_here) = tracks.split_at_mut(index);
                let Some((track, after)) = from_here.split_first_mut() else { break };

                // ── The sidechain key ──
                //
                // Resolved from the stored identity to a position every
                // block, never cached: a track that has been deleted must
                // fall back to the internal key *this* block rather than
                // reading whatever now sits where it used to. A key that
                // names this same track is no key at all.
                // Monitoring the key needs it resolved whether or not
                // anything in the chain asked for one — the point of the
                // switch is to hear what a compressor *would* be keying off,
                // including on a track that has not got one yet.
                let listening = key_listen == Some(track.id);
                let key = track
                    .key_source
                    .filter(|_| track.chain.wants_key() || listening)
                    .and_then(|id| {
                        before
                            .iter()
                            .find(|t| t.id == id)
                            .or_else(|| after.iter().find(|t| t.id == id))
                    })
                    .map(|source| {
                        (
                            &source.buf_l[..num_frames],
                            &source.buf_r[..num_frames],
                        )
                    });

                // The inserts run on a copy so that `buf_l`/`buf_r` stay as
                // the instrument left them — see this function's doc.
                work_l[..num_frames].copy_from_slice(&track.buf_l[..num_frames]);
                work_r[..num_frames].copy_from_slice(&track.buf_r[..num_frames]);
                if !track.chain.is_empty() {
                    let ctx = FxContext { key, ..context };
                    track.chain.process(
                        &mut work_l[..num_frames],
                        &mut work_r[..num_frames],
                        &ctx,
                        fx_scratch,
                    );
                }

                // ── Key listen ──
                //
                // The key replaces this track's signal, *after* the chain has
                // run — so the gain-reduction meter goes on moving while the
                // key is being auditioned, which is most of what the switch
                // is for. What is heard is the key as the sidechain tap
                // defines it: post-instrument, pre-insert, and this track's
                // own signal when no other track has been named.
                //
                // It is here and not inside the compressor deliberately. A
                // slot cannot replace the track's output, and a version that
                // wrote the key into the buffer from inside the chain would
                // make what you hear depend on which effects happen to sit
                // after it. The cost is that the detector's high-pass is not
                // in what you hear; the compensation is that what you hear
                // does not change when you add a delay.
                if listening {
                    match key {
                        Some((left, right)) => {
                            work_l[..num_frames].copy_from_slice(left);
                            work_r[..num_frames].copy_from_slice(right);
                        }
                        None => {
                            work_l[..num_frames].copy_from_slice(&track.buf_l[..num_frames]);
                            work_r[..num_frames].copy_from_slice(&track.buf_r[..num_frames]);
                        }
                    }
                }

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
            for bus in [&mut *bus_a, &mut *bus_b] {
                render_bus(
                    bus,
                    &mut master_l[..num_frames],
                    &mut master_r[..num_frames],
                    &context,
                    fx_scratch,
                );
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
        self.metronome.process(output, transport);

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
                        // Offs before ons at the same tick — see MidiClip::new.
                        clip.events.sort_by_key(|e| (e.tick, e.status & 0xF0));
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

/// Run a send bus: its inserts, its return level, its meter, and into the
/// master.
///
/// Skipped whole when nothing was sent to it and it has no effects in it. An
/// empty bus that still ran would add `+0.0` to every sample of the master —
/// harmless arithmetically, but it is the difference between "a session with
/// no sends renders exactly as it did before sends existed" being a
/// guarantee and being a claim about floating-point zero.
fn render_bus(
    bus: &mut BusStrip,
    master_l: &mut [f32],
    master_r: &mut [f32],
    context: &FxContext<'_>,
    scratch: &mut FxScratch,
) {
    if !bus.fed && bus.chain.is_empty() {
        if let Some(handle) = &bus.handle {
            publish_vu(&handle.vu, 0.0, 0.0);
        }
        return;
    }

    let frames = master_l.len();
    bus.chain.process(
        &mut bus.buf_l[..frames],
        &mut bus.buf_r[..frames],
        context,
        scratch,
    );

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpal_backend::{Requested, StreamFormat};
    use crate::project::TrackConfig;
    use phosphor_dsp::synth::PhosphorSynth;
    use phosphor_midi::message::{MidiMessage, MidiMessageType};

    fn make_note_on(note: u8, vel: u8) -> MidiMessage {
        MidiMessage {
            timestamp: Some(0),
            message_type: MidiMessageType::NoteOn { channel: 0, note, velocity: vel },
            raw: [0x90, note, vel],
            len: 3,
        }
    }

    /// Aftertouch has to reach a plugin, or an instrument with an aftertouch
    /// section has one that never does anything.
    #[test]
    fn channel_pressure_reaches_the_plugin_and_key_pressure_does_not() {
        let pressure = MidiMessage {
            timestamp: Some(0),
            message_type: MidiMessageType::ChannelPressure { channel: 0, pressure: 96 },
            raw: [0xD0, 96, 0],
            len: 2,
        };
        let event = midi_to_plugin_event(&pressure).expect("channel pressure is dropped");
        assert_eq!(event.status, 0xD0);
        assert_eq!(event.data1, 96);

        // Polyphonic key pressure parses as `Other` and stays there: nothing
        // in the rack has a per-key pressure destination.
        let key = MidiMessage::from_bytes(&[0xA0, 60, 96], 0).expect("parsed");
        assert!(
            midi_to_plugin_event(&key).is_none(),
            "polyphonic key pressure has no destination in the rack"
        );
    }

    fn make_note_off(note: u8) -> MidiMessage {
        MidiMessage {
            timestamp: Some(0),
            message_type: MidiMessageType::NoteOff { channel: 0, note, velocity: 0 },
            raw: [0x80, note, 0],
            len: 3,
        }
    }

    fn setup_mixer() -> (Mixer, Sender<MixerCommand>, Receiver<ClipSnapshot>, Arc<Transport>) {
        let (tx, rx) = mixer_command_channel();
        let (clip_tx, clip_rx) = clip_snapshot_channel();
        let master_vu = Arc::new(VuLevels::new());
        let transport = Arc::new(Transport::new(120.0));
        let mixer = Mixer::new(rx, master_vu, clip_tx, 44100, 256);
        (mixer, tx, clip_rx, transport)
    }

    fn add_armed_synth(tx: &Sender<MixerCommand>, id: usize) -> Arc<TrackHandle> {
        let handle = Arc::new(TrackHandle::new(id, TrackKind::Instrument));
        handle.config.midi_active.store(true, std::sync::atomic::Ordering::Relaxed);
        handle.config.armed.store(true, std::sync::atomic::Ordering::Relaxed);
        tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle: handle.clone() }).unwrap();
        tx.send(MixerCommand::SetInstrument { track_id: id, instrument: Box::new(PhosphorSynth::new()) }).unwrap();
        handle
    }

    #[test]
    fn mixer_empty_output() {
        let (mut mixer, _tx, _clip_rx, transport) = setup_mixer();
        let mut output = vec![0.0f32; 128];
        mixer.process(&mut output, &[], &transport);
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn mixer_live_midi_produces_sound() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let _handle = add_armed_synth(&tx, 0);
        transport.play();

        let midi = vec![make_note_on(60, 100)];
        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &midi, &transport);

        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        // Threshold is "not silence", not a level check — the instruments
        // carry a deep headroom trim on their output.
        assert!(peak > 0.001, "Should produce sound, peak={peak}");
    }

    #[test]
    fn mixer_records_midi_clip() {
        let (mut mixer, tx, clip_rx, transport) = setup_mixer();
        let _handle = add_armed_synth(&tx, 0);
        transport.play();
        transport.toggle_record();

        // Play a note while recording
        let midi = vec![make_note_on(60, 100)];
        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &midi, &transport);

        // Note off
        let midi = vec![make_note_off(60)];
        mixer.process(&mut output, &midi, &transport);

        // Stop recording
        transport.toggle_record();
        mixer.process(&mut output, &[], &transport);

        // Should have received a clip snapshot
        let snap = clip_rx.try_recv().expect("Should receive clip snapshot");
        assert_eq!(snap.track_id, 0);
        assert!(snap.event_count >= 2, "Should have note on + off, got {}", snap.event_count);
        assert!(!snap.notes.is_empty(), "Should have parsed notes");
    }

    #[test]
    fn mixer_plays_back_recorded_clip() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let _handle = add_armed_synth(&tx, 0);
        transport.play();
        transport.toggle_record();

        // Record a note
        let midi = vec![make_note_on(60, 100)];
        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &midi, &transport);

        let midi = vec![make_note_off(60)];
        mixer.process(&mut output, &midi, &transport);

        // Stop recording
        transport.toggle_record();
        mixer.process(&mut output, &[], &transport);

        // Stop and rewind
        transport.stop();

        // Play back — should hear the recorded clip
        transport.play();
        output.fill(0.0);
        mixer.process(&mut output, &[], &transport);

        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001, "Playback should produce sound, peak={peak}");
    }

    #[test]
    fn mixer_mute_silences() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let handle = add_armed_synth(&tx, 0);
        handle.config.muted.store(true, std::sync::atomic::Ordering::Relaxed);
        transport.play();

        let midi = vec![make_note_on(60, 100)];
        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &midi, &transport);

        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak == 0.0, "Muted track should be silent, peak={peak}");
    }

    #[test]
    fn mixer_no_record_when_not_armed() {
        let (mut mixer, tx, clip_rx, transport) = setup_mixer();
        let handle = add_armed_synth(&tx, 0);
        handle.config.armed.store(false, std::sync::atomic::Ordering::Relaxed);
        transport.play();
        transport.toggle_record();

        let midi = vec![make_note_on(60, 100)];
        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &midi, &transport);

        transport.toggle_record();
        mixer.process(&mut output, &[], &transport);

        assert!(clip_rx.try_recv().is_err(), "Should not record when not armed");
    }

    #[test]
    fn mixer_reset_commits_recording() {
        let (mut mixer, tx, clip_rx, transport) = setup_mixer();
        let _handle = add_armed_synth(&tx, 0);
        transport.play();
        transport.toggle_record();

        let midi = vec![make_note_on(60, 100)];
        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &midi, &transport);

        mixer.reset_all();

        // Reset should commit the active recording, not discard it
        assert!(clip_rx.try_recv().is_ok(), "Reset should commit active recording");
    }

    #[test]
    fn end_to_end_record_and_playback() {
        // Simulates exact app flow: add track, arm, record, play notes,
        // stop, rewind, play back — with transport.advance() each buffer.
        let (mut mixer, tx, clip_rx, transport) = setup_mixer();
        let _handle = add_armed_synth(&tx, 0);
        let sr = 44100u32;
        let buf_frames = 256;
        let buf_samples = buf_frames * 2; // stereo

        // 1. Enable recording, then play
        transport.toggle_record();
        transport.play();

        // 2. Process a few empty buffers (advance transport)
        let mut output = vec![0.0f32; buf_samples];
        for _ in 0..4 {
            mixer.process(&mut output, &[], &transport);
            transport.advance(buf_frames as u32, sr);
        }

        // 3. Play a note (should be recorded)
        let midi = vec![make_note_on(60, 100)];
        mixer.process(&mut output, &midi, &transport);
        let peak_during = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak_during > 0.001, "Should hear note during recording (monitoring)");
        transport.advance(buf_frames as u32, sr);

        // 4. A few more buffers of sustain
        for _ in 0..8 {
            output.fill(0.0);
            mixer.process(&mut output, &[], &transport);
            transport.advance(buf_frames as u32, sr);
        }

        // 5. Note off
        let midi = vec![make_note_off(60)];
        mixer.process(&mut output, &midi, &transport);
        transport.advance(buf_frames as u32, sr);

        // 6. A few more buffers
        for _ in 0..4 {
            output.fill(0.0);
            mixer.process(&mut output, &[], &transport);
            transport.advance(buf_frames as u32, sr);
        }

        // 7. Stop recording (commit clip)
        transport.toggle_record();
        mixer.process(&mut output, &[], &transport);
        transport.advance(buf_frames as u32, sr);

        // 8. Check we got a clip snapshot
        let snap = clip_rx.try_recv().expect("Should receive clip snapshot after stopping record");
        assert!(snap.event_count >= 2, "Clip should have note on + off");
        assert!(!snap.notes.is_empty(), "Clip should have parsed notes");

        // 9. Stop transport and rewind to 0
        transport.stop();

        // 10. Play back — the synth should be reset (no stuck notes from recording)
        transport.play();

        // 11. Process enough buffers to reach the recorded note position
        // The note was recorded after 4 initial buffers, so roughly at that tick position
        for _ in 0..4 {
            output.fill(0.0);
            mixer.process(&mut output, &[], &transport);
            transport.advance(buf_frames as u32, sr);
        }

        // 12. The next buffer should contain the played-back note
        output.fill(0.0);
        mixer.process(&mut output, &[], &transport);
        let peak_playback = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak_playback > 0.001, "Playback should produce sound at the recorded position, peak={peak_playback}");
    }

    #[test]
    fn loop_record_commits_on_wrap() {
        let (mut mixer, tx, clip_rx, transport) = setup_mixer();
        let _handle = add_armed_synth(&tx, 0);
        let sr = 44100u32;
        let buf_frames = 256u32;

        // Set loop to 1 bar (3840 ticks at 120bpm ≈ 346 buffers of 256 samples)
        transport.set_loop_bars(1, 1);
        transport.start_loop_record();

        let mut output = vec![0.0f32; buf_frames as usize * 2];

        // Play a note early in the loop
        let midi = vec![make_note_on(60, 100)];
        mixer.process(&mut output, &midi, &transport);
        transport.advance(buf_frames, sr);

        // Note off a few buffers later
        for _ in 0..5 {
            mixer.process(&mut output, &[], &transport);
            transport.advance(buf_frames, sr);
        }
        let midi = vec![make_note_off(60)];
        mixer.process(&mut output, &midi, &transport);
        transport.advance(buf_frames, sr);

        // Continue until we cross the loop boundary
        // 1 bar at 120bpm, 256 frames, 44100Hz ≈ 346 buffers
        for _ in 0..400 {
            mixer.process(&mut output, &[], &transport);
            transport.advance(buf_frames, sr);

            if let Ok(snap) = clip_rx.try_recv() {
                assert!(snap.event_count >= 2, "Clip should have events, got {}", snap.event_count);
                assert!(!snap.notes.is_empty(), "Clip should have notes");
                // Recording committed on loop wrap — success
                transport.stop_loop_record();
                return;
            }
        }

        panic!("Recording should have committed when the loop wrapped");
    }

    #[test]
    fn loop_playback_after_record() {
        let (mut mixer, tx, clip_rx, transport) = setup_mixer();
        let _handle = add_armed_synth(&tx, 0);
        let sr = 44100u32;
        let bf = 256u32;

        // Set loop to 1 bar, start recording
        transport.set_loop_bars(1, 1);
        transport.start_loop_record();

        let mut output = vec![0.0f32; bf as usize * 2];

        // Record a note
        mixer.process(&mut output, &[make_note_on(60, 100)], &transport);
        transport.advance(bf, sr);
        for _ in 0..3 {
            mixer.process(&mut output, &[], &transport);
            transport.advance(bf, sr);
        }
        mixer.process(&mut output, &[make_note_off(60)], &transport);
        transport.advance(bf, sr);

        // Run until loop wraps and clip commits
        for _ in 0..200 {
            mixer.process(&mut output, &[], &transport);
            transport.advance(bf, sr);
            if clip_rx.try_recv().is_ok() { break; }
        }

        // Stop recording, rewind
        transport.stop_loop_record();
        transport.set_position(0);

        // Play back with looping on
        transport.toggle_loop(); // enable looping
        transport.play();

        output.fill(0.0);
        mixer.process(&mut output, &[], &transport);
        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001, "Should hear playback, peak={peak}");
    }

    /// Undo mid-pass: the notes already played this pass are gone, the notes
    /// played after the discard are the whole take. Recording never stops.
    #[test]
    fn discard_drops_the_uncommitted_pass_and_keeps_recording() {
        let (mut mixer, tx, clip_rx, transport) = setup_mixer();
        let _handle = add_armed_synth(&tx, 0);
        let sr = 44100u32;
        let bf = 256u32;

        transport.set_loop_bars(1, 1);
        transport.start_loop_record();
        let mut output = vec![0.0f32; bf as usize * 2];

        // The flubbed phrase: a note on and off, both in the buffer.
        mixer.process(&mut output, &[make_note_on(60, 100)], &transport);
        transport.advance(bf, sr);
        mixer.process(&mut output, &[make_note_off(60)], &transport);
        transport.advance(bf, sr);

        // Undo reaches the recorder.
        tx.send(MixerCommand::DiscardRecording).unwrap();
        mixer.process(&mut output, &[], &transport);
        transport.advance(bf, sr);

        // The replayed phrase.
        mixer.process(&mut output, &[make_note_on(62, 100)], &transport);
        transport.advance(bf, sr);
        mixer.process(&mut output, &[make_note_off(62)], &transport);
        transport.advance(bf, sr);

        // Round the loop: the commit is the replay alone.
        let mut snap = None;
        for _ in 0..400 {
            mixer.process(&mut output, &[], &transport);
            transport.advance(bf, sr);
            if let Ok(s) = clip_rx.try_recv() {
                snap = Some(s);
                break;
            }
        }
        let snap = snap.expect("the pass after a discard still commits at the wrap");
        assert!(
            snap.notes.iter().any(|n| n.note == 62),
            "the note played after the discard was lost"
        );
        assert!(
            snap.notes.iter().all(|n| n.note != 60),
            "the discarded note came back at the wrap"
        );
    }

    /// A discard racing a stop: the stop must not commit the notes the
    /// discard was sent to remove.
    #[test]
    fn discard_racing_a_stop_commits_nothing() {
        let (mut mixer, tx, clip_rx, transport) = setup_mixer();
        let _handle = add_armed_synth(&tx, 0);
        let sr = 44100u32;
        let bf = 256u32;

        transport.set_loop_bars(1, 1);
        transport.start_loop_record();
        let mut output = vec![0.0f32; bf as usize * 2];

        mixer.process(&mut output, &[make_note_on(60, 100)], &transport);
        transport.advance(bf, sr);
        mixer.process(&mut output, &[make_note_off(60)], &transport);
        transport.advance(bf, sr);

        // Both arrive before the next callback runs.
        tx.send(MixerCommand::DiscardRecording).unwrap();
        transport.stop_loop_record();
        mixer.process(&mut output, &[], &transport);

        assert!(
            clip_rx.try_recv().is_err(),
            "the stop committed a pass the discard had already scrapped"
        );
    }

    /// A discard with nothing recording is consumed without effect — and
    /// without poisoning the take that comes after it.
    #[test]
    fn discard_when_idle_is_a_noop() {
        let (mut mixer, tx, clip_rx, transport) = setup_mixer();
        let _handle = add_armed_synth(&tx, 0);
        let sr = 44100u32;
        let bf = 256u32;
        let mut output = vec![0.0f32; bf as usize * 2];

        // Idle discard: no transport, no recording.
        tx.send(MixerCommand::DiscardRecording).unwrap();
        mixer.process(&mut output, &[], &transport);
        assert!(clip_rx.try_recv().is_err());

        // A recording made afterwards commits exactly as it always did.
        transport.set_loop_bars(1, 1);
        transport.start_loop_record();
        mixer.process(&mut output, &[make_note_on(64, 100)], &transport);
        transport.advance(bf, sr);
        mixer.process(&mut output, &[make_note_off(64)], &transport);
        transport.advance(bf, sr);
        transport.stop_loop_record();
        mixer.process(&mut output, &[], &transport);

        let snap = clip_rx.try_recv().expect("the take after an idle discard was lost");
        assert!(snap.notes.iter().any(|n| n.note == 64));
    }

    // ── Command budget ──

    /// The most work one callback can do, in the units [`command_cost`]
    /// returns: the budget is tested before a command is taken and charged
    /// after, so the last one can overshoot by its own cost.
    const WORST_CALLBACK: u32 = COMMAND_BUDGET - 1 + HEAVY_COMMAND;

    /// A plugin that remembers every parameter it was given, in order, so a
    /// test can see exactly what reached the audio thread and when.
    ///
    /// The lock is not something an instrument would do — nothing may block in
    /// `process` — but `set_parameter` is called from the command drain and
    /// this one never renders.
    #[derive(Clone)]
    struct ParamLog(Arc<std::sync::Mutex<Vec<(usize, f32)>>>);

    impl ParamLog {
        fn new() -> Self {
            Self(Arc::new(std::sync::Mutex::new(Vec::new())))
        }
        fn seen(&self) -> Vec<(usize, f32)> {
            self.0.lock().unwrap().clone()
        }
    }

    impl Plugin for ParamLog {
        fn info(&self) -> phosphor_plugin::PluginInfo {
            phosphor_plugin::PluginInfo {
                name: "ParamLog".into(),
                version: "0".into(),
                author: "test".into(),
                category: phosphor_plugin::PluginCategory::Instrument,
            }
        }
        fn init(&mut self, _sample_rate: f64, _max_buffer_size: usize) {}
        fn process(&mut self, _inputs: &[&[f32]], _outputs: &mut [&mut [f32]], _midi: &[MidiEvent]) {}
        fn parameter_count(&self) -> usize { 8 }
        fn parameter_info(&self, _index: usize) -> Option<phosphor_plugin::ParameterInfo> { None }
        fn get_parameter(&self, _index: usize) -> f32 { 0.0 }
        fn set_parameter(&mut self, index: usize, value: f32) {
            self.0.lock().unwrap().push((index, value));
        }
        fn reset(&mut self) {}
    }

    /// Add a track carrying a [`ParamLog`], applying the commands immediately.
    fn add_logging_track(mixer: &mut Mixer, tx: &Sender<MixerCommand>, id: usize) -> ParamLog {
        let log = ParamLog::new();
        let handle = Arc::new(TrackHandle::new(id, TrackKind::Instrument));
        tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle }).unwrap();
        tx.send(MixerCommand::SetInstrument {
            track_id: id,
            instrument: Box::new(log.clone()),
        }).unwrap();
        mixer.drain_commands();
        log
    }

    /// The defect: the drain used to be `while let Ok(cmd) = try_recv()`, so
    /// the callback did as much work as the UI had queued. Opening a session
    /// queues hundreds of commands and the callback has a hard deadline.
    #[test]
    fn one_callback_applies_a_bounded_amount_of_work() {
        let (mut mixer, tx, _clip_rx, _transport) = setup_mixer();
        let log = add_logging_track(&mut mixer, &tx, 0);

        for i in 0..500 {
            tx.send(MixerCommand::SetParameter {
                track_id: 0,
                param_index: i % 8,
                value: i as f32,
            }).unwrap();
        }

        let spent = mixer.drain_commands();
        assert!(
            spent <= WORST_CALLBACK,
            "one callback spent {spent} units, over the {WORST_CALLBACK} bound"
        );
        assert_eq!(
            log.seen().len(),
            COMMAND_BUDGET as usize,
            "a parameter costs one unit, so a full budget is exactly that many"
        );
        assert!(!mixer.command_rx.is_empty(), "the rest has to still be queued");
    }

    /// Bounded is only half of it: everything queued still has to arrive, once
    /// each, in the order it was sent.
    #[test]
    fn nothing_is_lost_or_reordered_across_callbacks() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let log = add_logging_track(&mut mixer, &tx, 0);

        let sent: Vec<(usize, f32)> = (0..500).map(|i| (i % 8, i as f32)).collect();
        for &(param_index, value) in &sent {
            tx.send(MixerCommand::SetParameter { track_id: 0, param_index, value }).unwrap();
        }

        // Run callbacks until the queue is empty, counting them: 500 commands
        // at one unit each cannot fit in fewer than eight budgets, which is
        // what makes this a test of the bound and not just of the FIFO.
        let mut output = vec![0.0f32; 128];
        let mut callbacks = 0;
        while !mixer.command_rx.is_empty() {
            mixer.process(&mut output, &[], &transport);
            callbacks += 1;
            assert!(callbacks < 100, "the drain is not making progress");
        }
        assert!(
            callbacks >= 500 / COMMAND_BUDGET as usize,
            "500 commands went through in {callbacks} callbacks, so the budget did not hold"
        );
        assert_eq!(log.seen(), sent, "the audio thread saw a different sequence");
    }

    /// The ordering guarantee, at the one place it matters: a track has to
    /// exist before its instrument is attached. Splitting the queue between
    /// the two would drop the instrument on the floor — `SetInstrument` for a
    /// track that is not there yet is silently discarded — and the track would
    /// play nothing for the rest of the session.
    #[test]
    fn a_track_and_its_instrument_survive_a_budget_boundary() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let log = ParamLog::new();

        // Fill this callback's budget with cheap commands first, so that the
        // pair below is guaranteed to land in a later one.
        for _ in 0..COMMAND_BUDGET {
            tx.send(MixerCommand::SetParameter { track_id: 99, param_index: 0, value: 0.0 })
                .unwrap();
        }
        let handle = Arc::new(TrackHandle::new(7, TrackKind::Instrument));
        tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle }).unwrap();
        tx.send(MixerCommand::SetInstrument {
            track_id: 7,
            instrument: Box::new(log.clone()),
        }).unwrap();
        tx.send(MixerCommand::SetParameter { track_id: 7, param_index: 3, value: 0.5 }).unwrap();

        let mut output = vec![0.0f32; 128];
        mixer.process(&mut output, &[], &transport);
        assert!(mixer.tracks.is_empty(), "the budget did not stop at the parameters");

        while !mixer.command_rx.is_empty() {
            mixer.process(&mut output, &[], &transport);
        }
        assert_eq!(mixer.tracks.len(), 1);
        assert!(mixer.tracks[0].instrument.is_some(), "the instrument never arrived");
        assert_eq!(
            log.seen(),
            vec![(3, 0.5)],
            "the parameter that follows the instrument did not reach it"
        );
    }

    /// An instrument load is not a parameter change: it calls `Plugin::init`,
    /// which allocates a voice array and, on some instruments, a delay line.
    /// A flat count of commands per callback would let sixteen of those
    /// through where it lets sixteen stores through.
    #[test]
    fn an_instrument_load_costs_more_than_a_parameter() {
        let param = MixerCommand::SetParameter { track_id: 0, param_index: 0, value: 0.0 };
        let load = MixerCommand::SetInstrument {
            track_id: 0,
            instrument: Box::new(FixedOutput(0.0)),
        };
        assert!(command_cost(&load) > command_cost(&param));

        // Four loads per callback, not sixty-four.
        let (mut mixer, tx, _clip_rx, _transport) = setup_mixer();
        for id in 0..8 {
            let handle = Arc::new(TrackHandle::new(id, TrackKind::Instrument));
            tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle }).unwrap();
        }
        while !mixer.command_rx.is_empty() {
            mixer.drain_commands();
        }
        for id in 0..8 {
            tx.send(MixerCommand::SetInstrument {
                track_id: id,
                instrument: Box::new(FixedOutput(0.25)),
            }).unwrap();
        }
        mixer.drain_commands();
        let loaded = mixer.tracks.iter().filter(|t| t.instrument.is_some()).count();
        assert_eq!(loaded, (COMMAND_BUDGET / HEAVY_COMMAND) as usize);
    }

    /// `AddTrack` pushes onto the track list, and a push that grows the list
    /// reallocates — on the audio thread. The list is built with room for more
    /// tracks than a session will hold so that it does not.
    #[test]
    fn adding_tracks_does_not_grow_the_track_list() {
        let (mut mixer, tx, _clip_rx, _transport) = setup_mixer();
        let capacity = mixer.tracks.capacity();
        assert!(capacity >= TRACK_CAPACITY);

        for id in 0..TRACK_CAPACITY {
            let handle = Arc::new(TrackHandle::new(id, TrackKind::Instrument));
            tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle }).unwrap();
        }
        while !mixer.command_rx.is_empty() {
            mixer.drain_commands();
        }
        assert_eq!(mixer.tracks.len(), TRACK_CAPACITY);
        assert_eq!(
            mixer.tracks.capacity(), capacity,
            "the track list reallocated on the audio thread"
        );
    }

    // ── Master limiter ──

    /// A plugin that writes whatever it is told to, so the limiter can be
    /// driven with signals no real instrument would produce.
    struct FixedOutput(f32);

    impl Plugin for FixedOutput {
        fn info(&self) -> phosphor_plugin::PluginInfo {
            phosphor_plugin::PluginInfo {
                name: "Fixed".into(),
                version: "0".into(),
                author: "test".into(),
                category: phosphor_plugin::PluginCategory::Instrument,
            }
        }
        fn init(&mut self, _sample_rate: f64, _max_buffer_size: usize) {}
        fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], _midi: &[MidiEvent]) {
            for ch in outputs.iter_mut() {
                ch.fill(self.0);
            }
        }
        fn parameter_count(&self) -> usize { 0 }
        fn parameter_info(&self, _index: usize) -> Option<phosphor_plugin::ParameterInfo> { None }
        fn get_parameter(&self, _index: usize) -> f32 { 0.0 }
        fn set_parameter(&mut self, _index: usize, _value: f32) {}
        fn reset(&mut self) {}
    }

    fn add_fixed_track(tx: &Sender<MixerCommand>, id: usize, value: f32) -> Arc<TrackHandle> {
        let handle = Arc::new(TrackHandle::new(id, TrackKind::Instrument));
        handle.config.set_volume(1.0);
        tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle: handle.clone() }).unwrap();
        tx.send(MixerCommand::SetInstrument {
            track_id: id,
            instrument: Box::new(FixedOutput(value)),
        }).unwrap();
        handle
    }

    /// The guarantee. Six tracks each running at three quarters of full scale
    /// sum to 4.5x — without the limiter that is what would reach the device.
    #[test]
    fn master_limiter_bounds_many_loud_tracks() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        for id in 0..6 {
            add_fixed_track(&tx, id, 0.75);
        }
        transport.play();

        let mut output = vec![0.0f32; 512];
        for _ in 0..8 {
            mixer.process(&mut output, &[], &transport);
            for (i, &s) in output.iter().enumerate() {
                assert!(s.is_finite(), "non-finite sample at {i}");
                assert!(s.abs() <= 1.0, "sample {i} left the mixer at {s}");
            }
        }

        // And it is actually holding the ceiling, not silencing the mix.
        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.8, "limiter over-attenuated, peak={peak}");
    }

    /// A NaN out of a diverging filter must not reach the device: at full
    /// scale it is a noise burst, and it also poisons every sample after it
    /// if it is allowed into the limiter's gain state.
    #[test]
    fn non_finite_track_output_becomes_silence() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        add_fixed_track(&tx, 0, f32::NAN);
        transport.play();

        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &[], &transport);
        assert!(output.iter().all(|s| *s == 0.0), "NaN track should render as silence");

        // ...and the mixer still works afterwards: the gain state was not
        // left as NaN by the sample that was thrown away.
        tx.send(MixerCommand::RemoveTrack { track_id: 0 }).unwrap();
        add_fixed_track(&tx, 1, 0.5);
        mixer.process(&mut output, &[], &transport);
        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!((peak - 0.5).abs() < 1.0e-6, "mixer did not recover, peak={peak}");
    }

    #[test]
    fn infinite_track_output_becomes_silence() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        add_fixed_track(&tx, 0, f32::INFINITY);
        transport.play();

        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &[], &transport);
        assert!(output.iter().all(|s| *s == 0.0), "infinite track should render as silence");
    }

    /// Below the ceiling the limiter is not a processor, it is a wire. Any
    /// deviation here would be gain riding on material that never asked for
    /// it — which is exactly what makes a limiter audible.
    #[test]
    fn limiter_is_bit_identical_below_the_ceiling() {
        let mut limiter = MasterLimiter::new(44_100);

        // A sweep of levels up to the ceiling, plus signs and denormals.
        let mut input: Vec<f32> = Vec::new();
        for i in 0..20_000u32 {
            let phase = i as f32 * 0.01;
            let amp = LIMITER_CEILING * (i as f32 / 20_000.0);
            input.push(phase.sin() * amp);
            input.push(phase.cos() * amp);
        }
        input.push(LIMITER_CEILING);
        input.push(-LIMITER_CEILING);
        input.push(0.0);
        input.push(-0.0);
        input.push(f32::MIN_POSITIVE);
        input.push(-f32::MIN_POSITIVE);

        let mut output = input.clone();
        limiter.process(&mut output);

        for (i, (a, b)) in input.iter().zip(output.iter()).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "limiter altered sample {i}: {a} -> {b}");
        }
    }

    /// The ceiling holds for anything, including levels no instrument in the
    /// project can produce.
    #[test]
    fn limiter_holds_the_ceiling_under_abuse() {
        let mut limiter = MasterLimiter::new(44_100);
        for amplitude in [1.0f32, 2.0, 10.0, 1.0e3, 1.0e6, 1.0e30] {
            let mut buf: Vec<f32> = (0..4_096)
                .map(|i| (i as f32 * 0.05).sin() * amplitude)
                .collect();
            limiter.process(&mut buf);
            for (i, &s) in buf.iter().enumerate() {
                assert!(s.is_finite(), "amplitude {amplitude}: sample {i} is {s}");
                assert!(
                    s.abs() <= LIMITER_CEILING,
                    "amplitude {amplitude}: sample {i} reached {s}, above the ceiling"
                );
            }
        }
    }

    /// A step from silence to well over the ceiling: the very first sample of
    /// the step must already be limited. Anything else means overshoot, and
    /// the only thing left to catch overshoot is a hard clip.
    #[test]
    fn limiter_attack_has_no_overshoot() {
        let mut limiter = MasterLimiter::new(44_100);
        let mut buf = vec![0.0f32; 64];
        limiter.process(&mut buf);
        let mut step = vec![4.0f32; 64];
        limiter.process(&mut step);
        assert!(
            step[0].abs() <= LIMITER_CEILING,
            "first sample of the step overshot to {}",
            step[0]
        );
    }

    /// Gain reduction must come back smoothly, not step. A step would be a
    /// click; a release faster than a low note's period would distort it.
    #[test]
    fn limiter_release_is_gradual() {
        let mut limiter = MasterLimiter::new(44_100);
        let mut loud = vec![4.0f32; 64];
        limiter.process(&mut loud);
        let reduced = limiter.gain;
        assert!(reduced < 0.5, "limiter did not engage, gain={reduced}");

        // 10 ms of quiet material (441 stereo frames): partly recovered, not
        // all the way.
        let mut quiet = vec![0.1f32; 441 * 2];
        limiter.process(&mut quiet);
        assert!(limiter.gain > reduced, "gain did not recover at all");
        assert!(
            limiter.gain < 1.0,
            "gain snapped back to unity within 10 ms, which is a click"
        );

        // 500 ms is ten time constants: fully recovered.
        let mut long = vec![0.1f32; 22_050 * 2];
        limiter.process(&mut long);
        assert!(
            (limiter.gain - 1.0).abs() < 1.0e-4,
            "gain never returned to unity: {}",
            limiter.gain
        );
    }

    /// Stereo-linked: one gain from `max(|L|, |R|)`, so a peak on one side
    /// does not pull the image across to the other.
    #[test]
    fn limiter_does_not_shift_the_stereo_image() {
        let mut limiter = MasterLimiter::new(44_100);
        // Left twice the level of right, both well over the ceiling.
        let mut buf: Vec<f32> = Vec::new();
        for i in 0..1_024 {
            let phase = i as f32 * 0.05;
            buf.push(phase.sin() * 3.0);
            buf.push(phase.sin() * 1.5);
        }
        limiter.process(&mut buf);
        for frame in buf.chunks_exact(2) {
            if frame[1].abs() > 1.0e-4 {
                let ratio = frame[0] / frame[1];
                assert!(
                    (ratio - 2.0).abs() < 1.0e-3,
                    "channel balance moved: L/R = {ratio}"
                );
            }
        }
    }

    /// The loudest single voice in the project: ROM3A's TIMPANI, voice 147 of
    /// the DX7's 256 factory voices, which is what `phosphor-dsp`'s headroom
    /// sweep measures as the hottest thing any instrument here can produce.
    ///
    /// The DX7 has two selectors — a cartridge and a voice — so picking one by
    /// number goes through `voice_knobs`.
    fn loudest_dx7_voice() -> phosphor_dsp::dx7::Dx7Synth {
        use phosphor_dsp::dx7;
        let mut synth = dx7::Dx7Synth::new();
        let (bank, patch) = dx7::voice_knobs(147);
        synth.set_parameter(dx7::P_BANK, bank);
        synth.set_parameter(dx7::P_PATCH, patch);
        debug_assert_eq!(dx7::voice_name(147), "TIMPANI");
        synth
    }

    /// Four tracks of the loudest DX7 voice, each playing a two-handed
    /// eight-note chord at full velocity with the fader open — a heavier mix
    /// than anything the application can produce by accident.
    #[test]
    fn master_limiter_bounds_four_loud_instrument_tracks() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        for id in 0..4 {
            let handle = Arc::new(TrackHandle::new(id, TrackKind::Instrument));
            handle.config.midi_active.store(true, std::sync::atomic::Ordering::Relaxed);
            handle.config.set_volume(1.0);
            let synth = loudest_dx7_voice();
            tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle }).unwrap();
            tx.send(MixerCommand::SetInstrument {
                track_id: id,
                instrument: Box::new(synth),
            }).unwrap();
        }
        transport.play();

        let chord: Vec<MidiMessage> = [36u8, 43, 48, 55, 60, 64, 67, 72]
            .iter()
            .map(|&note| make_note_on(note, 127))
            .collect();

        let mut output = vec![0.0f32; 512];
        let mut peak = 0.0f32;
        for block in 0..200 {
            output.fill(0.0);
            if block == 0 {
                mixer.process(&mut output, &chord, &transport);
            } else {
                mixer.process(&mut output, &[], &transport);
            }
            for (i, &s) in output.iter().enumerate() {
                assert!(s.is_finite(), "block {block} sample {i} is {s}");
                assert!(s.abs() <= 1.0, "block {block} sample {i} left the mixer at {s}");
                peak = peak.max(s.abs());
            }
        }
        assert!(peak > 0.5, "four loud tracks should be loud, peak={peak}");
    }

    /// The limiter must be inaudible in ordinary playing, which means it must
    /// not engage at all. The worst single track the application can produce
    /// is the loudest preset in the bank, an eight-note chord at velocity 127,
    /// with the fader all the way open — and that still has to leave the gain
    /// at exactly unity, so the mix is the track sum sample for sample.
    #[test]
    fn limiter_idle_for_the_worst_single_track() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
        handle.config.midi_active.store(true, std::sync::atomic::Ordering::Relaxed);
        handle.config.set_volume(1.0);
        let synth = loudest_dx7_voice();
        tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle }).unwrap();
        tx.send(MixerCommand::SetInstrument { track_id: 0, instrument: Box::new(synth) }).unwrap();
        transport.play();

        let chord: Vec<MidiMessage> = [36u8, 43, 48, 55, 60, 64, 67, 72]
            .iter()
            .map(|&note| make_note_on(note, 127))
            .collect();

        let mut output = vec![0.0f32; 512];
        let mut peak = 0.0f32;
        for block in 0..200 {
            output.fill(0.0);
            if block == 0 {
                mixer.process(&mut output, &chord, &transport);
            } else {
                mixer.process(&mut output, &[], &transport);
            }
            peak = peak.max(output.iter().map(|s| s.abs()).fold(0.0f32, f32::max));
            assert_eq!(
                mixer.limiter.gain, 1.0,
                "limiter engaged at block {block}, peak {peak}"
            );
        }
        assert!(peak > 0.3, "expected a loud chord, peak={peak}");
    }

    // ── Fader ──

    /// Render the loudest thing one track in this project can produce, with
    /// the fader at `volume`. Returns the output peak and the lowest gain the
    /// limiter reached.
    fn worst_track_through_the_mixer(volume: f32) -> (f32, f32) {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
        handle.config.midi_active.store(true, std::sync::atomic::Ordering::Relaxed);
        handle.config.set_volume(volume);
        let synth = loudest_dx7_voice();
        tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle }).unwrap();
        tx.send(MixerCommand::SetInstrument { track_id: 0, instrument: Box::new(synth) }).unwrap();
        transport.play();

        let chord: Vec<MidiMessage> = [36u8, 43, 48, 55, 60, 64, 67, 72]
            .iter()
            .map(|&note| make_note_on(note, 127))
            .collect();

        let mut output = vec![0.0f32; 512];
        let mut peak = 0.0f32;
        let mut min_gain = 1.0f32;
        for block in 0..200 {
            output.fill(0.0);
            if block == 0 {
                mixer.process(&mut output, &chord, &transport);
            } else {
                mixer.process(&mut output, &[], &transport);
            }
            for &s in output.iter() {
                assert!(s.is_finite(), "block {block}: non-finite sample");
                assert!(s.abs() <= 1.0, "block {block}: sample left the mixer at {s}");
                peak = peak.max(s.abs());
            }
            min_gain = min_gain.min(mixer.limiter.gain);
        }
        (peak, min_gain)
    }

    /// Anywhere from the bottom of the fader up to unity, the limiter is not
    /// in the signal path at all — not "barely", not at all — even for the
    /// loudest patch in the project played as hard as the format allows.
    ///
    /// This is what the instrument trims buy. Gain reduction on the master
    /// bus is then always a mix decision (several loud tracks at once) rather
    /// than something one instrument can cause on its own.
    #[test]
    fn fader_below_unity_never_engages_the_limiter() {
        for volume in [
            0.25,
            TrackConfig::DEFAULT_VOLUME,
            TrackConfig::UNITY_VOLUME,
        ] {
            let (peak, min_gain) = worst_track_through_the_mixer(volume);
            assert_eq!(
                min_gain, 1.0,
                "limiter reduced by {:.2} dB at fader {volume} (peak {peak:.4})",
                20.0 * min_gain.log10()
            );
        }
    }

    /// Above unity the fader is makeup gain the user asked for, and the
    /// limiter is what makes asking for it safe. Two things have to hold:
    /// the output stays bounded, and turning the fader up never makes the
    /// track quieter than leaving it at unity — a limiter that over-ducks
    /// would turn the top of the fader into a trap.
    #[test]
    fn fader_makeup_gain_is_bounded_not_wasted() {
        let (unity_peak, _) = worst_track_through_the_mixer(TrackConfig::UNITY_VOLUME);
        let (max_peak, min_gain) = worst_track_through_the_mixer(TrackConfig::MAX_VOLUME);

        assert!(
            max_peak <= LIMITER_CEILING,
            "fader at maximum let {max_peak:.4} through, above the ceiling"
        );
        assert!(
            max_peak >= unity_peak,
            "turning the fader up made the track quieter: {unity_peak:.4} -> {max_peak:.4}"
        );
        // The limiter took back some of the boost, but not more than the
        // fader added — otherwise it is attenuating, not limiting.
        let reduction_db = -20.0 * min_gain.log10();
        let boost_db = 20.0 * (TrackConfig::MAX_VOLUME / TrackConfig::UNITY_VOLUME).log10();
        assert!(
            reduction_db <= boost_db,
            "limiter took {reduction_db:.2} dB off a {boost_db:.2} dB boost"
        );
    }

    // ── Metronome balance ──

    /// The click has no fader and is not mixed through a track, so nothing
    /// downstream can compensate for it being wrong: it only sits right
    /// relative to the music if `CLICK_VOLUME` tracks the instruments'
    /// headroom trims. That coupling is invisible from either file and has
    /// already drifted once, when the trims moved and the click did not.
    ///
    /// So: a click against the level a user hears while playing — the default
    /// preset, a triad at velocity 100, fader at its default. Loud enough to
    /// play to, not so loud it is the loudest thing in the mix.
    #[test]
    fn metronome_click_sits_with_the_music() {
        use phosphor_dsp::dx7;

        fn render(with_track: bool, metronome: bool) -> f32 {
            let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
            let chord: Vec<MidiMessage> = if with_track {
                let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
                handle.config.midi_active.store(true, std::sync::atomic::Ordering::Relaxed);
                tx.send(MixerCommand::AddTrack {
                    kind: TrackKind::Instrument,
                    handle,
                })
                .unwrap();
                tx.send(MixerCommand::SetInstrument {
                    track_id: 0,
                    instrument: Box::new(dx7::Dx7Synth::new()),
                })
                .unwrap();
                [60u8, 64, 67].iter().map(|&n| make_note_on(n, 100)).collect()
            } else {
                Vec::new()
            };
            if metronome {
                transport.toggle_metronome();
            }
            transport.play();

            let mut output = vec![0.0f32; 512];
            let mut peak = 0.0f32;
            for block in 0..200 {
                output.fill(0.0);
                if block == 0 {
                    mixer.process(&mut output, &chord, &transport);
                } else {
                    mixer.process(&mut output, &[], &transport);
                }
                peak = peak.max(output.iter().map(|s| s.abs()).fold(0.0f32, f32::max));
                transport.advance(256, 44_100);
            }
            peak
        }

        let music = render(true, false);
        let click = render(false, true);
        assert!(music > 0.0 && click > 0.0, "music {music}, click {click}");

        let relative_db = 20.0 * (click / music).log10();
        assert!(
            (-12.0..=0.0).contains(&relative_db),
            "the click is {relative_db:.1} dB against a triad (click {click:.4}, \
             music {music:.4}); it has to be audible over the music without \
             being the loudest thing in the mix"
        );
    }

    /// The fader reaches the audio thread. Not a tautology: `volume` is read
    /// per buffer through the atomic, so this catches a mix path that caches
    /// it or ignores it.
    #[test]
    fn fader_scales_the_track() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let handle = add_fixed_track(&tx, 0, 0.25);
        transport.play();

        let mut output = vec![0.0f32; 512];
        for (volume, expected) in [(0.0f32, 0.0f32), (0.5, 0.125), (1.0, 0.25), (2.0, 0.5)] {
            handle.config.set_volume(volume);
            output.fill(0.0);
            mixer.process(&mut output, &[], &transport);
            let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!(
                (peak - expected).abs() < 1.0e-6,
                "fader at {volume} gave {peak}, expected {expected}"
            );
        }
    }

    // ── The device decides the rate ──

    /// A device that would not give us the rate we asked for.
    fn refused(asked: u32, sample_rate: u32, max_buffer_frames: u32) -> StreamFormat {
        StreamFormat {
            sample_rate,
            buffer_size: Some(64),
            max_buffer_frames,
            channels: 2,
            sample_rate_request: Requested::Refused(asked),
            buffer_size_request: Requested::Granted,
        }
    }

    /// The defect: the mixer was built from the command-line sample rate while
    /// the stream ran at the device's. Everything the mixer derives from the
    /// rate — oscillator increments, envelope times, the tick advance — was
    /// then wrong by the ratio between the two.
    #[test]
    fn the_mixer_runs_at_the_rate_the_device_granted() {
        let requested = crate::EngineConfig { buffer_size: 64, sample_rate: 44100 };
        let format = refused(44100, 48000, 4096);
        let effective = crate::EngineConfig::from(format);

        let (_tx, rx) = mixer_command_channel();
        let (clip_tx, _clip_rx) = clip_snapshot_channel();
        let mixer = Mixer::new(
            rx,
            Arc::new(VuLevels::new()),
            clip_tx,
            effective.sample_rate,
            format.max_buffer_frames as usize,
        );

        assert_eq!(mixer.sample_rate, 48000, "mixer must adopt the device's rate");
        assert_ne!(
            mixer.sample_rate, requested.sample_rate,
            "the request was 44100 and the device said 48000; taking the \
             request here is the 8.84%-sharp bug"
        );
        assert_eq!(mixer.max_buffer_size, 4096);
    }

    /// A device that offers exactly what was asked for changes nothing.
    #[test]
    fn a_device_that_agrees_leaves_the_request_alone() {
        let requested = crate::EngineConfig { buffer_size: 64, sample_rate: 44100 };
        let format = StreamFormat {
            sample_rate: 44100,
            buffer_size: Some(64),
            max_buffer_frames: 4096,
            channels: 2,
            sample_rate_request: Requested::Granted,
            buffer_size_request: Requested::Granted,
        };
        assert_eq!(crate::EngineConfig::from(format), requested);
    }

    /// The default path, and the one that has to be right for the most
    /// people: nothing asked for, so the mixer is built at whatever the
    /// device was already set to.
    #[test]
    fn asking_for_nothing_builds_the_mixer_at_the_devices_rate() {
        let format = StreamFormat {
            sample_rate: 48000,
            buffer_size: None,
            max_buffer_frames: 4096,
            channels: 2,
            sample_rate_request: Requested::Unasked,
            buffer_size_request: Requested::Unasked,
        };
        let effective = crate::EngineConfig::from(format);

        let (_tx, rx) = mixer_command_channel();
        let (clip_tx, _clip_rx) = clip_snapshot_channel();
        let mixer = Mixer::new(
            rx,
            Arc::new(VuLevels::new()),
            clip_tx,
            effective.sample_rate,
            format.max_buffer_frames as usize,
        );
        assert_eq!(mixer.sample_rate, 48000);
        assert_eq!(mixer.max_buffer_size, 4096);
        assert!(format.divergence_notice().is_none(), "following the device is not news");
    }

    /// The defect: buffers were sized from the requested block, the device
    /// handed the callback a larger one, and `process` grew them — a heap
    /// allocation on the audio thread, on the very first callback.
    #[test]
    fn the_largest_block_the_device_promised_never_grows_a_buffer() {
        let max_frames = 512usize;
        let (tx, rx) = mixer_command_channel();
        let (clip_tx, _clip_rx) = clip_snapshot_channel();
        let mut mixer = Mixer::new(
            rx,
            Arc::new(VuLevels::new()),
            clip_tx,
            48000,
            max_frames,
        );
        let transport = Arc::new(Transport::new(120.0));
        let _handle = add_armed_synth(&tx, 0);
        mixer.drain_commands();

        // Snapshot after the track exists: adding one is a UI-driven
        // allocation, not a per-callback one.
        let before = (
            mixer.scratch_l.capacity(),
            mixer.scratch_r.capacity(),
            mixer.tracks[0].buf_l.capacity(),
            mixer.tracks[0].buf_r.capacity(),
        );

        transport.play();
        let mut output = vec![0.0f32; max_frames * 2];
        mixer.process(&mut output, &[make_note_on(60, 100)], &transport);

        let after = (
            mixer.scratch_l.capacity(),
            mixer.scratch_r.capacity(),
            mixer.tracks[0].buf_l.capacity(),
            mixer.tracks[0].buf_r.capacity(),
        );
        assert_eq!(
            before, after,
            "a block the size the device promised must fit the buffers as \
             allocated; growing one means the audio thread called the allocator"
        );
    }

    /// The invariant stated everywhere in this crate, held to by the
    /// allocator rather than by reading the code: a steady-state callback
    /// touches no heap.
    #[test]
    fn a_steady_state_callback_does_not_allocate() {
        let max_frames = 512usize;
        let (tx, rx) = mixer_command_channel();
        let (clip_tx, _clip_rx) = clip_snapshot_channel();
        let mut mixer = Mixer::new(rx, Arc::new(VuLevels::new()), clip_tx, 48000, max_frames);
        let transport = Arc::new(Transport::new(120.0));
        let _handle = add_armed_synth(&tx, 0);
        mixer.drain_commands();
        transport.play();

        let mut output = vec![0.0f32; max_frames * 2];
        // One warm-up block: anything lazily built on first use — the
        // wavetable bank behind its `OnceLock`, for one — is built here,
        // outside the region under test.
        mixer.process(&mut output, &[make_note_on(60, 100)], &transport);

        let allocations = crate::alloc_count::allocations_during(|| {
            for _ in 0..8 {
                mixer.process(&mut output, &[], &transport);
            }
        });
        assert_eq!(allocations, 0, "Mixer::process reached the allocator");
    }

    // ── The step sequencer ──

    use crate::pattern::{ChainEntry, Lane, PatternEvent, Rate, Step};

    /// A mixer at a given rate, with nothing on it.
    fn bare_mixer(
        sample_rate: u32,
        max_frames: usize,
    ) -> (Mixer, Sender<MixerCommand>, Arc<Transport>) {
        let (tx, rx) = mixer_command_channel();
        let (clip_tx, _clip_rx) = clip_snapshot_channel();
        let mixer = Mixer::new(rx, Arc::new(VuLevels::new()), clip_tx, sample_rate, max_frames);
        (mixer, tx, Arc::new(Transport::new(120.0)))
    }

    /// A pattern with one drum lane on the steps named.
    fn kick_pattern(on: &[usize]) -> PatternBlock {
        let mut block = PatternBlock::empty();
        block.playing = true;
        block.lanes[0] = Lane::drum(36);
        for &index in on {
            block.lanes[0].steps[index].on = true;
        }
        block
    }

    fn add_track(tx: &Sender<MixerCommand>, id: usize) -> Arc<TrackHandle> {
        let handle = Arc::new(TrackHandle::new(id, TrackKind::Instrument));
        tx.send(MixerCommand::AddTrack {
            kind: TrackKind::Instrument,
            handle: handle.clone(),
        })
        .unwrap();
        handle
    }

    fn apply_all(mixer: &mut Mixer) {
        while !mixer.command_rx.is_empty() {
            mixer.drain_commands();
        }
    }

    fn note_ons(track: &AudioTrack) -> impl Iterator<Item = &MidiEvent> {
        track.plugin_events.iter().filter(|e| e.status == 0x90 && e.data2 > 0)
    }

    /// **The sync guarantee.** A pattern step and a clip note on the same
    /// beat have to reach the instrument at the same sample, in the same
    /// callback — at every block size and every sample rate, because those
    /// are what a wrong answer would be a function of.
    ///
    /// It holds by construction rather than by agreement: both go through
    /// one `PlaybackWindow`. This is the test that would catch that ceasing
    /// to be true.
    #[test]
    fn a_pattern_step_and_a_clip_note_land_on_the_same_sample() {
        for sample_rate in [44_100u32, 48_000, 96_000] {
            for frames in [64usize, 256, 470] {
                let (mut mixer, tx, transport) = bare_mixer(sample_rate, 512);

                // Track 0: a clip with one note on beat two.
                let _clip_track = add_track(&tx, 0);
                tx.send(MixerCommand::CreateClip {
                    track_id: 0,
                    start_tick: 0,
                    length_ticks: 3840,
                })
                .unwrap();
                tx.send(MixerCommand::UpdateClip {
                    track_id: 0,
                    clip_index: 0,
                    events: vec![ClipEvent { tick: 960, status: 0x90, data1: 60, data2: 100 }],
                })
                .unwrap();

                // Track 1: a pattern whose fourth sixteenth is beat two.
                let _seq_track = add_track(&tx, 1);
                tx.send(MixerCommand::SetPattern {
                    track_id: 1,
                    slot: 0,
                    block: kick_pattern(&[4]),
                })
                .unwrap();
                apply_all(&mut mixer);

                transport.play();
                let mut output = vec![0.0f32; frames * 2];
                let mut landed = None;
                while transport.position_ticks() < 1_200 {
                    mixer.process(&mut output, &[], &transport);
                    let clip_note = note_ons(&mixer.tracks[0]).find(|e| e.data1 == 60);
                    let step_note = note_ons(&mixer.tracks[1]).find(|e| e.data1 == 36);
                    match (clip_note, step_note) {
                        (Some(c), Some(s)) => {
                            landed = Some((c.sample_offset, s.sample_offset));
                            break;
                        }
                        (None, None) => {}
                        (clip, step) => panic!(
                            "at {sample_rate} Hz / {frames} frames only one of them fired: \
                             clip={clip:?} step={step:?}"
                        ),
                    }
                    transport.advance(frames as u32, sample_rate);
                }
                let (clip_at, step_at) =
                    landed.unwrap_or_else(|| panic!("nothing fired at {sample_rate}/{frames}"));
                assert_eq!(
                    clip_at, step_at,
                    "at {sample_rate} Hz / {frames} frames the clip note landed on sample \
                     {clip_at} and the step on {step_at}"
                );
            }
        }
    }

    /// A pattern is timed in ticks, so the same pattern has to occupy the
    /// same wall-clock time at every sample rate the application supports.
    #[test]
    fn step_timing_is_the_same_at_every_sample_rate() {
        let frames = 256usize;
        for sample_rate in [44_100u32, 48_000, 96_000] {
            let (mut mixer, tx, transport) = bare_mixer(sample_rate, 512);
            let _track = add_track(&tx, 0);
            tx.send(MixerCommand::SetPattern {
                track_id: 0,
                slot: 0,
                block: kick_pattern(&[0, 4, 8, 12]),
            })
            .unwrap();
            apply_all(&mut mixer);

            transport.play();
            let mut output = vec![0.0f32; frames * 2];
            let mut seconds = Vec::new();
            let mut block = 0usize;
            while seconds.len() < 4 && transport.position_ticks() < 3_600 {
                mixer.process(&mut output, &[], &transport);
                for event in note_ons(&mixer.tracks[0]) {
                    let sample = block * frames + event.sample_offset as usize;
                    seconds.push(sample as f64 / f64::from(sample_rate));
                }
                transport.advance(frames as u32, sample_rate);
                block += 1;
            }

            // Four steps a beat apart at 120 BPM: half a second each.
            assert_eq!(seconds.len(), 4, "at {sample_rate} Hz");
            for (index, at) in seconds.iter().enumerate() {
                let expected = index as f64 * 0.5;
                assert!(
                    (at - expected).abs() < 0.002,
                    "at {sample_rate} Hz step {index} landed at {at:.4}s, expected {expected:.4}s"
                );
            }
        }
    }

    /// The wrap, which is where a sequencer written around a free-running
    /// cursor loses or repeats a step. Sixteen onsets per time round, every
    /// time round: the window stops at the loop point so nothing on the far
    /// side of it plays early, and the step is derived from the position so
    /// nothing is skipped when it comes back.
    #[test]
    fn a_loop_wrap_neither_drops_nor_doubles_the_first_step() {
        let frames = 256usize;
        let (mut mixer, tx, transport) = bare_mixer(44_100, 512);
        let _track = add_track(&tx, 0);
        let all_sixteen: Vec<usize> = (0..16).collect();
        tx.send(MixerCommand::SetPattern {
            track_id: 0,
            slot: 0,
            block: kick_pattern(&all_sixteen),
        })
        .unwrap();
        apply_all(&mut mixer);

        transport.set_loop_bars(1, 1);
        transport.toggle_loop();
        transport.play();

        let mut output = vec![0.0f32; frames * 2];
        let mut fired = 0usize;
        let mut wraps = 0usize;
        let mut last = transport.position_ticks();
        for _ in 0..4_000 {
            mixer.process(&mut output, &[], &transport);
            fired += note_ons(&mixer.tracks[0]).count();
            transport.advance(frames as u32, 44_100);
            let now = transport.position_ticks();
            if now < last {
                wraps += 1;
                if wraps == 4 {
                    break;
                }
            }
            last = now;
        }
        assert_eq!(wraps, 4, "the transport did not loop");
        assert_eq!(fired, 64, "four times round a 16-step pattern is 64 onsets");
    }

    /// A sequencer track makes no sound of its own: it drives the instrument
    /// in the track's plugin slot, which is an ordinary instrument in an
    /// ordinary slot. Nothing in the audio path knows a sequencer exists.
    #[test]
    fn a_sequencer_track_plays_its_child_instrument() {
        let (mut mixer, tx, transport) = bare_mixer(44_100, 512);
        let handle = add_track(&tx, 0);
        handle.config.set_volume(1.0);
        tx.send(MixerCommand::SetInstrument {
            track_id: 0,
            instrument: Box::new(PhosphorSynth::new()),
        })
        .unwrap();
        let mut block = PatternBlock::empty();
        block.playing = true;
        block.lanes[0].steps[0].on = true;
        block.lanes[0].steps[0].gate = 200;
        tx.send(MixerCommand::SetPattern { track_id: 0, slot: 0, block }).unwrap();
        apply_all(&mut mixer);

        transport.play();
        let mut output = vec![0.0f32; 512 * 2];
        let mut peak = 0.0f32;
        for _ in 0..8 {
            mixer.process(&mut output, &[], &transport);
            peak = peak.max(output.iter().map(|s| s.abs()).fold(0.0, f32::max));
            transport.advance(512, 44_100);
        }
        assert!(peak > 0.001, "the child instrument never sounded, peak={peak}");
    }

    /// Stopping the transport ends every note the sequencer is holding. A
    /// tied step has no note-off of its own, so without this it is a voice
    /// that sounds until the next panic.
    #[test]
    fn stopping_the_transport_ends_every_pattern_note() {
        let (mut mixer, tx, transport) = bare_mixer(44_100, 512);
        let _track = add_track(&tx, 0);
        let mut block = kick_pattern(&[0]);
        block.lanes[0].steps[0].gate = Step::TIE;
        tx.send(MixerCommand::SetPattern { track_id: 0, slot: 0, block }).unwrap();
        apply_all(&mut mixer);

        transport.play();
        let mut output = vec![0.0f32; 256 * 2];
        mixer.process(&mut output, &[], &transport);
        assert_eq!(note_ons(&mixer.tracks[0]).count(), 1);
        transport.advance(256, 44_100);

        transport.pause();
        mixer.process(&mut output, &[], &transport);
        let offs: Vec<u8> = mixer.tracks[0]
            .plugin_events
            .iter()
            .filter(|e| e.status == 0x80)
            .map(|e| e.data1)
            .collect();
        assert_eq!(offs, vec![36], "the tied note was left sounding");

        // ...and only once.
        mixer.process(&mut output, &[], &transport);
        assert!(mixer.tracks[0].plugin_events.is_empty());
    }

    /// A panic drops the table rather than sounding it: the instruments are
    /// being reset underneath, so the offs would be addressed to voices that
    /// no longer exist.
    #[test]
    fn a_panic_leaves_the_sequencer_holding_nothing() {
        let (mut mixer, tx, transport) = bare_mixer(44_100, 512);
        let _track = add_track(&tx, 0);
        let mut block = kick_pattern(&[0]);
        block.lanes[0].steps[0].gate = Step::TIE;
        tx.send(MixerCommand::SetPattern { track_id: 0, slot: 0, block }).unwrap();
        apply_all(&mut mixer);

        transport.play();
        let mut output = vec![0.0f32; 256 * 2];
        mixer.process(&mut output, &[], &transport);
        assert!(mixer.tracks[0].pattern.as_ref().unwrap().held_notes() > 0);

        mixer.reset_all();
        assert_eq!(mixer.tracks[0].pattern.as_ref().unwrap().held_notes(), 0);
    }

    /// The bounce, end to end and through a real instrument: one cycle of a
    /// swung pattern compiled to a clip, played back as a clip, has to be the
    /// same audio the sequencer produced live. Sample for sample — the two
    /// paths share a generator, so anything less is a defect rather than a
    /// tolerance.
    ///
    /// Every gate closes inside the cycle. A bounce is one time through, so a
    /// note that outlives the cycle has nowhere to go and the two renders
    /// would legitimately differ at the tail.
    #[test]
    fn a_bounced_pattern_renders_identically_to_the_live_one() {
        const SWING: u8 = 62;
        let mut block = PatternBlock::empty();
        block.playing = true;
        block.swing = SWING;
        block.rate = Rate::Sixteenth;
        for (index, (key, chord, gate)) in [
            (0usize, 0u8, 5u8, 50u8),
            (3, 3, 6, 90),
            (5, 7, 1, 25),
            (9, 5, 14, 75),
            (11, 10, 12, 40),
            (14, 0, 15, 60),
        ]
        .iter()
        .map(|(i, k, c, g)| (*i, (*k, *c, *g)))
        {
            let step = &mut block.lanes[0].steps[index];
            step.on = true;
            step.key = key;
            step.chord = chord;
            step.gate = gate;
            step.accent = index % 2 == 1;
        }

        let cycle = block.length_ticks();
        let blocks = 24; // 24 x 512 frames at 44.1 kHz covers a bar and a bit

        // Live: the sequencer driving the synth.
        let live = {
            let (mut mixer, tx, transport) = bare_mixer(44_100, 512);
            let handle = add_track(&tx, 0);
            handle.config.set_volume(1.0);
            tx.send(MixerCommand::SetInstrument {
                track_id: 0,
                instrument: Box::new(PhosphorSynth::new()),
            })
            .unwrap();
            tx.send(MixerCommand::SetPattern { track_id: 0, slot: 0, block }).unwrap();
            apply_all(&mut mixer);
            transport.play();

            let mut rendered = Vec::new();
            let mut output = vec![0.0f32; 512 * 2];
            for _ in 0..blocks {
                mixer.process(&mut output, &[], &transport);
                rendered.extend_from_slice(&output);
                transport.advance(512, 44_100);
            }
            rendered
        };

        // Bounced: the same cycle compiled to a clip, played as a clip.
        let bounced = {
            let mut events = Vec::new();
            crate::pattern::compile_cycle(&block, 0, &mut events);
            assert!(!events.is_empty());
            let clip_events: Vec<ClipEvent> = events
                .iter()
                .map(|e: &PatternEvent| ClipEvent {
                    tick: e.tick,
                    status: e.status,
                    data1: e.data1,
                    data2: e.data2,
                })
                .collect();

            let (mut mixer, tx, transport) = bare_mixer(44_100, 512);
            let handle = add_track(&tx, 0);
            handle.config.set_volume(1.0);
            tx.send(MixerCommand::SetInstrument {
                track_id: 0,
                instrument: Box::new(PhosphorSynth::new()),
            })
            .unwrap();
            tx.send(MixerCommand::CreateClip {
                track_id: 0,
                start_tick: 0,
                length_ticks: cycle,
            })
            .unwrap();
            tx.send(MixerCommand::UpdateClip {
                track_id: 0,
                clip_index: 0,
                events: clip_events,
            })
            .unwrap();
            apply_all(&mut mixer);
            transport.play();

            let mut rendered = Vec::new();
            let mut output = vec![0.0f32; 512 * 2];
            for _ in 0..blocks {
                mixer.process(&mut output, &[], &transport);
                rendered.extend_from_slice(&output);
                transport.advance(512, 44_100);
            }
            rendered
        };

        assert_eq!(live.len(), bounced.len());
        let peak = live.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001, "the live render was silent, so this proves nothing");
        for (i, (a, b)) in live.iter().zip(&bounced).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "sample {i} differs: live {a} bounced {b} at {SWING}% swing"
            );
        }
    }

    /// The rule the audio thread lives by, with a sequencer on it: taking a
    /// new pattern while notes are sounding, switching patterns, advancing a
    /// chain, playing chords and turning everything off are all writes into
    /// memory that already exists.
    #[test]
    fn pattern_playback_does_not_allocate() {
        let (mut mixer, tx, transport) = bare_mixer(48_000, 512);
        let _track = add_track(&tx, 0);
        tx.send(MixerCommand::SetInstrument {
            track_id: 0,
            instrument: Box::new(PhosphorSynth::new()),
        })
        .unwrap();

        // Slot 0: chords on a melodic lane. Slot 1: a drum lane.
        let mut chords = PatternBlock::empty();
        chords.playing = true;
        chords.mode = crate::pattern::Mode::Aeolian;
        for index in 0..16 {
            let step = &mut chords.lanes[0].steps[index];
            step.on = true;
            step.chord = 4; // diatonic seventh
            step.voicing = 1 | Step::ROOT_BELOW;
            step.key = (index as u8 * 2) % 12;
        }
        let drums = kick_pattern(&[0, 4, 8, 12]);

        tx.send(MixerCommand::SetPattern { track_id: 0, slot: 1, block: drums }).unwrap();
        tx.send(MixerCommand::SetPattern { track_id: 0, slot: 0, block: chords }).unwrap();
        // A clip on the same track, so the shared window is exercised from
        // both sides while the measurement is running.
        tx.send(MixerCommand::CreateClip { track_id: 0, start_tick: 0, length_ticks: 3840 })
            .unwrap();
        tx.send(MixerCommand::UpdateClip {
            track_id: 0,
            clip_index: 0,
            events: (0..16)
                .flat_map(|i| {
                    [
                        ClipEvent { tick: i * 240, status: 0x90, data1: 40, data2: 90 },
                        ClipEvent { tick: i * 240 + 120, status: 0x80, data1: 40, data2: 0 },
                    ]
                })
                .collect(),
        })
        .unwrap();
        apply_all(&mut mixer);
        transport.play();

        let mut output = vec![0.0f32; 512 * 2];
        // Warm-up: anything built lazily on first use is built here.
        for _ in 0..2 {
            mixer.process(&mut output, &[], &transport);
            transport.advance(512, 48_000);
        }

        let mut queued = chords;
        queued.pending_slot = Some(1);
        let mut chained = chords;
        chained.chain[0] = ChainEntry { slot: 0, repeats: 1 };
        chained.chain[1] = ChainEntry { slot: 1, repeats: 1 };
        chained.chain_len = 2;

        let allocations = crate::alloc_count::allocations_during(|| {
            for block in 0..400 {
                if block == 20 {
                    tx.send(MixerCommand::SetPattern { track_id: 0, slot: 0, block: queued })
                        .unwrap();
                }
                if block == 120 {
                    tx.send(MixerCommand::SetPattern { track_id: 0, slot: 0, block: chained })
                        .unwrap();
                }
                mixer.process(&mut output, &[], &transport);
                transport.advance(512, 48_000);
            }
            transport.pause();
            mixer.process(&mut output, &[], &transport);
        });
        assert_eq!(allocations, 0, "the sequencer reached the allocator");
    }

    /// What a queued command costs to sit in the channel. The block travels
    /// by value so that receiving one cannot reach the allocator, and this is
    /// the price of that: every `MixerCommand`, whichever variant, is now as
    /// wide as the widest one.
    ///
    /// Worth stating out loud rather than discovering later. A full command
    /// budget in flight is 150 kB of queue, which is nothing on the heap and
    /// everything on the audio thread's deadline, and that is the trade.
    #[test]
    fn a_command_is_as_wide_as_a_pattern() {
        assert_eq!(
            std::mem::size_of::<MixerCommand>(),
            crate::pattern::PatternBlock::SIZE + 11
        );
    }

    /// What the UI reads to draw the playhead and the queued-slot countdown.
    /// Atomics on the track handle, the same shape as the VU meters.
    #[test]
    fn the_track_handle_reports_where_the_pattern_is() {
        let (mut mixer, tx, transport) = bare_mixer(44_100, 512);
        let handle = add_track(&tx, 0);
        let all_sixteen: Vec<usize> = (0..16).collect();
        let block = kick_pattern(&all_sixteen);
        tx.send(MixerCommand::SetPattern { track_id: 0, slot: 1, block }).unwrap();
        let mut queued = block;
        queued.pending_slot = Some(1);
        tx.send(MixerCommand::SetPattern { track_id: 0, slot: 0, block: queued }).unwrap();
        apply_all(&mut mixer);

        // One step in, rather than on the downbeat: tick zero is itself a
        // pattern boundary, so a switch queued there is due immediately.
        transport.set_position(240);
        transport.play();
        let mut output = vec![0.0f32; 256 * 2];
        mixer.process(&mut output, &[], &transport);
        assert_eq!(handle.pattern.live_slot(), 0);
        assert_eq!(handle.pattern.queued_slot(), Some(1));
        assert_eq!(handle.pattern.step(), 1);
        assert!(handle.pattern.is_running());

        // Half a bar in: step 8, and the switch has not happened yet.
        transport.set_position(1920);
        mixer.process(&mut output, &[], &transport);
        assert_eq!(handle.pattern.step(), 8);
        assert_eq!(handle.pattern.live_slot(), 0);

        // Past the pattern end: the queued slot took over.
        transport.set_position(3840);
        mixer.process(&mut output, &[], &transport);
        assert_eq!(handle.pattern.live_slot(), 1);
        assert_eq!(handle.pattern.queued_slot(), None);
    }

    /// The same, for the shorter blocks the device may hand us when the
    /// buffers were sized for its maximum.
    #[test]
    fn a_short_callback_does_not_allocate_either() {
        let max_frames = 512usize;
        let (tx, rx) = mixer_command_channel();
        let (clip_tx, _clip_rx) = clip_snapshot_channel();
        let mut mixer = Mixer::new(rx, Arc::new(VuLevels::new()), clip_tx, 48000, max_frames);
        let transport = Arc::new(Transport::new(120.0));
        let _handle = add_armed_synth(&tx, 0);
        mixer.drain_commands();
        transport.play();

        let mut output = vec![0.0f32; 64 * 2];
        mixer.process(&mut output, &[make_note_on(60, 100)], &transport);

        let allocations = crate::alloc_count::allocations_during(|| {
            for _ in 0..8 {
                mixer.process(&mut output, &[], &transport);
            }
        });
        assert_eq!(allocations, 0, "Mixer::process reached the allocator");
    }

    // ── The insert layer ──

    use crate::fx::{db_to_gain, FxParamInfo, Gain, MAX_FX_SLOTS};

    /// Attach a handle to one of the bus strips, the way the front end does
    /// at start-up: an `AddTrack` carrying a bus kind.
    fn attach_bus(tx: &Sender<MixerCommand>, kind: TrackKind) -> Arc<TrackHandle> {
        let handle = Arc::new(TrackHandle::new(usize::MAX, kind));
        handle.config.set_volume(1.0);
        tx.send(MixerCommand::AddTrack { kind, handle: handle.clone() }).unwrap();
        handle
    }

    /// Render `blocks` callbacks and keep every sample.
    fn render(mixer: &mut Mixer, transport: &Transport, frames: usize, blocks: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames * 2];
        let mut all = Vec::with_capacity(frames * 2 * blocks);
        for _ in 0..blocks {
            out.fill(0.0);
            mixer.process(&mut out, &[], transport);
            all.extend_from_slice(&out);
            transport.advance(frames as u32, 44_100);
        }
        all
    }

    /// Run the mixer until a meter has settled on the level it is being fed,
    /// and read it.
    ///
    /// A meter falls back to a lower level over several blocks — fast attack,
    /// slow decay — so a bare read taken while it is still falling is not the
    /// level being fed. The maximum across two consecutive blocks is.
    ///
    /// It used to be wrong for a second reason as well: the decay fired on
    /// equality, so a steady signal alternated between the true peak and 85%
    /// of it. `publish_vu` holds on equality now, and this still takes the
    /// maximum because settling is the thing it is really waiting for.
    fn settled_vu(
        mixer: &mut Mixer,
        transport: &Transport,
        frames: usize,
        handle: &TrackHandle,
    ) -> (f32, f32) {
        let _ = render(mixer, transport, frames, 40);
        let first = handle.vu.get();
        let _ = render(mixer, transport, frames, 1);
        let second = handle.vu.get();
        (first.0.max(second.0), first.1.max(second.1))
    }

    fn peak_of(samples: &[f32]) -> f32 {
        samples.iter().map(|s| s.abs()).fold(0.0, f32::max)
    }

    /// The peaks of the two channels of an interleaved buffer.
    fn peaks(samples: &[f32]) -> (f32, f32) {
        let mut l = 0.0f32;
        let mut r = 0.0f32;
        for frame in samples.chunks_exact(2) {
            l = l.max(frame[0].abs());
            r = r.max(frame[1].abs());
        }
        (l, r)
    }

    /// **The null test.** Six unity trims in a track's inserts have to leave
    /// the render exactly where an empty chain left it — every sample, every
    /// bit. It is the whole insert layer's licence to exist in the signal
    /// path: chains that are not doing anything must not be doing anything.
    ///
    /// The out-of-tree half of this — the same session rendered by v0.3.38 —
    /// is `examples/render_digest.rs`.
    #[test]
    fn a_chain_of_unity_trims_is_bit_identical_to_no_chain() {
        fn take(with_chain: bool) -> Vec<f32> {
            let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
            let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
            handle.config.midi_active.store(true, std::sync::atomic::Ordering::Relaxed);
            handle.config.set_volume(0.9);
            tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle }).unwrap();
            tx.send(MixerCommand::SetInstrument {
                track_id: 0,
                instrument: Box::new(PhosphorSynth::new()),
            })
            .unwrap();
            if with_chain {
                for slot in 0..MAX_FX_SLOTS {
                    tx.send(MixerCommand::AddFx {
                        target: FxTarget::Track(0),
                        slot,
                        effect: Box::new(Gain::new()),
                    })
                    .unwrap();
                }
            }
            apply_all(&mut mixer);
            transport.play();

            let mut out = vec![0.0f32; 256 * 2];
            mixer.process(&mut out, &[make_note_on(60, 100)], &transport);
            transport.advance(256, 44_100);
            let mut all = out.clone();
            all.extend_from_slice(&render(&mut mixer, &transport, 256, 40));
            all
        }

        let bare = take(false);
        let chained = take(true);
        assert!(peak_of(&bare) > 0.001, "the reference render was silent");
        assert_eq!(bare.len(), chained.len());
        for (i, (a, b)) in bare.iter().zip(&chained).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "sample {i}: an empty chain changed {a} into {b}"
            );
        }
    }

    /// A bypassed effect, once its crossfade has landed, is not in the signal
    /// path at all — not "inaudible", not in it.
    #[test]
    fn a_settled_bypass_is_bit_identical_through_the_mixer() {
        fn take(with_effect: bool) -> Vec<f32> {
            let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
            let _handle = add_fixed_track(&tx, 0, 0.4);
            if with_effect {
                tx.send(MixerCommand::AddFx {
                    target: FxTarget::Track(0),
                    slot: 0,
                    // Loud enough that a fade that never finished would be
                    // obvious in the first sample of the comparison, quiet
                    // enough that the master limiter never engages — a
                    // limiter riding on one of the two runs would make this
                    // test about the limiter's release instead.
                    effect: Box::new(Gain::at(-12.0)),
                })
                .unwrap();
                tx.send(MixerCommand::SetFxBypass {
                    target: FxTarget::Track(0),
                    slot: 0,
                    bypass: true,
                })
                .unwrap();
            }
            apply_all(&mut mixer);
            transport.play();
            // Two blocks of 512 at 44.1 kHz is 23 ms: the 8 ms crossfade is
            // long over.
            let _settling = render(&mut mixer, &transport, 512, 2);
            render(&mut mixer, &transport, 512, 4)
        }

        let bare = take(false);
        let bypassed = take(true);
        assert!(peak_of(&bare) > 0.1);
        for (i, (a, b)) in bare.iter().zip(&bypassed).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "bypassed effect altered sample {i}");
        }
    }

    /// An effect in a slot is in the signal path, and its parameter reaches
    /// it. The chain's proof of life.
    #[test]
    fn an_effect_in_a_slot_processes_the_track() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let _handle = add_fixed_track(&tx, 0, 0.25);
        tx.send(MixerCommand::AddFx {
            target: FxTarget::Track(0),
            slot: 0,
            effect: Box::new(Gain::at(-6.0)),
        })
        .unwrap();
        apply_all(&mut mixer);
        transport.play();

        let peak = peak_of(&render(&mut mixer, &transport, 256, 1));
        assert!((peak - 0.125_297).abs() < 1.0e-4, "-6 dB trim gave {peak}");

        tx.send(MixerCommand::SetFxParam {
            target: FxTarget::Track(0),
            slot: 0,
            param: 0,
            value: 0.0,
        })
        .unwrap();
        apply_all(&mut mixer);
        let peak = peak_of(&render(&mut mixer, &transport, 256, 1));
        assert!((peak - 0.25).abs() < 1.0e-6, "the parameter did not arrive: {peak}");
    }

    /// The cap is six, and it holds at the audio thread rather than only in
    /// the UI that is supposed to enforce it.
    #[test]
    fn a_seventh_effect_never_reaches_a_chain() {
        let (mut mixer, tx, _clip_rx, _transport) = setup_mixer();
        let _handle = add_fixed_track(&tx, 0, 0.1);
        for slot in 0..MAX_FX_SLOTS + 3 {
            tx.send(MixerCommand::AddFx {
                target: FxTarget::Track(0),
                slot,
                effect: Box::new(Gain::new()),
            })
            .unwrap();
        }
        apply_all(&mut mixer);
        assert_eq!(mixer.tracks[0].chain.len(), MAX_FX_SLOTS);
    }

    // ── Pan ──

    /// The pan law, measured: equal power across the sweep, one channel and
    /// silence at the ends, and the ends 3.01 dB above the centre.
    ///
    /// The reference point is the deviation to know about. FX.md's wording
    /// puts the centre at −3 dB and the extremes at 0 dB; this puts the
    /// centre at unity and the extremes at +3. The shape — which is what a
    /// pan law *is* — is identical; the difference is whether adding this
    /// feature makes every existing session 3 dB quieter. See `fx::pan_gains`.
    #[test]
    fn pan_sweeps_at_equal_power_with_unity_at_the_centre() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let _handle = add_fixed_track(&tx, 0, 0.5);
        apply_all(&mut mixer);
        transport.play();

        // A hard-panned track is at full travel in its own channel: half a
        // full-scale signal times √2.
        let end = std::f32::consts::FRAC_1_SQRT_2;
        let mut power = Vec::new();
        for (pan, expect) in [
            (-1.0f32, (end, 0.0)),
            (0.0, (0.5, 0.5)),
            (1.0, (0.0, end)),
        ] {
            tx.send(MixerCommand::SetPan { track_id: 0, pan }).unwrap();
            apply_all(&mut mixer);
            let (l, r) = peaks(&render(&mut mixer, &transport, 256, 1));
            assert!(
                (l - expect.0).abs() < 1.0e-3 && (r - expect.1).abs() < 1.0e-3,
                "pan {pan} gave ({l:.4}, {r:.4}), expected ({:.4}, {:.4})",
                expect.0,
                expect.1
            );
            power.push(l * l + r * r);
        }
        for p in &power {
            assert!(
                (p - power[0]).abs() < 1.0e-4,
                "the sweep changed power: {power:?}"
            );
        }
        let ends_over_centre = 20.0 * (end / 0.5).log10();
        assert!((ends_over_centre - 3.0103).abs() < 0.01);
    }

    /// The centre is exactly where the mixer was before pan existed: the same
    /// multiply by the fader and nothing else. Every session written before
    /// this milestone renders bit for bit as it did.
    #[test]
    fn the_pan_centre_changes_no_existing_render() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let handle = add_fixed_track(&tx, 0, 0.25);
        for volume in [0.0f32, 0.5, 0.75, 1.0, 2.0] {
            handle.config.set_volume(volume);
            apply_all(&mut mixer);
            let peak = peak_of(&render(&mut mixer, &transport, 128, 1));
            let expected = 0.25 * volume;
            assert_eq!(
                peak.to_bits(),
                expected.to_bits(),
                "fader {volume} gave {peak}, not {expected}"
            );
        }
    }

    // ── Sends ──

    /// A send at −6 dB puts the track into the bus 6 dB down. Measured at the
    /// bus meter, which reads after the bus's own chain and return level.
    #[test]
    fn a_send_reaches_the_bus_at_the_level_it_was_given() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let bus = attach_bus(&tx, TrackKind::SendA);
        let _track = add_fixed_track(&tx, 0, 0.25);
        apply_all(&mut mixer);
        transport.play();

        for send_db in [0.0f32, -6.0, -12.0, -20.0] {
            tx.send(MixerCommand::SetSendLevel {
                track_id: 0,
                send: SendSlot::A,
                gain: db_to_gain(send_db),
            })
            .unwrap();
            apply_all(&mut mixer);
            let (bus_peak, _) = settled_vu(&mut mixer, &transport, 256, &bus);
            let measured = 20.0 * (bus_peak / 0.25).log10();
            assert!(
                (measured - send_db).abs() < 0.05,
                "a {send_db} dB send arrived at {measured:.2} dB (bus peak {bus_peak:.4})"
            );
        }
    }

    /// Mute is at the fader and the sends are after it, so muting a track
    /// takes it out of the reverb as well as out of the mix. A send that
    /// survived the mute is the classic "why can I still hear it" bug.
    #[test]
    fn muting_a_track_kills_its_sends() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let bus = attach_bus(&tx, TrackKind::SendA);
        let handle = add_fixed_track(&tx, 0, 0.25);
        tx.send(MixerCommand::SetSendLevel { track_id: 0, send: SendSlot::A, gain: 1.0 })
            .unwrap();
        apply_all(&mut mixer);
        transport.play();

        let out = render(&mut mixer, &transport, 256, 1);
        assert!((peak_of(&out) - 0.5).abs() < 1.0e-3, "track plus its send should be double");
        assert!(bus.vu.get().0 > 0.2);

        handle.config.muted.store(true, std::sync::atomic::Ordering::Relaxed);
        let out = render(&mut mixer, &transport, 256, 1);
        assert_eq!(peak_of(&out), 0.0, "a muted track was still audible");
        // The meter decays rather than snapping, so run it out.
        let _ = render(&mut mixer, &transport, 256, 60);
        assert!(bus.vu.get().0 < 1.0e-3, "the send survived the mute");
    }

    /// Solo is about the tracks. A soloed track still has its reverb, so the
    /// buses are exempt — without the exemption every solo goes bone dry.
    #[test]
    fn a_solo_leaves_the_buses_audible() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let bus = attach_bus(&tx, TrackKind::SendA);
        let handle = add_fixed_track(&tx, 0, 0.25);
        let _other = add_fixed_track(&tx, 1, 0.25);
        tx.send(MixerCommand::SetSendLevel { track_id: 0, send: SendSlot::A, gain: 1.0 })
            .unwrap();
        apply_all(&mut mixer);
        transport.play();

        handle.config.soloed.store(true, std::sync::atomic::Ordering::Relaxed);
        let out = render(&mut mixer, &transport, 256, 1);
        assert!(bus.vu.get().0 > 0.2, "the send bus went silent under solo");
        assert!(
            (peak_of(&out) - 0.5).abs() < 1.0e-3,
            "the soloed track lost its send: peak {}",
            peak_of(&out)
        );
    }

    /// A send is post-pan as well as post-fader, so a hard-panned track
    /// arrives in the bus panned.
    #[test]
    fn a_send_is_tapped_after_the_pan() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let bus = attach_bus(&tx, TrackKind::SendA);
        let _track = add_fixed_track(&tx, 0, 0.25);
        tx.send(MixerCommand::SetSendLevel { track_id: 0, send: SendSlot::A, gain: 1.0 })
            .unwrap();
        tx.send(MixerCommand::SetPan { track_id: 0, pan: -1.0 }).unwrap();
        apply_all(&mut mixer);
        transport.play();

        let _ = render(&mut mixer, &transport, 256, 1);
        let (l, r) = bus.vu.get();
        // Hard left is the full travel: the track's level times √2.
        let expected = 0.25 * std::f32::consts::SQRT_2;
        assert!((l - expected).abs() < 1.0e-3, "bus left is {l}, expected {expected}");
        assert!(r < 1.0e-6, "a hard-left send leaked {r} into the bus's right");
    }

    /// An effect on the bus is in the return path, and the bus meter reads
    /// after it.
    #[test]
    fn the_bus_meter_reads_after_the_bus_chain() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let bus = attach_bus(&tx, TrackKind::SendA);
        let _track = add_fixed_track(&tx, 0, 0.5);
        tx.send(MixerCommand::SetSendLevel { track_id: 0, send: SendSlot::A, gain: 1.0 })
            .unwrap();
        tx.send(MixerCommand::AddFx {
            target: FxTarget::BusA,
            slot: 0,
            effect: Box::new(Gain::at(-6.0)),
        })
        .unwrap();
        apply_all(&mut mixer);
        transport.play();

        let _ = render(&mut mixer, &transport, 256, 1);
        let (l, _) = bus.vu.get();
        assert!((l - 0.2506).abs() < 1.0e-3, "bus meter reads {l}, not the post-chain level");
    }

    /// The bus's return level is its fader, and muting the bus takes the
    /// return out without touching the tracks feeding it.
    #[test]
    fn the_bus_return_level_is_its_fader() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let bus = attach_bus(&tx, TrackKind::SendA);
        let _track = add_fixed_track(&tx, 0, 0.25);
        tx.send(MixerCommand::SetSendLevel { track_id: 0, send: SendSlot::A, gain: 1.0 })
            .unwrap();
        apply_all(&mut mixer);
        transport.play();

        bus.config.set_volume(0.5);
        let out = render(&mut mixer, &transport, 256, 1);
        assert!(
            (peak_of(&out) - 0.375).abs() < 1.0e-4,
            "track 0.25 plus a half-return send should be 0.375, got {}",
            peak_of(&out)
        );

        bus.config.muted.store(true, std::sync::atomic::Ordering::Relaxed);
        let out = render(&mut mixer, &transport, 256, 1);
        assert!(
            (peak_of(&out) - 0.25).abs() < 1.0e-6,
            "muting the bus left {} in the mix",
            peak_of(&out)
        );
    }

    // ── Meters ──

    /// The defect this milestone fixes: the channel meter read the
    /// instrument's raw output, so pulling the fader down did nothing to it.
    /// A meter that does not follow the fader is not a meter.
    #[test]
    fn the_channel_meter_follows_the_fader() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let handle = add_fixed_track(&tx, 0, 0.5);
        apply_all(&mut mixer);
        transport.play();

        let _ = render(&mut mixer, &transport, 256, 1);
        let (unity, _) = handle.vu.get();
        assert!((unity - 0.5).abs() < 1.0e-6, "at unity the meter reads {unity}");

        handle.config.set_volume(db_to_gain(-12.0));
        let (reduced, _) = settled_vu(&mut mixer, &transport, 256, &handle);
        let moved = 20.0 * (reduced / unity).log10();
        assert!(
            (moved - (-12.0)).abs() < 0.05,
            "a 12 dB cut moved the meter by {moved:.2} dB"
        );
    }

    /// ...and it reads after the inserts too, so an effect that changes the
    /// level shows on the meter.
    #[test]
    fn the_channel_meter_reads_after_the_inserts() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let handle = add_fixed_track(&tx, 0, 0.5);
        tx.send(MixerCommand::AddFx {
            target: FxTarget::Track(0),
            slot: 0,
            effect: Box::new(Gain::at(-6.0)),
        })
        .unwrap();
        apply_all(&mut mixer);
        transport.play();

        let _ = render(&mut mixer, &transport, 256, 1);
        let (peak, _) = handle.vu.get();
        assert!((peak - 0.2506).abs() < 1.0e-3, "the meter reads {peak}, before the insert");
    }

    /// The master limiter's gain reduction becomes a number the UI can draw,
    /// computed on this side because only this side sees every sample.
    #[test]
    fn the_limiter_publishes_its_gain_reduction() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let meter = mixer.limiter_gr_meter();
        assert_eq!(meter.get(), (0.0, 0.0));

        for id in 0..6 {
            add_fixed_track(&tx, id, 0.75);
        }
        apply_all(&mut mixer);
        transport.play();

        let _ = render(&mut mixer, &transport, 256, 4);
        let (current, peak) = meter.get();
        assert!(current < -6.0, "a 4.5x mix published {current:.2} dB of reduction");
        assert!(peak <= current, "the peak cell is above the bar");

        // And it comes back: the tracks are gone, so the reduction releases.
        for id in 0..6 {
            tx.send(MixerCommand::RemoveTrack { track_id: id }).unwrap();
        }
        apply_all(&mut mixer);
        let _ = render(&mut mixer, &transport, 256, 600);
        assert_eq!(meter.current_db(), 0.0, "the meter never released");
    }

    /// The master's own inserts are in the path, ahead of the limiter — so an
    /// effect that pushes the mix over the ceiling is caught by it rather
    /// than reaching the device.
    #[test]
    fn the_master_chain_runs_before_the_limiter() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let _track = add_fixed_track(&tx, 0, 0.25);
        tx.send(MixerCommand::AddFx {
            target: FxTarget::Master,
            slot: 0,
            effect: Box::new(Gain::at(24.0)),
        })
        .unwrap();
        apply_all(&mut mixer);
        transport.play();

        let out = render(&mut mixer, &transport, 256, 2);
        let peak = peak_of(&out);
        assert!(peak > 0.8, "the master trim did nothing: peak {peak}");
        assert!(peak <= LIMITER_CEILING, "the master chain got past the limiter: {peak}");
        assert!(mixer.limiter_gr_meter().current_db() < -6.0);
    }

    // ── The sidechain key ──

    /// An effect that replaces its input with the key it was given, so a test
    /// can see exactly which block of which track reached it.
    struct KeyCopy;

    impl Effect for KeyCopy {
        fn name(&self) -> &'static str {
            "keycopy"
        }
        fn init(&mut self, _sample_rate: f64, _max_buffer_size: usize) {}
        fn process(&mut self, left: &mut [f32], right: &mut [f32], ctx: &FxContext<'_>) {
            // With no key this effect leaves the signal alone, which is the
            // internal fallback the mixer promises.
            if let Some((key_l, key_r)) = ctx.key {
                left.copy_from_slice(&key_l[..left.len()]);
                right.copy_from_slice(&key_r[..right.len()]);
            }
        }
        fn reset(&mut self) {}
        fn parameter_count(&self) -> usize {
            0
        }
        fn parameter_info(&self, _index: usize) -> Option<FxParamInfo> {
            None
        }
        fn get_parameter(&self, _index: usize) -> f32 {
            0.0
        }
        fn set_parameter(&mut self, _index: usize, _value: f32) {}
        fn wants_key(&self) -> bool {
            true
        }
    }

    /// **The key tap.** A track keyed to another one gets that track's
    /// signal from *this* block, whichever order the two sit in. The two
    /// passes are what buy that: a single pass would hand the key a stale
    /// block whenever the source happens to come later in the list.
    ///
    /// The source is muted, so anything reaching the output arrived through
    /// the key. That also states the tap's position out loud: it is
    /// post-instrument and pre-insert, which is ahead of the fader — a muted
    /// track still keys.
    #[test]
    fn a_key_is_the_same_block_whatever_the_order() {
        fn take(source_first: bool) -> Vec<f32> {
            let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
            let (source_id, keyed_id) = if source_first { (0, 1) } else { (1, 0) };

            let make = |id: usize, source: bool| {
                let handle = Arc::new(TrackHandle::new(id, TrackKind::Instrument));
                handle.config.set_volume(1.0);
                if source {
                    handle.config.muted.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                tx.send(MixerCommand::AddTrack {
                    kind: TrackKind::Instrument,
                    handle,
                })
                .unwrap();
                if source {
                    tx.send(MixerCommand::SetInstrument {
                        track_id: id,
                        instrument: Box::new(Ramp::new()),
                    })
                    .unwrap();
                }
            };

            // Added in the order the ids say, so the two runs really do put
            // the source on opposite sides of the keyed track.
            for id in 0..2 {
                make(id, id == source_id);
            }
            tx.send(MixerCommand::AddFx {
                target: FxTarget::Track(keyed_id),
                slot: 0,
                effect: Box::new(KeyCopy),
            })
            .unwrap();
            tx.send(MixerCommand::SetKeySource {
                track_id: keyed_id,
                source: Some(source_id),
            })
            .unwrap();
            apply_all(&mut mixer);
            transport.play();
            render(&mut mixer, &transport, 64, 8)
        }

        let source_before = take(true);
        let source_after = take(false);
        assert!(peak_of(&source_before) > 0.1, "nothing came through the key");
        for (i, (a, b)) in source_before.iter().zip(&source_after).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "sample {i} depends on which order the tracks are in: {a} vs {b}"
            );
        }

        // And what came through is the source's own signal, sample for
        // sample — the same block, not the one before it.
        let expected = {
            let mut ramp = Ramp::new();
            let mut l = vec![0.0f32; 64 * 8];
            let mut r = vec![0.0f32; 64 * 8];
            for block in 0..8 {
                let range = block * 64..(block + 1) * 64;
                let mut outs: [&mut [f32]; 2] =
                    [&mut l[range.clone()], &mut r[range]];
                ramp.process(&[], &mut outs, &[]);
            }
            l
        };
        for (i, (a, b)) in expected.iter().zip(source_before.chunks_exact(2)).enumerate() {
            assert_eq!(a.to_bits(), b[0].to_bits(), "key sample {i}");
        }
    }

    /// A key that names a track which no longer exists falls back to the
    /// internal key **this block**. Never a stale buffer, never silence, and
    /// never whatever track happens to have moved into that position.
    #[test]
    fn a_deleted_key_track_falls_back_to_internal() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let _source = add_fixed_track(&tx, 7, 0.25);
        let _keyed = add_fixed_track(&tx, 1, 0.0625);
        tx.send(MixerCommand::AddFx {
            target: FxTarget::Track(1),
            slot: 0,
            effect: Box::new(KeyCopy),
        })
        .unwrap();
        tx.send(MixerCommand::SetKeySource { track_id: 1, source: Some(7) }).unwrap();
        apply_all(&mut mixer);
        transport.play();

        // The keyed track copies the source, so the mix is the source
        // twice over rather than the source plus its own quiet output.
        let peak = peak_of(&render(&mut mixer, &transport, 128, 1));
        assert!((peak - 0.5).abs() < 1.0e-4, "the key never arrived: {peak}");

        tx.send(MixerCommand::RemoveTrack { track_id: 7 }).unwrap();
        apply_all(&mut mixer);
        let peak = peak_of(&render(&mut mixer, &transport, 128, 1));
        assert!(
            (peak - 0.0625).abs() < 1.0e-6,
            "a deleted key track left {peak} — the keyed track kept reading something"
        );
    }

    // ── Key listen ──

    /// **Key listen plays the key.**
    ///
    /// The track's own output is replaced, sample for sample, by the signal
    /// its compressor would be keying off — which is what makes the sidechain
    /// tunable by ear rather than by guesswork.
    #[test]
    fn key_listen_plays_the_key() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        // The source is a ramp so every sample is a different number: a key
        // that arrived from the wrong place, or a block late, would show.
        let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
        handle.config.set_volume(1.0);
        handle.config.muted.store(true, std::sync::atomic::Ordering::Relaxed);
        tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle }).unwrap();
        tx.send(MixerCommand::SetInstrument { track_id: 0, instrument: Box::new(Ramp::new()) })
            .unwrap();
        let _keyed = add_fixed_track(&tx, 1, 0.25);
        tx.send(MixerCommand::SetKeySource { track_id: 1, source: Some(0) }).unwrap();
        apply_all(&mut mixer);
        transport.play();

        // Off: the keyed track is its own quiet self.
        let plain = render(&mut mixer, &transport, 64, 4);
        assert!((peak_of(&plain) - 0.25).abs() < 1.0e-6, "the rig was not what it says");

        // On: the keyed track is the ramp, and the ramp is nothing like 0.25.
        tx.send(MixerCommand::SetKeyListen { track: Some(1) }).unwrap();
        apply_all(&mut mixer);
        assert_eq!(mixer.key_listen(), Some(1));
        let listened = render(&mut mixer, &transport, 64, 4);

        let expected = {
            let mut ramp = Ramp::new();
            let mut l = vec![0.0f32; 64 * 8];
            let mut r = vec![0.0f32; 64 * 8];
            for block in 0..8 {
                let range = block * 64..(block + 1) * 64;
                let mut outs: [&mut [f32]; 2] = [&mut l[range.clone()], &mut r[range]];
                ramp.process(&[], &mut outs, &[]);
            }
            l
        };
        // The source has already rendered four blocks for the `plain` pass,
        // so the audible ramp starts where that left off.
        for (i, frame) in listened.chunks_exact(2).enumerate() {
            assert_eq!(
                frame[0].to_bits(),
                expected[64 * 4 + i].to_bits(),
                "sample {i}: key listen played {} and the key was {}",
                frame[0],
                expected[64 * 4 + i]
            );
        }
    }

    /// **Only one, and the type is what says so.**
    ///
    /// Arming a second track disarms the first by itself, because there is
    /// one `Option` and not a flag per track. A rule the compiler enforces is
    /// a rule nobody can forget.
    #[test]
    fn only_one_key_listen_is_ever_armed() {
        let (mut mixer, tx, _clip_rx, _transport) = setup_mixer();
        let _a = add_fixed_track(&tx, 0, 0.25);
        let _b = add_fixed_track(&tx, 1, 0.5);
        tx.send(MixerCommand::SetKeyListen { track: Some(0) }).unwrap();
        apply_all(&mut mixer);
        assert_eq!(mixer.key_listen(), Some(0));

        tx.send(MixerCommand::SetKeyListen { track: Some(1) }).unwrap();
        apply_all(&mut mixer);
        assert_eq!(mixer.key_listen(), Some(1), "the second arming did not take");

        // A track that does not exist is refused rather than stored, so the
        // flag cannot outlive what it names.
        tx.send(MixerCommand::SetKeyListen { track: Some(99) }).unwrap();
        apply_all(&mut mixer);
        assert_eq!(mixer.key_listen(), None);

        tx.send(MixerCommand::SetKeyListen { track: Some(0) }).unwrap();
        apply_all(&mut mixer);
        tx.send(MixerCommand::SetKeyListen { track: None }).unwrap();
        apply_all(&mut mixer);
        assert_eq!(mixer.key_listen(), None, "it could not be switched off");
    }

    /// **It clears itself on a stop, and on a panic.**
    ///
    /// Both are the audio thread's own doing rather than the front end's: a
    /// UI that crashed, or a panel that was closed by something that forgot,
    /// must not leave a track monitoring its sidechain for the rest of the
    /// session.
    #[test]
    fn key_listen_clears_itself_on_a_stop() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let _a = add_fixed_track(&tx, 0, 0.25);
        transport.play();
        tx.send(MixerCommand::SetKeyListen { track: Some(0) }).unwrap();
        apply_all(&mut mixer);
        let _ = render(&mut mixer, &transport, 64, 1);
        assert_eq!(mixer.key_listen(), Some(0));

        transport.pause();
        let _ = render(&mut mixer, &transport, 64, 1);
        assert_eq!(mixer.key_listen(), None, "the stop did not clear it");

        // Armed while stopped, it stays armed — auditioning a key against
        // live playing is a thing people do.
        tx.send(MixerCommand::SetKeyListen { track: Some(0) }).unwrap();
        apply_all(&mut mixer);
        let _ = render(&mut mixer, &transport, 64, 4);
        assert_eq!(mixer.key_listen(), Some(0));

        // ...and the panic path drops it with everything else.
        mixer.reset_all();
        assert_eq!(mixer.key_listen(), None, "a panic left it armed");
    }

    /// With no external key, listening plays the track's own pre-insert
    /// signal — the internal key, which is what the detector would be reading.
    #[test]
    fn key_listen_with_no_external_key_plays_the_track_itself() {
        let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
        let handle = add_fixed_track(&tx, 0, 0.25);
        tx.send(MixerCommand::AddFx {
            target: FxTarget::Track(0),
            slot: 0,
            effect: Box::new(Gain::at(-20.0)),
        })
        .unwrap();
        tx.send(MixerCommand::SetKeyListen { track: Some(0) }).unwrap();
        apply_all(&mut mixer);
        transport.play();
        let _ = handle;

        let out = render(&mut mixer, &transport, 256, 4);
        // The insert took 20 dB off, and the key listen puts it back: what is
        // heard is the pre-insert signal, which is the tap's own position.
        assert!(
            (peak_of(&out) - 0.25).abs() < 1.0e-4,
            "listening with no key played {} rather than the track itself",
            peak_of(&out)
        );
    }

    /// Keying a track to itself is the internal key, not a self-reference the
    /// borrow checker has to be argued out of.
    #[test]
    fn a_track_cannot_key_off_itself() {
        let (mut mixer, tx, _clip_rx, _transport) = setup_mixer();
        let _track = add_fixed_track(&tx, 3, 0.5);
        tx.send(MixerCommand::SetKeySource { track_id: 3, source: Some(3) }).unwrap();
        apply_all(&mut mixer);
        assert_eq!(mixer.tracks[0].key_source, None);
    }

    /// A ramp, so that every sample of a block is a different number and a
    /// key that arrived a block late would be visibly wrong.
    struct Ramp {
        phase: f32,
    }

    impl Ramp {
        fn new() -> Self {
            Self { phase: 0.0 }
        }
    }

    impl Plugin for Ramp {
        fn info(&self) -> phosphor_plugin::PluginInfo {
            phosphor_plugin::PluginInfo {
                name: "Ramp".into(),
                version: "0".into(),
                author: "test".into(),
                category: phosphor_plugin::PluginCategory::Instrument,
            }
        }
        fn init(&mut self, _sample_rate: f64, _max_buffer_size: usize) {}
        fn process(&mut self, _i: &[&[f32]], outputs: &mut [&mut [f32]], _m: &[MidiEvent]) {
            for frame in 0..outputs[0].len() {
                self.phase += 0.01;
                if self.phase > 0.5 {
                    self.phase = -0.5;
                }
                outputs[0][frame] = self.phase;
                outputs[1][frame] = self.phase * 0.5;
            }
        }
        fn parameter_count(&self) -> usize {
            0
        }
        fn parameter_info(&self, _: usize) -> Option<phosphor_plugin::ParameterInfo> {
            None
        }
        fn get_parameter(&self, _: usize) -> f32 {
            0.0
        }
        fn set_parameter(&mut self, _: usize, _: f32) {}
        fn reset(&mut self) {}
    }

    // ── The rule the audio thread lives by ──

    /// Four tracks with six effects each, both buses loaded, the master
    /// loaded, sends open, a bypass crossfading and a key resolving — and not
    /// one call to the allocator.
    #[test]
    fn a_full_insert_layer_does_not_allocate() {
        let max_frames = 512usize;
        let (tx, rx) = mixer_command_channel();
        let (clip_tx, _clip_rx) = clip_snapshot_channel();
        let mut mixer = Mixer::new(rx, Arc::new(VuLevels::new()), clip_tx, 48_000, max_frames);
        let transport = Arc::new(Transport::new(120.0));
        attach_bus(&tx, TrackKind::SendA);
        attach_bus(&tx, TrackKind::SendB);
        attach_bus(&tx, TrackKind::Master);

        for id in 0..4 {
            let handle = Arc::new(TrackHandle::new(id, TrackKind::Instrument));
            handle.config.midi_active.store(true, std::sync::atomic::Ordering::Relaxed);
            tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle }).unwrap();
            tx.send(MixerCommand::SetInstrument {
                track_id: id,
                instrument: Box::new(PhosphorSynth::new()),
            })
            .unwrap();
            for slot in 0..MAX_FX_SLOTS {
                tx.send(MixerCommand::AddFx {
                    target: FxTarget::Track(id),
                    slot,
                    effect: Box::new(Gain::at(-0.5)),
                })
                .unwrap();
            }
            tx.send(MixerCommand::SetPan { track_id: id, pan: (id as f32 - 1.5) / 1.5 })
                .unwrap();
            tx.send(MixerCommand::SetSendLevel {
                track_id: id,
                send: SendSlot::A,
                gain: 0.5,
            })
            .unwrap();
            tx.send(MixerCommand::SetSendLevel {
                track_id: id,
                send: SendSlot::B,
                gain: 0.25,
            })
            .unwrap();
        }
        // A key that resolves, on a chain that asks for one.
        tx.send(MixerCommand::AddFx {
            target: FxTarget::Track(0),
            slot: 0,
            effect: Box::new(KeyCopy),
        })
        .unwrap();
        tx.send(MixerCommand::SetKeySource { track_id: 0, source: Some(3) }).unwrap();
        for target in [FxTarget::BusA, FxTarget::BusB, FxTarget::Master] {
            for slot in 0..MAX_FX_SLOTS {
                tx.send(MixerCommand::AddFx {
                    target,
                    slot,
                    effect: Box::new(Gain::at(-0.25)),
                })
                .unwrap();
            }
        }
        apply_all(&mut mixer);
        transport.play();

        let mut output = vec![0.0f32; max_frames * 2];
        // Warm-up: the wavetable bank behind its `OnceLock` is built here.
        mixer.process(&mut output, &[make_note_on(60, 100)], &transport);

        let allocations = crate::alloc_count::allocations_during(|| {
            for block in 0..16 {
                // A bypass thrown mid-run, so the crossfade path — the one
                // that copies the dry signal aside — is inside the
                // measurement too.
                if block == 4 {
                    tx.send(MixerCommand::SetFxBypass {
                        target: FxTarget::Track(1),
                        slot: 2,
                        bypass: true,
                    })
                    .unwrap();
                }
                // ...and a key listen armed and disarmed mid-run, so the
                // block that copies a whole key over a track's output is
                // inside the measurement too.
                if block == 6 {
                    tx.send(MixerCommand::SetKeyListen { track: Some(2) }).unwrap();
                }
                if block == 12 {
                    tx.send(MixerCommand::SetKeyListen { track: None }).unwrap();
                }
                mixer.process(&mut output, &[], &transport);
                transport.advance(max_frames as u32, 48_000);
            }
        });
        assert_eq!(allocations, 0, "the insert layer reached the allocator");
    }

    /// A short block, with the same load: the crossfade scratch is sized for
    /// the device's maximum and must not be re-sized for a smaller one.
    #[test]
    fn a_short_block_through_a_full_chain_does_not_allocate() {
        let (tx, rx) = mixer_command_channel();
        let (clip_tx, _clip_rx) = clip_snapshot_channel();
        let mut mixer = Mixer::new(rx, Arc::new(VuLevels::new()), clip_tx, 48_000, 512);
        let transport = Arc::new(Transport::new(120.0));
        let _handle = add_fixed_track(&tx, 0, 0.25);
        for slot in 0..MAX_FX_SLOTS {
            tx.send(MixerCommand::AddFx {
                target: FxTarget::Track(0),
                slot,
                effect: Box::new(Gain::at(-1.0)),
            })
            .unwrap();
        }
        tx.send(MixerCommand::SetFxBypass {
            target: FxTarget::Track(0),
            slot: 0,
            bypass: true,
        })
        .unwrap();
        apply_all(&mut mixer);
        transport.play();

        let mut output = vec![0.0f32; 32 * 2];
        mixer.process(&mut output, &[], &transport);
        let allocations = crate::alloc_count::allocations_during(|| {
            for _ in 0..8 {
                mixer.process(&mut output, &[], &transport);
            }
        });
        assert_eq!(allocations, 0, "a short block reached the allocator");
    }

    /// The panic key drops the tails as well as the notes. A reverb still
    /// ringing after everything has been silenced is what the key is for.
    #[test]
    fn a_panic_drops_the_insert_tails() {
        let (mut mixer, tx, _clip_rx, _transport) = setup_mixer();
        attach_bus(&tx, TrackKind::SendA);
        let _track = add_fixed_track(&tx, 0, 0.5);
        tx.send(MixerCommand::AddFx {
            target: FxTarget::Track(0),
            slot: 0,
            effect: Box::new(Tail::default()),
        })
        .unwrap();
        tx.send(MixerCommand::AddFx {
            target: FxTarget::BusA,
            slot: 0,
            effect: Box::new(Tail::default()),
        })
        .unwrap();
        apply_all(&mut mixer);

        let mut output = vec![0.0f32; 128];
        mixer.process(&mut output, &[], &_transport);
        mixer.reset_all();
        // Both chains were reset, and the bus buffer with them.
        let peak = peak_of(&output);
        assert!(peak > 0.0, "nothing was rendered, so this proves nothing");
        mixer.process(&mut output, &[], &_transport);
        assert!(
            mixer.bus_a.buf_l.iter().all(|s| *s == 0.0),
            "the bus kept a tail through the panic"
        );
    }

    /// An effect that would ring forever if it were never reset.
    #[derive(Default)]
    struct Tail {
        held: f32,
    }

    impl Effect for Tail {
        fn name(&self) -> &'static str {
            "tail"
        }
        fn init(&mut self, _sample_rate: f64, _max_buffer_size: usize) {}
        fn process(&mut self, left: &mut [f32], right: &mut [f32], _ctx: &FxContext<'_>) {
            for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                self.held = self.held.max(l.abs());
                *l += self.held;
                *r += self.held;
            }
        }
        fn reset(&mut self) {
            self.held = 0.0;
        }
        fn parameter_count(&self) -> usize {
            0
        }
        fn parameter_info(&self, _index: usize) -> Option<FxParamInfo> {
            None
        }
        fn get_parameter(&self, _index: usize) -> f32 {
            0.0
        }
        fn set_parameter(&mut self, _index: usize, _value: f32) {}
    }

    /// The flicker: a steady signal produced the same peak every block, and a
    /// meter that decayed on equality alternated between that peak and 85% of
    /// it forever. It holds now, and still falls when the signal does.
    #[test]
    fn a_steady_signal_holds_the_meter_still() {
        let vu = VuLevels::new();
        for _ in 0..8 {
            publish_vu(&vu, 0.5, 0.25);
        }
        assert_eq!(vu.get(), (0.5, 0.25));
        // Ten more blocks of exactly the same level do not move it.
        for _ in 0..10 {
            publish_vu(&vu, 0.5, 0.25);
            assert_eq!(vu.get(), (0.5, 0.25), "a level that is not moving moved the meter");
        }

        // ...and a signal that stops still falls.
        publish_vu(&vu, 0.0, 0.0);
        let (l, r) = vu.get();
        assert!(l < 0.5 && l > 0.0, "the meter did not decay: {l}");
        assert!(r < 0.25 && r > 0.0);

        // ...and a louder block is taken at once.
        publish_vu(&vu, 0.9, 0.9);
        assert_eq!(vu.get(), (0.9, 0.9));
    }
}
