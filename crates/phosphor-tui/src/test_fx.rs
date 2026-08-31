//! Integration tests for the insert layer's front end.
//!
//! The audio side of it is tested where it lives, in `phosphor-core`. What is
//! under test here is the wiring: that the bus strips reach the mixer at all,
//! that choosing an effect either produces one or says why it did not, and
//! that pan, sends and chains survive being written to a file and read back —
//! including the part that has bitten this format twice already, which is a
//! session written before a feature existed still opening afterwards.

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::state::*;
    use phosphor_core::fx::{FxTarget, SendSlot};
    use phosphor_core::mixer::MixerCommand;
    use phosphor_core::project::TrackKind;
    use phosphor_core::EngineConfig;

    fn app() -> App {
        App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false)
    }

    /// A directory of this test's own, so two of them cannot race on a file.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("phosphor-fx-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("test.phos")
    }

    fn instrument_track(app: &App) -> &TrackState {
        app.nav
            .tracks
            .iter()
            .find(|t| t.instrument_type.is_some())
            .expect("expected an instrument track")
    }

    /// The buses are strips in the mixer, not decorations on the screen. If
    /// this stops happening they have no meters, no return level and nowhere
    /// to put a reverb — and nothing else in the application would notice.
    #[test]
    fn the_bus_strips_are_registered_with_the_mixer() {
        let app = app();
        let commands = app.drain_mixer_commands();
        let kinds: Vec<TrackKind> = commands
            .iter()
            .filter_map(|c| match c {
                MixerCommand::AddTrack { kind, .. } => Some(*kind),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            vec![TrackKind::SendA, TrackKind::SendB, TrackKind::Master],
            "the bus strips never reached the audio thread"
        );
    }

    /// Every bus strip carries a handle, which is what its meter and its
    /// return level are read through — and none of them counts as a track the
    /// player can arm or load a patch onto.
    #[test]
    fn a_bus_has_a_handle_but_is_not_a_live_track() {
        let app = app();
        for track in app.nav.tracks.iter().filter(|t| t.is_bus()) {
            assert!(track.handle.is_some(), "{} has no handle", track.name);
            assert!(!track.is_live(), "{} counts as a live track", track.name);
            assert!(track.mixer_id.is_none(), "{} was given a track id", track.name);
            assert_eq!(track.volume, 1.0, "{} does not return at unity", track.name);
        }
    }

    /// The menu does not lie. An effect that this build cannot make yet is
    /// refused out loud rather than added as a slot that does nothing — and
    /// nothing is sent to the audio thread.
    #[test]
    fn choosing_an_unbuilt_effect_says_so_and_adds_nothing() {
        let mut app = app();
        app.create_instrument_track(InstrumentType::Synth);
        let _ = app.drain_mixer_commands();

        app.nav.fx_menu.open = true;
        app.nav.fx_menu.cursor = 0;
        app.fx_menu_choose();

        assert!(!app.nav.fx_menu.open, "the menu stayed open");
        let commands = app.drain_mixer_commands();
        let added = commands
            .iter()
            .filter(|c| matches!(c, MixerCommand::AddFx { .. }))
            .count();
        let status = app.live_status().unwrap_or_default().to_string();
        if added == 0 {
            assert!(
                status.contains("not built"),
                "nothing was added and nothing was said: status was {status:?}"
            );
            assert!(instrument_track(&app).fx_chain.is_empty());
        } else {
            // Once the effect exists this is the branch that runs: the chain
            // and the audio thread move together.
            assert_eq!(instrument_track(&app).fx_chain.len(), 1);
        }
    }

    /// Pan and the sends are the two routing controls this milestone adds,
    /// and both have to reach the audio thread and come back out of a file.
    #[test]
    fn pan_and_sends_survive_a_save_and_load() {
        let path = scratch("routing");
        let mut saving = app();
        saving.create_instrument_track(InstrumentType::Synth);
        let track_index = saving.nav.track_cursor;
        {
            let track = &mut saving.nav.tracks[track_index];
            track.adjust_pan(-8); // hard left, in eight presses
            track.set_send_db(SendSlot::A, -6.0);
            track.set_send_db(SendSlot::B, -12.0);
        }
        saving.sync_routing(track_index);

        // What reached the audio thread.
        let commands = saving.drain_mixer_commands();
        let pan = commands.iter().find_map(|c| match c {
            MixerCommand::SetPan { pan, .. } => Some(*pan),
            _ => None,
        });
        assert!(
            pan.is_some_and(|p| (p - (-0.4)).abs() < 1.0e-5),
            "the pan the audio thread got was {pan:?}"
        );
        let sends: Vec<f32> = commands
            .iter()
            .filter_map(|c| match c {
                MixerCommand::SetSendLevel { gain, .. } => Some(*gain),
                _ => None,
            })
            .collect();
        assert_eq!(sends.len(), 2, "both sends have to be pushed");
        assert!((sends[0] - 0.501_187).abs() < 1.0e-4, "send A arrived at {}", sends[0]);

        saving.do_save(&path.to_string_lossy());

        let mut reopened = app();
        reopened.do_load(&path.to_string_lossy());
        let track = instrument_track(&reopened);
        assert!((track.pan - (-0.4)).abs() < 1.0e-5, "pan came back as {}", track.pan);
        assert!(
            (track.send_db(SendSlot::A).unwrap() - (-6.0)).abs() < 0.01,
            "send A came back as {:?}",
            track.send_db(SendSlot::A)
        );
        assert!(
            (track.send_db(SendSlot::B).unwrap() - (-12.0)).abs() < 0.01,
            "send B came back as {:?}",
            track.send_db(SendSlot::B)
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A sidechain key is stored as a position in the file and comes back as
    /// the identity of whatever track that position turned into. Track ids
    /// are handed out per run and mean nothing between sessions; this is the
    /// test that would catch someone storing one.
    #[test]
    fn a_sidechain_key_survives_a_save_and_load() {
        let path = scratch("key");
        let mut saving = app();
        saving.create_instrument_track(InstrumentType::Synth); // the key source
        let source_index = saving.nav.track_cursor;
        saving.create_instrument_track(InstrumentType::DX7); // the keyed track
        let keyed_index = saving.nav.track_cursor;
        let source_id = saving.nav.tracks[source_index].mixer_id.unwrap();
        saving.nav.tracks[keyed_index].key_source = Some(source_id);
        saving.do_save(&path.to_string_lossy());

        let mut reopened = app();
        reopened.do_load(&path.to_string_lossy());
        let tracks: Vec<&TrackState> = reopened
            .nav
            .tracks
            .iter()
            .filter(|t| t.instrument_type.is_some())
            .collect();
        assert_eq!(tracks.len(), 2);
        let source_id = tracks[0].mixer_id.expect("the source has an id");
        assert_eq!(
            tracks[1].key_source,
            Some(source_id),
            "the key did not come back pointing at the first track"
        );
        assert!(tracks[0].key_source.is_none());

        let keyed_id = tracks[1].mixer_id.unwrap();
        let commands = reopened.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(
                c,
                MixerCommand::SetKeySource { track_id, source: Some(id) }
                    if *track_id == keyed_id && *id == source_id
            )),
            "the audio thread was never told about the key"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// **The additive-format guarantee.** A session that uses nothing this
    /// milestone added writes nothing this milestone added: the file has the
    /// same shape, key for key, that it had before the insert layer existed.
    ///
    /// The comparison is on the keys rather than the bytes because a
    /// save-load-save round trip has never been byte-stable — a selector is
    /// stored by position and comes back as that position's exact knob
    /// fraction, which is not always the fraction it was saved from. That
    /// predates this work and is what the `discrete` block exists to do.
    #[test]
    fn a_session_that_uses_no_effects_has_the_shape_it_always_had() {
        let path = scratch("shape");
        let mut saving = app();
        saving.create_instrument_track(InstrumentType::Juno60);
        saving.create_instrument_track(InstrumentType::DX7);
        saving.do_save(&path.to_string_lossy());
        let first = std::fs::read_to_string(&path).expect("the session was written");

        let mut reopened = app();
        reopened.do_load(&path.to_string_lossy());
        let resaved = scratch("shape2");
        reopened.do_save(&resaved.to_string_lossy());
        let second = std::fs::read_to_string(&resaved).expect("written again");

        /// Every JSON key in the file, in order.
        fn keys(text: &str) -> Vec<&str> {
            text.lines()
                .filter_map(|line| line.trim().strip_prefix('"'))
                .filter_map(|rest| rest.split_once('"'))
                .map(|(key, _)| key)
                .collect()
        }
        assert_eq!(
            keys(&first),
            keys(&second),
            "a save-load-save round trip changed the shape of the file"
        );
        for text in [&first, &second] {
            for absent in ["\"fx\"", "\"pan\"", "\"send_a\"", "\"key_track\"", "\"buses\""] {
                assert!(
                    !text.contains(absent),
                    "a session that uses no effects wrote {absent}"
                );
            }
        }

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        let _ = std::fs::remove_dir_all(resaved.parent().unwrap());
    }

    /// A bus chain belongs to the session that was open. Loading another one
    /// takes it off — on both sides — rather than leaving the last session's
    /// reverb on the send.
    #[test]
    fn loading_a_session_clears_the_previous_ones_bus_chain() {
        let path = scratch("buses");
        let mut saving = app();
        saving.create_instrument_track(InstrumentType::Synth);
        saving.do_save(&path.to_string_lossy());

        let mut reopened = app();
        // A chain the UI mirror believes in, as if the previous session had
        // one. It is dropped on load whether or not this build can make the
        // effect, which is the property under test.
        let bus_index = reopened
            .nav
            .tracks
            .iter()
            .position(|t| t.kind == TrackKind::SendA)
            .unwrap();
        reopened.nav.tracks[bus_index].fx_chain =
            vec![FxInstance::new(FxType::Reverb, vec![])];
        let _ = reopened.drain_mixer_commands();

        reopened.do_load(&path.to_string_lossy());
        assert!(
            reopened.nav.tracks[bus_index].fx_chain.is_empty(),
            "the previous session's bus chain survived the load"
        );
        let commands = reopened.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(
                c,
                MixerCommand::RemoveFx { target: FxTarget::BusA, slot: 0 }
            )),
            "the audio thread was never told to take it off"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// The strip names a bus by what is in it. An empty bus keeps its letter,
    /// which is what every bus is until the reverb and the delay are built.
    #[test]
    fn a_bus_is_named_by_its_first_effect() {
        let mut app = app();
        let index = app
            .nav
            .tracks
            .iter()
            .position(|t| t.kind == TrackKind::SendA)
            .unwrap();
        assert_eq!(
            phosphor_app::fx::bus_label(&app.nav.tracks[index].fx_chain, SendSlot::A),
            "snd a"
        );
        app.nav.tracks[index].fx_chain = vec![FxInstance::new(FxType::Reverb, vec![])];
        assert_eq!(
            phosphor_app::fx::bus_label(&app.nav.tracks[index].fx_chain, SendSlot::A),
            "rvb"
        );
    }

    /// The pan control snaps onto the centre on the way past. The centre is
    /// the one position that has to be exactly reachable, because it is the
    /// one that leaves the track untouched.
    #[test]
    fn the_pan_control_lands_on_centre() {
        let mut app = app();
        app.create_instrument_track(InstrumentType::Synth);
        let track = &mut app.nav.tracks[app.nav.track_cursor];
        assert_eq!(track.pan, TrackState::CENTRE_PAN);

        for _ in 0..3 {
            track.adjust_pan(1);
        }
        assert!((track.pan - 0.15).abs() < 1.0e-6, "pan is {}", track.pan);
        for _ in 0..3 {
            track.adjust_pan(-1);
        }
        assert_eq!(track.pan, TrackState::CENTRE_PAN, "the centre was walked past");

        for _ in 0..100 {
            track.adjust_pan(-1);
        }
        assert_eq!(track.pan, -1.0, "the travel has an end");
    }

    /// A send is closed until it is opened, and closing it again is a level
    /// rather than a very small one.
    #[test]
    fn a_send_closes_completely() {
        let mut app = app();
        app.create_instrument_track(InstrumentType::Synth);
        let track = &mut app.nav.tracks[app.nav.track_cursor];
        assert_eq!(track.send(SendSlot::A), 0.0);
        assert_eq!(track.send_db(SendSlot::A), None, "a closed send has no level");

        track.set_send_db(SendSlot::A, 0.0);
        assert_eq!(track.send(SendSlot::A), 1.0);
        track.set_send_db(SendSlot::A, 6.0);
        assert_eq!(track.send(SendSlot::A), 1.0, "a send cannot be pushed past unity");
        track.set_send_db(SendSlot::A, phosphor_core::fx::SILENT_DB);
        assert_eq!(track.send(SendSlot::A), 0.0, "the bottom of a send is silence");
    }
}
