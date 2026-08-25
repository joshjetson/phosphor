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

// ── The step grid: keys, and what reaches the screen ──
//
// Everything below drives `handle_event` with the events a terminal sends,
// because that is the only door the application has. A test that calls
// `sequencer_op` directly proves the op works; these prove the *key* works,
// which is a different thing and the one that breaks.

mod grid {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use phosphor_app::sequencer::ops::SeqOp;
    use phosphor_app::state::{ClipTab, ClipViewFocus, InstrumentType, Pane, SeqBand};
    use phosphor_core::mixer::MixerCommand;
    use phosphor_core::pattern::{Mode, Step};

    use super::headless;
    use crate::app::App;

    /// A sequencer track, with the step grid focused — the state the view is
    /// in the moment it is opened.
    fn grid_app() -> App {
        let mut app = headless();
        app.create_instrument_track(InstrumentType::Sequencer);
        app.nav.focus_pane(Pane::ClipView);
        assert_eq!(
            app.nav.clip_view.clip_tab,
            ClipTab::Sequencer,
            "a new sequencer track did not open on its grid"
        );
        assert_eq!(app.nav.clip_view.focus, ClipViewFocus::PianoRoll);
        let _ = app.drain_mixer_commands();
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }));
    }

    fn press_shift(app: &mut App, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }));
    }

    fn state(app: &App) -> &phosphor_app::sequencer::SequencerState {
        app.nav.current_track().unwrap().sequencer.as_deref().unwrap()
    }

    /// The rule the whole design rests on: a key names an op and the op is
    /// the only thing that happens. `n` writes the step under the cursor, and
    /// nothing else in the application moves — not the piano roll's cursor,
    /// not the clip list, and not a second pattern slot.
    #[test]
    fn a_key_writes_exactly_the_step_it_named_and_nothing_else() {
        let mut app = grid_app();
        app.nav.clip_view.piano_roll.column = 3;
        let clips_before = app.nav.current_track().unwrap().clips.len();

        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('n'));

        let seq = state(&app);
        assert_eq!(seq.step_cursor(), 2, "h/l did not walk the steps");
        assert!(seq.lane().steps[2].on, "n did not write a hit");
        assert_eq!(
            seq.lane().steps.iter().filter(|s| s.on).count(),
            1,
            "one key wrote more than one step"
        );

        // The commands that left: exactly the one slot the edit touched.
        let commands = app.drain_mixer_commands();
        assert_eq!(commands.len(), 1, "an edit sent more than the slot it changed");
        assert!(matches!(commands[0], MixerCommand::SetPattern { slot: 0, .. }));

        // ...and nothing else the key could have leaked into.
        assert_eq!(app.nav.clip_view.piano_roll.column, 3, "the key reached the piano roll");
        assert_eq!(app.nav.current_track().unwrap().clips.len(), clips_before);
        assert_eq!(app.nav.focused_pane, Pane::ClipView);
    }

    /// `j`/`k` walks the bands and stops at both ends; `[`/`]` walks the
    /// lanes, at any depth, without a knob having to be let go of first.
    #[test]
    fn the_bands_walk_with_jk_and_the_lanes_with_brackets() {
        let mut app = grid_app();
        assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Grid);

        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Grid, "k walked off the top");

        for expected in [SeqBand::Step, SeqBand::Pattern, SeqBand::Slots, SeqBand::Slots] {
            press(&mut app, KeyCode::Char('j'));
            assert_eq!(app.nav.clip_view.sequencer.band, expected);
        }

        // The lane is not a band and never becomes one.
        press(&mut app, KeyCode::Char(']'));
        press(&mut app, KeyCode::Char(']'));
        assert_eq!(state(&app).lane_cursor(), 2);
        assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Slots, "] moved the band");
        press(&mut app, KeyCode::Char('['));
        assert_eq!(state(&app).lane_cursor(), 1);
    }

    /// Enter holds a knob, `h`/`l` turn it, `H`/`L` turn it in strides, and
    /// Esc lets go. The fader's contract, on a pitch control.
    #[test]
    fn a_held_knob_turns_and_lets_go() {
        let mut app = grid_app();
        // A melodic child, so the step has a pitch to set at all.
        app.sequencer_op(SeqOp::SetChild(InstrumentType::Juno60));
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Char('j')); // → the step's panel
        press(&mut app, KeyCode::Enter); // hold the pitch knob
        assert!(app.nav.clip_view.sequencer.locked);

        let before = state(&app).step().root();
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(state(&app).step().root(), before + 1, "a semitone");
        press_shift(&mut app, KeyCode::Char('L'));
        assert_eq!(state(&app).step().root(), before + 13, "an octave");
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(state(&app).step().root(), before + 12);

        press(&mut app, KeyCode::Esc);
        assert!(!app.nav.clip_view.sequencer.locked);
        // Released, h/l is a cursor again rather than a value.
        let root = state(&app).step().root();
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(state(&app).step().root(), root, "a released knob is still turning");
        assert_eq!(app.nav.clip_view.sequencer.knob, 1, "h/l did not move between knobs");
    }

    /// A held knob takes every key it is given. Tab and undo are the two that
    /// would otherwise walk out of the view mid-edit.
    #[test]
    fn a_held_knob_swallows_the_keys_that_would_leave() {
        let mut app = grid_app();
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);
        assert!(app.nav.clip_view.sequencer.locked);

        press(&mut app, KeyCode::Tab);
        assert_eq!(app.nav.clip_view.clip_tab, ClipTab::Sequencer, "Tab left a held knob");
        press(&mut app, KeyCode::Char('u'));
        assert!(app.nav.clip_view.sequencer.locked, "undo ran from inside a held knob");
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Step, "j left a held knob");
    }

    /// Every knob the cursor can reach does something. A control that answers
    /// keys and changes nothing is worse than no control at all, and the two
    /// panels are long enough that one could sit there unnoticed.
    #[test]
    fn every_knob_on_every_panel_moves_something() {
        for child in [InstrumentType::DrumRack, InstrumentType::Juno60] {
            for band in [SeqBand::Step, SeqBand::Pattern] {
                let mut app = grid_app();
                app.sequencer_op(SeqOp::SetChild(child));
                app.sequencer_op(SeqOp::ToggleStep);
                let count = crate::app::sequencer_keys::knobs_of(state(&app), band).len();
                assert!(count > 0, "{band:?} has no knobs on it for {child:?}");

                for index in 0..count {
                    app.nav.clip_view.sequencer.focus_band(band);
                    app.nav.clip_view.sequencer.knob = index;
                    let before = state(&app).clone();
                    press(&mut app, KeyCode::Enter);
                    press(&mut app, KeyCode::Char('l'));
                    // Either direction counts: the accent velocity starts at
                    // the top of its travel, and a knob already against its
                    // stop is not a dead knob.
                    if &before == state(&app) {
                        press(&mut app, KeyCode::Char('h'));
                    }
                    assert_ne!(
                        &before,
                        state(&app),
                        "knob {index} of {band:?} on {child:?} changed nothing",
                    );
                    press(&mut app, KeyCode::Esc);
                }
            }
        }
    }

    /// A drum lane's panel is its lane, because a step on a kit says only
    /// *when*; a melodic lane's panel is the step, because that is where the
    /// pitch is.
    #[test]
    fn the_step_panel_follows_what_the_sequencer_is_driving() {
        let mut app = grid_app();
        let drum = crate::app::sequencer_keys::step_knobs(state(&app));
        assert!(drum.contains(&phosphor_app::state::SeqKnob::Voice));
        assert!(!drum.contains(&phosphor_app::state::SeqKnob::Pitch));

        app.sequencer_op(SeqOp::SetChild(InstrumentType::Juno60));
        let keys = crate::app::sequencer_keys::step_knobs(state(&app));
        assert_eq!(keys.first(), Some(&phosphor_app::state::SeqKnob::Pitch));
    }

    /// The one pitch control walks scale degrees when the pattern is in a
    /// mode: five presses in dorian is five notes of the scale, not five
    /// semitones.
    #[test]
    fn the_pitch_control_walks_the_mode() {
        let mut app = grid_app();
        app.sequencer_op(SeqOp::SetChild(InstrumentType::Juno60));
        app.sequencer_op(SeqOp::ToggleStep);
        app.sequencer_op(SeqOp::CycleMode(2)); // dorian
        app.sequencer_op(SeqOp::SetTonic(0));
        assert_eq!(state(&app).pattern().mode, Mode::Dorian);

        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);
        let root = state(&app).step().root();
        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('l'));
        // C to D is a tone, D to E flat is a semitone: three semitones for
        // two degrees, which a chromatic walk could not produce.
        assert_eq!(state(&app).step().root(), root + 3);
    }

    /// The tie key ties, and the same key rests under the record cursor.
    #[test]
    fn the_tie_key_holds_a_step_into_the_next() {
        let mut app = grid_app();
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Char('_'));
        assert_eq!(state(&app).step().gate, Step::TIE);
        press(&mut app, KeyCode::Char('_'));
        assert_ne!(state(&app).step().gate, Step::TIE, "the tie key does not let go");
    }

    /// The slot strip: `h`/`l` picks which pattern is being looked at, Enter
    /// queues it, and Enter on the queued one takes it back.
    #[test]
    fn the_slot_strip_selects_and_queues() {
        let mut app = grid_app();
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Slots);

        press(&mut app, KeyCode::Char('l'));
        assert_eq!(state(&app).selected_slot(), 1, "the slot cursor is the selected slot");

        app.sequencer_op(SeqOp::SetPlaying(true));
        press(&mut app, KeyCode::Enter);
        assert_eq!(state(&app).queued_slot(), Some(1));
        press(&mut app, KeyCode::Enter);
        assert_eq!(state(&app).queued_slot(), None, "Enter did not take the queue back");

        // ...and a digit jumps.
        press(&mut app, KeyCode::Char('4'));
        assert_eq!(state(&app).selected_slot(), 3);
    }

    /// `c` builds a chain by asking for the same pattern again, which is how
    /// `A×4 B×2` gets written: four presses, then a slot, then two.
    #[test]
    fn the_chain_key_counts_repeats() {
        let mut app = grid_app();
        app.nav.clip_view.sequencer.focus_band(SeqBand::Slots);
        for _ in 0..4 {
            press(&mut app, KeyCode::Char('c'));
        }
        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Char('c'));

        let chain: Vec<(u8, u8)> =
            state(&app).chain().iter().map(|e| (e.slot, e.repeats)).collect();
        assert_eq!(chain, vec![(0, 4), (1, 2)]);

        press_shift(&mut app, KeyCode::Char('C'));
        assert!(state(&app).chain().is_empty());
    }

    /// Digits jump to a step, and two of them make a number bigger than nine
    /// — a sixteen-step pattern has steps a single digit cannot name.
    #[test]
    fn digits_jump_to_a_step() {
        let mut app = grid_app();
        press(&mut app, KeyCode::Char('5'));
        assert_eq!(state(&app).step_cursor(), 4);
        press(&mut app, KeyCode::Char('1'));
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(state(&app).step_cursor(), 11);
    }
}

