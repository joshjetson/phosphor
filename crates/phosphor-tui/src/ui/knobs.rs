//! A panel of knobs, in the house style: `" label ◔ value "`, wrapped to the
//! width it has, under a heading.
//!
//! The step grid drew this first and the pad map draws the same thing, so it
//! lives here rather than in either: two panels of knobs that disagree about
//! what a held control looks like are two panels a player has to learn
//! separately. What a knob *says* is still the caller's — this knows a
//! label, a value and where the dial points, and nothing about patterns or
//! pads.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme;

/// The dial. Five positions is what one cell can say honestly, and the value
/// is printed beside it, so the glyph is for reading the panel at a glance
/// rather than for reading the number off.
pub(super) fn knob_char(frac: f64) -> char {
    const RAMP: [char; 5] = ['\u{25CB}', '\u{25D4}', '\u{25D1}', '\u{25D5}', '\u{25CF}'];
    let index = (frac.clamp(0.0, 1.0) * 4.0).round() as usize;
    RAMP[index.min(4)]
}

/// One control on a panel.
pub(super) struct Knob {
    pub label: &'static str,
    pub value: String,
    /// Where the dial is pointing, 0..=1.
    pub frac: f64,
}

impl Knob {
    pub fn new(label: &'static str, value: impl Into<String>, frac: f64) -> Self {
        Self { label, value: value.into(), frac }
    }

    /// One of a list, by position — how a discrete control reads.
    pub fn at(label: &'static str, value: impl Into<String>, index: usize, count: usize) -> Self {
        let frac = if count > 1 { index as f64 / (count - 1) as f64 } else { 0.0 };
        Self::new(label, value, frac)
    }

    pub fn toggle(label: &'static str, on: bool) -> Self {
        Self::new(label, if on { "on" } else { "off" }, if on { 1.0 } else { 0.0 })
    }

    /// What [`Panel::spans`] will draw: `" label ◔ value "`.
    ///
    /// Counted rather than measured, and the count has to be exact — a knob
    /// that is one cell wider than the wrapper thinks runs off the right of
    /// the panel, where a `Paragraph` cuts it in half.
    pub fn width(&self) -> usize {
        self.label.chars().count() + self.value.chars().count() + 5
    }
}

/// Everything the drawing needs to know that is not a knob: where the cursor
/// is, whether the view has the keyboard, and how much room there is.
pub(super) struct Panel {
    /// Which control the cursor is standing on. Followed by the window even
    /// when the cursor is somewhere else entirely, so that a panel too tall
    /// for its space does not jump the moment it gets the keys.
    pub cursor: usize,
    /// Whether the cursor is in *this* panel rather than another band.
    pub active: bool,
    /// Enter was pressed on the control under the cursor.
    pub locked: bool,
    /// The view has the keyboard at all.
    pub focused: bool,
    /// The track's colour, which the dials take when they are not the one
    /// being turned.
    pub colour: Color,
    /// The width to wrap at.
    pub width: usize,
    /// The column the knobs start in, under the heading.
    pub indent: usize,
}

impl Panel {
    /// The styles a knob's parts take, given where the cursor is.
    pub fn spans(&self, knob: &Knob, index: usize) -> Vec<Span<'static>> {
        let selected = self.active && self.cursor == index;
        let locked = selected && self.locked;

        let label_style = if selected {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else if self.focused {
            theme::muted()
        } else {
            theme::dim()
        };
        let dial_style = if selected {
            Style::default().fg(theme::amber_bright_val()).bg(theme::bg_val())
        } else {
            Style::default().fg(self.colour).bg(theme::bg_val())
        };
        // Locked reads as inverse video in the theme's own colours rather than
        // as a colour of its own: every palette has a background and an amber,
        // and swapping them is legible in all nine.
        let value_style = if locked {
            Style::default()
                .fg(theme::bg_val())
                .bg(theme::amber_bright_val())
                .add_modifier(Modifier::BOLD)
        } else if selected {
            theme::amber_bright()
        } else if self.focused {
            theme::normal()
        } else {
            theme::dim()
        };

        vec![
            Span::styled(format!(" {} ", knob.label), label_style),
            Span::styled(knob_char(knob.frac).to_string(), dial_style),
            Span::styled(format!(" {}", knob.value), value_style),
            Span::styled(" ", theme::bg()),
        ]
    }

