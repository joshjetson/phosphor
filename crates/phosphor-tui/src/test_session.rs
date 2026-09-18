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

    /// The list shows projects and real folders, and nothing else — not the
    /// loose files Enter cannot open, and not a session's own `.samples`
    /// sidecar, which used to sort first, take the cursor, and swallow the
    /// next save into itself.
    #[test]
    fn the_picker_lists_projects_and_folders_and_nothing_else() {
        let dir = projects("listing");
        let _ = saved_into(&dir, "jam");
        std::fs::write(dir.join("notes.txt"), b"not a session").unwrap();
        std::fs::write(dir.join("kick.wav"), b"not a session either").unwrap();
        std::fs::create_dir_all(dir.join("jam.samples")).unwrap(); // the sidecar
        std::fs::create_dir_all(dir.join("ideas")).unwrap(); // a real folder

        let mut opening = app();
        opening.browse_sessions = Some(dir.clone());
        opening.open_session_picker();
        assert_eq!(
            opening.nav.file_picker.visible().iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
            vec!["ideas", "jam.phos"],
            "the picker listed the sidecar or a loose file",
        );

        // Enter on the real folder walks into it rather than opening a session.
        press(&mut opening, KeyCode::Enter);
        assert!(opening.nav.file_picker.open, "the folder closed the picker");
        assert!(
            opening.nav.file_picker.dir.ends_with("ideas"),
            "Enter on a folder did not walk into it",
        );
        assert!(opening.session_path.is_none(), "walking into a folder opened a session");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The field report, fixed: save a project, change something, Space+S,
    /// press Enter — it saves straight over the same file, no confirm, no
    /// second copy nested in the sidecar. The sidecar folder that used to
    /// derail this is not even in the list.
    #[test]
    fn saving_over_the_open_session_is_one_key() {
        let dir = projects("overwrite");
        let mut app = saved_into(&dir, "918");
        let session = dir.join("918.phos");
        assert_eq!(app.session_path.as_deref(), Some(session.as_path()));
        // A recorded take would put a sidecar here; make one so the trap is
        // present exactly as it was in the field.
        std::fs::create_dir_all(dir.join("918.samples")).unwrap();

        app.open_save_picker();
        // The open session's name is offered, so Enter alone would save it.
        assert_eq!(app.nav.file_picker.name, "918", "the name was not offered");
        assert!(app.nav.file_picker.name_suggested);
        // And the sidecar is not a row that could take the cursor.
        assert!(
            !app.nav.file_picker.visible().iter().any(|e| e.name == "918.samples"),
            "the sidecar is still in the save list",
        );

        press(&mut app, KeyCode::Enter);
        // Straight over the same file: no overwrite question for your own
        // open session, and the picker is done.
        assert!(!app.nav.confirm_modal.open, "overwriting your own session asked a question");
        assert!(!app.nav.file_picker.open, "the save did not close the picker");
        assert_eq!(app.session_path.as_deref(), Some(session.as_path()));
        // Exactly one 918.phos, and none hiding inside the sidecar.
        assert!(session.exists());
        assert!(!dir.join("918.samples").join("918.phos").exists(), "a second copy nested itself");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Typing turns the same save into a spin-off: the offered name is
    /// replaced, not appended, and a fresh file is written beside the
    /// original — which stays untouched.
    #[test]
    fn typing_over_the_offered_name_saves_a_spinoff() {
        let dir = projects("spinoff");
        let mut app = saved_into(&dir, "918");
        app.open_save_picker();
        assert_eq!(app.nav.file_picker.name, "918");

        type_text(&mut app, "919");
        assert_eq!(app.nav.file_picker.name, "919", "the name was appended, not replaced");
        press(&mut app, KeyCode::Enter);
        assert!(dir.join("919.phos").exists(), "the spin-off was not written");
        assert!(dir.join("918.phos").exists(), "the original was lost");
        assert_eq!(app.session_path.as_deref(), Some(dir.join("919.phos").as_path()));

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

    /// `/` continues the walk rather than undoing it: the field starts on
    /// the folder the list was showing, on both pickers.
    ///
    /// A road out that dropped the player back at the home folder would be a
    /// road that costs them the walk they just did — and on the save side it
    /// would put the file somewhere other than the folder they were looking
    /// at, which is the whole thing this work exists to prevent.
    #[test]
    fn the_typed_road_starts_where_the_list_had_walked_to() {
        let dir = projects("typedwalk");
        std::fs::create_dir_all(dir.join("ideas")).unwrap();

        let mut app = app();
        app.browse_sessions = Some(dir.clone());
        app.open_session_picker();
        press(&mut app, KeyCode::Enter); // into `ideas`
        let walked = app.nav.file_picker.dir.display().to_string();
        assert!(walked.ends_with("ideas"), "the walk did not happen: {walked}");

        press(&mut app, KeyCode::Char('/'));
        assert!(
            app.nav.input_modal.value().starts_with(&walked),
            "the open prompt went back to the top: {:?}",
            app.nav.input_modal.value(),
        );
        press(&mut app, KeyCode::Esc);

        // The save side names it under the field rather than in it, and it
        // is the same folder — which is where the bare name will land.
        app.open_save_picker();
        assert!(app.nav.file_picker.dir.ends_with("ideas"), "the save picker forgot the walk");
        press(&mut app, KeyCode::Char('/'));
        assert_eq!(app.nav.input_modal.hint(), walked, "the save prompt named another folder");
        type_text(&mut app, "sketch");
        press(&mut app, KeyCode::Enter);
        assert!(
            dir.join("ideas").join("sketch.phos").is_file(),
            "the bare name did not land in the folder the prompt named",
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Saving through the picker ──
    //
    // The other half of the same list. A save has two halves — which folder,
    // and called what — and the field only ever asked the second one: the
    // folder was decided somewhere else, by a rule the player could not see
    // and could not change. These drive the whole gesture, including every
    // way it is supposed to refuse.

    /// What the terminal would show, as text.
    fn screen(app: &App) -> String {
        let backend = ratatui::backend::TestBackend::new(100, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let snapshot = app.engine.transport.snapshot();
        let status = app.live_status();
        terminal
            .draw(|frame| crate::ui::render(frame, &snapshot, &app.nav, status))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..40)
            .map(|y| {
                (0..100).map(|x| buffer[(x, y)].symbol()).collect::<String>().trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Ctrl+S with a key held, which is how the real one arrives.
    fn ctrl(app: &mut App, ch: char) {
        app.handle_event(Event::Key(KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }));
    }

    /// An app with a song in it and its pickers pointed at `dir`.
    fn ready_to_save(dir: &std::path::Path, instrument: InstrumentType) -> App {
        let mut app = app();
        app.browse_sessions = Some(dir.to_path_buf());
        add_track(&mut app, instrument);
        app
    }

    /// What instrument the session file at `path` holds — read off disk, so
    /// the assertion is about the bytes and not about what an App thinks.
    fn saved_instrument(path: &std::path::Path) -> InstrumentType {
        let file: SessionFile =
            serde_json::from_str(&std::fs::read_to_string(path).expect("no session file")).unwrap();
        crate::session::parse_instrument_type(&file.tracks[0].instrument_type)
            .expect("the session names an instrument this build does not have")
    }

    /// The whole first save: Ctrl+S opens the list, a name is typed, Enter
    /// writes it into the folder on the screen — and every Ctrl+S after that
    /// is silent.
    #[test]
    fn ctrl_s_on_a_new_session_opens_the_save_picker_and_a_name_writes_it() {
        let dir = projects("savepicker");
        let mut app = ready_to_save(&dir, InstrumentType::Rhodes);

        ctrl(&mut app, 's');
        assert!(app.nav.file_picker.open, "ctrl+s did not open the save picker");
        assert_eq!(app.nav.file_picker.purpose, PickerPurpose::SaveSession);
        assert!(!app.nav.input_modal.open, "ctrl+s asked for a path as well");
        assert!(
            app.nav.file_picker.dir.ends_with(dir.file_name().unwrap()),
            "the save picker opened somewhere else: {}",
            app.nav.file_picker.dir.display(),
        );

        type_text(&mut app, "neon_causeway");
        assert_eq!(app.nav.file_picker.name, "neon_causeway", "the letters went somewhere else");
        assert!(app.nav.file_picker.filter.is_empty(), "the name narrowed the list instead");

        press(&mut app, KeyCode::Enter);
        assert!(!app.nav.file_picker.open, "the picker stayed up over its own answer");
        assert!(dir.join("neon_causeway.phos").is_file(), "the name did not land in the folder");
        // The name first, then the folder. The bottom bar cuts what does not
        // fit off the right, and on an absolute path that is the filename —
        // a save reported as `saved: /Users/somebody/Libr…` is a file going
        // missing while being announced.
        let status = app.live_status().unwrap_or_default();
        assert!(
            status.starts_with("saved: neon_causeway.phos"),
            "the save did not name the file first: {status:?}",
        );
        assert!(
            status.contains(&dir.display().to_string()),
            "the save did not say which folder it went into: {status:?}",
        );

        // From here Ctrl+S is the quick save it always was: no list, no
        // question, straight back to the same file.
        std::fs::remove_file(dir.join("neon_causeway.phos")).unwrap();
        ctrl(&mut app, 's');
        assert!(!app.nav.file_picker.open, "the quick save asked again");
        assert!(!app.nav.confirm_modal.open, "the quick save asked about its own file");
        assert!(dir.join("neon_causeway.phos").is_file(), "the quick save wrote nothing");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Space+S is Save As, always — and it saves into the folder the picker
    /// was walked into, which the next picker then opens on.
    #[test]
    fn save_as_writes_into_the_folder_the_picker_walked_into() {
        let dir = projects("savewalk");
        std::fs::create_dir_all(dir.join("ideas")).unwrap();
        let mut app = ready_to_save(&dir, InstrumentType::Rhodes);

        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char('s'));
        assert!(app.nav.file_picker.open, "space+s did not open the save picker");

        // Nothing typed yet, so Enter on the folder walks into it.
        assert_eq!(app.nav.file_picker.selected().map(|e| e.name.clone()), Some("ideas".into()));
        press(&mut app, KeyCode::Enter);
        assert!(app.nav.file_picker.open, "the folder closed the picker");
        assert!(app.nav.file_picker.dir.ends_with("ideas"), "Enter did not walk in");

        type_text(&mut app, "sketch");
        press(&mut app, KeyCode::Enter);
        assert!(dir.join("ideas").join("sketch.phos").is_file(), "the save missed the folder");
        assert!(!dir.join("sketch.phos").exists(), "the save landed where it opened instead");

        // ...and the next picker opens where the player just was, rather
        // than back at the top.
        app.open_session_picker();
        assert!(
            app.nav.file_picker.dir.ends_with("ideas"),
            "the picker forgot where it was: {}",
            app.nav.file_picker.dir.display(),
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A name that is already in the folder is asked about, and `y` writes
    /// over it.
    #[test]
    fn the_save_picker_asks_before_it_writes_over_a_project() {
        let dir = projects("overwrite");
        let _ = saved_into(&dir, "911");
        let path = dir.join("911.phos");
        assert_eq!(saved_instrument(&path), InstrumentType::DrumRack, "the setup did not save a drum rack");

        let mut app = ready_to_save(&dir, InstrumentType::Rhodes);
        app.open_save_picker();
        type_text(&mut app, "911");
        press(&mut app, KeyCode::Enter);

        assert!(app.nav.confirm_modal.open, "the save wrote over a project without asking");
        assert_eq!(app.nav.confirm_modal.kind, ConfirmKind::OverwriteSession);
        assert!(
            app.nav.confirm_modal.message.contains("911.phos"),
            "the question does not name the file: {}",
            app.nav.confirm_modal.message,
        );
        assert_eq!(saved_instrument(&path), InstrumentType::DrumRack, "the question wrote the file anyway");

        press(&mut app, KeyCode::Char('y'));
        assert!(!app.nav.confirm_modal.open);
        assert!(!app.nav.file_picker.open, "the picker stayed up over a finished save");
        assert_eq!(saved_instrument(&path), InstrumentType::Rhodes, "yes did not write the file");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ...and `n` writes nothing, keeps the name, and gives the list back —
    /// "not that file", rather than "start again".
    #[test]
    fn no_to_the_overwrite_question_keeps_the_name_and_the_picker() {
        let dir = projects("overwriteno");
        let _ = saved_into(&dir, "911");
        let path = dir.join("911.phos");

        let mut app = ready_to_save(&dir, InstrumentType::Rhodes);
        app.open_save_picker();
        type_text(&mut app, "911");
        press(&mut app, KeyCode::Enter);
        assert!(app.nav.confirm_modal.open);

        press(&mut app, KeyCode::Char('n'));
        assert!(!app.nav.confirm_modal.open, "no left the question up");
        assert!(app.nav.file_picker.open, "no closed the picker as well");
        assert_eq!(app.nav.file_picker.name, "911", "no threw the name away");
        assert_eq!(saved_instrument(&path), InstrumentType::DrumRack, "no wrote the file anyway");
        assert!(app.session_path.is_none(), "no still took the file as this session's");

        // One more character and another Enter, and it is a new file: the
        // player never had to retype anything.
        type_text(&mut app, "2");
        press(&mut app, KeyCode::Enter);
        assert!(dir.join("9112.phos").is_file(), "the edited name did not save");
        assert_eq!(saved_instrument(&path), InstrumentType::DrumRack, "the original was written after all");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Esc cancels the save outright: nothing written, nothing named, and
    /// the song untouched.
    #[test]
    fn esc_in_the_save_picker_writes_nothing() {
        let dir = projects("saveesc");
        let mut app = ready_to_save(&dir, InstrumentType::Rhodes);
        let tracks = app.nav.tracks.len();

        app.open_save_picker();
        type_text(&mut app, "neon_causeway");
        press(&mut app, KeyCode::Esc);

        assert!(!app.nav.file_picker.open, "esc left the picker up");
        assert!(!app.nav.input_modal.open, "esc opened a field on the way out");
        assert!(app.session_path.is_none(), "esc named this session anyway");
        assert_eq!(app.nav.tracks.len(), tracks, "esc changed the song");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            0,
            "esc wrote something into the folder",
        );

        // ...and the next save starts from an empty name rather than the
        // one that was abandoned.
        app.open_save_picker();
        assert!(app.nav.file_picker.name.is_empty(), "the abandoned name came back");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Enter on a project already in the folder takes its *name*. It writes
    /// nothing: a second Enter, and the question it raises, is what saving
    /// over somebody's song costs.
    #[test]
    fn enter_on_a_project_takes_its_name_and_a_second_enter_saves_over_it() {
        let dir = projects("adopt");
        let _ = saved_into(&dir, "neon_causeway");
        let path = dir.join("neon_causeway.phos");

        let mut app = ready_to_save(&dir, InstrumentType::Rhodes);
        app.open_save_picker();
        assert_eq!(
            app.nav.file_picker.selected().map(|e| e.name.clone()),
            Some("neon_causeway.phos".into()),
        );

        press(&mut app, KeyCode::Enter);
        assert!(app.nav.file_picker.open, "adopting a name closed the picker");
        assert!(!app.nav.confirm_modal.open, "one press asked about writing");
        assert_eq!(app.nav.file_picker.name, "neon_causeway", "the name was not taken");
        assert_eq!(saved_instrument(&path), InstrumentType::DrumRack, "one press wrote the file");
        assert!(
            app.live_status().is_some_and(|s| s.contains("enter again")),
            "nothing said what the next press does: {:?}",
            app.live_status(),
        );

        press(&mut app, KeyCode::Enter);
        assert!(app.nav.confirm_modal.open, "the second press did not ask");
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(saved_instrument(&path), InstrumentType::Rhodes, "the adopted name did not save over it");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).count(),
            1,
            "a second file appeared beside the one being written over",
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The escape hatch saves too: `/` swaps the list for the typed prompt,
    /// carries the half-typed name into it, and names the folder it is
    /// writing into — the one the picker was showing.
    #[test]
    fn slash_in_the_save_picker_still_saves_by_name() {
        let dir = projects("savetyped");
        let mut app = ready_to_save(&dir, InstrumentType::Rhodes);

        app.open_save_picker();
        type_text(&mut app, "neon");
        press(&mut app, KeyCode::Char('/'));
        assert!(!app.nav.file_picker.open, "the picker stayed up behind the field");
        assert!(app.nav.input_modal.open, "`/` did not offer the typed prompt");
        assert_eq!(app.nav.input_modal.kind, InputModalKind::SaveAs);
        assert_eq!(app.nav.input_modal.value(), "neon", "the name was dropped on the way");
        assert_eq!(
            app.nav.input_modal.hint(),
            app.projects_dir().display().to_string(),
            "the prompt names a folder the save would not use",
        );

        type_text(&mut app, "_causeway");
        press(&mut app, KeyCode::Enter);
        assert!(dir.join("neon_causeway.phos").is_file(), "the typed road saved nothing");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The 911 test.** A file saved and then not found again is what all
    /// of this is for.
    ///
    /// The save picker and the open picker are two doors onto one folder. A
    /// project saved through the first is in the list the second shows — in
    /// the same run and in a fresh one, because a player who quits and comes
    /// back is the case that lost the file.
    #[test]
    fn the_911_test_the_save_picker_and_the_open_picker_show_one_folder() {
        let dir = projects("911");
        let mut saving = ready_to_save(&dir, InstrumentType::Rhodes);

        ctrl(&mut saving, 's');
        let saved_in = saving.nav.file_picker.dir.clone();
        type_text(&mut saving, "911");
        press(&mut saving, KeyCode::Enter);
        let written = saving.session_path.clone().expect("the save named no file");

        // The same run: Space+O lists the file that was just saved, in the
        // folder the save picker was showing.
        press(&mut saving, KeyCode::Char(' '));
        press(&mut saving, KeyCode::Char('o'));
        assert_eq!(
            saving.nav.file_picker.dir, saved_in,
            "the open picker opened on a different folder than the save picker",
        );
        assert_eq!(
            saving.nav.file_picker.visible().iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
            vec!["911.phos"],
            "the project that was just saved is not in the list",
        );

        // A fresh run, which is the one that lost the file: nothing carried
        // over in memory, and the list still has it.
        let mut opening = app();
        opening.browse_sessions = Some(dir.clone());
        opening.open_session_picker();
        assert_eq!(
            opening.nav.file_picker.visible().iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
            vec!["911.phos"],
        );
        press(&mut opening, KeyCode::Enter);
        assert_eq!(opening.session_path.as_deref(), Some(written.as_path()), "it opened something else");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A folder that will not take the write says so and keeps the list up,
    /// so the player can walk somewhere that will.
    ///
    /// Read-only directories mean nothing to root, and CI sometimes is root,
    /// so this checks that the folder actually refuses before asserting what
    /// the refusal looks like.
    #[test]
    fn a_folder_that_will_not_take_the_save_keeps_the_picker_and_says_so() {
        let dir = projects("readonly");
        let locked = dir.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        }
        let writable = std::fs::write(locked.join("probe"), b"x").is_ok();
        if writable {
            // Running as somebody who can write anywhere. There is no
            // refusal to test, and pretending otherwise would be a test that
            // passes for the wrong reason.
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }

        let mut app = ready_to_save(&dir, InstrumentType::Rhodes);
        app.open_save_picker();
        press(&mut app, KeyCode::Enter); // into `locked`
        assert!(app.nav.file_picker.dir.ends_with("locked"));
        type_text(&mut app, "neon_causeway");
        press(&mut app, KeyCode::Enter);

        assert!(app.nav.file_picker.open, "the refused save took the picker down with it");
        assert_eq!(app.nav.file_picker.name, "neon_causeway", "the refused save lost the name");
        assert!(app.session_path.is_none(), "a save that never happened named the session");
        let status = app.live_status().unwrap_or_default();
        assert!(status.contains("save failed"), "the refusal said nothing useful: {status:?}");

        // ...and the way out is the way in: walk up, save there instead.
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Enter);
        assert!(dir.join("neon_causeway.phos").is_file(), "there was no way to recover the save");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An empty name with nothing under the cursor does nothing, out loud.
    #[test]
    fn an_empty_name_over_an_empty_folder_says_what_to_do() {
        let dir = projects("saveempty");
        let mut app = ready_to_save(&dir, InstrumentType::Rhodes);
        app.open_save_picker();
        assert!(app.nav.file_picker.selected().is_none(), "the folder was not empty");

        press(&mut app, KeyCode::Enter);
        assert!(app.nav.file_picker.open, "Enter on nothing closed the picker");
        assert!(app.session_path.is_none(), "Enter on nothing saved something");
        let status = app.live_status().unwrap_or_default();
        assert!(status.contains("type a name"), "Enter on nothing said nothing: {status:?}");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "a file appeared out of nothing");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The keys that walk every other list here are letters in this one, and
    /// the walking is on the arrows.
    ///
    /// The pty found this one: `ghost_take` typed into a name line that lent
    /// `g` and `h` to the list walked to the top, then up a folder, and
    /// saved `ost_take` into a folder nobody had chosen — the same defect
    /// this picker exists to end, arriving through the picker itself. So
    /// every letter is a letter here, and the folder cannot move under a
    /// name being typed.
    #[test]
    fn the_name_line_takes_the_letters_that_walk_every_other_list() {
        let dir = projects("walkletter");
        std::fs::create_dir_all(dir.join("ideas")).unwrap();
        let mut app = ready_to_save(&dir, InstrumentType::Rhodes);
        app.open_save_picker();
        let opened_on = app.nav.file_picker.dir.clone();

        type_text(&mut app, "ghost_take");
        assert_eq!(app.nav.file_picker.name, "ghost_take", "the letters went to the list");
        assert_eq!(app.nav.file_picker.dir, opened_on, "a letter walked to another folder");

        press(&mut app, KeyCode::Enter);
        assert!(dir.join("ghost_take.phos").is_file(), "the name did not save where it opened");
        assert!(!dir.join("ideas").join("ghost_take.phos").exists());

        // Re-opening now offers that session's name for a one-key
        // overwrite — and the arrows still walk the list underneath it.
        app.open_save_picker();
        assert_eq!(app.nav.file_picker.name, "ghost_take", "the open session's name was not offered");
        press(&mut app, KeyCode::Down);
        assert_eq!(
            app.nav.file_picker.selected().map(|e| e.name.clone()),
            Some("ghost_take.phos".into()),
            "the down arrow did not move the cursor",
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Everything the box has to say is actually drawn in it: the folder,
    /// what is already there, the name with the extension waiting after the
    /// cursor, and the keys.
    #[test]
    fn the_save_picker_draws_the_folder_the_name_and_the_keys() {
        let dir = projects("savedraw");
        let mut app = ready_to_save(&dir, InstrumentType::Rhodes);

        // An empty folder is a perfectly good place to save into, and the
        // box says so rather than reading as a dead end.
        app.open_save_picker();
        let empty = screen(&app);
        assert!(empty.contains("save project"), "the box does not say what it is for:\n{empty}");
        assert!(empty.contains("nothing here yet"), "an empty folder said nothing:\n{empty}");
        assert!(empty.contains("name \u{2588}.phos"), "the name line is not drawn:\n{empty}");
        assert!(empty.contains("type"), "the footer does not offer the name:\n{empty}");

        let _ = saved_into(&dir, "neon_causeway");
        app.open_save_picker();
        type_text(&mut app, "myjam");
        let text = screen(&app);
        assert!(
            text.contains("name myjam\u{2588}.phos"),
            "the name and its extension are not on the screen:\n{text}",
        );
        assert!(
            text.contains(dir.file_name().unwrap().to_str().unwrap()),
            "the folder it will write into is not on the screen:\n{text}",
        );
        assert!(
            text.contains("neon_causeway.phos"),
            "the project already there is not shown:\n{text}",
        );
        assert!(text.contains("enter save"), "the footer does not say enter saves:\n{text}");

        // ...and a name already carrying the extension is not offered a
        // second one.
        type_text(&mut app, ".phos");
        assert!(
            screen(&app).contains("name myjam.phos\u{2588}"),
            "the extension was offered twice:\n{}",
            screen(&app),
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A separator belongs to `/`'s road, not to a name — and the refusal is
    /// a sentence rather than a shrug.
    #[test]
    fn a_separator_typed_into_the_name_is_refused_in_words() {
        let dir = projects("savesep");
        let mut app = ready_to_save(&dir, InstrumentType::Rhodes);
        app.open_save_picker();

        type_text(&mut app, "ideas");
        press(&mut app, KeyCode::Char('\\'));
        type_text(&mut app, "jam");
        assert_eq!(app.nav.file_picker.name, "ideasjam", "a separator reached the name");
        let status = app.live_status().unwrap_or_default();
        assert!(status.contains("path"), "the refusal did not name the way to do it: {status:?}");

        press(&mut app, KeyCode::Enter);
        assert!(dir.join("ideasjam.phos").is_file(), "the name did not save");
        assert!(!dir.join("ideas").exists(), "a folder was made out of a name");

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