// ── Bounce, step record, and what is actually on the screen ──

mod screen {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use phosphor_app::sequencer::ops::SeqOp;
    use phosphor_app::state::{ClipTab, InstrumentType, Pane, SeqBand};
    use phosphor_core::mixer::MixerCommand;
    use phosphor_core::pattern::{Chord, Step};

    use super::headless;
    use crate::app::App;

    fn grid_app() -> App {
        let mut app = headless();
        app.create_instrument_track(InstrumentType::Sequencer);
        app.nav.focus_pane(Pane::ClipView);
        let _ = app.drain_mixer_commands();
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }));
    }

    fn state(app: &App) -> &phosphor_app::sequencer::SequencerState {
        app.nav.current_track().unwrap().sequencer.as_deref().unwrap()
    }

    /// Four on the floor, so there is something to bounce and something to
    /// draw.
    fn four_on_the_floor(app: &mut App) {
        for step in [0u8, 4, 8, 12] {
            app.sequencer_op(SeqOp::SelectStep(step));
            app.sequencer_op(SeqOp::ToggleStep);
        }
        app.sequencer_op(SeqOp::SelectStep(0));
    }

    /// The frame, as text. Everything below reads the screen rather than the
    /// state behind it, because a value that is right and invisible is a
    /// control nobody can use.
    fn screen(app: &App, width: u16, height: u16) -> Vec<String> {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let snapshot = app.engine.transport.snapshot();
        terminal
            .draw(|frame| crate::ui::render(frame, &snapshot, &app.nav, None))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect()
    }

    fn joined(app: &App, width: u16, height: u16) -> String {
        screen(app, width, height).join("\n")
    }

    /// `b` compiles the pattern onto the timeline at the next free bar, tells
    /// the audio thread about the clip, and stops the pattern — because a
    /// bounce playing under the pattern that made it is every note flammed
    /// against itself.
    #[test]
    fn bounce_writes_a_clip_and_stops_the_pattern() {
        let mut app = grid_app();
        four_on_the_floor(&mut app);
        app.sequencer_op(SeqOp::SetPlaying(true));
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Char('b'));

        let track = app.nav.current_track().unwrap();
        assert_eq!(track.clips.len(), 1, "the bounce made no clip");
        let clip = &track.clips[0];
        assert_eq!(clip.start_tick, 0, "a bounce at the playhead lands on the bar under it");
        assert_eq!(clip.length_ticks, 3840, "a sixteen-step sixteenth pattern is one bar");
        assert_eq!(clip.notes.len(), 4, "four hits, four notes");
        assert!(!state(&app).is_playing(), "the pattern kept running under its own bounce");

        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::CreateClip { .. })),
            "the audio thread was not told about the clip",
        );
        assert!(commands.iter().any(|c| matches!(c, MixerCommand::UpdateClip { .. })));
        let status = app.live_status().unwrap_or_default().to_string();
        assert!(status.contains("bar 1"), "the status line did not say where: {status}");
        assert!(status.contains("stopped"), "the status line did not say it stopped: {status}");

        // A second bounce goes after the first rather than on top of it.
        press(&mut app, KeyCode::Char('b'));
        let track = app.nav.current_track().unwrap();
        assert_eq!(track.clips.len(), 2);
        assert_eq!(track.clips[1].start_tick, 3840, "two clips on one bar");
    }

    /// An empty pattern has nothing to write, and says so rather than
    /// producing a clip with no notes in it.
    #[test]
    fn bouncing_nothing_says_so() {
        let mut app = grid_app();
        press(&mut app, KeyCode::Char('b'));
        assert!(app.nav.current_track().unwrap().clips.is_empty());
        assert!(app.live_status().unwrap_or_default().contains("nothing to bounce"));
    }

    /// Step record: the notes held during one gesture become one step, and
    /// the cursor moves on once however many fingers were down.
    #[test]
    fn step_record_writes_a_chord_when_the_last_key_lifts() {
        let mut app = grid_app();
        app.sequencer_op(SeqOp::SetChild(InstrumentType::Juno60));
        press(&mut app, KeyCode::Char('r'));
        assert!(state(&app).is_step_recording(), "r did not arm step record");

        for note in [60, 64, 67] {
            app.step_record_note_on(note);
        }
        assert!(!state(&app).lane().steps[0].on, "the step was written before the keys came up");

        app.step_record_note_off(64);
        assert!(!state(&app).lane().steps[0].on, "a chord was written on the first finger up");
        app.step_record_note_off(60);
        app.step_record_note_off(67);

        let step = *state(&app).lane().steps.first().unwrap();
        assert!(step.on);
        assert_eq!(step.root(), 60, "the chord was rooted somewhere else");
        assert_eq!(step.chord_kind(), Chord::Maj, "three notes a third apart is a triad");
        assert_eq!(state(&app).step_cursor(), 1, "one gesture, one step forward");

        // A rest moves on without writing; the tie key holds what was written.
        press(&mut app, KeyCode::Char('.'));
        assert_eq!(state(&app).step_cursor(), 2);
        assert!(!state(&app).lane().steps[1].on, "a rest wrote a note");
        app.sequencer_op(SeqOp::SelectStep(1));
        press(&mut app, KeyCode::Char('_'));
        assert_eq!(state(&app).lane().steps[0].gate, Step::TIE, "the tie did not reach the step behind it");
    }

    /// Nothing is written when nothing is armed: playing along with a pattern
    /// is the ordinary way to use a sequencer.
    #[test]
    fn playing_without_arming_writes_nothing() {
        let mut app = grid_app();
        app.step_record_note_on(60);
        app.step_record_note_off(60);
        assert!(!state(&app).lane().steps[0].on);
        assert_eq!(state(&app).step_cursor(), 0);
    }

    /// The queued slot counts down in steps, on the screen, before it
    /// happens — which is the whole reason the switch point is arithmetic
    /// rather than a message that may not have arrived.
    #[test]
    fn the_countdown_reaches_the_screen() {
        let mut app = grid_app();
        four_on_the_floor(&mut app);
        app.sequencer_op(SeqOp::SetPlaying(true));
        app.sequencer_op(SeqOp::QueueSlot(2));
        app.engine.transport.set_position(2880); // step 12 of 16

        let text = joined(&app, 120, 40);
        assert!(text.contains("C in 4 steps"), "no countdown on the screen:\n{text}");
    }

    /// The hits from the other lanes are behind the one being edited. Without
    /// them a kick is written blind against a hat, which is the reason one
    /// lane at a time is workable at all.
    #[test]
    fn the_grid_draws_ghost_hits_from_the_other_lanes() {
        let mut app = grid_app();
        // A snare on step 5, then back to the kick lane, which is empty.
        app.sequencer_op(SeqOp::SelectLane(1));
        app.sequencer_op(SeqOp::SelectStep(4));
        app.sequencer_op(SeqOp::ToggleStep);
        app.sequencer_op(SeqOp::SelectLane(0));

        let text = joined(&app, 120, 40);
        assert!(text.contains('\u{25E6}'), "no ghost hit on the grid:\n{text}");

        // ...and a hit of its own is drawn differently from a ghost of one.
        app.sequencer_op(SeqOp::ToggleStep);
        let text = joined(&app, 120, 40);
        assert!(text.contains('\u{25CF}'), "a written step is not drawn:\n{text}");
    }

    /// Eighty by twenty-four is the floor. The bands that carry the cursor
    /// and the pattern are all still there.
    #[test]
    fn the_view_fits_eighty_by_twenty_four() {
        let mut app = grid_app();
        four_on_the_floor(&mut app);
        let text = joined(&app, 80, 24);

        assert!(text.contains("seq"), "no header:\n{text}");
        assert!(text.contains("lane"), "no lane strip:\n{text}");
        assert!(text.contains("BD"), "no kit names:\n{text}");
        assert!(text.contains("slots"), "no pattern strip:\n{text}");
        assert!(text.contains("chain"), "no chain:\n{text}");
        // ...and the band with the cursor on it is drawn whatever else is not.
        app.nav.clip_view.sequencer.focus_band(SeqBand::Pattern);
        let text = joined(&app, 80, 24);
        assert!(text.contains("swing"), "the focused panel was dropped:\n{text}");
    }

    /// The mini-map is what a large terminal buys: every lane, one glyph per
    /// step, without leaving the lane being edited.
    #[test]
    fn a_tall_terminal_gets_the_whole_pattern() {
        let mut app = grid_app();
        four_on_the_floor(&mut app);
        let short = joined(&app, 120, 24);
        let tall = joined(&app, 120, 44);

        let count = |text: &str| text.matches("BD").count();
        assert!(
            count(&tall) > count(&short),
            "the mini-map did not appear on a terminal with room for it:\n{tall}",
        );
    }

    /// Nine themes, one grid. Nothing here picks a colour of its own: a view
    /// that hardcodes one is a view that is unreadable in eight of them.
    #[test]
    fn the_grid_belongs_to_the_theme() {
        assert!(
            !include_str!("ui/sequencer.rs").contains("Color::Rgb"),
            "the step grid names a colour instead of asking the theme for one",
        );

        let mut app = grid_app();
        four_on_the_floor(&mut app);
        let first = joined(&app, 100, 30);
        for index in 0..crate::theme::THEME_COUNT {
            crate::theme::set_theme(index);
            assert_eq!(
                joined(&app, 100, 30),
                first,
                "the grid drew different characters in theme {}",
                crate::theme::theme_name(),
            );
        }
        crate::theme::set_theme(0);
    }

    /// A panel with more knobs than there is room for scrolls to the one
    /// being turned. A control under the cursor and off the bottom of the
    /// screen answers keys nobody can see the effect of.
    #[test]
    fn a_full_panel_scrolls_to_the_knob_being_used() {
        let app = &mut grid_app();
        app.nav.clip_view.sequencer.focus_band(SeqBand::Pattern);
        let last = crate::app::sequencer_keys::PATTERN_KNOBS.len() - 1;
        app.nav.clip_view.sequencer.knob = last;

        let text = joined(app, 80, 24);
        assert!(text.contains("switch"), "the last knob never reaches the screen:\n{text}");
        assert!(!text.contains("steps \u{25D1}"), "a panel that fits was expected to scroll");
    }

    /// Half a step number is on the screen while it is being typed: sixteen
    /// steps need two digits, so `1` waits to see whether it is `12`, and a
    /// key that appears to have done nothing is a key that gets pressed
    /// again.
    #[test]
    fn a_half_typed_number_is_shown() {
        let mut app = grid_app();
        press(&mut app, KeyCode::Char('1'));
        assert_eq!(state(&app).step_cursor(), 0, "the jump happened on the first digit");
        assert!(joined(&app, 100, 30).contains("step 1_"), "nothing said a digit had landed");

        // ...and anything else abandons it.
        press(&mut app, KeyCode::Char('n'));
        assert!(!joined(&app, 100, 30).contains("step 1_"));
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(state(&app).step_cursor(), 1, "a stale digit turned 2 into 12");
    }

    /// Thirty-two steps is two rows of sixteen, and a tie runs its tail into
    /// the cells its own step does not need.
    #[test]
    fn a_long_pattern_wraps_and_a_tie_shows_its_tail() {
        let mut app = grid_app();
        app.sequencer_op(SeqOp::CycleLength(2)); // 16 → 32
        app.sequencer_op(SeqOp::SelectStep(20));
        app.sequencer_op(SeqOp::ToggleStep);
        app.sequencer_op(SeqOp::ToggleTie);

        let rows = screen(&app, 120, 44);
        assert!(
            rows.iter().any(|row| row.contains("\u{2502}  BD ")),
            "no lane row:\n{}",
            rows.join("\n"),
        );
        assert!(
            rows.iter().any(|row| row.contains("\u{2502}  17 ")),
            "a thirty-two step pattern did not wrap onto a second row:\n{}",
            rows.join("\n"),
        );
        assert!(
            rows.iter().any(|row| row.contains('\u{254C}')),
            "a tie has no tail:\n{}",
            rows.join("\n"),
        );
    }

    /// Any terminal, any size. The view is laid out by what fits rather than
    /// by a fixed geometry, and every one of those decisions is arithmetic on
    /// a width and a height that could go wrong at the edges.
    #[test]
    fn the_view_draws_at_any_size() {
        let mut app = grid_app();
        four_on_the_floor(&mut app);
        for band in SeqBand::ALL {
            app.nav.clip_view.sequencer.focus_band(band);
            for (width, height) in
                [(40, 10), (52, 12), (60, 14), (80, 24), (100, 30), (140, 50), (200, 60)]
            {
                let rows = screen(&app, width, height);
                assert_eq!(rows.len(), height as usize);
                assert!(rows.iter().all(|row| row.chars().count() == width as usize));
            }
        }
    }

    /// The playhead on the grid is the audio thread's, not a cursor of the
    /// UI's own: it is published by the callback that generated the notes,
    /// which is the only place that knows where the pattern actually is.
    #[test]
    fn the_playhead_marks_the_step_that_is_sounding() {
        let mut app = grid_app();
        app.sequencer_op(SeqOp::SetPlaying(true));
        let handle = app.nav.current_track().unwrap().handle.clone().unwrap();

        let grid_row = |app: &App| -> String {
            screen(app, 100, 30)
                .into_iter()
                .find(|row| row.contains("\u{2502}  BD "))
                .map(|row| row.split("  BD ").nth(1).unwrap_or_default().to_string())
                .expect("no grid row on the screen")
        };
        assert!(!grid_row(&app).contains('\u{2502}'), "a stopped pattern has a playhead");

        handle.pattern.publish(0, None, 3, true);
        app.nav.sync_sequencers_from_audio();
        let row = grid_row(&app);
        assert!(row.contains('\u{2502}'), "the playhead is not on the grid: {row:?}");
        assert_eq!(
            row.chars().position(|c| c == '\u{2502}'),
            Some(9),
            "the playhead is on the wrong step: {row:?}",
        );

        // A slot that is only being looked at gets no playhead: the marker
        // belongs to the pattern that is sounding.
        app.sequencer_op(SeqOp::SelectSlot(4));
        assert!(!grid_row(&app).contains('\u{2502}'), "a playhead ran through a silent pattern");
    }

    /// Esc walks back out the way Enter came in: a knob, then the band, then
    /// the view. `q` is the same key by another name, as it is everywhere
    /// else in the clip view.
    #[test]
    fn escape_walks_back_out() {
        let mut app = grid_app();
        app.nav.clip_view.sequencer.focus_band(SeqBand::Pattern);
        press(&mut app, KeyCode::Enter);
        assert!(app.nav.clip_view.sequencer.locked);

        press(&mut app, KeyCode::Esc);
        assert!(!app.nav.clip_view.sequencer.locked, "Esc did not let go of the knob");
        assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Pattern, "Esc left the band too");

        press(&mut app, KeyCode::Esc);
        assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Grid);
        assert_eq!(app.nav.focused_pane, Pane::ClipView, "Esc left the view from a band");

        press(&mut app, KeyCode::Char('q'));
        assert_eq!(app.nav.focused_pane, Pane::Tracks, "Esc from the grid did not leave");
    }

    /// The rule, checked in the source: nothing in the terminal reaches into
    /// a pattern. Every edit names an op, and `dispatch` is the only writer.
    /// A `as_mut()` on a sequencer anywhere in this crate is the beginning of
    /// the second implementation this design exists to prevent.
    #[test]
    fn nothing_in_the_terminal_edits_a_pattern_behind_the_ops() {
        for (name, source) in [
            ("app/keys.rs", include_str!("app/keys.rs")),
            ("app/tracks.rs", include_str!("app/tracks.rs")),
            ("app/sequencer_keys.rs", include_str!("app/sequencer_keys.rs")),
            ("app/sequencer_bounce.rs", include_str!("app/sequencer_bounce.rs")),
            ("app/sequencer_record.rs", include_str!("app/sequencer_record.rs")),
            ("ui/sequencer.rs", include_str!("ui/sequencer.rs")),
        ] {
            assert!(
                !source.contains("sequencer.as_mut") && !source.contains("as_deref_mut"),
                "{name} takes a mutable sequencer out of a track",
            );
        }
        // ...and the renderer does not edit at all.
        assert!(
            !include_str!("ui/sequencer.rs").contains("sequencer_op"),
            "the step grid's renderer dispatches an operation",
        );
    }

    /// The step grid is a tab, and it is only a tab on a track that has a
    /// sequencer: Tab on an ordinary track steps straight over it.
    #[test]
    fn the_grid_is_a_tab_only_where_there_is_one() {
        let app = grid_app();
        assert_eq!(app.nav.clip_view.clip_tab, ClipTab::Sequencer);
        let text = joined(&app, 100, 30);
        assert!(text.contains("[seq]"), "no tab for it:\n{text}");

        let mut plain = headless();
        plain.create_instrument_track(InstrumentType::Juno60);
        plain.nav.focus_pane(Pane::ClipView);
        let text = joined(&plain, 100, 30);
        assert!(!text.contains("[seq]"), "an ordinary track has a step grid tab:\n{text}");

        // ...and cycling the tabs on one never lands on it.
        for _ in 0..8 {
            plain.nav.cycle_tab();
            assert_ne!(plain.nav.clip_view.clip_tab, ClipTab::Sequencer);
        }
    }

    /// A running pattern on a track that also has clips is the doubled-part
    /// trap. The track row says so where it can be seen from the timeline.
    #[test]
    fn a_doubled_track_is_marked_on_its_row() {
        let mut app = grid_app();
        four_on_the_floor(&mut app);
        press(&mut app, KeyCode::Char('b')); // makes a clip, stops the pattern
        assert!(!joined(&app, 100, 30).contains('\u{203C}'), "marked while stopped");

        app.sequencer_op(SeqOp::SetPlaying(true));
        assert!(
            joined(&app, 100, 30).contains('\u{203C}'),
            "a pattern running over its own bounce is not marked",
        );
    }
}
