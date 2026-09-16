//! The keyboard band: one drawn keyboard, two rooms.
//!
//! The practice room paints fingering on it and the sampler paints its pads,
//! so the geometry lives here once — which keys are black, where a black key
//! sits over the white one below it, how many whites fit in a width, and
//! which column a given key lands in. What a key *means* stays with the
//! caller: it answers with a colour and, if it wants one, a glyph for the
//! key's face. Nothing about targets or pads reaches this module.
//!
//! Five rows, because that is the fewest that reads as a keyboard: the upper
//! three carry the black keys over the white ones' shoulders, the lower two
//! are white key only. A white key is two cells wide, which is the narrowest
//! a key can be and still carry a character with air beside it.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme;

/// How wide a white key is drawn.
pub(super) const KEY_W: usize = 2;

/// The margin to the left of the band, in cells. Matched on the right so a
/// band that fills its space still has air at both ends.
pub(super) const MARGIN: usize = 2;

/// How many rows a band occupies.
pub(super) const ROWS: usize = 5;

/// The row a white key's glyph is written on — the bottom, where a real
/// keyboard has the part of the key a finger lands on.
const WHITE_GLYPH_ROW: usize = 4;

/// The row a black key's glyph is written on: the middle of the three rows
/// it occupies.
const BLACK_GLYPH_ROW: usize = 1;

pub(super) fn is_black(note: u8) -> bool {
    matches!(note % 12, 1 | 3 | 6 | 8 | 10)
}

/// White-key index of a note counted from `lo`, white keys only.
pub(super) fn white_index(lo: u8, note: u8) -> usize {
    (lo..note).filter(|n| !is_black(*n)).count()
}

/// The white key `index` places above `lo`, clamped to the top of the MIDI
/// range so a runaway count cannot spin.
fn white_at(lo: u8, index: usize) -> u8 {
    let mut note = lo;
    let mut seen = 0;
    while note < 127 {
        if !is_black(note) {
            if seen == index {
                return note;
            }
            seen += 1;
        }
        note += 1;
    }
    note
}

/// Whether a black key is drawn on the right half of `white`'s cell.
fn has_black_after(white: u8, hi: u8) -> bool {
    let black = white.saturating_add(1);
    black <= hi && is_black(black)
}

/// How many white keys a band of `width` cells can show.
pub(super) fn whites_fitting(width: usize) -> usize {
    width.saturating_sub(MARGIN * 2) / KEY_W
}

/// Where a key's cell starts, in cells from the left edge of the band.
///
/// A black key is drawn on the right half of the cell belonging to the white
/// key below it, which is where it sits on a real keyboard and what makes a
/// caret over one point at the right key.
pub(super) fn column_of(lo: u8, note: u8) -> usize {
    if is_black(note) {
        MARGIN + white_index(lo, note.saturating_sub(1)) * KEY_W + 1
    } else {
        MARGIN + white_index(lo, note) * KEY_W
    }
}

/// The lowest key of a window `width` cells wide that keeps `cursor` in
/// sight.
///
/// Stepping is in whole white keys, so a band that scrolls never shifts by
/// half a key — a keyboard whose C moves by one cell stops reading as a
/// keyboard. The cursor is centred rather than nudged to the edge, so the
/// keys either side of it are visible for the same reason a piano roll
/// scrolls before the cursor reaches the frame.
pub(super) fn window_lo(lo: u8, hi: u8, width: usize, cursor: u8) -> u8 {
    let total = white_index(lo, hi) + 1;
    let shown = whites_fitting(width).min(total);
    if shown == 0 || shown >= total {
        return lo;
    }
    let anchor = if is_black(cursor) { cursor.saturating_sub(1) } else { cursor };
    let start = white_index(lo, anchor)
        .saturating_sub(shown / 2)
        .min(total - shown);
    white_at(lo, start)
}

/// How a key is painted this frame.
#[derive(Clone, Copy)]
pub(super) struct KeyPaint {
    pub bg: Color,
    /// A character for the key's face — a finger number, a layer count.
    pub glyph: Option<char>,
    pub glyph_fg: Color,
}

impl KeyPaint {
    /// A key with nothing on it, in whichever of the two resting colours
    /// its kind takes.
    pub fn plain(note: u8) -> Self {
        Self {
            bg: if is_black(note) { black_rest() } else { theme::piano_white_bg() },
            glyph: None,
            glyph_fg: theme::normal_val(),
        }
    }

    pub fn lit(bg: Color) -> Self {
        Self { bg, glyph: None, glyph_fg: theme::bg_val() }
    }

    pub fn with_glyph(mut self, glyph: char, fg: Color) -> Self {
        self.glyph = Some(glyph);
        self.glyph_fg = fg;
        self
    }
}

/// The colour an untouched black key rests at: dark enough to read as the
/// gap between two white keys in every palette.
pub(super) fn black_rest() -> Color {
    theme::dim_color(theme::bg_val(), 40)
}

