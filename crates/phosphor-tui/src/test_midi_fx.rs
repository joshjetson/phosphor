//! The MIDI rack: the arp in a track's pre-instrument slot, driven the way
//! the front end drives it — add, adjust, switch, remove, undo, save.

#[cfg(test)]
mod tests {
    use phosphor_core::EngineConfig;

    use crate::app::App;
    use crate::state::{InstrumentType, MidiFxType, RackSlot};

    fn app() -> App {
        App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false)
    }

    fn app_with_track() -> (App, usize) {
        let mut app = app();
        app.create_instrument_track(InstrumentType::Synth);
        let ti = app.nav.track_cursor;
        (app, ti)
    }

    /// Adding the arp lands one slot at defaults, and one undo lifts it.
    #[test]
    fn adding_the_arp_is_one_undo_step() {
        let (mut app, ti) = app_with_track();
        app.add_midi_fx(ti, MidiFxType::Arp);
        assert_eq!(app.nav.tracks[ti].midi_fx.len(), 1, "the arp never landed");
        let inst = &app.nav.tracks[ti].midi_fx[0];
        assert_eq!(inst.params.len(), MidiFxType::Arp.params().len());
        assert!((inst.params[1] - 5.0).abs() < 1e-6, "rate default should be 1/16");

        app.perform_undo();
        assert!(app.nav.tracks[ti].midi_fx.is_empty(), "undo did not lift the arp");
        app.perform_redo();
        assert_eq!(app.nav.tracks[ti].midi_fx.len(), 1, "redo did not restore it");
    }

    /// A knob sweep folds to one undo step, values clamp to the table, and
    /// undo puts the sweep back where it began.
    #[test]
    fn a_knob_sweep_is_one_undo_step_and_clamps() {
        let (mut app, ti) = app_with_track();
        app.add_midi_fx(ti, MidiFxType::Arp);
        let gate0 = app.nav.tracks[ti].midi_fx[0].params[2];

        app.set_midi_fx_param(ti, 0, 2, 80.0);
        app.set_midi_fx_param(ti, 0, 2, 95.0);
        app.set_midi_fx_param(ti, 0, 2, 400.0); // clamps to 200
        assert!((app.nav.tracks[ti].midi_fx[0].params[2] - 200.0).abs() < 1e-6, "no clamp");

        app.perform_undo();
        assert!(
            (app.nav.tracks[ti].midi_fx[0].params[2] - gate0).abs() < 1e-6,
            "the sweep did not fold into one step"
        );
        // The next undo must be the add itself, not a mid-sweep value.
        app.perform_undo();
        assert!(app.nav.tracks[ti].midi_fx.is_empty());
    }

    /// The bypass switch is its own undoable step.
    #[test]
    fn bypass_is_undoable() {
        let (mut app, ti) = app_with_track();
        app.add_midi_fx(ti, MidiFxType::Arp);
        app.set_midi_fx_bypass(ti, 0, true);
        assert!(app.nav.tracks[ti].midi_fx[0].bypass);
        app.perform_undo();
        assert!(!app.nav.tracks[ti].midi_fx[0].bypass, "bypass did not undo");
    }

    /// The combined rack addresses MIDI slots first, then audio inserts.
    #[test]
    fn the_combined_rack_addresses_both_halves() {
        let (mut app, ti) = app_with_track();
        app.add_midi_fx(ti, MidiFxType::Arp);
        let outcome = app.nav.add_fx(crate::state::FxType::Eq);
        app.apply_fx_add(outcome);
        assert_eq!(app.nav.rack_len(), 2);
        assert_eq!(app.nav.rack_slot_at(0), Some(RackSlot::Midi(0)));
        assert_eq!(app.nav.rack_slot_at(1), Some(RackSlot::Audio(0)));
        assert_eq!(app.nav.rack_slot_at(2), None);
        let _ = ti;
    }

    /// The rack survives a save: the session stores it by name and params,
    /// and an unknown name from the future is dropped, not fatal.
    #[test]
    fn the_rack_saves_and_unknown_kinds_drop() {
        let (mut app, ti) = app_with_track();
        app.add_midi_fx(ti, MidiFxType::Arp);
        app.set_midi_fx_param(ti, 0, 2, 85.0);
        app.set_midi_fx_bypass(ti, 0, true);

        let stored = crate::session::midi_fx_to_session(&app.nav.tracks[ti].midi_fx);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].kind, "arp");
        assert!(stored[0].bypass);
        assert!((stored[0].params[2] - 85.0).abs() < 1e-6);

        let (rack, dropped) = crate::session::midi_fx_from_session(&stored);
        assert_eq!(dropped, 0);
        assert_eq!(rack, app.nav.tracks[ti].midi_fx);

        let alien = crate::session::SessionFx {
            kind: "chordizer-9000".into(),
            bypass: false,
            params: vec![1.0],
        };
        let (rack, dropped) = crate::session::midi_fx_from_session(&[alien]);
        assert!(rack.is_empty());
        assert_eq!(dropped, 1, "the unknown effect should be counted, not fatal");
    }

    /// Duplicating a track copies its MIDI rack with it.
    #[test]
    fn duplicate_carries_the_rack() {
        let (mut app, ti) = app_with_track();
        app.add_midi_fx(ti, MidiFxType::Arp);
        app.set_midi_fx_param(ti, 0, 6, 58.0); // the Dilla swing
        app.duplicate_current_track();
        let copy = &app.nav.tracks[ti + 1];
        assert_eq!(copy.midi_fx.len(), 1, "the rack did not copy");
        assert!((copy.midi_fx[0].params[6] - 58.0).abs() < 1e-6, "the settings did not copy");
    }

    /// Chord and arp land in signal order — chord first — whichever order
    /// they were added in, and a second copy of either is refused.
    #[test]
    fn the_rack_keeps_chord_before_arp() {
        let (mut app, ti) = app_with_track();
        app.add_midi_fx(ti, MidiFxType::Arp);
        app.add_midi_fx(ti, MidiFxType::Chord);
        let types: Vec<MidiFxType> =
            app.nav.tracks[ti].midi_fx.iter().map(|s| s.fx_type).collect();
        assert_eq!(types, vec![MidiFxType::Chord, MidiFxType::Arp], "order: {types:?}");

        app.add_midi_fx(ti, MidiFxType::Chord);
        assert_eq!(app.nav.tracks[ti].midi_fx.len(), 2, "a duplicate device slipped in");
    }

    /// The chord panel speaks music, not numbers: the root reads as a note
    /// name, the split as a note with its octave, the scale by name.
    #[test]
    fn the_chord_panel_reads_in_words() {
        assert_eq!(MidiFxType::Chord.value_text(0, 0.0), "C");
        assert_eq!(MidiFxType::Chord.value_text(1, 0.0), "major");
        assert_eq!(MidiFxType::Chord.value_text(2, 3.0), "lush");
        assert_eq!(MidiFxType::Chord.value_text(3, 4.0), "quartal");
        assert_eq!(MidiFxType::Chord.value_text(4, 60.0), "C4");
        assert_eq!(MidiFxType::Chord.value_text(5, 1.0), "root -1 oct");
        assert_eq!(MidiFxType::Chord.value_text(6, 25.0), "25ms");
    }
}
