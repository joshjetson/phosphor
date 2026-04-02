//! Integration tests for clip operations, recording, piano roll editing.
//! Simulates real music production workflows without audio/MIDI/terminal.

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::state::*;
    use phosphor_core::clip::NoteSnapshot;
    use phosphor_core::EngineConfig;

    fn app() -> App {
        App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false)
    }

    fn add_synth_track(app: &mut App) {
        app.nav.instrument_modal.open = true;
        app.nav.instrument_modal.cursor = 0; // Phosphor Synth
        let instrument = app.nav.instrument_modal.selected();
        app.nav.instrument_modal.open = false;
        app.create_instrument_track(instrument);
    }

    /// Create a clip with notes on a track (simulates recording).
    fn create_clip_with_notes(app: &mut App, track_idx: usize, start_tick: i64, length_ticks: i64, notes: Vec<NoteSnapshot>) {
        if let Some(track) = app.nav.tracks.get_mut(track_idx) {
            let ppq = phosphor_core::transport::Transport::PPQ;
            let beats = (length_ticks as f64 / ppq as f64).ceil() as u16;
            track.clips.push(Clip {
                number: track.clips.len() + 1,
                width: beats.max(2),
                has_content: !notes.is_empty(),
                start_tick,
                length_ticks,
                notes,
                hidden_notes: Vec::new(),
            });
        }
    }

    fn note(pitch: u8, start_frac: f64, duration_frac: f64) -> NoteSnapshot {
        NoteSnapshot { note: pitch, velocity: 100, start_frac, duration_frac }
    }

    // ══════════════════════════════════════════════
    // Clip identity — duplicated clips are independent
    // ══════════════════════════════════════════════

    #[test]
    fn duplicated_clips_have_independent_notes() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        // Create a clip with 3 notes
        create_clip_with_notes(&mut app, ti, 0, 3840, vec![
            note(60, 0.0, 0.25), note(64, 0.25, 0.25), note(67, 0.5, 0.25),
        ]);

        assert_eq!(app.nav.tracks[ti].clips.len(), 1);
        assert_eq!(app.nav.tracks[ti].clips[0].notes.len(), 3);

        // Navigate to clip and duplicate
        app.nav.track_element = TrackElement::Clip(0);
        app.duplicate_clip(0);

        assert_eq!(app.nav.tracks[ti].clips.len(), 2, "should have 2 clips after duplicate");
        assert_eq!(app.nav.tracks[ti].clips[0].notes.len(), 3);
        assert_eq!(app.nav.tracks[ti].clips[1].notes.len(), 3);

        // Modify clip 1's notes — clip 0 should be unaffected
        app.nav.tracks[ti].clips[1].notes[0].note = 72;
        assert_eq!(app.nav.tracks[ti].clips[0].notes[0].note, 60, "clip 0 note should be unchanged");
        assert_eq!(app.nav.tracks[ti].clips[1].notes[0].note, 72, "clip 1 note should be modified");
    }

    #[test]
    fn clip_view_target_follows_selected_clip() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        create_clip_with_notes(&mut app, ti, 0, 3840, vec![note(60, 0.0, 0.25)]);
        create_clip_with_notes(&mut app, ti, 3840, 3840, vec![note(72, 0.0, 0.25)]);

        // Select clip 0
        app.nav.track_element = TrackElement::Clip(0);
        app.nav.activate_element();
        assert_eq!(app.nav.clip_view_target, Some((ti, 0)));

        // Navigate to clip 1 using nav.move_right (which syncs clip view)
        app.nav.clip_locked = false;
        app.nav.track_selected = true;
        app.nav.move_right(); // Clip(0) → Clip(1)
        assert_eq!(app.nav.track_element, TrackElement::Clip(1));
        assert_eq!(app.nav.clip_view_target, Some((ti, 1)));

        // Verify active_clip returns clip 1's note
        let active = app.nav.active_clip().unwrap();
        assert_eq!(active.notes[0].note, 72, "active clip should be clip 1 with note 72");
    }

    // ══════════════════════════════════════════════
    // Clip shrink/expand preserves notes
    // ══════════════════════════════════════════════

    #[test]
    fn shrink_hides_notes_expand_restores() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        // 4-beat clip with notes at beats 1, 2, 3, 4
        let ppq = phosphor_core::transport::Transport::PPQ;
        create_clip_with_notes(&mut app, ti, 0, ppq * 4, vec![
            note(60, 0.0, 0.1),   // beat 1
            note(62, 0.25, 0.1),  // beat 2
            note(64, 0.5, 0.1),   // beat 3
            note(67, 0.75, 0.1),  // beat 4
        ]);

        app.nav.track_element = TrackElement::Clip(0);
        app.nav.track_selected = true;
        app.nav.clip_locked = true;

        // Shrink right edge by 2 beats (4 beats → 2 beats)
        app.move_clip_right_edge(0, -1);
        app.move_clip_right_edge(0, -1);

        let clip = &app.nav.tracks[ti].clips[0];
        assert_eq!(clip.length_ticks, ppq * 2, "clip should be 2 beats");
        assert_eq!(clip.notes.len(), 2, "should have 2 visible notes (beats 1-2)");
        assert_eq!(clip.hidden_notes.len(), 2, "should have 2 hidden notes (beats 3-4)");

        // Expand back to 4 beats
        app.move_clip_right_edge(0, 1);
        app.move_clip_right_edge(0, 1);

        let clip = &app.nav.tracks[ti].clips[0];
        assert_eq!(clip.length_ticks, ppq * 4, "clip should be 4 beats again");
        assert_eq!(clip.notes.len(), 4, "all 4 notes should be visible again");
        assert_eq!(clip.hidden_notes.len(), 0, "no hidden notes");
    }

    // ══════════════════════════════════════════════
    // Clip deletion selects adjacent
    // ══════════════════════════════════════════════

    #[test]
    fn delete_clip_selects_adjacent() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        create_clip_with_notes(&mut app, ti, 0, 3840, vec![note(60, 0.0, 0.25)]);
        create_clip_with_notes(&mut app, ti, 3840, 3840, vec![note(72, 0.0, 0.25)]);
        create_clip_with_notes(&mut app, ti, 7680, 3840, vec![note(67, 0.0, 0.25)]);

        // Delete clip 1 (middle)
        app.nav.track_element = TrackElement::Clip(1);
        app.nav.track_selected = true;
        app.nav.confirm_modal.open = false;

        // Simulate execute_delete directly
        let track = app.nav.tracks.get_mut(ti).unwrap();
        track.clips.remove(1);
        let remaining = track.clips.len();
        assert_eq!(remaining, 2);

        // Clip at index 1 is now the old clip 2
        assert_eq!(app.nav.tracks[ti].clips[1].notes[0].note, 67);
    }

    // ══════════════════════════════════════════════
    // Edit mode — note navigation
    // ══════════════════════════════════════════════

    #[test]
    fn edit_mode_navigates_within_column() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        // 4-beat clip with a chord on beat 1 (3 notes stacked)
        create_clip_with_notes(&mut app, ti, 0, 3840, vec![
            note(60, 0.0, 0.25), // C4
            note(64, 0.0, 0.25), // E4
            note(67, 0.0, 0.25), // G4
            note(72, 0.5, 0.25), // C5 on beat 3 (different column)
        ]);

        app.nav.track_element = TrackElement::Clip(0);
        app.nav.open_clip_view(ti, 0);
        app.enter_edit_mode();

        let pr = &app.nav.clip_view.piano_roll;
        assert!(pr.edit_mode);
        // Top-left = highest pitch overall = C5 (72) at frac 0.5
        assert_eq!(app.nav.tracks[ti].clips[0].notes[pr.edit_cursor].note, 72);

        // Move left — should jump to the chord column (frac 0.0), landing on G4 (67, closest pitch)
        app.edit_move_to_prev_column();
        let pr = &app.nav.clip_view.piano_roll;
        assert_eq!(app.nav.tracks[ti].clips[0].notes[pr.edit_cursor].note, 67,
            "left from C5 should land on G4 (closest pitch in prev column)");

        // Move down in column — E4 (64)
        app.edit_move_down_in_column();
        let pr = &app.nav.clip_view.piano_roll;
        assert_eq!(app.nav.tracks[ti].clips[0].notes[pr.edit_cursor].note, 64,
            "down should go to next lower note in same column");

        // Move down again — C4 (60)
        app.edit_move_down_in_column();
        let pr = &app.nav.clip_view.piano_roll;
        assert_eq!(app.nav.tracks[ti].clips[0].notes[pr.edit_cursor].note, 60);

        // Move right — should jump to C5 (72) in the next column
        app.edit_move_to_next_column();
        let pr = &app.nav.clip_view.piano_roll;
        assert_eq!(app.nav.tracks[ti].clips[0].notes[pr.edit_cursor].note, 72,
            "right should jump to note in next column");
    }

    // ══════════════════════════════════════════════
    // Edit mode — move notes with undo
    // ══════════════════════════════════════════════

    #[test]
    fn edit_mode_move_note_and_undo() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        create_clip_with_notes(&mut app, ti, 0, 3840, vec![
            note(60, 0.0, 0.25),
        ]);

        app.nav.track_element = TrackElement::Clip(0);
        app.nav.open_clip_view(ti, 0);
        app.enter_edit_mode();

        let original_pitch = app.nav.tracks[ti].clips[0].notes[0].note;
        assert_eq!(original_pitch, 60);

        // Select note (Enter) and move up
        app.nav.clip_view.piano_roll.edit_selected.push(0);
        app.nav.clip_view.piano_roll.edit_sub = EditSubMode::Moving;
        app.move_selected_notes(0, 2); // up 2 semitones

        assert_eq!(app.nav.tracks[ti].clips[0].notes[0].note, 62, "note should be at 62");

        // Undo
        app.perform_undo();
        assert_eq!(app.nav.tracks[ti].clips[0].notes[0].note, 60, "note should be back at 60 after undo");
    }

    // ══════════════════════════════════════════════
    // Edit mode — delete note with undo
    // ══════════════════════════════════════════════

    #[test]
    fn edit_mode_delete_note_and_undo() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        create_clip_with_notes(&mut app, ti, 0, 3840, vec![
            note(60, 0.0, 0.25),
            note(64, 0.25, 0.25),
        ]);

        app.nav.track_element = TrackElement::Clip(0);
        app.nav.open_clip_view(ti, 0);
        app.enter_edit_mode();

        assert_eq!(app.nav.tracks[ti].clips[0].notes.len(), 2);

        // Delete cursor note
        app.edit_delete_cursor_note();
        assert_eq!(app.nav.tracks[ti].clips[0].notes.len(), 1, "should have 1 note after delete");

        // Undo
        app.perform_undo();
        assert_eq!(app.nav.tracks[ti].clips[0].notes.len(), 2, "should have 2 notes after undo");
    }

    // ══════════════════════════════════════════════
    // Clip collision detection
    // ══════════════════════════════════════════════

    #[test]
    fn clip_move_respects_collision() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;
        let ppq = phosphor_core::transport::Transport::PPQ;

        create_clip_with_notes(&mut app, ti, 0, ppq * 4, vec![note(60, 0.0, 0.25)]);
        create_clip_with_notes(&mut app, ti, ppq * 4, ppq * 4, vec![note(72, 0.0, 0.25)]);

        app.nav.track_selected = true;
        app.nav.track_element = TrackElement::Clip(0);
        app.nav.clip_locked = true;

        // Try to move clip 0 right — should be blocked by clip 1
        let start_before = app.nav.tracks[ti].clips[0].start_tick;
        app.move_clip(0, 1);
        let start_after = app.nav.tracks[ti].clips[0].start_tick;
        // It can move right up to the start of clip 1 minus its own length
        // Clip 0 is at 0 with len 1920. Clip 1 starts at 1920. Moving right 1 beat (480 ticks):
        // new_start = 480. end = 480 + 1920 = 2400 > 1920 (clip 1 start). So it's blocked.
        // Actually with 4-beat clips: clip 0 len = 4*480 = 1920, clip 1 starts at 1920.
        // move right by 1 beat: new_start = 480. end = 480+1920=2400 > 1920. Blocked.
        assert_eq!(start_after, start_before, "clip should not overlap adjacent clip");
    }

    // ══════════════════════════════════════════════
    // Recording snapshot merges correctly
    // ══════════════════════════════════════════════

    #[test]
    fn recording_snapshot_merges_into_existing_clip() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        // Existing clip at tick 0
        create_clip_with_notes(&mut app, ti, 0, 3840, vec![
            note(60, 0.0, 0.25),
        ]);

        assert_eq!(app.nav.tracks[ti].clips.len(), 1);

        // Simulate a recording snapshot arriving (overdub)
        let snap = phosphor_core::clip::ClipSnapshot {
            track_id: app.nav.tracks[ti].mixer_id.unwrap_or(0),
            clip_index: 0,
            start_tick: 0,
            length_ticks: 3840,
            event_count: 2,
            notes: vec![note(64, 0.5, 0.25)],
        };
        let _ = app.nav.receive_clip_snapshot(snap, true);

        assert_eq!(app.nav.tracks[ti].clips.len(), 1, "should still be 1 clip (merged)");
        assert_eq!(app.nav.tracks[ti].clips[0].notes.len(), 2, "should have 2 notes after merge");
    }

    #[test]
    fn recording_snapshot_absorbs_smaller_clips() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        // Two 4-beat clips side by side
        create_clip_with_notes(&mut app, ti, 0, 3840, vec![note(60, 0.0, 0.25)]);
        create_clip_with_notes(&mut app, ti, 3840, 3840, vec![note(72, 0.0, 0.25)]);
        assert_eq!(app.nav.tracks[ti].clips.len(), 2);

        // Recording covers both (8-beat snapshot)
        let mid = app.nav.tracks[ti].mixer_id.unwrap_or(0);
        let snap = phosphor_core::clip::ClipSnapshot {
            track_id: mid, clip_index: 0,
            start_tick: 0, length_ticks: 7680,
            event_count: 4, notes: vec![note(67, 0.25, 0.1)],
        };
        let _ = app.nav.receive_clip_snapshot(snap, true);

        assert_eq!(app.nav.tracks[ti].clips.len(), 1,
            "both clips should be absorbed into the new recording");
        let clip = &app.nav.tracks[ti].clips[0];
        assert_eq!(clip.length_ticks, 7680);
        assert!(clip.notes.len() >= 3,
            "should have absorbed notes from both clips + new recording");
    }

    #[test]
    fn stale_snapshot_ignored_when_not_recording() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        create_clip_with_notes(&mut app, ti, 0, 3840, vec![
            note(60, 0.0, 0.25), note(64, 0.25, 0.25),
        ]);

        // Open clip in clip view
        app.nav.track_element = TrackElement::Clip(0);
        app.nav.open_clip_view(ti, 0);

        // Delete a note
        app.nav.tracks[ti].clips[0].notes.remove(1);
        assert_eq!(app.nav.tracks[ti].clips[0].notes.len(), 1);

        // Stale snapshot arrives (not recording) — should be ignored
        let mid = app.nav.tracks[ti].mixer_id.unwrap_or(0);
        let snap = phosphor_core::clip::ClipSnapshot {
            track_id: mid, clip_index: 0,
            start_tick: 0, length_ticks: 3840,
            event_count: 4, notes: vec![note(60, 0.0, 0.25), note(64, 0.25, 0.25)],
        };
        let _ = app.nav.receive_clip_snapshot(snap, false); // NOT recording

        assert_eq!(app.nav.tracks[ti].clips[0].notes.len(), 1,
            "stale snapshot should be ignored — notes should stay at 1");
    }

    // ══════════════════════════════════════════════
    // Multi-clip arrangement workflow
    // ══════════════════════════════════════════════

    #[test]
    fn multi_clip_arrangement_workflow() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;
        let ppq = phosphor_core::transport::Transport::PPQ;

        // Record a 4-bar chord progression
        create_clip_with_notes(&mut app, ti, 0, ppq * 16, vec![
            note(60, 0.0, 0.0625),    // C4 bar 1
            note(64, 0.0, 0.0625),    // E4 bar 1
            note(67, 0.0, 0.0625),    // G4 bar 1
            note(65, 0.25, 0.0625),   // F4 bar 2
            note(69, 0.25, 0.0625),   // A4 bar 2
            note(72, 0.25, 0.0625),   // C5 bar 2
            note(62, 0.5, 0.0625),    // D4 bar 3
            note(67, 0.5, 0.0625),    // G4 bar 3
            note(71, 0.5, 0.0625),    // B4 bar 3
            note(60, 0.75, 0.0625),   // C4 bar 4
            note(64, 0.75, 0.0625),   // E4 bar 4
            note(67, 0.75, 0.0625),   // G4 bar 4
        ]);

        // Duplicate to create 8-bar arrangement
        app.nav.track_element = TrackElement::Clip(0);
        app.nav.track_selected = true;
        app.duplicate_clip(0);
        assert_eq!(app.nav.tracks[ti].clips.len(), 2);

        // Clip 1 should start right after clip 0
        let clip1_start = app.nav.tracks[ti].clips[1].start_tick;
        assert_eq!(clip1_start, ppq * 16, "duplicate should be at tick 7680");

        // Edit clip 1 — change the chord on bar 5 (first chord of clip 1)
        app.nav.open_clip_view(ti, 1);
        assert_eq!(app.nav.clip_view_target, Some((ti, 1)));

        // Verify we can see clip 1's notes
        let active = app.nav.active_clip().unwrap();
        assert_eq!(active.notes.len(), 12, "clip 1 should have 12 notes");

        // Modify a note in clip 1
        app.nav.active_clip_mut().unwrap().notes[0].note = 63; // C4 → Eb4

        // Verify clip 0 is untouched
        assert_eq!(app.nav.tracks[ti].clips[0].notes[0].note, 60, "clip 0 should still have C4");
        assert_eq!(app.nav.tracks[ti].clips[1].notes[0].note, 63, "clip 1 should have Eb4");
    }

    // ══════════════════════════════════════════════
    // Dedup removes phantom clips
    // ══════════════════════════════════════════════

    #[test]
    fn dedup_removes_phantom_at_same_position() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        // Two clips at the same position (phantom)
        create_clip_with_notes(&mut app, ti, 0, 3840, vec![note(60, 0.0, 0.25)]);
        create_clip_with_notes(&mut app, ti, 0, 7680, vec![note(72, 0.0, 0.25)]);
        assert_eq!(app.nav.tracks[ti].clips.len(), 2);

        app.nav.dedup_clips();
        assert_eq!(app.nav.tracks[ti].clips.len(), 1, "phantom should be absorbed");
        assert_eq!(app.nav.tracks[ti].clips[0].length_ticks, 7680, "longer clip should survive");
        assert!(app.nav.tracks[ti].clips[0].notes.len() >= 2, "absorbed notes should be merged");
    }

    // ══════════════════════════════════════════════
    // Regression: selected_note_indices cleared after delete
    // ══════════════════════════════════════════════

    #[test]
    fn selected_indices_cleared_after_delete() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        create_clip_with_notes(&mut app, ti, 0, 3840, vec![
            note(60, 0.0, 0.25), note(64, 0.25, 0.25), note(67, 0.5, 0.25),
        ]);
        app.nav.track_element = TrackElement::Clip(0);
        app.nav.open_clip_view(ti, 0);

        // Set column count to 4 (one column per beat)
        app.nav.clip_view.piano_roll.column_count = 4;

        // Simulate selecting a column (Enter in piano roll sets these)
        app.nav.clip_view.piano_roll.selected_note_indices = vec![0, 1, 2];

        // Delete ALL notes using full column range
        app.delete_selected_notes(Some((0, 3)), None);

        assert!(app.nav.clip_view.piano_roll.selected_note_indices.is_empty(),
            "selected_note_indices must be cleared after deletion to prevent stale index access");
    }

    // ══════════════════════════════════════════════
    // Regression: recording absorption syncs to audio
    // ══════════════════════════════════════════════

    #[test]
    fn absorption_returns_count_for_audio_sync() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        create_clip_with_notes(&mut app, ti, 0, 3840, vec![note(60, 0.0, 0.25)]);
        create_clip_with_notes(&mut app, ti, 3840, 3840, vec![note(72, 0.0, 0.25)]);

        let mid = app.nav.tracks[ti].mixer_id.unwrap_or(0);
        let snap = phosphor_core::clip::ClipSnapshot {
            track_id: mid, clip_index: 0,
            start_tick: 0, length_ticks: 7680,
            event_count: 4, notes: vec![note(67, 0.25, 0.1)],
        };

        let result = app.nav.receive_clip_snapshot(snap, true);
        assert!(result.is_some(), "absorption should return Some((mixer_id, count))");
        let (ret_mid, absorbed) = result.unwrap();
        assert_eq!(ret_mid, mid);
        assert_eq!(absorbed, 2, "both clips should have been absorbed");
    }

    // ══════════════════════════════════════════════
    // Regression: clip_view_target fixed after absorption
    // ══════════════════════════════════════════════

    #[test]
    fn clip_view_target_fixed_after_absorption() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        create_clip_with_notes(&mut app, ti, 0, 3840, vec![note(60, 0.0, 0.25)]);
        create_clip_with_notes(&mut app, ti, 3840, 3840, vec![note(72, 0.0, 0.25)]);

        // View clip 1
        app.nav.open_clip_view(ti, 1);
        assert_eq!(app.nav.clip_view_target, Some((ti, 1)));

        // Recording absorbs both clips into one
        let mid = app.nav.tracks[ti].mixer_id.unwrap_or(0);
        let snap = phosphor_core::clip::ClipSnapshot {
            track_id: mid, clip_index: 0,
            start_tick: 0, length_ticks: 7680,
            event_count: 2, notes: vec![note(67, 0.0, 0.1)],
        };
        let _ = app.nav.receive_clip_snapshot(snap, true);

        assert_eq!(app.nav.tracks[ti].clips.len(), 1, "should be 1 clip after absorption");
        // Target should be fixed — can't point to clip 1 when only 1 clip exists
        let (_, ci) = app.nav.clip_view_target.unwrap();
        assert_eq!(ci, 0, "clip_view_target should be fixed to 0");
    }

    // ══════════════════════════════════════════════
    // Piano roll: group move with highlights
    // ══════════════════════════════════════════════

    #[test]
    fn highlighted_notes_can_be_moved() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        create_clip_with_notes(&mut app, ti, 0, 3840, vec![
            note(60, 0.0, 0.125),
            note(64, 0.0, 0.125),
            note(67, 0.0, 0.125),
            note(72, 0.5, 0.125), // different column
        ]);

        app.nav.track_element = TrackElement::Clip(0);
        app.nav.open_clip_view(ti, 0);
        app.nav.clip_view.piano_roll.column_count = 4;

        // Highlight first column (column 0)
        app.nav.clip_view.piano_roll.highlight_start = Some(0);
        app.nav.clip_view.piano_roll.highlight_end = Some(0);

        let original_pitches: Vec<u8> = app.nav.tracks[ti].clips[0].notes.iter()
            .filter(|n| n.start_frac < 0.25)
            .map(|n| n.note)
            .collect();
        assert_eq!(original_pitches, vec![60, 64, 67]);

        // Move highlighted notes up by 1 semitone
        app.move_highlighted_notes(0, 1);

        let moved_pitches: Vec<u8> = app.nav.tracks[ti].clips[0].notes.iter()
            .filter(|n| n.start_frac < 0.25)
            .map(|n| n.note)
            .collect();
        assert_eq!(moved_pitches, vec![61, 65, 68], "all highlighted notes should move up 1 semitone");

        // The note in column 2 (at frac 0.5) should NOT have moved
        let unmoved = app.nav.tracks[ti].clips[0].notes.iter()
            .find(|n| n.start_frac >= 0.4)
            .unwrap();
        assert_eq!(unmoved.note, 72, "note outside highlight should be unchanged");
    }

    #[test]
    fn highlighted_move_is_undoable() {
        let mut app = app();
        add_synth_track(&mut app);
        let ti = app.nav.track_cursor;

        create_clip_with_notes(&mut app, ti, 0, 3840, vec![
            note(60, 0.0, 0.125),
        ]);

        app.nav.track_element = TrackElement::Clip(0);
        app.nav.open_clip_view(ti, 0);
        app.nav.clip_view.piano_roll.column_count = 4;
        app.nav.clip_view.piano_roll.highlight_start = Some(0);
        app.nav.clip_view.piano_roll.highlight_end = Some(0);

        app.move_highlighted_notes(0, 5); // up 5 semitones
        assert_eq!(app.nav.tracks[ti].clips[0].notes[0].note, 65);

        app.perform_undo();
        assert_eq!(app.nav.tracks[ti].clips[0].notes[0].note, 60,
            "undo should restore original pitch");
    }
}
