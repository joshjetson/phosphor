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
    use crate::app::App;
    use crate::session::{SessionFile, SessionSelector};
    use crate::state::*;
    use phosphor_app::discrete;
    use phosphor_core::EngineConfig;
    use phosphor_dsp::{drum_rack, dx7};

    fn app() -> App {
        App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false)
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
            let expected: Vec<usize> = (0..track.synth_params.len())
                .filter(|&i| discrete::is_discrete(*instrument, i))
                .collect();
            let stored: Vec<usize> = track.discrete.iter().map(|s| s.param).collect();
            assert_eq!(stored, expected, "{instrument:?}");
            assert!(!expected.is_empty(), "{instrument:?} has no selector at all");

            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }
    }
}
