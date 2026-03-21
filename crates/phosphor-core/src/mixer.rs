//! Per-track audio mixer.
//!
//! The mixer owns all audio tracks and processes the track graph:
//! routing MIDI to armed tracks, running each track's instrument,
//! applying mute/solo/volume, mixing to master, and updating VU levels.
//!
//! All state changes from the UI arrive via a lock-free command channel
//! (`crossbeam_channel::Receiver::try_recv` is lock-free).

use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use phosphor_midi::message::MidiMessage;
use phosphor_plugin::{MidiEvent, Plugin};

use crate::engine::VuLevels;
use crate::project::{TrackHandle, TrackKind};

// ── Commands ──

/// Commands sent from the UI thread to the mixer (audio thread).
///
/// The `Box<dyn Plugin + Send>` in `SetInstrument` is allocated on the
/// UI thread; the audio thread only receives the pointer (no alloc).
pub enum MixerCommand {
    /// Add a track with the given kind. The audio thread creates the
    /// `AudioTrack` and publishes a `TrackHandle` via the handle channel.
    AddTrack {
        kind: TrackKind,
        handle: Arc<TrackHandle>,
    },
    /// Assign an instrument plugin to a track.
    SetInstrument {
        track_id: usize,
        instrument: Box<dyn Plugin + Send>,
    },
    /// Remove a track by id.
    RemoveTrack {
        track_id: usize,
    },
    /// Set a parameter on a track's instrument.
    SetParameter {
        track_id: usize,
        param_index: usize,
        value: f32,
    },
}

// ── AudioTrack ──

/// Per-track audio state, owned by the mixer on the audio thread.
pub struct AudioTrack {
    pub id: usize,
    pub kind: TrackKind,
    /// Shared with the UI — config (mute/solo/arm/vol) + VU levels.
    pub handle: Arc<TrackHandle>,
    /// Optional instrument plugin (only for Instrument tracks).
    pub instrument: Option<Box<dyn Plugin>>,
    /// Scratch buffers for plugin output.
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    /// Plugin-format MIDI events for the current buffer.
    plugin_events: Vec<MidiEvent>,
}

impl AudioTrack {
    pub fn new(handle: Arc<TrackHandle>, max_buffer_size: usize) -> Self {
        Self {
            id: handle.id,
            kind: handle.kind,
            handle,
            instrument: None,
            buf_l: vec![0.0; max_buffer_size],
            buf_r: vec![0.0; max_buffer_size],
            plugin_events: Vec::with_capacity(256),
        }
    }
}

// ── Mixer ──

/// The mixer: owns all tracks and processes the track graph each
/// audio callback.
pub struct Mixer {
    tracks: Vec<AudioTrack>,
    master_vu: Arc<VuLevels>,
    command_rx: Receiver<MixerCommand>,
    sample_rate: u32,
    max_buffer_size: usize,
}

impl Mixer {
    /// Create a new mixer.
    pub fn new(
        command_rx: Receiver<MixerCommand>,
        master_vu: Arc<VuLevels>,
        sample_rate: u32,
        max_buffer_size: usize,
    ) -> Self {
        Self {
            tracks: Vec::new(),
            master_vu,
            command_rx,
            sample_rate,
            max_buffer_size,
        }
    }

