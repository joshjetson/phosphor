use super::*;
use crate::cpal_backend::{Requested, StreamFormat};
use crate::project::TrackConfig;
use phosphor_dsp::synth::PhosphorSynth;
use phosphor_midi::message::{MidiMessage, MidiMessageType};

fn make_note_on(note: u8, vel: u8) -> MidiMessage {
    MidiMessage {
        received_micros: None,
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
        received_micros: None,
        message_type: MidiMessageType::ChannelPressure { channel: 0, pressure: 96 },
        raw: [0xD0, 96, 0],
        len: 2,
    };
    let event = midi_to_plugin_event(&pressure).expect("channel pressure is dropped");
    assert_eq!(event.status, 0xD0);
    assert_eq!(event.data1, 96);

    // Polyphonic key pressure parses as `Other` and stays there: nothing
    // in the rack has a per-key pressure destination.
    let key = MidiMessage::from_bytes(&[0xA0, 60, 96]).expect("parsed");
    assert!(
        midi_to_plugin_event(&key).is_none(),
        "polyphonic key pressure has no destination in the rack"
    );
}

fn make_note_off(note: u8) -> MidiMessage {
    MidiMessage {
        received_micros: None,
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

/// A whole bed delivered as one command arrives whole: every key it
/// names plays, phrases and layers together, and it costs one callback
/// rather than the forty-four the same edit used to take.
#[test]
fn a_whole_bed_of_pads_travels_as_one_command() {
    use phosphor_plugin::sample::{PadConfig, PadLayer, PadPhrase, PhraseEvent, SamplePcm};
    let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
    let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
    handle.config.midi_active.store(true, std::sync::atomic::Ordering::Relaxed);
    handle.config.armed.store(true, std::sync::atomic::Ordering::Relaxed);
    tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle }).unwrap();
    tx.send(MixerCommand::SetInstrument {
        track_id: 0,
        instrument: Box::new(phosphor_dsp::sampler::Sampler::new()),
    })
    .unwrap();
    tx.send(MixerCommand::SetSamplerChild {
        track_id: 0,
        child: Some(Box::new(PhosphorSynth::new())),
    })
    .unwrap();
    while !mixer.command_rx.is_empty() {
        mixer.drain_commands();
    }

    let data: Vec<f32> = (0..44_100)
        .map(|i| 0.5 * (std::f32::consts::TAU * 220.0 * i as f32 / 44_100.0).sin())
        .collect();
    let pcm = Arc::new(SamplePcm { data, channels: 1, sample_rate: 44_100.0 });
    let events: Arc<[PhraseEvent]> = Arc::from(vec![
        PhraseEvent { frame: 0, status: 0x90, data1: 64, data2: 100 },
        PhraseEvent { frame: 200_000, status: 0x80, data1: 64, data2: 0 },
    ]);
    // Eighty-eight pads, and eight more entries than the bed has keys —
    // the surplus must be shrugged off rather than walked.
    let pads: Vec<_> = (0..96u8)
        .map(|pad| {
            (
                pad,
                PadConfig::for_key(21u8.saturating_add(pad)),
                vec![PadLayer::from_pcm(Arc::clone(&pcm))],
                vec![PadPhrase::from_events(Arc::clone(&events), 200_001)],
            )
        })
        .collect();
    tx.send(MixerCommand::SetSamplerRange { track_id: 0, pads }).unwrap();
    // A range for a track that is not there is a shrug, as every other
    // sampler command's is.
    tx.send(MixerCommand::SetSamplerRange { track_id: 99, pads: Vec::new() }).unwrap();

    // The whole bed lands inside one callback's budget: three heavy
    // units for eighty-eight pads, against a budget of four.
    let spent = mixer.drain_commands();
    assert!(spent <= COMMAND_BUDGET, "a bed-wide range overran the budget: {spent}");
    assert!(mixer.command_rx.is_empty(), "the range was left half-applied");

    transport.play();
    // Both ends of the bed and the middle: a range that applied only its
    // first pad, or shifted its indices, is silent at two of the three.
    for note in [21u8, 60, 108] {
        let mut output = vec![0.0f32; 2_048];
        mixer.process(&mut output, &[make_note_on(note, 100)], &transport);
        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.01, "note {note} made no sound after a range delivery");
        mixer.process(&mut output, &[make_note_off(note)], &transport);
    }
}

/// A range is charged for what it carries, so one keypress cannot do a
/// bed's worth of work under a single pad's budget.
#[test]
fn a_range_costs_more_the_more_pads_it_carries() {
    use phosphor_plugin::sample::PadConfig;
    let range = |n: usize| MixerCommand::SetSamplerRange {
        track_id: 0,
        pads: (0..n)
            .map(|i| (i as u8, PadConfig::for_key(60), Vec::new(), Vec::new()))
            .collect(),
    };
    let one = MixerCommand::SetSamplerPad {
        track_id: 0,
        pad: 0,
        config: PadConfig::for_key(60),
        layers: Vec::new(),
    };
    assert_eq!(command_cost(&range(1)), HEAVY_COMMAND);
    assert_eq!(command_cost(&range(0)), HEAVY_COMMAND, "an empty range is still a command");
    assert_eq!(command_cost(&range(PADS_PER_HEAVY)), HEAVY_COMMAND);
    assert_eq!(command_cost(&range(PADS_PER_HEAVY + 1)), HEAVY_COMMAND * 2);
    assert!(command_cost(&range(MAX_RANGE_PADS)) > command_cost(&one));
    // And the bill is bounded however long the list is, because the work
    // is: a range never applies more pads than the bed has keys.
    assert_eq!(
        command_cost(&range(10_000)),
        command_cost(&range(MAX_RANGE_PADS)),
        "a hostile range could charge — and do — unbounded work",
    );
    assert!(
        command_cost(&range(MAX_RANGE_PADS)) <= COMMAND_BUDGET,
        "the widest honest range cannot fit in one callback",
    );
}

