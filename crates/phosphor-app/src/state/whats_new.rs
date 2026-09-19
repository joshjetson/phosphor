//! The "what's new" card: the state behind the startup overlay.
//!
//! The parsing and the show/don't decision live in [`crate::whats_new`]; this
//! is only what the running UI holds while the card is on the screen — the
//! entries to show, where the reader has scrolled to, and the version to write
//! back to disk once they dismiss it.
//!
//! The body is prose, so it wraps to the width it is drawn in. That makes the
//! total number of lines a function of the terminal's width, which the scroll
//! clamp has to know — otherwise a reader can scroll a narrow card past its own
//! last line. So the flattening lives here, as [`WhatsNewCard::lines`], and both
//! the renderer and the per-frame [`WhatsNewCard::set_layout`] measure through
//! it: the keys and the drawing then agree about where the bottom is, the same
//! way the help card's [`super::help_box_height`] keeps them agreeing.

use crate::whats_new::Entry;

/// One rendered line of the card, before any styling is applied.
///
/// An enum rather than a styled string because the version headers, the prose,
/// and the gaps between entries are drawn differently, and that decision is the
/// renderer's — this side only says which kind each line is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhatsNewLine {
    /// A version and its title, opening an entry.
    Version { version: String, title: String },
    /// One wrapped line of body prose.
    Body(String),
    /// A blank line — between paragraphs, or between entries.
    Gap,
}

/// The tallest the card is allowed to grow, however tall the terminal is —
/// matched to the help card so the two overlays feel like one thing.
const CARD_MAX_ROWS: u16 = 24;

/// The outer width of the card for a given terminal width.
///
/// Wide enough to read prose, capped so a full-screen terminal does not stretch
/// a paragraph into an unreadable ribbon, and never wider than the terminal.
#[must_use]
pub fn card_box_width(term_width: u16) -> u16 {
    term_width.clamp(20, 72).min(term_width)
}

/// The width prose wraps to inside the card: the box less its border and the
/// one column of padding on each side.
#[must_use]
pub fn card_inner_width(term_width: u16) -> usize {
    card_box_width(term_width).saturating_sub(4) as usize
}

/// How tall the card is: as tall as its content needs, capped by
/// [`CARD_MAX_ROWS`] and by the terminal.
#[must_use]
pub fn card_box_height(term_height: u16, total_lines: usize) -> u16 {
    let wanted = u16::try_from(total_lines.saturating_add(3)).unwrap_or(CARD_MAX_ROWS);
    wanted.min(CARD_MAX_ROWS).min(term_height.saturating_sub(2)).max(4)
}

/// The lines of content that fit in that card: the two borders and the footer
/// are chrome, not content.
#[must_use]
pub fn card_page_rows(term_height: u16, total_lines: usize) -> usize {
    (card_box_height(term_height, total_lines) as usize).saturating_sub(3)
}

/// Everything the UI holds while the card is up.
#[derive(Debug, Default)]
pub struct WhatsNewCard {
    /// Whether the card is on the screen.
    pub open: bool,
    /// The entries to show, newest first. Empty when there is nothing new.
    pub entries: Vec<Entry>,
    /// The first content line on the screen.
    pub scroll: usize,
    /// How many content lines fit, set from the terminal each frame.
    pub page_rows: usize,
    /// The width prose is wrapped to, set from the terminal each frame. Held so
    /// that a scroll key clamps against the width the card is actually drawn at
    /// without the key handler having to know the terminal's geometry.
    pub inner_width: usize,
    /// The version to record as seen when the card is dismissed. Held here so
    /// the dismissing keypress does not have to re-derive what it is looking at.
    pub current_version: String,
}

impl WhatsNewCard {
    #[must_use]
    pub fn new() -> Self {
        Self { page_rows: 1, inner_width: 1, ..Self::default() }
    }

    /// Put the card up over `entries`, remembering `current_version` for the
    /// write-back on dismiss. An empty `entries` shows nothing.
    pub fn show(&mut self, entries: Vec<Entry>, current_version: impl Into<String>) {
        if entries.is_empty() {
            return;
        }
        self.entries = entries;
        self.current_version = current_version.into();
        self.scroll = 0;
        self.open = true;
    }

    /// Close the card, answering the version that should now be recorded as
    /// seen — `None` if it was not open, so a stray Esc writes nothing.
    pub fn dismiss(&mut self) -> Option<String> {
        if !self.open {
            return None;
        }
        self.open = false;
        Some(std::mem::take(&mut self.current_version))
    }

    /// The card's content, flattened and wrapped to `width`.
    ///
    /// Version headers open each entry; the body's own blank lines become gaps
    /// so paragraphs stay apart; a gap separates one entry from the next but
    /// none trails the last, so the card does not end on empty space.
    #[must_use]
    pub fn lines(&self, width: usize) -> Vec<WhatsNewLine> {
        let mut out = Vec::new();
        for (i, entry) in self.entries.iter().enumerate() {
            if i > 0 {
                out.push(WhatsNewLine::Gap);
            }
            out.push(WhatsNewLine::Version {
                version: entry.version.clone(),
                title: entry.title.clone(),
            });
            for paragraph_line in entry.body.split('\n') {
                if paragraph_line.trim().is_empty() {
                    out.push(WhatsNewLine::Gap);
                } else {
                    for wrapped in wrap(paragraph_line, width) {
                        out.push(WhatsNewLine::Body(wrapped));
                    }
                }
            }
        }
        out
    }

