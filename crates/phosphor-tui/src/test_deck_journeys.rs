//! Journeys through Phosphor on the deck alone.
//!
//! Every gesture here goes through [`crate::deck_sim::DeckSim`] — the bytes
//! the hardware sends, routed as the MIDI callback routes them, through the
//! tap the hardware's messages arrive on. No test in this file types a key.
//! Where a journey needs something the deck cannot do (a file to load onto a
//! pad), the setup does it through the app and says so, and the rest of the
//! journey is still the deck's.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use phosphor_app::state::{InstrumentType, Pane, SPACE_ACTIONS};
    use phosphor_app::surface::layout::ControlId as C;
    use phosphor_app::surface::screen::Screen;
    use phosphor_midi::MidiMessageType;

    use crate::deck_sim::DeckSim;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("phosphor-deck-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn deck(tag: &str) -> DeckSim {
        DeckSim::new(&scratch(tag))
    }

    /// MENU, NAVIGATE down the list to `label`, LOCK.
    fn menu(d: &mut DeckSim, label: &str) {
        let row = SPACE_ACTIONS.iter().position(|(_, l, _)| *l == label).unwrap_or_else(|| panic!("no menu row {label}"));
        d.press(C::Menu);
        assert!(d.app.nav.space_menu.open, "MENU did not open the menu");
        d.turn(C::Navigate, row as i8);
        d.press(C::Lock);
    }

    /// A new track of `instrument`, from the menu and the instrument list.
    fn add_track(d: &mut DeckSim, instrument: InstrumentType) -> usize {
        let before = d.app.nav.tracks.len();
        menu(d, "add instr");
        assert!(d.app.nav.instrument_modal.open, "add instr did not open the instrument list");
        let row = InstrumentType::ALL.iter().position(|i| *i == instrument).unwrap();
        d.turn(C::Navigate, row as i8);
        d.press(C::Lock);
        assert_eq!(d.app.nav.tracks.len(), before + 1, "no {instrument:?} track was made");
        d.app.nav.track_cursor
    }

    /// PART until `screen` has the keys.
    fn part_to(d: &mut DeckSim, screen: Screen) {
        for _ in 0..8 {
            if d.screen() == screen {
                return;
            }
            d.press(C::Part);
        }
        panic!("PART never reached {screen:?}; on {:?}", d.screen());
    }

    /// The column a track's controls are under, with the bank at the start.
    fn column(idx: usize) -> u8 {
        assert!(idx < 8, "track {idx} is not in the first bank");
        idx as u8
    }

    #[test]
    fn the_transport_buttons_drive_the_transport() {
        let mut d = deck("transport");
        d.press(C::Play);
        assert!(d.app.engine.transport.is_playing(), "PLAY did not play");
        d.press(C::Play);
        assert!(!d.app.engine.transport.is_playing(), "PLAY again did not pause");
        d.press(C::Play).press(C::Stop);
        assert!(!d.app.engine.transport.is_playing(), "STOP did not stop");
        let click = d.app.engine.transport.is_metronome_on();
        d.press(C::Click);
        assert_ne!(d.app.engine.transport.is_metronome_on(), click, "CLICK did not toggle the metronome");
        let rec = d.app.engine.transport.is_recording();
        d.press(C::Rec);
        assert_ne!(d.app.engine.transport.is_recording(), rec, "REC did not arm");
        d.press(C::Rec);
        assert!(!d.app.nav.space_menu.open, "a transport button left the menu open");
    }

    /// The function knob turns the tempo until another job is picked, and
    /// TAP sets it from the beat.
    #[test]
    fn the_function_knob_turns_the_tempo_and_tap_sets_it() {
        let mut d = deck("tempo");
        let bpm = d.app.engine.transport.tempo_bpm();
        d.press(C::FnTempo).turn(C::Function, 3).turn(C::Function, -1);
        assert_eq!(d.app.engine.transport.tempo_bpm(), bpm + 2.0, "TEMPO is not one BPM a click");
        // Taps half a second apart are 120.
        let t0 = Instant::now();
        for i in 0..4 {
            d.app.tap_tempo(t0 + Duration::from_millis(500 * i));
        }
        assert_eq!(d.app.engine.transport.tempo_bpm(), 120.0, "four taps at 2 Hz are not 120");
        // A bounce is not a beat.
        d.app.tap_tempo(t0 + Duration::from_millis(1_550));
        assert_eq!(d.app.engine.transport.tempo_bpm(), 120.0, "a bounce moved the tempo");
    }

    /// Every kind of track, made from the deck: the menu and the knob.
    #[test]
    fn every_instrument_can_be_added_from_the_deck() {
        let mut d = deck("instruments");
        for &instrument in InstrumentType::ALL {
            let idx = add_track(&mut d, instrument);
            let made = &d.app.nav.tracks[idx];
            if instrument == InstrumentType::Sequencer {
                assert!(made.sequencer.is_some(), "the sequencer track has no sequencer");
            } else {
                assert_eq!(made.instrument_type, Some(instrument));
            }
            d.press(C::Back).press(C::Back);
        }
    }

    /// NAVIGATE walks the track list, LOCK selects, BACK steps out.
    #[test]
    fn navigate_and_lock_walk_into_a_track_and_back_out() {
        let mut d = deck("lock");
        add_track(&mut d, InstrumentType::Synth);
        add_track(&mut d, InstrumentType::Rhodes);
        d.tap(C::Back, 4);
        d.press(C::ModeSelect).press(C::TrackButton(0)).press(C::Back);
        assert_eq!(d.screen(), Screen::Tracks, "BACK did not let go of the track");
        let at = d.app.nav.track_cursor;
        d.turn(C::Navigate, 1);
        assert_eq!(d.app.nav.track_cursor, at + 1, "NAVIGATE did not walk down the list");
        d.press(C::Lock);
        assert_eq!(d.screen(), Screen::Track, "LOCK did not select the track");
        d.press(C::Back);
        assert_eq!(d.screen(), Screen::Tracks);
    }

    #[test]
    fn track_buttons_select_mute_solo_and_arm_their_own_columns() {
        let mut d = deck("columns");
        let a = add_track(&mut d, InstrumentType::Synth);
        let b = add_track(&mut d, InstrumentType::Rhodes);
        d.press(C::TrackButton(column(b)));
        assert_eq!(d.app.nav.track_cursor, b, "the track button did not select its track");
        assert_eq!(d.screen(), Screen::Track);
        d.press(C::ModeMute).press(C::TrackButton(column(a)));
        assert!(d.app.nav.tracks[a].muted, "MUTE mode did not mute");
        assert_eq!(d.app.nav.track_cursor, b, "muting moved the selection");
        d.press(C::Undo);
        assert!(!d.app.nav.tracks[a].muted, "UNDO did not take the mute back");
        d.press(C::Redo);
        assert!(d.app.nav.tracks[a].muted, "REDO did not put it back");
        // Solo and arm are listening state, off the undo stack like a
        // selection; each press flips what is there.
        let (soloed, armed) = (d.app.nav.tracks[a].soloed, d.app.nav.tracks[a].armed);
        d.press(C::ModeSolo).press(C::TrackButton(column(a)));
        d.press(C::ModeArm).press(C::TrackButton(column(a)));
        assert_ne!(d.app.nav.tracks[a].soloed, soloed, "SOLO mode did not toggle solo");
        assert_ne!(d.app.nav.tracks[a].armed, armed, "ARM mode did not toggle arm");
    }

    /// A fader moves nothing until it reaches the track's level, then rides
    /// it — and a whole ride is one UNDO.
    #[test]
    fn a_fader_catches_the_level_and_one_undo_takes_the_ride_back() {
        let mut d = deck("fader");
        let idx = add_track(&mut d, InstrumentType::Synth);
        let n = column(idx);
        let start = d.app.nav.tracks[idx].volume;
        d.slide(n, 0, 30);
        assert_eq!(d.app.nav.tracks[idx].volume, start, "the fader jumped the level before catching it");
        d.slide(n, 30, 127);
        assert!(d.app.nav.tracks[idx].volume > start, "riding past the level did not take it up");
        d.slide(n, 127, 1);
        let bottom = d.app.nav.tracks[idx].volume_db().unwrap();
        assert!((bottom + 40.0).abs() < 0.6, "the bottom of travel is {bottom} dB, not the floor");
        d.slide(n, 1, 0);
        assert_eq!(d.app.nav.tracks[idx].volume_db(), None, "the very bottom is not silence");
        d.press(C::Undo);
        assert!((d.app.nav.tracks[idx].volume - start).abs() < 0.03, "one UNDO did not take the ride back");
    }

    #[test]
    fn the_function_knob_rides_the_master() {
        let mut d = deck("master");
        let master = d.app.nav.tracks.iter().position(|t| t.kind == phosphor_core::project::TrackKind::Master).unwrap();
        let start = d.app.nav.tracks[master].volume;
        d.press(C::FnMaster).turn(C::Function, -3);
        assert!(d.app.nav.tracks[master].volume < start, "MASTER did not move the master");
    }

    /// A locked instrument's controls are on the column knobs, eight at a
    /// time; a turn is undoable like a key press; LAST turns it again.
    #[test]
    fn the_column_knobs_turn_the_locked_instruments_controls() {
        let mut d = deck("knobs");
        let idx = add_track(&mut d, InstrumentType::Juno60);
        part_to(&mut d, Screen::Instrument);
        assert!(d.strip().contains("Instrument"), "the deck's line does not say what is locked: {}", d.strip());
        let before = d.app.nav.tracks[idx].synth_params[3];
        d.turn(C::Knob(3), 4);
        let after = d.app.nav.tracks[idx].synth_params[3];
        assert_ne!(after, before, "knob 4 did not turn control 4");
        d.press(C::Undo);
        assert!((d.app.nav.tracks[idx].synth_params[3] - before).abs() < 1e-6, "UNDO did not take the turn back");
        // Page two is controls 9-16.
        d.press(C::PageNext);
        let before = d.app.nav.tracks[idx].synth_params[8];
        d.turn(C::Knob(0), 3);
        assert_ne!(d.app.nav.tracks[idx].synth_params[8], before, "page two's first knob is not control 9");
        let mid = d.app.nav.tracks[idx].synth_params[8];
        d.press(C::FnLast).turn(C::Function, 2);
        assert_ne!(d.app.nav.tracks[idx].synth_params[8], mid, "LAST did not turn the last knob");
    }

    /// On a step grid the pads are steps; everywhere else they are notes.
    #[test]
    fn pads_write_steps_on_a_grid() {
        let mut d = deck("steps");
        let idx = add_track(&mut d, InstrumentType::Sequencer);
        part_to(&mut d, Screen::Steps);
        for pad in [0, 4, 8, 12] {
            d.hit(pad, 100);
        }
        let lane = |d: &DeckSim| -> Vec<usize> {
            let state = d.app.nav.tracks[idx].sequencer.as_ref().unwrap();
            let lane = state.pattern().lanes.iter().find(|l| l.steps.iter().any(|s| s.on));
            lane.map(|l| l.steps.iter().enumerate().filter(|(_, s)| s.on).map(|(i, _)| i).collect()).unwrap_or_default()
        };
        assert_eq!(lane(&d), [0, 4, 8, 12], "four on the floor did not land");
        assert!(d.played.is_empty(), "a step pad also played a note");
        d.hit(4, 100);
        assert_eq!(lane(&d), [0, 8, 12], "a second hit did not take the step off");
        d.press(C::Undo);
        assert_eq!(lane(&d), [0, 4, 8, 12], "UNDO did not put the step back");
    }

    #[test]
    fn pads_play_notes_from_c3_and_move_by_octaves() {
        let mut d = deck("notes");
        add_track(&mut d, InstrumentType::Synth);
        d.hit(8, 100);
        assert!(
            matches!(d.played.first(), Some(MidiMessageType::NoteOn { note: 48, velocity: 100, .. })),
            "the bottom-left pad did not play C3: {:?}",
            d.played
        );
        d.press(C::PadsUp);
        d.played.clear();
        d.hit(8, 100);
        assert!(matches!(d.played.first(), Some(MidiMessageType::NoteOn { note: 60, .. })), "{:?}", d.played);
    }

    /// COPY, walk two keys, PASTE: a pad copied from the deck. Loading the
    /// first sound is setup — there is no file on the deck to load.
    #[test]
    fn a_sampler_pad_copies_from_the_deck() {
        let mut d = deck("copy");
        let idx = add_track(&mut d, InstrumentType::Sampler);
        part_to(&mut d, Screen::Pads);
        let pcm = std::sync::Arc::new(phosphor_plugin::sample::SamplePcm { data: vec![0.3; 4_410], channels: 1, sample_rate: 44_100.0 });
        let kit = d.app.nav.tracks[idx].sampler.as_mut().unwrap();
        let from = kit.cursor;
        kit.add_wav_layer(from, PathBuf::from("kick.wav"), pcm).unwrap();

        d.press(C::Copy).turn(C::Navigate, 2).press(C::Paste);
        let kit = d.app.nav.tracks[idx].sampler.as_ref().unwrap();
        assert_eq!(kit.pads[from + 2], kit.pads[from], "PASTE did not copy the pad two keys along");
        d.press(C::Undo);
        assert!(d.app.nav.tracks[idx].sampler.as_ref().unwrap().pads[from + 2].layers.is_empty(), "UNDO left the paste");
        d.press(C::Redo);
        assert_eq!(d.app.nav.tracks[idx].sampler.as_ref().unwrap().pads[from + 2].layers.len(), 1, "REDO lost it");
    }

    /// The action buttons are the screen's own verbs: on the pad map, a pad
    /// knob turned and then MUTE.
    #[test]
    fn the_action_buttons_follow_the_screen() {
        let mut d = deck("actions");
        let idx = add_track(&mut d, InstrumentType::Sampler);
        part_to(&mut d, Screen::Pads);
        let pcm = std::sync::Arc::new(phosphor_plugin::sample::SamplePcm { data: vec![0.3; 4_410], channels: 1, sample_rate: 44_100.0 });
        let kit = d.app.nav.tracks[idx].sampler.as_mut().unwrap();
        let pad = kit.cursor;
        kit.add_wav_layer(pad, PathBuf::from("snare.wav"), pcm).unwrap();
        let muted = |d: &DeckSim| d.app.nav.tracks[idx].sampler.as_ref().unwrap().pads[pad].layers[0].mute;
        let slot = phosphor_app::surface::actions::actions(Screen::Pads, false)
            .iter()
            .position(|(label, _)| *label == "MUTE")
            .unwrap();
        d.press(C::Action(slot as u8));
        assert!(muted(&d), "the MUTE action did not mute the sound");
        d.press(C::Undo);
        assert!(!muted(&d), "UNDO did not take the mute back");
    }

    /// A question the app asks is answered by the action buttons: YES, NO.
    #[test]
    fn a_question_is_answered_from_the_deck() {
        let mut d = deck("confirm");
        let idx = add_track(&mut d, InstrumentType::Synth);
        let count = d.app.nav.tracks.len();
        d.press(C::TrackButton(column(idx))).press(C::Delete);
        assert!(d.app.nav.confirm_modal.open, "DELETE did not ask");
        d.press(C::Action(1));
        assert!(!d.app.nav.confirm_modal.open);
        assert_eq!(d.app.nav.tracks.len(), count, "NO deleted the track anyway");
        d.press(C::Delete).press(C::Action(0));
        assert_eq!(d.app.nav.tracks.len(), count - 1, "YES did not delete");
        d.press(C::Undo);
        assert_eq!(d.app.nav.tracks.len(), count, "UNDO did not bring the track back");
    }

    #[test]
    fn save_from_the_deck_opens_the_picker_and_back_closes_it() {
        let mut d = deck("save");
        d.press(C::Save);
        assert!(d.app.nav.file_picker.open, "SAVE on an unnamed session did not ask where");
        d.press(C::Back);
        assert!(!d.app.nav.file_picker.open, "BACK did not close the picker");
    }

    /// The menu's other doors open and close from the deck.
    #[test]
    fn the_menus_rooms_open_and_close_from_the_deck() {
        let mut d = deck("rooms");
        add_track(&mut d, InstrumentType::Synth);
        menu(&mut d, "fingers");
        assert!(d.app.nav.practice.open, "fingers did not open the practice room");
        d.tap(C::Back, 3);
        assert!(!d.app.nav.practice.open, "BACK did not leave the practice room");
        menu(&mut d, "presets");
        assert!(d.app.nav.preset_modal.open, "presets did not open");
        d.press(C::Back);
        assert!(!d.app.nav.preset_modal.open);
        menu(&mut d, "help");
        assert!(d.app.nav.space_menu.open, "help did not open");
        d.tap(C::Back, 2);
        assert!(!d.app.nav.space_menu.open);
    }

    #[test]
    fn unbuilt_controls_say_what_they_are_waiting_for() {
        let mut d = deck("unbuilt");
        d.press(C::MyPage);
        assert!(d.flash().contains("not built yet"), "{}", d.flash());
    }

    /// The keyboard the deck is clamped to still plays through: its notes are
    /// not the deck's and go on their way — here, moving a sampler's caret.
    #[test]
    fn the_keyboard_still_plays_through_the_deck() {
        let mut d = deck("keyboard");
        let idx = add_track(&mut d, InstrumentType::Sampler);
        part_to(&mut d, Screen::Pads);
        d.play(72, 100);
        let kit = d.app.nav.tracks[idx].sampler.as_ref().unwrap();
        assert_eq!(phosphor_app::sampler::SamplerState::note_of_pad(kit.cursor), 72, "a keyboard note was taken for the deck");
    }

    /// SHIFT is a held modifier, not a latch.
    #[test]
    fn shift_lets_go_when_it_is_let_go() {
        let mut d = deck("shift");
        d.shift(C::Copy);
        assert!(!d.app.deck.shift, "SHIFT stuck down");
    }

    /// Without a deck the screen is the screen it always was: the deck's line
    /// arrives with the deck's first message.
    #[test]
    fn the_deck_line_appears_only_once_a_deck_is_used() {
        let mut d = deck("line");
        crate::test_support::press(&mut d.app, crossterm::event::KeyCode::Char('j'));
        assert!(d.app.nav.deck_strip.is_none(), "a keyboard-only session drew the deck's line");
        d.press(C::FnTempo);
        assert!(d.strip().starts_with("DECK"), "{}", d.strip());
        assert_eq!(d.app.nav.focused_pane, Pane::Tracks, "FN TEMPO moved the focus");
    }
}
