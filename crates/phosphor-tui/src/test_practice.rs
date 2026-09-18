//! The practice room, driven the way the terminal drives it.

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use phosphor_app::practice::{judge, Family, Hands};
    use phosphor_core::EngineConfig;

    use crate::app::App;

    fn top_bar(app: &App) -> String {
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let snapshot = app.engine.transport.snapshot();
        terminal
            .draw(|frame| crate::ui::render(frame, &snapshot, &app.nav, None))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..120).map(|x| buffer.cell((x, 0)).unwrap().symbol().to_string()).collect()
    }

    /// When the audio callback has been missing its deadlines, the top
    /// bar says so — a starved callback stretches time with every number
    /// still right, and that must never again be a mystery.
    #[test]
    fn the_top_bar_warns_when_audio_falls_behind() {
        let mut app = app();
        assert!(!top_bar(&app).contains("audio"), "the warning is on before any overrun");
        app.nav.audio_struggling = true;
        assert!(top_bar(&app).contains("\u{26A0}audio"), "an overrun went unnamed");
        app.nav.audio_struggling = false;
        assert!(!top_bar(&app).contains("audio"), "the warning outlived the trouble");
    }

    /// While the drill click owns the metronome, the top bar says so by
    /// name — a free-running click at a drill tempo is indistinguishable
    /// by ear from the whole application running at the wrong tempo, and
    /// exactly that report once cost a day of measuring an engine that
    /// was honest all along.
    #[test]
    fn the_top_bar_names_the_click_on_loan() {
        let mut app = app();
        assert!(!top_bar(&app).contains("drill@"), "the loan is shown before it exists");
        app.nav.practice.engine_click = Some((60, 1));
        let bar = top_bar(&app);
        assert!(bar.contains("drill@60"), "the loaned click is unnamed: {bar}");
        assert!(bar.contains("2&4"), "the half-feel pattern is unnamed: {bar}");
        app.nav.practice.engine_click = None;
        assert!(!top_bar(&app).contains("drill@"), "the tell outlived the loan");
    }

    use crate::state::InstrumentType;

    fn app() -> App {
        App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// The room needs a sound to teach on: no instrument, no fingers.
    #[test]
    fn the_room_needs_an_instrument() {
        let mut app = app();
        app.open_practice();
        assert!(!app.nav.practice.open, "the room opened over nothing");

        app.create_instrument_track(InstrumentType::Rhodes);
        app.open_practice();
        assert!(app.nav.practice.open, "the room refused a Rhodes");
    }

    /// The whole journey: open, browse, start, play a clean rep through
    /// the real MIDI path, and see the room's judgment land.
    #[test]
    fn a_rep_travels_the_whole_road() {
        let mut app = app();
        app.create_instrument_track(InstrumentType::Synth);
        app.open_practice();

        // Down to the chromatic drill (row 2), LH via h h.
        app.handle_practice_keys(key(KeyCode::Char('j')));
        app.handle_practice_keys(key(KeyCode::Char('j')));
        assert_eq!(app.nav.practice.family(), Family::Chromatic);
        app.handle_practice_keys(key(KeyCode::Char('h')));
        assert_eq!(app.nav.practice.hands, Hands::Left);

        // Start in wait mode and play the drill perfectly.
        assert_eq!(app.nav.practice.mode, judge::Mode::Wait);
        app.handle_practice_keys(key(KeyCode::Enter));
        assert!(app.nav.practice.run.is_some(), "enter did not start the drill");
        let notes: Vec<u8> = app
            .nav
            .practice
            .run
            .as_ref()
            .unwrap()
            .exercise
            .notes
            .iter()
            .map(|n| n.note)
            .collect();
        for (k, n) in notes.iter().enumerate() {
            app.nav.practice.note_on(*n, 1000 + k as u64 * 100_000);
            app.nav.practice.note_off(*n);
        }
        let _ = app.nav.practice.tick(phosphor_midi::clock::now_micros());
        let run = app.nav.practice.run.as_ref().unwrap();
        let report = run.last_report.expect("the rep never finished");
        assert!(report.clean, "a perfect wait rep read dirty: {report:?}");
        assert_eq!(run.rep, 2, "the next rep did not roll");

        // The record learned the clean tempo.
        let id = run.exercise.id.clone();
        assert!(app.nav.practice.record_for(&id) > 0, "no record was written");

        // Esc stops the run; Esc again leaves the room.
        app.handle_practice_keys(key(KeyCode::Esc));
        assert!(app.nav.practice.run.is_none());
        assert!(app.nav.practice.open);
        app.nav.practice.progress_dirty = false; // keep the test off the real file
        app.handle_practice_keys(key(KeyCode::Esc));
        assert!(!app.nav.practice.open);
    }

    /// Keys cycle in fourths — the jazz walk — and the tempo override
    /// arms the ladder from where the player sets it.
    #[test]
    fn keys_walk_in_fourths() {
        let mut app = app();
        app.create_instrument_track(InstrumentType::Synth);
        app.open_practice();
        assert_eq!(app.nav.practice.key(), 0); // C
        app.handle_practice_keys(key(KeyCode::Char('>')));
        assert_eq!(app.nav.practice.key(), 5); // F
        app.handle_practice_keys(key(KeyCode::Char('>')));
        assert_eq!(app.nav.practice.key(), 10); // Bb

        app.handle_practice_keys(key(KeyCode::Char(']')));
        let up = app.nav.practice.start_bpm();
        app.handle_practice_keys(key(KeyCode::Char('[')));
        app.handle_practice_keys(key(KeyCode::Char('[')));
        assert_eq!(app.nav.practice.start_bpm(), up - 10);
    }
}
