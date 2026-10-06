//! The section clipboard, driven through the real App: yank, cut, paste,
//! stamp, undo — across tracks, at sub-bar sizes.

#[cfg(test)]
mod tests {
    use phosphor_core::clip::NoteSnapshot;
    use phosphor_core::transport::Transport;
    use phosphor_core::EngineConfig;

    use crate::app::App;
    use crate::state::{Clip, InstrumentType, LoopStep};

    const BAR: i64 = Transport::PPQ * 4;

    fn app() -> App {
        App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false)
    }

    fn note(pitch: u8, start_tick: i64) -> NoteSnapshot {
        NoteSnapshot { note: pitch, velocity: 100, start_tick, duration_ticks: 240, muted: false }
    }

    fn give_clip(app: &mut App, ti: usize, start: i64, len: i64, notes: Vec<NoteSnapshot>) {
        let number = app.nav.tracks[ti].clips.len() + 1;
        app.nav.tracks[ti].clips.push(Clip {
            number,
            width: 4,
            has_content: true,
            start_tick: start,
            length_ticks: len,
            notes,
            hidden_notes: Vec::new(),
            controls: Vec::new(),
        });
    }

    fn two_track_app() -> (App, usize, usize) {
        let mut app = app();
        app.create_instrument_track(InstrumentType::Synth);
        let a = app.nav.track_cursor;
        app.create_instrument_track(InstrumentType::Rhodes);
        let b = app.nav.track_cursor;
        give_clip(&mut app, a, 0, BAR, vec![note(60, 0), note(64, BAR / 2)]);
        give_clip(&mut app, b, 0, BAR, vec![note(36, 0)]);
        (app, a, b)
    }

    /// Cut takes both tracks in one gesture, and ONE undo brings both back.
    #[test]
    fn a_cut_across_tracks_is_one_undo_step() {
        let (mut app, a, b) = two_track_app();
        app.nav.loop_editor.set_region(0, BAR);
        app.cut_loop_section();
        assert!(app.nav.tracks[a].clips[0].notes.is_empty(), "track a kept its notes");
        assert!(app.nav.tracks[b].clips[0].notes.is_empty(), "track b kept its notes");
        assert!(app.nav.section_clip.is_some(), "the cut lifted nothing");

        app.perform_undo();
        assert_eq!(app.nav.tracks[a].clips[0].notes.len(), 2, "undo missed track a");
        assert_eq!(app.nav.tracks[b].clips[0].notes.len(), 1, "undo missed track b");
    }

    /// p stamps at the brace and leapfrogs it: p p lays two copies back to
    /// back, and the loop range follows the stamping.
    #[test]
    fn paste_stamps_and_leapfrogs() {
        let (mut app, a, b) = two_track_app();
        app.nav.loop_editor.set_region(0, BAR);
        app.yank_loop_section();

        // Walk the brace to bar 5 and stamp twice.
        app.nav.loop_editor.set_region(4 * BAR, 5 * BAR);
        app.paste_loop_section(true, 1);
        assert_eq!(app.nav.loop_editor.start, 5 * BAR, "the brace did not leapfrog");
        app.paste_loop_section(true, 1);
        assert_eq!(app.nav.loop_editor.start, 6 * BAR);

        for (ti, want) in [(a, 2usize), (b, 1usize)] {
            for stamp in [4 * BAR, 5 * BAR] {
                let hit = app.nav.tracks[ti]
                    .clips
                    .iter()
                    .find(|c| c.start_tick == stamp)
                    .unwrap_or_else(|| panic!("no stamp at {stamp} on track {ti}"));
                assert_eq!(hit.notes.len(), want, "stamp at {stamp} on {ti} is short");
            }
        }

        // One undo lifts one stamp, not both.
        app.perform_undo();
        assert!(
            !app.nav.tracks[a].clips.iter().any(|c| c.start_tick == 5 * BAR),
            "undo did not lift the second stamp"
        );
        assert!(
            app.nav.tracks[a].clips.iter().any(|c| c.start_tick == 4 * BAR),
            "undo lifted the first stamp too"
        );
    }

    /// The whole point: a quarter-bar chop, lifted and stamped four times,
    /// lands four back-to-back copies a beat apart.
    #[test]
    fn a_quarter_bar_chop_stamps_clean() {
        let (mut app, a, _b) = two_track_app();
        app.nav.loop_editor.step = LoopStep::Sixteenth;
        app.nav.loop_editor.set_region(0, BAR / 4);
        app.yank_loop_section();
        let lifted = app.nav.section_clip.as_ref().unwrap();
        assert_eq!(lifted.len_ticks, BAR / 4);

        app.nav.loop_editor.set_region(2 * BAR, 2 * BAR + BAR / 4);
        for _ in 0..4 {
            app.paste_loop_section(true, 1);
        }
        // Four stamps: 2·BAR, +1 beat, +2 beats, +3 beats.
        let mut found = 0;
        for k in 0..4i64 {
            let at = 2 * BAR + k * (BAR / 4);
            if app.nav.tracks[a].clips.iter().any(|c| {
                c.start_tick <= at && c.start_tick + c.length_ticks > at
                    && c.notes.iter().any(|n| c.start_tick + n.start_tick == at)
            }) {
                found += 1;
            }
        }
        assert_eq!(found, 4, "only {found} of 4 quarter-bar stamps landed");
        // And no clips overlap anywhere.
        let clips = &app.nav.tracks[a].clips;
        for (i, c1) in clips.iter().enumerate() {
            for c2 in clips.iter().skip(i + 1) {
                assert!(
                    c1.start_tick + c1.length_ticks <= c2.start_tick
                        || c2.start_tick + c2.length_ticks <= c1.start_tick,
                    "stamping produced overlapping clips"
                );
            }
        }
    }

    /// Yank with an empty brace refuses and leaves the clipboard alone.
    #[test]
    fn an_empty_brace_refuses() {
        let (mut app, _a, _b) = two_track_app();
        app.nav.loop_editor.set_region(10 * BAR, 11 * BAR);
        app.yank_loop_section();
        assert!(app.nav.section_clip.is_none(), "an empty yank filled the clipboard");
        app.paste_loop_section(true, 1);
        assert!(
            !app.nav.tracks.iter().any(|t| t.clips.iter().any(|c| c.start_tick == 10 * BAR)),
            "a paste with nothing lifted wrote something"
        );
    }

    /// The audio thread hears the edit: a cut sends a full resync for each
    /// touched track — removes then rebuilds.
    #[test]
    fn a_cut_resyncs_the_audio_side() {
        let (mut app, _a, _b) = two_track_app();
        let _ = app.drain_mixer_commands();
        app.nav.loop_editor.set_region(0, BAR);
        app.cut_loop_section();
        let commands = app.drain_mixer_commands();
        let removes = commands
            .iter()
            .filter(|c| matches!(c, phosphor_core::mixer::MixerCommand::RemoveClip { .. }))
            .count();
        let creates = commands
            .iter()
            .filter(|c| matches!(c, phosphor_core::mixer::MixerCommand::CreateClip { .. }))
            .count();
        assert_eq!(removes, 2, "both tracks should clear their audio clips");
        assert_eq!(creates, 2, "both tracks should rebuild their audio clips");
    }

    /// Sliding the brace never lets it cross bar zero, and the leap moves
    /// it by exactly its own length.
    #[test]
    fn the_brace_walks_and_leaps() {
        let (mut app, _a, _b) = two_track_app();
        app.nav.loop_editor.set_region(0, BAR);
        app.slide_loop_brace(false, false, 1);
        assert_eq!(app.nav.loop_editor.start, 0, "the brace crossed bar zero");
        app.slide_loop_brace(true, true, 1);
        assert_eq!(app.nav.loop_editor.start, BAR);
        assert_eq!(app.nav.loop_editor.end, 2 * BAR);
        app.slide_loop_brace(true, false, 1);
        assert_eq!(app.nav.loop_editor.start, BAR + app.nav.loop_editor.step.ticks());
    }

    /// The full undo contract, both directions: a cut undoes and REDOES
    /// through the same stack as everything else.
    #[test]
    fn a_cut_redoes_too() {
        let (mut app, a, b) = two_track_app();
        app.nav.loop_editor.set_region(0, BAR);
        app.cut_loop_section();
        app.perform_undo();
        assert_eq!(app.nav.tracks[a].clips[0].notes.len(), 2);
        app.perform_redo();
        assert!(app.nav.tracks[a].clips[0].notes.is_empty(), "redo did not re-cut track a");
        assert!(app.nav.tracks[b].clips[0].notes.is_empty(), "redo did not re-cut track b");
        app.perform_undo();
        assert_eq!(app.nav.tracks[a].clips[0].notes.len(), 2, "the second undo lost notes");
    }

    /// The destructive case: a replace-stamp overwrites standing material,
    /// and one undo brings the OVERWRITTEN material back — the Song slice
    /// photographed the target before the stamp cleared it.
    #[test]
    fn undoing_a_replace_stamp_restores_what_it_cleared() {
        let (mut app, a, _b) = two_track_app();
        // Standing material at the target that the stamp will destroy.
        give_clip(&mut app, a, 4 * BAR, BAR, vec![note(72, 100), note(74, 500)]);
        app.nav.loop_editor.set_region(0, BAR);
        app.yank_loop_section();
        app.nav.loop_editor.set_region(4 * BAR, 5 * BAR);
        app.paste_loop_section(true, 1);

        let has_72 = |app: &App| {
            app.nav.tracks[a]
                .clips
                .iter()
                .any(|c| c.notes.iter().any(|n| n.note == 72))
        };
        assert!(!has_72(&app), "the replace stamp did not clear the target");

        app.perform_undo();
        assert!(has_72(&app), "undo did not restore what the stamp destroyed");
        let restored = app.nav.tracks[a]
            .clips
            .iter()
            .find(|c| c.start_tick == 4 * BAR)
            .expect("the overwritten clip did not come back");
        assert_eq!(restored.notes.len(), 2, "the restored clip is missing notes");

        app.perform_redo();
        assert!(!has_72(&app), "redo did not re-apply the stamp");
    }

    /// Brace slides ride the coalesced LoopRange gesture: a walk of five
    /// steps is ONE undo back to where the walk began.
    #[test]
    fn a_brace_walk_coalesces_to_one_undo() {
        let (mut app, _a, _b) = two_track_app();
        app.nav.loop_editor.set_region(0, BAR);
        for _ in 0..5 {
            app.slide_loop_brace(true, false, 1);
        }
        assert_ne!(app.nav.loop_editor.start, 0);
        app.perform_undo();
        assert_eq!(
            app.nav.loop_editor.start, 0,
            "one undo should return the whole walk, not one step of it"
        );
    }

    /// The loop editor's keys, as a player types them: Space then `l` to
    /// get in, then the editor's own keys.
    fn in_the_loop_editor(app: &mut App) {
        use crossterm::event::KeyCode;
        crate::test_support::press(app, KeyCode::Char(' '));
        crate::test_support::press(app, KeyCode::Char('l'));
        assert!(app.nav.loop_editor.active, "Space l did not open the loop editor");
    }

    /// A typed count multiplies the next key: `8L` stretches the end by eight
    /// grid steps in one press, and the count does not leak into the key
    /// after it.
    #[test]
    fn a_typed_count_stretches_the_brace_in_one_go() {
        use crate::test_support::{press, type_line};
        use crossterm::event::KeyCode;
        let (mut app, _a, _b) = two_track_app();
        in_the_loop_editor(&mut app);
        app.nav.loop_editor.set_region(0, BAR);
        type_line(&mut app, "8L");
        assert_eq!(app.nav.loop_editor.end, 9 * BAR, "8L should add eight bars");
        press(&mut app, KeyCode::Char('L'));
        assert_eq!(app.nav.loop_editor.end, 10 * BAR, "the count leaked into the next key");
        // One undo takes back the whole eight, not one bar of it.
        app.perform_undo();
        app.perform_undo();
        assert_eq!(app.nav.loop_editor.end, BAR);
    }

    /// `3p` lays three copies back to back as one undo step.
    #[test]
    fn three_p_stamps_three_copies_as_one_step() {
        use crate::test_support::type_line;
        let (mut app, a, _b) = two_track_app();
        in_the_loop_editor(&mut app);
        app.nav.loop_editor.set_region(0, BAR);
        type_line(&mut app, "y");
        app.nav.loop_editor.set_region(4 * BAR, 5 * BAR);
        type_line(&mut app, "3p");
        for stamp in [4 * BAR, 5 * BAR, 6 * BAR] {
            assert!(
                app.nav.tracks[a].clips.iter().any(|c| c.start_tick == stamp && c.notes.len() == 2),
                "no copy at {stamp}"
            );
        }
        assert_eq!(app.nav.loop_editor.start, 7 * BAR, "the brace should wait after the third copy");
        app.perform_undo();
        assert!(
            !app.nav.tracks[a].clips.iter().any(|c| c.start_tick >= 4 * BAR),
            "one undo should lift all three copies"
        );
    }

    /// `c` wraps the brace round a clip on the cursor's track, whatever its
    /// size; `a` round the whole song.
    #[test]
    fn c_and_a_brace_a_clip_and_the_song() {
        use crate::test_support::type_line;
        let (mut app, _a, b) = two_track_app();
        give_clip(&mut app, b, 6 * BAR + BAR / 8, BAR / 8, vec![note(38, 0)]);
        app.nav.track_cursor = b;
        in_the_loop_editor(&mut app);
        app.nav.loop_editor.set_region(5 * BAR, 6 * BAR);
        type_line(&mut app, "c");
        assert_eq!(
            (app.nav.loop_editor.start, app.nav.loop_editor.end),
            (6 * BAR + BAR / 8, 6 * BAR + BAR / 4),
            "c should brace the next clip, a thirty-second-sized one included"
        );
        type_line(&mut app, "a");
        assert_eq!((app.nav.loop_editor.start, app.nav.loop_editor.end), (0, 7 * BAR));
    }

    /// The lanes follow the brace: walked past the right edge, they scroll;
    /// `z` zooms them to fit it.
    #[test]
    fn the_lanes_follow_and_fit_the_brace() {
        use crate::test_support::type_line;
        let (mut app, _a, _b) = two_track_app();
        in_the_loop_editor(&mut app);
        app.nav.loop_editor.set_region(0, BAR);
        type_line(&mut app, "20J");
        assert_eq!(app.nav.loop_editor.start, 20 * BAR);
        assert!(app.nav.timeline.shows(20 * BAR, 21 * BAR), "the lanes lost the brace: {:?}", app.nav.timeline);
        type_line(&mut app, "z");
        assert_eq!(app.nav.timeline.bars, 1, "a one-bar brace fits one bar");
        assert_eq!(app.nav.timeline.first_bar, 20);
    }
}
