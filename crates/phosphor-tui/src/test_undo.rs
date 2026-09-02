//! Undo/redo scenarios — above all the looper behaviour: undo while the
//! transport is still recording peels the newest layer and the music keeps
//! rolling.
//!
//! The audio thread is not running here. Takes arrive the way they do in the
//! real application — as [`ClipSnapshot`]s fed to `receive_clip_snapshot` —
//! and what would have gone to the mixer is read back off the headless
//! command channel, so a test can tell "the UI told the audio thread" from
//! "the UI only updated itself".

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::state::*;
    use phosphor_core::clip::{ClipSnapshot, NoteSnapshot};
    use phosphor_core::mixer::MixerCommand;
    use phosphor_core::transport::Transport;
    use phosphor_core::EngineConfig;

    const BAR: i64 = Transport::PPQ * 4;

    fn app() -> App {
        App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false)
    }

    fn add_synth_track(app: &mut App) -> usize {
        app.nav.instrument_modal.open = true;
        app.nav.instrument_modal.cursor = 0;
        let instrument = app.nav.instrument_modal.selected();
        app.nav.instrument_modal.open = false;
        app.create_instrument_track(instrument);
        app.nav.track_cursor
    }

    fn note(pitch: u8, start_tick: i64) -> NoteSnapshot {
        NoteSnapshot { note: pitch, velocity: 100, start_tick, duration_ticks: 384, muted: false }
    }

    /// One committed pass, as the audio thread reports it.
    fn take(app: &App, track_idx: usize, notes: Vec<NoteSnapshot>) -> ClipSnapshot {
        ClipSnapshot {
            track_id: app.nav.tracks[track_idx].mixer_id.expect("instrument track"),
            clip_index: 0,
            start_tick: 0,
            length_ticks: BAR,
            event_count: notes.len() * 2,
            notes,
            controls: Vec::new(),
        }
    }

    fn start_recording(app: &App) {
        app.engine.transport.set_loop_bars(1, 1);
        app.engine.transport.start_loop_record();
    }

    fn notes_on(app: &App, track_idx: usize) -> usize {
        app.nav.tracks[track_idx]
            .clips
            .iter()
            .map(|c| c.notes.len())
            .sum()
    }

    // ══════════════════════════════════════════════
    // Takes land on the stack
    // ══════════════════════════════════════════════

    #[test]
    fn a_committed_take_is_one_undo_step() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        start_recording(&app);

        let snap = take(&app, ti, vec![note(60, 0), note(64, 1920)]);
        app.nav.receive_clip_snapshot(snap, true);

        assert_eq!(notes_on(&app, ti), 2, "the take reached the clip");
        assert!(app.nav.undo_stack.top_is_take(), "the take reached the stack");
    }

    /// The looper's core move: `u` mid-recording peels the last committed
    /// take, and the transport does not so much as flinch.
    #[test]
    fn undo_mid_recording_peels_the_take_and_keeps_rolling() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        start_recording(&app);

        let snap = take(&app, ti, vec![note(60, 0)]);
        app.nav.receive_clip_snapshot(snap, true);
        let _ = app.drain_mixer_commands();

        app.live_take_notes = 0; // nothing in flight
        app.perform_undo();

        assert_eq!(notes_on(&app, ti), 0, "the take was not peeled");
        assert!(app.engine.transport.is_recording(), "undo stopped the recorder");
        assert!(app.engine.transport.is_playing(), "undo stopped the transport");
        assert!(app.nav.undo_stack.can_redo(), "the peeled take is not redoable");

        // And the audio thread was told to drop its copy of the clip.
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::RemoveClip { .. })),
            "the audio thread still holds the undone take"
        );
        assert!(
            !commands.iter().any(|c| matches!(c, MixerCommand::DiscardRecording)),
            "an empty pass was discarded"
        );
    }

    /// Notes played this pass are the newest layer: `u` scraps them on the
    /// audio thread and leaves every committed take alone.
    #[test]
    fn undo_with_an_inflight_pass_discards_it_first() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        start_recording(&app);

        let snap = take(&app, ti, vec![note(60, 0)]);
        app.nav.receive_clip_snapshot(snap, true);
        let _ = app.drain_mixer_commands();

        // Two beats in, three notes played: an in-flight pass.
        app.engine.transport.set_position(Transport::PPQ * 2);
        app.live_take_notes = 3;
        app.perform_undo();

        assert_eq!(notes_on(&app, ti), 1, "the committed take was eaten too");
        assert!(app.nav.undo_stack.top_is_take(), "the take left the stack");
        assert_eq!(app.live_take_notes, 0, "the pass still counts as in flight");
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::DiscardRecording)),
            "the recorder was never told to drop the pass"
        );

        // The next press, with the pass now empty, peels the take.
        app.perform_undo();
        assert_eq!(notes_on(&app, ti), 0, "the second press did not peel the take");
    }

    /// Just past the wrap, one stray note in the buffer is the fix the
    /// player already started, not the flub — the flub committed at the
    /// wrap. `u` targets the take.
    #[test]
    fn just_past_the_wrap_the_take_goes_not_the_fix() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        start_recording(&app);

        let snap = take(&app, ti, vec![note(60, 0)]);
        app.nav.receive_clip_snapshot(snap, true);
        let _ = app.drain_mixer_commands();

        // A quarter beat past the wrap, one note of the retake played.
        app.engine.transport.set_position(Transport::PPQ / 4);
        app.live_take_notes = 1;
        app.perform_undo();

        assert_eq!(notes_on(&app, ti), 0, "the take survived");
        let commands = app.drain_mixer_commands();
        assert!(
            !commands.iter().any(|c| matches!(c, MixerCommand::DiscardRecording)),
            "the started fix was thrown away with the flub"
        );
    }

    /// A hot `u` must not reach past the recording and eat an edit made
    /// before it.
    #[test]
    fn undo_mid_recording_never_reaches_past_the_takes() {
        let mut app = app();
        let ti = add_synth_track(&mut app);

        // An edit before recording started.
        let before = app.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackClips { track_idx: ti },
        );
        app.nav.tracks[ti].clips.push(Clip {
            number: 1, width: 4, has_content: true,
            start_tick: 0, length_ticks: BAR,
            notes: vec![note(48, 0)],
            hidden_notes: Vec::new(),
            controls: Vec::new(),
        });
        app.nav.commit_undo(before, "draw note");
        assert!(app.nav.undo_stack.can_undo());

        start_recording(&app);
        app.live_take_notes = 0;
        app.perform_undo();

        assert!(
            app.nav.undo_stack.can_undo(),
            "undo while recording consumed a pre-recording edit"
        );
        assert_eq!(notes_on(&app, ti), 1, "the pre-recording edit was reverted");
    }

    // ══════════════════════════════════════════════
    // Overdub layers
    // ══════════════════════════════════════════════

    /// Passes stack as layers; each press of `u` peels exactly one, newest
    /// first, all the way down to an empty track.
    #[test]
    fn overdub_layers_peel_one_per_press() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        start_recording(&app);
        app.live_take_notes = 0;

        let first = take(&app, ti, vec![note(60, 0)]);
        app.nav.receive_clip_snapshot(first, true);
        let second = take(&app, ti, vec![note(64, 960), note(67, 2880)]);
        app.nav.receive_clip_snapshot(second, true);

        assert_eq!(notes_on(&app, ti), 3, "the layers did not merge");

        app.perform_undo();
        assert_eq!(notes_on(&app, ti), 1, "one press peeled more than one layer");
        app.perform_undo();
        assert_eq!(notes_on(&app, ti), 0, "the first layer would not peel");
        assert!(app.engine.transport.is_recording(), "peeling stopped the recorder");
    }

    /// New material is a new timeline: recording another pass kills the
    /// redo of the peeled one, exactly as every looper does it.
    #[test]
    fn a_new_take_kills_the_redo_of_the_old_one() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        start_recording(&app);
        app.live_take_notes = 0;

        let first = take(&app, ti, vec![note(60, 0)]);
        app.nav.receive_clip_snapshot(first, true);
        app.perform_undo();
        assert!(app.nav.undo_stack.can_redo());

        let second = take(&app, ti, vec![note(64, 1920)]);
        app.nav.receive_clip_snapshot(second, true);
        assert!(!app.nav.undo_stack.can_redo(), "a stale take is still redoable");
    }

    // ══════════════════════════════════════════════
    // After the stop
    // ══════════════════════════════════════════════

    /// The habit learned while jamming keeps its meaning when the transport
    /// stops: `u` drops a take, `Ctrl+r` puts it back.
    #[test]
    fn takes_undo_and_redo_the_same_after_stop() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        start_recording(&app);

        let snap = take(&app, ti, vec![note(60, 0), note(64, 1920)]);
        app.nav.receive_clip_snapshot(snap, true);
        app.engine.transport.stop_loop_record();

        app.perform_undo();
        assert_eq!(notes_on(&app, ti), 0, "the take did not undo after stop");

        app.perform_redo();
        assert_eq!(notes_on(&app, ti), 2, "the take did not redo after stop");
        assert!(app.nav.undo_stack.top_is_take(), "redo lost the take marking");
    }

    // ══════════════════════════════════════════════
    // The insert chain
    // ══════════════════════════════════════════════

    /// Put the first buildable effect from the FX menu onto the current
    /// track, the way the player does.
    fn add_first_buildable_fx(app: &mut App) -> bool {
        for cursor in 0..FxType::ALL.len() {
            app.nav.fx_menu.open = true;
            app.nav.fx_menu.cursor = cursor;
            let len_before = app.nav.tracks[app.nav.track_cursor].fx_chain.len();
            app.fx_menu_choose();
            if app.nav.tracks[app.nav.track_cursor].fx_chain.len() > len_before {
                return true;
            }
        }
        false
    }

    /// Adding an effect is a step; undo takes it out on both sides, redo
    /// puts it back.
    #[test]
    fn adding_an_effect_is_undoable() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;
        assert!(add_first_buildable_fx(&mut app), "no effect in this build can be built");
        let _ = app.drain_mixer_commands();

        app.perform_undo();
        assert!(app.nav.tracks[ti].fx_chain.is_empty(), "undo left the effect in the chain");
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::RemoveFx { .. })),
            "the audio thread kept the undone effect"
        );

        app.perform_redo();
        assert_eq!(app.nav.tracks[ti].fx_chain.len(), 1, "redo did not restore the effect");
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::AddFx { .. })),
            "redo never told the audio thread"
        );
    }

    /// Removing an effect is undoable — and the undo restores its settings,
    /// not a factory-fresh copy.
    #[test]
    fn removing_an_effect_restores_its_settings() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;
        assert!(add_first_buildable_fx(&mut app));

        // Turn a knob so the slot has a setting worth restoring.
        let tweaked = app.nav.tracks[ti].fx_chain[0].params[0] + 1.0;
        app.set_fx_param(ti, 0, 0, tweaked);

        app.nav.clip_view.fx_cursor = 0;
        app.remove_fx_at_cursor();
        assert!(app.nav.tracks[ti].fx_chain.is_empty());

        app.perform_undo();
        let chain = &app.nav.tracks[ti].fx_chain;
        assert_eq!(chain.len(), 1, "undo did not restore the effect");
        assert_eq!(
            chain[0].params[0], tweaked,
            "the restored effect lost the knob setting"
        );
    }

    /// A knob sweep is one undo step: many calls, one press of `u`, and the
    /// player is back where the sweep started.
    #[test]
    fn a_knob_sweep_is_one_undo_step() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;
        assert!(add_first_buildable_fx(&mut app));
        let origin = app.nav.tracks[ti].fx_chain[0].params[0];

        for step in 1..=10 {
            app.set_fx_param(ti, 0, 0, origin + step as f32);
        }
        let _ = app.drain_mixer_commands();

        app.perform_undo();
        assert_eq!(
            app.nav.tracks[ti].fx_chain[0].params[0], origin,
            "one undo did not return to the sweep's start"
        );

        // And the sweep was ONE step: the next undo is the add itself.
        app.perform_undo();
        assert!(
            app.nav.tracks[ti].fx_chain.is_empty(),
            "the sweep left more than one step on the stack"
        );
    }

    /// Undoing a knob must not rebuild the chain — a rebuilt delay starts
    /// with an empty line, and the tail the player was listening to dies.
    #[test]
    fn undoing_a_knob_keeps_the_effect_running() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;
        assert!(add_first_buildable_fx(&mut app));
        let origin = app.nav.tracks[ti].fx_chain[0].params[0];
        app.set_fx_param(ti, 0, 0, origin + 3.0);
        let _ = app.drain_mixer_commands();

        app.perform_undo();
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::SetFxParam { .. })),
            "the undone knob never reached the audio thread"
        );
        assert!(
            !commands.iter().any(|c| matches!(
                c,
                MixerCommand::AddFx { .. } | MixerCommand::RemoveFx { .. }
            )),
            "undoing a knob rebuilt the chain"
        );
    }

    /// The bus strips carry chains too, and theirs undo the same way.
    #[test]
    fn a_bus_chain_edit_is_undoable() {
        let mut app = app();
        let bus = app
            .nav
            .tracks
            .iter()
            .position(|t| t.is_bus())
            .expect("the bus strips exist from the start");
        app.nav.track_cursor = bus;
        let chain_before = app.nav.tracks[bus].fx_chain.clone();
        if !add_first_buildable_fx(&mut app) {
            return; // nothing buildable in this build — nothing to test
        }

        app.perform_undo();
        assert_eq!(
            app.nav.tracks[bus].fx_chain, chain_before,
            "undo did not restore the bus chain"
        );
    }

    /// Bypass is a discrete step — never folded into a knob sweep beside it.
    #[test]
    fn bypass_is_its_own_step() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;
        assert!(add_first_buildable_fx(&mut app));
        let origin = app.nav.tracks[ti].fx_chain[0].params[0];

        app.set_fx_param(ti, 0, 0, origin + 2.0);
        app.set_fx_bypass(ti, 0, true);

        app.perform_undo();
        assert!(!app.nav.tracks[ti].fx_chain[0].bypass, "the bypass did not undo alone");
        assert_eq!(
            app.nav.tracks[ti].fx_chain[0].params[0],
            origin + 2.0,
            "undoing the bypass took the knob with it"
        );
    }

    // ══════════════════════════════════════════════
    // The sequencer
    // ══════════════════════════════════════════════

    fn add_sequencer_track(app: &mut App) -> usize {
        app.create_instrument_track(InstrumentType::Sequencer);
        app.nav.track_cursor
    }

    fn seq_state(app: &App, ti: usize) -> &phosphor_app::sequencer::SequencerState {
        app.nav.tracks[ti].sequencer.as_ref().expect("sequencer track")
    }

    /// A step toggle is a step on the stack; undoing it must not stop a
    /// pattern the player has since paused or started.
    #[test]
    fn a_step_toggle_undoes_without_touching_run_state() {
        use phosphor_app::sequencer::ops::SeqOp;
        let mut app = app();
        let ti = add_sequencer_track(&mut app);

        app.sequencer_op(SeqOp::ToggleStep);
        assert!(seq_state(&app, ti).step().on, "the toggle did not land");

        // The player stops the pattern after the edit.
        app.sequencer_op(SeqOp::SetPlaying(false));
        let _ = app.drain_mixer_commands();

        app.perform_undo();
        assert!(!seq_state(&app, ti).step().on, "the step did not undo");
        assert!(
            !seq_state(&app, ti).is_playing(),
            "undo restarted a pattern the player had stopped"
        );
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::SetPattern { .. })),
            "the undone step never reached the audio thread"
        );

        app.perform_redo();
        assert!(seq_state(&app, ti).step().on, "the step did not redo");
    }

    /// Cursor moves and performance controls leave no trace on the stack.
    #[test]
    fn sequencer_cursors_and_transport_push_nothing() {
        use phosphor_app::sequencer::ops::SeqOp;
        let mut app = app();
        add_sequencer_track(&mut app);
        let _ = app.drain_mixer_commands();

        app.sequencer_op(SeqOp::SelectSlot(3));
        app.sequencer_op(SeqOp::MoveStep(2));
        app.sequencer_op(SeqOp::MoveLane(1));
        app.sequencer_op(SeqOp::TogglePlaying);
        app.sequencer_op(SeqOp::QueueSlot(2));
        assert!(
            !app.nav.undo_stack.can_undo(),
            "a cursor move or a transport control pushed an undo step"
        );
    }

    /// A swing sweep folds into one step, and one press of `u` is back at
    /// the start of the ride.
    #[test]
    fn a_swing_sweep_is_one_undo_step() {
        use phosphor_app::sequencer::ops::SeqOp;
        let mut app = app();
        let ti = add_sequencer_track(&mut app);
        let origin = seq_state(&app, ti).pattern().swing;

        for _ in 0..5 {
            app.sequencer_op(SeqOp::NudgeSwing(1));
        }
        assert_ne!(seq_state(&app, ti).pattern().swing, origin);

        app.perform_undo();
        assert_eq!(
            seq_state(&app, ti).pattern().swing, origin,
            "one undo did not return to the sweep's start"
        );
        assert!(!app.nav.undo_stack.can_undo(), "the sweep left more than one step");
    }

    /// Swapping a sequencer's child instrument is undoable whole: the
    /// instrument comes back, its panel comes back, and the lanes a
    /// drum-to-keyboard swap re-laid come back with it.
    #[test]
    fn swapping_the_child_undoes_whole() {
        use phosphor_app::sequencer::ops::SeqOp;
        let mut app = app();
        let ti = add_sequencer_track(&mut app);
        let original = app.nav.tracks[ti].instrument_type.expect("child instrument");

        // A step in the pattern, so the lane relayout has something to lose.
        app.sequencer_op(SeqOp::ToggleStep);
        let lanes_before: Vec<u8> = seq_state(&app, ti)
            .pattern()
            .lanes
            .iter()
            .map(|l| l.note)
            .collect();
        // A panel edit, so the restored panel is a real state and not defaults.
        app.nav.clip_view.synth_param_cursor = 4;
        app.nav.adjust_synth_param(0.05);
        let params_before = app.nav.tracks[ti].synth_params.clone();
        let _ = app.drain_mixer_commands();

        // Drums to a keyboard: the drum-ness changes and the lanes re-lay.
        app.sequencer_op(SeqOp::SetChild(InstrumentType::Juno60));
        assert_eq!(app.nav.tracks[ti].instrument_type, Some(InstrumentType::Juno60));
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::SetInstrument { .. })),
            "the swap never reached the plugin slot"
        );

        app.perform_undo();
        assert_eq!(
            app.nav.tracks[ti].instrument_type, Some(original),
            "the old child did not come back"
        );
        assert_eq!(
            app.nav.tracks[ti].synth_params, params_before,
            "the old child's panel did not come back"
        );
        let lanes_after: Vec<u8> = seq_state(&app, ti)
            .pattern()
            .lanes
            .iter()
            .map(|l| l.note)
            .collect();
        assert_eq!(lanes_after, lanes_before, "the lanes did not come back");
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::SetInstrument { .. })),
            "undo never rebuilt the plugin slot"
        );

        app.perform_redo();
        assert_eq!(
            app.nav.tracks[ti].instrument_type, Some(InstrumentType::Juno60),
            "redo did not swap the child again"
        );
    }

    /// The child knob walks a list: a flick through several instruments is
    /// one step, back to the one the player left.
    #[test]
    fn a_child_flick_is_one_undo_step() {
        use phosphor_app::sequencer::ops::SeqOp;
        let mut app = app();
        let ti = add_sequencer_track(&mut app);
        let original = app.nav.tracks[ti].instrument_type.expect("child instrument");

        app.sequencer_op(SeqOp::SetChild(InstrumentType::Juno60));
        app.sequencer_op(SeqOp::SetChild(InstrumentType::DX7));
        app.sequencer_op(SeqOp::SetChild(InstrumentType::Rhodes));

        app.perform_undo();
        assert_eq!(
            app.nav.tracks[ti].instrument_type, Some(original),
            "one undo did not return to the child the flick started from"
        );
        assert!(
            !app.nav.undo_stack.can_undo(),
            "the flick left more than one step"
        );
    }

    // ══════════════════════════════════════════════
    // Panels, faders, routing
    // ══════════════════════════════════════════════

    /// A synth knob sweep is one step, and undo pushes the whole panel back
    /// to the audio thread.
    #[test]
    fn a_synth_knob_sweep_undoes_in_one_press() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        // A continuous control, not a patch selector.
        app.nav.clip_view.synth_param_cursor = 4;
        let origin = app.nav.tracks[ti].synth_params[4];

        for _ in 0..6 {
            app.nav.adjust_synth_param(0.05);
        }
        assert_ne!(app.nav.tracks[ti].synth_params[4], origin);
        let _ = app.drain_mixer_commands();

        app.perform_undo();
        assert_eq!(
            app.nav.tracks[ti].synth_params[4], origin,
            "one undo did not return the knob to the sweep's start"
        );
        assert!(!app.nav.undo_stack.can_undo(), "the sweep left more than one step");
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::SetParameter { .. })),
            "the undone panel never reached the audio thread"
        );
    }

    /// The fader ride undoes to its origin; mute is a step; solo never is.
    #[test]
    fn mix_strip_undo_honours_the_conventions() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        let origin = app.nav.tracks[ti].volume;

        app.nav.adjust_volume(4);
        app.nav.adjust_volume(-1);
        app.perform_undo();
        assert_eq!(app.nav.tracks[ti].volume, origin, "the ride did not undo whole");

        app.nav.toggle_mute();
        assert!(app.nav.tracks[ti].muted);
        app.perform_undo();
        assert!(!app.nav.tracks[ti].muted, "mute did not undo");

        app.nav.toggle_solo();
        let soloed = app.nav.tracks[ti].soloed;
        app.perform_undo(); // nothing on the stack — must not touch solo
        assert_eq!(app.nav.tracks[ti].soloed, soloed, "undo un-auditioned a track");
    }

    /// Pan and send moves are one gesture each, and undo resyncs the
    /// routing to the audio thread.
    #[test]
    fn routing_moves_are_undoable() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        app.nav.track_element = TrackElement::Pan;
        let origin = app.nav.tracks[ti].pan;

        app.step_routing(3);
        app.step_routing(2);
        assert_ne!(app.nav.tracks[ti].pan, origin);
        let _ = app.drain_mixer_commands();

        app.perform_undo();
        assert_eq!(app.nav.tracks[ti].pan, origin, "the pan ride did not undo whole");
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::SetPan { .. })),
            "the undone pan never reached the audio thread"
        );
    }

    // ══════════════════════════════════════════════
    // Tempo and the loop brace
    // ══════════════════════════════════════════════

    /// A tempo ride is one step and undoes to where it began.
    #[test]
    fn a_tempo_ride_is_one_undo_step() {
        let mut app = app();
        let origin = app.engine.transport.tempo_bpm();

        app.nudge_tempo(1.0);
        app.nudge_tempo(1.0);
        app.nudge_tempo(1.0);
        assert_eq!(app.engine.transport.tempo_bpm(), origin + 3.0);

        app.perform_undo();
        assert_eq!(
            app.engine.transport.tempo_bpm(), origin,
            "one undo did not return the tempo to the ride's start"
        );
        assert!(!app.nav.undo_stack.can_undo(), "the ride left more than one step");
    }

    /// The loop's range is an edit and undoes; its on/off switch is
    /// transport state and must survive the undo untouched.
    #[test]
    fn loop_range_undoes_but_the_switch_survives() {
        let mut app = app();
        let start = app.nav.loop_editor.end_bar;

        app.edit_loop_range(|l| l.move_end_left());
        app.edit_loop_range(|l| l.move_end_left());
        assert_ne!(app.nav.loop_editor.end_bar, start);

        // The player switches the loop on after the edit.
        app.nav.loop_editor.toggle_enabled();
        app.sync_loop_to_transport();
        let enabled = app.nav.loop_editor.enabled;

        app.perform_undo();
        assert_eq!(
            app.nav.loop_editor.end_bar, start,
            "the range move did not undo whole"
        );
        assert_eq!(
            app.nav.loop_editor.enabled, enabled,
            "undo threw the loop switch"
        );
        // And the transport was told about the restored range.
        assert_eq!(
            app.engine.transport.loop_end(),
            app.nav.loop_editor.end_ticks(),
            "the transport kept the undone range"
        );
    }

    // ══════════════════════════════════════════════
    // Tracks
    // ══════════════════════════════════════════════

    /// Adding a track is a step like any other — and redo brings it back
    /// with its instrument, not as a hollow row.
    #[test]
    fn adding_a_track_is_undoable_and_redoable() {
        let mut app = app();
        let strips = app.nav.tracks.len();

        app.nav.instrument_modal.open = true;
        app.nav.instrument_modal.cursor = 0;
        let instrument = app.nav.instrument_modal.selected();
        app.nav.instrument_modal.open = false;
        app.create_instrument_track_undoable(instrument);
        assert_eq!(app.nav.tracks.len(), strips + 1);

        app.perform_undo();
        assert_eq!(app.nav.tracks.len(), strips, "undo did not remove the new track");

        app.perform_redo();
        assert_eq!(app.nav.tracks.len(), strips + 1, "redo did not restore the track");
        let restored = &app.nav.tracks[app.nav.track_cursor];
        assert!(restored.instrument_type.is_some(), "the restored track has no instrument");
        assert!(restored.mixer_id.is_some(), "the restored track has no audio identity");
    }

    /// A deleted track comes back where it was, not at the bottom of the
    /// list — undo restores the running order, not just the contents.
    #[test]
    fn an_undeleted_track_returns_to_its_place() {
        let mut app = app();
        let first = add_synth_track(&mut app);
        app.nav.tracks[first].name = "one".into();
        add_synth_track(&mut app);
        let second = app.nav.track_cursor;
        app.nav.tracks[second].name = "two".into();

        // Delete "one", the earlier of the two.
        let (earlier, later) = if first < second { (first, second) } else { (second, first) };
        let earlier_name = app.nav.tracks[earlier].name.clone();
        let later_name = app.nav.tracks[later].name.clone();
        app.nav.track_cursor = earlier;
        app.nav.confirm_modal.kind = ConfirmKind::DeleteTrack;
        app.execute_delete(ConfirmKind::DeleteTrack);
        assert_eq!(app.nav.tracks[earlier].name, later_name, "delete did not shift the list");

        app.perform_undo();
        assert_eq!(
            app.nav.tracks[earlier].name, earlier_name,
            "the restored track did not return to its old position"
        );
        assert_eq!(app.nav.tracks[later].name, later_name);
    }
}