    /// A panel of knobs wrapped to the width it has, under a title.
    ///
    /// Answers which of the rows the cursor ended up on, so that a panel too
    /// tall for the space it has can be scrolled to the row being used rather
    /// than cut off at the bottom — see [`window`].
    pub fn rows(&self, title: &str, knobs: &[Knob]) -> (Vec<Line<'static>>, usize) {
        let heading = if self.active {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else if self.focused {
            theme::muted()
        } else {
            theme::dim()
        };

        let mut rows: Vec<Line> = Vec::new();
        let mut cursor_row = 0;
        let indent = self.indent.max(1);
        let mut spans: Vec<Span> =
            vec![Span::styled(format!("{:>w$} ", title, w = indent - 1), heading)];
        let mut used = indent;

        for (index, knob) in knobs.iter().enumerate() {
            if used + knob.width() > self.width && used > indent {
                rows.push(Line::from(std::mem::take(&mut spans)));
                spans.push(Span::styled(" ".repeat(indent), theme::bg()));
                used = indent;
            }
            if index == self.cursor {
                cursor_row = rows.len();
            }
            used += knob.width();
            spans.extend(self.spans(knob, index));
        }
        rows.push(Line::from(spans));
        (rows, cursor_row)
    }
}

/// `count` rows of a panel, chosen so that the row the cursor is on is one of
/// them. A knob under the cursor and off the bottom of the screen is a
/// control that answers keys nobody can see.
pub(super) fn window(
    rows: Vec<Line<'static>>,
    count: usize,
    cursor_row: usize,
) -> Vec<Line<'static>> {
    if count == 0 {
        return Vec::new();
    }
    if count >= rows.len() {
        return rows;
    }
    let start = (cursor_row + 1).saturating_sub(count).min(rows.len() - count);
    rows.into_iter().skip(start).take(count).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(width: usize) -> Panel {
        Panel {
            cursor: 0,
            active: true,
            locked: false,
            focused: true,
            colour: theme::track_color(0),
            width,
            indent: 8,
        }
    }

    /// The dial reads its whole travel, and anything a float can be does not
    /// take it off the end of the ramp.
    #[test]
    fn the_dial_covers_its_travel_and_survives_nonsense() {
        assert_eq!(knob_char(0.0), '\u{25CB}');
        assert_eq!(knob_char(1.0), '\u{25CF}');
        assert_ne!(knob_char(0.5), knob_char(0.0));
        for frac in [-1.0, 2.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let _ = knob_char(frac);
        }
    }

    /// The defect this catches, exactly: a knob whose declared width is one
    /// cell short of what it draws wraps a column too late and gets cut in
    /// half by the right edge of the panel.
    #[test]
    fn a_knob_is_as_wide_as_it_says_it_is() {
        let panel = panel(60);
        for knob in [
            Knob::new("pitch", "+12 st", 0.5),
            Knob::toggle("keytrk", true),
            Knob::at("trig", "one-shot", 0, 2),
            Knob::new("", "", 0.0),
        ] {
            let cells: usize =
                panel.spans(&knob, 0).iter().map(|s| s.content.chars().count()).sum();
            assert_eq!(cells, knob.width(), "{} draws {cells} cells", knob.label);
        }
    }

    /// A wrapped panel never lays a row wider than the width it was given,
    /// however narrow that is — including narrower than one knob, where the
    /// only honest answer is one knob per row.
    #[test]
    fn rows_never_run_past_the_right_edge() {
        let knobs: Vec<Knob> = (0..6).map(|_| Knob::new("release", "1.50 s", 0.4)).collect();
        for width in [12usize, 20, 30, 60, 200] {
            let panel = panel(width);
            let (rows, _) = panel.rows("pad", &knobs);
            let longest = rows
                .iter()
                .map(|r| r.spans.iter().map(|s| s.content.chars().count()).sum::<usize>())
                .max()
                .unwrap_or(0);
            let one = panel.indent + knobs[0].width();
            assert!(
                longest <= width.max(one),
                "a {longest}-cell row in {width} columns",
            );
        }
    }

    /// A panel taller than its space shows the row the cursor is on,
    /// wherever in the panel that row is.
    #[test]
    fn a_windowed_panel_keeps_the_cursor_on_the_screen() {
        let rows: Vec<Line<'static>> = (0..5)
            .map(|index| Line::from(Span::raw(format!("row {index}"))))
            .collect();
        for cursor in 0..5 {
            let shown = window(rows.clone(), 2, cursor);
            assert_eq!(shown.len(), 2);
            let text: Vec<String> =
                shown.iter().map(|line| line.spans[0].content.to_string()).collect();
            assert!(
                text.contains(&format!("row {cursor}")),
                "row {cursor} fell off the screen: {text:?}",
            );
        }
        assert_eq!(window(rows.clone(), 9, 0).len(), 5, "a panel that fits is not windowed");
        assert!(window(rows, 0, 0).is_empty());
    }
}
