//! A busy song through the real audio engine, for measuring and checking it.
//!
//! Every instrument plays — live chords, a clip, a step pattern — through
//! MIDI effects, insert chains with a sidechain key, both send buses and the
//! master chain, while tracks are panned, muted, soloed, keyed and bypassed
//! mid-flight and a loop recording commits takes. It exercises everything the
//! engine does in one callback, so it is what the engine's equivalence test
//! renders (`tests/engine_render.rs`) and what the CPU budget tool times
//! (`examples/cpu_budget.rs`). The script repeats every [`SCRIPT_BLOCKS`]
//! blocks, so it can be played for as long as a measurement needs.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crossbeam_channel::Receiver;
use phosphor_core::clip::{ClipEvent, ClipSnapshot};
use phosphor_core::engine::VuLevels;
use phosphor_core::fx::{FxTarget, SendSlot};
use phosphor_core::midi_fx::{Arpeggiator, ChordDevice};
use phosphor_core::mixer::{clip_snapshot_channel, mixer_command_channel, Mixer, MixerCommand};
use phosphor_core::pattern::{Lane, PatternBlock};
use phosphor_core::project::{TrackHandle, TrackKind};
use phosphor_core::transport::Transport;
use phosphor_midi::MidiMessage;

use crate::instrument::build_plugin;
use crate::state::{FxType, InstrumentType};

pub const RATE: u32 = 48_000;
pub const FRAMES: usize = 128;
/// How long the script runs before it repeats: 1.6 s.
pub const SCRIPT_BLOCKS: usize = 600;

/// The song, mid-performance.
pub struct BusySong {
    mixer: Mixer,
    transport: Transport,
    commands: crossbeam_channel::Sender<MixerCommand>,
    takes: Receiver<ClipSnapshot>,
    master_vu: Arc<VuLevels>,
    /// The instrument tracks' handles, in track order.
    handles: Vec<Arc<TrackHandle>>,
    out: Vec<f32>,
    block: usize,
}

fn note(on: bool, note: u8, velocity: u8) -> MidiMessage {
    MidiMessage::from_bytes(&[if on { 0x90 } else { 0x80 }, note, velocity]).expect("three bytes")
}

fn effect(fx: FxType) -> Box<dyn phosphor_core::fx::Effect> {
    crate::fx::build(fx).expect("every menu effect builds")
}

const CHORD: [u8; 5] = [48, 55, 60, 64, 67];

