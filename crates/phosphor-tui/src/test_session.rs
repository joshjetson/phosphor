//! Integration tests for the `.phos` session format.
//!
//! What is under test is the one thing a session has to get right: reopening
//! it puts the instrument the player chose back on the track. That failed
//! twice — silently, because a drum kit that is not the one you left is still
//! a drum kit — and both times for the same reason: every control was stored
//! as the fraction of the knob's travel it sat at, and a selector turns that
//! fraction into a position by multiplying it by however many positions it has
//! *now*. Adding five kits moved every stored kit; adding 22 patches to the
//! Jupiter moved every stored patch.
//!
//! These drive the real `do_save` / `do_load`, because the format is only half
//! of it — the other half is the loader applying it to a fresh track.

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use crate::app::App;
    use crate::session::{SessionFile, SessionSelector};
    use crate::state::*;
    use phosphor_app::discrete;
    use phosphor_core::EngineConfig;
    use phosphor_dsp::{drum_rack, dx7};

    fn app() -> App {
        App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false)
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }));
    }

    fn type_text(app: &mut App, text: &str) {
        for ch in text.chars() {
            press(app, KeyCode::Char(ch));
        }
    }

    fn add_track(app: &mut App, instrument: InstrumentType) {
        app.create_instrument_track(instrument);
    }

    /// A directory of this test's own, so two of them cannot race on a file.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("phosphor-session-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("test.phos")
    }

    fn instrument_track(app: &App) -> &TrackState {
        app.nav
            .tracks
            .iter()
            .find(|t| t.instrument_type.is_some())
            .expect("the session should have restored an instrument track")
    }

    fn params_of(app: &App) -> Vec<f32> {
        instrument_track(app).synth_params.clone()
    }

    /// The obvious case, and the one nobody thought needed a test: choose a
    /// kit, save, reopen, and the same kit is loaded.
    #[test]
    fn a_drum_kit_survives_a_save_and_load() {
        let path = scratch("kit");
        let mut saving = app();
        add_track(&mut saving, InstrumentType::DrumRack);
        // Four presses up the selector, which is the 777.
        saving.nav.clip_view.synth_param_cursor = drum_rack::P_KIT;
        for _ in 0..4 {
            saving.nav.adjust_synth_param(0.05);
        }
        let chosen = params_of(&saving)[drum_rack::P_KIT];
        assert_eq!(drum_rack::discrete_label(drum_rack::P_KIT, chosen), Some("777"));
        saving.do_save(&path.to_string_lossy());

        let mut reopened = app();
        reopened.do_load(&path.to_string_lossy());
        let kit = params_of(&reopened)[drum_rack::P_KIT];
        assert_eq!(
            drum_rack::discrete_label(drum_rack::P_KIT, kit),
            Some("777"),
            "reopened on a different kit"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Both of the DX7's selectors, which is the case a single "index 0 is the
    /// patch" rule would miss: the cartridge is as discrete as the voice, and
    /// it is the last parameter rather than the first.
    #[test]
    fn both_dx7_selectors_survive_a_save_and_load() {
        let path = scratch("dx7");
        let mut saving = app();
        add_track(&mut saving, InstrumentType::DX7);
        saving.nav.clip_view.synth_param_cursor = dx7::P_BANK;
        for _ in 0..3 {
            saving.nav.adjust_synth_param(0.05);
        }
        saving.nav.clip_view.synth_param_cursor = dx7::P_PATCH;
        for _ in 0..7 {
            saving.nav.adjust_synth_param(0.05);
        }
        let chosen = params_of(&saving);
        let voice = dx7::voice_index(chosen[dx7::P_BANK], chosen[dx7::P_PATCH]);
        saving.do_save(&path.to_string_lossy());

        let mut reopened = app();
        reopened.do_load(&path.to_string_lossy());
        let back = params_of(&reopened);
        assert_eq!(
            dx7::voice_index(back[dx7::P_BANK], back[dx7::P_PATCH]),
            voice,
            "reopened on {} instead of {}",
            dx7::voice_name(dx7::voice_index(back[dx7::P_BANK], back[dx7::P_PATCH])),
            dx7::voice_name(voice),
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// The regression, played out: a session written when the rack held ten
    /// kits, opened by a build that holds fifteen.
    ///
    /// The stored fraction for the 909 was 0.15, which against fifteen kits
    /// reads as the 707 — that is the defect, and it is asserted here so that
    /// the test fails if the loader ever goes back to trusting the fraction.
    /// The stored *position* is 1, and 1 is still the 909.
    #[test]
    fn a_kit_chosen_before_the_bank_grew_still_names_that_kit() {
        let path = scratch("grown");

        // Write the session a ten-kit build would have written, with the
        // position it would have written under this format.
        let mut saving = app();
        add_track(&mut saving, InstrumentType::DrumRack);
        saving.do_save(&path.to_string_lossy());
        let mut file: SessionFile =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let stale_fraction = 1.5 / 10.0;
        file.tracks[0].synth_params[drum_rack::P_KIT] = stale_fraction;
        file.tracks[0].discrete = vec![SessionSelector { param: drum_rack::P_KIT, index: 1 }];
        std::fs::write(&path, serde_json::to_string_pretty(&file).unwrap()).unwrap();

        // The fraction on its own is the 707 now. This is the bug.
        assert_eq!(
            drum_rack::discrete_label(drum_rack::P_KIT, stale_fraction),
            Some("707"),
            "the fifteen-kit rack no longer reads a ten-kit fraction as the 707, \
             so this test is no longer testing anything"
        );

        let mut reopened = app();
        reopened.do_load(&path.to_string_lossy());
        assert_eq!(
            drum_rack::discrete_label(drum_rack::P_KIT, params_of(&reopened)[drum_rack::P_KIT]),
            Some("909"),
            "the session named the 909 and did not get it"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// The same thing from the other end: a session naming a position the bank
    /// no longer has loads the last one there is, rather than panicking or
    /// wrapping round to the first.
    #[test]
    fn a_patch_the_bank_no_longer_has_lands_on_the_last_one() {
        let path = scratch("shrunk");
        let mut saving = app();
        add_track(&mut saving, InstrumentType::DrumRack);
        saving.do_save(&path.to_string_lossy());
        let mut file: SessionFile =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        file.tracks[0].discrete = vec![SessionSelector { param: drum_rack::P_KIT, index: 900 }];
        std::fs::write(&path, serde_json::to_string_pretty(&file).unwrap()).unwrap();

        let mut reopened = app();
        reopened.do_load(&path.to_string_lossy());
        assert_eq!(
            drum_rack::discrete_label(drum_rack::P_KIT, params_of(&reopened)[drum_rack::P_KIT]),
            drum_rack::KIT_LABELS.last().copied(),
        );
        assert!(
            reopened.status_message.as_ref().unwrap().0.contains("no longer in the bank"),
            "the bottom bar said nothing: {:?}",
            reopened.status_message
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A session written by the old format has nothing but the fraction, so
    /// its selectors are only right if the bank has not moved since. It is
    /// still loaded — the fraction is the only evidence there is of what the
    /// player chose — but the bottom bar says to check.
    #[test]
    fn a_session_from_the_old_format_says_it_may_have_moved() {
        let path = scratch("legacy");
        let mut saving = app();
        add_track(&mut saving, InstrumentType::DrumRack);
        saving.nav.clip_view.synth_param_cursor = drum_rack::P_KIT;
        saving.nav.adjust_synth_param(0.05);
        saving.do_save(&path.to_string_lossy());

        // Strip it back to what version 1 wrote.
        let mut file: SessionFile =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        file.version = 1;
        for track in &mut file.tracks {
            track.discrete.clear();
        }
        let json = serde_json::to_string_pretty(&file).unwrap();
        std::fs::write(&path, &json).unwrap();

        let mut reopened = app();
        reopened.do_load(&path.to_string_lossy());
        // The fraction is unchanged since the bank has not moved in between,
        // so the kit is still the 909 — and the player is told to look.
        assert_eq!(
            drum_rack::discrete_label(drum_rack::P_KIT, params_of(&reopened)[drum_rack::P_KIT]),
            Some("909")
        );
        let (message, _) = reopened.status_message.as_ref().expect("no status message");
        assert!(
            message.contains("older format"),
            "an old session loaded without saying so: {message:?}"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    // ── The picker ──
    //
    // What a player actually does: save under a name, and later open the
    // thing they saved without remembering where this application keeps it.
    // Both roads are driven — the list, and the typed path behind `/` —
    // because the second one is the escape hatch and an escape hatch
    // nothing drives is one that quietly stops working.

    /// A folder for a picker to open on, with nothing of anybody else's in
    /// it.
    fn projects(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("phosphor-picker-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An app whose projects folder is `dir`, with a drum track saved into
    /// it under a bare name — the whole of what Space+S asks for.
    fn saved_into(dir: &std::path::Path, name: &str) -> App {
        let mut saving = app();
        saving.browse_sessions = Some(dir.to_path_buf());
        add_track(&mut saving, InstrumentType::DrumRack);
        saving.nav.clip_view.synth_param_cursor = drum_rack::P_KIT;
        for _ in 0..4 {
            saving.nav.adjust_synth_param(0.05);
        }
        saving.do_save(name);
        saving
    }

    /// Space+O, the cursor on the session, Enter — and the song is back,
    /// kit and all. No path typed anywhere in it.
    #[test]
    fn the_picker_lists_a_saved_project_and_enter_opens_it() {
        let dir = projects("open");
        let saved = saved_into(&dir, "neon_causeway");
        assert!(dir.join("neon_causeway.phos").is_file(), "the bare name did not land here");
        let chosen = params_of(&saved)[drum_rack::P_KIT];

        let mut opening = app();
        opening.browse_sessions = Some(dir.clone());
        press(&mut opening, KeyCode::Char(' '));
        press(&mut opening, KeyCode::Char('o'));
        assert!(opening.nav.file_picker.open, "space+o did not open the picker");
        assert!(!opening.nav.input_modal.open, "space+o asked for a path");
        assert_eq!(
            opening.nav.file_picker.visible().iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
            vec!["neon_causeway.phos"],
            "the picker did not list the project that was just saved",
        );

        press(&mut opening, KeyCode::Enter);
        assert!(!opening.nav.file_picker.open, "the picker stayed up over its own answer");
        assert_eq!(
            params_of(&opening)[drum_rack::P_KIT],
            chosen,
            "the session opened on a different kit",
        );
        assert_eq!(
            opening.nav.tracks.iter().filter(|t| t.instrument_type.is_some()).count(),
            1,
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Typing narrows the list to the one project meant, and Enter takes
    /// the row the narrowing left under the cursor.
    #[test]
    fn typing_in_the_picker_narrows_to_the_project_it_opens() {
        let dir = projects("filter");
        for name in ["morning_jam", "neon_causeway", "night_drive"] {
            let _ = saved_into(&dir, name);
        }

        let mut opening = app();
        opening.browse_sessions = Some(dir.clone());
        opening.open_session_picker();
        assert_eq!(opening.nav.file_picker.visible_count(), 3, "three were saved");

        type_text(&mut opening, "neon");
        assert_eq!(opening.nav.file_picker.visible_count(), 1, "the filter did not narrow");
        press(&mut opening, KeyCode::Enter);
        assert_eq!(
            opening.session_path.as_deref(),
            Some(dir.join("neon_causeway.phos").as_path()),
            "the picker opened something other than the row it was showing",
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The list shows projects and folders, and nothing else in the
    /// directory — a `.phos` file is the only thing Enter could open.
    #[test]
    fn the_picker_lists_projects_and_folders_and_nothing_else() {
        let dir = projects("listing");
        let _ = saved_into(&dir, "jam");
        std::fs::write(dir.join("notes.txt"), b"not a session").unwrap();
        std::fs::write(dir.join("kick.wav"), b"not a session either").unwrap();
        std::fs::create_dir_all(dir.join("jam.samples")).unwrap();

        let mut opening = app();
        opening.browse_sessions = Some(dir.clone());
        opening.open_session_picker();
        assert_eq!(
            opening.nav.file_picker.visible().iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
            vec!["jam.samples", "jam.phos"],
            "the picker listed something Enter cannot open",
        );

        // Enter on the folder walks into it rather than trying to open it.
        press(&mut opening, KeyCode::Enter);
        assert!(opening.nav.file_picker.open, "the folder closed the picker");
        assert!(
            opening.nav.file_picker.dir.ends_with("jam.samples"),
            "Enter on a folder did not walk into it",
        );
        assert!(opening.session_path.is_none(), "walking into a folder opened a session");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Esc leaves everything exactly as it was: no session loaded, no
    /// prompt left behind, and the keys back where they were.
    #[test]
    fn esc_leaves_the_session_picker_with_nothing_touched() {
        let dir = projects("esc");
        let _ = saved_into(&dir, "jam");

        let mut opening = app();
        opening.browse_sessions = Some(dir.clone());
        add_track(&mut opening, InstrumentType::Rhodes);
        let before = opening.nav.tracks.len();
        opening.open_session_picker();
        press(&mut opening, KeyCode::Esc);

        assert!(!opening.nav.file_picker.open, "esc left the picker up");
        assert!(!opening.nav.input_modal.open, "esc opened a field on the way out");
        assert!(opening.session_path.is_none(), "esc opened a session");
        assert_eq!(opening.nav.tracks.len(), before, "esc changed the song");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The picker owns every key while it is up.
    ///
    /// `q` quits from the tracks pane, `u` undoes from almost anywhere and a
    /// space opens the menu — and inside a filename all three are letters.
    /// This is the defect the sampler's prompt was built around: a key that
    /// falls through to the pane underneath runs as a command.
    #[test]
    fn the_picker_swallows_the_keys_that_would_act_underneath() {
        let dir = projects("swallow");
        let _ = saved_into(&dir, "quiet jam");

        let mut app = app();
        app.browse_sessions = Some(dir.clone());
        add_track(&mut app, InstrumentType::Rhodes);
        let tracks = app.nav.tracks.len();
        app.open_session_picker();

        type_text(&mut app, "q u");
        assert!(app.running, "`q` inside a filename quit the application");
        assert!(!app.nav.space_menu.open, "a space inside a filename opened the menu");
        assert_eq!(app.nav.tracks.len(), tracks, "a key reached the tracks underneath");
        assert_eq!(app.nav.file_picker.filter, "q u", "the letters did not reach the filter");
        assert!(app.nav.file_picker.open, "the picker closed on a letter");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ...including the keys of a room that claims them all.
    ///
    /// The practice room takes `j`, `k` and `enter` and lets the space menu
    /// through — which is how a picker comes to be open over it. The letters
    /// typed into the picker must not walk the drill list underneath.
    #[test]
    fn the_picker_takes_the_keys_from_the_practice_room_under_it() {
        let dir = projects("practice");
        let _ = saved_into(&dir, "jam");

        let mut app = app();
        app.browse_sessions = Some(dir.clone());
        add_track(&mut app, InstrumentType::Rhodes);
        app.open_practice();
        assert!(app.nav.practice.open, "the practice room did not open");
        let drill = app.nav.practice.cursor;

        app.open_session_picker();
        type_text(&mut app, "jam");
        assert_eq!(app.nav.practice.cursor, drill, "a key walked the drill list");
        // The `j` is the picker's own way down the list, because nothing had
        // been typed yet; `a` and `m` are letters. Neither reached the room.
        assert_eq!(app.nav.file_picker.filter, "am", "the letters did not reach the filter");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The escape hatch: `/` swaps the list for the typed field, and the
    /// typed path opens exactly what it names — the road every session test
    /// above this one drives through `do_load`.
    #[test]
    fn slash_swaps_the_picker_for_a_typed_path_that_still_opens() {
        let dir = projects("typed");
        let saved = saved_into(&dir, "neon_causeway");
        let chosen = params_of(&saved)[drum_rack::P_KIT];
        let path = dir.join("neon_causeway.phos");

        let mut opening = app();
        opening.browse_sessions = Some(dir.clone());
        opening.open_session_picker();
        press(&mut opening, KeyCode::Char('/'));
        assert!(!opening.nav.file_picker.open, "the picker stayed up behind the field");
        assert!(opening.nav.input_modal.open, "`/` did not offer the typed path");
        assert_eq!(opening.nav.input_modal.kind, InputModalKind::Open);

        // The field opens on the folder, so a name is all that is left to
        // type — but the whole path works too, which is the point of it.
        for _ in 0..opening.nav.input_modal.value().chars().count() {
            press(&mut opening, KeyCode::Backspace);
        }
        type_text(&mut opening, &path.to_string_lossy());
        press(&mut opening, KeyCode::Enter);

        assert_eq!(params_of(&opening)[drum_rack::P_KIT], chosen, "the typed path opened nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A saved session names every selector its instrument has, not just the
    /// one at index 0 — the Jupiter has seven switches behind its patch knob
    /// and the DX7 keeps its cartridge at the far end of the panel.
    #[test]
    fn every_selector_on_every_instrument_is_stored() {
        for instrument in InstrumentType::ALL {
            let path = scratch(&format!("all-{instrument:?}"));
            let mut saving = app();
            add_track(&mut saving, *instrument);
            saving.do_save(&path.to_string_lossy());

            let file: SessionFile =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let track = &file.tracks[0];
            // What was saved rather than what was picked: choosing the step
            // sequencer from the menu makes a track carrying its *child*, and
            // the panel on it is the child's.
            let saved = crate::session::parse_instrument_type(&track.instrument_type)
                .unwrap_or(*instrument);
            let expected: Vec<usize> = (0..track.synth_params.len())
                .filter(|&i| discrete::is_discrete(saved, i))
                .collect();
            let stored: Vec<usize> = track.discrete.iter().map(|s| s.param).collect();
            assert_eq!(stored, expected, "{instrument:?}");
            // The sampler's flat panel is two faders; its selector-shaped
            // state lives on the pads, outside the parameter system.
            assert!(
                !expected.is_empty() || saved == InstrumentType::Sampler,
                "{instrument:?} has no selector at all"
            );
            assert_eq!(
                track.sequencer.is_some(),
                instrument.is_sequencer(),
                "{instrument:?} saved the wrong kind of track"
            );

            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }
    }
}