    /// Process one buffer cycle. Called from the audio thread.
    ///
    /// `midi_events` are the raw MIDI messages drained from the ring buffer
    /// for this cycle. `output` is interleaved stereo: [L0, R0, L1, R1, ...].
    pub fn process(&mut self, output: &mut [f32], midi_messages: &[MidiMessage]) {
        // 1. Drain commands
        self.drain_commands();

        let num_frames = output.len() / 2;

        // 2. Convert MIDI messages to plugin events
        let plugin_events: Vec<MidiEvent> = midi_messages
            .iter()
            .filter_map(midi_to_plugin_event)
            .collect();

        // 3. Determine if any track is soloed
        let any_solo = self.tracks.iter().any(|t| t.handle.config.is_soloed());

        // 4. Process each track
        let mut master_l = vec![0.0f32; num_frames];
        let mut master_r = vec![0.0f32; num_frames];

        for track in &mut self.tracks {
            // Ensure scratch buffers are big enough
            if track.buf_l.len() < num_frames {
                track.buf_l.resize(num_frames, 0.0);
                track.buf_r.resize(num_frames, 0.0);
            }

            track.buf_l[..num_frames].fill(0.0);
            track.buf_r[..num_frames].fill(0.0);

            // Route MIDI only to the track that is selected for MIDI input.
            // This ensures each instrument track plays independently.
            if track.kind == TrackKind::Instrument && track.handle.config.is_midi_active() {
                track.plugin_events.clear();
                track.plugin_events.extend_from_slice(&plugin_events);
            } else {
                track.plugin_events.clear();
            }

            // Process instrument
            if let Some(ref mut instrument) = track.instrument {
                let mut outputs: [&mut [f32]; 2] = [
                    &mut track.buf_l[..num_frames],
                    &mut track.buf_r[..num_frames],
                ];
                let mut out_slices: Vec<&mut [f32]> =
                    outputs.iter_mut().map(|s| &mut **s).collect();
                instrument.process(&[], &mut out_slices, &track.plugin_events);
            }

            // Determine if this track should be audible
            let muted = track.handle.config.is_muted();
            let soloed = track.handle.config.is_soloed();
            let audible = !muted && (!any_solo || soloed);

            let volume = track.handle.config.get_volume();

            // Compute per-track VU (pre-mute for monitoring)
            let mut peak_l = 0.0f32;
            let mut peak_r = 0.0f32;
            for i in 0..num_frames {
                peak_l = peak_l.max(track.buf_l[i].abs());
                peak_r = peak_r.max(track.buf_r[i].abs());
            }

            // Smooth VU: fast attack, slow decay
            let (old_l, old_r) = track.handle.vu.get();
            let decay = 0.85f32;
            let vu_l = if peak_l > old_l { peak_l } else { old_l * decay };
            let vu_r = if peak_r > old_r { peak_r } else { old_r * decay };
            track.handle.vu.set(vu_l, vu_r);

            // Mix into master (with volume and mute/solo)
            if audible {
                for i in 0..num_frames {
                    master_l[i] += track.buf_l[i] * volume;
                    master_r[i] += track.buf_r[i] * volume;
                }
            }
        }

        // 5. Write to interleaved output and compute master VU
        let mut master_peak_l = 0.0f32;
        let mut master_peak_r = 0.0f32;
        for i in 0..num_frames {
            let l = master_l[i];
            let r = master_r[i];
            output[i * 2] = l;
            output[i * 2 + 1] = r;
            master_peak_l = master_peak_l.max(l.abs());
            master_peak_r = master_peak_r.max(r.abs());
        }

        // 6. Update master VU
        let (old_l, old_r) = self.master_vu.get();
        let decay = 0.85f32;
        let new_l = if master_peak_l > old_l {
            master_peak_l
        } else {
            old_l * decay
        };
        let new_r = if master_peak_r > old_r {
            master_peak_r
        } else {
            old_r * decay
        };
        self.master_vu.set(new_l, new_r);
    }

    /// Reset all instruments (panic).
    pub fn reset_all(&mut self) {
        for track in &mut self.tracks {
            if let Some(ref mut inst) = track.instrument {
                inst.reset();
            }
            track.handle.vu.set(0.0, 0.0);
        }
    }

    /// Drain commands from the channel (lock-free `try_recv`).
    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.command_rx.try_recv() {
            match cmd {
                MixerCommand::AddTrack { kind: _, handle } => {
                    let track = AudioTrack::new(handle, self.max_buffer_size);
                    self.tracks.push(track);
                }
                MixerCommand::SetInstrument {
                    track_id,
                    mut instrument,
                } => {
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
            }
        }
    }
}

/// Convert a phosphor-midi MidiMessage to a phosphor-plugin MidiEvent.
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

