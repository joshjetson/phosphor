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
    use phosphor_app::sampler::capture::SourceMode;
    use phosphor_app::sampler::{SamplerState, TakeKind};
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

    /// The QA sweep's worst find: with a confirm on the screen, a played
    /// key moved the cursor, and "remove kick from pad C3?" answered yes
    /// removed the snare. A question freezes the pad it named.
    #[test]
    fn a_modal_freezes_the_pad_cursor() {
        let dir = scratch("modalfreeze");
        let wav = dir.join("kick.wav");
        write_wav(&wav, 441);
        let mut app = sampler_app();
        app.sampler_follow_note(60);
        press(&mut app, KeyCode::Char('a'));
        type_line(&mut app, &wav.display().to_string());
        press(&mut app, KeyCode::Enter);

        // The delete confirm goes up on C3; a stray key must not move it.
        press(&mut app, KeyCode::Char('d'));
        assert!(app.nav.confirm_modal.open);
        app.sampler_follow_note(62);
        assert_eq!(
            phosphor_app::sampler::SamplerState::note_of_pad(sampler_state(&app).cursor),
            60,
            "the confirm let the cursor walk"
        );
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(sampler_state(&app).pads[60 - 21].layers.len(), 0, "the named pad kept its layer");

        // The load prompt freezes it the same way.
        press(&mut app, KeyCode::Char('a'));
        assert!(app.nav.input_modal.open);
        app.sampler_follow_note(64);
        assert_eq!(
            phosphor_app::sampler::SamplerState::note_of_pad(sampler_state(&app).cursor),
            60,
            "the prompt let the cursor walk"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A session file that will not parse must cost nothing: source mode
    /// used to be abandoned before the parse, and a failed open left the
    /// sampler out of its slot — a silent kit with a normal screen.
    #[test]
    fn a_failed_open_leaves_source_mode_standing() {
        let mut app = sampler_app();
        press(&mut app, KeyCode::Char('i'));
        assert!(app.nav.instrument_modal.open);
        press(&mut app, KeyCode::Enter);
        assert!(app.nav.sampler_source.is_some(), "source mode never began");

        app.do_load("/nowhere/at/all.phos");
        let (message, _) = app.status_message.as_ref().expect("no word to the player");
        assert!(message.contains("open failed"), "{message}");
        assert!(
            app.nav.sampler_source.is_some(),
            "a failed open tore down source mode anyway"
        );
    }

    /// Root-learn armed before source mode must not survive into it: the
    /// banner hides the question, and the first note after leaving used
    /// to retune the zone instead of being a note.
    #[test]
    fn root_learn_dies_at_the_source_door() {
        let mut app = sampler_app();
        press(&mut app, KeyCode::Char('K'));
        press(&mut app, KeyCode::Char('w'));
        press(&mut app, KeyCode::Char('R'));
        assert!(app.nav.clip_view.sampler.root_learn, "R never armed the learn");
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);
        assert!(
            !app.nav.clip_view.sampler.root_learn,
            "the learn survived into source mode"
        );
    }

    /// A full pad still opens the prompt: refusing at `a` meant the path
    /// being typed ran as key commands — QA watched the `d` in a filename
    /// delete a layer. The refusal belongs at Enter, behind the field.
    #[test]
    fn a_full_pad_still_gets_its_prompt() {
        let dir = scratch("fullprompt");
        let wav = dir.join("hat.wav");
        write_wav(&wav, 441);
        let mut app = sampler_app();
        for _ in 0..phosphor_app::sampler::MAX_LAYERS {
            press(&mut app, KeyCode::Char('a'));
            type_line(&mut app, &wav.display().to_string());
            press(&mut app, KeyCode::Enter);
        }
        press(&mut app, KeyCode::Char('a'));
        assert!(app.nav.input_modal.open, "the full pad refused the prompt");
        type_line(&mut app, "drums/yell.wav");
        press(&mut app, KeyCode::Enter);
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
        let cleared = synced(&app);
        assert!(
            cleared.contains(&(second, 0)),
            "the emptied pad was never cleared in the engine: {cleared:?}",
        );
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

    /// Tab does not walk out of the strip.
    ///
    /// The strip's own doc says it "owns every key while it is open", and a
    /// held knob on the same tab is already guarded that way. Tab was the one
    /// key that walked, which meant the two modes with the same stated
    /// contract behaved differently and only one of them could be predicted.
    /// `esc` is still the way out.
    #[test]
    fn tab_does_not_walk_out_of_the_trim_strip() {
        let dir = scratch("trim-tab");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('t'));
        let tab = app.nav.clip_view.clip_tab;

        for _ in 0..4 {
            press(&mut app, KeyCode::Tab);
        }
        assert_eq!(app.nav.clip_view.clip_tab, tab, "Tab walked the strip off the tab");
        assert!(app.nav.clip_view.sampler.trim.is_some(), "the strip was left behind");
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.nav.clip_view.clip_tab, tab, "shift-Tab walked out instead");
        assert!(app.nav.clip_view.sampler.trim.is_some());

        // And `esc` still is the way out.
        press(&mut app, KeyCode::Esc);
        assert!(app.nav.clip_view.sampler.trim.is_none(), "esc stopped closing the strip");
        press(&mut app, KeyCode::Tab);
        assert_ne!(app.nav.clip_view.clip_tab, tab, "Tab stopped working once the strip was shut");
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

    /// Walking away from the pad map stops the audition.
    ///
    /// A loop belongs to the strip, and the strip does not let Tab past it,
    /// so the way out of a loop is `esc` — which silences it on the way. An
    /// audition started on the pad map itself does follow the player out of
    /// the tab, which is the rule this exists for: nobody presses `esc` on
    /// the way to another tab.
    #[test]
    fn leaving_the_pads_tab_silences_a_looping_audition() {
        let dir = scratch("trim-leave");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('t'));
        press(&mut app, KeyCode::Char('t')); // looping
        let _ = app.drain_mixer_commands();

        // Tab is the strip's, so the loop is left playing for the player who
        // is still looking at it.
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.nav.clip_view.clip_tab, ClipTab::Pads);
        assert!(previews(&app).is_empty(), "the loop was silenced by a key that did nothing");

        // `esc` closes the strip, and the loop goes with it.
        press(&mut app, KeyCode::Esc);
        assert!(app.nav.clip_view.sampler.trim.is_none());
        assert_eq!(previews(&app), vec![None], "the loop outlived the strip");

        // And from the pad map, Tab really does walk away — and takes the
        // audition with it.
        press(&mut app, KeyCode::Char(']'));
        press(&mut app, KeyCode::Char('['));
        let _ = app.drain_mixer_commands();
        press(&mut app, KeyCode::Char('1')); // audition layer one
        assert!(
            previews(&app).iter().any(Option::is_some),
            "the layer list stopped auditioning",
        );
        press(&mut app, KeyCode::Tab);
        assert_ne!(app.nav.clip_view.clip_tab, ClipTab::Pads);
        assert_eq!(previews(&app), vec![None], "the audition followed the player out of the tab");

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

    // ── Source mode and the resampler ──

    /// Play a key through the same door the MIDI callback uses, so the
    /// modes that intercept the stream get their say.
    fn play(app: &mut App, note: u8, velocity: u8, on: bool, at: u64) {
        use phosphor_midi::MidiMessageType;
        let event = if on {
            MidiMessageType::NoteOn { channel: 0, note, velocity }
        } else {
            MidiMessageType::NoteOff { channel: 0, note, velocity: 0 }
        };
        app.handle_tap_event(event, Some(at));
    }

    /// A short performance on one key, stamped from now.
    fn perform(app: &mut App, note: u8) {
        let t0 = phosphor_midi::clock::now_micros();
        play(app, note, 100, true, t0);
        play(app, note, 0, false, t0 + 150_000);
    }

    /// A sampler track in source mode on its current pad, playing the
    /// Phosphor synth — the whole way in, through the real keys.
    fn source_app() -> App {
        let mut app = sampler_app();
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter); // the first entry: Phosphor Synth
        app
    }

    /// Space is refused in words while source mode is on.
    ///
    /// The mode has the track's plugin slot on loan, so the sampler is not
    /// playing and half of what the space menu opens is about a kit that is
    /// not in the box right now. Every other key the mode does not take is
    /// answered with a sentence; space used to be the one that opened a menu
    /// over it instead. It is refused from every tab, because the mode is on
    /// from every tab.
    #[test]
    fn space_is_refused_in_words_while_source_mode_is_on() {
        let mut app = source_app();
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.nav.space_menu.open, "the space menu opened over source mode");
        let text = screen(&app, 120, 40);
        assert!(text.contains("source mode is on"), "the refusal said nothing:\n{text}");
        assert!(text.contains("esc puts the sampler back"), "no way out named:\n{text}");
        assert!(text.contains("-- SOURCE --"), "the mode was left:\n{text}");

        // From another tab too: the slot is on loan wherever the player is
        // standing.
        press(&mut app, KeyCode::Tab);
        assert_ne!(app.nav.clip_view.clip_tab, ClipTab::Pads);
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.nav.space_menu.open, "the menu opened from the next tab along");
        assert!(app.in_sampler_source(), "the refusal cost the mode");

        // And the moment the sampler is back, space is space again.
        for _ in 0..8 {
            if app.nav.clip_view.clip_tab == ClipTab::Pads {
                break;
            }
            press(&mut app, KeyCode::Tab);
        }
        assert_eq!(app.nav.clip_view.clip_tab, ClipTab::Pads, "never found the way back");
        press(&mut app, KeyCode::Esc);
        assert!(!app.in_sampler_source());
        press(&mut app, KeyCode::Char(' '));
        assert!(app.nav.space_menu.open, "space stopped opening the menu");
    }

    /// `i` opens the picker for the pad, and it offers what can be
    /// recorded from — never the sequencer, and never the sampler itself.
    #[test]
    fn i_offers_every_instrument_that_can_be_recorded_from() {
        let mut app = sampler_app();
        press(&mut app, KeyCode::Char('i'));
        assert!(app.nav.instrument_modal.open, "`i` did not ask");
        let items = app.nav.instrument_modal.items();
        assert!(!items.contains(&InstrumentType::Sequencer), "the sequencer is not an instrument");
        assert!(
            !items.contains(&InstrumentType::Sampler),
            "a sampler recording itself is a feedback loop",
        );
        assert_eq!(items.len(), InstrumentType::ALL.len() - 2);
        let text = screen(&app, 120, 40);
        assert!(text.contains("record this pad from"), "the menu does not say what for:\n{text}");
    }

    /// Entering puts the instrument in the track's slot, says so, and
    /// freezes the pad cursor: the keys are a performance now.
    #[test]
    fn source_mode_swaps_the_instrument_and_freezes_the_pad_cursor() {
        let mut app = sampler_app();
        let pad = sampler_state(&app).cursor;
        let _ = app.drain_mixer_commands();
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);

        assert!(app.nav.sampler_source.is_some(), "the mode did not start");
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::SetInstrument { .. })),
            "the track never took the instrument",
        );
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::SetParameter { .. })),
            "the instrument arrived with no panel behind it",
        );
        let text = screen(&app, 120, 40);
        assert!(text.contains("-- SOURCE --"), "the bar does not say which mode:\n{text}");
        assert!(text.contains("source \u{00b7} Phosphor Synth"), "no banner:\n{text}");
        assert!(text.contains("[PAD:C3 source]"), "the strip does not say either:\n{text}");

        // A key played is a performance: the pad cursor stays put.
        play(&mut app, 72, 100, true, phosphor_midi::clock::now_micros());
        assert_eq!(sampler_state(&app).cursor, pad, "a played key moved the pad cursor");
        // ...and so do the keys that walk the bed, which says so.
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(sampler_state(&app).cursor, pad, "`l` walked out of the mode's pad");
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("source mode is on pad C3"), "unhelpful: {message}");
    }

    /// The pad remembers what it was recorded from, and `i` comes back
    /// standing on it.
    #[test]
    fn the_pad_remembers_its_instrument_and_its_panel() {
        let mut app = sampler_app();
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Char('j')); // Drum Rack
        press(&mut app, KeyCode::Enter);
        let pad = sampler_state(&app).cursor;
        let source = sampler_state(&app).pads[pad].source.clone().expect("no memory");
        assert_eq!(source.instrument, InstrumentType::DrumRack);
        assert_eq!(source.params.len(), phosphor_app::preset::defaults(InstrumentType::DrumRack).len());

        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('i'));
        assert_eq!(
            app.nav.instrument_modal.selected(),
            InstrumentType::DrumRack,
            "the picker did not open on the pad's own instrument",
        );
    }

    /// `esc` puts the sampler back — and replays the kit into it, because
    /// the instance that comes back is empty.
    #[test]
    fn esc_restores_the_sampler_and_replays_the_kit() {
        let dir = scratch("source-esc");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Esc);
        assert!(app.nav.sampler_source.is_none(), "esc did not leave the mode");
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::SetInstrument { .. })),
            "the sampler never came back",
        );
        let replayed = commands
            .iter()
            .filter(|c| matches!(c, MixerCommand::SetSamplerPad { layers, .. } if !layers.is_empty()))
            .count();
        assert_eq!(replayed, 1, "the kit was not replayed into the fresh sampler");

        let text = screen(&app, 120, 40);
        assert!(text.contains("-- PADS --"), "the bar still says source:\n{text}");
        assert!(text.contains("kick"), "the kit went away with the mode:\n{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A performance lands as a take: selected, named, timed, and one
    /// press of `u` deep.
    #[test]
    fn a_performance_lands_as_a_take_in_one_undo_step() {
        let mut app = source_app();
        press(&mut app, KeyCode::Char('r'));
        perform(&mut app, 60);
        press(&mut app, KeyCode::Char('r'));

        let state = sampler_state(&app);
        let pad = state.cursor;
        assert_eq!(state.pads[pad].layers.len(), 1, "no take landed");
        let layer = &state.pads[pad].layers[0];
        assert_eq!(layer.name, "take 1");
        assert_eq!(layer.source, phosphor_app::sampler::LayerSource::Take);
        let pcm = layer.pcm.as_ref().expect("a take with no audio");
        assert_eq!(pcm.sample_rate, 44_100.0, "the take is not at the engine's rate");
        assert!(pcm.frames() > 0);
        assert_eq!(app.nav.clip_view.sampler.layer, 0, "the panel is not on the new take");
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("take 1"), "the flash does not name the take: {message}");
        assert!(message.contains("peak"), "the flash does not give the peak: {message}");

        // One step back off the pad, and redo brings it back with its
        // audio — the buffer was in the step all along.
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(pad_layers(&app, pad), 0, "one `u` did not take the take off");
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(
                c,
                MixerCommand::SetSamplerPad { layers, .. } if layers.is_empty()
            )),
            "undo left the take in the engine",
        );
        app.handle_event(Event::Key(KeyEvent {
            code: KeyCode::Char('r'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert_eq!(pad_layers(&app, pad), 1, "redo did not bring the take back");
        assert!(
            sampler_state(&app).pads[pad].layers[0].pcm.is_some(),
            "the take came back with no audio behind it",
        );
    }

    fn pad_layers(app: &App, pad: usize) -> usize {
        sampler_state(app).pads[pad].layers.len()
    }

    /// Armed and nothing played: nothing lands, and it says so. A
    /// zero-frame layer would be a sound that cannot be heard, trimmed or
    /// removed without a puzzle.
    #[test]
    fn an_empty_take_lands_nothing_and_says_so() {
        let mut app = source_app();
        press(&mut app, KeyCode::Char('r'));
        press(&mut app, KeyCode::Char('r'));
        assert_eq!(pad_layers(&app, sampler_state(&app).cursor), 0, "silence landed on the pad");
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("nothing played"), "unhelpful: {message}");
        assert!(app.nav.sampler_source.is_some(), "an empty take ended the mode");
    }

    /// A single-pitch performance teaches the pad its root; a chord does
    /// not, because there is nothing for it to teach.
    #[test]
    fn a_one_finger_take_teaches_the_pad_its_root() {
        let mut app = source_app();
        press(&mut app, KeyCode::Char('r'));
        perform(&mut app, 45);
        press(&mut app, KeyCode::Char('r'));
        let pad = sampler_state(&app).cursor;
        assert_eq!(sampler_state(&app).pads[pad].config.root, 45);
        assert!(!sampler_state(&app).pads[pad].config.keytrack, "a root turned keytrack on");

        // A chord leaves the root where the first take put it.
        press(&mut app, KeyCode::Char('r'));
        let t0 = phosphor_midi::clock::now_micros();
        play(&mut app, 50, 100, true, t0);
        play(&mut app, 57, 100, true, t0 + 1_000);
        play(&mut app, 50, 0, false, t0 + 120_000);
        play(&mut app, 57, 0, false, t0 + 130_000);
        press(&mut app, KeyCode::Char('r'));
        assert_eq!(pad_layers(&app, pad), 2, "the second take did not land");
        assert_eq!(sampler_state(&app).pads[pad].config.root, 45, "a chord moved the root");
        assert_eq!(sampler_state(&app).pads[pad].layers[1].name, "take 2");
    }

    /// Rolling, a take is whole bars — the length that loops.
    #[test]
    fn a_take_against_the_transport_is_whole_bars() {
        let mut app = source_app();
        app.engine.transport.play();
        press(&mut app, KeyCode::Char('r'));
        perform(&mut app, 64);
        press(&mut app, KeyCode::Char('r'));

        let state = sampler_state(&app);
        let layer = &state.pads[state.cursor].layers[0];
        let pcm = layer.pcm.as_ref().expect("no take");
        // One bar of 4/4 at 120 BPM, at 44.1 kHz: exactly two seconds.
        assert_eq!(pcm.frames(), 88_200, "the take is not a whole bar");
        // ...and it is not auto-trimmed, or the loop point would move.
        assert_eq!(layer.start_frame, 0);
        assert_eq!(layer.end_frame, 88_200);
        app.engine.transport.pause();
    }

    /// A full pad refuses the arm before the performance, not after it.
    #[test]
    fn a_full_pad_refuses_the_arm_before_a_note_is_played() {
        let dir = scratch("source-full");
        let wav = dir.join("hat.wav");
        write_wav(&wav, 441);
        let mut app = sampler_app();
        for _ in 0..phosphor_app::sampler::MAX_LAYERS {
            press(&mut app, KeyCode::Char('a'));
            type_line(&mut app, &wav.display().to_string());
            press(&mut app, KeyCode::Enter);
        }
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('r'));
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("full"), "the refusal said: {message}");
        assert!(
            app.nav.sampler_source.as_deref().is_some_and(|m| !m.is_armed()),
            "a full pad armed anyway",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Stopping the transport ends a take: it is a performance against the
    /// transport, and the transport stopped.
    #[test]
    fn stopping_the_transport_lands_the_take() {
        let mut app = source_app();
        app.engine.transport.play();
        press(&mut app, KeyCode::Char('r'));
        perform(&mut app, 60);
        app.stop_playback();
        assert_eq!(pad_layers(&app, sampler_state(&app).cursor), 1, "the stop lost the take");
        assert!(
            app.nav.sampler_source.as_deref().is_some_and(|m| !m.is_armed()),
            "the take is still armed after a stop",
        );
    }

    /// `n` normalizes as a gain and `n` again puts it back. The buffer is
    /// never rewritten — the same audio, at a different level.
    #[test]
    fn n_normalizes_a_layer_as_a_gain_and_toggles_back() {
        let mut app = source_app();
        press(&mut app, KeyCode::Char('r'));
        perform(&mut app, 60);
        press(&mut app, KeyCode::Char('r'));
        press(&mut app, KeyCode::Esc); // back to the map, where `n` lives

        let pad = sampler_state(&app).cursor;
        let before = sampler_state(&app).pads[pad].layers[0].pcm.clone().unwrap();
        press(&mut app, KeyCode::Char('n'));
        let layer = &sampler_state(&app).pads[pad].layers[0];
        assert!(layer.gain > 1.0, "normalize did not raise the level");
        // Either it reached -0.5 dB or it ran out of level control — our
        // instruments render with enough headroom that one quiet note can
        // want more than the twelve decibels there are.
        let peak_db = 20.0 * (layer.region_peak() * layer.gain).log10();
        let ceiling = phosphor_app::sampler::knobs::MAX_GAIN;
        assert!(
            (peak_db + 0.5).abs() < 0.01 || (layer.gain - ceiling).abs() < 1e-6,
            "the normalized peak is {peak_db:.2} dB at a gain of {}",
            layer.gain,
        );
        assert!(
            std::sync::Arc::ptr_eq(&before, layer.pcm.as_ref().unwrap()),
            "normalize rewrote the buffer",
        );
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(
            message.contains("-0.5 dB") || message.contains("as far as it goes"),
            "the flash does not say what it did: {message}",
        );

        press(&mut app, KeyCode::Char('n'));
        assert_eq!(sampler_state(&app).pads[pad].layers[0].gain, 1.0, "`n` did not toggle back");
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("unity"), "the flash does not say so: {message}");

        // And each throw is its own undo step.
        press(&mut app, KeyCode::Char('u'));
        assert!(sampler_state(&app).pads[pad].layers[0].gain > 1.0, "undo lost the normalize");
    }

    /// A take survives the session: the audio is written beside the file,
    /// named relatively, and comes back playable.
    #[test]
    fn a_take_is_written_beside_the_session_and_comes_back() {
        let dir = scratch("take-session");
        let session = dir.join("kit.phos");
        let mut recording = source_app();
        press(&mut recording, KeyCode::Char('r'));
        perform(&mut recording, 60);
        press(&mut recording, KeyCode::Char('r'));
        let frames = {
            let state = sampler_state(&recording);
            state.pads[state.cursor].layers[0].pcm.as_ref().unwrap().frames()
        };
        recording.do_save(&session.display().to_string());

        let wav = dir.join("kit.samples").join("C3-1.wav");
        assert!(wav.exists(), "the take was not written beside the session");
        let json = std::fs::read_to_string(&session).unwrap();
        assert!(json.contains("kit.samples/C3-1.wav"), "the path is not in the file:\n{json}");
        assert!(!json.contains(&dir.display().to_string()), "the session names this machine");
        assert!(json.contains("\"kind\": \"take\""), "the layer is not marked a take:\n{json}");

        let mut loading = app();
        loading.do_load(&session.display().to_string());
        let state = sampler_state(&loading);
        let pad = SamplerState::pad_of_note(60).unwrap();
        assert_eq!(state.pads[pad].layers.len(), 1, "the take did not come back");
        let layer = &state.pads[pad].layers[0];
        assert_eq!(layer.source, phosphor_app::sampler::LayerSource::Take);
        assert_eq!(
            layer.pcm.as_ref().expect("the take came back silent").frames(),
            frames,
            "the take changed length on the way through the file",
        );
        assert_eq!(
            state.pads[pad].source.as_ref().map(|s| s.instrument),
            Some(InstrumentType::Synth),
            "the pad forgot what it was recorded from",
        );

        // The sidecar deleted: the pad keeps its seat, red and counted —
        // the same rule a missing wav has always followed.
        std::fs::remove_file(&wav).unwrap();
        let mut after = app();
        after.do_load(&session.display().to_string());
        let state = sampler_state(&after);
        assert_eq!(state.pads[pad].layers.len(), 1, "a missing take cost the pad its seat");
        assert!(state.pads[pad].layers[0].pcm.is_none());
        assert!(state.pads[pad].has_missing(), "the pad is not marked");
        assert_eq!(state.missing_layers(), 1);
        let (message, _) = after.status_message.as_ref().unwrap();
        assert!(message.contains("missing"), "the player was not told: {message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A second save writes nothing new: a take already on disk keeps its
    /// file.
    #[test]
    fn saving_twice_writes_the_take_once() {
        let dir = scratch("take-twice");
        let session = dir.join("kit.phos");
        let mut app = source_app();
        press(&mut app, KeyCode::Char('r'));
        perform(&mut app, 60);
        press(&mut app, KeyCode::Char('r'));
        app.do_save(&session.display().to_string());
        let wav = dir.join("kit.samples").join("C3-1.wav");
        let stamp = std::fs::metadata(&wav).unwrap().len();
        app.do_save(&session.display().to_string());
        assert_eq!(std::fs::read_dir(dir.join("kit.samples")).unwrap().count(), 1);
        assert_eq!(std::fs::metadata(&wav).unwrap().len(), stamp);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The take sounds like what the player heard: a chord device on the
    /// track is in the render.
    #[test]
    fn the_tracks_midi_devices_are_in_the_take() {
        let mut bare = source_app();
        press(&mut bare, KeyCode::Char('r'));
        perform(&mut bare, 48);
        press(&mut bare, KeyCode::Char('r'));

        let mut chorded = sampler_app();
        chorded.add_midi_fx(chorded.nav.track_cursor, crate::state::MidiFxType::Chord);
        press(&mut chorded, KeyCode::Char('i'));
        press(&mut chorded, KeyCode::Enter);
        press(&mut chorded, KeyCode::Char('r'));
        perform(&mut chorded, 48);
        press(&mut chorded, KeyCode::Char('r'));

        let one = take_peak(&bare);
        let chord = take_peak(&chorded);
        assert!(one > 0.0 && chord > 0.0, "one of the takes is silent");
        assert!(
            (one - chord).abs() > 1e-4,
            "the chord device never reached the take: {one} vs {chord}",
        );
    }

    fn take_peak(app: &App) -> f32 {
        let state = sampler_state(app);
        let pcm = state.pads[state.cursor].layers[0].pcm.as_ref().expect("no take");
        pcm.data.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// Only one track can have its plugin slot on loan: starting the mode
    /// on a second sampler gives the first its sampler back, rather than
    /// leaving a track playing a synth with nothing on the screen offering
    /// to put it right.
    #[test]
    fn a_second_source_mode_hands_the_first_track_its_sampler_back() {
        let dir = scratch("source-handoff");
        let mut app = loaded_app(&dir); // track 0: a kit on C3
        let first = app.nav.track_cursor;
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);

        app.create_instrument_track(InstrumentType::Sampler);
        let second = app.nav.track_cursor;
        assert_ne!(first, second);
        app.nav.focused_pane = Pane::ClipView;
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);
        let mode = app.nav.sampler_source.as_deref().expect("the second mode did not start");
        assert_eq!(mode.track_idx, second, "the mode stayed on the first track");
        let first_id = app.nav.tracks[first].mixer_id;
        let replayed = app.drain_mixer_commands().into_iter().any(|c| matches!(
            c,
            MixerCommand::SetSamplerPad { track_id, layers, .. }
                if Some(track_id) == first_id && !layers.is_empty()
        ));
        assert!(replayed, "the track left behind never got its kit back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The track the mode was borrowing is deleted: the mode goes with it
    /// rather than waiting to land a take on a track that is not there.
    #[test]
    fn deleting_the_track_ends_source_mode() {
        let mut app = source_app();
        press(&mut app, KeyCode::Char('r'));
        app.execute_confirm(ConfirmKind::DeleteTrack);
        press(&mut app, KeyCode::Char('l')); // any key: the reconciler runs after each
        assert!(app.nav.sampler_source.is_none(), "the mode outlived its track");
    }

    // ── Keys mode: the bed as zones ──

    /// The zone under the caret, for a test that wants to read it.
    fn zone(app: &App) -> &phosphor_app::sampler::Zone {
        let state = sampler_state(app);
        let index = state.cursor_zone().expect("no zone under the caret");
        &state.zones[index]
    }

    /// Every key the engine was told about, as `(pad, layers)`.
    ///
    /// Both shapes, flattened: a single-pad edit travels as `SetSamplerPad`
    /// and a span travels as one `SetSamplerRange`, and what these tests
    /// care about is which keys the engine now holds — not which envelope
    /// they arrived in.
    fn synced(app: &App) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for command in app.drain_mixer_commands() {
            match command {
                MixerCommand::SetSamplerPad { pad, layers, .. } => {
                    out.push((pad as usize, layers.len()));
                }
                MixerCommand::SetSamplerRange { pads, .. } => {
                    out.extend(pads.iter().map(|(pad, _, layers, _)| (*pad as usize, layers.len())));
                }
                _ => {}
            }
        }
        out
    }

    /// The same for phrases, as `(pad, phrases)`.
    fn synced_phrases(app: &App) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for command in app.drain_mixer_commands() {
            match command {
                MixerCommand::SetSamplerPhrases { pad, phrases, .. } => {
                    out.push((pad as usize, phrases.len()));
                }
                MixerCommand::SetSamplerRange { pads, .. } => {
                    out.extend(
                        pads.iter().map(|(pad, _, _, phrases)| (*pad as usize, phrases.len())),
                    );
                }
                _ => {}
            }
        }
        out
    }

    /// A kit with `kick.wav` on C3 in keys mode, with one zone across the
    /// whole bed seeded from it — the `K`-then-`w` journey, through the
    /// real keys.
    fn keys_app(dir: &std::path::Path) -> App {
        let mut app = loaded_app(dir);
        press_shift(&mut app, 'K');
        press(&mut app, KeyCode::Char('w'));
        let _ = app.drain_mixer_commands();
        app
    }

    /// A bed-wide zone edit is **one** command, not a hundred and seventy-six.
    ///
    /// The audit's S1: one press of a trim key under a zone across the bed
    /// queued 88 `SetSamplerPad` and 88 `SetSamplerPhrases`, against a drain
    /// of four allocating commands per callback. Held at a terminal's
    /// auto-repeat the queue grew faster than it emptied and the engine fell
    /// seconds behind the screen. The whole span now travels in one command
    /// charged for what it carries.
    #[test]
    fn a_bed_wide_edit_is_one_command_and_not_a_hundred_and_seventy_six() {
        let dir = scratch("keys-flood");
        let mut app = keys_app(&dir);

        // Twenty presses of the trim key: a second of auto-repeat.
        press(&mut app, KeyCode::Char('t'));
        let _ = app.drain_mixer_commands();
        for _ in 0..20 {
            press(&mut app, KeyCode::Char('l'));
        }
        let commands = app.drain_mixer_commands();
        let deliveries = commands
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    MixerCommand::SetSamplerPad { .. }
                        | MixerCommand::SetSamplerPhrases { .. }
                        | MixerCommand::SetSamplerRange { .. }
                )
            })
            .count();
        assert_eq!(deliveries, 20, "a nudge cost more than one delivery: {deliveries}");
        // Which used to be 20 x 176. The bound that matters is per press.
        assert!(
            deliveries * 176 / 20 > deliveries,
            "the comparison this test exists to make has stopped being true",
        );

        // Every one of them is a range carrying the whole bed, and the range
        // says the same thing the eighty-eight commands used to.
        let range = commands
            .iter()
            .find_map(|c| match c {
                MixerCommand::SetSamplerRange { pads, .. } => Some(pads),
                _ => None,
            })
            .expect("a bed-wide edit did not travel as a range");
        assert_eq!(range.len(), phosphor_app::sampler::NUM_PADS, "the range lost keys");
        assert!(range.iter().all(|(_, _, layers, _)| layers.len() == 1), "a key went silent");
        let notes: Vec<u8> = range.iter().map(|(pad, ..)| *pad).collect();
        assert_eq!(notes.first().copied(), Some(0));
        assert_eq!(notes.last().copied(), Some(phosphor_app::sampler::NUM_PADS as u8 - 1));

        // ...and a single-pad edit still takes the cheap road it always did.
        press(&mut app, KeyCode::Esc);
        press_shift(&mut app, 'K');
        let _ = app.drain_mixer_commands();
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('l'));
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(c, MixerCommand::SetSamplerPad { .. })),
            "a one-pad knob turn stopped taking the single-pad command",
        );
        assert!(
            !commands.iter().any(|c| matches!(c, MixerCommand::SetSamplerRange { .. })),
            "a one-pad knob turn built a range for one pad",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `K` puts the bed into zones and back, and says so in both places a
    /// player looks: the chip on the tab strip and the word on the bar.
    #[test]
    fn k_switches_the_bed_between_pads_and_keys() {
        let dir = scratch("keys-toggle");
        let mut app = loaded_app(&dir);
        let text = screen(&app, 120, 40);
        assert!(text.contains("[PAD:C3]"), "the strip does not name the pad:\n{text}");
        assert!(text.contains("-- PADS --"), "{text}");

        press_shift(&mut app, 'K');
        assert_eq!(sampler_state(&app).mode, phosphor_app::sampler::MapMode::Keys);
        let text = screen(&app, 120, 40);
        assert!(text.contains("[KEY:"), "the chip did not change:\n{text}");
        assert!(text.contains("-- KEYS --"), "the bar did not change:\n{text}");
        assert!(text.contains("no zone"), "the panel does not say the key is bare:\n{text}");

        // ...and back, with the kit still on it.
        press_shift(&mut app, 'K');
        assert_eq!(sampler_state(&app).mode, phosphor_app::sampler::MapMode::Pads);
        let text = screen(&app, 120, 40);
        assert!(text.contains("[PAD:C3]"), "{text}");
        assert!(text.contains("C3  kick"), "the pad map lost its kit:\n{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The switch resyncs the union: a pad that only sounds in the mode
    /// being left has to be told it is empty now, or it plays on with
    /// nothing on the screen to explain it. The M3 undo lesson, applied to
    /// a mode switch.
    #[test]
    fn switching_modes_silences_what_the_other_mode_was_playing() {
        let dir = scratch("keys-union");
        let mut app = loaded_app(&dir);
        let here = sampler_state(&app).cursor;
        let _ = app.drain_mixer_commands();

        press_shift(&mut app, 'K');
        let cleared = synced(&app);
        assert!(
            cleared.contains(&(here, 0)),
            "the pad's sound was left in the engine: {cleared:?}",
        );

        // A zone over the bottom octave, and back to pads: now the zone's
        // keys are the ones that have to go quiet.
        app.sampler_follow_note(36);
        press(&mut app, KeyCode::Char('o'));
        let _ = app.drain_mixer_commands();
        press_shift(&mut app, 'K');
        let cleared = synced(&app);
        let low = SamplerState::pad_of_note(36).unwrap();
        assert!(cleared.contains(&(low, 0)), "the zone kept playing in pads mode: {cleared:?}");
        assert!(
            cleared.contains(&(here, 1)),
            "the pad's own sound did not come back: {cleared:?}",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `w` throws one zone across the bed, seeded with the sound under the
    /// caret — and every key of it reaches the engine keytracking, rooted
    /// where the pad was, pointing at the one buffer.
    #[test]
    fn w_covers_the_bed_with_the_sound_under_the_caret() {
        let dir = scratch("keys-whole");
        let mut app = loaded_app(&dir);
        press_shift(&mut app, 'K');
        let _ = app.drain_mixer_commands();
        press(&mut app, KeyCode::Char('w'));

        assert_eq!(sampler_state(&app).zones.len(), 1);
        assert_eq!(zone(&app).keys(), phosphor_app::sampler::NUM_PADS);
        assert_eq!(zone(&app).pad.layers.len(), 1, "the zone was not seeded");
        assert_eq!(zone(&app).root(), 60, "the zone is not rooted where the pad was");

        let sent = synced(&app);
        assert_eq!(sent.len(), phosphor_app::sampler::NUM_PADS, "not every key was shipped");
        assert!(sent.iter().all(|(_, layers)| *layers == 1), "a key of the zone is silent");

        // The same audio on every key, never a copy of it.
        let state = sampler_state(&app);
        let first = state.voice(0);
        let last = state.voice(phosphor_app::sampler::NUM_PADS - 1);
        assert!(
            std::sync::Arc::ptr_eq(
                first.layers[0].pcm.as_ref().unwrap(),
                last.layers[0].pcm.as_ref().unwrap(),
            ),
            "the bed copied the sample eighty-eight times",
        );
        assert!(first.config.keytrack, "a zone that does not track the keyboard");

        // And the screen says what was made.
        let text = screen(&app, 120, 40);
        assert!(text.contains("zones"), "the list is not the zone list:\n{text}");
        assert!(text.contains("root C3"), "the list does not give the root:\n{text}");
        assert!(text.contains("\u{251C}"), "the rule is not under the band:\n{text}");
        assert!(text.contains("span"), "the span control is not on the panel:\n{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `o` covers the octave the caret is in, and a second one beside it
    /// stops where the first begins.
    #[test]
    fn o_covers_the_octave_and_zones_stop_at_each_other() {
        let dir = scratch("keys-octave");
        let mut app = loaded_app(&dir);
        press_shift(&mut app, 'K');
        press(&mut app, KeyCode::Char('o'));
        assert_eq!(zone(&app).keys(), 12);
        assert_eq!(
            SamplerState::note_of_pad(zone(&app).lo),
            60,
            "the octave does not start on its C",
        );

        // A zone in the octave above, then its low edge walked down into
        // the first one — where it stops.
        app.sampler_follow_note(72);
        press(&mut app, KeyCode::Char('o'));
        assert_eq!(sampler_state(&app).zones.len(), 2);
        press(&mut app, KeyCode::Enter); // hold the span
        for _ in 0..20 {
            press(&mut app, KeyCode::Char('h'));
        }
        let state = sampler_state(&app);
        assert_eq!(
            state.zones[1].lo,
            state.zones[0].hi + 1,
            "the zones ran into each other",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The brace: `enter` holds the span, `h`/`l` move the low edge and
    /// `H`/`L` the high one, the readout follows, and a whole run is one
    /// press of `u`.
    #[test]
    fn the_span_is_a_brace_with_two_edges() {
        let dir = scratch("keys-brace");
        let mut app = keys_app(&dir);
        app.sampler_follow_note(60);
        press(&mut app, KeyCode::Enter);
        let (held, _) = app.status_message.as_ref().unwrap();
        assert!(held.contains("low edge"), "the hold does not say what it holds: {held}");

        let before = (zone(&app).lo, zone(&app).hi);
        for _ in 0..3 {
            press(&mut app, KeyCode::Char('l'));
        }
        assert_eq!(zone(&app).lo, before.0 + 3, "`l` did not move the low edge");
        assert_eq!(zone(&app).hi, before.1, "`l` moved the high edge too");
        let (shown, _) = app.status_message.as_ref().unwrap();
        assert!(shown.contains("keys"), "the span readout does not follow: {shown}");

        press_shift(&mut app, 'H');
        assert_eq!(zone(&app).hi, before.1 - 1, "`H` did not move the high edge");

        // The keys the zone left were emptied in the engine, not just on
        // the screen.
        let sent = synced(&app);
        assert!(
            sent.iter().any(|(pad, layers)| *pad == before.0 && *layers == 0),
            "a key the brace left is still sounding: {sent:?}",
        );

        // One `u` per edge run, not one per press.
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(zone(&app).hi, before.1, "the high edge did not come back");
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(zone(&app).lo, before.0, "one `u` did not take the whole run back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An edge pushed past the caret takes the caret with it. Without that,
    /// the second press of the same key is refused by a panel that has
    /// stopped showing the zone being moved — which is what a one-octave
    /// zone does on its very first press, because the caret is standing on
    /// the edge that is about to move.
    #[test]
    fn the_caret_rides_the_edge_it_is_pushing() {
        let dir = scratch("keys-ride");
        let mut app = loaded_app(&dir);
        press_shift(&mut app, 'K');
        press(&mut app, KeyCode::Char('o')); // C3-B3, caret on C3, its low edge
        let lo = zone(&app).lo;
        press(&mut app, KeyCode::Enter);
        for _ in 0..3 {
            press(&mut app, KeyCode::Char('l'));
        }
        assert_eq!(zone(&app).lo, lo + 3, "the brace stopped following the caret");
        assert_eq!(sampler_state(&app).cursor, zone(&app).lo, "the caret was left behind");
        assert_eq!(zone(&app).keys(), 9, "the high edge moved too");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `s` splits the zone under the caret: the left keeps its root, the
    /// right is rooted at its own first key, and both point at the one
    /// buffer.
    #[test]
    fn s_splits_the_zone_under_the_caret() {
        let dir = scratch("keys-split");
        let mut app = keys_app(&dir);
        app.sampler_follow_note(72);
        press(&mut app, KeyCode::Char('s'));

        let state = sampler_state(&app);
        assert_eq!(state.zones.len(), 2);
        let at = SamplerState::pad_of_note(72).unwrap();
        assert_eq!(state.zones[0].hi, at - 1, "the halves overlap");
        assert_eq!(state.zones[1].lo, at);
        assert_eq!(state.zones[0].root(), 60, "the left half was retuned");
        assert_eq!(state.zones[1].root(), 72, "the right half is not rooted at itself");
        assert!(
            std::sync::Arc::ptr_eq(
                state.zones[0].pad.layers[0].pcm.as_ref().unwrap(),
                state.zones[1].pad.layers[0].pcm.as_ref().unwrap(),
            ),
            "the split copied the audio",
        );

        // One step back, and the bed is one zone again.
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(sampler_state(&app).zones.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `D` asks before taking a zone off the bed, the keys go quiet in the
    /// engine, and `u` brings the zone back with its audio.
    #[test]
    fn d_takes_a_zone_off_the_bed_after_asking() {
        let dir = scratch("keys-drop");
        let mut app = keys_app(&dir);
        press_shift(&mut app, 'D');
        assert!(app.nav.confirm_modal.open, "`D` did not ask");
        assert_eq!(app.nav.confirm_modal.kind, ConfirmKind::DeleteSamplerZone);
        assert!(
            app.nav.confirm_modal.message.contains("off the bed"),
            "the question is unclear: {}",
            app.nav.confirm_modal.message,
        );

        press(&mut app, KeyCode::Char('y'));
        assert!(sampler_state(&app).zones.is_empty(), "the zone stayed");
        let sent = synced(&app);
        assert!(
            sent.iter().all(|(_, layers)| *layers == 0) && !sent.is_empty(),
            "the zone's keys are still sounding: {sent:?}",
        );

        press(&mut app, KeyCode::Char('u'));
        assert_eq!(sampler_state(&app).zones.len(), 1, "undo lost the zone");
        assert!(
            zone(&app).pad.layers[0].pcm.is_some(),
            "the zone came back with no audio behind it",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Undo crosses a mode switch: the step that made the zone is under the
    /// step that switched the mode, and both come back with the engine.
    #[test]
    fn undo_walks_back_across_a_mode_switch() {
        let dir = scratch("keys-undo");
        let mut app = keys_app(&dir);
        assert_eq!(sampler_state(&app).zones.len(), 1);

        press(&mut app, KeyCode::Char('u')); // the zone
        assert!(sampler_state(&app).zones.is_empty());
        assert_eq!(sampler_state(&app).mode, phosphor_app::sampler::MapMode::Keys);

        let _ = app.drain_mixer_commands();
        press(&mut app, KeyCode::Char('u')); // the switch
        assert_eq!(sampler_state(&app).mode, phosphor_app::sampler::MapMode::Pads);
        let here = sampler_state(&app).cursor;
        let sent = synced(&app);
        assert!(
            sent.contains(&(here, 1)),
            "the pad's sound did not come back to the engine: {sent:?}",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `R` arms the keyboard: the next key played is the zone's root, and
    /// nothing else happens to that key.
    #[test]
    fn r_learns_a_root_from_the_next_key_played() {
        let dir = scratch("keys-learn");
        let mut app = keys_app(&dir);
        let cursor = sampler_state(&app).cursor;
        press_shift(&mut app, 'R');
        assert!(app.nav.clip_view.sampler.root_learn, "`R` did not arm");
        assert!(screen(&app, 120, 40).contains("root?"), "the chip does not say it is armed");

        play(&mut app, 45, 100, true, phosphor_midi::clock::now_micros());
        assert_eq!(zone(&app).root(), 45, "the key played did not become the root");
        assert!(!app.nav.clip_view.sampler.root_learn, "the arming was not spent");
        assert_eq!(
            sampler_state(&app).cursor,
            cursor,
            "the key played walked the caret off the zone it was teaching",
        );
        // The engine heard the new root on every key of the zone.
        assert!(!synced(&app).is_empty(), "the root never reached the engine");

        // `esc` disarms without changing anything.
        press_shift(&mut app, 'R');
        press(&mut app, KeyCode::Esc);
        assert!(!app.nav.clip_view.sampler.root_learn, "esc did not disarm");
        play(&mut app, 50, 100, true, phosphor_midi::clock::now_micros());
        assert_eq!(zone(&app).root(), 45, "a disarmed learn still took a root");

        // ...and so does walking away from the pad map.
        press_shift(&mut app, 'R');
        press(&mut app, KeyCode::Tab);
        assert!(!app.nav.clip_view.sampler.root_learn, "the arming outlived the tab");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file that says its own pitch teaches the zone its root, and one
    /// that does not leaves it alone.
    #[test]
    fn a_file_name_that_says_its_pitch_teaches_the_root() {
        let dir = scratch("keys-sniff");
        let named = dir.join("Piano_A#1.wav");
        write_wav(&named, 441);
        let plain = dir.join("07_kick.wav");
        write_wav(&plain, 441);

        let mut app = sampler_app();
        press_shift(&mut app, 'K');
        press(&mut app, KeyCode::Char('w')); // an empty zone across the bed
        press(&mut app, KeyCode::Char('a'));
        type_line(&mut app, &named.display().to_string());
        press(&mut app, KeyCode::Enter);
        assert_eq!(zone(&app).root(), 46, "the name did not teach the root");
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("root A#1"), "the flash did not say what it learned: {message}");

        press(&mut app, KeyCode::Char('a'));
        type_line(&mut app, &plain.display().to_string());
        press(&mut app, KeyCode::Enter);
        assert_eq!(zone(&app).root(), 46, "a name with no note in it moved the root");

        // On a pad that does not keytrack, a name is not worth reading: the
        // pad plays the same sound on every key, so a root is a number
        // nobody can hear.
        press_shift(&mut app, 'K');
        let before = sampler_state(&app).current().config.root;
        press(&mut app, KeyCode::Char('a'));
        type_line(&mut app, &named.display().to_string());
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            sampler_state(&app).current().config.root,
            before,
            "a pad that does not track the keyboard was retuned by a file name",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The panel, the layer list, the trim strip and the normalize all
    /// point at the zone's own sound in keys mode — not at the pad hiding
    /// under the caret.
    #[test]
    fn the_panel_and_the_strip_edit_the_zone_not_the_pad_under_it() {
        let dir = scratch("keys-panel");
        let mut app = keys_app(&dir);
        let here = sampler_state(&app).cursor;

        // A knob: the zone's, and every key of the zone hears it.
        press(&mut app, KeyCode::Char('j')); // off the span, onto trig
        press(&mut app, KeyCode::Char('j')); // poly
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(zone(&app).pad.config.poly, 2, "the knob did not turn the zone");
        assert_eq!(
            sampler_state(&app).pads[here].config.poly, 1,
            "the knob turned the pad hiding under the caret",
        );
        press(&mut app, KeyCode::Esc);

        // The trim strip opens on the zone's layer and trims it.
        press(&mut app, KeyCode::Char('t'));
        assert!(app.nav.clip_view.sampler.trim.is_some(), "the strip did not open on the zone");
        press(&mut app, KeyCode::Char('l'));
        assert!(zone(&app).pad.layers[0].start_frame > 0, "the trim missed the zone's sound");
        press(&mut app, KeyCode::Esc);

        // And so does normalize.
        press(&mut app, KeyCode::Char('n'));
        assert!(zone(&app).pad.layers[0].gain > 1.0, "the normalize missed the zone's sound");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A take recorded while the bed is in keys mode lands in the zone's
    /// own sound, and every key of the zone plays it.
    #[test]
    fn a_take_recorded_in_keys_mode_lands_in_the_zone() {
        let dir = scratch("keys-take");
        let mut app = keys_app(&dir);
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter); // the first instrument in the list
        press(&mut app, KeyCode::Char('r'));
        perform(&mut app, 57);
        press(&mut app, KeyCode::Char('r'));

        assert_eq!(zone(&app).pad.layers.len(), 2, "the take did not land in the zone");
        assert_eq!(zone(&app).pad.layers[1].name, "take 1");
        assert_eq!(zone(&app).root(), 57, "the performance did not teach the zone its root");
        // The pad under the caret was not touched: the take belongs to the
        // zone, and pads mode still has whatever was on it.
        let here = sampler_state(&app).cursor;
        assert_eq!(sampler_state(&app).pads[here].layers.len(), 1);

        press(&mut app, KeyCode::Esc); // out of source mode, kit replayed
        let sent = synced(&app);
        for pad in 0..phosphor_app::sampler::NUM_PADS {
            assert!(
                sent.contains(&(pad, 2)),
                "the take did not reach key {pad} of the zone: {sent:?}",
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A session saved in keys mode comes back in keys mode, with its
    /// zones, their roots and their sounds — and the pad map underneath.
    #[test]
    fn a_session_in_keys_mode_comes_back_as_one() {
        let dir = scratch("keys-session");
        let mut saving = keys_app(&dir);
        press(&mut saving, KeyCode::Char('j')); // trig
        press(&mut saving, KeyCode::Enter);
        press(&mut saving, KeyCode::Char('l')); // gate
        press(&mut saving, KeyCode::Esc);

        let session = dir.join("zoned.phos");
        saving.do_save(&session.display().to_string());
        let mut back = app();
        back.do_load(&session.display().to_string());

        let state = back
            .nav
            .tracks
            .iter()
            .find_map(|t| t.sampler.as_deref())
            .expect("the sampler did not come back");
        assert_eq!(state.mode, phosphor_app::sampler::MapMode::Keys);
        assert_eq!(state.zones.len(), 1);
        assert_eq!(state.zones[0].keys(), phosphor_app::sampler::NUM_PADS);
        assert_eq!(state.zones[0].root(), 60);
        assert_eq!(state.zones[0].pad.config.trig, phosphor_plugin::sample::TrigMode::Gate);
        assert!(state.zones[0].pad.layers[0].pcm.is_some(), "the zone's wav did not reload");
        // The pad map rode along under it.
        assert_eq!(state.pads[state.cursor].layers.len(), 1, "the pads were lost on the way");
        // And the engine was told about the zone, not about the pads.
        let sent = synced(&back);
        assert!(
            sent.iter().filter(|(_, layers)| *layers == 1).count()
                == phosphor_app::sampler::NUM_PADS,
            "the zone was not replayed into a fresh engine: {sent:?}",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Where zones overlap their layers stack, and the ninth is turned away
    /// — which the edit that caused it says out loud, because a layer that
    /// does not sound is a surprise and the moment to be surprised is the
    /// moment you made it happen.
    #[test]
    fn zones_that_stack_past_eight_say_so_at_the_edit() {
        let dir = scratch("keys-crowd");
        let mut app = keys_app(&dir); // one whole-bed zone, one layer
        {
            let track = app.nav.tracks.iter_mut().find(|t| t.sampler.is_some()).unwrap();
            let sampler = track.sampler.as_mut().unwrap();
            let sound = sampler.zones[0].pad.layers[0].clone();
            while sampler.zones[0].pad.layers.len() < phosphor_app::sampler::MAX_LAYERS {
                sampler.zones[0].pad.layers.push(sound.clone());
            }
            // A second zone over the middle octave, one layer deep.
            let mut pad = phosphor_app::sampler::PadState::empty(60);
            pad.layers.push(sound);
            let lo = SamplerState::pad_of_note(60).unwrap();
            sampler.zones.push(phosphor_app::sampler::Zone::new(lo, lo + 11, pad));
        }
        app.sampler_follow_note(64); // inside the overlap
        press(&mut app, KeyCode::Enter); // hold the span
        press(&mut app, KeyCode::Char('l'));
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(
            message.contains("do not sound"),
            "the crowded keys were not mentioned: {message}",
        );

        let state = sampler_state(&app);
        let key = SamplerState::pad_of_note(64).unwrap();
        assert_eq!(
            state.voice(key).layers.len(),
            phosphor_app::sampler::MAX_LAYERS,
            "the eight-layer bed did not hold where the zones overlap",
        );
        // Outside the overlap each zone is alone and whole.
        let alone = SamplerState::pad_of_note(40).unwrap();
        assert_eq!(state.voice(alone).layers.len(), phosphor_app::sampler::MAX_LAYERS);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A kit built entirely in keys mode — no pad ever touched — still
    /// goes into the session. The defect this catches is the save leaving
    /// the sampler out because no *pad* is occupied, which would lose every
    /// zone the player made.
    #[test]
    fn a_kit_that_lives_only_in_zones_still_saves() {
        let dir = scratch("keys-only");
        let wav = dir.join("piano.wav");
        write_wav(&wav, 441);
        let session = dir.join("zones-only.phos");

        let mut saving = sampler_app();
        press_shift(&mut saving, 'K');
        press(&mut saving, KeyCode::Char('o'));
        press(&mut saving, KeyCode::Char('a'));
        type_line(&mut saving, &wav.display().to_string());
        press(&mut saving, KeyCode::Enter);
        assert_eq!(sampler_state(&saving).occupied_pads().count(), 0, "a pad was touched");
        saving.do_save(&session.display().to_string());

        let mut back = app();
        back.do_load(&session.display().to_string());
        let state = back
            .nav
            .tracks
            .iter()
            .find_map(|t| t.sampler.as_deref())
            .expect("the sampler did not come back");
        assert_eq!(state.mode, phosphor_app::sampler::MapMode::Keys);
        assert_eq!(state.zones.len(), 1, "the zone was left out of the file");
        assert_eq!(state.zones[0].keys(), 12);
        assert!(state.zones[0].pad.layers[0].pcm.is_some(), "the zone's wav did not reload");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The zone keys say what to press instead of doing nothing in pads
    /// mode, and the layer keys say what `D` is for on a zone with nothing
    /// on it. A key that is silent is a key a player thinks is broken.
    #[test]
    fn the_zone_keys_refuse_in_words_where_they_do_not_apply() {
        let dir = scratch("keys-refuse");
        let mut app = loaded_app(&dir);
        for key in ['w', 'o', 's'] {
            press(&mut app, KeyCode::Char(key));
            let (message, _) = app.status_message.as_ref().unwrap();
            assert!(message.contains('K'), "`{key}` in pads mode said: {message}");
        }
        assert!(sampler_state(&app).zones.is_empty(), "a zone was made in pads mode");

        // In keys mode, on a key no zone covers.
        press_shift(&mut app, 'K');
        press(&mut app, KeyCode::Char('s'));
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("no zone"), "the split said: {message}");
        press_shift(&mut app, 'R');
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("no zone"), "the root learn said: {message}");
        assert!(!app.nav.clip_view.sampler.root_learn, "a bare key armed a learn");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Phrases: the performance kept as notes ──

    /// The phrases on the pad under the caret.
    fn phrases(app: &App) -> &[phosphor_app::sampler::PhraseState] {
        &sampler_state(app).current().phrases
    }

    /// Source mode with `p` already pressed, so `r` lands a phrase.
    fn phrase_app() -> App {
        let mut app = source_app();
        press(&mut app, KeyCode::Char('p'));
        app
    }

    /// A phrase landed on the pad under the caret, through the real keys.
    fn record_phrase(app: &mut App, note: u8) {
        press(app, KeyCode::Char('r'));
        perform(app, note);
        press(app, KeyCode::Char('r'));
    }

    /// `p` swaps what `r` lands, the banner says which, and the pad
    /// remembers it — so coming back to the pad opens the way you left it.
    #[test]
    fn p_swaps_what_r_lands_and_the_pad_remembers() {
        let mut app = source_app();
        let pad = sampler_state(&app).cursor;
        assert_eq!(sampler_state(&app).pads[pad].take, TakeKind::Audio, "phrases are the default");
        let text = screen(&app, 120, 40);
        assert!(text.contains("take: audio"), "the banner does not say what r lands:\n{text}");

        press(&mut app, KeyCode::Char('p'));
        assert_eq!(sampler_state(&app).pads[pad].take, TakeKind::Phrase);
        let text = screen(&app, 120, 40);
        assert!(text.contains("take: phrase"), "the banner did not follow p:\n{text}");
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("plays them back through"), "unhelpful: {message}");

        press(&mut app, KeyCode::Char('p'));
        assert_eq!(sampler_state(&app).pads[pad].take, TakeKind::Audio, "p did not swap back");

        // The memory rides with the pad: leaving and coming back finds it.
        press(&mut app, KeyCode::Char('p'));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.nav.sampler_source.as_deref().map(|m| m.take),
            Some(TakeKind::Phrase),
            "the mode opened on audio after the pad was left on phrase",
        );
    }

    /// A performance lands as a phrase: the notes are kept, nothing is
    /// rendered, and the sampler's child is pointed at the instrument it
    /// was played on — because a phrase with no child is silent.
    #[test]
    fn a_performance_lands_as_a_phrase_with_a_child_behind_it() {
        let mut app = phrase_app();
        let _ = app.drain_mixer_commands();
        record_phrase(&mut app, 60);

        assert_eq!(phrases(&app).len(), 1, "no phrase landed");
        let phrase = &phrases(&app)[0];
        assert_eq!(phrase.name, "phrase 1");
        assert!(phrase.note_count() > 0, "the phrase holds no notes");
        assert_eq!(phrase.events[0].status, 0x90);
        assert!(phrase.frames > 0);
        assert!(!phrase.transpose_with_key, "a phrase arrived transposing");
        let pad = sampler_state(&app).cursor;
        assert!(sampler_state(&app).pads[pad].layers.is_empty(), "a phrase rendered audio");
        assert_eq!(sampler_state(&app).pads[pad].config.root, 60, "the root was not learned");

        // The sampler now has a child, and the engine was told about it —
        // instrument first, then the panel it was played with.
        let child = sampler_state(&app).child.clone().expect("no child was set");
        assert_eq!(child.instrument, InstrumentType::Synth);
        assert_eq!(child.params, phosphor_app::preset::defaults(InstrumentType::Synth));
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(
                c,
                MixerCommand::SetSamplerChild { child: Some(_), .. }
            )),
            "the engine never got the child",
        );
        assert_eq!(
            commands
                .iter()
                .filter(|c| matches!(c, MixerCommand::SetSamplerChildParam { .. }))
                .count(),
            child.params.len(),
            "the child arrived with no panel behind it",
        );
        let shipped = commands.iter().find_map(|c| match c {
            MixerCommand::SetSamplerPhrases { pad, phrases, .. } if !phrases.is_empty() => {
                Some((*pad as usize, phrases.len()))
            }
            _ => None,
        });
        assert_eq!(shipped, Some((pad, 1)), "the phrase never reached the engine");

        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("phrase 1"), "the flash does not name it: {message}");
        assert!(message.contains("notes"), "the flash does not count them: {message}");
        assert!(message.contains("child is now"), "the flash hid the child: {message}");
    }

    /// One child per sampler is the design, so a phrase recorded from a
    /// second instrument replaces it — and says so, because it changes how
    /// every phrase on the kit sounds.
    #[test]
    fn a_phrase_from_another_instrument_replaces_the_child_in_words() {
        let mut app = phrase_app();
        record_phrase(&mut app, 60);
        assert_eq!(
            sampler_state(&app).child.as_ref().map(|c| c.instrument),
            Some(InstrumentType::Synth),
        );

        // Another pad, another instrument, another phrase.
        app.sampler_follow_note(64);
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Char('j')); // Drum Rack
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('p'));
        let _ = app.drain_mixer_commands();
        record_phrase(&mut app, 64);

        assert_eq!(
            sampler_state(&app).child.as_ref().map(|c| c.instrument),
            Some(InstrumentType::DrumRack),
            "the child did not follow the second phrase",
        );
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(
            message.contains("child is now") && message.contains("Drum Rack"),
            "the swap was silent: {message}",
        );
        assert!(
            app.drain_mixer_commands()
                .iter()
                .any(|c| matches!(c, MixerCommand::SetSamplerChild { child: Some(_), .. })),
            "the engine kept the old child",
        );

        // A third phrase from the same instrument with the same panel
        // changes nothing, and must not rebuild the child under a phrase
        // that is playing through it.
        app.sampler_follow_note(67);
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Char('j')); // Drum Rack again
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('p'));
        let _ = app.drain_mixer_commands();
        record_phrase(&mut app, 67);
        assert!(
            !app.drain_mixer_commands()
                .iter()
                .any(|c| matches!(c, MixerCommand::SetSamplerChild { .. })),
            "an unchanged child was rebuilt on the audio thread",
        );
    }

    /// A pad with four phrases refuses the arm before the performance, in
    /// words, naming the bed that is full.
    #[test]
    fn a_pad_full_of_phrases_refuses_the_arm_before_a_note_is_played() {
        let mut app = phrase_app();
        for _ in 0..phosphor_app::sampler::MAX_PHRASES {
            record_phrase(&mut app, 60);
        }
        assert_eq!(phrases(&app).len(), phosphor_app::sampler::MAX_PHRASES);

        press(&mut app, KeyCode::Char('r'));
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("four phrases"), "the refusal said: {message}");
        assert!(
            app.nav.sampler_source.as_deref().is_some_and(|m| !m.is_armed()),
            "a full phrase bed armed anyway",
        );
        // ...and the audio bed is still open on the same pad, because they
        // are different beds.
        press(&mut app, KeyCode::Char('p'));
        press(&mut app, KeyCode::Char('r'));
        assert!(
            app.nav.sampler_source.as_deref().is_some_and(SourceMode::is_armed),
            "a full phrase bed refused an audio take",
        );
    }

    /// The phrase sounds like what the player heard: an arpeggiator on the
    /// track is baked into the notes, exactly as it is rendered into a take.
    #[test]
    fn the_tracks_midi_devices_are_baked_into_a_phrase() {
        let mut bare = phrase_app();
        record_phrase(&mut bare, 48);
        let plain = phrases(&bare)[0].note_count();

        let mut arped = sampler_app();
        arped.add_midi_fx(arped.nav.track_cursor, crate::state::MidiFxType::Arp);
        press(&mut arped, KeyCode::Char('i'));
        press(&mut arped, KeyCode::Enter);
        press(&mut arped, KeyCode::Char('p'));
        press(&mut arped, KeyCode::Char('r'));
        let t0 = phosphor_midi::clock::now_micros();
        play(&mut arped, 48, 100, true, t0);
        play(&mut arped, 55, 100, true, t0 + 1_000);
        play(&mut arped, 48, 0, false, t0 + 600_000);
        play(&mut arped, 55, 0, false, t0 + 600_000);
        press(&mut arped, KeyCode::Char('r'));

        let run = phrases(&arped)[0].note_count();
        assert!(plain > 0 && run > 0, "one of the phrases holds no notes");
        assert!(
            run > plain,
            "the arpeggiator never reached the phrase: {run} notes against {plain}",
        );
    }

    /// One press of `u` takes a phrase back off, and the child goes with
    /// it; redo brings both back, pointing at the same events.
    #[test]
    fn undo_takes_a_phrase_and_its_child_off_together() {
        let mut app = phrase_app();
        record_phrase(&mut app, 60);
        let events = std::sync::Arc::clone(&phrases(&app)[0].events);
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Char('u'));
        assert!(phrases(&app).is_empty(), "one u did not take the phrase off");
        assert!(sampler_state(&app).child.is_none(), "the child outlived the only phrase");
        let commands = app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(
                c,
                MixerCommand::SetSamplerPhrases { phrases, .. } if phrases.is_empty()
            )),
            "undo left the phrase in the engine",
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, MixerCommand::SetSamplerChild { child: None, .. })),
            "undo left the child in the engine",
        );

        app.handle_event(Event::Key(KeyEvent {
            code: KeyCode::Char('r'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert_eq!(phrases(&app).len(), 1, "redo did not bring the phrase back");
        assert!(
            std::sync::Arc::ptr_eq(&phrases(&app)[0].events, &events),
            "the phrase came back as a copy of itself",
        );
        assert_eq!(
            sampler_state(&app).child.as_ref().map(|c| c.instrument),
            Some(InstrumentType::Synth),
            "redo left the phrase with nothing to play through",
        );
        assert!(
            app.drain_mixer_commands()
                .iter()
                .any(|c| matches!(c, MixerCommand::SetSamplerChild { child: Some(_), .. })),
            "the engine was not given the child back",
        );
    }

    /// The phr rows sit in the same list as the audio ones: `[`/`]` walk
    /// both, the panel swaps to the phrase's own three controls, and the
    /// audio-only knobs are not there to be turned.
    #[test]
    fn the_sound_list_walks_layers_and_phrases_as_one() {
        let dir = scratch("phrase-rows");
        let mut app = loaded_app(&dir); // kick.wav on C3
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('p'));
        record_phrase(&mut app, 60);
        press(&mut app, KeyCode::Esc); // back to the map

        assert_eq!(sampler_state(&app).current().rows(), 2, "the two are not one list");
        // The panel landed on what just arrived: the phrase's row.
        assert_eq!(app.nav.clip_view.sampler.layer, 1);
        let text = screen(&app, 120, 40);
        assert!(text.contains("phrase 1"), "the list does not name the phrase:\n{text}");
        assert!(text.contains("phr"), "the list does not mark the row's kind:\n{text}");
        assert!(text.contains("kick"), "the audio layer left the list:\n{text}");
        assert!(text.contains("vel"), "the phrase's own control is not drawn:\n{text}");
        assert!(!text.contains("rev"), "a phrase row offered reverse:\n{text}");

        // `[` walks back onto the audio row and the panel swaps back.
        press(&mut app, KeyCode::Char('['));
        assert_eq!(app.nav.clip_view.sampler.layer, 0);
        let text = screen(&app, 120, 40);
        assert!(text.contains("rev"), "the layer's controls did not come back:\n{text}");
        // `2` jumps straight to the phrase again.
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(app.nav.clip_view.sampler.layer, 1);

        // The knobs on a phrase row turn the phrase and nothing else.
        let before = sampler_state(&app).current().layers[0].clone();
        for _ in 0..phosphor_app::sampler::knobs::PadKnob::PAD_CONTROLS {
            press(&mut app, KeyCode::Char('j'));
        }
        press(&mut app, KeyCode::Enter); // hold `vel`
        press(&mut app, KeyCode::Char('h'));
        press(&mut app, KeyCode::Esc);
        assert!(phrases(&app)[0].gain < 1.0, "the vel knob did not turn the phrase");
        assert_eq!(
            sampler_state(&app).current().layers[0],
            before,
            "a phrase control reached the audio layer beside it",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `m` mutes a phrase and `d` removes it after asking — the same two
    /// keys the audio rows answer, on the row the cursor is on.
    #[test]
    fn m_and_d_act_on_the_phrase_row_the_cursor_is_on() {
        let mut app = phrase_app();
        record_phrase(&mut app, 60);
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Char('m'));
        assert!(phrases(&app)[0].mute, "m did not mute the phrase");
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("phrase 1"), "the flash named the wrong row: {message}");
        press(&mut app, KeyCode::Char('m'));
        assert!(!phrases(&app)[0].mute);

        press(&mut app, KeyCode::Char('d'));
        assert!(app.nav.confirm_modal.open, "d did not ask first");
        let text = screen(&app, 120, 40);
        assert!(text.contains("remove phrase 1"), "the modal names the wrong row:\n{text}");
        press(&mut app, KeyCode::Char('y'));
        assert!(phrases(&app).is_empty(), "the phrase survived the confirm");
        assert!(
            app.drain_mixer_commands().iter().any(|c| matches!(
                c,
                MixerCommand::SetSamplerPhrases { phrases, .. } if phrases.is_empty()
            )),
            "the engine still holds the removed phrase",
        );

        press(&mut app, KeyCode::Char('u'));
        assert_eq!(phrases(&app).len(), 1, "u did not bring the phrase back");
        assert!(phrases(&app)[0].note_count() > 0, "it came back with no notes");
    }

    /// The layer list's audition has nothing to play for a phrase, so the
    /// cursor landing on one says what to press instead of going silent —
    /// silence reads as an audition that has broken.
    #[test]
    fn the_cursor_landing_on_a_phrase_says_how_to_hear_it() {
        let dir = scratch("phrase-audition");
        let mut app = loaded_app(&dir);
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('p'));
        record_phrase(&mut app, 60);
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('[')); // onto the audio row: it sounds
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Char(']')); // onto the phrase row
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("plays through the child"), "unhelpful: {message}");
        assert!(message.contains("C3"), "the flash does not say which key: {message}");
        let started = app.drain_mixer_commands().into_iter().any(|c| {
            matches!(c, MixerCommand::SetSamplerPreview { preview: Some(_), .. })
        });
        assert!(!started, "a phrase row started an audition of something else");

        // And `t` on a phrase row refuses in words rather than opening a
        // waveform of nothing.
        press(&mut app, KeyCode::Char('t'));
        assert!(app.nav.clip_view.sampler.trim.is_none(), "the strip opened on a phrase");
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("notes, not a waveform"), "unhelpful: {message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The keys a phrase has no answer for say what they are for instead of
    /// doing nothing. A key that is silent is a key a player thinks is
    /// broken — and `n` on a pad that plainly has sounds on it must not say
    /// there is nothing here.
    #[test]
    fn the_phrase_keys_refuse_in_words_where_they_do_not_apply() {
        let dir = scratch("phrase-refuse");
        let mut app = loaded_app(&dir);
        // `p` outside source mode has nowhere to go, and says where it does.
        press(&mut app, KeyCode::Char('p'));
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("source-mode choice"), "`p` said: {message}");

        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('p'));
        record_phrase(&mut app, 60);
        press(&mut app, KeyCode::Esc);

        // `n` on a phrase row: there is no level to measure.
        press(&mut app, KeyCode::Char('n'));
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("no level to normalize"), "`n` said: {message}");
        assert_eq!(phrases(&app)[0].gain, 1.0, "n turned the velocity scale");

        // `p` while a take is running swaps nothing: the shape of the take
        // was decided when it was armed.
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('r'));
        let armed = app.nav.sampler_source.as_deref().map(|m| m.take);
        press(&mut app, KeyCode::Char('p'));
        assert_eq!(app.nav.sampler_source.as_deref().map(|m| m.take), armed);
        let (message, _) = app.status_message.as_ref().unwrap();
        assert!(message.contains("take is running"), "`p` while armed said: {message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The trim strip closes when the row cursor lands on a phrase. Playing
    /// a key moves the pad cursor without anything being pressed, so the
    /// strip can end up over a row that has no waveform — a mode with
    /// nothing on the screen and no key that answers.
    #[test]
    fn the_trim_strip_closes_when_the_cursor_lands_on_a_phrase() {
        let dir = scratch("phrase-strip");
        let mut app = loaded_app(&dir); // kick.wav on C3
        // A phrase-only pad up the keyboard.
        app.sampler_follow_note(64);
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('p'));
        record_phrase(&mut app, 64);
        press(&mut app, KeyCode::Esc);

        // Back on C3, open the strip on its audio layer.
        app.sampler_follow_note(60);
        press(&mut app, KeyCode::Char('t'));
        assert!(app.nav.clip_view.sampler.trim.is_some(), "the strip did not open");

        // Playing the phrase-only key takes the cursor onto a phrase row.
        app.sampler_follow_note(64);
        assert!(app.nav.clip_view.sampler.trim.is_none(), "the strip stayed open on a phrase");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A phrase survives the session with its notes, its settings and the
    /// child that plays it — and the reloaded track is told about all of it.
    #[test]
    fn a_phrase_and_its_child_survive_the_session() {
        let dir = scratch("phrase-session");
        let session = dir.join("kit.phos");
        let mut saving = phrase_app();
        record_phrase(&mut saving, 45);
        press(&mut saving, KeyCode::Esc);
        press(&mut saving, KeyCode::Char('m')); // mute it, so a setting travels
        let (notes, frames) = {
            let phrase = &phrases(&saving)[0];
            (phrase.note_count(), phrase.frames)
        };
        saving.do_save(&session.display().to_string());
        // Nothing was written beside the file: a phrase has no audio.
        assert!(!dir.join("kit.samples").exists(), "a phrase wrote a sidecar wav");
        let json = std::fs::read_to_string(&session).unwrap();
        assert!(json.contains("\"phrases\""), "the phrase is not in the file:\n{json}");
        assert!(json.contains("\"child\""), "the child is not in the file:\n{json}");

        let mut back = app();
        back.do_load(&session.display().to_string());
        let state = back
            .nav
            .tracks
            .iter()
            .find_map(|t| t.sampler.as_deref())
            .expect("the sampler did not come back");
        // The pad the mode was standing on, not the key that was played:
        // the pad cursor is frozen while source mode is on.
        let pad = SamplerState::pad_of_note(60).unwrap();
        assert_eq!(state.pads[pad].phrases.len(), 1, "the phrase did not come back");
        assert_eq!(state.pads[pad].phrases[0].note_count(), notes);
        assert_eq!(state.pads[pad].phrases[0].frames, frames);
        assert!(state.pads[pad].phrases[0].mute, "the mute did not travel");
        assert_eq!(state.pads[pad].config.root, 45, "the root the phrase taught went missing");
        assert_eq!(
            state.child.as_ref().map(|c| c.instrument),
            Some(InstrumentType::Synth),
            "the child did not come back",
        );

        // The fresh engine was given the child and the phrase, or the kit
        // would open silent on the keys it looks full on.
        let commands = back.drain_mixer_commands();
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, MixerCommand::SetSamplerChild { child: Some(_), .. })),
            "the reloaded sampler has no child",
        );
        assert!(
            commands.iter().any(|c| matches!(
                c,
                MixerCommand::SetSamplerPhrases { phrases, .. } if !phrases.is_empty()
            )),
            "the reloaded sampler was never told about the phrase",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Leaving source mode rebuilds the sampler from scratch, so the child
    /// has to be replayed with the kit. Forgetting it is a pad that looks
    /// full and makes no sound.
    #[test]
    fn leaving_source_mode_gives_the_child_back_with_the_kit() {
        let mut app = phrase_app();
        record_phrase(&mut app, 60);
        let _ = app.drain_mixer_commands();
        press(&mut app, KeyCode::Esc);

        let commands = app.drain_mixer_commands();
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, MixerCommand::SetSamplerChild { child: Some(_), .. })),
            "the fresh sampler came back with no child",
        );
        assert!(
            commands.iter().any(|c| matches!(
                c,
                MixerCommand::SetSamplerPhrases { phrases, .. } if !phrases.is_empty()
            )),
            "the fresh sampler came back with no phrases",
        );
    }

    /// A phrase recorded while the bed is in keys mode lands in the zone
    /// and reaches every key of it, transposing from the zone's root.
    #[test]
    fn a_phrase_recorded_in_keys_mode_lands_in_the_zone() {
        let dir = scratch("keys-phrase");
        let mut app = keys_app(&dir);
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('p'));
        record_phrase(&mut app, 57);
        assert_eq!(zone(&app).pad.phrases.len(), 1, "the phrase missed the zone");
        assert_eq!(zone(&app).root(), 57, "the performance did not teach the zone its root");
        assert!(
            sampler_state(&app).current().phrases.is_empty(),
            "it landed on the pad hiding under the caret",
        );

        press(&mut app, KeyCode::Esc); // out of source mode, kit replayed
        let sent = synced_phrases(&app);
        for pad in 0..phosphor_app::sampler::NUM_PADS {
            assert!(
                sent.contains(&(pad, 1)),
                "the phrase did not reach key {pad} of the zone",
            );
        }
        let state = sampler_state(&app);
        assert!(
            state.voice(0).phrases[0].transpose_with_key,
            "a zone's phrase would play one pitch across the whole span",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
