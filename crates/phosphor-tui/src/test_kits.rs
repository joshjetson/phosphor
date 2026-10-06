//! Journeys through the player's own drum kits: made on a sampler, picked on
//! a Drum Rack's kit knob like any machine, kept by a session, edited by
//! opening them back onto a sampler, and deleted with a question first.
//!
//! The engine side is checked by what the front end sends — the pads a kit
//! fills, and the empty pads that clear it — because a kit that shows on the
//! knob and never reaches the audio thread is the failure this feature can
//! have that nothing on the screen would reveal. The rack playing what it is
//! sent is the DSP crate's own tests.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use phosphor_app::kits;
    use phosphor_app::sampler::SamplerState;
    use phosphor_core::mixer::MixerCommand;
    use phosphor_core::EngineConfig;
    use phosphor_dsp::drum_rack::{kit_knob, KIT_COUNT, P_KIT};
    use phosphor_plugin::sample::SamplePcm;

    use crate::app::App;
    use crate::state::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("phosphor-kits-ui-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An app whose kits live in `dir/kits` and nowhere else.
    fn kit_app(dir: &Path) -> App {
        let mut app = App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false);
        app.preset_dir = Some(dir.join("presets"));
        app.kits_dir = Some(dir.join("kits"));
        app.reload_kit_library();
        app
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

    fn open_browser(app: &mut App) {
        press(app, KeyCode::Char(' '));
        press(app, KeyCode::Char('w'));
        assert!(app.nav.preset_modal.open, "Space+w did not open the browser");
    }

    /// Walk the browser's cursor onto `row`.
    fn browser_to(app: &mut App, row: PresetRow) {
        while app.nav.preset_modal.cursor > 0 {
            press(app, KeyCode::Char('k'));
        }
        for _ in 0..64 {
            if app.nav.preset_modal.row() == row {
                return;
            }
            press(app, KeyCode::Char('j'));
        }
        panic!("the browser has no row {row:?}");
    }

    fn pcm(frames: usize, value: f32) -> Arc<SamplePcm> {
        Arc::new(SamplePcm { data: vec![value; frames], channels: 1, sample_rate: 44_100.0 })
    }

    /// A sampler with one sound on the pad of `note`.
    fn one_pad(note: u8) -> SamplerState {
        let mut state = SamplerState::new();
        let pad = SamplerState::pad_of_note(note).unwrap();
        state.add_wav_layer(pad, PathBuf::from("hit.wav"), pcm(200, 0.3)).unwrap();
        state
    }

    /// Two kits on disk — alpha on the kick's key, beta on the snare's.
    fn two_kits(dir: &Path) {
        kits::save(&dir.join("kits"), "alpha", &one_pad(36)).unwrap();
        kits::save(&dir.join("kits"), "beta", &one_pad(38)).unwrap();
    }

    fn sampler_track(app: &mut App) -> usize {
        app.create_instrument_track(InstrumentType::Sampler);
        app.nav.track_cursor
    }

    fn drum_track(app: &mut App) -> usize {
        app.create_instrument_track(InstrumentType::DrumRack);
        let idx = app.nav.track_cursor;
        app.nav.clip_view.synth_param_cursor = P_KIT;
        idx
    }

    /// One press of the kit knob, and the pass every key is followed by.
    fn turn(app: &mut App, up: bool) {
        app.nav.adjust_synth_param(if up { 0.05 } else { -0.05 });
        app.reconcile_user_kits();
    }

    fn kit_name(app: &App, idx: usize) -> Option<String> {
        app.nav.tracks[idx].kit.as_ref().map(|k| k.name.clone())
    }

    /// Every pad the front end sent to `mixer_id`, and how many layers each
    /// carried, in the order sent.
    fn pads_sent(app: &App, mixer_id: usize) -> Vec<(u8, usize)> {
        let mut out = Vec::new();
        for cmd in app.drain_mixer_commands() {
            match cmd {
                MixerCommand::SetSamplerPad { track_id, pad, layers, .. } if track_id == mixer_id => {
                    out.push((pad, layers.len()));
                }
                MixerCommand::SetSamplerRange { track_id, pads } if track_id == mixer_id => {
                    out.extend(pads.iter().map(|(pad, _, layers, _)| (*pad, layers.len())));
                }
                _ => {}
            }
        }
        out
    }

    fn pad_of(note: u8) -> u8 {
        SamplerState::pad_of_note(note).unwrap() as u8
    }

    /// What the running application would be showing, as text.
    fn screen(app: &App) -> String {
        let backend = ratatui::backend::TestBackend::new(140, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let snapshot = app.engine.transport.snapshot();
        let status = app.live_status();
        terminal
            .draw(|frame| crate::ui::render(frame, &snapshot, &app.nav, status))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..40u16)
            .map(|y| (0..140u16).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ── Making one ──

    /// Space+w on a sampler, the kit row, a name: the pads are a kit, on
    /// disk and in the list. The same name again asks before it overwrites.
    #[test]
    fn a_sampler_saves_its_pads_as_a_drum_kit() {
        let dir = scratch("save");
        let mut app = kit_app(&dir);
        let idx = sampler_track(&mut app);
        app.nav.tracks[idx].sampler = Some(Box::new(one_pad(36)));

        open_browser(&mut app);
        assert_eq!(app.nav.preset_modal.kits.as_deref(), Some(&[][..]), "no kit section");
        browser_to(&mut app, PresetRow::SaveKit);
        press(&mut app, KeyCode::Enter);
        assert!(app.nav.input_modal.open, "the kit row did not ask for a name");
        assert_eq!(app.nav.input_modal.kind, InputModalKind::KitName);
        type_line(&mut app, "my kit");
        press(&mut app, KeyCode::Enter);

        assert_eq!(app.nav.preset_modal.kits, Some(vec!["my kit".to_string()]));
        assert_eq!(app.nav.kit_library.len(), 1);
        assert!(app.live_status().unwrap().contains("drum kit saved: my kit"));
        assert!(kits::exists(&dir.join("kits"), "my kit"));
        assert!(screen(&app).contains("my kit"), "the browser does not list the kit");

        // The same name again: a question, and `n` leaves the kit alone.
        browser_to(&mut app, PresetRow::SaveKit);
        press(&mut app, KeyCode::Enter);
        type_line(&mut app, "my kit");
        press(&mut app, KeyCode::Enter);
        assert!(app.nav.confirm_modal.open, "an existing kit was overwritten without asking");
        assert_eq!(app.nav.confirm_modal.kind, ConfirmKind::OverwriteKit);
        assert_eq!(app.nav.confirm_modal.message, "Overwrite drum kit 'my kit'?  y/n");
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(app.nav.kit_library.len(), 1, "an overwrite made a second kit");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A sampler with nothing on it is not a kit, and says so before the
    /// player types a name.
    #[test]
    fn an_empty_sampler_will_not_save_a_kit() {
        let dir = scratch("empty");
        let mut app = kit_app(&dir);
        sampler_track(&mut app);
        open_browser(&mut app);
        browser_to(&mut app, PresetRow::SaveKit);
        press(&mut app, KeyCode::Enter);
        assert!(!app.nav.input_modal.open, "an empty sampler asked for a kit name");
        assert!(app.live_status().unwrap().contains("nothing on the pads"));
        assert!(app.nav.kit_library.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Other instruments' browsers are as they were: no kit rows.
    #[test]
    fn only_a_sampler_has_a_kit_section() {
        let dir = scratch("other");
        let mut app = kit_app(&dir);
        drum_track(&mut app);
        open_browser(&mut app);
        assert!(app.nav.preset_modal.kits.is_none());
        assert_eq!(app.nav.preset_modal.item_count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Picking one ──

    /// The kit knob walks the eighteen machines and on into the player's
    /// kits by name, and back; each landing is heard — the kit's pads
    /// filled, the last one's emptied — and the knob reads the kit's name.
    #[test]
    fn the_kit_knob_walks_into_the_players_kits_and_back() {
        let dir = scratch("walk");
        two_kits(&dir);
        let mut app = kit_app(&dir);
        let idx = drum_track(&mut app);
        let mixer_id = app.nav.tracks[idx].mixer_id.unwrap();
        let last = kit_knob(KIT_COUNT - 1);
        app.nav.tracks[idx].synth_params[P_KIT] = kit_knob(KIT_COUNT - 2);
        let _ = app.drain_mixer_commands();

        turn(&mut app, true);
        assert_eq!(app.nav.tracks[idx].synth_params[P_KIT], last);
        assert_eq!(kit_name(&app, idx), None);
        assert!(pads_sent(&app, mixer_id).is_empty(), "a machine sent pads");

        turn(&mut app, true);
        assert_eq!(kit_name(&app, idx).as_deref(), Some("alpha"));
        assert_eq!(app.nav.tracks[idx].synth_params[P_KIT], last, "a user kit moved the knob");
        assert_eq!(pads_sent(&app, mixer_id), [(pad_of(36), 1)]);
        let shown = screen(&app);
        assert!(shown.contains("alpha"), "the knob does not read the kit's name:\n{shown}");
        assert!(shown.contains("plays its own samples"), "nothing says the kit is samples");

        turn(&mut app, true);
        assert_eq!(kit_name(&app, idx).as_deref(), Some("beta"));
        let mut sent = pads_sent(&app, mixer_id);
        sent.sort_unstable();
        assert_eq!(sent, [(pad_of(36), 0), (pad_of(38), 1)], "alpha's kick was left on");

        turn(&mut app, true);
        assert_eq!(kit_name(&app, idx).as_deref(), Some("beta"), "the knob ran off the end");

        turn(&mut app, false);
        turn(&mut app, false);
        assert_eq!(kit_name(&app, idx), None);
        assert_eq!(app.nav.tracks[idx].synth_params[P_KIT], last);
        let sent = pads_sent(&app, mixer_id);
        assert_eq!(sent.last(), Some(&(pad_of(36), 0)), "stepping back left the kit playing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Choosing a kit is one undo step, coalesced with the panel's sweep,
    /// and undo and redo both reach the engine.
    #[test]
    fn choosing_a_kit_undoes_and_redoes() {
        let dir = scratch("undo");
        two_kits(&dir);
        let mut app = kit_app(&dir);
        let idx = drum_track(&mut app);
        let mixer_id = app.nav.tracks[idx].mixer_id.unwrap();
        app.nav.tracks[idx].synth_params[P_KIT] = kit_knob(KIT_COUNT - 1);
        app.nav.undo_stack.clear();
        turn(&mut app, true);
        turn(&mut app, true);
        assert_eq!(kit_name(&app, idx).as_deref(), Some("beta"));
        let _ = app.drain_mixer_commands();

        press(&mut app, KeyCode::Char('u'));
        assert_eq!(kit_name(&app, idx), None, "undo did not put the machine back");
        assert_eq!(pads_sent(&app, mixer_id), [(pad_of(38), 0)]);
        assert!(!app.nav.undo_stack.can_undo(), "the sweep was more than one step");

        app.perform_redo();
        app.reconcile_user_kits();
        assert_eq!(kit_name(&app, idx).as_deref(), Some("beta"));
        assert_eq!(pads_sent(&app, mixer_id), [(pad_of(38), 1)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A Drum Rack preset names one of the machines: loading it over a kit
    /// of the player's own plays that machine, and `u` brings the kit back.
    #[test]
    fn loading_a_preset_plays_its_machine_and_undo_brings_the_kit_back() {
        let dir = scratch("preset");
        two_kits(&dir);
        let mut app = kit_app(&dir);
        let idx = drum_track(&mut app);
        let mixer_id = app.nav.tracks[idx].mixer_id.unwrap();
        open_browser(&mut app);
        browser_to(&mut app, PresetRow::Save);
        press(&mut app, KeyCode::Enter);
        type_line(&mut app, "machine");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Esc);

        app.nav.tracks[idx].synth_params[P_KIT] = kit_knob(KIT_COUNT - 1);
        turn(&mut app, true);
        assert_eq!(kit_name(&app, idx).as_deref(), Some("alpha"));
        let _ = app.drain_mixer_commands();

        open_browser(&mut app);
        browser_to(&mut app, PresetRow::Preset(0));
        press(&mut app, KeyCode::Enter);
        assert_eq!(kit_name(&app, idx), None, "the kit stayed on over the preset's machine");
        assert_eq!(pads_sent(&app, mixer_id), [(pad_of(36), 0)], "the kit's kick was left in the engine");

        app.perform_undo();
        app.reconcile_user_kits();
        assert_eq!(kit_name(&app, idx).as_deref(), Some("alpha"), "undo did not bring the kit back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A sequencer's drum child keeps the eighteen machines: its slot is
    /// the sequencer's, and a kit on it would have nowhere to be heard.
    #[test]
    fn a_sequencers_drum_child_does_not_step_into_kits() {
        let dir = scratch("seq");
        two_kits(&dir);
        let mut app = kit_app(&dir);
        app.create_instrument_track(InstrumentType::Sequencer);
        let idx = app.nav.track_cursor;
        assert_eq!(app.nav.tracks[idx].instrument_type, Some(InstrumentType::DrumRack));
        app.nav.clip_view.synth_param_cursor = P_KIT;
        app.nav.tracks[idx].synth_params[P_KIT] = kit_knob(KIT_COUNT - 1);
        turn(&mut app, true);
        assert_eq!(kit_name(&app, idx), None);
        assert_eq!(app.nav.tracks[idx].synth_params[P_KIT], kit_knob(KIT_COUNT - 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Keeping one ──

    /// A session keeps its own copy of the kit: saved, the kit deleted from
    /// the library, and opened again, the Drum Rack still plays it — and the
    /// engine is handed it on the way in.
    #[test]
    fn a_session_keeps_its_own_copy_of_the_kit() {
        let dir = scratch("session");
        two_kits(&dir);
        let mut app = kit_app(&dir);
        let idx = drum_track(&mut app);
        app.nav.tracks[idx].synth_params[P_KIT] = kit_knob(KIT_COUNT - 1);
        turn(&mut app, true);
        assert_eq!(kit_name(&app, idx).as_deref(), Some("alpha"));
        let id_before = app.nav.tracks[idx].kit.as_ref().unwrap().id;
        let session = dir.join("song.phos");
        assert!(app.do_save(&session.display().to_string()));
        // The save copies the kit's audio beside the song, and the kit stays
        // the same kit as far as the engine is concerned.
        assert!(dir.join("song.samples").join("C2-1.wav").exists());
        assert_eq!(app.nav.tracks[idx].kit.as_ref().unwrap().id, id_before);
        let json = std::fs::read_to_string(&session).unwrap();
        assert!(json.contains("\"kit\""), "the session did not keep the kit");

        assert!(kits::delete(&dir.join("kits"), "alpha").unwrap());

        let mut opened = kit_app(&dir);
        opened.do_load(&session.display().to_string());
        let idx = opened
            .nav
            .tracks
            .iter()
            .position(|t| t.instrument_type == Some(InstrumentType::DrumRack))
            .unwrap();
        let mixer_id = opened.nav.tracks[idx].mixer_id.unwrap();
        let kit = opened.nav.tracks[idx].kit.clone().expect("the kit did not come back");
        assert_eq!(kit.name, "alpha");
        let pad = SamplerState::pad_of_note(36).unwrap();
        assert!(kit.state.pads[pad].layers[0].pcm.is_some(), "the kit's sound did not reload");
        assert_eq!(pads_sent(&opened, mixer_id), [(pad_of(36), 1)], "the engine was not told");
        // And the knob reads its name, though the library no longer has it.
        opened.nav.track_cursor = idx;
        opened.nav.clip_view.synth_param_cursor = P_KIT;
        assert!(screen(&opened).contains("alpha"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A session from before kits existed — a Drum Rack and no kit block —
    /// opens exactly as it did: the machine on the knob, no kit.
    #[test]
    fn a_session_without_a_kit_opens_as_it_always_did() {
        let dir = scratch("old");
        let mut app = kit_app(&dir);
        let idx = drum_track(&mut app);
        app.nav.tracks[idx].synth_params[P_KIT] = kit_knob(3);
        let session = dir.join("old.phos");
        assert!(app.do_save(&session.display().to_string()));
        let json = std::fs::read_to_string(&session).unwrap();
        assert!(!json.contains("\"kit\""), "a track with no kit wrote one:\n{json}");

        let mut opened = kit_app(&dir);
        opened.do_load(&session.display().to_string());
        let track = opened
            .nav
            .tracks
            .iter()
            .find(|t| t.instrument_type == Some(InstrumentType::DrumRack))
            .unwrap();
        assert!(track.kit.is_none());
        assert_eq!(track.synth_params[P_KIT], kit_knob(3));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Editing one ──

    /// Enter on a kit opens it on the sampler's pads, as one undo step.
    /// Saved back under its own name, every Drum Rack playing it hears the
    /// new version.
    #[test]
    fn a_kit_opens_on_the_pads_and_saving_it_back_updates_the_drum_racks() {
        let dir = scratch("edit");
        two_kits(&dir);
        let mut app = kit_app(&dir);
        let drums = drum_track(&mut app);
        let drums_id = app.nav.tracks[drums].mixer_id.unwrap();
        app.nav.tracks[drums].synth_params[P_KIT] = kit_knob(KIT_COUNT - 1);
        turn(&mut app, true);
        let old_id = app.nav.tracks[drums].kit.as_ref().unwrap().id;

        let sampler = sampler_track(&mut app);
        app.nav.undo_stack.clear();
        open_browser(&mut app);
        browser_to(&mut app, PresetRow::Kit(0));
        assert_eq!(app.nav.preset_modal.selected_kit(), Some("alpha"));
        press(&mut app, KeyCode::Enter);
        assert!(!app.nav.preset_modal.open);
        let kick = SamplerState::pad_of_note(36).unwrap();
        let state = app.nav.tracks[sampler].sampler.as_deref().unwrap();
        assert_eq!(state.pads[kick].layers.len(), 1, "the kit did not land on the pads");

        // One `u` takes it back off.
        press(&mut app, KeyCode::Char('u'));
        let state = app.nav.tracks[sampler].sampler.as_deref().unwrap();
        assert!(state.pads[kick].layers.is_empty(), "undo left the kit on the pads");
        app.perform_redo();

        // An edit — a second sound — and a save over the kit.
        app.nav.tracks[sampler]
            .sampler
            .as_mut()
            .unwrap()
            .add_wav_layer(SamplerState::pad_of_note(42).unwrap(), "hat.wav".into(), pcm(50, 0.2))
            .unwrap();
        let _ = app.drain_mixer_commands();
        open_browser(&mut app);
        browser_to(&mut app, PresetRow::SaveKit);
        press(&mut app, KeyCode::Enter);
        type_line(&mut app, "alpha");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('y'));

        let now = app.nav.tracks[drums].kit.clone().unwrap();
        assert_ne!(now.id, old_id, "the drum rack kept the old version");
        assert_eq!(now.state.sounding_pads().len(), 2);
        let mut sent = pads_sent(&app, drums_id);
        sent.sort_unstable();
        assert_eq!(sent, [(pad_of(36), 1), (pad_of(42), 1)], "the engine did not hear the edit");
        assert!(app.live_status().unwrap().contains("has the new version"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Deleting one ──

    /// `d` on a kit asks first; `n` keeps it, `y` deletes it — and a Drum
    /// Rack already playing it carries on.
    #[test]
    fn deleting_a_kit_asks_first() {
        let dir = scratch("delete");
        two_kits(&dir);
        let mut app = kit_app(&dir);
        let drums = drum_track(&mut app);
        app.nav.tracks[drums].synth_params[P_KIT] = kit_knob(KIT_COUNT - 1);
        turn(&mut app, true);

        sampler_track(&mut app);
        open_browser(&mut app);
        browser_to(&mut app, PresetRow::Kit(0));
        press(&mut app, KeyCode::Char('d'));
        assert!(app.nav.confirm_modal.open);
        assert_eq!(app.nav.confirm_modal.kind, ConfirmKind::DeleteKit);
        assert_eq!(app.nav.confirm_modal.message, "Delete drum kit 'alpha'?  y/n");
        press(&mut app, KeyCode::Char('n'));
        assert!(kits::exists(&dir.join("kits"), "alpha"), "n deleted the kit");

        press(&mut app, KeyCode::Char('d'));
        press(&mut app, KeyCode::Char('y'));
        assert!(!kits::exists(&dir.join("kits"), "alpha"));
        assert_eq!(app.nav.preset_modal.kits, Some(vec!["beta".to_string()]));
        assert_eq!(app.nav.kit_library.len(), 1);
        assert!(app.live_status().unwrap().contains("drum kit deleted: alpha"));
        assert_eq!(kit_name(&app, drums).as_deref(), Some("alpha"), "a playing rack lost its kit");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
