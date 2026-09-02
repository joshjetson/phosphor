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

    // Four on the floor. NO SetPlaying: a fresh pattern runs by default, so
    // pressing play is all a beginner has to do. This pins the default —
    // it shipped as not-running once and the first real user pressed play
    // into silence.
    for step in [0u8, 4, 8, 12] {
        app.sequencer_op(SeqOp::SelectStep(step));
        app.sequencer_op(SeqOp::ToggleStep);
    }
    if let Some(track) = app.nav.current_track_mut() {
        track.volume = 1.0;
        track.sync_to_audio();
    }

    let (mut mixer, transport) = mixer_from(&app);
    transport.play();
    let peak = render(&mut mixer, &transport, 16);
    assert!(peak > 0.001, "a fresh sequencer + play made no sound, peak={peak}");
}

/// A sequencer that has been STOPPED is silent — `t` is a real mute. (A
/// fresh one runs by default; the silence that needs guaranteeing is the one
/// the player asked for by stopping it.)
#[test]
fn a_sequencer_that_is_not_running_is_silent() {
    let mut app = headless();
    app.create_instrument_track(InstrumentType::Sequencer);
    for step in [0u8, 4, 8, 12] {
        app.sequencer_op(SeqOp::SelectStep(step));
        app.sequencer_op(SeqOp::ToggleStep);
    }
    app.sequencer_op(SeqOp::SetPlaying(false));

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
    assert_eq!(InstrumentType::ALL.len(), 12);
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
    use phosphor_core::pattern::{Mode, Step, LANES};

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

    /// `j`/`k` walks *down the screen*: the sounds first, because on a kit
    /// the rows are the sounds, and then on into the panels below. The hand
    /// that reaches down from the kick expects the snare.
    #[test]
    fn jk_walks_the_sounds_then_down_into_the_panels() {
        let mut app = grid_app();
        assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Grid);
        assert_eq!(state(&app).lane_cursor(), 0);

        press(&mut app, KeyCode::Char('k'));
        assert_eq!(state(&app).lane_cursor(), 0, "k walked off the top of the kit");

        for expected in 1..LANES {
            press(&mut app, KeyCode::Char('j'));
            assert_eq!(state(&app).lane_cursor(), expected);
            assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Grid, "j left the grid early");
        }

        // Off the last sound and into the panels, and back up onto the last
        // sound again — one continuous column of things to stand on.
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Step);
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Grid);
        assert_eq!(state(&app).lane_cursor(), LANES - 1);

        for expected in [SeqBand::Step, SeqBand::Pattern, SeqBand::Slots, SeqBand::Slots] {
            press(&mut app, KeyCode::Char('j'));
            assert_eq!(app.nav.clip_view.sequencer.band, expected);
        }

        // `[` and `]` stay as the fast way between sounds, at any depth.
        press(&mut app, KeyCode::Char('['));
        press(&mut app, KeyCode::Char('['));
        assert_eq!(state(&app).lane_cursor(), LANES - 3);
        assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Slots, "[ moved the band");
        press(&mut app, KeyCode::Char(']'));
        assert_eq!(state(&app).lane_cursor(), LANES - 2);
    }

    /// A keyboard has eight rows too, and `j`/`k` walks them exactly as it
    /// walks a kit's sounds. They are what a chord gets layered across — a
    /// seventh on one row, the ninth above it on the next — which the engine
    /// has always played and the view used to hide.
    #[test]
    fn jk_walks_the_rows_on_a_keyboard_too() {
        let mut app = grid_app();
        app.sequencer_op(SeqOp::SetChild(InstrumentType::Juno60));
        assert!(state(&app).lane().is_pitched());

        for expected in 1..LANES {
            press(&mut app, KeyCode::Char('j'));
            assert_eq!(state(&app).lane_cursor(), expected, "j did not walk the voices");
            assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Grid);
        }
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Step, "the last row is a dead end");
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(state(&app).lane_cursor(), LANES - 1);
    }

    /// Enter opens what is under the cursor — the step's panel on a keyboard,
    /// the lane's on a kit — with the cursor on its first control. It used to
    /// be a second `n`, which left no key for "what is this step set to".
    #[test]
    fn enter_opens_the_panel_for_what_is_under_the_cursor() {
        for child in [InstrumentType::DrumRack, InstrumentType::Juno60] {
            let mut app = grid_app();
            app.sequencer_op(SeqOp::SetChild(child));
            app.nav.clip_view.sequencer.knob = 3;

            press(&mut app, KeyCode::Enter);
            assert_eq!(
                app.nav.clip_view.sequencer.band,
                SeqBand::Step,
                "enter did not open the panel on {child:?}",
            );
            assert_eq!(app.nav.clip_view.sequencer.knob, 0, "it did not land on the first knob");
            assert!(!app.nav.clip_view.sequencer.locked, "it held a knob without being asked");
            assert!(
                state(&app).lane().steps.iter().all(|step| !step.on),
                "enter wrote a step on {child:?}",
            );

            // ...and Esc comes straight back out to the grid.
            press(&mut app, KeyCode::Esc);
            assert_eq!(app.nav.clip_view.sequencer.band, SeqBand::Grid);

            // `n` is still the only key that writes one.
            press(&mut app, KeyCode::Char('n'));
            assert!(state(&app).step().on);
        }
    }

    /// The panel edits the row under the cursor, not the first one. A pitch
    /// set on voice three has to land on voice three or layering is writing
    /// into one place eight times.
    #[test]
    fn the_panel_edits_the_row_under_the_cursor() {
        let mut app = grid_app();
        app.sequencer_op(SeqOp::SetChild(InstrumentType::Juno60));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(state(&app).lane_cursor(), 2);

        press(&mut app, KeyCode::Enter); // the step panel
        press(&mut app, KeyCode::Enter); // hold the pitch knob
        let before = state(&app).pattern().lanes[2].steps[0].root();
        press(&mut app, KeyCode::Char('l'));

        let pattern = state(&app).pattern();
        assert_eq!(pattern.lanes[2].steps[0].root(), before + 1, "the edit missed its row");
        assert_eq!(pattern.lanes[0].steps[0].root(), before, "the edit landed on row one");
        assert!(!pattern.lanes[0].steps[0].on, "row one was written to at all");
    }

    /// Mute and solo are per row on a keyboard as they are on a kit: they go
    /// through the same ops, and a layered chord needs one voice out of it.
    #[test]
    fn mute_and_solo_work_on_a_keyboards_rows() {
        let mut app = grid_app();
        app.sequencer_op(SeqOp::SetChild(InstrumentType::Juno60));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('m'));
        assert!(state(&app).pattern().lanes[1].muted);
        assert!(!state(&app).pattern().lanes[0].muted, "mute reached the wrong row");
        assert!(!state(&app).pattern().lane_audible(1), "a muted row is still audible");

        press(&mut app, KeyCode::Char('m'));
        assert!(!state(&app).pattern().lanes[1].muted);

        press(&mut app, KeyCode::Char('s'));
        assert!(state(&app).pattern().lanes[1].soloed);
        assert!(!state(&app).pattern().lane_audible(0), "a solo did not silence the others");
        assert!(state(&app).pattern().lane_audible(1));
    }

    /// Enter holds a knob, `h`/`l` turn it, `H`/`L` turn it in strides, and
    /// Esc lets go. The fader's contract, on a pitch control.
    #[test]
    fn a_held_knob_turns_and_lets_go() {
        let mut app = grid_app();
        // A melodic child, so the step has a pitch to set at all.
        app.sequencer_op(SeqOp::SetChild(InstrumentType::Juno60));
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Enter); // open the step's panel
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
        app.nav.clip_view.sequencer.focus_band(SeqBand::Step);
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
                    let knob = crate::app::sequencer_keys::knobs_of(state(&app), band)[index];
                    let before = state(&app).clone();
                    let child_before = app.nav.current_track().and_then(|t| t.instrument_type);
                    press(&mut app, KeyCode::Enter);
                    press(&mut app, KeyCode::Char('l'));
                    // Either direction counts: the accent velocity starts at
                    // the top of its travel, and a knob already against its
                    // stop is not a dead knob.
                    if &before == state(&app)
                        && child_before == app.nav.current_track().and_then(|t| t.instrument_type)
                    {
                        press(&mut app, KeyCode::Char('h'));
                    }
                    // The child knob edits the track, not the pattern — that
                    // is its whole point — so it is judged on what it edits.
                    if knob == phosphor_app::state::SeqKnob::Child {
                        assert_ne!(
                            child_before,
                            app.nav.current_track().and_then(|t| t.instrument_type),
                            "the child knob on {child:?} changed nothing",
                        );
                        press(&mut app, KeyCode::Esc);
                        // Put the child back so the rest of the band is
                        // tested on the instrument this pass is about.
                        app.sequencer_op(SeqOp::SetChild(child));
                        continue;
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

        press(&mut app, KeyCode::Enter); // → the step's panel
        press(&mut app, KeyCode::Enter); // → hold the pitch knob
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
        app.nav.clip_view.sequencer.focus_band(SeqBand::Slots);
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
    use phosphor_core::pattern::{Chord, Step, LANES};

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

    /// The columns the step grid is drawn in — everything to the right of
    /// the instrument panel, which draws bars out of the same block
    /// characters the steps are made of.
    fn grid_area(app: &App, width: u16, height: u16) -> Vec<String> {
        const PANEL: usize = 25; // the fx panel and its separator
        screen(app, width, height)
            .into_iter()
            .map(|row| row.chars().skip(PANEL).collect())
            .collect()
    }

    /// The rows that have the running light on them, and the columns it
    /// covers in each — read off the buffer's colours, because a light is a
    /// background and text cannot show one.
    fn lit_columns(app: &App) -> std::collections::BTreeMap<usize, Vec<usize>> {
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let snapshot = app.engine.transport.snapshot();
        terminal
            .draw(|frame| crate::ui::render(frame, &snapshot, &app.nav, None))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let light = crate::theme::playhead_fg();

        let mut found = std::collections::BTreeMap::new();
        for y in 0..40u16 {
            let columns: Vec<usize> = (25..120u16)
                .filter(|&x| buffer[(x, y)].bg == light)
                .map(usize::from)
                .collect();
            if !columns.is_empty() {
                found.insert(usize::from(y), columns);
            }
        }
        found
    }

    /// How many steps are lit on the grid — hits and accents together.
    ///
    /// The count a player would make by looking at the screen, which is the
    /// number that has to match the number of times they pressed `n`.
    fn marks(app: &App) -> usize {
        // A button is two solid columns wide at this size, which is the
        // point of it: a step you can see from across the room.
        let text = grid_area(app, 120, 40).join("\n");
        text.matches("\u{2593}\u{2593}").count() + text.matches("\u{2588}\u{2588}").count()
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

    /// The complaint that caused this view to be rebuilt: writing one step
    /// made three marks appear — the step, a ghost of it on the lane being
    /// looked at, and a dot on a map underneath — and no way to tell which
    /// of them had been asked for. One toggle, one mark, on the row named
    /// after the sound it plays.
    #[test]
    fn one_toggle_makes_exactly_one_mark() {
        let mut app = grid_app();
        assert_eq!(marks(&app), 0, "an empty pattern is not empty on the screen");

        press(&mut app, KeyCode::Char('n'));
        assert_eq!(marks(&app), 1, "one step written, {} marks drawn", marks(&app));

        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(marks(&app), 2);

        // ...on another sound, still one each.
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(marks(&app), 3);

        // ...and taking one off takes off exactly one.
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(marks(&app), 2);
    }

    /// Every lane is a row, named, on a terminal with room for them: that is
    /// what a drum machine looks like, and it is the answer to "how do I
    /// change the sound on each step" — the sounds are the rows.
    #[test]
    fn a_kit_shows_every_sound_as_a_row() {
        let mut app = grid_app();
        four_on_the_floor(&mut app);
        let rows = screen(&app, 120, 40);

        for name in ["BD", "SD", "CH", "OH", "CP", "LT", "MT", "HT"] {
            let row = rows
                .iter()
                .find(|row| row.contains(&format!("{name} \u{2591}")) || row.contains(&format!("{name} \u{2593}")))
                .unwrap_or_else(|| panic!("no row for {name}:\n{}", rows.join("\n")));
            assert!(
                row.matches('\u{2591}').count() + row.matches('\u{2593}').count() >= 16,
                "the {name} row is not a full set of steps: {row:?}",
            );
        }

        // The lane being written is marked, and moving down moves the mark.
        let marked = |app: &App| {
            screen(app, 120, 40)
                .into_iter()
                .find(|row| row.contains('\u{25B8}'))
                .unwrap_or_default()
        };
        assert!(marked(&app).contains("BD"), "the kick row is not marked: {:?}", marked(&app));
        press(&mut app, KeyCode::Char('j'));
        assert!(marked(&app).contains("SD"), "j did not move onto the snare");
    }

    /// Eighty by twenty-four is the floor. The bands that carry the cursor
    /// and the pattern are all still there.
    #[test]
    fn the_view_fits_eighty_by_twenty_four() {
        let mut app = grid_app();
        four_on_the_floor(&mut app);
        let text = joined(&app, 80, 24);

        assert!(text.contains("stopped") || text.contains("step "), "no header:\n{text}");
        assert!(text.contains("sound "), "no sound strip:\n{text}");
        assert!(text.contains("BD") && text.contains("SD"), "no kit names:\n{text}");
        assert!(text.contains(" 16"), "no step numbers:\n{text}");
        assert!(text.contains("slots"), "no pattern strip:\n{text}");
        assert!(text.contains("chain"), "no chain:\n{text}");

        // ...and the knob under the cursor is drawn whatever else is not: a
        // panel with no room scrolls to the row being used.
        app.nav.clip_view.sequencer.focus_band(SeqBand::Pattern);
        let swing = crate::app::sequencer_keys::PATTERN_KNOBS
            .iter()
            .position(|knob| *knob == phosphor_app::state::SeqKnob::Swing)
            .unwrap();
        app.nav.clip_view.sequencer.knob = swing;
        let text = joined(&app, 80, 24);
        assert!(text.contains("swing"), "the knob under the cursor was dropped:\n{text}");
    }

    /// A short terminal scrolls the rows instead of dropping to one of them.
    /// Three lanes of a kit still shows the kick against the hat, which is
    /// the thing a step grid is for; one lane shows nothing at all.
    #[test]
    fn a_short_terminal_scrolls_the_sounds() {
        let mut app = grid_app();
        four_on_the_floor(&mut app);

        let rows_named = |app: &App, height: u16| -> Vec<String> {
            grid_area(app, 120, height)
                .into_iter()
                .filter(|row| row.contains('\u{2591}') || row.contains('\u{2593}'))
                .collect()
        };

        assert_eq!(rows_named(&app, 40).len(), LANES, "a tall terminal is missing sounds");

        let short = rows_named(&app, 26);
        assert!(short.len() > 1, "a short terminal fell back to a single sound");
        assert!(short.len() < LANES, "this height was supposed to be short");
        assert!(
            joined(&app, 120, 26).contains("sound "),
            "the strip does not stand in for the rows that are missing",
        );

        // The sound being written is always one of the rows on the screen.
        for _ in 0..LANES {
            let visible = rows_named(&app, 26);
            assert!(
                visible.iter().any(|row| row.contains('\u{25B8}')),
                "the sound being written scrolled off the screen: {visible:?}",
            );
            press(&mut app, KeyCode::Char('j'));
        }
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

    /// Thirty-two steps is two pages of sixteen where sixteen is all that
    /// fits, and the page follows the cursor. The steps stay the size they
    /// are: a button squeezed to one character is not a button.
    #[test]
    fn a_long_pattern_pages_and_a_tie_shows_its_tail() {
        let mut app = grid_app();
        app.sequencer_op(SeqOp::CycleLength(2)); // 16 → 32
        app.sequencer_op(SeqOp::SelectStep(4));
        app.sequencer_op(SeqOp::ToggleStep);
        app.sequencer_op(SeqOp::ToggleTie);

        let first = grid_area(&app, 80, 24).join("\n");
        assert!(first.contains("1/2"), "a paged grid does not say which page:\n{first}");
        assert!(first.contains('\u{2500}'), "a tie has no tail:\n{first}");
        assert!(first.contains(" 16"), "the first page does not run to sixteen:\n{first}");
        assert!(!first.contains(" 17"), "both pages were drawn at once:\n{first}");

        // Walking past the sixteenth step turns the page over.
        app.sequencer_op(SeqOp::SelectStep(20));
        let second = grid_area(&app, 80, 24).join("\n");
        assert!(second.contains("2/2"), "the page did not turn:\n{second}");
        assert!(second.contains(" 17"), "the second page does not start at seventeen");
        assert_eq!(marks(&app), 0, "a step from the other page was drawn on this one");
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

    /// The layering ask, end to end: a seventh chord on one row and the
    /// ninth above it on the next, sounding together.
    ///
    /// The eight rows were always played — the generator walks every lane
    /// whatever the child is — and the view was the only thing that said a
    /// keyboard had one. This is what unhiding them buys: chords a single
    /// row's chord table cannot spell.
    #[test]
    fn rows_layer_into_a_chord_the_table_cannot_spell() {
        // Not `grid_app`: that one empties the command channel to keep the
        // command-counting tests honest, and this one has to build a mixer
        // out of exactly those commands and listen to it.
        let mut app = headless();
        app.create_instrument_track(InstrumentType::Sequencer);
        app.nav.focus_pane(Pane::ClipView);
        app.sequencer_op(SeqOp::SetChild(InstrumentType::Juno60));

        // Row one: a C major seventh.
        press(&mut app, KeyCode::Char('n'));
        for _ in 0..Chord::Maj7.index() {
            app.sequencer_op(SeqOp::CycleChord(1));
        }
        let seventh = pitches_at(&app, 0);
        assert_eq!(seventh, vec![60, 64, 67, 71], "row one is not a Cmaj7: {seventh:?}");

        // Row two: the ninth above it, on the same step.
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Enter); // the step panel, on the pitch knob
        press(&mut app, KeyCode::Enter); // hold it
        for _ in 0..14 {
            press(&mut app, KeyCode::Char('l'));
        }
        press(&mut app, KeyCode::Esc);

        let layered = pitches_at(&app, 0);
        assert_eq!(
            layered,
            vec![60, 64, 67, 71, 74],
            "the two rows did not stack into a ninth chord: {layered:?}",
        );
        assert!(
            layered.len() > seventh.len(),
            "layering a second row produced no more notes than one row alone",
        );

        // ...and the child plays all five at once.
        if let Some(track) = app.nav.current_track_mut() {
            track.volume = 1.0;
            track.sync_to_audio();
        }
        let (mut mixer, transport) = super::mixer_from(&app);
        transport.set_position(0);
        transport.play();
        assert!(
            super::render(&mut mixer, &transport, 16) > 0.001,
            "a layered chord made no sound",
        );
    }

    /// The distinct pitches a pattern starts at `tick`, as the generator
    /// produces them — the same arithmetic the audio thread runs.
    fn pitches_at(app: &App, tick: i64) -> Vec<u8> {
        let state = state(app);
        let block = state.block(state.selected_slot() as usize);
        let mut events = Vec::new();
        phosphor_core::pattern::compile_cycle(&block, 0, &mut events);
        let mut pitches: Vec<u8> = events
            .iter()
            .filter(|event| event.is_note_on() && event.tick == tick)
            .map(|event| event.data1)
            .collect();
        pitches.sort_unstable();
        pitches.dedup();
        pitches
    }

    /// The walk a person takes the first time they open this, in the order
    /// they take it. Every assertion is something they can see on the screen
    /// or hear out of the speakers — this is the test that stands in for the
    /// user who could not work out how to play the thing.
    #[test]
    fn a_first_time_walkthrough() {
        // 1. Add the track. The screen says what to press.
        let mut app = headless();
        app.create_instrument_track(InstrumentType::Sequencer);
        assert_eq!(app.nav.focused_pane, Pane::ClipView, "the keys landed somewhere else");
        let text = joined(&app, 100, 30);
        assert!(text.contains("write a step"), "the empty grid teaches nothing:\n{text}");
        assert!(text.contains("BD") && text.contains("SD"), "the sounds are not named:\n{text}");

        // 2. Write two steps on the kick. Exactly two marks appear.
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(marks(&app), 2, "two presses of n did not make two marks");

        // 3. Down onto the snare — visibly.
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(state(&app).lane_cursor(), 1);
        let marked = grid_area(&app, 100, 30)
            .into_iter()
            .find(|row| row.contains('\u{25B8}'))
            .unwrap_or_default();
        assert!(marked.contains("SD"), "the cursor is not visibly on the snare: {marked:?}");
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(marks(&app), 4);

        // 4. Space then p — never having pressed `t`. The pattern is running
        //    from birth, so play is all there is to do.
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char('p'));
        assert!(app.engine.transport.is_playing(), "Space p did not start the transport");
        assert!(state(&app).is_playing(), "the pattern was not running when play was pressed");

        // ...and what the audio thread publishes lights the grid.
        let handle = app.nav.current_track().unwrap().handle.clone().unwrap();
        handle.pattern.publish(0, None, 5, true);
        app.nav.sync_sequencers_from_audio();
        assert!(!lit_columns(&app).is_empty(), "the pattern is playing and nothing is lit");

        // 5. Space then 0 — stopped, and the needle back at the beginning.
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char('0'));
        assert!(!app.engine.transport.is_playing());
        assert_eq!(app.engine.transport.position_ticks(), 0, "stop did not rewind");
        handle.pattern.publish(0, None, 0, false);
        app.nav.sync_sequencers_from_audio();
        assert!(lit_columns(&app).is_empty(), "the light kept running after the stop");
    }

    /// The running light. A step sequencer is recognised across a room by
    /// the column of light chasing through it, and this one is drawn on
    /// every lane at once — the whole bar lights, not a mark on one row.
    ///
    /// It is the audio thread's position, not a cursor of the UI's own: the
    /// callback that generated the notes is the only thing that knows where
    /// the pattern actually is.
    #[test]
    fn the_running_light_chases_across_every_lane() {
        let mut app = grid_app();
        four_on_the_floor(&mut app);
        let handle = app.nav.current_track().unwrap().handle.clone().unwrap();

        assert!(lit_columns(&app).is_empty(), "a stopped pattern has a light on it");

        handle.pattern.publish(0, None, 0, true);
        app.nav.sync_sequencers_from_audio();
        let home = lit_columns(&app).values().next().cloned().unwrap();

        handle.pattern.publish(0, None, 3, true);
        app.nav.sync_sequencers_from_audio();
        let lit = lit_columns(&app);
        assert!(!lit.is_empty(), "the pattern is running and nothing is lit");
        assert_eq!(
            lit.len(),
            LANES + 1,
            "the light is on {} rows, not on all eight lanes and the ruler",
            lit.len(),
        );
        let columns: Vec<&Vec<usize>> = lit.values().collect();
        assert!(
            columns.windows(2).all(|pair| pair[0] == pair[1]),
            "the light is not one column: {lit:?}",
        );
        let first = columns[0].clone();

        // ...and it moves along, and wraps.
        handle.pattern.publish(0, None, 4, true);
        app.nav.sync_sequencers_from_audio();
        let next = lit_columns(&app).values().next().cloned().unwrap();
        assert!(next[0] > first[0], "the light did not move on: {first:?} → {next:?}");

        handle.pattern.publish(0, None, 0, true);
        app.nav.sync_sequencers_from_audio();
        let wrapped = lit_columns(&app).values().next().cloned().unwrap();
        assert_eq!(wrapped, home, "the light did not wrap back to the first step");
        assert!(first > home, "the light was not moving left to right");

        // A slot that is only being looked at gets no light: it belongs to
        // the pattern that is sounding.
        app.sequencer_op(SeqOp::SelectSlot(4));
        assert!(lit_columns(&app).is_empty(), "a light ran through a silent pattern");
    }

    /// The header says what the machine is doing, in words, because "I
    /// pressed play and nothing happened" is the first thing that goes wrong
    /// for someone who has not used this one before.
    #[test]
    fn the_header_says_what_the_machine_is_doing() {
        let mut app = grid_app();
        let text = joined(&app, 120, 40);
        assert!(text.contains("stopped"), "a stopped machine does not say so:\n{text}");
        assert!(text.contains("SPC p"), "and does not say what to press:\n{text}");

        let handle = app.nav.current_track().unwrap().handle.clone().unwrap();
        handle.pattern.publish(0, None, 4, true);
        app.nav.sync_sequencers_from_audio();
        let text = joined(&app, 120, 40);
        assert!(text.contains("step 5 of 16"), "a running machine does not say where:\n{text}");

        app.sequencer_op(SeqOp::SetPlaying(false));
        let text = joined(&app, 120, 40);
        assert!(text.contains("muted"), "a muted pattern does not say so:\n{text}");
    }

    /// An empty pattern says what to press, and stops saying it the moment
    /// there is something on the grid to look at instead.
    #[test]
    fn an_empty_pattern_coaches_and_then_gets_out_of_the_way() {
        let mut app = grid_app();
        let text = joined(&app, 120, 40);
        assert!(text.contains("write a step"), "an empty grid teaches nothing:\n{text}");
        assert!(text.contains("pick a sound"));
        assert!(text.contains("play"));

        press(&mut app, KeyCode::Char('n'));
        assert!(
            !joined(&app, 120, 40).contains("write a step"),
            "the coaching line stayed after the first step was written",
        );
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

#[test]
fn a_step_yanks_and_pastes_with_everything_on_it() {
    use phosphor_app::state::SeqBand;
    let mut app = headless();
    app.create_instrument_track(InstrumentType::Sequencer);
    // A melodic child, so the step carries pitch and chord.
    app.sequencer_op(SeqOp::SetChild(InstrumentType::Synth));

    // Dress step 2: on, up a third, a chord, a longer gate, accented.
    app.sequencer_op(SeqOp::SelectStep(2));
    app.sequencer_op(SeqOp::ToggleStep);
    app.sequencer_op(SeqOp::NudgePitch(4));
    app.sequencer_op(SeqOp::CycleChord(2));
    app.sequencer_op(SeqOp::NudgeGate(2));
    app.sequencer_op(SeqOp::ToggleAccent);
    let source = *app
        .nav
        .current_track()
        .and_then(|t| t.sequencer.as_deref())
        .unwrap()
        .step();

    // Yank on the grid, walk away, paste.
    app.nav.clip_view.sequencer.band = SeqBand::Grid;
    app.sequencer_yank();
    app.sequencer_op(SeqOp::SelectStep(9));
    app.sequencer_paste();

    let state = app.nav.current_track().and_then(|t| t.sequencer.as_deref()).unwrap();
    let pasted = state.pattern().lanes[state.lane_cursor()].steps[9];
    assert_eq!(pasted, source, "the step did not travel whole");

    // One undo lifts the paste and leaves the original.
    app.perform_undo();
    let state = app.nav.current_track().and_then(|t| t.sequencer.as_deref()).unwrap();
    assert!(!state.pattern().lanes[state.lane_cursor()].steps[9].on, "undo left the paste");
    assert!(state.pattern().lanes[state.lane_cursor()].steps[2].on, "undo took the original");
}

#[test]
fn yanking_an_empty_step_refuses() {
    use phosphor_app::state::SeqBand;
    let mut app = headless();
    app.create_instrument_track(InstrumentType::Sequencer);
    app.nav.clip_view.sequencer.band = SeqBand::Grid;
    app.sequencer_yank();
    assert!(app.seq_step_clip.is_none(), "an empty step landed in the clipboard");
    app.sequencer_paste();
    let state = app.nav.current_track().and_then(|t| t.sequencer.as_deref()).unwrap();
    assert!(!state.step().on, "a refused paste still wrote something");
}

#[test]
fn a_pattern_yanked_from_the_instrument_row_crosses_tracks() {
    use phosphor_app::state::SeqBand;
    let mut app = headless();
    app.create_instrument_track(InstrumentType::Sequencer);
    let first = app.nav.track_cursor;
    for step in [0u8, 4, 8, 12] {
        app.sequencer_op(SeqOp::SelectStep(step));
        app.sequencer_op(SeqOp::ToggleStep);
    }
    app.sequencer_op(SeqOp::CycleLength(-1)); // a non-default length travels too
    let source = app
        .nav
        .current_track()
        .and_then(|t| t.sequencer.as_deref())
        .map(|s| s.block(s.selected_slot() as usize))
        .unwrap();

    // Yank from the pattern band — where the instrument knob lives.
    app.nav.clip_view.sequencer.band = SeqBand::Pattern;
    app.sequencer_yank();

    // A second sequencer track; paste there.
    app.create_instrument_track(InstrumentType::Sequencer);
    let second = app.nav.track_cursor;
    assert_ne!(first, second);
    app.nav.clip_view.sequencer.band = SeqBand::Pattern;
    app.sequencer_paste();

    let landed = app
        .nav
        .current_track()
        .and_then(|t| t.sequencer.as_deref())
        .map(|s| s.block(s.selected_slot() as usize))
        .unwrap();
    assert_eq!(landed, source, "the pattern did not cross whole — midi, length and all");

    // One undo puts the second track's pattern back to empty.
    app.perform_undo();
    let state = app.nav.current_track().and_then(|t| t.sequencer.as_deref()).unwrap();
    assert!(
        state.pattern().lanes.iter().all(|l| l.steps.iter().all(|s| !s.on)),
        "undo did not clear the pasted pattern"
    );
}

#[test]
fn yank_from_the_step_panel_takes_the_step() {
    use phosphor_app::state::SeqBand;
    let mut app = headless();
    app.create_instrument_track(InstrumentType::Sequencer);
    app.sequencer_op(SeqOp::SetChild(InstrumentType::Synth));
    app.sequencer_op(SeqOp::SelectStep(3));
    app.sequencer_op(SeqOp::ToggleStep);
    app.sequencer_op(SeqOp::CycleChord(1));

    // The player is still in the step panel, looking at the chord they
    // just set — y must take the step, not the pattern.
    app.nav.clip_view.sequencer.band = SeqBand::Step;
    app.sequencer_yank();
    assert!(app.seq_step_clip.is_some(), "the panel yank did not take the step");
    assert!(app.seq_pattern_clip.is_none(), "the panel yank took the pattern instead");

    // And p from the panel drops it on the cursor position too.
    app.sequencer_op(SeqOp::SelectStep(7));
    app.sequencer_paste();
    let state = app.nav.current_track().and_then(|t| t.sequencer.as_deref()).unwrap();
    let pasted = state.pattern().lanes[state.lane_cursor()].steps[7];
    assert!(pasted.on, "the paste never landed");
    assert_eq!(pasted.chord, state.pattern().lanes[state.lane_cursor()].steps[3].chord);
}