/// What a glyph is written in on a lit key.
///
/// The one flat colour in this module, and it earns it: a lit key is a
/// bright colour in every palette — amber, green, red, a track colour — and
/// a light character on any of them washes out. Near-black reads on all of
/// them.
pub(super) const INK: Color = Color::Rgb(12, 12, 12);

/// Draw the keys from `lo` to `hi` into [`ROWS`] lines, asking `paint` what
/// each one looks like.
pub(super) fn band(
    lo: u8,
    hi: u8,
    width: usize,
    paint: impl Fn(u8) -> KeyPaint,
) -> Vec<Line<'static>> {
    let white_total = white_index(lo, hi) + 1;
    let white_shown = whites_fitting(width).min(white_total);

    // The margin gives way before the keys do: a band drawn in fewer cells
    // than its own margin must still not run off the end.
    let margin = " ".repeat(MARGIN.min(width));
    let mut rows: Vec<Vec<Span<'static>>> =
        vec![vec![Span::styled(margin, theme::bg())]; ROWS];
    let mut white = lo;
    let mut shown = 0usize;
    while shown < white_shown && white <= hi {
        if is_black(white) {
            white += 1;
            continue;
        }
        let face = paint(white);
        let white_style = Style::default().bg(face.bg);
        for (row, spans) in rows.iter_mut().enumerate() {
            // The black key takes the right half of this cell, on the three
            // rows it reaches down; everywhere else the white key's body
            // fills both halves.
            let black = white + 1;
            if row < BLACK_GLYPH_ROW + 2 && has_black_after(white, hi) {
                let above = paint(black);
                spans.push(Span::styled(" ".to_string(), white_style));
                let glyph = (row == BLACK_GLYPH_ROW).then_some(above.glyph).flatten();
                spans.push(Span::styled(
                    glyph.map_or_else(|| " ".to_string(), |g| g.to_string()),
                    Style::default()
                        .bg(above.bg)
                        .fg(above.glyph_fg)
                        .add_modifier(Modifier::BOLD),
                ));
            } else if row == WHITE_GLYPH_ROW && face.glyph.is_some() {
                let glyph = face.glyph.unwrap_or(' ');
                spans.push(Span::styled(
                    format!("{glyph} "),
                    white_style.fg(face.glyph_fg).add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled("  ".to_string(), white_style));
            }
        }
        white += 1;
        shown += 1;
    }
    rows.into_iter().map(Line::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The band is exactly as wide as it says it is, and never wider than
    /// the space it was given — a row one cell too long is cut in half by
    /// the right edge of the pane it is drawn in.
    #[test]
    fn a_band_fits_the_width_it_was_given() {
        for width in [0, 1, 4, 5, 20, 41, 108, 200] {
            let rows = band(21, 108, width, KeyPaint::plain);
            assert_eq!(rows.len(), ROWS);
            for row in &rows {
                let cells: usize =
                    row.spans.iter().map(|s| s.content.chars().count()).sum();
                assert!(cells <= width, "{cells} cells in {width} columns");
                assert_eq!(
                    cells,
                    MARGIN.min(width) + whites_fitting(width).min(52) * KEY_W,
                    "the band drew a different number of keys than it counted",
                );
            }
        }
    }

    /// The caret over a key lands on the key: the column a note is drawn in
    /// is the column [`column_of`] names, black keys included.
    #[test]
    fn a_key_is_where_the_column_says_it_is() {
        // C3 is the 24th white key up from A0; C#3 shares its cell.
        assert_eq!(column_of(21, 60), MARGIN + 23 * KEY_W);
        assert_eq!(column_of(21, 61), MARGIN + 23 * KEY_W + 1);
        // E and B have no black key above them, so the next white starts
        // one cell along as usual.
        assert_eq!(column_of(21, 65) - column_of(21, 64), KEY_W);
    }

    /// A band too narrow for the bed scrolls in whole keys and keeps the
    /// cursor on the screen, at either end of the keyboard.
    #[test]
    fn the_window_keeps_the_cursor_in_sight() {
        let (lo, hi) = (21u8, 108u8);
        for width in [20usize, 40, 60, 90] {
            for cursor in [21u8, 22, 40, 60, 61, 100, 108] {
                let start = window_lo(lo, hi, width, cursor);
                assert!(!is_black(start), "the band starts on a black key");
                let shown = whites_fitting(width).min(white_index(lo, hi) + 1);
                let anchor = if is_black(cursor) { cursor - 1 } else { cursor };
                assert!(anchor >= start, "cursor {cursor} is left of the window");
                assert!(
                    white_index(start, anchor) < shown,
                    "cursor {cursor} is right of a {width}-wide window",
                );
            }
        }
        // A window with room for the whole bed does not scroll at all.
        assert_eq!(window_lo(lo, hi, 200, 108), lo);
    }
}
