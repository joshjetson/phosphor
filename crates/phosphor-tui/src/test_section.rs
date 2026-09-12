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
        app.paste_loop_section(true);
        assert_eq!(app.nav.loop_editor.start, 5 * BAR, "the brace did not leapfrog");
        app.paste_loop_section(true);
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
            app.paste_loop_section(true);
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
        app.paste_loop_section(true);
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
        app.slide_loop_brace(false, false);
        assert_eq!(app.nav.loop_editor.start, 0, "the brace crossed bar zero");
        app.slide_loop_brace(true, true);
        assert_eq!(app.nav.loop_editor.start, BAR);
        assert_eq!(app.nav.loop_editor.end, 2 * BAR);
        app.slide_loop_brace(true, false);
        assert_eq!(app.nav.loop_editor.start, BAR + app.nav.loop_editor.step.ticks() + 0);
    }
}
