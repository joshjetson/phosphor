//! UI rendering: the Phosphor Deck's line.
//!
//! One row above the bottom bar saying what is locked and what the deck's
//! eight knobs turn there. Drawn only once a deck has been used, so a
//! keyboard-only screen is the screen it always was.

use super::*;

pub(super) fn render_deck_strip(frame: &mut Frame, area: Rect, text: &str) {
    let line = Line::from(Span::styled(format!(" {text}"), theme::dim()));
    frame.render_widget(Paragraph::new(line), area);
}
