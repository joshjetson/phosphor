//! The crates.io update notice, in the running UI.
//!
//! The comparison and the index parse are proven pure in [`crate::update`];
//! these prove the notice a player actually meets — that the bottom bar draws
//! it, and that Esc waves it away only where Esc is otherwise idle, never
//! stealing the Esc that backs out of a mode. Nothing here spawns the check or
//! touches the network: the notice is placed directly on the navigation state,
//! which is exactly what the background thread would do.

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use phosphor_core::EngineConfig;

    use crate::app::App;
    use crate::state::Pane;

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

    /// The notice names the version and the one command that updates — the
    /// whole point of it, since a player who does not know the command cannot
    /// act on the news.
    #[test]
    fn the_notice_names_the_version_and_the_install_command() {
        let mut a = app();
        a.nav.update_available = Some("0.9.9".to_string());
        let text = screen(&a, 120, 40);
        assert!(text.contains("v0.9.9 available"), "no version:\n{text}");
        assert!(
            text.contains("cargo install phosphor-studio --locked"),
            "no install command:\n{text}"
        );
    }

    /// Esc at the plain track list — where Esc otherwise does nothing — waves
    /// the notice away, and it stays away.
    #[test]
    fn esc_at_the_track_list_dismisses_the_notice() {
        let mut a = app();
        a.nav.update_available = Some("0.9.9".to_string());
        assert!(a.nav.update_notice().is_some());

        press(&mut a, KeyCode::Esc);
        assert!(a.nav.update_dismissed, "esc did not dismiss the notice");
        assert!(a.nav.update_notice().is_none(), "the notice should be gone");

        let text = screen(&a, 120, 40);
        assert!(!text.contains("v0.9.9 available"), "the notice is still drawn:\n{text}");
    }

    /// An Esc meant for backing out of something must not be spent on the
    /// notice. With a track selected, Esc deselects — the notice must survive
    /// that, so a player does not lose the news to a reflexive Esc.
    #[test]
    fn a_backing_out_esc_does_not_steal_the_notice() {
        let mut a = app();
        a.nav.focus_pane(Pane::Tracks);
        a.nav.track_selected = true;
        a.nav.update_available = Some("0.9.9".to_string());

        press(&mut a, KeyCode::Esc);
        assert!(!a.nav.update_dismissed, "esc was stolen from backing out");
        assert!(a.nav.update_notice().is_some(), "the notice should still be up");
        assert!(!a.nav.track_selected, "esc should have deselected the track");
    }

    /// A dismissed notice does not come back on its own, even though the shared
    /// slot still holds the version — the dismissal is what the UI reads, not
    /// the presence of a newer version.
    #[test]
    fn a_dismissed_notice_stays_dismissed() {
        let mut a = app();
        a.nav.update_available = Some("0.9.9".to_string());
        press(&mut a, KeyCode::Esc);
        assert!(a.nav.update_notice().is_none());
        // The version is still known; only the dismissal hides it.
        assert_eq!(a.nav.update_available.as_deref(), Some("0.9.9"));
    }
}
