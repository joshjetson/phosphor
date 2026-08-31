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
                InstrumentType::Teo5 => &phosphor_dsp::teo5::PARAM_NAMES[..],
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

/// The help browser: a list of topics that opens a reference card.
///
/// It shipped as nine one-line summaries with nothing behind them and an
/// Enter key that resolved rows by their shortcut — help topics have no
/// shortcut, so Enter did nothing at all and the section was a menu to
/// nowhere. There was no themes topic either, which is the one thing a
/// player is most likely to go looking for.
#[cfg(test)]
mod help {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use phosphor_app::state::{HelpLine, HELP_TOPICS};
    use phosphor_core::EngineConfig;

    use crate::app::App;
    use crate::state::SpaceMenuSection;

    fn app() -> App {
        let mut app = App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false);
        // What the main loop tells it every frame.
        app.nav.space_menu.set_terminal_rows(40);
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }));
    }

    fn screen(app: &App) -> String {
        let backend = ratatui::backend::TestBackend::new(100, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let snapshot = app.engine.transport.snapshot();
        terminal
            .draw(|frame| crate::ui::render(frame, &snapshot, &app.nav, None))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..40u16)
            .map(|y| (0..100u16).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Open the help list with the cursor on topic `index`.
    fn open_list(index: usize) -> App {
        let mut app = app();
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.nav.space_menu.section, SpaceMenuSection::Help);
        for _ in 0..index {
            press(&mut app, KeyCode::Char('j'));
        }
        assert_eq!(app.nav.space_menu.cursor, index);
        app
    }

    /// The defect, for every topic there is: Enter opens a card with the
    /// topic's own content on it.
    #[test]
    fn enter_opens_a_card_for_every_topic() {
        for (index, topic) in HELP_TOPICS.iter().enumerate() {
            let mut app = open_list(index);
            press(&mut app, KeyCode::Enter);

            assert_eq!(
                app.nav.space_menu.topic,
                Some(index),
                "enter on {:?} opened nothing",
                topic.title,
            );
            assert!(!topic.body.is_empty(), "{:?} has an empty card", topic.title);

            let text = screen(&app);
            assert!(
                text.contains(&format!("help \u{00b7} {}", topic.title)),
                "the card for {:?} is not on the screen:\n{text}",
                topic.title,
            );
            // ...and the first thing the card says is on the screen too, so
            // "opened" means "readable" rather than "an empty box".
            let first = topic
                .body
                .iter()
                .find_map(|line| match line {
                    HelpLine::Heading(text) | HelpLine::Note(text) => Some(*text),
                    HelpLine::Key(keys, _) => Some(*keys),
                    HelpLine::Gap => None,
                })
                .expect("a card of nothing but blank lines");
            assert!(
                text.contains(first),
                "the card for {:?} drew none of its content:\n{text}",
                topic.title,
            );
        }
    }

    /// The topic list is the application as it is today. Two of the old ones
    /// described things that were never built — "plugins: loading and
    /// managing plugins" promised a runtime plugin loader — and the one a
    /// player asks for first was not there at all.
    #[test]
    fn the_topics_are_the_ones_the_application_has() {
        let titles: Vec<&str> = HELP_TOPICS.iter().map(|topic| topic.title).collect();
        assert_eq!(
            titles,
            vec![
                "navigation",
                "transport",
                "tracks",
                "clips",
                "piano roll",
                "step sequencer",
                "effects",
                "instruments",
                "presets & sessions",
                "themes",
                "shortcuts",
            ],
        );
    }

    /// Themes, which is what the player went looking for and could not find.
    #[test]
    fn the_themes_card_says_how_to_cycle_and_where_the_choice_is_kept() {
        let index = HELP_TOPICS.iter().position(|t| t.title == "themes").unwrap();
        let mut app = open_list(index);
        press(&mut app, KeyCode::Enter);
        let text = screen(&app);

        assert!(text.contains("spc+v"), "the card does not say which key:\n{text}");
        assert!(text.contains("config.json"), "it does not say where the choice is kept");
        assert!(text.contains(".phosphor"), "it does not say which folder");
        for name in ["Phosphor", "Gruvbox", "Catppuccin", "SpaceVim2"] {
            assert!(text.contains(name), "the theme {name} is not named on the card");
        }
        // All nine of them, by the theme module's own list.
        for name in crate::theme::THEME_NAMES {
            assert!(text.contains(name), "the theme {name} is missing from the card");
        }
    }

    /// The transport card carries the stop key, which is newer than the
    /// manual and the first thing a player looks for after play.
    #[test]
    fn the_transport_card_carries_stop() {
        let index = HELP_TOPICS.iter().position(|t| t.title == "transport").unwrap();
        let mut app = open_list(index);
        press(&mut app, KeyCode::Enter);
        let text = screen(&app);
        assert!(text.contains("spc+0"), "no stop key on the transport card:\n{text}");
        assert!(text.contains("bar 1"), "it does not say where stop goes");
    }

    /// Esc walks back out the way Enter came in: card, list, closed.
    #[test]
    fn escape_walks_out_of_the_card_and_then_the_menu() {
        let mut app = open_list(2);
        press(&mut app, KeyCode::Enter);
        assert!(app.nav.space_menu.topic.is_some());

        press(&mut app, KeyCode::Esc);
        assert!(app.nav.space_menu.topic.is_none(), "esc did not close the card");
        assert!(app.nav.space_menu.open, "esc closed the menu as well as the card");
        assert_eq!(app.nav.space_menu.cursor, 2, "the list lost its place");

        press(&mut app, KeyCode::Esc);
        assert!(!app.nav.space_menu.open, "esc did not close the menu");
    }

    /// A card longer than its box scrolls, and stops at both ends.
    #[test]
    fn a_long_card_scrolls_and_stops() {
        let index = HELP_TOPICS
            .iter()
            .position(|t| t.title == "step sequencer")
            .unwrap();
        let mut app = open_list(index);
        press(&mut app, KeyCode::Enter);
        assert!(
            app.nav.space_menu.scroll_max() > 0,
            "this card was supposed to be longer than the box",
        );

        let top = screen(&app);
        assert!(top.contains("more"), "a scrollable card does not say so:\n{top}");
        for _ in 0..6 {
            press(&mut app, KeyCode::Char('j'));
        }
        let moved = screen(&app);
        assert_ne!(top, moved, "j did not scroll the card");
        assert_eq!(app.nav.space_menu.scroll, 6);

        // ...and it stops at the bottom rather than scrolling into nothing.
        for _ in 0..200 {
            press(&mut app, KeyCode::Char('j'));
        }
        let bottom = screen(&app);
        assert_eq!(app.nav.space_menu.scroll, app.nav.space_menu.scroll_max());
        let last = match HELP_TOPICS[index].body.last().unwrap() {
            HelpLine::Key(keys, _) => *keys,
            HelpLine::Heading(text) | HelpLine::Note(text) => *text,
            HelpLine::Gap => " ",
        };
        assert!(bottom.contains(last), "the end of the card is unreachable:\n{bottom}");

        for _ in 0..200 {
            press(&mut app, KeyCode::Char('k'));
        }
        assert_eq!(app.nav.space_menu.scroll, 0);
        assert_eq!(screen(&app), top, "scrolling back up did not come home");
    }

    /// A page of text is not a menu: the shortcuts underneath do not fire
    /// while one is open, so reading about the transport cannot start it.
    #[test]
    fn a_card_swallows_the_shortcuts_underneath_it() {
        let mut app = open_list(1);
        press(&mut app, KeyCode::Enter);

        press(&mut app, KeyCode::Char('p'));
        assert!(!app.engine.transport.is_playing(), "reading about play started it");
        assert!(app.nav.space_menu.topic.is_some(), "a stray key closed the card");

        press(&mut app, KeyCode::Tab);
        assert_eq!(
            app.nav.space_menu.section,
            SpaceMenuSection::Help,
            "tab switched sections from under the card",
        );
    }

    /// Nine themes, one card. Nothing in the help browser picks a colour of
    /// its own — the overlay is drawn from the palette like everything else.
    #[test]
    fn the_card_belongs_to_the_theme() {
        let mut app = open_list(5);
        press(&mut app, KeyCode::Enter);
        let first = screen(&app);
        for index in 0..crate::theme::THEME_COUNT {
            crate::theme::set_theme(index);
            assert_eq!(
                screen(&app),
                first,
                "the help card drew different characters in theme {}",
                crate::theme::theme_name(),
            );
        }
        crate::theme::set_theme(0);
        assert!(
            !include_str!("ui/overlays.rs").contains("Color::Rgb"),
            "an overlay names a colour instead of asking the theme for one",
        );
    }

    /// Every line of every card fits the card. A reference that is clipped
    /// at the right margin is a reference that lies by omission — and the
    /// lines are written by hand, so nothing else stops one from growing.
    #[test]
    fn every_line_fits_the_card() {
        // The card is 72 columns wide; four of them are its borders and the
        // padding inside them, and a key column is sixteen.
        const TEXT: usize = 72 - 4;
        const KEYS: usize = 16;

        for topic in HELP_TOPICS {
            assert!(topic.title.chars().count() <= 20, "{:?} is a long title", topic.title);
            assert!(
                topic.summary.chars().count() <= 46,
                "the summary of {:?} does not fit the list",
                topic.title,
            );
            for line in topic.body {
                let width = match line {
                    HelpLine::Heading(text) | HelpLine::Note(text) => text.chars().count() + 2,
                    HelpLine::Key(keys, action) => {
                        assert!(
                            keys.chars().count() < KEYS,
                            "the keys {keys:?} in {:?} overflow their column",
                            topic.title,
                        );
                        2 + KEYS + action.chars().count()
                    }
                    HelpLine::Gap => 0,
                };
                assert!(
                    width <= TEXT,
                    "a line of {:?} is {width} columns wide: {line:?}",
                    topic.title,
                );
            }
        }
    }
}