#[test]
fn a_sampler_pad_travels_to_the_engine_and_speaks() {
    use phosphor_plugin::sample::{PadConfig, PadLayer, SamplePcm};
    let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
    let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
    handle.config.midi_active.store(true, std::sync::atomic::Ordering::Relaxed);
    handle.config.armed.store(true, std::sync::atomic::Ordering::Relaxed);
    tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle: handle.clone() })
        .unwrap();
    tx.send(MixerCommand::SetInstrument {
        track_id: 0,
        instrument: Box::new(phosphor_dsp::sampler::Sampler::new()),
    })
    .unwrap();

    // A 220 Hz-ish sine on C3's pad, delivered the way the UI will.
    let data: Vec<f32> =
        (0..44_100).map(|i| (std::f32::consts::TAU * 220.0 * i as f32 / 44_100.0).sin()).collect();
    let pcm = Arc::new(SamplePcm { data, channels: 1, sample_rate: 44_100.0 });
    tx.send(MixerCommand::SetSamplerPad {
        track_id: 0,
        pad: 39, // note 60
        config: PadConfig::for_key(60),
        layers: vec![PadLayer::from_pcm(pcm)],
    })
    .unwrap();
    // A pad off the bed must be shrugged off, not panicked over.
    tx.send(MixerCommand::SetSamplerPad {
        track_id: 0,
        pad: 200,
        config: PadConfig::for_key(60),
        layers: Vec::new(),
    })
    .unwrap();

    transport.play();
    let midi = vec![make_note_on(60, 100)];
    let mut output = vec![0.0f32; 2_048];
    mixer.process(&mut output, &midi, &transport);
    let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(peak > 0.01, "the delivered pad made no sound, peak={peak}");
}

/// A phrase layer is a recording played through an instrument rather
/// than a buffer, so the proof that it arrived is that a synth the
/// sampler was handed makes a sound the sampler was never given audio
/// for.
#[test]
fn a_sampler_phrase_travels_to_the_engine_and_sounds_through_its_child() {
    use phosphor_plugin::sample::{PadConfig, PadPhrase, PhraseEvent};
    let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
    let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
    handle.config.midi_active.store(true, std::sync::atomic::Ordering::Relaxed);
    handle.config.armed.store(true, std::sync::atomic::Ordering::Relaxed);
    tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle: handle.clone() })
        .unwrap();
    tx.send(MixerCommand::SetInstrument {
        track_id: 0,
        instrument: Box::new(phosphor_dsp::sampler::Sampler::new()),
    })
    .unwrap();
    tx.send(MixerCommand::SetSamplerChild {
        track_id: 0,
        child: Some(Box::new(PhosphorSynth::new())),
    })
    .unwrap();

    // The pad carries no audio at all: a config with no layers, and a
    // phrase holding one long note.
    tx.send(MixerCommand::SetSamplerPad {
        track_id: 0,
        pad: 39, // note 60
        config: PadConfig::for_key(60),
        layers: Vec::new(),
    })
    .unwrap();
    let events: Arc<[PhraseEvent]> = Arc::from(vec![
        PhraseEvent { frame: 0, status: 0x90, data1: 64, data2: 100 },
        PhraseEvent { frame: 200_000, status: 0x80, data1: 64, data2: 0 },
    ]);
    tx.send(MixerCommand::SetSamplerPhrases {
        track_id: 0,
        pad: 39,
        phrases: vec![PadPhrase::from_events(events.clone(), 200_001)],
    })
    .unwrap();
    // A pad off the bed and a track that is not there are both shrugs.
    tx.send(MixerCommand::SetSamplerPhrases {
        track_id: 0,
        pad: 200,
        phrases: vec![PadPhrase::from_events(events, 200_001)],
    })
    .unwrap();
    tx.send(MixerCommand::SetSamplerPhrases { track_id: 99, pad: 0, phrases: Vec::new() })
        .unwrap();
    tx.send(MixerCommand::SetSamplerChild { track_id: 99, child: None }).unwrap();

    transport.play();
    // Eight allocating commands are three callbacks' worth of budget,
    // so the kit is let land before the key is pressed — which is what
    // happens in the application too, where the edits go down long
    // before anything is played.
    let mut output = vec![0.0f32; 256];
    for _ in 0..3 {
        mixer.process(&mut output, &[], &transport);
    }

    let midi = vec![make_note_on(60, 100)];
    let mut output = vec![0.0f32; 4_096];
    mixer.process(&mut output, &midi, &transport);
    // One more block, so the synth's attack has time to arrive.
    let mut output = vec![0.0f32; 4_096];
    mixer.process(&mut output, &[], &transport);
    let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(peak > 0.001, "the phrase never reached the child, peak={peak}");

    // And the pad off the bed stored nothing: its own key is silent.
    let mut output = vec![0.0f32; 1_024];
    mixer.process(&mut output, &[make_note_on(21, 100)], &transport);
    assert!(output.iter().all(|s| s.is_finite()));
}

