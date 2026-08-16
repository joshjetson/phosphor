//! Integration tests for user presets.
//!
//! These drive the browser the way a player does — through `handle_event` —
//! because the thing being tested is a chain of modals, not a data structure:
//! Space+W, Enter, a typed name, a confirmation. The bank format itself is
//! covered by unit tests in `phosphor_app::preset`.
//!
//! The load test compares rendered audio rather than parameter values. A
//! parameter block that matches is evidence; the same samples coming out of
//! the instrument is the claim.

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use crate::app::App;
    use crate::state::*;
    use phosphor_app::preset;
    use phosphor_core::EngineConfig;
    use phosphor_plugin::{MidiEvent, Plugin};

    /// A scratch preset directory of this test's own, so nothing here can
    /// read or write the player's `~/.phosphor/presets`.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("phosphor-preset-ui-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn app(tag: &str) -> App {
        let mut app = App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false);
        app.preset_dir = Some(scratch(tag));
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

    fn type_name(app: &mut App, name: &str) {
        for ch in name.chars() {
            press(app, KeyCode::Char(ch));
        }
    }

    fn add_track(app: &mut App, instrument: InstrumentType) {
        app.create_instrument_track(instrument);
    }

    /// Space+W, the way the binding is actually reached.
    fn open_browser(app: &mut App) {
        press(app, KeyCode::Char(' '));
        press(app, KeyCode::Char('w'));
    }

    fn params(app: &App) -> Vec<f32> {
        app.nav.tracks[app.nav.track_cursor].synth_params.clone()
    }

    /// Play one note into a fresh instance of the instrument and keep the
    /// left channel. Mirrors what the mixer does: `init`, then one
    /// `set_parameter` per control, then `process` per block.
    fn render(instrument: InstrumentType, panel: &[f32]) -> Vec<f32> {
        let mut plugin: Box<dyn Plugin> = match instrument {
            InstrumentType::Juno60 => Box::new(phosphor_dsp::juno::Juno60Synth::new()),
            InstrumentType::DrumRack => Box::new(phosphor_dsp::drum_rack::DrumRack::new()),
            other => panic!("no render harness for {other:?}"),
        };
        const BLOCK: usize = 256;
        // ~0.6 s: long enough to get past a slow attack, so the comparison
        // covers the body of the note rather than the first few milliseconds
        // of it, where two different panels can still look alike.
        const BLOCKS: usize = 96;
        // The kick pad on the drum rack; middle C on a keyboard instrument,
        // where a bottom-octave note through a filter dialled down is nearly
        // nothing and would make this a comparison of two silences.
        let note = match instrument {
            InstrumentType::DrumRack => 36u8,
            _ => 60,
        };

        plugin.init(44_100.0, BLOCK);
        for (i, &value) in panel.iter().enumerate() {
            plugin.set_parameter(i, value);
        }

        let mut left = vec![0.0f32; BLOCK];
        let mut right = vec![0.0f32; BLOCK];
        let mut out = Vec::with_capacity(BLOCK * BLOCKS);
        for block in 0..BLOCKS {
            left.fill(0.0);
            right.fill(0.0);
            let events: &[MidiEvent] = if block == 0 {
                &[MidiEvent { sample_offset: 0, status: 0x90, data1: note, data2: 100 }]
            } else {
                &[]
            };
            let mut slices: [&mut [f32]; 2] = [&mut left, &mut right];
            plugin.process(&[], &mut slices, events);
            out.extend_from_slice(&left);
        }
        out
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// Assert two panels are the same panel: every level bit-identical, every
    /// selector on the same position.
    ///
    /// Not plain equality, because a preset stores its selectors by position
    /// and a load puts them back at the exact centre of the step they name,
    /// which is not always the fraction that was dialled in. The Juno's patch
    /// knob loads at 0.0 and position 0's centre is 0.0089; the drum rack's
    /// kit knob loads at 0.0 and its centre is 0.0333. Same patch, same kit —
    /// and the render comparison beside every use of this is what proves it.
    fn assert_same_panel(
        instrument: InstrumentType,
        got: &[f32],
        want: &[f32],
        what: &str,
    ) {
        use phosphor_app::discrete;

        assert_eq!(got.len(), want.len(), "{what}: {} controls against {}", got.len(), want.len());
        for (index, (a, b)) in got.iter().zip(want.iter()).enumerate() {
            if discrete::is_discrete(instrument, index) {
                assert_eq!(
                    discrete::index_of(instrument, index, *a),
                    discrete::index_of(instrument, index, *b),
                    "{what}: selector {index} came back on a different position \
                     ({a} against {b})"
                );
            } else {
                assert_eq!(a, b, "{what}: control {index} came back changed");
            }
        }
    }

    // ── Opening ──

    /// Space+W on an instrument track opens its bank, empty the first time.
    #[test]
    fn space_w_opens_the_browser_for_the_selected_instrument() {
        let mut app = app("open");
        add_track(&mut app, InstrumentType::Juno60);
        app.nav.focus_pane(Pane::Tracks);

        open_browser(&mut app);
        assert!(app.nav.preset_modal.open, "Space+W did not open the browser");
        assert_eq!(app.nav.preset_modal.instrument, Some(InstrumentType::Juno60));
        assert!(app.nav.preset_modal.entries.is_empty());
        // Row 0 is the save row, so a fresh bank still has something to do.
        assert_eq!(app.nav.preset_modal.item_count(), 1);

        press(&mut app, KeyCode::Esc);
        assert!(!app.nav.preset_modal.open);

        let _ = std::fs::remove_dir_all(app.preset_dir.as_ref().unwrap());
    }

    /// A send or master track has no panel to save, so the browser says so
    /// rather than opening on nothing.
    #[test]
    fn the_browser_does_not_open_on_a_bus_track() {
        let mut app = app("bus");
        app.nav.focus_pane(Pane::Tracks);
        app.nav.track_cursor = 0; // send A
        assert!(app.nav.tracks[0].instrument_type.is_none());

        open_browser(&mut app);
        assert!(!app.nav.preset_modal.open);
        assert!(app.status_message.as_ref().unwrap().0.contains("instrument track"));
    }

    // ── The point of the feature ──

    /// A dialled-in sound, saved, walked away from, and brought back — and
    /// what comes back out of the instrument is the same audio, sample for
    /// sample, not merely a parameter block that looks similar.
    #[test]
    fn a_preset_restores_the_sound_it_was_saved_from() {
        let mut app = app("restore");
        add_track(&mut app, InstrumentType::Juno60);
        app.nav.focus_pane(Pane::Tracks);

        // Dial in a panel: brighter and more resonant than the strings patch
        // it starts on, with the attack shortened so the note is up and
        // sounding inside the render window, and the chorus switch at the far
        // end of the block — a preset that restored all but the last control
        // would otherwise pass this.
        for (param, presses) in [
            (phosphor_dsp::juno::P_CUTOFF, 3i32),
            (phosphor_dsp::juno::P_RESO, 5),
            (phosphor_dsp::juno::P_ATTACK, -3),
            (phosphor_dsp::juno::P_SUSTAIN, 3),
            (phosphor_dsp::juno::PARAM_COUNT - 1, 1),
        ] {
            app.nav.clip_view.synth_param_cursor = param;
            for _ in 0..presses.abs() {
                app.nav.adjust_synth_param(if presses > 0 { 0.05 } else { -0.05 });
            }
        }
        let dialled = params(&app);
        let sound = render(InstrumentType::Juno60, &dialled);
        // Guard against the whole comparison being silence against silence,
        // which would pass no matter what the preset did.
        assert!(peak(&sound) > 0.01, "the reference render is near silent");

        // Save it.
        open_browser(&mut app);
        press(&mut app, KeyCode::Enter); // the save row
        assert!(app.nav.input_modal.open, "Enter on the save row did not ask for a name");
        type_name(&mut app, "evening pad");
        press(&mut app, KeyCode::Enter);
        assert!(!app.nav.input_modal.open);
        assert_eq!(app.nav.preset_modal.entries, vec!["evening pad".to_string()]);

        // Walk away: stepping the patch selector loads a whole factory panel
        // over the top of it.
        app.nav.clip_view.synth_param_cursor = phosphor_dsp::juno::P_PATCH;
        for _ in 0..7 {
            app.nav.adjust_synth_param(0.05);
        }
        let walked = params(&app);
        assert_ne!(walked, dialled, "the patch knob did not change the panel");
        let other_sound = render(InstrumentType::Juno60, &walked);
        assert_ne!(other_sound, sound, "two different panels rendered the same audio");

        // Bring it back: the browser is still open with the cursor on the
        // preset that was just written.
        assert_eq!(app.nav.preset_modal.cursor, 1);
        let mixer_id = app.nav.tracks[app.nav.track_cursor].mixer_id.unwrap();
        let _ = app.drain_mixer_commands();
        press(&mut app, KeyCode::Enter);
        assert_same_panel(
            InstrumentType::Juno60,
            &params(&app),
            &dialled,
            "the panel did not come back",
        );

        let restored = render(InstrumentType::Juno60, &params(&app));
        assert_eq!(
            restored, sound,
            "the restored panel does not sound like the one that was saved"
        );

        // The UI's copy being right is not the same as the instrument
        // hearing about it. Every control has to reach the audio thread, or
        // the panel reads correctly and the speakers still play the old
        // sound until something else nudges a knob.
        let sent: Vec<(usize, f32)> = app
            .drain_mixer_commands()
            .into_iter()
            .filter_map(|cmd| match cmd {
                phosphor_core::mixer::MixerCommand::SetParameter {
                    track_id,
                    param_index,
                    value,
                } if track_id == mixer_id => Some((param_index, value)),
                _ => None,
            })
            .collect();
        // Against the panel that was loaded rather than the one that was
        // dialled in: those are the same panel, and `assert_same_panel` above
        // is what says so. What is being claimed here is narrower and is about
        // the command channel — every control reached the audio thread, in
        // order, with the value the UI is showing.
        let expected: Vec<(usize, f32)> =
            params(&app).iter().copied().enumerate().collect();
        assert_eq!(
            sent, expected,
            "the restored panel did not reach the audio thread control for control"
        );

        let _ = std::fs::remove_dir_all(app.preset_dir.as_ref().unwrap());
    }

    /// The drum rack is the reason this exists: 35 controls across eight
    /// voices, none of them reachable from the kit knob.
    #[test]
    fn the_drum_racks_whole_panel_survives_a_round_trip() {
        let mut app = app("drums");
        add_track(&mut app, InstrumentType::DrumRack);
        app.nav.focus_pane(Pane::Tracks);
        assert_eq!(params(&app).len(), phosphor_dsp::drum_rack::PARAM_COUNT);

        // Move every continuous control off its default, so a preset that
        // restored only the first few would fail.
        for idx in 0..phosphor_dsp::drum_rack::PARAM_COUNT {
            if phosphor_dsp::drum_rack::is_discrete(idx) {
                continue;
            }
            app.nav.clip_view.synth_param_cursor = idx;
            app.nav.adjust_synth_param(if idx % 2 == 0 { 0.05 } else { -0.05 });
        }
        let dialled = params(&app);
        let sound = render(InstrumentType::DrumRack, &dialled);
        // Guard against the whole comparison being silence against silence.
        assert!(peak(&sound) > 0.01, "the kick render is near silent");

        open_browser(&mut app);
        press(&mut app, KeyCode::Enter);
        type_name(&mut app, "my kit");
        press(&mut app, KeyCode::Enter);

        // Reset to the factory panel, then load the preset back.
        if let Some(track) = app.nav.tracks.get_mut(app.nav.track_cursor) {
            track.synth_params = phosphor_dsp::drum_rack::PARAM_DEFAULTS.to_vec();
        }
        assert_ne!(params(&app), dialled);
        press(&mut app, KeyCode::Enter);

        assert_same_panel(
            InstrumentType::DrumRack,
            &params(&app),
            &dialled,
            "35 controls did not all come back",
        );
        assert_eq!(
            render(InstrumentType::DrumRack, &params(&app)),
            sound,
            "the restored kit does not sound like the one that was saved"
        );

        let _ = std::fs::remove_dir_all(app.preset_dir.as_ref().unwrap());
    }

    // ── Selectors ──

    /// The defect the preset format's version 2 exists for: a kit chosen when
    /// the rack had ten of them opened on a different drum machine once it had
    /// fifteen, because a selector was stored as a fraction of the bank's size.
    /// The preset now carries the position as well, and the position wins.
    #[test]
    fn a_kit_survives_the_rack_growing() {
        use phosphor_dsp::drum_rack;

        let mut app = app("kit-grew");
        add_track(&mut app, InstrumentType::DrumRack);
        app.nav.focus_pane(Pane::Tracks);

        // Put the rack on the 909 and save it.
        if let Some(track) = app.nav.tracks.get_mut(app.nav.track_cursor) {
            track.synth_params[drum_rack::P_KIT] = drum_rack::kit_knob(1);
        }
        assert_eq!(
            drum_rack::discrete_label(drum_rack::P_KIT, params(&app)[drum_rack::P_KIT]),
            Some("909")
        );
        open_browser(&mut app);
        press(&mut app, KeyCode::Enter);
        type_name(&mut app, "my 909");
        press(&mut app, KeyCode::Enter);

        // Rewrite the stored fraction as a ten-kit build would have written
        // it, leaving the stored position alone: this is the file on disk.
        let dir = app.preset_dir.clone().unwrap();
        let mut bank = preset::load_bank(&dir, InstrumentType::DrumRack).unwrap();
        bank.presets[0].params[drum_rack::P_KIT] = 1.5 / 10.0;
        preset::save_bank(&dir, InstrumentType::DrumRack, &bank).unwrap();

        // Walk the rack somewhere else, then load the preset back.
        if let Some(track) = app.nav.tracks.get_mut(app.nav.track_cursor) {
            track.synth_params[drum_rack::P_KIT] = drum_rack::kit_knob(6);
        }
        press(&mut app, KeyCode::Esc);
        open_browser(&mut app);
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);

        assert_eq!(
            drum_rack::discrete_label(drum_rack::P_KIT, params(&app)[drum_rack::P_KIT]),
            Some("909"),
            "the preset opened on a different drum machine"
        );
        let msg = &app.status_message.as_ref().unwrap().0;
        assert!(msg.contains("preset loaded"), "the load did not report success: {msg}");
        assert!(!msg.contains("older format"), "a current preset was called an old one: {msg}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A version 1 preset still loads — the fraction is the only evidence of
    /// what the player chose, and it is right whenever the bank has not moved
    /// — but the bottom bar says to check the patch, the same as a version 1
    /// session does.
    #[test]
    fn an_older_preset_loads_and_the_bottom_bar_says_so() {
        use phosphor_dsp::drum_rack;

        let mut app = app("old-format");
        add_track(&mut app, InstrumentType::DrumRack);
        app.nav.focus_pane(Pane::Tracks);

        let dir = app.preset_dir.clone().unwrap();
        let mut bank = preset::PresetFile::new(InstrumentType::DrumRack);
        bank.store("old", InstrumentType::DrumRack, &params(&app)).unwrap();
        // As version 1 wrote it: no positions, and the version that says so.
        bank.version = 1;
        bank.presets[0].version = 1;
        bank.presets[0].discrete.clear();
        bank.presets[0].params[drum_rack::P_KIT] = 1.5 / 10.0;
        preset::save_bank(&dir, InstrumentType::DrumRack, &bank).unwrap();

        open_browser(&mut app);
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);

        // It loaded, from the only evidence it has...
        assert_eq!(params(&app)[drum_rack::P_KIT], 1.5 / 10.0);
        // ...and it said so.
        let msg = &app.status_message.as_ref().unwrap().0;
        assert!(msg.contains("preset loaded"), "the preset did not load: {msg}");
        assert!(msg.contains("older format"), "an old preset loaded quietly: {msg}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Refusals ──

    /// A preset written when the Juno had 16 controls, opened after it grew
    /// to 25. Loading it would put 16 values into the first 16 of 25 holes:
    /// a plausible sound that is not the one that was saved. It is refused,
    /// the panel is left alone, and the reason is on screen.
    #[test]
    fn a_preset_with_the_wrong_control_count_is_refused() {
        let mut app = app("count");
        add_track(&mut app, InstrumentType::Juno60);
        app.nav.focus_pane(Pane::Tracks);

        let dir = app.preset_dir.clone().unwrap();
        let mut bank = preset::PresetFile::new(InstrumentType::Juno60);
        bank.store("old panel", InstrumentType::Juno60, &params(&app)).unwrap();
        bank.presets[0].params.truncate(16);
        bank.presets[0].param_count = 16;
        preset::save_bank(&dir, InstrumentType::Juno60, &bank).unwrap();

        let before = params(&app);
        open_browser(&mut app);
        assert_eq!(app.nav.preset_modal.entries, vec!["old panel".to_string()]);
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);

        assert_eq!(params(&app), before, "a 16-control preset was loaded into a 25-control panel");
        let msg = &app.status_message.as_ref().unwrap().0;
        assert!(msg.contains("not loaded"), "no refusal on screen, got: {msg}");
        assert!(msg.contains("16"), "the refusal does not say what was wrong: {msg}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A preset saved against an older ordering of the same number of
    /// controls is refused too — the count alone cannot see a reorder.
    #[test]
    fn a_preset_from_a_reordered_panel_is_refused() {
        let mut app = app("layout");
        add_track(&mut app, InstrumentType::Juno60);
        app.nav.focus_pane(Pane::Tracks);

        let dir = app.preset_dir.clone().unwrap();
        let mut bank = preset::PresetFile::new(InstrumentType::Juno60);
        bank.store("shuffled", InstrumentType::Juno60, &params(&app)).unwrap();
        // Right instrument, right count, panel from before the reorder.
        bank.presets[0].layout = "0123456789abcdef".into();
        preset::save_bank(&dir, InstrumentType::Juno60, &bank).unwrap();

        let before = params(&app);
        open_browser(&mut app);
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);

        assert_eq!(params(&app), before);
        let msg = &app.status_message.as_ref().unwrap().0;
        assert!(msg.contains("panel layout"), "unhelpful refusal: {msg}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One bank file per instrument, so a Juno preset is never even offered
    /// on a DX7 — and a hand-copied one is refused if it is.
    #[test]
    fn a_preset_saved_for_another_instrument_is_not_offered() {
        let mut app = app("instrument");
        add_track(&mut app, InstrumentType::Juno60);
        app.nav.focus_pane(Pane::Tracks);

        open_browser(&mut app);
        press(&mut app, KeyCode::Enter);
        type_name(&mut app, "juno only");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.nav.preset_modal.entries, vec!["juno only".to_string()]);
        press(&mut app, KeyCode::Esc);

        // A DX7 on the next track sees its own, empty bank.
        add_track(&mut app, InstrumentType::DX7);
        app.nav.focus_pane(Pane::Tracks);
        open_browser(&mut app);
        assert_eq!(app.nav.preset_modal.instrument, Some(InstrumentType::DX7));
        assert!(
            app.nav.preset_modal.entries.is_empty(),
            "a Juno preset showed up in the DX7's browser"
        );

        // And if the entry is pasted into the DX7's file by hand, loading it
        // still refuses rather than reading a Juno panel as FM parameters.
        let dir = app.preset_dir.clone().unwrap();
        let juno = preset::load_bank(&dir, InstrumentType::Juno60).unwrap();
        let mut dx7 = preset::load_bank(&dir, InstrumentType::DX7).unwrap();
        dx7.presets.push(juno.presets[0].clone());
        preset::save_bank(&dir, InstrumentType::DX7, &dx7).unwrap();

        press(&mut app, KeyCode::Esc);
        open_browser(&mut app);
        let before = params(&app);
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);
        assert_eq!(params(&app), before, "a Juno preset loaded into a DX7");
        assert!(app.status_message.as_ref().unwrap().0.contains("not loaded"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Name collisions ──

    /// A name the bank already holds rewrites that slot rather than adding a
    /// second row with the same label — but it asks first, because the slot
    /// it rewrites is a sound the player kept.
    #[test]
    fn saving_over_a_name_asks_before_replacing_it() {
        let mut app = app("collide");
        add_track(&mut app, InstrumentType::Juno60);
        app.nav.focus_pane(Pane::Tracks);

        open_browser(&mut app);
        press(&mut app, KeyCode::Enter);
        type_name(&mut app, "brass");
        press(&mut app, KeyCode::Enter);
        let first = params(&app);
        assert_eq!(app.nav.preset_modal.entries, vec!["brass".to_string()]);

        // Change the panel and save under the same name.
        app.nav.clip_view.synth_param_cursor = phosphor_dsp::juno::P_CUTOFF;
        for _ in 0..5 {
            app.nav.adjust_synth_param(0.05);
        }
        let second = params(&app);
        assert_ne!(second, first);

        app.nav.preset_modal.cursor = PresetModal::SAVE_ROW;
        press(&mut app, KeyCode::Enter);
        type_name(&mut app, "brass");
        press(&mut app, KeyCode::Enter);
        assert!(app.nav.confirm_modal.open, "overwriting did not ask");
        assert_eq!(app.nav.confirm_modal.kind, ConfirmKind::OverwritePreset);

        // Answering no leaves the bank as it was.
        press(&mut app, KeyCode::Char('n'));
        let dir = app.preset_dir.clone().unwrap();
        let bank = preset::load_bank(&dir, InstrumentType::Juno60).unwrap();
        assert_eq!(bank.presets.len(), 1);
        assert_eq!(bank.presets[0].params, first, "'no' overwrote the preset anyway");
        assert!(app.nav.preset_modal.pending_name.is_empty());

        // Answering yes replaces it in place — one row, not two.
        press(&mut app, KeyCode::Enter);
        type_name(&mut app, "brass");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('y'));

        let bank = preset::load_bank(&dir, InstrumentType::Juno60).unwrap();
        assert_eq!(bank.presets.len(), 1, "the same name was stored twice");
        assert_eq!(bank.presets[0].params, second);
        assert_eq!(app.nav.preset_modal.entries, vec!["brass".to_string()]);
        assert!(app.status_message.as_ref().unwrap().0.contains("overwrote"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `d` deletes through the same confirmation the rest of the application
    /// uses, and only after a yes.
    #[test]
    fn d_deletes_a_preset_after_confirming() {
        let mut app = app("delete");
        add_track(&mut app, InstrumentType::Juno60);
        app.nav.focus_pane(Pane::Tracks);

        open_browser(&mut app);
        for name in ["one", "two"] {
            app.nav.preset_modal.cursor = PresetModal::SAVE_ROW;
            press(&mut app, KeyCode::Enter);
            type_name(&mut app, name);
            press(&mut app, KeyCode::Enter);
        }
        assert_eq!(app.nav.preset_modal.entries.len(), 2);

        // Cursor is on "two" after saving it. Ask, then say no.
        assert_eq!(app.nav.preset_modal.selected_name(), Some("two"));
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.nav.confirm_modal.kind, ConfirmKind::DeletePreset);
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(app.nav.preset_modal.entries.len(), 2, "'no' deleted it anyway");

        // Then say yes.
        press(&mut app, KeyCode::Char('d'));
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(app.nav.preset_modal.entries, vec!["one".to_string()]);

        let dir = app.preset_dir.clone().unwrap();
        let bank = preset::load_bank(&dir, InstrumentType::Juno60).unwrap();
        assert_eq!(bank.names(), vec!["one"]);
        // The cursor followed the list down instead of pointing past the end.
        assert!(app.nav.preset_modal.cursor < app.nav.preset_modal.item_count());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Drawing ──

    /// A full bank in a small terminal still draws. The modal sizes itself
    /// from the number of presets, so it is the one overlay whose height is
    /// not a constant — and a modal taller than the buffer is an index past
    /// the end of it, which takes the whole application down.
    #[test]
    fn a_full_bank_draws_at_every_terminal_size() {
        let mut app = app("draw");
        add_track(&mut app, InstrumentType::Juno60);
        app.nav.focus_pane(Pane::Tracks);

        let dir = app.preset_dir.clone().unwrap();
        let mut bank = preset::PresetFile::new(InstrumentType::Juno60);
        for i in 0..preset::MAX_PRESETS {
            bank.store(&format!("preset {i}"), InstrumentType::Juno60, &params(&app))
                .unwrap();
        }
        preset::save_bank(&dir, InstrumentType::Juno60, &bank).unwrap();
        open_browser(&mut app);
        assert_eq!(app.nav.preset_modal.entries.len(), preset::MAX_PRESETS);

        for (w, h) in [(80u16, 24u16), (120, 40), (60, 16), (40, 12), (200, 60)] {
            // Top of the list, middle, and bottom — the scroll window is the
            // part with arithmetic in it.
            for cursor in [0, preset::MAX_PRESETS / 2, preset::MAX_PRESETS] {
                app.nav.preset_modal.cursor = cursor;
                draw(&app, w, h);
            }
        }

        // And an empty bank, which draws the hint line instead of a list.
        app.nav.preset_modal.set_entries(Vec::new());
        app.nav.preset_modal.error = Some("juno60.json is unreadable".into());
        draw(&app, 40, 12);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refusal has to reach the screen. Refusing to load a preset and
    /// saying so only in the debug log is the same silent wrong-sound the
    /// refusal exists to prevent, one step removed.
    #[test]
    fn a_refusal_reaches_the_bottom_bar() {
        let mut app = app("visible");
        add_track(&mut app, InstrumentType::Juno60);
        app.nav.focus_pane(Pane::Tracks);

        let dir = app.preset_dir.clone().unwrap();
        let mut bank = preset::PresetFile::new(InstrumentType::Juno60);
        bank.store("old panel", InstrumentType::Juno60, &params(&app)).unwrap();
        bank.presets[0].params.truncate(16);
        bank.presets[0].param_count = 16;
        preset::save_bank(&dir, InstrumentType::Juno60, &bank).unwrap();

        open_browser(&mut app);
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Esc); // out of the way, so the bar is visible

        let screen = draw(&app, 80, 24).join("\n");
        // 80 columns, the narrowest terminal anyone runs this in: both the
        // refusal and the number that explains it have to survive the width.
        assert!(
            screen.contains("not loaded"),
            "the refusal never made it to the screen:\n{screen}"
        );
        assert!(
            screen.contains("16 controls"),
            "the reason was cut off at 80 columns:\n{screen}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Render one frame into an off-screen buffer and return it as text, one
    /// string per row. Panics on the way out if anything draws outside it.
    fn draw(app: &App, width: u16, height: u16) -> Vec<String> {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let snapshot = app.engine.transport.snapshot();
        let status = app.live_status();
        terminal
            .draw(|frame| crate::ui::render(frame, &snapshot, &app.nav, status))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    /// An empty name is not a preset. The browser stays open and nothing is
    /// written.
    #[test]
    fn an_empty_name_saves_nothing() {
        let mut app = app("empty");
        add_track(&mut app, InstrumentType::Juno60);
        app.nav.focus_pane(Pane::Tracks);

        open_browser(&mut app);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter); // straight back out, nothing typed
        assert!(app.nav.preset_modal.entries.is_empty());
        assert!(app.nav.preset_modal.open);

        let dir = app.preset_dir.clone().unwrap();
        assert!(
            !preset::bank_path(&dir, InstrumentType::Juno60).exists(),
            "an empty name created a bank file"
        );
    }
}