/// The effect layer's face: the chain, and the eight-band parametric behind
/// a slot of it.
///
/// Everything here drives `handle_event` and reads the rendered buffer,
/// because the two questions that matter about an effect panel are whether a
/// key reaches the audio thread and whether the number on the screen is the
/// one the filter is running.
#[cfg(test)]
mod fx {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use phosphor_core::fx::SendSlot;
    use phosphor_core::mixer::MixerCommand;
    use phosphor_core::EngineConfig;

    use crate::app::App;
    use crate::state::{ClipTab, ClipViewFocus, FxPanelTab, FxType, FxView, InstrumentType, Pane, TrackElement};

    fn press(app: &mut App, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }));
    }

    fn press_shift(app: &mut App, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }));
    }

    fn screen(app: &App, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let snapshot = app.engine.transport.snapshot();
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

    /// A track with an EQ in it, the chain list focused.
    fn chain_app() -> App {
        let mut app = App::new(EngineConfig { buffer_size: 64, sample_rate: 48_000 }, false, false);
        app.create_instrument_track(InstrumentType::Juno60);
        app.nav.add_fx(FxType::Eq);
        app.nav.focus_pane(Pane::ClipView);
        app.nav.clip_view.focus = ClipViewFocus::FxPanel;
        app.nav.clip_view.fx_panel_tab = FxPanelTab::TrackFx;
        app.nav.clip_view.fx_cursor = 0;
        let _ = app.drain_mixer_commands();
        app
    }

    /// The panel open on the EQ, in whichever layout the width implies.
    fn eq_app(wide: bool) -> App {
        let mut app = chain_app();
        app.nav.clip_view.fx.wide = wide;
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.nav.clip_view.clip_tab, ClipTab::Fx);
        app
    }

    fn params(app: &App) -> Vec<f32> {
        app.nav.current_track().unwrap().fx_chain[0].params.clone()
    }

    /// A track with a reverb in it, the panel open on it.
    fn reverb_app() -> App {
        let mut app = App::new(EngineConfig { buffer_size: 64, sample_rate: 48_000 }, false, false);
        app.create_instrument_track(InstrumentType::Juno60);
        let outcome = app.nav.add_fx(FxType::Reverb);
        app.apply_fx_add(outcome);
        app.nav.focus_pane(Pane::ClipView);
        app.nav.clip_view.focus = ClipViewFocus::FxPanel;
        app.nav.clip_view.fx_panel_tab = FxPanelTab::TrackFx;
        app.nav.clip_view.fx_cursor = 0;
        let _ = app.drain_mixer_commands();
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.nav.clip_view.clip_tab, ClipTab::Fx);
        app
    }

    // ── The chain ──

    /// The slot list: what is in the chain, which one the cursor is on, and
    /// whether each one is in the signal path.
    #[test]
    fn the_chain_lists_its_slots() {
        let app = chain_app();
        let text = screen(&app, 120, 40);
        assert!(text.contains("eq"), "the slot is not listed:\n{text}");
        assert!(text.contains('\u{25CF}'), "no active mark on an active slot");
        assert!(text.contains("enter open"), "the list does not say what enter does");
    }

    /// Bypass, from the list, on both sides: the glyph changes and the audio
    /// thread is told.
    #[test]
    fn bypass_toggles_the_slot_glyph_and_the_signal_path() {
        // The slot's own row, not the hint line under the list.
        let slot_row = |app: &App| {
            screen(app, 120, 40)
                .lines()
                .find(|line| line.contains("eq"))
                .unwrap_or_default()
                .to_string()
        };
        let mut app = chain_app();
        assert!(slot_row(&app).contains('\u{25CF}'), "an active slot is not marked active");
        assert!(!slot_row(&app).contains("byp"));

        press(&mut app, KeyCode::Char('b'));
        assert!(app.nav.current_track().unwrap().fx_chain[0].bypass);
        let row = slot_row(&app);
        assert!(row.contains("byp"), "the row does not say it is bypassed: {row:?}");
        assert!(row.contains('\u{25CB}'), "the switch glyph did not change: {row:?}");
        assert!(
            app.drain_mixer_commands().iter().any(|c| matches!(
                c,
                MixerCommand::SetFxBypass { bypass: true, slot: 0, .. }
            )),
            "the audio thread was not told",
        );

        press(&mut app, KeyCode::Char('b'));
        assert!(!app.nav.current_track().unwrap().fx_chain[0].bypass);
    }

    /// Reorder round-trips: the mirror moves, the audio thread is told, and
    /// moving it back puts the chain where it started.
    #[test]
    fn reordering_round_trips() {
        let mut app = chain_app();
        app.nav.add_fx(FxType::Eq);
        let _ = app.drain_mixer_commands();
        assert_eq!(app.nav.current_track().unwrap().fx_chain.len(), 2);

        // Mark the two apart, so a move is visible in the mirror.
        app.set_fx_param(app.nav.track_cursor, 1, 2, 6.0);
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.clip_view.fx_cursor, 1);
        press(&mut app, KeyCode::Char('['));
        assert_eq!(app.nav.clip_view.fx_cursor, 0, "the cursor did not follow the slot");
        assert_eq!(app.nav.current_track().unwrap().fx_chain[0].params[2], 6.0);
        assert!(
            app.drain_mixer_commands()
                .iter()
                .any(|c| matches!(c, MixerCommand::MoveFx { from: 1, to: 0, .. })),
            "the audio thread was not told to move it",
        );

        press(&mut app, KeyCode::Char(']'));
        assert_eq!(app.nav.current_track().unwrap().fx_chain[1].params[2], 6.0);
        assert_eq!(app.nav.clip_view.fx_cursor, 1);
    }

    /// Removing asks first, and then removes on both sides.
    #[test]
    fn removing_an_effect_asks_and_then_removes_it() {
        let mut app = chain_app();
        press(&mut app, KeyCode::Char('d'));
        assert!(app.nav.confirm_modal.open, "it removed without asking");
        assert!(!app.nav.current_track().unwrap().fx_chain.is_empty());

        press(&mut app, KeyCode::Char('y'));
        assert!(app.nav.current_track().unwrap().fx_chain.is_empty());
        assert!(
            app.drain_mixer_commands()
                .iter()
                .any(|c| matches!(c, MixerCommand::RemoveFx { slot: 0, .. })),
            "the audio thread kept the effect",
        );
    }

    /// The chain is the same feature on a bus and on the master: same list,
    /// same keys, same commands, addressed at the strip the cursor is on.
    #[test]
    fn the_chain_works_on_a_bus_and_on_the_master() {
        for name in ["snd a", "mstr"] {
            let mut app = chain_app();
            let index = app
                .nav
                .tracks
                .iter()
                .position(|t| t.name == name)
                .unwrap_or_else(|| panic!("no {name} track"));
            app.nav.track_cursor = index;
            // Send A already carries the plate a new session ships with, so
            // the EQ lands ahead of it: the canonical order is tone before
            // time, and adding an effect inserts at its place rather than
            // appending.
            let before = app.nav.tracks[index].fx_chain.len();
            app.nav.add_fx(FxType::Eq);
            let _ = app.drain_mixer_commands();

            assert_eq!(
                app.nav.tracks[index].fx_chain.len(),
                before + 1,
                "no slot on {name}"
            );
            assert_eq!(app.nav.tracks[index].fx_chain[0].fx_type, FxType::Eq);
            app.nav.clip_view.fx_cursor = 0;
            press(&mut app, KeyCode::Char('b'));
            assert!(app.nav.tracks[index].fx_chain[0].bypass, "bypass did not reach {name}");
            assert!(
                app.drain_mixer_commands()
                    .iter()
                    .any(|c| matches!(c, MixerCommand::SetFxBypass { .. })),
                "the audio thread was not told about {name}",
            );
        }
    }

    // ── The EQ panel ──

    /// The curve is drawn where there is room for it and dropped where there
    /// is not — and the gain column survives either way, because a player can
    /// mix off numbers and cannot mix off a picture.
    #[test]
    fn the_curve_is_wide_only_and_the_numbers_are_not() {
        let wide = screen(&eq_app(true), 120, 40);
        assert!(
            wide.contains('\u{2800}') || wide.chars().any(|c| ('\u{2801}'..='\u{28FF}').contains(&c)),
            "no curve at 120 columns:\n{wide}",
        );
        assert!(wide.contains("+12") && wide.contains("-12"), "no gridlines:\n{wide}");
        assert!(wide.contains("100") && wide.contains("10k"), "no decade ticks:\n{wide}");
        assert!(wide.contains("gain"), "no gain column at 120");

        let narrow = screen(&eq_app(false), 80, 24);
        assert!(
            !narrow.chars().any(|c| ('\u{2801}'..='\u{28FF}').contains(&c)),
            "the curve was drawn at 80 columns:\n{narrow}",
        );
        assert!(narrow.contains("gain"), "the gain column went:\n{narrow}");
        assert!(narrow.contains("2.5k"), "the frequencies went:\n{narrow}");
    }

    /// A gain edit moves the number and the curve together. The curve is
    /// drawn from the EQ's own response, so if they ever disagree it is the
    /// drawing that is wrong.
    #[test]
    fn a_gain_edit_moves_the_number_and_the_curve() {
        let mut app = eq_app(true);
        // Band 5 — the 2.5 kHz bell — and its gain.
        press(&mut app, KeyCode::Char('5'));
        press(&mut app, KeyCode::Char('j')); // the frequency, then the gain
        assert_eq!(app.nav.clip_view.fx.control, 2, "not on the gain");

        let before = screen(&app, 120, 40);
        press(&mut app, KeyCode::Enter);
        assert!(app.nav.clip_view.fx.locked);
        // Four coarse presses: +12 dB.
        for _ in 0..4 {
            press_shift(&mut app, KeyCode::Char('L'));
        }

        assert_eq!(params(&app)[4 * FxView::CONTROLS + 2], 12.0, "the gain did not land on +12");
        let after = screen(&app, 120, 40);
        assert!(after.contains("+12.0"), "the number is not on the screen:\n{after}");
        assert_ne!(before, after, "nothing moved");
        // ...and the curve itself changed, not only the number.
        let braille = |text: &str| {
            text.chars().filter(|c| ('\u{2801}'..='\u{28FF}').contains(c)).count()
        };
        assert!(braille(&after) > 0);
        let curve_before: String = before.lines().filter(|l| l.contains('\u{2800}') || braille(l) > 0).collect();
        let curve_after: String = after.lines().filter(|l| l.contains('\u{2800}') || braille(l) > 0).collect();
        assert_ne!(curve_before, curve_after, "the curve did not move with the number");

        // ...and the audio thread got the same value.
        assert!(
            app.drain_mixer_commands().iter().any(|c| matches!(
                c,
                MixerCommand::SetFxParam { param, value, .. }
                    if *param == 4 * FxView::CONTROLS + 2 && (*value - 12.0).abs() < 1e-6
            )),
            "the filter was never told",
        );
    }

    /// A control the band type does not use is greyed, and refuses to move.
    /// The two are the same fact: the panel greys what the key handler
    /// refuses, both from the band type's own answer.
    #[test]
    fn a_control_the_band_does_not_use_refuses_to_move() {
        let mut app = eq_app(true);
        // Band 1 is the high-pass: no gain.
        press(&mut app, KeyCode::Char('1'));
        // The panel opens on the frequency; one row down is the gain.
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.clip_view.fx.control, 2);

        let before = params(&app);
        press(&mut app, KeyCode::Enter);
        assert!(!app.nav.clip_view.fx.locked, "it held a control that does nothing");
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(params(&app), before, "a greyed control moved");
        assert!(
            app.drain_mixer_commands().is_empty(),
            "a greyed control reached the audio thread",
        );
        assert!(
            screen(&app, 120, 40).contains('\u{2014}'),
            "a control that does nothing is not drawn as such",
        );
    }

    /// Frequencies walk the ISO centres, so the readout is always a number an
    /// EQ says out loud.
    #[test]
    fn frequencies_walk_the_iso_centres() {
        let mut app = eq_app(true);
        press(&mut app, KeyCode::Char('5'));
        assert_eq!(app.nav.clip_view.fx.control, 1, "the panel does not open on the frequency");
        assert_eq!(params(&app)[4 * FxView::CONTROLS + 1], 2500.0);

        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(params(&app)[4 * FxView::CONTROLS + 1], 2800.0, "not an ISO centre");
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(params(&app)[4 * FxView::CONTROLS + 1], 2500.0);

        // A stride is an octave of them.
        press_shift(&mut app, KeyCode::Char('L'));
        assert_eq!(params(&app)[4 * FxView::CONTROLS + 1], 5000.0);
        assert!(screen(&app, 120, 40).contains("5k"), "the readout is not in kilohertz");
    }

    /// The cursor moves the way the screen looks: bands are columns when
    /// there is room and rows when there is not, and `h` moves the cursor the
    /// way `h` points either way.
    #[test]
    fn the_cursor_follows_the_layout() {
        let mut app = eq_app(true);
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.nav.clip_view.fx.band, 1, "l did not move a column");
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.clip_view.fx.control, 2, "j did not move a row");

        let mut app = eq_app(false);
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.clip_view.fx.band, 1, "j did not move a row of bands");
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.nav.clip_view.fx.control, 2, "l did not move a column");
    }

    /// Esc walks back out: release the control, then leave the panel for the
    /// chain it was opened from.
    #[test]
    fn escape_releases_then_leaves() {
        let mut app = eq_app(true);
        press(&mut app, KeyCode::Enter);
        assert!(app.nav.clip_view.fx.locked);

        press(&mut app, KeyCode::Esc);
        assert!(!app.nav.clip_view.fx.locked);
        assert_eq!(app.nav.clip_view.clip_tab, ClipTab::Fx, "esc left the panel too");

        press(&mut app, KeyCode::Esc);
        assert!(app.nav.clip_view.fx.slot.is_none());
        assert_eq!(app.nav.clip_view.focus, ClipViewFocus::FxPanel, "not back on the chain");
    }

    /// A held control takes the keys that would otherwise leave.
    #[test]
    fn a_held_control_swallows_tab_and_undo() {
        let mut app = eq_app(true);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.nav.clip_view.clip_tab, ClipTab::Fx, "tab left a held control");
        press(&mut app, KeyCode::Char('u'));
        assert!(app.nav.clip_view.fx.locked, "undo ran from inside a held control");
    }

    /// The band on switch, and the trim at the end of the strip.
    #[test]
    fn the_band_switch_and_the_trim_are_reachable() {
        let mut app = eq_app(true);
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(params(&app)[FxView::CONTROLS + 5], 1.0);
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(params(&app)[FxView::CONTROLS + 5], 0.0, "n did not switch the band off");

        // Off the end of the eight bands is the output trim.
        for _ in 0..8 {
            press(&mut app, KeyCode::Char('l'));
        }
        assert_eq!(app.nav.clip_view.fx.band, FxView::TRIM);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('l'));
        assert!(params(&app)[phosphor_dsp::fx::eq::PARAM_COUNT - 1] > 0.0, "the trim did not move");
        assert!(screen(&app, 120, 40).contains("trim"));
    }

    /// The selected band's own contribution is drawn on top of the
    /// composite, so a player can see which hump is the one they are turning.
    ///
    /// Read as "the trace is not all one colour" rather than against a named
    /// colour: the palette can be cycled by another test in the same binary
    /// between the render and the read, and what is being asserted is that
    /// the highlight exists, not which amber it is.
    #[test]
    fn the_selected_bands_trace_is_highlighted() {
        let mut app = eq_app(true);
        press(&mut app, KeyCode::Char('5'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);
        for _ in 0..4 {
            press_shift(&mut app, KeyCode::Char('L'));
        }
        press(&mut app, KeyCode::Esc);

        // The highlight is a style, not a character, so this reads colours.
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let snapshot = app.engine.transport.snapshot();
        terminal
            .draw(|frame| crate::ui::render(frame, &snapshot, &app.nav, None))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();

        let mut colours: Vec<String> = Vec::new();
        for y in 0..40u16 {
            for x in 0..120u16 {
                let cell = &buffer[(x, y)];
                let braille = cell
                    .symbol()
                    .chars()
                    .next()
                    .is_some_and(|c| ('\u{2801}'..='\u{28FF}').contains(&c));
                if braille {
                    colours.push(format!("{:?}", cell.fg));
                }
            }
        }
        assert!(!colours.is_empty(), "no curve was drawn at all");
        colours.sort();
        colours.dedup();
        assert!(
            colours.len() > 1,
            "the whole curve is one colour \u{2014} the band being turned has no trace of its own",
        );
    }

    /// Nine themes, one panel. The curve, the gridlines and the greyed
    /// controls all come from the palette.
    #[test]
    fn the_panel_belongs_to_the_theme() {
        let app = eq_app(true);
        let first = screen(&app, 120, 40);
        for index in 0..crate::theme::THEME_COUNT {
            crate::theme::set_theme(index);
            assert_eq!(
                screen(&app, 120, 40),
                first,
                "the eq panel drew different characters in theme {}",
                crate::theme::theme_name(),
            );
        }
        crate::theme::set_theme(0);
        assert!(
            !include_str!("ui/fx.rs").contains("Color::Rgb"),
            "the effect panel names a colour instead of asking the theme for one",
        );
    }


    // ── The reverb panel ──

    /// The panel draws what it is: the algorithm, the twelve controls, and
    /// the readout line that always survives.
    #[test]
    fn the_reverb_panel_lists_its_controls() {
        let app = reverb_app();
        let text = screen(&app, 120, 40);
        assert!(text.contains("rvb"), "the panel does not name itself:\n{text}");
        assert!(text.contains("plate"), "the algorithm is not on the panel:\n{text}");
        for name in [
            "alg", "predly", "decay", "size", "damp", "locut", "early", "diff", "mrate",
            "mdepth", "width", "mix",
        ] {
            assert!(text.contains(name), "the {name} control is not drawn:\n{text}");
        }
        // The values, in the units a person reads them in.
        assert!(text.contains("20 ms"), "the predelay does not read in milliseconds");
        assert!(text.contains("1.8 s"), "the decay does not read in seconds");
        assert!(text.contains("25%"), "the mix does not read as a percentage");
        assert!(text.contains("6k Hz"), "the damping does not read in hertz:\n{text}");
        assert!(text.contains("j/k picks"), "the hint bar is missing:\n{text}");
    }

    /// **The readout moves when the knob does.** `j`/`k` picks a control and
    /// `h`/`l` turns it, both on the screen and in the signal path.
    #[test]
    fn turning_the_decay_moves_the_readout_and_the_signal_path() {
        let mut app = reverb_app();
        // Down two, to `decay`.
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.clip_view.fx.band, 2);
        assert!(screen(&app, 120, 40).contains("1.8 s"));

        let _ = app.drain_mixer_commands();
        for _ in 0..4 {
            press(&mut app, KeyCode::Char('l'));
        }
        let decay = app.nav.current_track().unwrap().fx_chain[0].params[2];
        assert!(decay > 2.3 && decay < 2.6, "four fine presses gave {decay} s");
        assert!(
            screen(&app, 120, 40).contains("2.4 s"),
            "the readout did not follow the knob:\n{}",
            screen(&app, 120, 40)
        );
        assert!(
            app.drain_mixer_commands()
                .iter()
                .any(|c| matches!(c, MixerCommand::SetFxParam { param: 2, .. })),
            "the audio thread was never told about the decay"
        );

        // A shifted press is a bigger one, in the same direction.
        press_shift(&mut app, KeyCode::Char('L'));
        let coarse = app.nav.current_track().unwrap().fx_chain[0].params[2];
        assert!(coarse > decay * 1.4, "H/L did not stride: {decay} -> {coarse}");
    }

    /// Enter holds the control and `j`/`k` stop moving; escape lets go.
    #[test]
    fn holding_a_reverb_control_pins_the_cursor() {
        let mut app = reverb_app();
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.clip_view.fx.band, 1);

        press(&mut app, KeyCode::Enter);
        assert!(app.nav.clip_view.fx.locked);
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.clip_view.fx.band, 1, "j moved the cursor while held");
        // ...and h/l still adjusts, which is the fader's contract.
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.nav.current_track().unwrap().fx_chain[0].params[1], 21.0);

        press(&mut app, KeyCode::Esc);
        assert!(!app.nav.clip_view.fx.locked, "escape did not let go");
        assert_eq!(app.nav.clip_view.clip_tab, ClipTab::Fx, "escape closed the panel too");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.nav.clip_view.clip_tab, ClipTab::InstConfig, "escape did not close it");
    }

    /// **A control that does nothing on this algorithm is greyed, and the
    /// keys refuse to move it.** The spring's input stage is its dispersion
    /// chain, so there are no diffuser coefficients for `diff` to scale.
    #[test]
    fn a_control_the_algorithm_does_not_use_is_refused() {
        let mut app = reverb_app();
        // Walk the algorithm to the spring.
        for _ in 0..3 {
            press(&mut app, KeyCode::Char('l'));
        }
        assert_eq!(app.nav.current_track().unwrap().fx_chain[0].params[0], 3.0);
        assert!(screen(&app, 120, 40).contains("spring"));

        // Down to `diff`, and it will not move.
        for _ in 0..7 {
            press(&mut app, KeyCode::Char('j'));
        }
        assert_eq!(app.nav.clip_view.fx.band, 7);
        let before = app.nav.current_track().unwrap().fx_chain[0].params[7];
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(
            app.nav.current_track().unwrap().fx_chain[0].params[7],
            before,
            "a control the spring does not use was moved anyway"
        );
        let status = app.live_status().unwrap_or_default().to_string();
        assert!(status.contains("no diff control"), "the panel said {status:?}");
        assert!(
            screen(&app, 120, 40).contains("no effect on the spring"),
            "the panel does not say the control is inert"
        );
    }

    /// **Choosing a hall brings its early reflections with it — once.**
    ///
    /// A bare eight-line hall says nothing at all for 125 ms, so it needs an
    /// early-reflection section where a plate needs none. The algorithm knob
    /// moves the `early` control to the incoming algorithm's suggestion, on
    /// screen, in the same keystroke — and stops doing so the moment a player
    /// has set `early` themselves.
    #[test]
    fn the_algorithm_selector_brings_the_early_reflections_it_needs() {
        let mut app = reverb_app();
        assert_eq!(app.nav.current_track().unwrap().fx_chain[0].params[6], 0.0);

        // Plate -> room.
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.nav.current_track().unwrap().fx_chain[0].params[0], 1.0);
        assert_eq!(
            app.nav.current_track().unwrap().fx_chain[0].params[6], 50.0,
            "the room arrived with no early reflections"
        );

        // A player who sets it themselves owns it from then on.
        for _ in 0..6 {
            press(&mut app, KeyCode::Char('j'));
        }
        assert_eq!(app.nav.clip_view.fx.band, 6);
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.nav.current_track().unwrap().fx_chain[0].params[6], 51.0);
        for _ in 0..6 {
            press(&mut app, KeyCode::Char('k'));
        }
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(app.nav.current_track().unwrap().fx_chain[0].params[0], 0.0);
        assert_eq!(
            app.nav.current_track().unwrap().fx_chain[0].params[6], 51.0,
            "going back to the plate overwrote the level the player set"
        );
    }

    /// The panel survives an eighty-column terminal: the columns collapse to
    /// one and the readout and the hint bar stay.
    #[test]
    fn the_reverb_panel_survives_a_narrow_terminal() {
        let app = reverb_app();
        let text = screen(&app, 80, 24);
        assert!(text.contains("rvb"), "the panel lost its name:\n{text}");
        assert!(text.contains("decay"), "the panel lost its controls:\n{text}");
        assert!(text.contains("1.8 s"), "the panel lost its numbers:\n{text}");
    }

    /// Every control can be reached and read, at every algorithm — which is
    /// the test that a control added to the reverb cannot be invisible.
    #[test]
    fn every_reverb_control_can_be_seen_at_every_algorithm() {
        for algorithm in 0..4 {
            let mut app = reverb_app();
            for _ in 0..algorithm {
                press(&mut app, KeyCode::Char('l'));
            }
            for control in 0..phosphor_dsp::fx::reverb::PARAM_COUNT {
                while app.nav.clip_view.fx.band < control {
                    press(&mut app, KeyCode::Char('j'));
                }
                assert_eq!(app.nav.clip_view.fx.band, control);
                let name = phosphor_dsp::fx::reverb::param_name(control);
                let text = screen(&app, 120, 40);
                assert!(
                    text.contains(name),
                    "algorithm {algorithm}: control {name} is not on the screen"
                );
            }
            // ...and the cursor stops at the end rather than running off it.
            for _ in 0..4 {
                press(&mut app, KeyCode::Char('j'));
            }
            assert_eq!(
                app.nav.clip_view.fx.band,
                phosphor_dsp::fx::reverb::PARAM_COUNT - 1
            );
        }
    }

    /// The bus row is named after what is in it. Send A ships with the plate,
    /// so it reads `rvb` before anyone touches anything; an emptied bus goes
    /// back to its letter; and whatever ends up first in the chain is what
    /// the strip says, because "the reverb" is what a player calls that bus.
    #[test]
    fn a_bus_is_labelled_by_its_first_effect() {
        let mut app = chain_app();
        assert!(
            screen(&app, 120, 40).contains("rvb"),
            "Send A did not ship with the plate"
        );

        let index = app.nav.tracks.iter().position(|t| t.name == "snd a").unwrap();
        app.nav.tracks[index].fx_chain.clear();
        assert!(screen(&app, 120, 40).contains("snd a"), "an empty bus lost its label");

        app.nav.track_cursor = index;
        app.nav.add_fx(FxType::Eq);
        let text = screen(&app, 120, 40);
        assert!(text.contains("eq"), "the bus is not named after its effect:\n{text}");
    }

    /// The panel is a tab while it is open, and stops being one when it is
    /// not: a tab for a panel with no effect behind it shows nothing.
    #[test]
    fn the_panel_is_a_tab_only_while_it_is_open() {
        let app = chain_app();
        assert!(!screen(&app, 120, 40).contains("[fx]"), "a tab with nothing behind it");

        let mut app = eq_app(true);
        assert!(screen(&app, 120, 40).contains("[fx]"), "the open panel is not a tab");
        press(&mut app, KeyCode::Esc);
        assert!(!screen(&app, 120, 40).contains("[fx]"), "the tab outlived the panel");
    }

    // ── The strip ──

    /// Pan and the two sends are cells on the track row, they lock like the
    /// fader, and they reach the audio thread.
    #[test]
    fn pan_and_sends_are_reachable_from_the_strip() {
        let mut app = chain_app();
        app.nav.focus_pane(Pane::Tracks);
        app.nav.track_selected = true;
        app.nav.track_element = TrackElement::Pan;
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Enter);
        assert!(app.nav.element_locked, "pan did not lock");
        for _ in 0..3 {
            press(&mut app, KeyCode::Char('l'));
        }
        assert!(app.nav.current_track().unwrap().pan > 0.0);
        assert!(app.live_status().unwrap_or_default().starts_with("pan: R"));
        assert!(
            app.drain_mixer_commands()
                .iter()
                .any(|c| matches!(c, MixerCommand::SetPan { .. })),
            "the mixer was not told about the pan",
        );

        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('l')); // → send A
        assert_eq!(app.nav.track_element, TrackElement::SendA);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('l'));
        assert!(
            app.nav.current_track().unwrap().send(SendSlot::A) > 0.0,
            "the send did not open",
        );
        assert!(
            app.drain_mixer_commands()
                .iter()
                .any(|c| matches!(c, MixerCommand::SetSendLevel { send: SendSlot::A, .. })),
            "the mixer was not told about the send",
        );

        // ...and all three read on the row.
        let text = screen(&app, 120, 40);
        assert!(text.contains('R'), "the pan does not read on the strip:\n{text}");
    }

    /// The safety limiter's reduction reaches the top bar, and stays off it
    /// while there is nothing to report.
    #[test]
    fn the_limiter_readout_is_the_real_meter() {
        let app = chain_app();
        assert!(!screen(&app, 120, 40).contains("lim"), "an idle limiter is on the screen");

        // What the audio thread publishes is what the bar shows. Published
        // through the ballistics the audio thread runs, rather than poked in
        // behind them, so the number on the bar is one the mixer could
        // actually produce.
        let mut ballistics = phosphor_core::fx::GrBallistics::new();
        ballistics.publish(&app.nav.limiter_gr, 0.676, 512, 48_000.0);
        let shown = app.nav.limiter_gr.current_db();
        assert!(shown < -3.0 && shown > -3.8, "the meter read {shown}");

        let text = screen(&app, 120, 40);
        assert!(
            text.contains(&format!("lim {shown:.1}")),
            "the bar does not show what the meter says ({shown:.1}):\n{text}",
        );
    }
}