    /// The furthest the card can scroll: how much content is below the window.
    #[must_use]
    fn scroll_max(&self) -> usize {
        self.lines(self.inner_width).len().saturating_sub(self.page_rows.max(1))
    }

    /// Told the terminal's size each frame, so "the bottom" means the same to
    /// the keys and to the drawing. Records the wrap width so a scroll key can
    /// clamp against it, and re-clamps the scroll in case a resize shrank the
    /// content below where the reader had scrolled to.
    pub fn set_layout(&mut self, term_height: u16, width: usize) {
        self.inner_width = width.max(1);
        let total = self.lines(self.inner_width).len();
        self.page_rows = card_page_rows(term_height, total).max(1);
        self.scroll = self.scroll.min(self.scroll_max());
    }

    /// Move the window over the content, stopping at both ends. Clamps against
    /// the width last given to [`set_layout`], so a card that has been laid out
    /// scrolls exactly as far as it draws.
    pub fn scroll_body(&mut self, delta: i32) {
        let max = self.scroll_max() as i32;
        self.scroll = (self.scroll as i32 + delta).clamp(0, max) as usize;
    }
}

/// Greedy word wrap on whitespace. A word longer than `width` is left whole on
/// its own line rather than split mid-word, because a hyphenated break in the
/// middle of a knob name or a path reads as two things.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(version: &str, title: &str, body: &str) -> Entry {
        Entry { version: version.into(), title: title.into(), body: body.into() }
    }

    #[test]
    fn wrapping_breaks_on_words_and_keeps_them_whole() {
        let lines = wrap("the quick brown fox", 9);
        assert_eq!(lines, ["the quick", "brown fox"]);
    }

    #[test]
    fn a_word_longer_than_the_width_is_left_whole() {
        // Splitting a long path or knob name mid-word reads as two things; the
        // over-long word takes its own line instead.
        let lines = wrap("short supercalifragilistic ok", 6);
        assert_eq!(lines, ["short", "supercalifragilistic", "ok"]);
    }

    #[test]
    fn a_version_header_opens_each_entry() {
        let mut card = WhatsNewCard::new();
        card.show(vec![entry("0.3.85", "Title", "Body words here.")], "0.3.85");
        let lines = card.lines(40);
        assert_eq!(
            lines[0],
            WhatsNewLine::Version { version: "0.3.85".into(), title: "Title".into() }
        );
    }

    #[test]
    fn a_gap_separates_entries_but_none_trails_the_last() {
        let mut card = WhatsNewCard::new();
        card.show(
            vec![entry("0.3.85", "New", "One."), entry("0.3.84", "Old", "Two.")],
            "0.3.85",
        );
        let lines = card.lines(40);
        assert_ne!(lines.last(), Some(&WhatsNewLine::Gap), "the card must not end on empty space");
        // Exactly one gap, and it is between the two entries.
        let gaps = lines.iter().filter(|l| **l == WhatsNewLine::Gap).count();
        assert_eq!(gaps, 1);
    }

    #[test]
    fn a_blank_line_in_the_body_becomes_a_gap() {
        let mut card = WhatsNewCard::new();
        card.show(vec![entry("0.4.0", "Two paras", "First.\n\nSecond.")], "0.4.0");
        let lines = card.lines(40);
        assert!(lines.contains(&WhatsNewLine::Gap));
    }

    #[test]
    fn dismiss_answers_the_version_once_and_then_closes() {
        let mut card = WhatsNewCard::new();
        card.show(vec![entry("0.3.85", "T", "B")], "0.3.85");
        assert_eq!(card.dismiss(), Some("0.3.85".to_string()));
        assert!(!card.open);
        // A second dismiss writes nothing — the card is already down.
        assert_eq!(card.dismiss(), None);
    }

    #[test]
    fn showing_nothing_leaves_the_card_closed() {
        let mut card = WhatsNewCard::new();
        card.show(Vec::new(), "0.3.85");
        assert!(!card.open);
    }

    #[test]
    fn scroll_stops_at_both_ends() {
        let mut card = WhatsNewCard::new();
        // A body long enough to overflow a short window.
        let body = "line ".repeat(60);
        card.show(vec![entry("0.3.85", "Long", &body)], "0.3.85");
        card.set_layout(10, 40);
        // Cannot scroll above the top.
        card.scroll_body(-5);
        assert_eq!(card.scroll, 0);
        // Cannot scroll past the last line.
        card.scroll_body(9999);
        let total = card.lines(40).len();
        assert_eq!(card.scroll, total.saturating_sub(card.page_rows));
    }

    #[test]
    fn a_short_card_cannot_scroll_at_all() {
        let mut card = WhatsNewCard::new();
        card.show(vec![entry("0.3.85", "Short", "One short line.")], "0.3.85");
        card.set_layout(40, 40);
        card.scroll_body(50);
        assert_eq!(card.scroll, 0, "content that fits has nowhere to scroll");
    }
}