impl BusySong {
    /// The song, on a mixer `configure` has had its say over (its thread
    /// count, say).
    pub fn new(configure: impl FnOnce(&mut Mixer)) -> Self {
        let (commands, rx) = mixer_command_channel();
        let (clip_tx, takes) = clip_snapshot_channel();
        let master_vu = Arc::new(VuLevels::new());
        let mut mixer = Mixer::new(rx, master_vu.clone(), clip_tx, RATE, FRAMES);
        configure(&mut mixer);
        let transport = Transport::new(120.0);
        // A one-beat loop, so the recorder commits a take every half second.
        transport.set_loop_range(0, Transport::PPQ);
        transport.toggle_loop();
        transport.toggle_record();
        transport.play();

        let send = |cmd: MixerCommand| commands.send(cmd).expect("the mixer holds the receiver");
        let kinds: Vec<InstrumentType> =
            InstrumentType::ALL.iter().copied().filter(|i| !i.is_sequencer()).collect();
        let mut handles = Vec::new();
        for (id, &kind) in kinds.iter().enumerate() {
            let handle = Arc::new(TrackHandle::new(id, TrackKind::Instrument));
            handle.config.midi_active.store(true, Ordering::Relaxed);
            handle.config.armed.store(id % 3 == 0, Ordering::Relaxed);
            handle.config.set_volume(0.6 + 0.04 * id as f32);
            send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle: handle.clone() });
            send(MixerCommand::SetInstrument { track_id: id, instrument: build_plugin(kind) });
            send(MixerCommand::SetPan { track_id: id, pan: (id as f32 / 5.0) - 1.0 });
            send(MixerCommand::AddFx { target: FxTarget::Track(id), slot: 0, effect: effect(FxType::Compressor) });
            let extra = match id % 5 {
                0 => Some(FxType::Eq),
                1 => Some(FxType::Delay),
                2 => Some(FxType::Tape),
                3 => Some(FxType::Reverb),
                _ => None,
            };
            if let Some(fx) = extra {
                send(MixerCommand::AddFx { target: FxTarget::Track(id), slot: 1, effect: effect(fx) });
            }
            send(MixerCommand::SetSendLevel { track_id: id, send: SendSlot::A, gain: if id % 2 == 0 { 0.5 } else { 0.0 } });
            send(MixerCommand::SetSendLevel { track_id: id, send: SendSlot::B, gain: if id % 3 == 0 { 0.3 } else { 0.0 } });
            handles.push(handle);
        }

        // A sidechain: track 4's compressor keys off track 0.
        send(MixerCommand::SetKeySource { track_id: 4, source: Some(0) });
        // MIDI effects on two tracks.
        send(MixerCommand::AddMidiFx { track_id: 2, slot: 0, fx: Box::new(Arpeggiator::new()) });
        send(MixerCommand::AddMidiFx { track_id: 5, slot: 0, fx: Box::new(ChordDevice::new()) });
        // A clip on track 3, and a kick pattern on the drum rack.
        send(MixerCommand::CreateClip { track_id: 3, start_tick: 0, length_ticks: Transport::PPQ });
        send(MixerCommand::UpdateClip {
            track_id: 3,
            clip_index: 0,
            events: vec![
                ClipEvent { tick: 0, status: 0x90, data1: 64, data2: 90 },
                ClipEvent { tick: Transport::PPQ / 2, status: 0x80, data1: 64, data2: 0 },
                ClipEvent { tick: Transport::PPQ / 2, status: 0x90, data1: 67, data2: 70 },
                ClipEvent { tick: Transport::PPQ - 10, status: 0x80, data1: 67, data2: 0 },
            ],
        });
        let drum = kinds.iter().position(|k| *k == InstrumentType::DrumRack).expect("a drum rack");
        let mut kick = PatternBlock::empty();
        kick.playing = true;
        kick.lanes[0] = Lane::drum(36);
        for step in [0, 4, 8, 12] {
            kick.lanes[0].steps[step].on = true;
        }
        send(MixerCommand::SetPattern { track_id: drum, slot: 0, block: kick });

        // The buses and the master, each with a chain.
        for kind in [TrackKind::SendA, TrackKind::SendB, TrackKind::Master] {
            let handle = Arc::new(TrackHandle::new(usize::MAX, kind));
            handle.config.set_volume(0.8);
            send(MixerCommand::AddTrack { kind, handle });
        }
        send(MixerCommand::AddFx { target: FxTarget::BusA, slot: 0, effect: effect(FxType::Reverb) });
        send(MixerCommand::AddFx { target: FxTarget::BusB, slot: 0, effect: effect(FxType::Delay) });
        send(MixerCommand::AddFx { target: FxTarget::Master, slot: 0, effect: effect(FxType::Eq) });
        send(MixerCommand::AddFx { target: FxTarget::Master, slot: 1, effect: effect(FxType::Compressor) });

        Self { mixer, transport, commands, takes, master_vu, handles, out: vec![0.0; FRAMES * 2], block: 0 }
    }

    /// What the script does to the song before block `block` renders, and
    /// the live MIDI that arrives with it.
    fn script(&self, block: usize) -> Vec<MidiMessage> {
        let send = |cmd: MixerCommand| self.commands.send(cmd).expect("the mixer holds the receiver");
        match block {
            100 => send(MixerCommand::SetKeyListen { track: Some(4) }),
            120 => send(MixerCommand::SetFxBypass { target: FxTarget::Track(0), slot: 1, bypass: true }),
            200 => send(MixerCommand::SetKeyListen { track: None }),
            250 => send(MixerCommand::SetFxBypass { target: FxTarget::BusA, slot: 0, bypass: true }),
            260 => self.handles[8].config.muted.store(true, Ordering::Relaxed),
            300 => self.handles[9].config.soloed.store(true, Ordering::Relaxed),
            350 => self.handles[9].config.soloed.store(false, Ordering::Relaxed),
            400 => send(MixerCommand::SetFxBypass { target: FxTarget::BusA, slot: 0, bypass: false }),
            450 => send(MixerCommand::SetParameter { track_id: 1, param_index: 1, value: 0.3 }),
            _ => {}
        }
        match block {
            0 | 220 | 430 => CHORD.iter().map(|&n| note(true, n, 100)).collect(),
            150 | 380 => CHORD.iter().map(|&n| note(false, n, 0)).collect(),
            300 => vec![MidiMessage::from_bytes(&[0xE0, 0x00, 0x60]).expect("pitch bend")],
            _ => Vec::new(),
        }
    }

    /// Render the next block, and return its interleaved output.
    pub fn next_block(&mut self) -> &[f32] {
        let midi = self.script(self.block % SCRIPT_BLOCKS);
        self.block += 1;
        self.out.fill(0.0);
        self.mixer.process(&mut self.out, &midi, &self.transport);
        self.transport.advance(FRAMES as u32, RATE);
        &self.out
    }

    /// Every instrument track's meter, then the master's, as they stand.
    pub fn meters(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        let read = |vu: &VuLevels| (vu.peak_l.load(Ordering::Relaxed), vu.peak_r.load(Ordering::Relaxed));
        self.handles.iter().map(move |h| read(&h.vu)).chain(std::iter::once(read(&self.master_vu)))
    }

    /// Stop, so the recorder commits what it holds, and return every take
    /// it sent, in order.
    pub fn finish(mut self) -> Vec<ClipSnapshot> {
        self.transport.stop();
        self.out.fill(0.0);
        self.mixer.process(&mut self.out, &[], &self.transport);
        self.takes.try_iter().collect()
    }
}
