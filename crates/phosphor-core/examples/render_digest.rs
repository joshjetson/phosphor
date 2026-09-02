//! Renders fixed scenarios through the mixer and prints a digest of every
//! sample, so that two builds can be compared bit for bit.
//!
//! `cargo run -p phosphor-core --example render_digest`
//!
//! Not part of the product. It exists so that "the mixer was rebuilt and the
//! audio did not change" can be *checked* rather than asserted: copy this
//! file into a worktree of the previous release, run it there, run it here,
//! and diff the two outputs. Anything that differs is a change in what the
//! application sounds like.
//!
//! **It deliberately uses only the mixer API that existed before the insert
//! layer** — `AddTrack`, `SetInstrument`, `SetParameter`, `CreateClip`,
//! `UpdateClip`, `SetPattern` — so that the same file compiles in an older
//! worktree. The insert layer's own null tests are unit tests in `mixer.rs`;
//! this is the half that no in-tree test can do, because it needs the other
//! build.
//!
//! Everything here is deterministic: fixed sample rate, fixed block size, no
//! clock, no device, no randomness. The digest is FNV-1a over the raw bits of
//! every sample, so two runs agree only if every sample agrees.

use std::sync::Arc;

use phosphor_core::clip::ClipEvent;
use phosphor_core::engine::VuLevels;
use phosphor_core::mixer::{clip_snapshot_channel, mixer_command_channel, Mixer, MixerCommand};
use phosphor_core::pattern::{Lane, PatternBlock};
use phosphor_core::project::{TrackHandle, TrackKind};
use phosphor_core::transport::Transport;
use phosphor_midi::message::{MidiMessage, MidiMessageType};
use phosphor_plugin::Plugin;

const SAMPLE_RATE: u32 = 44_100;
const FRAMES: usize = 256;

/// FNV-1a over the bits of every sample.
fn digest(samples: &[f32]) -> u64 {
    samples.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, s| {
        s.to_bits().to_le_bytes().iter().fold(h, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    })
}

fn peaks(samples: &[f32]) -> (f32, f32) {
    let mut l = 0.0f32;
    let mut r = 0.0f32;
    for frame in samples.chunks_exact(2) {
        l = l.max(frame[0].abs());
        r = r.max(frame[1].abs());
    }
    (l, r)
}

fn note_on(note: u8, velocity: u8) -> MidiMessage {
    MidiMessage {
        received_micros: None,
        message_type: MidiMessageType::NoteOn { channel: 0, note, velocity },
        raw: [0x90, note, velocity],
        len: 3,
    }
}

struct Rig {
    mixer: Mixer,
    tx: crossbeam_channel::Sender<MixerCommand>,
    transport: Arc<Transport>,
    // Kept alive: the mixer sends recording snapshots down it.
    _clip_rx: crossbeam_channel::Receiver<phosphor_core::clip::ClipSnapshot>,
    handles: Vec<Arc<TrackHandle>>,
}

impl Rig {
    fn new() -> Self {
        let (tx, rx) = mixer_command_channel();
        let (clip_tx, clip_rx) = clip_snapshot_channel();
        Self {
            mixer: Mixer::new(rx, Arc::new(VuLevels::new()), clip_tx, SAMPLE_RATE, FRAMES),
            tx,
            transport: Arc::new(Transport::new(120.0)),
            _clip_rx: clip_rx,
            handles: Vec::new(),
        }
    }

    /// A track with an instrument on it, at a stated fader position.
    fn track(&mut self, id: usize, instrument: Box<dyn Plugin + Send>, volume: f32) -> Arc<TrackHandle> {
        let handle = Arc::new(TrackHandle::new(id, TrackKind::Instrument));
        handle
            .config
            .midi_active
            .store(true, std::sync::atomic::Ordering::Relaxed);
        handle.config.set_volume(volume);
        self.tx
            .send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle: handle.clone() })
            .unwrap();
        self.tx
            .send(MixerCommand::SetInstrument { track_id: id, instrument })
            .unwrap();
        self.handles.push(handle.clone());
        handle
    }

    /// Render `blocks` callbacks, sending `midi` on the first one.
    fn render(&mut self, blocks: usize, midi: &[MidiMessage]) -> Vec<f32> {
        let mut out = vec![0.0f32; FRAMES * 2];
        let mut all = Vec::with_capacity(FRAMES * 2 * blocks);
        for block in 0..blocks {
            out.fill(0.0);
            let events: &[MidiMessage] = if block == 0 { midi } else { &[] };
            self.mixer.process(&mut out, events, &self.transport);
            all.extend_from_slice(&out);
            self.transport.advance(FRAMES as u32, SAMPLE_RATE);
        }
        all
    }
}