/// The audition takes the same road as a pad and needs no note: the UI
/// asks, the sampler sounds, and `None` puts it back to silence.
#[test]
fn a_sampler_audition_travels_to_the_engine_and_sounds_without_a_note() {
    use phosphor_plugin::sample::{PadConfig, PadLayer, PreviewLayer, PreviewMode, SamplePcm};
    let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
    let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
    handle.config.midi_active.store(true, std::sync::atomic::Ordering::Relaxed);
    tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle: handle.clone() })
        .unwrap();
    tx.send(MixerCommand::SetInstrument {
        track_id: 0,
        instrument: Box::new(phosphor_dsp::sampler::Sampler::new()),
    })
    .unwrap();

    let data: Vec<f32> = (0..44_100)
        .map(|i| 0.5 * (std::f32::consts::TAU * 220.0 * i as f32 / 44_100.0).sin())
        .collect();
    let pcm = Arc::new(SamplePcm { data, channels: 1, sample_rate: 44_100.0 });
    // Nothing is on any pad: the audition carries its own layer, which
    // is what lets the trim strip sound a sound before it is committed.
    tx.send(MixerCommand::SetSamplerPreview {
        track_id: 0,
        preview: Some(PreviewLayer {
            config: PadConfig::for_key(60),
            layer: PadLayer::from_pcm(pcm),
            mode: PreviewMode::Loop,
        }),
    })
    .unwrap();

    transport.play();
    let mut output = vec![0.0f32; 2_048];
    mixer.process(&mut output, &[], &transport);
    let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(peak > 0.01, "the audition made no sound, peak={peak}");

    tx.send(MixerCommand::SetSamplerPreview { track_id: 0, preview: None }).unwrap();
    // A pad off the bed and a track that does not exist are both shrugs.
    tx.send(MixerCommand::SetSamplerPreview { track_id: 99, preview: None }).unwrap();
    let mut output = vec![0.0f32; 8_192];
    mixer.process(&mut output, &[], &transport);
    let tail = output[6_000..].iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(tail < 1e-3, "the audition kept going after off, tail={tail}");
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


/// The recorder taps the stream upstream of the MIDI effects: a take
/// made through an arpeggiator stores the chord the player held, not
/// the run the arp generated. Change the arp tomorrow and the take
/// plays through the new setting — the whole point of the layer.
#[test]
fn recording_through_an_arp_stores_the_played_keys() {
    let (mut mixer, tx, clip_rx, transport) = setup_mixer();
    let _handle = add_armed_synth(&tx, 0);
    tx.send(MixerCommand::AddMidiFx {
        track_id: 0,
        slot: 0,
        fx: Box::new(crate::midi_fx::Arpeggiator::new()),
    })
    .unwrap();
    transport.play();
    transport.toggle_record();

    let mut output = vec![0.0f32; 512];
    let chord = vec![make_note_on(60, 100), make_note_on(64, 100), make_note_on(67, 100)];
    mixer.process(&mut output, &chord, &transport);
    transport.advance(256, 44_100);
    for _ in 0..40 {
        mixer.process(&mut output, &[], &transport);
        transport.advance(256, 44_100);
    }
    let offs = vec![make_note_off(60), make_note_off(64), make_note_off(67)];
    mixer.process(&mut output, &offs, &transport);
    transport.toggle_record();
    mixer.process(&mut output, &[], &transport);

    let snap = clip_rx.try_recv().expect("commit produced no snapshot");
    assert_eq!(snap.notes.len(), 3, "the clip stored the arp's output, not the keys");
    let mut pitches: Vec<u8> = snap.notes.iter().map(|n| n.note).collect();
    pitches.sort_unstable();
    assert_eq!(pitches, vec![60, 64, 67]);
}

/// The instrument hears the arp's generated notes, not the raw keys:
/// with latch on, a key pressed and released immediately keeps sounding
/// through the arp — only the generated steps can be doing that.
#[test]
fn the_instrument_hears_the_arp_not_the_chord() {
    let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
    let _handle = add_armed_synth(&tx, 0);
    use crate::midi_fx::MidiEffect as _;
    let mut arp = crate::midi_fx::Arpeggiator::new();
    arp.set_parameter(4, 1.0); // latch on
    tx.send(MixerCommand::AddMidiFx { track_id: 0, slot: 0, fx: Box::new(arp) }).unwrap();
    transport.play();

    // Tap the key: on and off in consecutive blocks.
    let mut output = vec![0.0f32; 512];
    mixer.process(&mut output, &[make_note_on(60, 110)], &transport);
    transport.advance(256, 44_100);
    mixer.process(&mut output, &[make_note_off(60)], &transport);
    transport.advance(256, 44_100);

    // Long after any release tail, the latched arp is still firing.
    let mut late = 0.0f32;
    for b in 0..600 {
        output.fill(0.0);
        mixer.process(&mut output, &[], &transport);
        transport.advance(256, 44_100);
        if b > 500 {
            late = late.max(output.iter().fold(0.0f32, |a, &s| a.max(s.abs())));
        }
    }
    assert!(
        late > 1.0e-3,
        "the latched arp went silent — its output is not reaching the instrument"
    );
}

