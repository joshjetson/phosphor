//! Integration tests for the track fader.
//!
//! The fader was navigable and drawn but had no key handler behind it, so
//! the only level control in the application was the operating system's.
//! These drive it the way a user does — through `handle_event` — rather than
//! calling the state methods directly, because the gap was in the key
//! routing rather than in the state.

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use crate::app::App;
    use crate::state::*;
    use phosphor_core::project::TrackConfig;
    use phosphor_core::EngineConfig;

    fn app() -> App {
        App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false)
    }

    fn add_synth_track(app: &mut App) {
        app.nav.instrument_modal.open = true;
        app.nav.instrument_modal.cursor = 0; // Phosphor Synth
        let instrument = app.nav.instrument_modal.selected();
        app.nav.instrument_modal.open = false;
        app.create_instrument_track(instrument);
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
    }

    /// Put the cursor on the fader of a fresh instrument track and lock it.
    fn app_with_locked_fader() -> App {
        let mut app = app();
        add_synth_track(&mut app);
        app.nav.focus_pane(Pane::Tracks);
        press(&mut app, KeyCode::Enter); // select the track
        press(&mut app, KeyCode::Char('l')); // Label -> Fx
        press(&mut app, KeyCode::Char('l')); // Fx -> Volume
        assert_eq!(app.nav.track_element, TrackElement::Volume);
        press(&mut app, KeyCode::Enter); // lock the fader
        assert!(app.nav.element_locked, "Enter did not lock the fader");
        app
    }

    fn volume(app: &App) -> f32 {
        app.nav.tracks[app.nav.track_cursor].volume
    }

    fn audio_volume(app: &App) -> f32 {
        app.nav.tracks[app.nav.track_cursor]
            .handle
            .as_ref()
            .expect("instrument track should have an audio handle")
            .config
            .get_volume()
    }

    /// The defect: `l` on the fader used to step to the next element and
    /// leave the level alone. It has to change the level, and it has to
    /// change it on the audio thread, not only in the UI mirror.
    #[test]
    fn l_raises_the_fader_and_h_lowers_it() {
        let mut app = app_with_locked_fader();
        let start = volume(&app);

        press(&mut app, KeyCode::Char('l'));
        let raised = volume(&app);
        assert!(raised > start, "l did not raise the fader: {start} -> {raised}");
        assert_eq!(audio_volume(&app), raised, "the audio thread did not get it");

        press(&mut app, KeyCode::Char('h'));
        press(&mut app, KeyCode::Char('h'));
        let lowered = volume(&app);
        assert!(lowered < raised, "h did not lower the fader: {raised} -> {lowered}");
        assert_eq!(audio_volume(&app), lowered);
    }

    /// Arrow keys do the same thing as h/l, as they do everywhere else.
    #[test]
    fn arrow_keys_move_the_fader_too() {
        let mut app = app_with_locked_fader();
        let start = volume(&app);
        press(&mut app, KeyCode::Right);
        assert!(volume(&app) > start);
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Left);
        assert!(volume(&app) < start);
    }

    /// Above unity is the makeup gain the range was widened for: +6 dB of it,
    /// and not a decibel more however long the key is held.
    #[test]
    fn the_fader_reaches_plus_six_db_and_stops() {
        let mut app = app_with_locked_fader();
        for _ in 0..60 {
            press(&mut app, KeyCode::Char('l'));
        }
        assert_eq!(volume(&app), TrackConfig::MAX_VOLUME);
        assert_eq!(audio_volume(&app), TrackConfig::MAX_VOLUME);
        assert!(volume(&app) > TrackConfig::UNITY_VOLUME, "no makeup gain available");
    }

    /// And silence at the other end, without going negative.
    #[test]
    fn the_fader_reaches_silence_and_stops() {
        let mut app = app_with_locked_fader();
        for _ in 0..60 {
            press(&mut app, KeyCode::Char('h'));
        }
        assert_eq!(volume(&app), TrackConfig::MIN_VOLUME);
        assert_eq!(audio_volume(&app), TrackConfig::MIN_VOLUME);
    }

    /// Esc releases the lock and h/l goes back to stepping between elements —
    /// otherwise the fader is a trap the user cannot navigate out of.
    #[test]
    fn esc_releases_the_fader_and_navigation_resumes() {
        let mut app = app_with_locked_fader();
        press(&mut app, KeyCode::Esc);
        assert!(!app.nav.element_locked);
        assert_eq!(app.nav.track_element, TrackElement::Volume);

        let held = volume(&app);
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.nav.track_element, TrackElement::Mute, "h/l did not resume navigating");
        assert_eq!(volume(&app), held, "navigating moved the fader");
    }

    /// Enter releases too, matching the transport's BPM field.
    #[test]
    fn enter_releases_the_fader() {
        let mut app = app_with_locked_fader();
        press(&mut app, KeyCode::Enter);
        assert!(!app.nav.element_locked);
    }

    /// `u` is undo everywhere else, so while the fader is locked it must not
    /// silently unwind the last clip edit instead of doing nothing.
    #[test]
    fn undo_is_inert_while_the_fader_is_locked() {
        let mut app = app_with_locked_fader();
        let before = volume(&app);
        press(&mut app, KeyCode::Char('u'));
        assert!(app.nav.element_locked, "u released the fader");
        assert_eq!(volume(&app), before);
    }

    /// A fader position survives a save/load round trip and lands on the
    /// audio thread when the session comes back.
    #[test]
    fn fader_position_survives_save_and_load() {
        let dir = std::env::temp_dir().join(format!("phosphor-fader-{}", std::process::id()));
        let path = dir.join("fader.phos");

        let mut saving = app_with_locked_fader();
        for _ in 0..5 {
            press(&mut saving, KeyCode::Char('l'));
        }
        let saved = volume(&saving);
        assert!(saved > TrackConfig::DEFAULT_VOLUME);
        saving.do_save(&path.to_string_lossy());

        let mut reopened = app();
        reopened.do_load(&path.to_string_lossy());
        let track = reopened
            .nav
            .tracks
            .iter()
            .find(|t| t.instrument_type.is_some())
            .expect("session should have restored the instrument track");
        assert!(
            (track.volume - saved).abs() < 1.0e-6,
            "fader came back as {} instead of {saved}",
            track.volume
        );
        assert_eq!(
            track.handle.as_ref().unwrap().config.get_volume(),
            track.volume,
            "restored fader was not pushed to the audio thread"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
