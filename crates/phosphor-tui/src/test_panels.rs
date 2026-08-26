//! The panels a player types into, and what the screen says back.
//!
//! Three defects found by building a five-track song in the running
//! application, all of the same family: a control that looks like it works.
//!
//! * the `[inst]` tab drew a mock-up — `LFO rate`, `Filter cutoff`, four
//!   sections of plausible names at a hard-coded `0%`, wired to nothing —
//!   while the instrument's real panel sat in the narrow column beside it;
//! * the save prompt opened with `sessions/untitled.phos` already in the
//!   field and the cursor at the end, so a typed name landed after the
//!   extension and the file that appeared was called untitled;
//! * drawing the first note on an empty track made a clip out of nothing and
//!   said so nowhere.
//!
//! Every test here drives `handle_event` with the events a terminal sends, or
//! reads the rendered buffer, because all three passed every test that did
//! neither.

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use phosphor_core::mixer::MixerCommand;
    use phosphor_core::EngineConfig;

    use crate::app::App;
    use crate::state::{ClipTab, ClipViewFocus, InstrumentType, Pane};

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

    fn screen(app: &App, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let snapshot = app.engine.transport.snapshot();
        // The status the running application would be showing, because that
        // is what the bottom bar draws and half of what is under test here
        // is what the bottom bar says.
        let status = app.live_status();
        terminal
            .draw(|frame| crate::ui::render(frame, &snapshot, &app.nav, status))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// An instrument track with the `[inst]` tab open and the keyboard on it.
    fn panel_app(instrument: InstrumentType) -> App {
        let mut app = app();
        app.create_instrument_track(instrument);
        app.nav.focus_pane(Pane::ClipView);
        app.nav.clip_view.focus = ClipViewFocus::PianoRoll;
        app.nav.clip_view.clip_tab = ClipTab::InstConfig;
        app.nav.clip_view.synth_param_cursor = 0;
        let _ = app.drain_mixer_commands();
        app
    }

    fn params(app: &App) -> Vec<f32> {
        app.nav.current_track().unwrap().synth_params.clone()
    }

    // ── The instrument tab ──

    /// The tab is the instrument's own panel now, not a drawing of one.
    #[test]
    fn the_instrument_tab_draws_the_real_controls() {
        let app = panel_app(InstrumentType::Juno60);
        let text = screen(&app, 120, 40);

        assert!(text.contains("Juno-60"), "the panel does not say whose it is:\n{text}");
        assert!(text.contains("25 controls"), "no control count:\n{text}");
        for real in ["patch", "lfo rate", "dco lfo", "pwm mode", "sustain", "chorus"] {
            assert!(text.contains(real), "the real control {real:?} is missing:\n{text}");
        }
        // The patch selector reads its patch, not a percentage.
        assert!(text.contains("STRINGS"), "the patch is not named:\n{text}");

        // ...and none of the mock-up survives.
        for fake in ["bend range", "portamento", "env amt", "Envelope"] {
            assert!(!text.contains(fake), "the placeholder panel is still drawn: {fake:?}");
        }
    }

    /// Every control is reachable and visible: put the cursor on each one in
    /// turn and it is on the screen, whichever page it lives on. The
    /// Prophet-6's eighty-four are the reason this tab exists.
    #[test]
    fn every_control_of_every_instrument_can_be_seen() {
        for instrument in InstrumentType::ALL.iter().copied() {
            if instrument.is_sequencer() {
                continue;
            }
            let mut app = panel_app(instrument);
            let names = match instrument {
                InstrumentType::DrumRack => &phosphor_dsp::drum_rack::PARAM_NAMES[..],
                InstrumentType::DX7 => &phosphor_dsp::dx7::PARAM_NAMES[..],
                InstrumentType::Jupiter8 => &phosphor_dsp::jupiter::PARAM_NAMES[..],
                InstrumentType::Odyssey => &phosphor_dsp::odyssey::PARAM_NAMES[..],
                InstrumentType::Juno60 => &phosphor_dsp::juno::PARAM_NAMES[..],
                InstrumentType::Rhodes => &phosphor_dsp::rhodes::PARAM_NAMES[..],
                InstrumentType::LittlePhatty => &phosphor_dsp::phatty::PARAM_NAMES[..],
                InstrumentType::Prophet6 => &phosphor_dsp::prophet6::PARAM_NAMES[..],
                _ => &phosphor_dsp::synth::PARAM_NAMES[..],
            };
            let count = params(&app).len().min(names.len());
            assert!(count > 0, "{instrument:?} has no parameters");

            for (index, name) in names.iter().enumerate().take(count) {
                app.nav.clip_view.synth_param_cursor = index;
                let text = screen(&app, 120, 40);
                assert!(
                    text.contains(name),
                    "{instrument:?} control {index} ({name}) is not on the screen when the \
                     cursor is on it",
                );
            }
        }
    }

    /// Keys typed into the tab reach the instrument — the panel changes and
    /// so does the audio thread. This is the whole complaint: the tab
    /// answered keys and nothing happened anywhere.
    #[test]
    fn keys_in_the_instrument_tab_reach_the_instrument() {
        let mut app = panel_app(InstrumentType::Juno60);
        press(&mut app, KeyCode::Char('j')); // off the patch selector
        press(&mut app, KeyCode::Char('j'));
        let index = app.nav.clip_view.synth_param_cursor;
        assert_eq!(index, 2, "j did not walk the controls");

        let before = params(&app);
        press(&mut app, KeyCode::Char('l'));
        let after = params(&app);
        assert!(after[index] > before[index], "l did not turn the knob");
        assert_eq!(
            before.iter().zip(&after).filter(|(a, b)| a != b).count(),
            1,
            "one keypress moved more than one control",
        );

        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|command| matches!(
                command,
                MixerCommand::SetParameter { param_index, .. } if *param_index == index
            )),
            "the audio thread was never told",
        );

        press(&mut app, KeyCode::Char('h'));
        assert!(
            (params(&app)[index] - before[index]).abs() < 1e-6,
            "h did not put it back",
        );
    }

    /// The patch knob is the first control, and turning it reloads the whole
    /// panel — every value of which has to reach the audio thread, or the
    /// sound is half the old patch.
    #[test]
    fn the_patch_knob_reloads_the_whole_panel() {
        let mut app = panel_app(InstrumentType::Juno60);
        let before = params(&app);

        press(&mut app, KeyCode::Char('l'));
        let after = params(&app);
        assert_ne!(before[0], after[0], "the patch did not change");
        assert!(
            before.iter().zip(&after).filter(|(a, b)| a != b).count() > 1,
            "the patch changed but nothing else did",
        );

        let sent = app
            .drain_mixer_commands()
            .into_iter()
            .filter(|command| matches!(command, MixerCommand::SetParameter { .. }))
            .count();
        assert_eq!(sent, after.len(), "the audio thread got part of a patch");
    }

    /// Esc leaves the panel rather than being swallowed by it.
    #[test]
    fn escape_leaves_the_instrument_tab() {
        let mut app = panel_app(InstrumentType::Juno60);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.nav.focused_pane, Pane::Tracks);
    }

    // ── The save prompt ──

    /// The prompt opens on the directory alone, and typing produces exactly
    /// what was typed. It used to open on `sessions/untitled.phos` with the
    /// cursor at the end: a player typing the name of their song got
    /// `sessions/untitled.phosneon_causeway`, and the file was untitled.
    #[test]
    fn the_save_prompt_takes_exactly_what_is_typed() {
        let mut app = app();
        app.handle_save();
        assert!(app.nav.input_modal.open, "no prompt");

        let start = app.nav.input_modal.value().to_string();
        assert!(start.ends_with('/') || start.ends_with('\\'), "the field is not a bare directory: {start:?}");
        assert!(!start.contains("untitled"), "the field opens with a name in it: {start:?}");
        assert_eq!(app.nav.input_modal.cursor, start.chars().count(), "the cursor is not at the end");

        type_text(&mut app, "neon_causeway");
        assert_eq!(app.nav.input_modal.value(), format!("{start}neon_causeway"));
        assert_eq!(app.nav.input_modal.resolved(), format!("{start}neon_causeway"));
        assert!(app.nav.input_modal.placeholder().is_empty(), "the suggestion outstayed its welcome");
    }

    /// Enter on an untouched prompt still means untitled — the suggestion is
    /// a default, not a decoration.
    #[test]
    fn an_untouched_save_prompt_falls_back_to_the_suggestion() {
        let mut app = app();
        app.handle_save();
        let directory = app.nav.input_modal.value().to_string();

        assert_eq!(app.nav.input_modal.placeholder(), "untitled.phos");
        assert_eq!(app.nav.input_modal.resolved(), format!("{directory}untitled.phos"));

        // ...and it is on the screen, so nobody has to guess what Enter does.
        assert!(
            screen(&app, 100, 30).contains("untitled.phos"),
            "the suggestion is not shown",
        );
    }

    /// A field longer than the box it is drawn in scrolls, so what is being
    /// typed is what is visible. The application directory is an absolute
    /// path on a machine whose home directory can be any length, and the
    /// prompt is fifty columns wide wherever it is opened.
    #[test]
    fn a_long_path_scrolls_rather_than_running_off_the_prompt() {
        let mut app = app();
        app.handle_save();
        type_text(&mut app, "a_rather_long_song_name_for_the_evening");

        let text = screen(&app, 100, 30);
        assert!(
            text.contains("evening"),
            "the end of what was typed is not on the screen:\n{text}",
        );
        assert!(text.contains('\u{2026}'), "nothing said the field was scrolled:\n{text}");

        // ...and every line of the modal still fits inside its border.
        for line in text.lines().filter(|line| line.contains("filename:")) {
            let width = line.trim_end().chars().count();
            assert!(width <= 100, "the field ran off the terminal: {width}");
        }
    }

    /// A name with characters outside ASCII in it does not take the
    /// application down. The field used to slice its buffer by bytes.
    #[test]
    fn a_name_with_wide_characters_does_not_panic() {
        let mut app = app();
        app.handle_save();
        type_text(&mut app, "prélude_日本");
        let _ = screen(&app, 100, 30);
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Left);
        let _ = screen(&app, 100, 30);
        press(&mut app, KeyCode::Backspace);
        let _ = screen(&app, 100, 30);
        assert!(app.nav.input_modal.value().contains("prélude"));
    }

    /// Typing takes the suggestion off the screen: it is where the name goes,
    /// not something to read around.
    #[test]
    fn the_suggestion_gets_out_of_the_way() {
        let mut app = app();
        app.handle_save();
        type_text(&mut app, "b");
        assert!(app.nav.input_modal.placeholder().is_empty());
        assert!(!screen(&app, 100, 30).contains("untitled.phos"));

        // ...and comes back if the name is erased again.
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.nav.input_modal.placeholder(), "untitled.phos");
    }

    /// The open prompt was always right, and stays right.
    #[test]
    fn the_open_prompt_has_no_suggestion() {
        let mut app = app();
        app.nav.input_modal.open_load();
        assert!(app.nav.input_modal.placeholder().is_empty());
        let value = app.nav.input_modal.value().to_string();
        assert_eq!(app.nav.input_modal.resolved(), value);
    }

    // ── The clip that appears out of nowhere ──

    /// Drawing the first note on an empty track makes a clip. Saying so is
    /// the difference between a feature and a surprise: the only other sign
    /// of it is a block appearing in a pane the player is not looking at.
    #[test]
    fn drawing_on_an_empty_track_says_a_clip_was_made() {
        let mut app = app();
        app.create_instrument_track(InstrumentType::Juno60);
        app.nav.focus_pane(Pane::ClipView);
        app.nav.clip_view.focus = ClipViewFocus::PianoRoll;
        app.nav.clip_view.clip_tab = ClipTab::PianoRoll;
        assert!(app.nav.current_track().unwrap().clips.is_empty());

        press(&mut app, KeyCode::Char('n'));

        assert_eq!(app.nav.current_track().unwrap().clips.len(), 1, "no clip was made");
        let status = app.live_status().unwrap_or_default().to_string();
        assert!(status.contains("clip 1 created"), "nothing was said: {status:?}");
        assert!(status.contains("bar"), "the status does not say how long: {status:?}");
        assert!(
            screen(&app, 100, 30).contains("clip 1 created"),
            "the message never reached the bottom bar",
        );

        // The second note on the same clip says nothing — one clip, one
        // announcement.
        app.status_message = None;
        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(app.nav.current_track().unwrap().clips.len(), 1);
        assert!(app.live_status().is_none(), "the message repeated on every note");
    }
}