/// Bypassing the slot mid-run flushes its note-offs — nothing hangs.
#[test]
fn bypassing_the_arp_hangs_no_note() {
    let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
    let _handle = add_armed_synth(&tx, 0);
    tx.send(MixerCommand::AddMidiFx {
        track_id: 0,
        slot: 0,
        fx: Box::new(crate::midi_fx::Arpeggiator::new()),
    })
    .unwrap();
    transport.play();
    let mut output = vec![0.0f32; 512];
    mixer.process(&mut output, &[make_note_on(60, 110)], &transport);
    transport.advance(256, 44_100);
    mixer.process(&mut output, &[], &transport);
    transport.advance(256, 44_100);

    // Bypass, release the key, and let the tail die.
    tx.send(MixerCommand::SetMidiFxBypass { track_id: 0, slot: 0, bypassed: true }).unwrap();
    mixer.process(&mut output, &[make_note_off(60)], &transport);
    transport.advance(256, 44_100);
    let mut tail = 0.0f32;
    for b in 0..400 {
        output.fill(0.0);
        mixer.process(&mut output, &[], &transport);
        transport.advance(256, 44_100);
        if b > 300 {
            tail = tail.max(output.iter().fold(0.0f32, |a, &s| a.max(s.abs())));
        }
    }
    assert!(tail < 1.0e-3, "a note hung after bypass: tail peak {tail}");
}

/// A loop smaller than a bar — one beat — wraps cleanly and keeps
/// striking its note every pass: the chop-a-quarter-bar case.
#[test]
fn a_one_beat_loop_keeps_striking() {
    let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
    let _handle = add_armed_synth(&tx, 0);
    tx.send(MixerCommand::CreateClip { track_id: 0, start_tick: 0, length_ticks: Transport::PPQ })
        .unwrap();
    tx.send(MixerCommand::UpdateClip {
        track_id: 0,
        clip_index: 0,
        events: vec![
            ClipEvent { tick: 0, status: 0x90, data1: 60, data2: 110 },
            ClipEvent { tick: Transport::PPQ / 2, status: 0x80, data1: 60, data2: 0 },
        ],
    })
    .unwrap();
    transport.set_loop_range(0, Transport::PPQ);
    if !transport.is_looping() {
        transport.toggle_loop();
    }
    transport.play();

    // One beat at 120 = 0.5 s = 86 blocks of 256. Run twenty beats and
    // count the strikes by watching the level rise from silence.
    let mut output = vec![0.0f32; 512];
    let mut strikes = 0usize;
    let mut quiet = true;
    for _ in 0..(86 * 20) {
        output.fill(0.0);
        mixer.process(&mut output, &[], &transport);
        transport.advance(256, 44_100);
        let peak = output.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        if peak > 0.02 && quiet {
            strikes += 1;
            quiet = false;
        } else if peak < 0.002 {
            quiet = true;
        }
    }
    assert!(
        (15..=22).contains(&strikes),
        "a one-beat loop should strike ~20 times in 20 beats, got {strikes}"
    );
}

