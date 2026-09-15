//! Journeys through the sampler: choosing it, putting a sound on a pad,
//! and getting the kit back from a session file.
//!
//! These drive the real key handler and the real loader against real WAV
//! files on disk, because the sampler's whole M2 promise is one sentence:
//! type a path, hear the pad. Every failure mode a player can type is
//! walked — the wrong path, the full pad, the file that moved between
//! save and load.

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::state::*;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use phosphor_app::sampler::SamplerState;
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

    /// Create a sampler track and land the keys on its instrument panel,
    /// the way a player does: Space+a is already behind us (the track
    /// exists), and the panel tab is where `a` means "add a sound".
    fn sampler_app() -> App {
        let mut app = app();
        app.create_instrument_track(InstrumentType::Sampler);
        // Creating the track lands on its panel strip; the keys go to the
        // clip view, which is where the panel lives.
        app.nav.focused_pane = Pane::ClipView;
        app
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
