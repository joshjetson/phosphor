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
use crate::metronome::Metronome;
use crate::project::{TrackHandle, TrackKind};
use crate::transport::Transport;

// ── Commands ──

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
        MixerCommand::SetParameter { .. } | MixerCommand::UpdateClipPosition { .. } => 1,
        MixerCommand::AddTrack { .. }
        | MixerCommand::SetInstrument { .. }
        | MixerCommand::RemoveTrack { .. }
        | MixerCommand::CreateClip { .. }
        | MixerCommand::UpdateClip { .. }
        | MixerCommand::RemoveClip { .. } => HEAVY_COMMAND,
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
}

impl MasterLimiter {
    fn new(sample_rate: u32) -> Self {
        let sr = (sample_rate as f32).max(1.0);
        Self {
            gain: 1.0,
            release_coeff: 1.0 - (-1.0 / (LIMITER_RELEASE_SECONDS * sr)).exp(),
        }
    }

    fn reset(&mut self) {
        self.gain = 1.0;
    }

    /// Limit an interleaved stereo buffer in place.
    ///
    /// On return every sample is finite and within ±1.0. Any frame that was
    /// not finite on the way in leaves as silence.
    fn process(&mut self, output: &mut [f32]) {
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

pub struct AudioTrack {
    pub id: usize,
    pub kind: TrackKind,
    pub handle: Arc<TrackHandle>,
    pub instrument: Option<Box<dyn Plugin>>,
    /// Recorded clips on this track's timeline.
    pub clips: Vec<MidiClip>,
    /// Active recording buffer (when armed + transport recording).
    record_buf: RecordBuffer,
    /// Whether we were recording last buffer (to detect stop).
    was_recording: bool,
    /// Last tick position seen during recording (to detect loop wraps).
    last_record_tick: i64,
    /// Last tick position seen during playback (to detect loop wraps for clip playback).
    last_playback_tick: i64,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    plugin_events: Vec<MidiEvent>,
}

impl AudioTrack {
    pub fn new(handle: Arc<TrackHandle>, max_buffer_size: usize) -> Self {
        Self {
            id: handle.id,
            kind: handle.kind,
            handle,
            instrument: None,
            clips: Vec::new(),
            record_buf: RecordBuffer::new(),
            was_recording: false,
            last_record_tick: -1,
            last_playback_tick: -1,
            buf_l: vec![0.0; max_buffer_size],
            buf_r: vec![0.0; max_buffer_size],
            plugin_events: Vec::with_capacity(256),
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
    /// Final stage before the audio device — see [`MasterLimiter`].
    limiter: MasterLimiter,
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
            limiter: MasterLimiter::new(sample_rate),
        }
    }

    /// Process one buffer cycle.
    pub fn process(&mut self, output: &mut [f32], midi_messages: &[MidiMessage], transport: &Transport) {
        // Bounded: whatever does not fit in this callback's budget is applied
        // by the next one, in order. See `drain_commands`.
        let _ = self.drain_commands();

        let num_frames = output.len() / 2;
        let playing = transport.is_playing();
        let recording = transport.is_recording();
        let looping = transport.is_looping();
        let current_tick = transport.position_ticks();
        let bpm = transport.tempo_bpm();
        let ticks_per_sample = (bpm * Transport::PPQ as f64) / (60.0 * self.sample_rate as f64);
        let buffer_ticks = (num_frames as f64 * ticks_per_sample) as i64;
        let loop_end = transport.loop_end();

        // Convert live MIDI to plugin events (reuse pre-allocated buffer)
        self.live_events.clear();
        for msg in midi_messages {
            if let Some(ev) = midi_to_plugin_event(msg) {
                self.live_events.push(ev);
            }
        }

        let any_solo = self.tracks.iter().any(|t| t.handle.config.is_soloed());

        // Reuse pre-allocated scratch buffers for master mix.
        // Swap out of self to avoid borrow conflicts in the track loop.
        let mut master_l = std::mem::take(&mut self.scratch_l);
        let mut master_r = std::mem::take(&mut self.scratch_r);
        let live_events = std::mem::take(&mut self.live_events);
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

        let clip_tx = &self.clip_tx;

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
                commit_recording(track, loop_end, clip_tx);
                // Start new recording at loop start, not current_tick
                // (current_tick may be a few ticks past 0 due to buffer boundaries)
                track.record_buf.start(transport.loop_start());
            }
            if should_record {
                track.last_record_tick = current_tick;
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

            // ── Playback ──
            if playing && !track.clips.is_empty() {
                let from = current_tick;
                let to = current_tick + buffer_ticks;

                // Detect loop wrap using dedicated playback tick tracker
                // (separate from recording tick to avoid interference)
                let just_wrapped = looping && track.last_playback_tick >= 0
                    && current_tick < track.last_playback_tick;
                track.last_playback_tick = current_tick;

                if just_wrapped {
                    // Play events from loop_start to current position (the wrapped portion)
                    let wrap_start = transport.loop_start();
                    for clip in &track.clips {
                        for (tick_offset, event) in clip.events_in_range(wrap_start, to) {
                            let sample_offset = (tick_offset as f64 / ticks_per_sample) as u32;
                            track.plugin_events.push(MidiEvent {
                                sample_offset: sample_offset.min(num_frames as u32 - 1),
                                status: event.status,
                                data1: event.data1,
                                data2: event.data2,
                            });
                        }
                    }
                } else {
                    for clip in &track.clips {
                        for (tick_offset, event) in clip.events_in_range(from, to) {
                            let sample_offset = (tick_offset as f64 / ticks_per_sample) as u32;
                            track.plugin_events.push(MidiEvent {
                                sample_offset: sample_offset.min(num_frames as u32 - 1),
                                status: event.status,
                                data1: event.data1,
                                data2: event.data2,
                            });
                        }
                    }
                }
                track.plugin_events.sort_by_key(|e| e.sample_offset);
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

            // ── VU + Mix ──
            let muted = track.handle.config.is_muted();
            let soloed = track.handle.config.is_soloed();
            let audible = !muted && (!any_solo || soloed);
            let volume = track.handle.config.get_volume();

            let mut peak_l = 0.0f32;
            let mut peak_r = 0.0f32;
            for i in 0..num_frames {
                peak_l = peak_l.max(track.buf_l[i].abs());
                peak_r = peak_r.max(track.buf_r[i].abs());
            }

            let (old_l, old_r) = track.handle.vu.get();
            let decay = 0.85f32;
            track.handle.vu.set(
                if peak_l > old_l { peak_l } else { old_l * decay },
                if peak_r > old_r { peak_r } else { old_r * decay },
            );

            if audible {
                for i in 0..num_frames {
                    master_l[i] += track.buf_l[i] * volume;
                    master_r[i] += track.buf_r[i] * volume;
                }
            }
        }

        // Write tracks to interleaved output
        for i in 0..num_frames {
            output[i * 2] = master_l[i];
            output[i * 2 + 1] = master_r[i];
        }

        // Return scratch buffers to self (no allocation, just moves)
        self.scratch_l = master_l;
        self.scratch_r = master_r;
        self.live_events = live_events;

        // Mix metronome click into output (after tracks, so it's always audible)
        self.metronome.process(output, transport);

        // ── Master limiter ──
        // Everything that reaches the device passes through here, the
        // metronome included: it is summed on top of the track mix, so
        // limiting before it would leave a gap in the guarantee.
        self.limiter.process(output);

        // Master VU (includes metronome), read after limiting so the meter
        // shows what actually left rather than what would have.
        let mut mp_l = 0.0f32;
        let mut mp_r = 0.0f32;
        for i in 0..num_frames {
            mp_l = mp_l.max(output[i * 2].abs());
            mp_r = mp_r.max(output[i * 2 + 1].abs());
        }

        let (old_l, old_r) = self.master_vu.get();
        let decay = 0.85f32;
        self.master_vu.set(
            if mp_l > old_l { mp_l } else { old_l * decay },
            if mp_r > old_r { mp_r } else { old_r * decay },
        );
    }

    pub fn reset_all(&mut self) {
        let clip_tx = &self.clip_tx;
        for track in &mut self.tracks {
            if let Some(ref mut inst) = track.instrument {
                inst.reset();
            }
            track.handle.vu.set(0.0, 0.0);
            // Commit any active recording before resetting (don't lose overdubs)
            if track.record_buf.is_active() && track.was_recording {
                let end_tick = track.last_record_tick.max(0);
                commit_recording(track, end_tick, clip_tx);
            } else if track.record_buf.is_active() {
                track.record_buf.discard();
            }
            track.was_recording = false;
            track.last_playback_tick = -1;
        }
        self.metronome.reset();
        self.limiter.reset();
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
            MixerCommand::AddTrack { kind: _, handle } => {
                let track = AudioTrack::new(handle, self.max_buffer_size);
                self.tracks.push(track);
            }
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
                        clip.events.sort_by_key(|e| e.tick);
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
        }
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
}