/// The practice click runs with the transport parked, and pattern 1
/// A clip's quarter notes land one real half-second apart at 120 BPM —
/// the song itself, measured the way the metronome is below. The two
/// tests together bracket a field report of "everything sounds half
/// speed": if both are green, the audio engine's clock is honest end
/// to end and the cause lives above it.
#[test]
fn clip_notes_land_on_the_beat_at_both_device_rates() {
    use crate::clip::ClipEvent;
    for sr in [44_100u32, 48_000] {
        let (tx, rx) = mixer_command_channel();
        let (clip_tx, _clip_rx) = clip_snapshot_channel();
        let master_vu = Arc::new(VuLevels::new());
        let transport = Arc::new(Transport::new(120.0));
        let mut mixer = Mixer::new(rx, master_vu, clip_tx, sr, 256);
        let handle = Arc::new(TrackHandle::new(0, TrackKind::Instrument));
        handle.config.armed.store(true, std::sync::atomic::Ordering::Relaxed);
        tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle }).unwrap();
        tx.send(MixerCommand::SetInstrument {
            track_id: 0,
            instrument: Box::new(PhosphorSynth::new()),
        })
        .unwrap();
        // One bar, four quarter notes.
        tx.send(MixerCommand::CreateClip {
            track_id: 0,
            start_tick: 0,
            length_ticks: Transport::PPQ * 4,
        })
        .unwrap();
        let mut events = Vec::new();
        for beat in 0..4i64 {
            let on = beat * Transport::PPQ;
            events.push(ClipEvent { tick: on, status: 0x90, data1: 60, data2: 110 });
            events.push(ClipEvent {
                tick: on + Transport::PPQ / 4,
                status: 0x80,
                data1: 60,
                data2: 0,
            });
        }
        tx.send(MixerCommand::UpdateClip { track_id: 0, clip_index: 0, events }).unwrap();
        transport.play();

        let mut onsets: Vec<usize> = Vec::new();
        let block = 256usize;
        let blocks = (2 * sr as usize) / block; // one bar plus slack
        for b in 0..blocks {
            let mut out = vec![0.0f32; block * 2];
            mixer.process(&mut out, &[], &transport);
            transport.advance(block as u32, sr);
            for (i, frame) in out.chunks(2).enumerate() {
                let level = frame[0].abs().max(frame[1].abs());
                let at = b * block + i;
                // One onset per note: the refractory gap does the
                // work, because a synth's own envelope wobbles across
                // any amplitude hysteresis. Notes are a half-second
                // apart; 200 ms of deafness cannot miss one.
                if level > 0.02
                    && onsets.last().is_none_or(|&last| at - last > sr as usize / 5)
                {
                    onsets.push(at);
                }
            }
        }
        let beat = sr as usize / 2;
        assert_eq!(
            onsets.len(),
            4,
            "{sr} Hz: {} note onsets in one bar at {:?}",
            onsets.len(),
            onsets.iter().map(|o| *o as f64 / f64::from(sr)).collect::<Vec<_>>()
        );
        for pair in onsets.windows(2) {
            let gap = pair[1] - pair[0];
            // 256 samples of slack (~6 ms): onset detection rides the
            // synth's own attack, which crosses the threshold at
            // slightly different envelope points note to note. A real
            // timing defect is a beat's worth of error, not six ms.
            assert!(
                (gap as i64 - beat as i64).abs() < 256,
                "{sr} Hz: notes {gap} apart, a beat is {beat}"
            );
        }
    }
}

/// The session metronome clicks once per beat at the transport's own
/// tempo — measured as onset spacing in rendered audio, at the two
/// device rates that exist in the field. A field report of "120 sounds
/// slow" is either this test failing or something outside the mixer;
/// green here is what sends the search up a layer.
#[test]
fn the_metronome_clicks_on_the_beat_at_both_device_rates() {
    for sr in [44_100u32, 48_000] {
        let (tx, rx) = mixer_command_channel();
        let (clip_tx, _clip_rx) = clip_snapshot_channel();
        let master_vu = Arc::new(VuLevels::new());
        let transport = Arc::new(Transport::new(120.0));
        let mut mixer = Mixer::new(rx, master_vu, clip_tx, sr, 256);
        drop(tx);
        if !transport.is_metronome_on() {
            transport.toggle_metronome();
        }
        transport.play();

        // Four seconds of stereo-interleaved output, one block at a time.
        let mut onsets: Vec<usize> = Vec::new();
        let mut above = false;
        let block = 256usize;
        let blocks = (4 * sr as usize) / block;
        for b in 0..blocks {
            let mut out = vec![0.0f32; block * 2];
            mixer.process(&mut out, &[], &transport);
            // The engine advances after processing; the harness must
            // too, or the metronome stares at beat zero forever.
            transport.advance(block as u32, sr);
            for (i, frame) in out.chunks(2).enumerate() {
                let level = frame[0].abs().max(frame[1].abs());
                let at = b * block + i;
                // A click is a burst of cycles: one onset per burst,
                // enforced by a 50 ms refractory gap rather than by
                // amplitude hysteresis, which single cycles defeat.
                if level > 0.05
                    && !above
                    && onsets.last().is_none_or(|&last| at - last > sr as usize / 20)
                {
                    onsets.push(at);
                    above = true;
                } else if level < 0.02 {
                    above = false;
                }
            }
        }
        // 120 BPM = one click every half second, whatever the rate.
        let beat = sr as usize / 2;
        assert!(
            onsets.len() >= 7,
            "{sr} Hz: only {} clicks in four seconds at {:?}",
            onsets.len(),
            onsets.iter().map(|o| *o as f64 / f64::from(sr)).collect::<Vec<_>>()
        );
        for pair in onsets.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(
                (gap as i64 - beat as i64).abs() < 64,
                "{sr} Hz: clicks {} apart, a beat is {beat}",
                gap
            );
        }
    }
}

/// clicks half as often — beats 2 and 4 only, the jazz convention.
#[test]
fn the_practice_click_runs_without_the_transport() {
    let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
    let mut output = vec![0.0f32; 512];
    // Two bars at 120: 4 seconds = 689 blocks of 256 frames.
    let count_clicks = |mixer: &mut Mixer, transport: &Transport| -> usize {
        let mut clicks = 0usize;
        let mut above = false;
        for _ in 0..689 {
            let mut out = vec![0.0f32; 512];
            mixer.process(&mut out, &[], transport);
            let peak = out.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
            if peak > 0.01 && !above {
                clicks += 1;
                above = true;
            } else if peak < 0.001 {
                above = false;
            }
        }
        clicks
    };

    tx.send(MixerCommand::SetPracticeClick { bpm: 120.0, pattern: 0 }).unwrap();
    mixer.process(&mut output, &[], &transport);
    let all_beats = count_clicks(&mut mixer, &transport);
    assert!((7..=9).contains(&all_beats), "expected ~8 clicks in 2 bars, got {all_beats}");

    tx.send(MixerCommand::SetPracticeClick { bpm: 120.0, pattern: 1 }).unwrap();
    mixer.process(&mut output, &[], &transport);
    let two_and_four = count_clicks(&mut mixer, &transport);
    assert!(
        (3..=5).contains(&two_and_four),
        "2&4 should click half as often: {two_and_four} vs {all_beats}"
    );

    tx.send(MixerCommand::SetPracticeClick { bpm: 0.0, pattern: 0 }).unwrap();
    mixer.process(&mut output, &[], &transport);
    let off = count_clicks(&mut mixer, &transport);
    assert_eq!(off, 0, "the click kept going after off");
}

