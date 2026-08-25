//! The step sequencer, end to end: created from the menu, given a child
//! instrument, given a pattern, and heard.
//!
//! These drive the real application — the same `App` the terminal drives, the
//! same commands, the same `Mixer` the audio callback owns — with the audio
//! device left out. What they prove is that the route from a key to a speaker
//! is joined up, which no test inside `phosphor-core` or `phosphor-app` can
//! see on its own: those two only meet here.

use std::sync::Arc;

use phosphor_app::sequencer::ops::SeqOp;
use phosphor_app::state::InstrumentType;
use phosphor_core::engine::VuLevels;
use phosphor_core::mixer::{clip_snapshot_channel, Mixer, MixerCommand};
use phosphor_core::transport::Transport;
use phosphor_core::EngineConfig;

use crate::app::App;

fn headless() -> App {
    App::new(EngineConfig { buffer_size: 512, sample_rate: 44_100 }, false, false)
}

/// A mixer fed everything the app has sent so far.
///
/// The application's own mixer is not built when audio is off, so the
/// commands it queued are taken out of the channel and applied to one made
/// here. Same commands, same mixer, same order.
fn mixer_from(app: &App) -> (Mixer, Arc<Transport>) {
    let (tx, rx) = phosphor_core::mixer::mixer_command_channel();
    let (clip_tx, _clip_rx) = clip_snapshot_channel();
    for command in app.drain_mixer_commands() {
        tx.send(command).unwrap();
    }
    let mut mixer = Mixer::new(rx, Arc::new(VuLevels::new()), clip_tx, 44_100, 512);

    // The mixer applies a bounded amount of command work per callback, so the
    // queue is drained by running callbacks — with the transport stopped, so
    // nothing plays while it is being built.
    let transport = app.engine.transport.clone();
    let was_playing = transport.is_playing();
    transport.pause();
    let mut output = vec![0.0f32; 512 * 2];
    for _ in 0..16 {
        mixer.process(&mut output, &[], &transport);
    }
    if was_playing {
        transport.play();
    }
    (mixer, transport)
}

fn render(mixer: &mut Mixer, transport: &Transport, blocks: usize) -> f32 {
    let mut output = vec![0.0f32; 512 * 2];
    let mut peak = 0.0f32;
    for _ in 0..blocks {
        mixer.process(&mut output, &[], transport);
        peak = peak.max(output.iter().map(|s| s.abs()).fold(0.0, f32::max));
        transport.advance(512, 44_100);
    }
    peak
}

/// The whole point, in one test: pick the sequencer from the add-track menu,
/// write four hits, start it, and hear the child instrument play them.
#[test]
fn a_sequencer_track_is_created_given_a_pattern_and_heard() {
    let mut app = headless();
    app.create_instrument_track(InstrumentType::Sequencer);

    // The track it made is an ordinary instrument track carrying the default
    // child — there is no sequencer plugin and no sequencer audio path.
    let track = app.nav.current_track().expect("a track");
    assert_eq!(track.instrument_type, Some(phosphor_app::sequencer::DEFAULT_CHILD));
    assert!(track.sequencer.is_some(), "the track is not a sequencer");
    assert_eq!(track.name, "seq");

    // Four on the floor, and run it.
    for step in [0u8, 4, 8, 12] {
        app.sequencer_op(SeqOp::SelectStep(step));
        app.sequencer_op(SeqOp::ToggleStep);
    }
    app.sequencer_op(SeqOp::SetPlaying(true));
    if let Some(track) = app.nav.current_track_mut() {
        track.volume = 1.0;
        track.sync_to_audio();
    }

    let (mut mixer, transport) = mixer_from(&app);
    transport.play();
    let peak = render(&mut mixer, &transport, 16);
    assert!(peak > 0.001, "the sequencer produced no sound, peak={peak}");
}

/// A sequencer that has not been started is silent, so that adding one to a
/// session does not make a noise nobody asked for.
#[test]
fn a_sequencer_that_is_not_running_is_silent() {
    let mut app = headless();
    app.create_instrument_track(InstrumentType::Sequencer);
    for step in [0u8, 4, 8, 12] {
        app.sequencer_op(SeqOp::SelectStep(step));
        app.sequencer_op(SeqOp::ToggleStep);
    }

    let (mut mixer, transport) = mixer_from(&app);
    transport.play();
    assert_eq!(render(&mut mixer, &transport, 16), 0.0);
}