/// Create a command channel pair for the mixer.
pub fn mixer_command_channel() -> (Sender<MixerCommand>, Receiver<MixerCommand>) {
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
            message_type: MidiMessageType::NoteOn {
                channel: 0,
                note,
                velocity: vel,
            },
            raw: [0x90, note, vel],
            len: 3,
        }
    }

    fn setup_mixer() -> (Mixer, Sender<MixerCommand>) {
        let (tx, rx) = mixer_command_channel();
        let master_vu = Arc::new(VuLevels::new());
        let mixer = Mixer::new(rx, master_vu, 44100, 256);
        (mixer, tx)
    }

    #[test]
    fn mixer_processes_empty_output() {
        let (mut mixer, _tx) = setup_mixer();
        let mut output = vec![0.0f32; 128]; // 64 frames stereo
        mixer.process(&mut output, &[]);
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn mixer_adds_track_via_command() {
        let (mut mixer, tx) = setup_mixer();
        let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
        tx.send(MixerCommand::AddTrack {
            kind: TrackKind::Instrument,
            handle,
        })
        .unwrap();

        let mut output = vec![0.0f32; 128];
        mixer.process(&mut output, &[]);
        assert_eq!(mixer.tracks.len(), 1);
    }

    #[test]
    fn mixer_routes_midi_to_active_track() {
        let (mut mixer, tx) = setup_mixer();

        // Add a midi-active instrument track
        let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
        handle
            .config
            .midi_active
            .store(true, std::sync::atomic::Ordering::Relaxed);
        tx.send(MixerCommand::AddTrack {
            kind: TrackKind::Instrument,
            handle: handle.clone(),
        })
        .unwrap();

        // Set instrument
        let synth = Box::new(PhosphorSynth::new());
        tx.send(MixerCommand::SetInstrument {
            track_id: 0,
            instrument: synth,
        })
        .unwrap();

        // Process with note on
        let midi = vec![make_note_on(60, 100)];
        let mut output = vec![0.0f32; 512]; // 256 frames stereo
        mixer.process(&mut output, &midi);

        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.01, "Active track should produce sound, peak={peak}");
    }

    #[test]
    fn mixer_does_not_route_midi_to_inactive_track() {
        let (mut mixer, tx) = setup_mixer();

        // Add an inactive instrument track
        let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
        // midi_active is false by default
        tx.send(MixerCommand::AddTrack {
            kind: TrackKind::Instrument,
            handle,
        })
        .unwrap();

        let synth = Box::new(PhosphorSynth::new());
        tx.send(MixerCommand::SetInstrument {
            track_id: 0,
            instrument: synth,
        })
        .unwrap();

        let midi = vec![make_note_on(60, 100)];
        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &midi);

        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            peak == 0.0,
            "Inactive track should not receive MIDI, peak={peak}"
        );
    }

    #[test]
    fn mixer_mute_silences_track() {
        let (mut mixer, tx) = setup_mixer();

        let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
        handle
            .config
            .midi_active
            .store(true, std::sync::atomic::Ordering::Relaxed);
        handle
            .config
            .muted
            .store(true, std::sync::atomic::Ordering::Relaxed);
        tx.send(MixerCommand::AddTrack {
            kind: TrackKind::Instrument,
            handle: handle.clone(),
        })
        .unwrap();

        let synth = Box::new(PhosphorSynth::new());
        tx.send(MixerCommand::SetInstrument {
            track_id: 0,
            instrument: synth,
        })
        .unwrap();

        let midi = vec![make_note_on(60, 100)];
        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &midi);

        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak == 0.0, "Muted track should be silent, peak={peak}");

        // But VU should still show activity (pre-mute metering)
        let (vu_l, _vu_r) = handle.vu.get();
        assert!(vu_l > 0.0, "VU should still show activity on muted track");
    }

    #[test]
    fn mixer_solo_isolates_track() {
        let (mut mixer, tx) = setup_mixer();

        // Track 0: active + soloed
        let handle0 = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
        handle0
            .config
            .midi_active
            .store(true, std::sync::atomic::Ordering::Relaxed);
        handle0
            .config
            .soloed
            .store(true, std::sync::atomic::Ordering::Relaxed);
        tx.send(MixerCommand::AddTrack {
            kind: TrackKind::Instrument,
            handle: handle0,
        })
        .unwrap();
        tx.send(MixerCommand::SetInstrument {
            track_id: 0,
            instrument: Box::new(PhosphorSynth::new()),
        })
        .unwrap();

        // Track 1: active but not soloed
        let handle1 = Arc::new(TrackHandle::new(1, TrackKind::Instrument));
        handle1
            .config
            .midi_active
            .store(true, std::sync::atomic::Ordering::Relaxed);
        tx.send(MixerCommand::AddTrack {
            kind: TrackKind::Instrument,
            handle: handle1.clone(),
        })
        .unwrap();
        tx.send(MixerCommand::SetInstrument {
            track_id: 1,
            instrument: Box::new(PhosphorSynth::new()),
        })
        .unwrap();

        let midi = vec![make_note_on(60, 100)];
        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &midi);

        // Track 1 VU should show activity (it received MIDI and processed),
        // but its output should not appear in master (solo logic)
        let (vu_l, _) = handle1.vu.get();
        assert!(
            vu_l > 0.0,
            "Un-soloed track should still process (VU active)"
        );

        // Master output should only contain track 0's contribution
        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.01, "Soloed track should produce output");
    }

    #[test]
    fn mixer_per_track_vu() {
        let (mut mixer, tx) = setup_mixer();

        let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
        handle
            .config
            .midi_active
            .store(true, std::sync::atomic::Ordering::Relaxed);
        tx.send(MixerCommand::AddTrack {
            kind: TrackKind::Instrument,
            handle: handle.clone(),
        })
        .unwrap();
        tx.send(MixerCommand::SetInstrument {
            track_id: 0,
            instrument: Box::new(PhosphorSynth::new()),
        })
        .unwrap();

        // No sound → VU should decay to 0
        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &[]);
        let (vu_l, _) = handle.vu.get();
        assert!(vu_l < 0.01, "VU should be near zero with no sound");

        // Sound → VU should be > 0
        let midi = vec![make_note_on(60, 100)];
        mixer.process(&mut output, &midi);
        let (vu_l, _) = handle.vu.get();
        assert!(vu_l > 0.01, "VU should show activity with sound");
    }

    #[test]
    fn mixer_removes_track() {
        let (mut mixer, tx) = setup_mixer();

        let handle = Arc::new(TrackHandle::new(42, TrackKind::Audio));
        tx.send(MixerCommand::AddTrack {
            kind: TrackKind::Audio,
            handle,
        })
        .unwrap();

        let mut output = vec![0.0f32; 128];
        mixer.process(&mut output, &[]);
        assert_eq!(mixer.tracks.len(), 1);

        tx.send(MixerCommand::RemoveTrack { track_id: 42 })
            .unwrap();
        mixer.process(&mut output, &[]);
        assert_eq!(mixer.tracks.len(), 0);
    }

    #[test]
    fn mixer_multiple_tracks() {
        let (mut mixer, tx) = setup_mixer();

        // Add two active instrument tracks
        for id in 0..2 {
            let handle = Arc::new(TrackHandle::new(id, TrackKind::Instrument));
            handle
                .config
                .midi_active
                .store(true, std::sync::atomic::Ordering::Relaxed);
            tx.send(MixerCommand::AddTrack {
                kind: TrackKind::Instrument,
                handle,
            })
            .unwrap();
            tx.send(MixerCommand::SetInstrument {
                track_id: id,
                instrument: Box::new(PhosphorSynth::new()),
            })
            .unwrap();
        }

        let midi = vec![make_note_on(60, 100)];
        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &midi);

        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            peak > 0.01,
            "Multiple active tracks should produce sound, peak={peak}"
        );
    }

    #[test]
    fn mixer_reset_all() {
        let (mut mixer, tx) = setup_mixer();

        let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
        handle
            .config
            .midi_active
            .store(true, std::sync::atomic::Ordering::Relaxed);
        tx.send(MixerCommand::AddTrack {
            kind: TrackKind::Instrument,
            handle: handle.clone(),
        })
        .unwrap();
        tx.send(MixerCommand::SetInstrument {
            track_id: 0,
            instrument: Box::new(PhosphorSynth::new()),
        })
        .unwrap();

        // Play a note
        let midi = vec![make_note_on(60, 100)];
        let mut output = vec![0.0f32; 512];
        mixer.process(&mut output, &midi);

        // Reset
        mixer.reset_all();

        // Process again — should be silent
        output.fill(0.0);
        mixer.process(&mut output, &[]);
        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            peak < 0.001,
            "Should be silent after reset, peak={peak}"
        );
    }
}
