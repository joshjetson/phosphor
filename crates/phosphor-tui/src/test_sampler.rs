//! Journeys through the sampler: choosing it, putting a sound on a pad,
//! working the pad map, and getting the kit back from a session file.
//!
//! These drive the real key handler and the real loader against real WAV
//! files on disk, because the sampler's whole promise is one sentence:
//! type a path, hear the pad. Every failure mode a player can type is
//! walked — the wrong path, the full pad, the file that moved between
//! save and load, the `d` pressed on the wrong row.

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::state::*;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use phosphor_app::sampler::SamplerState;
    use phosphor_core::mixer::MixerCommand;
    use phosphor_core::EngineConfig;

    fn app() -> App {
        App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false)
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
    }

    /// A key with shift down, the way a terminal sends it: the uppercase
    /// character *and* the modifier.
    fn press_shift(app: &mut App, ch: char) {
        app.handle_event(Event::Key(KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
    }

    /// What the running application would be showing, as text.
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

    fn type_line(app: &mut App, text: &str) {
        for ch in text.chars() {
            press(app, KeyCode::Char(ch));
        }
    }

    /// A directory of this test's own, holding both its wavs and its
    /// session file.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("phosphor-sampler-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_wav(path: &std::path::Path, frames: usize) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..frames {
            let s = (core::f32::consts::TAU * 220.0 * i as f32 / 44_100.0).sin();
            w.write_sample((s * 20_000.0) as i32).unwrap();
        }
        w.finalize().unwrap();
    }

    /// Create a sampler track, the way a player does — Space+a is already
    /// behind us. Creating it opens the pad map with the keys on it, which
    /// is where `a` means "add a sound".
    fn sampler_app() -> App {
        let mut app = app();
        app.create_instrument_track(InstrumentType::Sampler);
        app.nav.focused_pane = Pane::ClipView;
        app
    }

    /// A sampler track with `kick.wav` on the pad under the cursor, and the
    /// mixer's command queue drained so a test can see what its own keys
    /// send.
    fn loaded_app(dir: &std::path::Path) -> App {
        let wav = dir.join("kick.wav");
        write_wav(&wav, 4_410);
        let mut app = sampler_app();
        press(&mut app, KeyCode::Char('a'));
        type_line(&mut app, &wav.display().to_string());
        press(&mut app, KeyCode::Enter);
        let _ = app.drain_mixer_commands();
        app
    }

    /// The pad the panel is pointing at.
    fn pad(app: &App) -> &phosphor_app::sampler::PadState {
        sampler_state(app).current()
    }

    fn sampler_state(app: &App) -> &SamplerState {
        app.nav
            .tracks
            .iter()
            .find_map(|t| t.sampler.as_deref())
            .expect("the sampler track should carry sampler state")
    }

    #[test]
    fn choosing_the_sampler_makes_a_sampler_not_a_synth_in_disguise() {
        let app = sampler_app();
        let track = app
            .nav
            .tracks
            .iter()
            .find(|t| t.instrument_type == Some(InstrumentType::Sampler))
            .expect("no sampler track");
        assert!(track.sampler.is_some(), "the track carries no pad state");
        // The panel is the sampler's two globals, not the synth's fifteen.
        assert_eq!(track.synth_params.len(), phosphor_dsp::sampler::PARAM_COUNT);
    }

    #[test]
    fn type_a_path_and_the_pad_carries_the_sound() {
        let dir = scratch("load");
        let wav = dir.join("kick.wav");
        write_wav(&wav, 4_410);

        let mut app = sampler_app();
        press(&mut app, KeyCode::Char('a'));
        assert!(app.nav.input_modal.open, "`a` did not ask for a file");
        assert_eq!(app.nav.input_modal.kind, InputModalKind::SamplePath);

        type_line(&mut app, &wav.display().to_string());
        press(&mut app, KeyCode::Enter);

        let state = sampler_state(&app);
        let pad = state.cursor;
        assert_eq!(state.pads[pad].layers.len(), 1, "the layer did not land");
        let layer = &state.pads[pad].layers[0];
        assert_eq!(layer.name, "kick");
        assert!(layer.pcm.is_some());
        assert_eq!(layer.end_frame, 4_410);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wrong_path_reports_and_leaves_the_pad_alone() {
        let mut app = sampler_app();
        press(&mut app, KeyCode::Char('a'));
        type_line(&mut app, "/nowhere/at/all.wav");
        press(&mut app, KeyCode::Enter);
        assert_eq!(sampler_state(&app).occupied_pads().count(), 0);
        let (message, _) = app.status_message.as_ref().expect("no word to the player");
        assert!(message.contains("not found"), "unhelpful message: {message}");
    }

    #[test]
    fn the_ninth_layer_is_refused_at_the_prompt() {
        let dir = scratch("full");
        let wav = dir.join("hat.wav");
        write_wav(&wav, 441);
        let mut app = sampler_app();
        for _ in 0..phosphor_app::sampler::MAX_LAYERS + 1 {
            press(&mut app, KeyCode::Char('a'));
            type_line(&mut app, &wav.display().to_string());
            press(&mut app, KeyCode::Enter);
        }
        let state = sampler_state(&app);
        assert_eq!(state.pads[state.cursor].layers.len(), phosphor_app::sampler::MAX_LAYERS);
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("full"), "the refusal said: {message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn playing_a_key_selects_its_pad() {
        let mut app = sampler_app();
        app.sampler_follow_note(64);
        assert_eq!(SamplerState::note_of_pad(sampler_state(&app).cursor), 64);
        // A note off the bed leaves the cursor where it was.
        app.sampler_follow_note(5);
        assert_eq!(SamplerState::note_of_pad(sampler_state(&app).cursor), 64);
    }

    #[test]
    fn on_another_instrument_the_keys_neither_follow_nor_prompt() {
        let mut app = app();
        app.create_instrument_track(InstrumentType::Rhodes);
        app.nav.focused_pane = Pane::ClipView;
        app.sampler_follow_note(64); // must not panic
        press(&mut app, KeyCode::Char('a'));
        assert!(!app.nav.input_modal.open, "a Rhodes offered to load a sample");
    }

    #[test]
    fn the_kit_survives_save_and_load() {
        let dir = scratch("roundtrip");
        let wav = dir.join("snare.wav");
        write_wav(&wav, 2_000);
        let session = dir.join("kit.phos");

        let mut saving = sampler_app();
        saving.sampler_follow_note(38); // D1's pad, the snare seat
        press(&mut saving, KeyCode::Char('a'));
        type_line(&mut saving, &wav.display().to_string());
        press(&mut saving, KeyCode::Enter);
        // A pad tweak that must round-trip too.
        {
            let track = saving.nav.tracks.iter_mut().find(|t| t.sampler.is_some()).unwrap();
            let sampler = track.sampler.as_mut().unwrap();
            let pad = sampler.cursor;
            sampler.pads[pad].config.choke = 2;
            sampler.pads[pad].layers[0].reverse = true;
        }
        saving.do_save(&session.display().to_string());

        let mut loading = app();
        loading.do_load(&session.display().to_string());
        let state = sampler_state(&loading);
        let pad = SamplerState::pad_of_note(38).unwrap();
        assert_eq!(state.pads[pad].layers.len(), 1);
        assert_eq!(state.pads[pad].config.choke, 2);
        assert!(state.pads[pad].layers[0].reverse);
        assert!(state.pads[pad].layers[0].pcm.is_some(), "the wav did not reload");

        // The file moves away; the pad keeps its seat and says so.
        std::fs::remove_file(&wav).unwrap();
        let mut after_move = app();
        after_move.do_load(&session.display().to_string());
        let state = sampler_state(&after_move);
        assert_eq!(state.pads[pad].layers.len(), 1, "a missing file cost the pad its layer");
        assert!(state.pads[pad].layers[0].pcm.is_none());
        assert_eq!(state.missing_layers(), 1);
        let (message, _) = after_move.status_message.as_ref().unwrap();
        assert!(message.contains("missing"), "the player was not told: {message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── The pad map ──

    /// Making a sampler opens its pads, with the keyboard on them: all four
    /// of the tab, the side, the panel cursor and the pane.
    #[test]
    fn a_sampler_track_opens_on_its_pads() {
        let app = sampler_app();
        assert_eq!(app.nav.clip_view.clip_tab, ClipTab::Pads);
        assert_eq!(app.nav.clip_view.focus, ClipViewFocus::PianoRoll);
        assert_eq!(app.nav.clip_view.sampler.knob, 0);
        assert!(!app.nav.clip_view.sampler.locked);
        assert_eq!(app.nav.focused_pane, Pane::ClipView);

        // And the tab is on the strip, which nothing else puts it on.
        let text = screen(&app, 120, 40);
        assert!(text.contains("[pads]"), "no pads tab on the strip:\n{text}");
    }

    /// Any other instrument has no pad map at all — no tab, and the tab
    /// cycle steps over it rather than landing on an empty view.
    #[test]
    fn an_instrument_with_no_pads_has_no_pads_tab() {
        let mut app = app();
        app.create_instrument_track(InstrumentType::Rhodes);
        app.nav.focus_pane(Pane::ClipView);
        let text = screen(&app, 120, 40);
        assert!(!text.contains("[pads]"), "a Rhodes grew a pad map:\n{text}");
        for _ in 0..8 {
            app.nav.cycle_tab();
            assert_ne!(app.nav.clip_view.clip_tab, ClipTab::Pads);
        }
    }

    /// The map draws: the keyboard band, the filled list with the pad's
    /// name and the sound on it, and the panel for the pad under the caret.
    #[test]
    fn the_pad_map_draws_the_bed_the_list_and_the_panel() {
        let dir = scratch("draws");
        let app = loaded_app(&dir);
        let text = screen(&app, 120, 40);

        assert!(text.contains("\u{25BC}"), "no caret over the pad:\n{text}");
        assert!(text.contains("1 filled"), "the filled list is empty:\n{text}");
        assert!(text.contains("kick"), "the sound is not named:\n{text}");
        assert!(text.contains("pad C3"), "the panel does not say whose it is:\n{text}");
        assert!(text.contains("one-shot"), "the trigger mode is not shown:\n{text}");
        assert!(text.contains("[PAD:C3]"), "the strip does not say which pad:\n{text}");
        // The band is five rows of keys: the caret row plus the keyboard.
        assert!(text.contains("0.10s"), "the layer's length is not shown:\n{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `h`/`l` walk the bed a key at a time and `H`/`L` an octave, and the
    /// panel follows the pad they land on.
    #[test]
    fn the_pad_cursor_walks_the_keyboard() {
        let mut app = sampler_app();
        let start = sampler_state(&app).cursor;
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(sampler_state(&app).cursor, start + 1);
        press(&mut app, KeyCode::Char('h'));
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(sampler_state(&app).cursor, start - 1);
        press_shift(&mut app, 'L');
        assert_eq!(sampler_state(&app).cursor, start + 11);
        press_shift(&mut app, 'H');
        assert_eq!(sampler_state(&app).cursor, start - 1);

        // The ends of the bed are walls, not wraps.
        for _ in 0..100 {
            press_shift(&mut app, 'H');
        }
        assert_eq!(sampler_state(&app).cursor, 0);
        for _ in 0..100 {
            press_shift(&mut app, 'L');
        }
        assert_eq!(sampler_state(&app).cursor, phosphor_app::sampler::NUM_PADS - 1);
    }

    /// A control is held before it turns: `h` and `l` walk the keyboard
    /// until Enter says otherwise. This is the whole grammar of the tab,
    /// and getting it wrong means the bed is unreachable.
    #[test]
    fn a_knob_turns_only_once_it_is_held() {
        let dir = scratch("hold");
        let mut app = loaded_app(&dir);
        let start = sampler_state(&app).cursor;
        let poly = |app: &App, pad: usize| sampler_state(app).pads[pad].config.poly;

        // Down to poly, which is a number a test can watch.
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.clip_view.sampler.knob, 1);
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(poly(&app, start), 1, "an unheld knob turned");
        assert_eq!(sampler_state(&app).cursor, start + 1, "`l` did not walk the bed");

        press(&mut app, KeyCode::Char('h')); // back to the pad with the sound on it
        press(&mut app, KeyCode::Enter);
        assert!(app.nav.clip_view.sampler.locked);
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(poly(&app, start), 2, "a held knob did not turn");
        assert_eq!(sampler_state(&app).cursor, start, "a held `l` moved the bed too");

        press(&mut app, KeyCode::Esc);
        assert!(!app.nav.clip_view.sampler.locked);
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(poly(&app, start), 2, "esc did not let go of the knob");
        assert_eq!(sampler_state(&app).cursor, start + 1, "the bed is unreachable again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Turning a knob reaches the engine, and a sweep of it is one press of
    /// `u` — not one per detent.
    #[test]
    fn a_sweep_is_one_undo_step_and_the_engine_hears_every_turn() {
        let dir = scratch("sweep");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('j')); // poly
        press(&mut app, KeyCode::Enter);
        for _ in 0..4 {
            press(&mut app, KeyCode::Char('l'));
        }
        assert_eq!(pad(&app).config.poly, 5);

        let commands = app.drain_mixer_commands();
        let pads = commands
            .iter()
            .filter(|c| matches!(c, MixerCommand::SetSamplerPad { .. }))
            .count();
        assert_eq!(pads, 4, "the engine was not told about every turn: {pads}");

        press(&mut app, KeyCode::Esc); // let go, so `u` is not eaten by the hold
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(pad(&app).config.poly, 1, "one `u` did not undo the whole sweep");
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(pad(&app).layers.len(), 0, "the step before the sweep was the load");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `m` mutes the layer under the cursor and `m` again brings it back,
    /// both times telling the engine.
    #[test]
    fn m_mutes_the_layer_under_the_cursor() {
        let dir = scratch("mute");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('m'));
        assert!(pad(&app).layers[0].mute);
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::SetSamplerPad { .. })),
            "a mute never reached the engine",
        );
        press(&mut app, KeyCode::Char('m'));
        assert!(!pad(&app).layers[0].mute);
        // And each one is its own step.
        press(&mut app, KeyCode::Char('u'));
        assert!(pad(&app).layers[0].mute, "undo did not put the mute back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `d` asks first, and `n` leaves the pad alone.
    #[test]
    fn d_asks_before_it_takes_a_sound_off_a_pad() {
        let dir = scratch("refuse");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('d'));
        assert!(app.nav.confirm_modal.open, "`d` removed a layer without asking");
        assert_eq!(app.nav.confirm_modal.kind, ConfirmKind::DeleteSamplerLayer);
        assert!(
            app.nav.confirm_modal.message.contains("kick"),
            "the question does not say what it is about to take: {}",
            app.nav.confirm_modal.message,
        );
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(pad(&app).layers.len(), 1, "`n` took the layer anyway");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole delete journey: `d`, `y`, gone from the screen and from
    /// the engine's copy — then `u` brings it back *with its audio*, which
    /// is the contract that keeps the audio thread from freeing a buffer.
    #[test]
    fn a_deleted_layer_comes_back_with_its_audio() {
        let dir = scratch("delete");
        let mut app = loaded_app(&dir);
        // The file goes away too: what comes back must be the decode the
        // undo step was holding, not a fresh read from disk.
        std::fs::remove_file(dir.join("kick.wav")).unwrap();

        press(&mut app, KeyCode::Char('d'));
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(pad(&app).layers.len(), 0);
        let commands = app.drain_mixer_commands();
        let emptied = commands.iter().any(|c| matches!(
            c,
            MixerCommand::SetSamplerPad { layers, .. } if layers.is_empty()
        ));
        assert!(emptied, "the engine was never told the pad is empty");
        let text = screen(&app, 120, 40);
        assert!(
            text.contains("pads \u{00b7} empty"),
            "the list still shows a pad with something on it:\n{text}",
        );
        assert!(
            text.contains("a loads a sound onto this pad"),
            "the emptied pad does not say what to do next:\n{text}",
        );

        press(&mut app, KeyCode::Char('u'));
        let layer = &pad(&app).layers[0];
        assert_eq!(layer.name, "kick");
        assert!(layer.pcm.is_some(), "the layer came back with no audio behind it");
        let restored = app.drain_mixer_commands();
        assert!(
            restored.iter().any(|c| matches!(
                c,
                MixerCommand::SetSamplerPad { layers, .. } if layers.len() == 1
            )),
            "undo put the layer back on the screen but not in the engine",
        );

        // And redo takes it away again — the direction-blind half.
        app.handle_event(Event::Key(KeyEvent {
            code: KeyCode::Char('r'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert_eq!(pad(&app).layers.len(), 0, "redo did not take the layer back off");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Undoing a pad edit that emptied a pad has to empty it in the engine
    /// too: the union of what was occupied on either side is shipped, or a
    /// sound stays on a key with nothing on the screen to explain it.
    #[test]
    fn undo_empties_the_engines_copy_of_a_pad_it_emptied() {
        let dir = scratch("union");
        let mut app = loaded_app(&dir);
        let here = sampler_state(&app).cursor;
        // A second pad, so the undo has more than one to think about.
        app.sampler_follow_note(SamplerState::note_of_pad(here) + 2);
        let wav = dir.join("kick.wav");
        press(&mut app, KeyCode::Char('a'));
        type_line(&mut app, &wav.display().to_string());
        press(&mut app, KeyCode::Enter);
        let second = sampler_state(&app).cursor;
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Char('u')); // undo the second load
        assert_eq!(sampler_state(&app).pads[second].layers.len(), 0);
        let commands = app.drain_mixer_commands();
        let cleared = commands.iter().any(|c| matches!(
            c,
            MixerCommand::SetSamplerPad { pad, layers, .. }
                if *pad as usize == second && layers.is_empty()
        ));
        assert!(cleared, "the emptied pad was never cleared in the engine");
        // The pad that was not touched still has its sound.
        assert_eq!(sampler_state(&app).pads[here].layers.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The layer cursor walks with `[` and `]` and jumps with the digits,
    /// and `m` follows it rather than always muting the first sound.
    #[test]
    fn the_layer_cursor_picks_which_sound_the_keys_act_on() {
        let dir = scratch("layers");
        let mut app = loaded_app(&dir);
        let wav = dir.join("clap.wav");
        write_wav(&wav, 800);
        press(&mut app, KeyCode::Char('a'));
        type_line(&mut app, &wav.display().to_string());
        press(&mut app, KeyCode::Enter);
        assert_eq!(pad(&app).layers.len(), 2);
        // A load points the panel at what just arrived.
        assert_eq!(app.nav.clip_view.sampler.layer, 1);

        press(&mut app, KeyCode::Char('['));
        assert_eq!(app.nav.clip_view.sampler.layer, 0);
        press(&mut app, KeyCode::Char('m'));
        assert!(pad(&app).layers[0].mute);
        assert!(!pad(&app).layers[1].mute, "`m` muted a layer it was not on");

        press(&mut app, KeyCode::Char('2'));
        assert_eq!(app.nav.clip_view.sampler.layer, 1);
        // A number past the end of the list is not a cursor anywhere.
        press(&mut app, KeyCode::Char('8'));
        assert_eq!(app.nav.clip_view.sampler.layer, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Walking onto a pad with fewer layers than the panel was showing
    /// takes the cursors with it — including a held knob that no longer
    /// exists, which would otherwise turn whatever slid into its place.
    #[test]
    fn a_pad_with_nothing_on_it_takes_the_cursor_off_the_layer_controls() {
        let dir = scratch("stale");
        let mut app = loaded_app(&dir);
        for _ in 0..30 {
            press(&mut app, KeyCode::Char('j')); // down to the layer's controls
        }
        let deep = app.nav.clip_view.sampler.knob;
        assert!(deep >= phosphor_app::sampler::knobs::PadKnob::PAD_CONTROLS);
        press(&mut app, KeyCode::Enter);

        press(&mut app, KeyCode::Char('l')); // held: this turns the knob
        press(&mut app, KeyCode::Esc);
        app.sampler_follow_note(30); // a pad with nothing on it
        assert!(
            app.nav.clip_view.sampler.knob < phosphor_app::sampler::knobs::PadKnob::PAD_CONTROLS,
            "the cursor stayed on a control the pad has not got",
        );
        // And the empty pad's own controls still answer.
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('l'));
        let _ = screen(&app, 120, 40);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `a` still asks for a file from the pad map, which is where a player
    /// spends their time.
    #[test]
    fn a_loads_a_sound_from_the_pad_map() {
        let mut app = sampler_app();
        assert_eq!(app.nav.clip_view.clip_tab, ClipTab::Pads);
        press(&mut app, KeyCode::Char('a'));
        assert!(app.nav.input_modal.open, "`a` did not ask for a file");
        assert_eq!(app.nav.input_modal.kind, InputModalKind::SamplePath);
    }

    /// A sampler track that is deleted and brought back brings its kit with
    /// it — the buffers were in the undo step all along.
    #[test]
    fn undoing_a_deleted_sampler_track_brings_the_kit_back() {
        let dir = scratch("track");
        let mut app = loaded_app(&dir);
        std::fs::remove_file(dir.join("kick.wav")).unwrap();
        let idx = app.nav.track_cursor;
        app.execute_confirm(ConfirmKind::DeleteTrack); // the modal's `y`
        assert!(app.nav.tracks.get(idx).is_none_or(|t| t.sampler.is_none()));

        press(&mut app, KeyCode::Char('u'));
        let state = sampler_state(&app);
        let layer = &state.pads[state.cursor].layers[0];
        assert_eq!(layer.name, "kick");
        assert!(layer.pcm.is_some(), "the kit came back without its audio");
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::SetSamplerPad { .. })),
            "the rebuilt track's pads were never sent to the engine",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── The trim strip ──

    /// The layer the strip is on.
    fn trimmed(app: &App) -> &phosphor_app::sampler::LayerState {
        let layer = app.nav.clip_view.sampler.layer;
        &pad(app).layers[layer]
    }

    fn trim_view(app: &App) -> phosphor_app::state::TrimView {
        app.nav.clip_view.sampler.trim.expect("the trim strip is not open")
    }

    /// Commands the app has sent that are auditions, and what they asked for.
    fn previews(app: &App) -> Vec<Option<phosphor_plugin::sample::PreviewMode>> {
        app.drain_mixer_commands()
            .into_iter()
            .filter_map(|c| match c {
                MixerCommand::SetSamplerPreview { preview, .. } => {
                    Some(preview.map(|p| p.mode))
                }
                _ => None,
            })
            .collect()
    }

    /// `t` opens the strip on a layer that has audio, and the pane turns
    /// into a waveform with a header over it.
    #[test]
    fn t_opens_the_trim_strip_on_a_loaded_layer() {
        let dir = scratch("trim-open");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('t'));
        assert!(app.nav.clip_view.sampler.trim.is_some(), "`t` did not open the strip");

        let text = screen(&app, 120, 40);
        assert!(text.contains("unit 10ms"), "no unit on the header:\n{text}");
        assert!(text.contains("snap on"), "snap is not on by default:\n{text}");
        assert!(text.contains("0.000s \u{2192} 0.100s"), "no trim times:\n{text}");
        assert!(text.contains("-- TRIM --"), "the bar does not say which mode:\n{text}");
        assert!(text.contains("[PAD:C3 trim]"), "the strip does not say either:\n{text}");
        assert!(text.contains('['), "no start marker on the screen:\n{text}");
        assert!(text.contains("esc back"), "no way out on the screen:\n{text}");
        // The keyboard band stays: a player trimming still has to be able to
        // see which pad they are trimming.
        assert!(text.contains("\u{25BC}"), "the bed went away with the map:\n{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A layer with no audio is refused in words, and so is an empty pad.
    /// Both are one keypress away from a pad that would have worked.
    #[test]
    fn t_refuses_a_layer_it_cannot_draw() {
        let dir = scratch("trim-refuse");
        let mut app = loaded_app(&dir);
        // An empty pad two keys up.
        app.sampler_follow_note(62);
        press(&mut app, KeyCode::Char('t'));
        assert!(app.nav.clip_view.sampler.trim.is_none(), "the strip opened over nothing");
        let (message, _) = app.status_message.as_ref().expect("no word to the player");
        assert!(message.contains("nothing on this pad"), "unhelpful: {message}");

        // ...and a layer whose file went missing.
        app.sampler_follow_note(60);
        {
            let track = app.nav.tracks.iter_mut().find(|t| t.sampler.is_some()).unwrap();
            let sampler = track.sampler.as_mut().unwrap();
            let pad = sampler.cursor;
            sampler.pads[pad].layers[0].pcm = None;
        }
        press(&mut app, KeyCode::Char('t'));
        assert!(app.nav.clip_view.sampler.trim.is_none(), "the strip opened over a missing file");
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("lost its file"), "unhelpful: {message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The Right-Left Trick, on a waveform: `h`/`l` take the start, `H`/`L`
    /// the end, and neither touches the other's marker.
    #[test]
    fn the_edges_move_independently_and_the_engine_hears_every_move() {
        let dir = scratch("trim-nudge");
        let mut app = loaded_app(&dir); // 4 410 frames at 44.1 kHz
        press(&mut app, KeyCode::Char('t'));
        let _ = app.drain_mixer_commands();

        // One press of 10 ms is 441 frames. The wav is a 220 Hz sine, whose
        // crossings fall every 100 frames, so the snap moves this a little —
        // the region's end is what must not move at all.
        press(&mut app, KeyCode::Char('l'));
        assert!(trimmed(&app).start_frame > 0, "`l` did not move the start");
        assert_eq!(trimmed(&app).end_frame, 4_410, "`l` moved the end too");
        let moved = trimmed(&app).start_frame;

        press_shift(&mut app, 'H');
        assert!(trimmed(&app).end_frame < 4_410, "`H` did not move the end");
        assert_eq!(trimmed(&app).start_frame, moved, "`H` moved the start too");

        // Every one of those reached the engine, as a pad *and* as a sound.
        let commands = app.drain_mixer_commands();
        let pads = commands
            .iter()
            .filter(|c| matches!(c, MixerCommand::SetSamplerPad { .. }))
            .count();
        assert_eq!(pads, 2, "the engine was not told about both nudges: {pads}");
        let auditions = commands
            .iter()
            .filter(|c| matches!(c, MixerCommand::SetSamplerPreview { preview: Some(_), .. }))
            .count();
        assert_eq!(auditions, 2, "a nudge did not audition: {auditions}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A run of nudges is one press of `u` — a player deciding where the
    /// start goes has made one decision, not thirty.
    #[test]
    fn a_nudge_run_coalesces_into_one_undo_step() {
        let dir = scratch("trim-undo");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('t'));
        for _ in 0..6 {
            press(&mut app, KeyCode::Char('l'));
        }
        assert!(trimmed(&app).start_frame > 0);

        press(&mut app, KeyCode::Char('u'));
        assert_eq!(trimmed(&app).start_frame, 0, "one `u` did not take back the whole run");
        // And the step before it is the load, not another slice of the run.
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(pad(&app).layers.len(), 0, "the run left more than one step behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The edges stop a millisecond apart and say so, rather than meeting.
    #[test]
    fn the_start_stops_a_millisecond_short_of_the_end() {
        let dir = scratch("trim-floor");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('t'));
        press(&mut app, KeyCode::Char('k')); // 10ms -> 1/16
        press(&mut app, KeyCode::Char('k')); // -> beat
        press(&mut app, KeyCode::Char('k')); // -> bar
        for _ in 0..10 {
            press(&mut app, KeyCode::Char('l'));
        }
        let layer = trimmed(&app);
        assert!(layer.start_frame < layer.end_frame, "the edges met or crossed");
        assert!(
            layer.end_frame - layer.start_frame >= 44, // 1 ms at 44.1 kHz
            "the region is {} frames",
            layer.end_frame - layer.start_frame,
        );
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("shortest region"), "the floor was silent: {message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `j` and `k` walk the unit ladder, and the header follows.
    #[test]
    fn j_and_k_walk_the_nudge_unit_and_the_header_says_which() {
        let dir = scratch("trim-unit");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('t'));
        assert!(screen(&app, 120, 40).contains("unit 10ms"));

        press(&mut app, KeyCode::Char('j'));
        assert!(screen(&app, 120, 40).contains("unit 1ms"), "`j` did not go deeper");
        press(&mut app, KeyCode::Char('j'));
        assert!(screen(&app, 120, 40).contains("unit 1smp"), "`j` did not reach samples");
        // The bottom of the ladder is a wall.
        for _ in 0..5 {
            press(&mut app, KeyCode::Char('j'));
        }
        assert!(screen(&app, 120, 40).contains("unit 1smp"));

        // ...and back up to bars, where one press is two seconds at 120 BPM.
        for _ in 0..10 {
            press(&mut app, KeyCode::Char('k'));
        }
        let text = screen(&app, 120, 40);
        assert!(text.contains("unit bar"), "`k` did not reach bars:\n{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `z` turns the snap off and on, and the header says which it is.
    #[test]
    fn z_toggles_the_zero_crossing_snap() {
        let dir = scratch("trim-snap");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('t'));
        assert!(trim_view(&app).snap, "the snap is not on by default");

        press(&mut app, KeyCode::Char('z'));
        assert!(!trim_view(&app).snap);
        let text = screen(&app, 120, 40);
        assert!(text.contains("snap off"), "the header still says on:\n{text}");

        // With it off, a press lands exactly where the unit says: 10 ms of a
        // 44.1 kHz recording is 441 frames, no more and no less.
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(trimmed(&app).start_frame, 441, "the snap moved an edge it was off for");

        press(&mut app, KeyCode::Char('z'));
        assert!(trim_view(&app).snap);
        assert!(screen(&app, 120, 40).contains("snap on"));
        // And with it on, the edge lands on a crossing of the 220 Hz sine —
        // every 100 frames or so, and never on the 882 the unit alone says.
        press(&mut app, KeyCode::Char('l'));
        assert_ne!(trimmed(&app).start_frame, 882, "the snap did nothing");
        assert!((782..=982).contains(&trimmed(&app).start_frame), "the snap left its cap");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `r` reverses the layer from inside the strip, keeps the markers where
    /// they were, and tells the engine.
    #[test]
    fn r_reverses_the_layer_without_moving_its_markers() {
        let dir = scratch("trim-rev");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('t'));
        press(&mut app, KeyCode::Char('l'));
        let region = (trimmed(&app).start_frame, trimmed(&app).end_frame);
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Char('r'));
        assert!(trimmed(&app).reverse, "`r` did not reverse the layer");
        assert_eq!(
            (trimmed(&app).start_frame, trimmed(&app).end_frame),
            region,
            "reversing moved the trim",
        );
        assert!(screen(&app, 120, 40).contains("rev"), "the header does not say so");
        assert!(
            app.drain_mixer_commands()
                .iter()
                .any(|c| matches!(c, MixerCommand::SetSamplerPad { .. })),
            "the reverse never reached the engine",
        );

        press(&mut app, KeyCode::Char('r'));
        assert!(!trimmed(&app).reverse);
        // Each direction is its own step, the way a mute is.
        press(&mut app, KeyCode::Char('u'));
        assert!(trimmed(&app).reverse, "undo did not put the reverse back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `t` inside the strip loops the region, and every nudge after it keeps
    /// looping rather than dropping back to a single pass.
    #[test]
    fn t_inside_the_strip_loops_the_region() {
        let dir = scratch("trim-loop");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('t'));
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Char('t'));
        assert!(trim_view(&app).looping);
        assert_eq!(previews(&app), vec![Some(phosphor_plugin::sample::PreviewMode::Loop)]);
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(
            previews(&app),
            vec![Some(phosphor_plugin::sample::PreviewMode::Loop)],
            "a nudge dropped the loop",
        );

        press(&mut app, KeyCode::Char('t'));
        assert!(!trim_view(&app).looping);
        assert_eq!(previews(&app), vec![None], "turning the loop off did not silence it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `esc` goes back to the pad map with everything on it, and takes the
    /// audition with it.
    #[test]
    fn esc_leaves_the_strip_quiet_and_the_pad_map_intact() {
        let dir = scratch("trim-esc");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('t'));
        press(&mut app, KeyCode::Char('t')); // looping
        press(&mut app, KeyCode::Char('l'));
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Esc);
        assert!(app.nav.clip_view.sampler.trim.is_none(), "esc did not leave the strip");
        assert_eq!(previews(&app), vec![None], "esc left the loop playing");

        let text = screen(&app, 120, 40);
        assert!(text.contains("pad C3"), "the panel did not come back:\n{text}");
        assert!(text.contains("1 filled"), "the pad list did not come back:\n{text}");
        assert!(text.contains("kick"), "the layer list did not come back:\n{text}");
        assert!(text.contains("-- PADS --"), "the bar still says trim:\n{text}");
        // And the keys are the map's again.
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(
            SamplerState::note_of_pad(sampler_state(&app).cursor),
            61,
            "`l` did not go back to walking the bed",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Walking away from the pad map stops the audition, whichever way the
    /// player walked. Nobody presses `esc` on the way to another tab.
    #[test]
    fn leaving_the_pads_tab_silences_a_looping_audition() {
        let dir = scratch("trim-leave");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('t'));
        press(&mut app, KeyCode::Char('t')); // looping
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Tab);
        assert_ne!(app.nav.clip_view.clip_tab, ClipTab::Pads);
        assert_eq!(previews(&app), vec![None], "the loop followed the player out of the tab");

        // And one keystroke later it is not sending anything else.
        press(&mut app, KeyCode::Tab);
        assert!(previews(&app).is_empty(), "the audition is still being switched off");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Moving the layer cursor sounds what it lands on — the deferral M3
    /// made, which the trim strip's machinery pays for.
    #[test]
    fn walking_the_layer_list_auditions_each_sound() {
        let dir = scratch("trim-audition");
        let mut app = loaded_app(&dir);
        let wav = dir.join("clap.wav");
        write_wav(&wav, 800);
        press(&mut app, KeyCode::Char('a'));
        type_line(&mut app, &wav.display().to_string());
        press(&mut app, KeyCode::Enter);
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Char('['));
        assert_eq!(app.nav.clip_view.sampler.layer, 0);
        assert_eq!(previews(&app), vec![Some(phosphor_plugin::sample::PreviewMode::Once)]);
        press(&mut app, KeyCode::Char(']'));
        assert_eq!(previews(&app), vec![Some(phosphor_plugin::sample::PreviewMode::Once)]);
        // The end of the list is not a move and not a sound.
        press(&mut app, KeyCode::Char(']'));
        assert!(previews(&app).is_empty(), "a cursor that did not move made a sound");

        // A muted layer auditions as silence rather than as the one sound in
        // the box that plays while it is muted.
        press(&mut app, KeyCode::Char('m'));
        let _ = app.drain_mixer_commands();
        press(&mut app, KeyCode::Char('1'));
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(previews(&app), vec![Some(phosphor_plugin::sample::PreviewMode::Once), None]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A pad cursor that walks onto an empty pad closes the strip — and
    /// takes its loop with it. Playing a key moves that cursor, so this
    /// happens with nothing pressed on the computer keyboard at all, which
    /// is exactly where a loop would otherwise be left playing with nothing
    /// on the screen to explain it.
    #[test]
    fn playing_an_empty_pad_closes_a_strip_open_over_it_and_stops_its_loop() {
        let dir = scratch("trim-follow");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('t'));
        press(&mut app, KeyCode::Char('t')); // looping
        assert!(app.nav.clip_view.sampler.trim.is_some());
        let _ = app.drain_mixer_commands();

        app.sampler_follow_note(70); // a pad with nothing on it
        assert!(app.nav.clip_view.sampler.trim.is_none(), "a strip stayed open over nothing");
        assert_eq!(previews(&app), vec![None], "the loop outlived its strip");
        // And the map is what the keys are on again.
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(SamplerState::note_of_pad(sampler_state(&app).cursor), 71);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Muting or removing a layer stops the audition of it. The preview
    /// carries its own copy of the layer — which is what lets it sound a
    /// trim the pad has not been told about yet, and what would otherwise
    /// leave a ten-second sample playing after the sound was muted.
    #[test]
    fn an_edit_under_a_running_audition_silences_it() {
        let dir = scratch("trim-edit");
        let mut app = loaded_app(&dir);
        let wav = dir.join("clap.wav");
        write_wav(&wav, 800);
        press(&mut app, KeyCode::Char('a'));
        type_line(&mut app, &wav.display().to_string());
        press(&mut app, KeyCode::Enter);

        press(&mut app, KeyCode::Char('[')); // audition layer one
        let _ = app.drain_mixer_commands();
        press(&mut app, KeyCode::Char('m'));
        assert!(pad(&app).layers[0].mute);
        assert_eq!(previews(&app), vec![None], "a muted layer kept sounding");

        // Unmuting does not start one: nothing asked for a sound.
        press(&mut app, KeyCode::Char('m'));
        assert!(previews(&app).is_empty(), "an unmute started an audition nobody asked for");

        // And a layer taken off the pad cannot be sounding either.
        press(&mut app, KeyCode::Char(']'));
        let _ = app.drain_mixer_commands();
        press(&mut app, KeyCode::Char('d'));
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(pad(&app).layers.len(), 1);
        assert_eq!(previews(&app), vec![None], "a removed layer kept sounding");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A trim round-trips through a session file: the region is what the
    /// player left, not the whole file again.
    #[test]
    fn a_trim_survives_save_and_load() {
        let dir = scratch("trim-session");
        let wav = dir.join("kick.wav");
        write_wav(&wav, 4_410);
        let session = dir.join("trim.phos");

        let mut saving = sampler_app();
        press(&mut saving, KeyCode::Char('a'));
        type_line(&mut saving, &wav.display().to_string());
        press(&mut saving, KeyCode::Enter);
        press(&mut saving, KeyCode::Char('t'));
        press(&mut saving, KeyCode::Char('z')); // snap off, so the frames are exact
        press(&mut saving, KeyCode::Char('l'));
        press_shift(&mut saving, 'H');
        let region = (trimmed(&saving).start_frame, trimmed(&saving).end_frame);
        assert_eq!(region, (441, 3_969));
        saving.do_save(&session.display().to_string());

        let mut loading = app();
        loading.do_load(&session.display().to_string());
        let state = sampler_state(&loading);
        let layer = &state.pads[state.cursor].layers[0];
        assert_eq!((layer.start_frame, layer.end_frame), region, "the trim did not survive");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_sampler_writes_no_sampler_block() {
        // The byte-stability promise: a session with nothing on the pads
        // is a file that never mentions them.
        let dir = scratch("empty");
        let session = dir.join("bare.phos");
        let mut saving = sampler_app();
        saving.do_save(&session.display().to_string());
        let json = std::fs::read_to_string(&session).unwrap();
        assert!(!json.contains("\"pads\""), "an empty kit was written out:\n{json}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