/// The stop edge flushes the MIDI-FX layer: an arp still holding keys
/// when the transport stops goes quiet instead of folding those notes
/// — live or from a cut-off clip — into whatever is played next.
#[test]
fn stopping_the_transport_empties_the_arp_pool() {
    let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
    let _handle = add_armed_synth(&tx, 0);
    tx.send(MixerCommand::AddMidiFx {
        track_id: 0,
        slot: 0,
        fx: Box::new(crate::midi_fx::Arpeggiator::new()),
    })
    .unwrap();
    transport.play();
    let mut output = vec![0.0f32; 512];
    // Hold a key with no off — the shape of a clip note cut by a stop.
    mixer.process(&mut output, &[make_note_on(60, 110)], &transport);
    transport.advance(256, 44_100);
    let mut heard = 0.0f32;
    for _ in 0..200 {
        output.fill(0.0);
        mixer.process(&mut output, &[], &transport);
        transport.advance(256, 44_100);
        heard = heard.max(output.iter().fold(0.0f32, |a, &s| a.max(s.abs())));
    }
    assert!(heard > 1.0e-3, "the arp never sounded while playing");

    // Stop. The pool must empty: no new steps, and the tail dies.
    transport.pause();
    let mut mid = 0.0f32;
    let mut late = 0.0f32;
    for b in 0..2400 {
        output.fill(0.0);
        mixer.process(&mut output, &[], &transport);
        let peak = output.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        if (500..600).contains(&b) {
            mid = mid.max(peak);
        }
        if b > 2300 {
            late = late.max(peak);
        }
    }
    let _ = mid;
    assert!(
        late < 1.0e-3,
        "the arp kept firing after the stop — its pool still holds the cut note: {late}"
    );
}

/// A parsed message carrying a receipt-site arrival stamp, for the
/// timing tests. `from_bytes` itself never invents one.
fn stamped(bytes: &[u8], stamp: u64) -> MidiMessage {
    let mut msg = MidiMessage::from_bytes(bytes).expect("parsed");
    msg.received_micros = Some(stamp);
    msg
}

#[test]
fn event_age_only_believes_believable_stamps() {
    let tps = 120.0 * Transport::PPQ as f64 / (60.0 * 44_100.0);
    // No stamp, a stamp from the future, and a stamp too old to be a
    // note that merely waited for the next callback: all block-edge.
    assert_eq!(event_age_ticks(None, 1_000_000, tps, 44_100), 0);
    assert_eq!(event_age_ticks(Some(2_000_000), 1_000_000, tps, 44_100), 0);
    assert_eq!(
        event_age_ticks(Some(1_000_000 - MAX_EVENT_AGE_MICROS - 1), 1_000_000, tps, 44_100),
        0
    );
    // A believable 10ms: 441 frames' worth of ticks.
    let expected = (441.0 * tps) as i64;
    assert_eq!(event_age_ticks(Some(990_000), 1_000_000, tps, 44_100), expected);
    assert!(expected > 0, "the 10ms case must actually move the event");
}

/// A note that arrived 20ms before the callback drained it must land
/// 20ms earlier in the take than the block edge — the whole point of
/// stamping arrivals.
#[test]
fn recorded_note_lands_where_it_was_played() {
    // In a fresh test process the arrival clock's anchor is minutes
    // younger than in any real session, and a 20ms-old stamp would
    // saturate to zero. Age the anchor past the stamps this test builds.
    phosphor_midi::clock::init();
    std::thread::sleep(std::time::Duration::from_millis(30));
    let (mut mixer, tx, clip_rx, transport) = setup_mixer();
    let _handle = add_armed_synth(&tx, 0);
    let tps = 120.0 * Transport::PPQ as f64 / (60.0 * 44_100.0);
    transport.play();
    transport.toggle_record();
    let mut output = vec![0.0f32; 512];

    // Roll well past the believable-age window before playing.
    for _ in 0..100 {
        mixer.process(&mut output, &[], &transport);
        transport.advance(256, 44_100);
    }
    let pos = transport.position_ticks();
    let age_ticks = (0.020 * 44_100.0 * tps) as i64;
    let on = stamped(&[0x90, 60, 100], phosphor_midi::clock::now_micros() - 20_000);
    mixer.process(&mut output, &[on], &transport);
    transport.advance(256, 44_100);

    for _ in 0..30 {
        mixer.process(&mut output, &[], &transport);
        transport.advance(256, 44_100);
    }
    let off = stamped(&[0x80, 60, 0], phosphor_midi::clock::now_micros());
    mixer.process(&mut output, &[off], &transport);
    transport.toggle_record();
    mixer.process(&mut output, &[], &transport);

    let snap = clip_rx.try_recv().expect("commit produced no snapshot");
    let note = snap.notes.first().expect("the note was not recorded");
    let start = note.start_tick;
    let expected = pos - age_ticks;
    assert!(
        (start - expected).abs() <= 8,
        "note landed at {start}, expected about {expected} (block edge would be {pos})"
    );
    assert!(
        start < pos - age_ticks / 2,
        "note stayed on the block edge: {start} vs edge {pos}"
    );
}

