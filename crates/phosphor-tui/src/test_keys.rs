//! What arrives at `handle_event`, and what it is allowed to act on.
//!
//! A Unix terminal reports a keystroke once, as a press. The Windows console
//! reports the key going down *and* coming back up, and crossterm hands both
//! to us as `KeyEventKind::Press` and `KeyEventKind::Release` — one physical
//! keystroke, two trips through every action in `handle_event`.
//!
//! The existing suites do not catch this: `test_presets` and `test_fader` both
//! build their events with `kind: KeyEventKind::Press` spelled out, so they
//! describe a Unix terminal and pass whether or not the release is filtered.
//! Everything here sends the pair a Windows console would send.

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use crate::app::App;
    use crate::state::Pane;
    use phosphor_core::EngineConfig;

    fn app() -> App {
        App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false)
    }

    fn send(app: &mut App, code: KeyCode, kind: KeyEventKind) {
        app.handle_event(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind,
            state: KeyEventState::NONE,
        }));
    }

    /// One physical keystroke as a Windows console reports it.
    fn keystroke(app: &mut App, code: KeyCode) {
        send(app, code, KeyEventKind::Press);
        send(app, code, KeyEventKind::Release);
    }

    /// The symptom that would make the application look broken rather than
    /// lossy: a toggle flipped by the press and flipped straight back by the
    /// release, so the control reads as dead.
    #[test]
    fn a_toggle_flips_once_per_keystroke() {
        let mut app = app();
        assert!(!app.nav.space_menu.open, "the menu starts closed");

        keystroke(&mut app, KeyCode::Char(' '));
        assert!(app.nav.space_menu.open, "Space opened the menu and the key release closed it again");

        keystroke(&mut app, KeyCode::Char(' '));
        assert!(!app.nav.space_menu.open, "the second keystroke did not close the menu");
    }

    /// A cursor that moves two rows per press is the same defect on a control
    /// where it is merely wrong rather than invisible.
    #[test]
    fn a_cursor_moves_one_step_per_keystroke() {
        let mut app = app();
        assert_eq!(app.nav.focused_pane, Pane::Tracks);
        assert!(app.nav.tracks.len() >= 3, "this test needs rows to move between");
        assert_eq!(app.nav.track_cursor, 0);

        keystroke(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.track_cursor, 1, "one keystroke moved the cursor more than one row");

        keystroke(&mut app, KeyCode::Char('j'));
        assert_eq!(app.nav.track_cursor, 2);
    }

    /// A release on its own changes nothing at all.
    #[test]
    fn a_release_on_its_own_does_nothing() {
        let mut app = app();
        send(&mut app, KeyCode::Char('j'), KeyEventKind::Release);
        assert_eq!(app.nav.track_cursor, 0, "a key coming back up moved the cursor");

        send(&mut app, KeyCode::Char(' '), KeyEventKind::Release);
        assert!(!app.nav.space_menu.open, "a key coming back up opened the menu");
    }

    /// A held key is how a knob gets swept, so a repeat is a press.
    ///
    /// Nothing produces `Repeat` as this application is built — crossterm
    /// reports it only for the kitty keyboard protocol, which needs
    /// `PushKeyboardEnhancementFlags` and this never sends it — so this pins
    /// the intent rather than a live path. It is the assertion that stops the
    /// release filter from being written as "only `Press`" and quietly
    /// breaking held keys the day the protocol is turned on.
    #[test]
    fn a_repeat_counts_as_a_press() {
        let mut app = app();
        send(&mut app, KeyCode::Char('j'), KeyEventKind::Press);
        send(&mut app, KeyCode::Char('j'), KeyEventKind::Repeat);
        assert_eq!(app.nav.track_cursor, 2, "a held key stopped moving the cursor");
    }

    /// Quitting is one of the actions a doubled keystroke would run twice, and
    /// the one where the second run happens after the state it read is gone.
    #[test]
    fn ctrl_c_quits_on_the_press_and_not_again_on_the_release() {
        let mut app = app();
        assert!(app.running);
        app.handle_event(Event::Key(KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        }));
        assert!(app.running, "the key coming back up quit the application");
    }
}