fn report(name: &str, samples: &[f32]) {
    let (l, r) = peaks(samples);
    println!(
        "{name:<22} n={:<7} digest={:016x} peak={l:.6}/{r:.6}",
        samples.len(),
        digest(samples)
    );
}

fn main() {
    println!("== render digest ==");
    println!("rate {SAMPLE_RATE} block {FRAMES}");

    // One synth, a triad, played and held.
    {
        let mut rig = Rig::new();
        rig.track(0, Box::new(phosphor_dsp::synth::PhosphorSynth::new()), 0.75);
        rig.transport.play();
        let chord: Vec<MidiMessage> = [60u8, 64, 67].iter().map(|&n| note_on(n, 100)).collect();
        let out = rig.render(120, &chord);
        report("synth-triad", &out);
    }

    // Two tracks at different fader positions, one of them soloed — the
    // audible/solo path, and the fader.
    {
        let mut rig = Rig::new();
        let soloed = rig.track(0, Box::new(phosphor_dsp::synth::PhosphorSynth::new()), 1.0);
        rig.track(1, Box::new(phosphor_dsp::dx7::Dx7Synth::new()), 0.5);
        soloed
            .config
            .soloed
            .store(true, std::sync::atomic::Ordering::Relaxed);
        rig.transport.play();
        let chord: Vec<MidiMessage> = [48u8, 55, 60].iter().map(|&n| note_on(n, 110)).collect();
        let out = rig.render(120, &chord);
        report("solo-and-fader", &out);
    }

    // Four loud DX7 tracks: the master limiter working.
    {
        let mut rig = Rig::new();
        for id in 0..4 {
            let mut synth = phosphor_dsp::dx7::Dx7Synth::new();
            let (bank, patch) = phosphor_dsp::dx7::voice_knobs(147); // TIMPANI
            synth.set_parameter(phosphor_dsp::dx7::P_BANK, bank);
            synth.set_parameter(phosphor_dsp::dx7::P_PATCH, patch);
            rig.track(id, Box::new(synth), 1.0);
        }
        rig.transport.play();
        let chord: Vec<MidiMessage> = [36u8, 43, 48, 55, 60, 64, 67, 72]
            .iter()
            .map(|&n| note_on(n, 127))
            .collect();
        let out = rig.render(120, &chord);
        report("limiter-working", &out);
    }

    // A clip playing back, with the metronome on.
    {
        let mut rig = Rig::new();
        rig.track(0, Box::new(phosphor_dsp::synth::PhosphorSynth::new()), 0.75);
        rig.tx
            .send(MixerCommand::CreateClip { track_id: 0, start_tick: 0, length_ticks: 3840 })
            .unwrap();
        rig.tx
            .send(MixerCommand::UpdateClip {
                track_id: 0,
                clip_index: 0,
                events: (0..8)
                    .flat_map(|i| {
                        [
                            ClipEvent { tick: i * 480, status: 0x90, data1: 60 + (i as u8 % 5) * 2, data2: 96 },
                            ClipEvent { tick: i * 480 + 360, status: 0x80, data1: 60 + (i as u8 % 5) * 2, data2: 0 },
                        ]
                    })
                    .collect(),
            })
            .unwrap();
        rig.transport.toggle_metronome();
        rig.transport.play();
        let out = rig.render(300, &[]);
        report("clip-and-metronome", &out);
    }

    // A pattern driving a drum rack.
    {
        let mut rig = Rig::new();
        rig.track(0, Box::new(phosphor_dsp::drum_rack::DrumRack::new()), 0.75);
        let mut block = PatternBlock::empty();
        block.playing = true;
        block.lanes[0] = Lane::drum(36);
        block.lanes[1] = Lane::drum(38);
        for step in [0usize, 4, 8, 12] {
            block.lanes[0].steps[step].on = true;
        }
        for step in [4usize, 12] {
            block.lanes[1].steps[step].on = true;
        }
        rig.tx
            .send(MixerCommand::SetPattern { track_id: 0, slot: 0, block })
            .unwrap();
        rig.transport.play();
        let out = rig.render(300, &[]);
        report("pattern-drums", &out);
    }
}
