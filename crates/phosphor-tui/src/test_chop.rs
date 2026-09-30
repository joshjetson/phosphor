//! Journeys through the chop screen: both ways in, every row, a held cut,
//! the landing and its refusal, undo, and the kit coming back from a file.
//!
//! The real key handler, the real loader, real WAV files: a chop is a
//! screen a player drives, and the thing under test is that driving it
//! puts the right sound on the right key.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crossterm::event::KeyCode;
    use phosphor_app::sampler::SamplerState;
    use phosphor_core::mixer::MixerCommand;
    use phosphor_core::EngineConfig;

    use crate::app::App;
    use crate::state::*;
    use crate::test_support::{press, press_ctrl, press_shift, screen, type_line};

    const RATE: u32 = 48_000;

    /// Where the loud hits are, in frames, and the one quiet one — 24 dB
    /// down, a ghost note the default sensitivity leaves out.
    const HITS: [usize; 4] = [4_800, 16_800, 28_800, 40_800];
    const GHOST: usize = 52_800;

    fn app() -> App {
        let mut app = App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false);
        app.browse_samples = Some(scratch("browse"));
        app.browse_sessions = Some(scratch("sessions"));
        app
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("phosphor-chop-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A 1.5 s recording: four hits and a ghost, each a 1 kHz tone dying away.
    fn write_break(path: &Path) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: RATE,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        let hit = |i: usize, at: usize, gain: f32| {
            if i < at {
                return 0.0;
            }
            let t = (i - at) as f32 / RATE as f32;
            gain * (std::f32::consts::TAU * 1_000.0 * t).sin() * (-t / 0.05).exp()
        };
        for i in 0..(RATE as usize * 3 / 2) {
            let s: f32 = HITS.iter().map(|&at| hit(i, at, 0.8)).sum::<f32>() + hit(i, GHOST, 0.05);
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
    }

    /// A sampler track with the break on C4, and the command queue drained.
    fn loaded(dir: &Path) -> App {
        let wav = dir.join("break.wav");
        write_break(&wav);
        let mut app = app();
        app.create_instrument_track(InstrumentType::Sampler);
        app.nav.focused_pane = Pane::ClipView;
        app.sampler_follow_note(60);
        load_typed(&mut app, &wav.display().to_string());
        let _ = app.drain_mixer_commands();
        app
    }

    fn load_typed(app: &mut App, path: &str) {
        press(app, KeyCode::Char('a'));
        press(app, KeyCode::Char('/'));
        type_line(app, path);
        press(app, KeyCode::Enter);
    }

    fn sampler(app: &App) -> &SamplerState {
        app.nav.tracks.iter().find_map(|t| t.sampler.as_deref()).expect("no sampler")
    }

    fn chop(app: &App) -> &ChopScreen {
        app.nav.chop_here().expect("the chop screen is not open")
    }

    fn pad_of(note: u8) -> usize {
        SamplerState::pad_of_note(note).unwrap()
    }

    fn flash(app: &App) -> String {
        app.status_message.as_ref().map(|(m, _)| m.clone()).unwrap_or_default()
    }

    /// Walk the row cursor to the row called `label`.
    fn to_row(app: &mut App, label: &str) {
        for _ in 0..10 {
            press(app, KeyCode::Char('k'));
        }
        for _ in 0..10 {
            if chop(app).current().label() == label {
                return;
            }
            press(app, KeyCode::Char('j'));
        }
        panic!("no row called {label}");
    }

    fn previews(commands: &[MixerCommand]) -> usize {
        commands
            .iter()
            .filter(|c| matches!(c, MixerCommand::SetSamplerPreview { preview: Some(_), .. }))
            .count()
    }

    /// The whole feature in one walk: `c` on a loaded pad, `c` again, and
    /// the four hits are on C2 to D#2 — one layer each, one buffer between
    /// them — and one `u` takes all four back.
    #[test]
    fn c_chops_the_pad_and_c_again_lands_it_from_c1() {
        let dir = scratch("walk");
        let mut app = loaded(&dir);
        let source = Arc::clone(sampler(&app).pads[pad_of(60)].layers[0].pcm.as_ref().unwrap());

        press(&mut app, KeyCode::Char('c'));
        assert_eq!(chop(&app).plan.cuts().len(), HITS.len(), "{}", flash(&app));
        let shown = screen(&app, 140, 44);
        assert!(shown.contains("-- CHOP --"), "the mode line does not say chop:\n{shown}");
        assert!(shown.contains("c lands them on C2\u{2013}D#2"), "{shown}");
        assert!(previews(&app.drain_mixer_commands()) >= 1, "opening the screen made no sound");

        press(&mut app, KeyCode::Char('c'));
        assert!(app.nav.sampler_chop.is_none(), "the screen stayed open after landing");
        assert!(flash(&app).contains("4 slices on C2\u{2013}D#2"), "{}", flash(&app));
        let state = sampler(&app);
        for (i, &hit) in HITS.iter().enumerate() {
            let layer = &state.pads[pad_of(36) + i as u8 as usize].layers[0];
            assert!(Arc::ptr_eq(layer.pcm.as_ref().unwrap(), &source), "slice {i} copied the audio");
            assert!(layer.start_frame < hit as u64 && hit as u64 - layer.start_frame < 200, "slice {i} starts at {}", layer.start_frame);
        }
        assert_eq!(state.cursor, pad_of(36), "the cursor did not follow the chop");
        assert!(state.pads[pad_of(40)].layers.is_empty(), "a fifth key was touched");
        assert!(app.nav.tracks[app.nav.track_cursor].clips.is_empty(), "a clip was written unasked");
        let synced = app.drain_mixer_commands();
        assert!(!synced.is_empty(), "the engine was not told about the slices");

        press(&mut app, KeyCode::Char('u'));
        let state = sampler(&app);
        assert!((0..4).all(|i| state.pads[pad_of(36) + i].layers.is_empty()), "one u did not take the chop back");
        assert_eq!(state.pads[pad_of(60)].layers.len(), 1, "undo took the source with it");
    }

    /// The owner's rule, driven. The screen opens past a key in the way; a
    /// player who walks the landing back onto it is refused in words,
    /// nothing changes, nothing lands on the undo stack — and moving the
    /// landing an octave up with the from row gets it through.
    #[test]
    fn a_key_in_the_way_refuses_then_the_from_row_moves_the_landing_clear() {
        let dir = scratch("refuse");
        let mut app = loaded(&dir);
        app.sampler_follow_note(37);
        load_typed(&mut app, &dir.join("break.wav").display().to_string());
        app.sampler_follow_note(60);
        let before = sampler(&app).clone();

        press(&mut app, KeyCode::Char('c'));
        assert!(screen(&app, 140, 44).contains("c lands them on D2\u{2013}F2"), "it opened on the key in the way");
        to_row(&mut app, "from");
        press(&mut app, KeyCode::Char('h'));
        press(&mut app, KeyCode::Char('h'));
        assert!(screen(&app, 140, 44).contains("1 of the 4 keys C2\u{2013}D#2 already holds a sound"));
        press(&mut app, KeyCode::Char('c'));
        assert!(flash(&app).contains("already holds a sound"), "{}", flash(&app));
        assert_eq!(sampler(&app), &before, "a refused chop changed the kit");
        assert!(app.nav.sampler_chop.is_some(), "a refusal closed the screen");

        to_row(&mut app, "from");
        press_shift(&mut app, 'L');
        assert_eq!(SamplerState::pad_label(chop(&app).plan.first), "C3");
        press(&mut app, KeyCode::Char('c'));
        assert!(flash(&app).contains("on C3\u{2013}D#3"), "{}", flash(&app));
        assert_eq!(sampler(&app).pads[pad_of(48)].layers.len(), 1);

        // The refusal left nothing on the undo stack: one `u` takes the chop
        // back, and the next takes back the load that was in the way.
        press(&mut app, KeyCode::Char('u'));
        assert!(sampler(&app).pads[pad_of(48)].layers.is_empty());
        assert_eq!(sampler(&app).pads[pad_of(37)].layers.len(), 1);
        press(&mut app, KeyCode::Char('u'));
        assert!(sampler(&app).pads[pad_of(37)].layers.is_empty(), "a refused chop left an undo step");
    }

    /// The clip row: the chop lands with a clip that replays the recording,
    /// one note per slice on its key at the hit's own time — and one `u`
    /// takes back the slices and the clip together, one redo brings both.
    #[test]
    fn the_clip_row_writes_a_clip_that_replays_the_chop() {
        let dir = scratch("replay");
        let mut app = loaded(&dir);
        let track = app.nav.track_cursor;
        press(&mut app, KeyCode::Char('c'));
        to_row(&mut app, "clip");
        press(&mut app, KeyCode::Char('l'));
        assert!(chop(&app).plan.clip);
        let _ = app.drain_mixer_commands();
        press(&mut app, KeyCode::Char('c'));

        let clips = &app.nav.tracks[track].clips;
        assert_eq!(clips.len(), 1, "no clip was written");
        let clip = &clips[0];
        assert_eq!(clip.start_tick, 0, "the clip is not on bar 1");
        assert_eq!(clip.notes.len(), HITS.len());
        // A quarter second apart is half a beat at 120 BPM.
        let half_beat = phosphor_core::transport::Transport::PPQ / 2;
        for (k, note) in clip.notes.iter().enumerate() {
            assert_eq!(note.note, 36 + k as u8, "note {k} is on the wrong key");
            assert!((note.start_tick - k as i64 * half_beat).abs() <= 2, "note {k} at {}", note.start_tick);
            assert_eq!(note.velocity, 127, "equal hits replayed unequal");
        }
        let sent = app.drain_mixer_commands();
        assert!(sent.iter().any(|c| matches!(c, MixerCommand::CreateClip { .. })), "the engine never heard of the clip");
        assert!(sent.iter().any(|c| matches!(c, MixerCommand::UpdateClip { .. })));
        let words = flash(&app);
        assert!(words.contains("clip at bar 1"), "{words}");
        // 1.4 s from the first hit to the end, at 120 BPM.
        assert!(words.contains("the break runs 0.70 bars at 120 BPM"), "{words}");
        assert_eq!(app.nav.clip_view_target, Some((track, 0)), "the roll was not pointed at the notes");
        let roll = &app.nav.clip_view.piano_roll;
        let top = roll.view_bottom_note.saturating_add(roll.view_height);
        assert!(
            (36..36 + HITS.len() as u8).all(|n| n >= roll.view_bottom_note && n < top),
            "the roll opens on {}..{}, away from the notes",
            roll.view_bottom_note,
            top,
        );

        press(&mut app, KeyCode::Char('u'));
        assert!(app.nav.tracks[track].clips.is_empty(), "undo left the clip");
        assert!(sampler(&app).pads[pad_of(36)].layers.is_empty(), "undo left the slices");
        press_ctrl(&mut app, 'r');
        assert_eq!(app.nav.tracks[track].clips.len(), 1, "redo lost the clip");
        assert_eq!(sampler(&app).pads[pad_of(36)].layers.len(), 1, "redo lost the slices");
    }

    /// `C` opens the picker for a recording to chop, and choosing one opens
    /// the screen on it without touching the pad under the cursor.
    #[test]
    fn capital_c_picks_a_recording_and_leaves_the_pad_alone() {
        let dir = scratch("pick");
        let wav = dir.join("loop.wav");
        write_break(&wav);
        let mut app = app();
        app.create_instrument_track(InstrumentType::Sampler);
        app.nav.focused_pane = Pane::ClipView;

        press_shift(&mut app, 'C');
        assert!(app.nav.file_picker.open);
        assert_eq!(app.nav.file_picker.purpose, PickerPurpose::ChopSample);
        press(&mut app, KeyCode::Char('/'));
        assert_eq!(app.nav.input_modal.kind, InputModalKind::ChopPath);
        type_line(&mut app, &wav.display().to_string());
        press(&mut app, KeyCode::Enter);

        assert_eq!(chop(&app).plan.source().name, "loop");
        assert!(sampler(&app).pads.iter().all(|p| p.layers.is_empty()), "choosing a file put it on a pad");
        press(&mut app, KeyCode::Char('c'));
        let layer = &sampler(&app).pads[pad_of(36)].layers[0];
        assert_eq!(layer.path, wav, "the slice forgot the file it came from");
    }

    /// The screen owns the keys until `esc`, `Tab` included — the trim
    /// strip's contract. A `Tab` that walked off would leave a chop open on a
    /// tab nobody is looking at.
    #[test]
    fn tab_does_not_walk_out_of_the_chop() {
        let dir = scratch("tab");
        let mut app = loaded(&dir);
        press(&mut app, KeyCode::Char('c'));
        let tab = app.nav.clip_view.clip_tab;
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.nav.clip_view.clip_tab, tab);
        assert_eq!(app.nav.focused_pane, Pane::ClipView);
        assert!(app.nav.chop_here().is_some());
    }

    #[test]
    fn esc_leaves_with_nothing_changed() {
        let dir = scratch("esc");
        let mut app = loaded(&dir);
        let before = sampler(&app).clone();
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Esc);
        assert!(app.nav.sampler_chop.is_none());
        assert_eq!(sampler(&app), &before);
        assert!(flash(&app).contains("nothing changed"), "{}", flash(&app));
        // And the pad map has its keys back: `l` walks the bed again.
        let cursor = sampler(&app).cursor;
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(sampler(&app).cursor, cursor + 1);
    }

    /// On the cuts row `h`/`l` walk the slices and every step is heard; held,
    /// they move the cut and it becomes the player's.
    #[test]
    fn the_cuts_row_walks_and_auditions_and_a_held_cut_moves() {
        let dir = scratch("held");
        let mut app = loaded(&dir);
        press(&mut app, KeyCode::Char('c'));
        let _ = app.drain_mixer_commands();
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(chop(&app).plan.selected, 1);
        assert_eq!(previews(&app.drain_mixer_commands()), 1, "walking to a slice was silent");

        let was = chop(&app).plan.cuts()[1].frame;
        press(&mut app, KeyCode::Enter);
        assert!(chop(&app).held);
        assert!(screen(&app, 140, 44).contains("-- HOLD --"));
        press(&mut app, KeyCode::Char('l'));
        let now = chop(&app).plan.cuts()[1];
        // Ten milliseconds at 48 kHz, give or take a snap.
        assert!(now.frame > was + 400 && now.frame < was + 540, "moved from {was} to {}", now.frame);
        assert!(now.pinned, "a moved cut is not the player's");
        // `H`/`L` move the slice's end — the next cut — and leave this one.
        press_shift(&mut app, 'L');
        assert_eq!(chop(&app).plan.cuts()[1].frame, now.frame);
        assert!(chop(&app).plan.cuts()[2].pinned);
        press(&mut app, KeyCode::Esc);
        assert!(!chop(&app).held);
        assert!(app.nav.sampler_chop.is_some(), "esc on a held cut left the screen");
    }

    /// Turned up, the sensitivity reaches the ghost note; the band, the mode
    /// and the fit all change what is cut, and the screen says so.
    #[test]
    fn every_setting_row_reaches_the_cuts() {
        let dir = scratch("rows");
        let mut app = loaded(&dir);
        press(&mut app, KeyCode::Char('c'));
        to_row(&mut app, "sensitivity");
        press_shift(&mut app, 'L');
        press_shift(&mut app, 'L');
        assert_eq!(chop(&app).plan.cuts().len(), HITS.len() + 1, "the ghost note was not reached");
        assert!(flash(&app).contains("5 slices"), "{}", flash(&app));

        to_row(&mut app, "fit");
        press_shift(&mut app, 'H');
        assert_eq!(chop(&app).plan.cuts().len(), 1, "fit walked down an octave");
        press_shift(&mut app, 'L');
        assert_eq!(chop(&app).plan.cuts().len(), HITS.len() + 1, "fit came back up to every hit");

        to_row(&mut app, "mode");
        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(chop(&app).plan.cuts().len(), 8, "equal did not cut eight");
        to_row(&mut app, "slices");
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(chop(&app).plan.cuts().len(), 7);
        assert!(!screen(&app, 140, 44).contains("listen"), "the equal screen asks for a band");
    }

    /// `a` cuts where there was none, `d` takes one away, and a chop with no
    /// cuts refuses to land in words.
    #[test]
    fn add_and_remove_and_a_chop_of_nothing() {
        let dir = scratch("add");
        let mut app = loaded(&dir);
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Char('a'));
        assert_eq!(chop(&app).plan.cuts().len(), HITS.len() + 1);
        for _ in 0..10 {
            press(&mut app, KeyCode::Char('d'));
        }
        assert!(chop(&app).plan.cuts().is_empty());
        press(&mut app, KeyCode::Char('c'));
        assert!(flash(&app).contains("no cuts to land"), "{}", flash(&app));
    }

    /// Saved and opened again, a chopped file is one file and one buffer.
    #[test]
    fn a_chopped_kit_reopens_as_one_buffer() {
        let dir = scratch("session");
        let mut app = loaded(&dir);
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Char('c'));
        let session = dir.join("chopped.phos");
        app.do_save(&session.display().to_string());

        let mut back = self::app();
        back.do_load(&session.display().to_string());
        let state = sampler(&back);
        let first = state.pads[pad_of(36)].layers[0].pcm.as_ref().expect("a slice came back silent");
        for i in 0..HITS.len() {
            let layer = &state.pads[pad_of(36) + i].layers[0];
            assert!(Arc::ptr_eq(layer.pcm.as_ref().unwrap(), first), "slice {i} reopened as its own copy");
        }
        assert!(Arc::ptr_eq(state.pads[pad_of(60)].layers[0].pcm.as_ref().unwrap(), first));
    }

    /// A chop belongs to the track it was opened on: another sampler track
    /// has its own pad map, keys and all.
    #[test]
    fn another_track_does_not_see_the_chop() {
        let dir = scratch("tracks");
        let mut app = loaded(&dir);
        press(&mut app, KeyCode::Char('c'));
        let chop_track = app.nav.track_cursor;
        app.create_instrument_track(InstrumentType::Sampler);
        app.nav.focused_pane = Pane::ClipView;
        assert_ne!(app.nav.track_cursor, chop_track);
        assert!(app.nav.chop_here().is_none(), "the chop followed the cursor to another track");
        assert!(!screen(&app, 140, 44).contains("-- CHOP --"));
        let cursor = sampler_on(&app, app.nav.track_cursor).cursor;
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(sampler_on(&app, app.nav.track_cursor).cursor, cursor + 1, "the other track's keys went to the chop");
    }

    fn sampler_on(app: &App, track: usize) -> &SamplerState {
        app.nav.tracks[track].sampler.as_deref().unwrap()
    }
}
