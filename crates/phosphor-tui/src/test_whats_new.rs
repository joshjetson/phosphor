//! The "what's new" card: what it draws, that it takes the screen, and that
//! dismissing it records the version so it does not come back.
//!
//! The parsing and the decision are proven in `phosphor_app::whats_new`; these
//! drive the real key path and read the rendered buffer, because the card being
//! correct as a value and the card being reachable and dismissible in the
//! running UI are two different claims — and the second is the one a player
//! meets.
//!
//! Dismissing the card writes the last-seen version to the application
//! directory, so any test that dismisses must not touch the player's real
//! `~/.phosphor`. Those tests run under [`with_isolated_home`]: it points
//! `PHOSPHOR_HOME` at a scratch directory *and* holds a lock, because the
//! environment is process-global — two of these running at once would write
//! through each other's redirect. The tests that only show or draw the card
//! never reach the disk and need neither.

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use phosphor_core::EngineConfig;
    use phosphor_app::whats_new::Entry;

    use crate::app::App;

    /// Serializes every test that reads or writes the last-seen file. The only
    /// writer of that file anywhere in the suite is the card's own dismiss, so
    /// holding this lock across the `PHOSPHOR_HOME` redirect is enough to keep
    /// these tests from colliding — no other test writes there to race with.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    fn app() -> App {
        App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false)
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }));
    }

    fn screen(app: &App, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let snapshot = app.engine.transport.snapshot();
        let status = app.live_status();
        terminal
            .draw(|frame| crate::ui::render(frame, &snapshot, &app.nav, status))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn entry(version: &str, title: &str, body: &str) -> Entry {
        Entry { version: version.into(), title: title.into(), body: body.into() }
    }

    /// Run `f` with the application directory pointed at a scratch folder of
    /// this test's own, the environment restored afterward, and no other
    /// disk-touching card test running alongside.
    fn with_isolated_home<T>(name: &str, f: impl FnOnce() -> T) -> T {
        let _lock = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let var = phosphor_app::paths::OVERRIDE_VAR;
        let previous = std::env::var(var).ok();
        let dir = std::env::temp_dir()
            .join(format!("phosphor-whatsnew-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var(var, &dir);

        let result = f();

        match previous {
            Some(value) => std::env::set_var(var, value),
            None => std::env::remove_var(var),
        }
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    /// The card draws its title, the version, and the prose — the whole point of
    /// it, which is to say what changed in words the player can read. No disk:
    /// `show` fills the card directly, and drawing never touches a file.
    #[test]
    fn the_card_draws_the_version_title_and_body() {
        let mut a = app();
        a.nav.whats_new.show(
            vec![entry("0.3.85", "A mono trigger", "A pad gains a mono trigger mode.")],
            "0.3.85",
        );
        let text = screen(&a, 100, 40);
        assert!(text.contains("what's new"), "no title:\n{text}");
        assert!(text.contains("v0.3.85"), "no version:\n{text}");
        assert!(text.contains("A mono trigger"), "no title text:\n{text}");
        assert!(text.contains("mono trigger mode"), "no body prose:\n{text}");
    }

    /// A startup card is on the screen before the player has touched anything,
    /// so it must swallow every key but the ones that dismiss it — a stray `q`
    /// cannot quit the application out from under the notice. `q` does not
    /// dismiss, so nothing is written.
    #[test]
    fn the_card_swallows_keys_meant_for_the_session() {
        let mut a = app();
        a.nav.whats_new.show(vec![entry("0.3.85", "T", "B")], "0.3.85");
        press(&mut a, KeyCode::Char('q'));
        assert!(a.running, "q must not quit through an open card");
        assert!(a.nav.whats_new.open, "the card must still be up");
    }

    /// Esc closes the card and records the version, and a fresh launch on the
    /// same version then shows nothing — which is the whole "once" of "shown
    /// once after an update".
    #[test]
    fn dismissing_records_the_version_so_it_does_not_return() {
        with_isolated_home("dismiss", || {
            let running = env!("CARGO_PKG_VERSION");

            // Seed a last-seen a few versions back so the real changelog has news.
            phosphor_app::whats_new::write_last_seen("0.3.80");

            let mut a = app();
            a.show_whats_new_on_startup();
            assert!(a.nav.whats_new.open, "the card should be up with unseen versions");
            let text = screen(&a, 100, 40);
            assert!(
                text.contains(&format!("v{running}")),
                "the running version's entry should lead the card:\n{text}"
            );

            press(&mut a, KeyCode::Esc);
            assert!(!a.nav.whats_new.open, "Esc did not close the card");
            assert_eq!(
                phosphor_app::whats_new::read_last_seen().as_deref(),
                Some(running),
                "dismissing did not record the running version as seen"
            );

            // A second launch on the same version has nothing new to say.
            let mut again = app();
            again.show_whats_new_on_startup();
            assert!(!again.nav.whats_new.open, "the card returned after being seen");
        });
    }

    /// Enter dismisses as readily as Esc — the card is a notice, not a form,
    /// and both keys mean "I have read it". Isolated because Enter's dismiss
    /// writes the last-seen file.
    #[test]
    fn enter_also_dismisses() {
        with_isolated_home("enter", || {
            let mut a = app();
            a.nav.whats_new.show(vec![entry("0.3.85", "T", "B")], "0.3.85");
            press(&mut a, KeyCode::Enter);
            assert!(!a.nav.whats_new.open);
        });
    }
}
