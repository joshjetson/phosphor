//! Journeys through Phosphor on the deck alone.
//!
//! Every gesture here goes through [`crate::deck_sim::DeckSim`] — the bytes
//! the hardware sends, through the tap the hardware's messages arrive on. No
//! test in this file types a key. Where a journey needs something the deck
//! cannot do yet (load a file onto a pad, say — `a` is a listed gap), the
//! setup does it through the app and says so, and the rest of the journey is
//! still the deck's.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use phosphor_app::state::{InstrumentType, Pane, SPACE_ACTIONS};
    use phosphor_app::surface::layout::ControlId as C;

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

    /// MENU, down the list to `label`, ENTER — the Space menu by arrows.
    fn menu(d: &mut DeckSim, label: &str) {
        let row = SPACE_ACTIONS.iter().position(|(_, l, _)| *l == label).unwrap_or_else(|| panic!("no menu row {label}"));
        d.press(C::Menu);
        assert!(d.app.nav.space_menu.open, "MENU did not open the menu");
        d.tap(C::Down, row);
        d.press(C::Enter);
    }

    /// A new track of `instrument`, from the menu and the instrument list.
    fn add_track(d: &mut DeckSim, instrument: InstrumentType) -> usize {
        let before = d.app.nav.tracks.len();
        menu(d, "add instr");
        assert!(d.app.nav.instrument_modal.open, "add instr did not open the instrument list");
        let row = InstrumentType::ALL.iter().position(|i| *i == instrument).unwrap();
        d.tap(C::Down, row);
        d.press(C::Enter);
        assert_eq!(d.app.nav.tracks.len(), before + 1, "no {instrument:?} track was made");
        d.app.nav.track_cursor
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
        let bpm = d.app.engine.transport.tempo_bpm();
        d.turn(C::Tempo, 3).turn(C::Tempo, -1);
        assert_eq!(d.app.engine.transport.tempo_bpm(), bpm + 2.0, "TEMPO is not one BPM a click");
        let rec = d.app.engine.transport.is_recording();
        d.press(C::Rec);
        assert_ne!(d.app.engine.transport.is_recording(), rec, "REC did not arm");
        d.press(C::Rec);
        d.press(C::Panic);
        assert!(!d.app.nav.space_menu.open, "a transport button left the menu open");
    }

    #[test]
    fn the_pane_buttons_jump_and_tab_and_back_walk() {
        let mut d = deck("panes");
        d.press(C::PaneTracks);
        assert_eq!(d.app.nav.focused_pane, Pane::Tracks);
        d.press(C::PaneClip);
        assert_eq!(d.app.nav.focused_pane, Pane::ClipView);
        d.press(C::PaneTransport);
        assert_eq!(d.app.nav.focused_pane, Pane::Transport);
        d.press(C::Tab);
        assert_ne!(d.app.nav.focused_pane, Pane::Transport, "TAB did not move on");
        d.shift(C::Tab);
        assert_eq!(d.app.nav.focused_pane, Pane::Transport, "SHIFT+TAB did not come back");
    }

    /// Every kind of track, made from the deck: the menu and the arrows.
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

    #[test]
    fn strip_buttons_select_mute_solo_and_arm_their_own_rows() {
        let mut d = deck("strips");
        add_track(&mut d, InstrumentType::Synth);
        add_track(&mut d, InstrumentType::Rhodes);
        let scroll = d.app.nav.track_scroll;
        d.press(C::Track(1));
        assert_eq!(d.app.nav.track_cursor, scroll + 1, "TRK 2 did not select row 2");
        assert_eq!(d.app.nav.focused_pane, Pane::Tracks);
        d.press(C::Mute(0));
        assert!(d.app.nav.tracks[scroll].muted, "M 1 did not mute row 1");
        assert_eq!(d.app.nav.track_cursor, scroll + 1, "M 1 moved the selection");
        // A new track can arrive armed; each button flips what is there.
        let (soloed, armed) = (d.app.nav.tracks[scroll].soloed, d.app.nav.tracks[scroll].armed);
        d.press(C::Solo(0)).press(C::Arm(0));
        assert_ne!(d.app.nav.tracks[scroll].soloed, soloed, "S 1 did not toggle solo");
        assert_ne!(d.app.nav.tracks[scroll].armed, armed, "R 1 did not toggle arm");
        d.press(C::Undo);
        assert!(!d.app.nav.tracks[scroll].muted, "UNDO did not take the mute back");
        d.press(C::Redo);
        assert!(d.app.nav.tracks[scroll].muted, "REDO did not put it back");
    }

    /// A fader moves nothing until it reaches the track's level, then rides
    /// it — and a whole ride is one UNDO.
    #[test]
    fn a_fader_catches_the_level_and_one_undo_takes_the_ride_back() {
        let mut d = deck("fader");
        add_track(&mut d, InstrumentType::Synth);
        let row = d.app.nav.track_scroll;
        let start = d.app.nav.tracks[row].volume;
        d.slide(0, 0, 30);
        assert_eq!(d.app.nav.tracks[row].volume, start, "the fader jumped the level before catching it");
        d.slide(0, 30, 127);
        let top = d.app.nav.tracks[row].volume;
        assert!(top > start, "riding past the level did not take it up");
        d.slide(0, 127, 1);
        let bottom = d.app.nav.tracks[row].volume_db().unwrap();
        assert!((bottom + 40.0).abs() < 0.6, "the bottom of travel is {bottom} dB, not the floor");
        d.slide(0, 1, 0);
        assert_eq!(d.app.nav.tracks[row].volume_db(), None, "the very bottom is not silence");
        d.press(C::Undo);
        assert!((d.app.nav.tracks[row].volume - start).abs() < 0.03, "one UNDO did not take the ride back");
    }

    #[test]
    fn the_master_fader_rides_the_master() {
        let mut d = deck("master");
        let master = d.app.nav.tracks.iter().position(|t| t.kind == phosphor_core::project::TrackKind::Master).unwrap();
        let start = d.app.nav.tracks[master].volume;
        d.slide(5, 0, 127);
        assert!(d.app.nav.tracks[master].volume > start, "the master fader did not move the master");
    }

    /// VALUE turns the control under the cursor, SHIFT for strides, and
    /// the turn is undoable like a key press.
    #[test]
    fn value_turns_the_panel_control_under_the_cursor() {
        let mut d = deck("value");
        let idx = add_track(&mut d, InstrumentType::Juno60);
        d.press(C::PaneClip);
        d.tap(C::Down, 3);
        let cursor = d.app.nav.clip_view.synth_param_cursor;
        let before = d.app.nav.tracks[idx].synth_params[cursor];
        d.turn(C::Value, 4);
        let after = d.app.nav.tracks[idx].synth_params[cursor];
        assert!(after > before, "VALUE did not turn control {cursor}: {before} -> {after}");
        d.turn(C::Value, -2);
        assert!(d.app.nav.tracks[idx].synth_params[cursor] < after);
        d.press(C::Undo);
        assert!((d.app.nav.tracks[idx].synth_params[cursor] - before).abs() < 1e-6, "UNDO did not take the turn back");
    }

    /// The pads in STEP mode write a beat on the selected lane; LANE picks
    /// another; SHIFT+LANE queues a pattern; UNDO takes a step back.
    #[test]
    fn step_pads_write_a_beat() {
        let mut d = deck("steps");
        let idx = add_track(&mut d, InstrumentType::Sequencer);
        d.press(C::PaneClip);
        for pad in [0, 4, 8, 12] {
            d.hit(pad, 100);
        }
        let lane = |d: &DeckSim, lane: usize| -> Vec<usize> {
            let state = d.app.nav.tracks[idx].sequencer.as_ref().unwrap();
            state.pattern().lanes[lane].steps.iter().enumerate().filter(|(_, s)| s.on).map(|(i, _)| i).collect()
        };
        assert_eq!(lane(&d, 0), [0, 4, 8, 12], "four on the floor did not land");
        d.hit(4, 100);
        assert_eq!(lane(&d, 0), [0, 8, 12], "a second hit did not take the step off");
        d.press(C::Lane(1)).hit(2, 100);
        assert_eq!(lane(&d, 1), [2], "LANE 2 did not move the pads to lane 2");
        d.press(C::Accent);
        assert!(d.app.nav.tracks[idx].sequencer.as_ref().unwrap().pattern().lanes[1].steps[2].accent);
        d.press(C::Undo);
        assert!(!d.app.nav.tracks[idx].sequencer.as_ref().unwrap().pattern().lanes[1].steps[2].accent, "UNDO kept the accent");
        d.shift(C::Lane(1));
        assert_eq!(d.app.nav.tracks[idx].sequencer.as_ref().unwrap().queued_slot(), Some(1), "SHIFT+LANE 2 did not queue pattern B");
    }

    #[test]
    fn step_pads_on_a_track_without_a_sequencer_say_so() {
        let mut d = deck("nosteps");
        add_track(&mut d, InstrumentType::Synth);
        d.hit(0, 100);
        assert!(d.flash().contains("has none"), "{}", d.flash());
    }

    /// COPY, walk two keys, PASTE: a pad copied from the deck. Loading the
    /// first sound is setup — `a` is a listed gap.
    #[test]
    fn a_sampler_pad_copies_from_the_deck() {
        let dir = scratch("copy");
        let mut d = DeckSim::new(&dir);
        let idx = add_track(&mut d, InstrumentType::Sampler);
        d.press(C::PaneClip);
        // Setup outside the deck: a sound on the pad under the caret.
        let pcm = std::sync::Arc::new(phosphor_plugin::sample::SamplePcm { data: vec![0.3; 4_410], channels: 1, sample_rate: 44_100.0 });
        let kit = d.app.nav.tracks[idx].sampler.as_mut().unwrap();
        let from = kit.cursor;
        kit.add_wav_layer(from, PathBuf::from("kick.wav"), pcm).unwrap();

        d.press(C::Copy).tap(C::Right, 2).press(C::Paste);
        let kit = d.app.nav.tracks[idx].sampler.as_ref().unwrap();
        assert_eq!(kit.pads[from + 2], kit.pads[from], "PASTE did not copy the pad two keys along");
        d.press(C::Undo);
        assert!(d.app.nav.tracks[idx].sampler.as_ref().unwrap().pads[from + 2].layers.is_empty(), "UNDO left the paste");
        d.press(C::Redo);
        assert_eq!(d.app.nav.tracks[idx].sampler.as_ref().unwrap().pads[from + 2].layers.len(), 1, "REDO lost it");
    }

    /// A question the app asks can be answered from the deck: BACK is no,
    /// and COPY — which sends `y` — is yes. (ENTER is not; see the gap list.)
    #[test]
    fn a_question_is_answered_from_the_deck() {
        let mut d = deck("confirm");
        add_track(&mut d, InstrumentType::Synth);
        let count = d.app.nav.tracks.len();
        d.press(C::Track(0));
        menu(&mut d, "delete");
        assert!(d.app.nav.confirm_modal.open, "delete did not ask");
        d.press(C::Back);
        assert!(!d.app.nav.confirm_modal.open);
        assert_eq!(d.app.nav.tracks.len(), count, "BACK deleted the track anyway");
        menu(&mut d, "delete");
        d.press(C::Copy);
        assert_eq!(d.app.nav.tracks.len(), count - 1, "COPY did not answer yes");
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
        menu(&mut d, "quantize");
        d.tap(C::Back, 2);
        menu(&mut d, "help");
        assert!(d.app.nav.space_menu.open, "help did not open");
        d.tap(C::Back, 2);
        assert!(!d.app.nav.space_menu.open);
    }

    /// The controls the app has nothing behind yet say so in words.
    #[test]
    fn unbuilt_controls_say_what_they_are_waiting_for() {
        let mut d = deck("unbuilt");
        d.turn(C::Bank(0), 1);
        assert!(d.flash().contains("ENC 1: not built yet"), "{}", d.flash());
        d.press(C::Count);
        assert!(d.flash().contains("COUNT: not built yet"), "{}", d.flash());
        d.press(C::PageNext);
        assert!(d.flash().contains("not built yet"), "{}", d.flash());
        d.press(C::Tempo);
        assert!(d.flash().contains("push does nothing yet"), "{}", d.flash());
    }

    /// The keyboard the deck is clamped to still plays through: its notes are
    /// not the deck's and go on their way — here, moving a sampler's caret.
    #[test]
    fn the_keyboard_still_plays_through_the_deck() {
        let mut d = deck("keyboard");
        let idx = add_track(&mut d, InstrumentType::Sampler);
        d.press(C::PaneClip);
        d.play(72, 100);
        let kit = d.app.nav.tracks[idx].sampler.as_ref().unwrap();
        assert_eq!(phosphor_app::sampler::SamplerState::note_of_pad(kit.cursor), 72, "a keyboard note was taken for the deck");
    }

    /// SHIFT is a held modifier, not a latch: let go and the next press is
    /// plain again.
    #[test]
    fn shift_lets_go_when_it_is_let_go() {
        let mut d = deck("shift");
        d.shift(C::Tab);
        assert!(!d.app.deck.shift, "SHIFT stuck down");
        d.press(C::PaneTransport).press(C::Tab);
        assert_ne!(d.app.nav.focused_pane, Pane::Transport);
    }
}