/// Changing the child changes what is in the plugin slot, and everything the
/// audio thread needs to play the same pattern through it goes with it.
#[test]
fn changing_the_child_replaces_the_instrument_in_the_slot() {
    let mut app = headless();
    app.create_instrument_track(InstrumentType::Sequencer);
    let _ = app.drain_mixer_commands();

    app.sequencer_op(SeqOp::SetChild(InstrumentType::Juno60));
    let commands = app.drain_mixer_commands();

    assert!(
        commands
            .iter()
            .any(|c| matches!(c, MixerCommand::SetInstrument { .. })),
        "the plugin slot was not replaced"
    );
    assert!(
        commands
            .iter()
            .filter(|c| matches!(c, MixerCommand::SetParameter { .. }))
            .count()
            == phosphor_app::preset::param_count(InstrumentType::Juno60),
        "the new child's panel did not follow it"
    );
    // Every slot's lanes moved from a kit to a keyboard, so every slot goes.
    let patterns = commands
        .iter()
        .filter(|c| matches!(c, MixerCommand::SetPattern { .. }))
        .count();
    assert_eq!(patterns, 8);

    let track = app.nav.current_track().unwrap();
    assert_eq!(track.instrument_type, Some(InstrumentType::Juno60));
    assert!(track.sequencer.as_ref().unwrap().lane().is_pitched());
}

/// An edit sends the slot it changed, and nothing else: a pattern the audio
/// thread already has is not resent on every keypress.
#[test]
fn an_edit_sends_one_slot() {
    let mut app = headless();
    app.create_instrument_track(InstrumentType::Sequencer);
    let _ = app.drain_mixer_commands();

    app.sequencer_op(SeqOp::SelectSlot(3));
    assert!(app.drain_mixer_commands().is_empty(), "moving the cursor is not an edit");

    app.sequencer_op(SeqOp::ToggleStep);
    let commands = app.drain_mixer_commands();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        commands[0],
        MixerCommand::SetPattern { slot: 3, .. }
    ));
}

/// A sequencer survives a save and a load, with its child, its patterns and
/// its chain — and it plays afterwards.
#[test]
fn a_sequencer_survives_a_session() {
    let dir = std::env::temp_dir().join(format!("phosphor-seq-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sequencer.phos");

    let mut app = headless();
    app.create_instrument_track(InstrumentType::Sequencer);
    app.sequencer_op(SeqOp::SetChild(InstrumentType::Juno60));
    for step in [0u8, 3, 7, 11] {
        app.sequencer_op(SeqOp::SelectStep(step));
        app.sequencer_op(SeqOp::ToggleStep);
    }
    app.sequencer_op(SeqOp::NudgeSwing(8));
    app.sequencer_op(SeqOp::PushChainEntry { slot: 0, repeats: 3 });
    app.sequencer_op(SeqOp::SetPlaying(true));
    let before = *app.nav.current_track().unwrap().sequencer.clone().unwrap();
    app.do_save(&path.display().to_string());

    let mut reopened = headless();
    reopened.do_load(&path.display().to_string());
    let track = reopened
        .nav
        .tracks
        .iter()
        .find(|t| t.sequencer.is_some())
        .expect("the sequencer track did not come back");
    assert_eq!(track.instrument_type, Some(InstrumentType::Juno60));
    assert_eq!(track.sequencer.as_deref(), Some(&before));

    // ...and it still plays.
    if let Some(index) = reopened.nav.tracks.iter().position(|t| t.sequencer.is_some()) {
        reopened.nav.track_cursor = index;
        reopened.nav.tracks[index].volume = 1.0;
        reopened.nav.tracks[index].sync_to_audio();
    }
    let (mut mixer, transport) = mixer_from(&reopened);
    transport.set_position(0);
    transport.play();
    assert!(render(&mut mixer, &transport, 16) > 0.001, "the reopened sequencer is silent");

    let _ = std::fs::remove_file(&path);
}

/// A session with no sequencer in it is the file it always was. The field is
/// absent rather than null, so an older build reads it and a byte-for-byte
/// comparison against one still matches.
#[test]
fn a_session_without_a_sequencer_is_unchanged() {
    let dir = std::env::temp_dir().join(format!("phosphor-seq-plain-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plain.phos");

    let mut app = headless();
    app.create_instrument_track(InstrumentType::Juno60);
    app.do_save(&path.display().to_string());

    let json = std::fs::read_to_string(&path).unwrap();
    assert!(
        !json.contains("sequencer"),
        "a track with no sequencer wrote one into the file"
    );
    let _ = std::fs::remove_file(&path);
}

/// The step sequencer is the last entry in the add-track menu, which is what
/// makes every session written before it still name the same instruments.
#[test]
fn the_sequencer_is_at_the_end_of_the_menu() {
    assert_eq!(
        InstrumentType::ALL.last(),
        Some(&InstrumentType::Sequencer),
        "adding it anywhere but the end moves every instrument after it"
    );
    assert_eq!(InstrumentType::ALL.len(), 11);
}