/// An aged note drained on the take's very first block cannot land
/// before the take exists — it clamps to the start.
#[test]
fn aged_note_on_the_first_block_clamps_to_the_take_start() {
    phosphor_midi::clock::init();
    std::thread::sleep(std::time::Duration::from_millis(60));
    let (mut mixer, tx, clip_rx, transport) = setup_mixer();
    let _handle = add_armed_synth(&tx, 0);
    transport.play();
    transport.toggle_record();
    let mut output = vec![0.0f32; 512];

    let on = stamped(&[0x90, 60, 100], phosphor_midi::clock::now_micros().saturating_sub(50_000));
    mixer.process(&mut output, &[on], &transport);
    transport.advance(256, 44_100);
    for _ in 0..30 {
        mixer.process(&mut output, &[], &transport);
        transport.advance(256, 44_100);
    }
    mixer.process(&mut output, &[stamped(&[0x80, 60, 0], phosphor_midi::clock::now_micros())], &transport);
    transport.toggle_record();
    mixer.process(&mut output, &[], &transport);

    let snap = clip_rx.try_recv().expect("commit produced no snapshot");
    let note = snap.notes.first().expect("the note was not recorded");
    assert_eq!(
        note.start_tick,
        0,
        "an aged first-block note must clamp to the take start, not go before it"
    );
}

