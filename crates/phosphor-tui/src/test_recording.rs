//! Recording journeys, walked with pessimism: record, fix the wrong note,
//! fix the wrong chord, layer the take onto another track. These are the
//! gestures a player leans on hardest, so each test is one of their days.
//!
//! Takes arrive the way they do in the running application — as
//! [`ClipSnapshot`]s from the audio thread — and what the UI tells the
//! mixer is read back off the headless command channel.

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
        app.create_instrument_track(InstrumentType::Synth);
        app.nav.track_cursor
    }

    fn note(pitch: u8, start_tick: i64) -> NoteSnapshot {
        NoteSnapshot { note: pitch, velocity: 100, start_tick, duration_ticks: 384, muted: false }
    }

    /// One committed pass on `track_idx`, as the audio thread reports it.
    fn take(app: &App, track_idx: usize, start_tick: i64, notes: Vec<NoteSnapshot>) -> ClipSnapshot {
        ClipSnapshot {
            track_id: app.nav.tracks[track_idx].mixer_id.expect("instrument track"),
            clip_index: 0,
            start_tick,
            length_ticks: BAR,
            event_count: notes.len() * 2,
            notes,
            controls: Vec::new(),
        }
    }

    fn record_take(app: &mut App, track_idx: usize, start_tick: i64, notes: Vec<NoteSnapshot>) {
        app.engine.transport.set_loop_bars(1, 1);
        app.engine.transport.start_loop_record();
        let snap = take(app, track_idx, start_tick, notes);
        app.nav.receive_clip_snapshot(snap, true);
        app.engine.transport.stop_loop_record();
    }

    fn status(app: &App) -> String {
        app.live_status().unwrap_or_default().to_string()
    }

    // ══════════════════════════════════════════════
    // Layering: a take copied onto another track
    // ══════════════════════════════════════════════

    /// The layering journey: record a part, yank the clip, put it on a
    /// second track at the same bars so a second instrument doubles it.
    #[test]
    fn a_take_layers_onto_another_track_at_the_same_bars() {
        let mut app = app();
        let first = add_synth_track(&mut app);
        app.create_instrument_track(InstrumentType::Juno60);
        let second = app.nav.track_cursor;

        app.nav.track_cursor = first;
        record_take(&mut app, first, BAR, vec![note(60, 0), note(64, 960), note(67, 1920)]);
        assert_eq!(app.nav.tracks[first].clips.len(), 1, "the take never landed");

        app.yank_clip(0);
        app.nav.track_cursor = second;
        let _ = app.drain_mixer_commands();
        app.paste_clip_to_track();

        let source = &app.nav.tracks[first].clips[0];
        let layered = app.nav.tracks[second]
            .clips
            .first()
            .expect("the paste never landed on the second track");
        assert_eq!(layered.start_tick, source.start_tick, "the layer drifted off the bars");
        assert_eq!(layered.notes, source.notes, "the layer lost notes");
        assert_eq!(
            app.nav.tracks[first].clips.len(), 1,
            "the paste disturbed the source track"
        );

        // The audio thread was told to build the layer on the second track.
        let second_id = app.nav.tracks[second].mixer_id.unwrap();
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(
                c,
                MixerCommand::CreateClip { track_id, .. } if *track_id == second_id
            )),
            "the audio thread never heard about the layer"
        );
        assert!(
            commands.iter().any(|c| matches!(
                c,
                MixerCommand::UpdateClip { track_id, events, .. }
                    if *track_id == second_id && !events.is_empty()
            )),
            "the layer's notes never reached the audio thread"
        );
    }

    /// Clips cannot overlap — moving, stretching and trimming all stop at
    /// the neighbour, and paste must obey the same law rather than stacking
    /// two clips on the same bars to double-fire every note.
    #[test]
    fn paste_refuses_to_overlap_an_existing_clip() {
        let mut app = app();
        let first = add_synth_track(&mut app);
        app.create_instrument_track(InstrumentType::Juno60);
        let second = app.nav.track_cursor;

        // Both tracks already hold a clip on the same bars.
        app.nav.track_cursor = first;
        record_take(&mut app, first, BAR, vec![note(60, 0)]);
        app.nav.track_cursor = second;
        record_take(&mut app, second, BAR, vec![note(48, 0)]);

        app.nav.track_cursor = first;
        app.yank_clip(0);
        app.nav.track_cursor = second;
        app.paste_clip_to_track();

        assert_eq!(
            app.nav.tracks[second].clips.len(), 1,
            "paste stacked a clip on top of an existing one"
        );
        assert!(
            status(&app).contains("overlap"),
            "the refusal never said why: status was {:?}",
            status(&app)
        );
    }

    /// Duplicating a clip when the next clip sits flush against it has
    /// nowhere to go, and must say so instead of overlapping the neighbour.
    #[test]
    fn duplicate_refuses_when_the_neighbour_is_in_the_way() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        record_take(&mut app, ti, 0, vec![note(60, 0)]);
        record_take(&mut app, ti, BAR, vec![note(64, 0)]);
        assert_eq!(app.nav.tracks[ti].clips.len(), 2, "the fixture needs two clips");

        app.duplicate_clip(0);
        assert_eq!(
            app.nav.tracks[ti].clips.len(), 2,
            "duplicate overlapped the neighbouring clip"
        );
        assert!(
            status(&app).contains("overlap"),
            "the refusal never said why: status was {:?}",
            status(&app)
        );
    }

    /// A duplicate with clear road lands right after its source.
    #[test]
    fn duplicate_lands_after_its_source_when_there_is_room() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        record_take(&mut app, ti, 0, vec![note(60, 0)]);

        app.duplicate_clip(0);
        assert_eq!(app.nav.tracks[ti].clips.len(), 2, "the duplicate never landed");
        assert_eq!(
            app.nav.tracks[ti].clips[1].start_tick, BAR,
            "the duplicate did not land flush after its source"
        );
        assert_eq!(
            app.nav.tracks[ti].clips[1].notes,
            app.nav.tracks[ti].clips[0].notes,
        );
    }

    /// The whole-arrangement journey: three clips recorded across the song,
    /// yanked in one gesture from the track label, laid onto a second track
    /// on exactly their own bars.
    #[test]
    fn an_arrangement_layers_whole_onto_another_track() {
        let mut app = app();
        let first = add_synth_track(&mut app);
        app.create_instrument_track(InstrumentType::Juno60);
        let second = app.nav.track_cursor;

        app.nav.track_cursor = first;
        record_take(&mut app, first, 0, vec![note(60, 0)]);
        record_take(&mut app, first, BAR * 2, vec![note(64, 0)]);
        record_take(&mut app, first, BAR * 4, vec![note(67, 0)]);
        assert_eq!(app.nav.tracks[first].clips.len(), 3, "the fixture needs three clips");

        app.yank_all_clips();
        app.nav.track_cursor = second;
        app.paste_clip_to_track();

        let layered = &app.nav.tracks[second].clips;
        assert_eq!(layered.len(), 3, "the arrangement did not arrive whole");
        let starts: Vec<i64> = layered.iter().map(|c| c.start_tick).collect();
        assert_eq!(starts, vec![0, BAR * 2, BAR * 4], "the arrangement drifted off its bars");

        // And one undo takes the whole layer back off.
        app.perform_undo();
        assert!(
            app.nav.tracks[second].clips.is_empty(),
            "one undo did not lift the whole layer"
        );
        assert_eq!(app.nav.tracks[first].clips.len(), 3);
    }

    /// Half an arrangement is not an arrangement: if any clip is blocked on
    /// the target, nothing lands.
    #[test]
    fn an_arrangement_paste_is_all_or_nothing() {
        let mut app = app();
        let first = add_synth_track(&mut app);
        app.create_instrument_track(InstrumentType::Juno60);
        let second = app.nav.track_cursor;

        app.nav.track_cursor = first;
        record_take(&mut app, first, 0, vec![note(60, 0)]);
        record_take(&mut app, first, BAR * 2, vec![note(64, 0)]);

        // The target already holds something on the second clip's bars.
        app.nav.track_cursor = second;
        record_take(&mut app, second, BAR * 2, vec![note(48, 0)]);

        app.nav.track_cursor = first;
        app.yank_all_clips();
        app.nav.track_cursor = second;
        app.paste_clip_to_track();

        assert_eq!(
            app.nav.tracks[second].clips.len(), 1,
            "a blocked arrangement paste landed partially"
        );
        assert!(status(&app).contains("no room"), "the refusal never said why");
    }

    /// The bus strips carry effects, not clips. A paste aimed at one must
    /// refuse — a clip on a send would draw on screen and never play.
    #[test]
    fn paste_refuses_the_bus_strips() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        record_take(&mut app, ti, 0, vec![note(60, 0)]);
        app.yank_clip(0);

        let bus = app
            .nav
            .tracks
            .iter()
            .position(|t| t.is_bus())
            .expect("the bus strips exist from the start");
        app.nav.track_cursor = bus;
        app.paste_clip_to_track();

        assert!(
            app.nav.tracks[bus].clips.is_empty(),
            "a clip landed on a bus strip"
        );
    }

    // ══════════════════════════════════════════════
    // Fixing the wrong note, and the wrong chord
    // ══════════════════════════════════════════════

    /// The flubbed chord dies in one gesture: highlight its column, press
    /// d, and the melody either side is untouched. One u brings it back.
    #[test]
    fn a_wrong_chord_dies_in_one_gesture() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        // A melody with a three-note chord flubbed at beat 2 of 4.
        record_take(&mut app, ti, 0, vec![
            note(60, 0),
            note(58, 960), note(62, 960), note(65, 960), // the flub
            note(67, 1920),
            note(72, 2880),
        ]);
        app.nav.open_clip_view(ti, 0);
        app.nav.clip_view.piano_roll.total_beats = 4;
        app.nav.clip_view.piano_roll.update_column_count();
        let cols = app.nav.clip_view.piano_roll.column_count;

        // The columns covering beat 2 (fractions 0.25..0.5).
        let from = cols / 4;
        let to = cols / 2 - 1;
        app.delete_selected_notes(Some((from, to)), None);

        let notes = &app.nav.tracks[ti].clips[0].notes;
        assert_eq!(notes.len(), 3, "the gesture deleted the wrong count: {notes:?}");
        assert!(
            notes.iter().all(|n| (n.start_tick - 960).abs() > 38),
            "part of the chord survived"
        );

        app.perform_undo();
        assert_eq!(
            app.nav.tracks[ti].clips[0].notes.len(), 6,
            "undo did not bring the chord back"
        );
    }

    /// The flubbed single note dies under the edit cursor, and the rest of
    /// the take is untouched.
    #[test]
    fn a_wrong_single_note_dies_under_the_cursor() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        record_take(&mut app, ti, 0, vec![
            note(60, 0), note(61, 960), note(67, 1920),
        ]);
        app.nav.open_clip_view(ti, 0);

        // The cursor lands on the flub (index 1: the 61).
        app.nav.clip_view.piano_roll.edit_mode = true;
        app.nav.clip_view.piano_roll.edit_cursor = 1;
        app.edit_delete_cursor_note();

        let notes = &app.nav.tracks[ti].clips[0].notes;
        assert_eq!(notes.len(), 2);
        assert!(notes.iter().all(|n| n.note != 61), "the flub survived");

        app.perform_undo();
        assert_eq!(app.nav.tracks[ti].clips[0].notes.len(), 3, "undo lost the note");
    }

    /// Quantize pulls neighbours onto the same grid line; two copies of one
    /// pitch on one line collapse to the harder hit instead of stacking
    /// into a double-fired note.
    #[test]
    fn quantize_collapses_notes_that_land_together() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        // Two overdub passes of the same hat, a few ticks apart, plus an
        // innocent bystander on another pitch.
        record_take(&mut app, ti, 0, vec![
            NoteSnapshot { note: 42, velocity: 60, start_tick: 922, duration_ticks: 77, muted: false },
            NoteSnapshot { note: 42, velocity: 110, start_tick: 998, duration_ticks: 77, muted: false },
            NoteSnapshot { note: 60, velocity: 100, start_tick: 1920, duration_ticks: 384, muted: false },
        ]);
        app.nav.open_clip_view(ti, 0);
        app.nav.clip_view.piano_roll.total_beats = 4;
        app.nav.clip_view.piano_roll.update_column_count();

        app.apply_quantize(GridResolution::Quarter, 100);

        let notes = &app.nav.tracks[ti].clips[0].notes;
        let hats: Vec<_> = notes.iter().filter(|n| n.note == 42).collect();
        assert_eq!(hats.len(), 1, "the doubled hat survived quantize: {notes:?}");
        assert_eq!(hats[0].velocity, 110, "the softer hit won");
        assert_eq!(notes.len(), 2, "the bystander was harmed");
    }

    // ══════════════════════════════════════════════
    // Recording on several tracks
    // ══════════════════════════════════════════════

    /// Recording on a second track leaves the first track's take exactly as
    /// it was, and each track's take undoes independently, newest first.
    #[test]
    fn takes_on_two_tracks_stay_apart() {
        let mut app = app();
        let first = add_synth_track(&mut app);
        app.create_instrument_track(InstrumentType::Juno60);
        let second = app.nav.track_cursor;

        app.nav.track_cursor = first;
        record_take(&mut app, first, 0, vec![note(60, 0)]);
        app.nav.track_cursor = second;
        record_take(&mut app, second, 0, vec![note(48, 0), note(52, 1920)]);

        assert_eq!(app.nav.tracks[first].clips[0].notes.len(), 1);
        assert_eq!(app.nav.tracks[second].clips[0].notes.len(), 2);

        // Undo peels the second track's take first, the first track's next.
        app.perform_undo();
        assert!(app.nav.tracks[second].clips.is_empty(), "the newer take survived its undo");
        assert_eq!(
            app.nav.tracks[first].clips[0].notes.len(), 1,
            "undoing one track's take disturbed the other track"
        );
        app.perform_undo();
        assert!(app.nav.tracks[first].clips.is_empty(), "the older take survived its undo");
    }

    /// Recording over half an existing clip merges the two into one clip
    /// covering both — the timeline never holds two clips on the same bars,
    /// and no note from either side is lost.
    #[test]
    fn a_take_overlapping_half_a_clip_merges_into_one() {
        let mut app = app();
        let ti = add_synth_track(&mut app);

        // An older part sits at bars 3–7 (ticks 2*BAR .. 6*BAR).
        app.nav.tracks[ti].clips.push(Clip {
            number: 1,
            width: 16,
            has_content: true,
            start_tick: BAR * 2,
            length_ticks: BAR * 4,
            notes: vec![note(72, 0), note(74, 7680)],
            hidden_notes: Vec::new(),
            controls: Vec::new(),
        });

        // A new take is recorded over bars 1–5 (ticks 0 .. 4*BAR).
        app.engine.transport.set_loop_bars(1, 4);
        app.engine.transport.start_loop_record();
        let snap = ClipSnapshot {
            track_id: app.nav.tracks[ti].mixer_id.unwrap(),
            clip_index: 0,
            start_tick: 0,
            length_ticks: BAR * 4,
            event_count: 2,
            notes: vec![note(60, 0)],
            controls: Vec::new(),
        };
        app.nav.receive_clip_snapshot(snap, true);
        app.engine.transport.stop_loop_record();

        let clips = &app.nav.tracks[ti].clips;
        assert_eq!(clips.len(), 1, "the take left two clips on the same bars: {clips:?}");
        let merged = &clips[0];
        assert_eq!(merged.start_tick, 0, "the merge lost the take's opening bars");
        assert_eq!(merged.length_ticks, BAR * 6, "the merge lost the old part's tail");
        assert_eq!(merged.notes.len(), 3, "a note was lost in the merge");

        // Every note sits on its original absolute bars.
        let abs: Vec<i64> = merged
            .notes
            .iter()
            .map(|n| merged.start_tick + n.start_tick)
            .collect();
        assert!(abs.contains(&0), "the take's note moved: {abs:?}");
        assert!(abs.contains(&(BAR * 2)), "the old part's first note moved: {abs:?}");
        assert!(abs.contains(&(BAR * 4)), "the old part's second note moved: {abs:?}");
    }

    /// A take recorded mid-song lands on the bars it was played on, not at
    /// bar one.
    #[test]
    fn a_take_recorded_mid_song_lands_where_it_was_played() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        record_take(&mut app, ti, BAR * 4, vec![note(60, 0)]);
        assert_eq!(
            app.nav.tracks[ti].clips[0].start_tick,
            BAR * 4,
            "the take drifted to the wrong bars"
        );
    }

    // ══════════════════════════════════════════════
    // Record quantize, re-record, punch-in
    // ══════════════════════════════════════════════

    /// With record quantize set, a take's notes land on the grid as they
    /// commit — and two hits pulled onto one line collapse to the harder.
    #[test]
    fn record_quantize_snaps_the_take_on_the_way_in() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        app.nav.clip_view.piano_roll.record_quantize = Some(GridResolution::Quarter);

        app.engine.transport.set_loop_bars(1, 1);
        app.engine.transport.start_loop_record();
        let snap = ClipSnapshot {
            track_id: app.nav.tracks[ti].mixer_id.unwrap(),
            clip_index: 0,
            start_tick: 0,
            length_ticks: BAR,
            event_count: 6,
            notes: vec![
                NoteSnapshot { note: 60, velocity: 90, start_tick: 77, duration_ticks: 192, muted: false },
                NoteSnapshot { note: 60, velocity: 120, start_tick: 883, duration_ticks: 192, muted: false },
                NoteSnapshot { note: 60, velocity: 40, start_tick: 1037, duration_ticks: 192, muted: false },
            ],
            controls: Vec::new(),
        };
        app.nav.receive_clip_snapshot(snap, true);
        app.engine.transport.stop_loop_record();

        let notes = &app.nav.tracks[ti].clips[0].notes;
        assert!(
            notes.iter().all(|n| n.start_tick % 960 == 0),
            "a note missed the grid: {notes:?}"
        );
        // The two hits near beat 2 collapsed to the harder one.
        let at_q2: Vec<_> = notes.iter().filter(|n| n.start_tick == 960).collect();
        assert_eq!(at_q2.len(), 1, "the pulled-together hits did not collapse");
        assert_eq!(at_q2[0].velocity, 120, "the softer hit won");

        // Off means untouched.
        app.nav.clip_view.piano_roll.record_quantize = None;
        record_take(&mut app, ti, BAR * 2, vec![
            NoteSnapshot { note: 62, velocity: 90, start_tick: 77, duration_ticks: 192, muted: false },
        ]);
        let free = app.nav.tracks[ti].clips.iter()
            .flat_map(|c| c.notes.iter())
            .find(|n| n.note == 62)
            .unwrap();
        assert_eq!(free.start_tick, 77, "an unquantized take was moved");
    }

    /// Re-record mode: R clears the loop range on the armed track first —
    /// as an undo step — and overdub mode leaves it alone.
    #[test]
    fn re_record_clears_the_range_and_undoes() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        record_take(&mut app, ti, 0, vec![note(60, 0)]);
        assert_eq!(app.nav.tracks[ti].clips.len(), 1);

        app.engine.transport.set_loop_bars(1, 1);
        app.nav.loop_editor.start_bar = 1;
        app.nav.loop_editor.end_bar = 2;

        // Overdub (default): R keeps the clip.
        app.toggle_loop_record();
        assert_eq!(app.nav.tracks[ti].clips.len(), 1, "overdub cleared the range");
        app.toggle_loop_record();

        // Re-record: R clears it.
        app.nav.record_replace = true;
        app.toggle_loop_record();
        assert!(app.nav.tracks[ti].clips.is_empty(), "re-record left the old clip");
        app.toggle_loop_record();

        // And the clear is one undo step away from coming back.
        app.perform_undo();
        assert_eq!(app.nav.tracks[ti].clips.len(), 1, "undo did not restore the cleared clip");
    }

    /// R while the transport is already rolling punches in where it is: no
    /// rewind, no count-in, recording just joins the music.
    #[test]
    fn r_while_playing_punches_in_without_rewinding() {
        let mut app = app();
        add_synth_track(&mut app);
        app.engine.transport.set_count_in_bars(2); // must be ignored mid-roll
        app.engine.transport.play();
        app.engine.transport.set_position(BAR * 2);

        app.toggle_loop_record();
        assert!(app.engine.transport.is_recording(), "punch-in did not arm");
        assert!(app.engine.transport.is_playing(), "punch-in stopped the music");
        assert!(!app.engine.transport.is_counting_in(), "punch-in counted in mid-roll");
        assert_eq!(
            app.engine.transport.position_ticks(),
            BAR * 2,
            "punch-in rewound the playhead"
        );
    }

    /// The take counter climbs as passes commit and starts fresh with the
    /// next recording.
    #[test]
    fn the_take_counter_counts_passes() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        app.engine.transport.set_loop_bars(1, 1);
        app.engine.transport.start_loop_record();
        app.nav.take_count = 0;
        // Two passes commit — counted where the main loop counts them.
        for k in 0..2 {
            let snap = take(&app, ti, 0, vec![note(60 + k, 0)]);
            app.nav.receive_clip_snapshot(snap, true);
            app.nav.take_count += 1; // the main loop's increment, mirrored here
        }
        assert_eq!(app.nav.take_count, 2);
        app.engine.transport.stop_loop_record();
        app.toggle_loop_record(); // next recording starts at zero
        assert_eq!(app.nav.take_count, 0, "the counter did not reset");
        app.toggle_loop_record();
    }

    // ══════════════════════════════════════════════
    // Recorded controllers (automation)
    // ══════════════════════════════════════════════

    use phosphor_core::clip::ClipEvent;

    fn take_with_controls(
        app: &App,
        track_idx: usize,
        notes: Vec<NoteSnapshot>,
        controls: Vec<ClipEvent>,
    ) -> ClipSnapshot {
        ClipSnapshot {
            track_id: app.nav.tracks[track_idx].mixer_id.expect("instrument track"),
            clip_index: 0,
            start_tick: 0,
            length_ticks: BAR,
            event_count: notes.len() * 2 + controls.len(),
            notes,
            controls,
        }
    }

    fn cc(tick: i64, controller: u8, value: u8) -> ClipEvent {
        ClipEvent { tick, status: 0xB0, data1: controller, data2: value }
    }

    /// The regression the whole feature exists to kill: a recorded wheel
    /// sweep must survive the first note edit instead of being silently
    /// erased when the clip's events are rebuilt.
    #[test]
    fn a_recorded_sweep_survives_a_note_edit() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        app.engine.transport.set_loop_bars(1, 1);
        app.engine.transport.start_loop_record();
        let snap = take_with_controls(
            &app, ti,
            vec![note(60, 0), note(64, 1920)],
            vec![cc(100, 1, 40), cc(500, 1, 90)],
        );
        app.nav.receive_clip_snapshot(snap, true);
        app.engine.transport.stop_loop_record();
        assert_eq!(app.nav.tracks[ti].clips[0].controls.len(), 2, "the sweep never landed");

        // The first edit after the take: delete a note.
        app.nav.open_clip_view(ti, 0);
        app.nav.clip_view.piano_roll.edit_mode = true;
        app.nav.clip_view.piano_roll.edit_cursor = 0;
        let _ = app.drain_mixer_commands();
        app.edit_delete_cursor_note();

        assert_eq!(
            app.nav.tracks[ti].clips[0].controls.len(), 2,
            "the note edit erased the recorded sweep"
        );
        // And what went to the audio thread still carries it.
        let commands = app.drain_mixer_commands();
        let sent_sweep = commands.iter().any(|c| matches!(
            c,
            MixerCommand::UpdateClip { events, .. }
                if events.iter().any(|e| e.status & 0xF0 == 0xB0)
        ));
        assert!(sent_sweep, "the rebuilt clip reached the audio thread without the sweep");

        // Undo of the note edit keeps the sweep too.
        app.perform_undo();
        assert_eq!(app.nav.tracks[ti].clips[0].controls.len(), 2);
    }

    /// Overdubbing a second pass keeps the first pass's controllers on
    /// their bars, and a layered paste carries them to the other track.
    #[test]
    fn controllers_ride_through_overdub_and_paste() {
        let mut app = app();
        let first = add_synth_track(&mut app);
        app.create_instrument_track(InstrumentType::Juno60);
        let second = app.nav.track_cursor;

        app.nav.track_cursor = first;
        app.engine.transport.set_loop_bars(1, 1);
        app.engine.transport.start_loop_record();
        let pass_one = take_with_controls(&app, first, vec![note(60, 0)], vec![cc(200, 1, 80)]);
        app.nav.receive_clip_snapshot(pass_one, true);
        let pass_two = take_with_controls(&app, first, vec![note(64, 1920)], vec![]);
        app.nav.receive_clip_snapshot(pass_two, true);
        app.engine.transport.stop_loop_record();

        let clip = &app.nav.tracks[first].clips[0];
        assert_eq!(clip.notes.len(), 2, "the overdub lost a note");
        assert_eq!(clip.controls.len(), 1, "the overdub lost the sweep");
        assert_eq!(clip.controls[0].tick, 200, "the sweep drifted in the merge");

        app.yank_clip(0);
        app.nav.track_cursor = second;
        app.paste_clip_to_track();
        assert_eq!(
            app.nav.tracks[second].clips[0].controls.len(), 1,
            "the layer arrived without its sweep"
        );
    }

    /// X wipes a flubbed sweep from the viewed clip; u puts it back.
    #[test]
    fn a_flubbed_sweep_clears_and_undoes() {
        let mut app = app();
        let ti = add_synth_track(&mut app);
        record_take(&mut app, ti, 0, vec![note(60, 0)]);
        app.nav.tracks[ti].clips[0].controls = vec![cc(100, 1, 30), cc(300, 1, 90)];

        app.nav.open_clip_view(ti, 0);
        app.clear_clip_controls();
        assert!(app.nav.tracks[ti].clips[0].controls.is_empty(), "X did not clear");
        assert!(status(&app).contains("2 controller"), "the clear did not say what it found");

        app.perform_undo();
        assert_eq!(
            app.nav.tracks[ti].clips[0].controls.len(), 2,
            "undo did not restore the sweep"
        );
    }

    /// Controllers survive the session file.
    #[test]
    fn controllers_survive_the_session() {
        let dir = std::env::temp_dir().join("phosphor_controls_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("controls.phos");
        let path_str = path.to_str().unwrap();

        let mut saving = app();
        let ti = add_synth_track(&mut saving);
        record_take(&mut saving, ti, 0, vec![note(60, 0)]);
        saving.nav.tracks[ti].clips[0].controls = vec![cc(240, 1, 64)];
        saving.do_save(path_str);

        let mut loading = app();
        loading.do_load(path_str);
        let track = loading
            .nav
            .tracks
            .iter()
            .find(|t| t.instrument_type.is_some())
            .expect("the track loaded");
        assert_eq!(track.clips[0].controls.len(), 1, "the sweep was lost in the file");
        assert_eq!(track.clips[0].controls[0], cc(240, 1, 64));
        let _ = std::fs::remove_file(&path);
    }

    // ══════════════════════════════════════════════
    // Count-in
    // ══════════════════════════════════════════════

    /// The count-in fires on exactly three conditions together: the setting
    /// on, the record switch armed, play pressed. Any two alone behave as
    /// they always did.
    #[test]
    fn count_in_needs_all_three_conditions() {
        let mut app = app();
        add_synth_track(&mut app);

        // Setting on, record NOT armed, play → plays immediately, no count.
        app.engine.transport.set_count_in_bars(1);
        app.toggle_play_pause();
        assert!(app.engine.transport.is_playing(), "plain play stopped playing");
        assert!(!app.engine.transport.is_counting_in(), "play without record counted in");
        app.toggle_play_pause(); // stop

        // Record armed, setting OFF → rolls immediately, as it always did.
        app.engine.transport.set_count_in_bars(0);
        app.engine.transport.toggle_record();
        app.toggle_play_pause();
        assert!(app.engine.transport.is_playing(), "armed play without count-in did not roll");
        assert!(!app.engine.transport.is_counting_in());
        app.toggle_play_pause(); // stop
        if app.engine.transport.is_recording() {
            app.engine.transport.toggle_record();
        }

        // All three: setting on, record armed, play pressed → the countdown
        // runs and nothing rolls yet.
        app.engine.transport.set_count_in_bars(1);
        app.engine.transport.toggle_record();
        app.toggle_play_pause();
        assert!(app.engine.transport.is_counting_in(), "all three conditions did not count in");
        assert!(!app.engine.transport.is_playing(), "the countdown started playback early");

        // A second press backs out of it.
        app.toggle_play_pause();
        assert!(!app.engine.transport.is_counting_in(), "the second press did not cancel");
        assert!(!app.engine.transport.is_playing());
    }

    /// R is record intent and play in one press, so with the setting on it
    /// counts in too — and ends in the full loop-record start.
    #[test]
    fn r_with_count_in_counts_before_the_take() {
        let mut app = app();
        add_synth_track(&mut app);
        app.engine.transport.set_count_in_bars(2);

        app.toggle_loop_record();
        assert!(app.engine.transport.is_counting_in(), "R with count-in set did not count");
        assert!(!app.engine.transport.is_playing());
        assert!(!app.engine.transport.is_recording());

        // R again backs out.
        app.toggle_loop_record();
        assert!(!app.engine.transport.is_counting_in());

        // And with the setting off, R rolls immediately as it always did.
        app.engine.transport.set_count_in_bars(0);
        app.toggle_loop_record();
        assert!(app.engine.transport.is_recording() && app.engine.transport.is_playing());
        app.toggle_loop_record();
    }

    /// The setting survives a save and a load, like the metronome switch.
    #[test]
    fn count_in_setting_survives_the_session() {
        let dir = std::env::temp_dir().join("phosphor_count_in_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("count_in.phos");
        let path_str = path.to_str().unwrap();

        let mut saving = app();
        add_synth_track(&mut saving);
        saving.engine.transport.set_count_in_bars(2);
        saving.do_save(path_str);

        let mut loading = app();
        loading.do_load(path_str);
        assert_eq!(
            loading.engine.transport.count_in_bars(), 2,
            "the count-in setting was lost in the file"
        );
        let _ = std::fs::remove_file(&path);
    }

    // ══════════════════════════════════════════════
    // Notes travel between clips and tracks
    // ══════════════════════════════════════════════

    /// Yanked notes paste into another track's clip — the note-level way to
    /// carry a phrase across instruments.
    #[test]
    fn yanked_notes_paste_into_another_tracks_clip() {
        let mut app = app();
        let first = add_synth_track(&mut app);
        app.create_instrument_track(InstrumentType::Juno60);
        let second = app.nav.track_cursor;

        app.nav.track_cursor = first;
        record_take(&mut app, first, 0, vec![note(60, 0), note(64, 960)]);
        app.nav.track_cursor = second;
        record_take(&mut app, second, 0, vec![]);
        // An empty take commits nothing, so give the target an empty clip
        // the way drawing the first note would.
        if app.nav.tracks[second].clips.is_empty() {
            app.nav.tracks[second].clips.push(Clip {
                number: 1, width: 4, has_content: false,
                start_tick: 0, length_ticks: BAR,
                notes: Vec::new(), hidden_notes: Vec::new(),
                controls: Vec::new(),
            });
        }

        // Yank the phrase from the first track's clip.
        app.nav.open_clip_view(first, 0);
        app.nav.clip_view.piano_roll.total_beats = 4;
        app.nav.clip_view.piano_roll.update_column_count();
        let cols = app.nav.clip_view.piano_roll.column_count;
        app.yank_selected_notes(Some((0, cols - 1)), None);

        // Paste it into the second track's clip.
        app.nav.open_clip_view(second, 0);
        app.nav.clip_view.piano_roll.total_beats = 4;
        app.nav.clip_view.piano_roll.update_column_count();
        app.paste_selected_notes(0, None);

        assert_eq!(
            app.nav.tracks[second].clips[0].notes.len(), 2,
            "the phrase did not arrive on the second track"
        );
        assert_eq!(
            app.nav.tracks[first].clips[0].notes.len(), 2,
            "yank stole the notes instead of copying them"
        );
    }
}
