//! What a journey test needs to play the app: a key, a shifted key, and
//! the screen as text.
//!
//! One copy. Several older suites carry their own; new suites use these.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

use crate::app::App;

pub(crate) fn press(app: &mut App, code: KeyCode) {
    app.handle_event(Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }));
}

/// A key with shift down, the way a terminal sends it: the uppercase
/// character *and* the modifier.
pub(crate) fn press_shift(app: &mut App, ch: char) {
    app.handle_event(Event::Key(KeyEvent {
        code: KeyCode::Char(ch),
        modifiers: KeyModifiers::SHIFT,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }));
}

/// Type a line into whatever has the keys, a character at a time.
pub(crate) fn type_line(app: &mut App, text: &str) {
    for ch in text.chars() {
        press(app, KeyCode::Char(ch));
    }
}

/// What the running application would be showing, as text.
pub(crate) fn screen(app: &App, width: u16, height: u16) -> String {
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
