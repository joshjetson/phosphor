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
            tracks: Vec::new(),
            master_vu,
            command_rx,
            clip_tx,
            metronome: Metronome::new(sample_rate as f64),
            sample_rate,
            max_buffer_size,
            scratch_l: vec![0.0; max_buffer_size],
            scratch_r: vec![0.0; max_buffer_size],
            live_events: Vec::with_capacity(256),
        }
    }

    /// Process one buffer cycle.
    pub fn process(&mut self, output: &mut [f32], midi_messages: &[MidiMessage], transport: &Transport) {
        self.drain_commands();

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

                // Check if we just wrapped (position jumped backward since last buffer)
                // If so, we need to play events from the loop start that we skipped
                let just_wrapped = looping && track.last_record_tick >= 0
                    && current_tick < track.last_record_tick;

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

        // Master VU (includes metronome)
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
        }
        self.metronome.reset();
    }

    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.command_rx.try_recv() {
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

fn midi_to_plugin_event(msg: &MidiMessage) -> Option<MidiEvent> {
    use phosphor_midi::message::MidiMessageType;
    match msg.message_type {
        MidiMessageType::NoteOn { .. }
        | MidiMessageType::NoteOff { .. }
        | MidiMessageType::ControlChange { .. }
        | MidiMessageType::PitchBend { .. } => Some(MidiEvent {
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
        assert!(peak > 0.01, "Should produce sound, peak={peak}");
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
        assert!(peak > 0.01, "Playback should produce sound, peak={peak}");
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
        assert!(peak_during > 0.01, "Should hear note during recording (monitoring)");
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
}