/// A stamp older than any real callback gap is a broken clock, and a
/// broken clock must not move notes — the block edge is the fallback.
#[test]
fn stale_stamps_fall_back_to_the_block_edge() {
    // The anchor must be older than the stale stamp being tested, or the
    // stamp saturates to zero and reads as merely a few ms old.
    phosphor_midi::clock::init();
    std::thread::sleep(std::time::Duration::from_millis(130));
    let (mut mixer, tx, clip_rx, transport) = setup_mixer();
    let _handle = add_armed_synth(&tx, 0);
    transport.play();
    transport.toggle_record();
    let mut output = vec![0.0f32; 512];

    for _ in 0..100 {
        mixer.process(&mut output, &[], &transport);
        transport.advance(256, 44_100);
    }
    let pos = transport.position_ticks();
    let on = stamped(&[0x90, 60, 100], phosphor_midi::clock::now_micros().saturating_sub(500_000));
    mixer.process(&mut output, &[on], &transport);
    transport.advance(256, 44_100);
    for _ in 0..30 {
        mixer.process(&mut output, &[], &transport);
        transport.advance(256, 44_100);
    }
    mixer.process(&mut output, &[stamped(&[0x80, 60, 0], phosphor_midi::clock::now_micros())], &transport);
    transport.toggle_record();
    mixer.process(&mut output, &[], &transport);

    let snap = clip_rx.try_recv().expect("commit produced no snapshot");
    let note = snap.notes.first().expect("the note was not recorded");
    let start = note.start_tick;
    assert!(
        (start - pos).abs() <= 2,
        "a stale stamp moved the note: {start} vs block edge {pos}"
    );
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

/// The count-in end to end: the bars click down over a stopped
/// transport, and the block the countdown ends on is the block the take
/// starts rolling — no keypress in between.
#[test]
fn the_count_in_clicks_and_then_the_take_rolls() {
    let (mut mixer, tx, clip_rx, transport) = setup_mixer();
    let _handle = add_armed_synth(&tx, 0);
    let sr = 44100u32;
    let bf = 256u32;
    let mut output = vec![0.0f32; bf as usize * 2];

    transport.set_loop_bars(1, 1);
    transport.set_count_in_bars(1);
    transport.begin_count_in(true);

    // First blocks of the countdown: the click sounds, nothing rolls.
    let mut click_peak = 0.0f32;
    for _ in 0..4 {
        output.fill(0.0);
        mixer.process(&mut output, &[], &transport);
        click_peak = click_peak.max(output.iter().fold(0.0f32, |a, &s| a.max(s.abs())));
        assert!(!transport.is_playing(), "the countdown started playback early");
    }
    assert!(click_peak > 0.0001, "the countdown made no click, peak={click_peak}");

    // Run the countdown out; the mixer itself fires the transport.
    for _ in 0..500 {
        mixer.process(&mut output, &[], &transport);
        transport.advance(bf, sr);
        if transport.is_playing() {
            break;
        }
    }
    assert!(
        transport.is_playing() && transport.is_recording(),
        "the countdown never handed over to the take"
    );

    // And the take records exactly as one started by hand.
    mixer.process(&mut output, &[make_note_on(60, 100)], &transport);
    transport.advance(bf, sr);
    mixer.process(&mut output, &[make_note_off(60)], &transport);
    transport.advance(bf, sr);
    transport.stop_loop_record();
    mixer.process(&mut output, &[], &transport);
    let snap = clip_rx.try_recv().expect("the counted-in take was lost");
    assert!(snap.notes.iter().any(|n| n.note == 60));
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
///
/// The dearest single command is a whole-bed [`MixerCommand::SetSamplerRange`]
/// at three heavy units. In wall clock that overshoot is smaller than the
/// number suggests: a bed-wide range measures 13 µs against the 22 µs
/// three `SetInstrument`s would take, and the shortest deadline the
/// budget is sized against is 726 µs.
const MAX_COMMAND_COST: u32 =
    HEAVY_COMMAND * MAX_RANGE_PADS.div_ceil(PADS_PER_HEAVY) as u32;
const WORST_CALLBACK: u32 = COMMAND_BUDGET - 1 + MAX_COMMAND_COST;

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
    // Several tracks and several threads, so the work is really shared out
    // and the workers' side of the callback is held to the same rule.
    mixer.set_threads(4);
    let transport = Arc::new(Transport::new(120.0));
    let _handles: Vec<_> = (0..4).map(|id| add_armed_synth(&tx, id)).collect();
    mixer.drain_commands();
    transport.play();

    let mut output = vec![0.0f32; max_frames * 2];
    // One warm-up block: anything lazily built on first use — the
    // wavetable bank behind its `OnceLock`, for one — is built here,
    // outside the region under test.
    mixer.process(&mut output, &[make_note_on(60, 100)], &transport);

    let on_workers = mixer.workers.worker_allocations();
    let allocations = crate::alloc_count::allocations_during(|| {
        for _ in 0..8 {
            mixer.process(&mut output, &[], &transport);
        }
    });
    assert_eq!(allocations, 0, "Mixer::process reached the allocator");
    assert_eq!(mixer.workers.worker_allocations() - on_workers, 0, "a worker reached the allocator");
}

/// An instrument that checks, every block, whether it is being run with
/// subnormal numbers flushed to zero.
struct FlushProbe {
    blocks: Arc<std::sync::atomic::AtomicUsize>,
    kept: Arc<std::sync::atomic::AtomicUsize>,
}

impl Plugin for FlushProbe {
    fn info(&self) -> phosphor_plugin::PluginInfo {
        phosphor_plugin::PluginInfo {
            name: "FlushProbe".into(),
            version: "0".into(),
            author: "test".into(),
            category: phosphor_plugin::PluginCategory::Instrument,
        }
    }
    fn init(&mut self, _sample_rate: f64, _max_buffer_size: usize) {}
    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], _midi: &[MidiEvent]) {
        use std::hint::black_box;
        use std::sync::atomic::Ordering::Relaxed;
        self.blocks.fetch_add(1, Relaxed);
        if black_box(f32::MIN_POSITIVE) * black_box(0.5) != 0.0 {
            self.kept.fetch_add(1, Relaxed);
        }
        for ch in outputs.iter_mut() {
            ch.fill(0.0);
        }
    }
    fn parameter_count(&self) -> usize { 0 }
    fn parameter_info(&self, _index: usize) -> Option<phosphor_plugin::ParameterInfo> { None }
    fn get_parameter(&self, _index: usize) -> f32 { 0.0 }
    fn set_parameter(&mut self, _index: usize, _value: f32) {}
    fn reset(&mut self) {}
}

/// Every instrument runs with subnormals flushed — on the audio thread and on
/// every worker — and the thread that called the mixer gets its own mode back
/// when the block is done. Fading tails make subnormals, and on many CPUs
/// arithmetic on them is slow enough to make a dying note a CPU spike.
#[test]
fn every_track_renders_with_subnormals_flushed_and_the_caller_is_restored() {
    use std::hint::black_box;
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    if !crate::denormal::NoDenormals::available() {
        return;
    }
    let (mut mixer, tx, _clip_rx, transport) = setup_mixer();
    mixer.set_threads(4);
    let blocks = Arc::new(AtomicUsize::new(0));
    let kept = Arc::new(AtomicUsize::new(0));
    for id in 0..8 {
        let handle = Arc::new(TrackHandle::new(id, TrackKind::Instrument));
        tx.send(MixerCommand::AddTrack { kind: TrackKind::Instrument, handle }).unwrap();
        tx.send(MixerCommand::SetInstrument {
            track_id: id,
            instrument: Box::new(FlushProbe { blocks: blocks.clone(), kept: kept.clone() }),
        })
        .unwrap();
    }
    while !mixer.command_rx.is_empty() {
        mixer.drain_commands();
    }
    transport.play();
    let mut output = vec![0.0f32; 512];
    for _ in 0..50 {
        mixer.process(&mut output, &[], &transport);
    }
    assert_eq!(blocks.load(Relaxed), 8 * 50, "the probes did not all run");
    assert_eq!(kept.load(Relaxed), 0, "an instrument ran with subnormals switched on");
    assert!(
        (black_box(f32::MIN_POSITIVE) * black_box(0.5)).is_subnormal(),
        "the mixer left flushing switched on for its caller"
    );
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
