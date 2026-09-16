//! Keys mode on the screen: the rule under the keyboard, and the zone list
//! that stands where the filled-pad list stands in pads mode.
//!
//! ```text
//!               ▼
//!   ▕ ▕▌▕▌▕ ▕▌▕▌▕▌▕ ▕ ▕▌▕▌▕ ▕▌▕▌▕▌▕
//!   ├──────┼───────────┤├────────────┤
//!
//!   zones · 2
//!  ▸ C2-B3   piano       1  root C3  24 keys
//!    C4-B4   strings     2  root C4  12 keys
//! ```
//!
//! The rule is the one thing keys mode adds to the band, and it is the
//! whole point of the mode: a zone is a brace on the keyboard, so it is
//! drawn as one, under the keys it holds, with a tick where its root is.
//! Two braces that touch are `┤├` — one cell each — because a gap between
//! them would say there is a key neither of them covers.

use super::*;

use super::keyboard::{column_of, is_black};

/// The brace's two ends, its body, and the tick that marks a root.
const LEFT: char = '\u{251C}';
const RIGHT: char = '\u{2524}';
const BODY: char = '\u{2500}';
const ROOT: char = '\u{253C}';

/// One row under the keyboard drawing every zone's span.
///
/// The cells are worked out from the same [`column_of`] the keys are drawn
/// with, so a brace is under the keys it holds on every terminal width and
/// at every scroll position — a rule that agreed with the keyboard only at
/// full width would be worse than none.
pub(super) fn rule_line(map: &Map, lo: u8, width: usize) -> Line<'static> {
    let mut owner: Vec<Option<usize>> = vec![None; width];
    let mut root: Vec<bool> = vec![false; width];
    for (index, zone) in map.state.zones.iter().enumerate() {
        for pad in zone.lo..=zone.hi.min(NUM_PADS - 1) {
            let note = SamplerState::note_of_pad(pad);
            if note < lo {
                continue;
            }
            let column = column_of(lo, note);
            if column >= width {
                break;
            }
            // A black key is the right half of the white one below it; a
            // white key with no black above it owns both halves. The same
            // rule the band itself draws by.
            owner[column].get_or_insert(index);
            if !is_black(note) && !(note < HIGH && is_black(note + 1)) && column + 1 < width {
                owner[column + 1].get_or_insert(index);
            }
            if zone.root() == note {
                root[column] = true;
            }
        }
    }

    let here = map.state.cursor_zone();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut column = 0usize;
    while column < width {
        let Some(index) = owner[column] else {
            // Air between zones, or a key no zone covers.
            let run = (column..width).take_while(|c| owner[*c].is_none()).count();
            spans.push(Span::styled(" ".repeat(run), theme::bg()));
            column += run;
            continue;
        };
        let run = (column..width).take_while(|c| owner[*c] == Some(index)).count();
        let text: String = (column..column + run)
            .map(|cell| {
                if root[cell] {
                    ROOT
                } else if cell == column {
                    LEFT
                } else if cell + 1 == column + run {
                    RIGHT
                } else {
                    BODY
                }
            })
            .collect();
        spans.push(Span::styled(text, zone_style(map, index, here)));
        column += run;
    }
    Line::from(spans)
}

/// What a zone is drawn in: the theme's amber for the one under the caret,
/// and the track's colour for the rest — dimmed on every other zone, so
/// that two that touch are still two.
fn zone_style(map: &Map, index: usize, here: Option<usize>) -> Style {
    if Some(index) == here {
        return if map.focused { theme::amber_bright() } else { theme::amber() };
    }
    let colour = if index % 2 == 0 { map.colour } else { theme::dim_color(map.colour, 55) };
    Style::default().fg(colour).bg(theme::bg_val())
}

/// The zone list: one row per zone, in keyboard order.
pub(super) fn zone_list(map: &Map, width: usize, height: usize) -> Vec<Line<'static>> {
    let zones = &map.state.zones;
    let mut head = Row::new(width);
    head.push("  zones", map.heading());
    head.push(
        match zones.len() {
            0 => " \u{00b7} none yet".to_string(),
            1 => " \u{00b7} 1".to_string(),
            n => format!(" \u{00b7} {n}"),
        },
        theme::dim(),
    );
    let mut lines = vec![head.line()];

    if zones.is_empty() {
        for line in [
            "  w covers the whole bed \u{00b7} o this octave",
            "  s splits the zone under the caret",
        ] {
            lines.push(Line::from(Span::styled(clip_text(line, width), theme::dim())));
        }
        return lines;
    }

    // Scroll to the zone under the caret, the pad list's rule: a zone being
    // edited off the bottom of its own list is a cursor nobody can see.
    let rows = height.saturating_sub(1);
    let here = map.state.cursor_zone().unwrap_or(0);
    let start = (here + 1).saturating_sub(rows).min(zones.len().saturating_sub(rows));
    for (index, zone) in zones.iter().enumerate().skip(start).take(rows.max(1)) {
        lines.push(zone_row(map, index, zone, width));
    }
    lines
}

fn zone_row(map: &Map, index: usize, zone: &Zone, width: usize) -> Line<'static> {
    let here = map.state.cursor_zone() == Some(index);
    let label_style = if here && map.focused {
        theme::amber_bright().add_modifier(Modifier::BOLD)
    } else if here {
        theme::amber()
    } else {
        theme::normal()
    };
    // The columns in the order they matter, each one taken only if it fits.
    // A narrow list loses the width before it loses the root, and the root
    // before it loses the name of the zone.
    let mut row = Row::new(width);
    row.push(if here { " \u{25B8} " } else { "   " }, label_style);
    row.push(format!("{:<8}", zone.span_label()), label_style);
    let name_w = row.left().saturating_sub(19).clamp(4, 14);
    row.push(
        format!("{:<w$}", clip_text(&zone.sound_label(), name_w), w = name_w),
        theme::normal(),
    );
    row.push(format!("{:>2} ", zone.pad.layers.len()), theme::dim());
    row.push(format!("root {:<4}", phosphor_app::format::note_name(zone.root())), theme::dim());
    row.push(format!("{:>2} keys", zone.keys()), theme::dim());
    if zone.pad.has_missing() {
        row.push(" !", Style::default().fg(theme::rec_active_val()).bg(theme::bg_val()));
    }
    row.line()
}

#[cfg(test)]
mod tests {
    use super::super::tests::{map, text, zoned};
    use super::*;

    /// The rule draws a brace under the keys the zone holds, with a tick
    /// where its root is, and nothing under the keys it does not.
    #[test]
    fn the_rule_braces_the_keys_the_zone_holds() {
        let state = zoned();
        let view = SamplerView::new();
        let map = map(&state, &view);
        let line = rule_line(&map, LOW, 200);
        let drawn = text(&[line]);
        assert!(drawn.contains(LEFT), "the brace has no left end:\n{drawn}");
        assert!(drawn.contains(RIGHT), "the brace has no right end:\n{drawn}");
        assert!(drawn.contains(ROOT), "the root is not ticked:\n{drawn}");

        // The tick is over the key it names: C3, the zone's root.
        let root_cell = drawn.chars().position(|c| c == ROOT).expect("no tick");
        assert_eq!(root_cell, column_of(LOW, 60), "the tick is on the wrong key");

        // And the brace ends where the zone does.
        let first = drawn.chars().position(|c| c == LEFT).expect("no left end");
        assert_eq!(first, column_of(LOW, 48), "the brace starts on the wrong key");
    }

    /// A bed with no zones on it draws no rule at all — not a row of air
    /// dressed up as one.
    #[test]
    fn a_bed_with_no_zones_draws_an_empty_rule() {
        let mut state = zoned();
        state.zones.clear();
        let view = SamplerView::new();
        let drawn = text(&[rule_line(&map(&state, &view), LOW, 120)]);
        assert!(drawn.trim().is_empty(), "something was drawn for no zones:\n{drawn}");
    }

    /// The list says what each zone is: its span, its sound, its root and
    /// how many keys it holds.
    #[test]
    fn the_zone_list_reads_like_a_zone() {
        let state = zoned();
        let view = SamplerView::new();
        let list = text(&zone_list(&map(&state, &view), 46, 10));
        assert!(list.contains("zones"), "{list}");
        assert!(list.contains("C2-B3"), "{list}");
        assert!(list.contains("piano"), "{list}");
        assert!(list.contains("root C3"), "{list}");
        assert!(list.contains("24 keys"), "{list}");
    }

    /// An empty bed says which three keys make a zone rather than showing
    /// a blank column.
    #[test]
    fn an_empty_bed_tells_the_player_what_to_press() {
        let mut state = zoned();
        state.zones.clear();
        let view = SamplerView::new();
        let list = text(&zone_list(&map(&state, &view), 46, 10));
        assert!(list.contains("none yet"), "{list}");
        assert!(list.contains("w covers the whole bed"), "{list}");
        assert!(list.contains("s splits"), "{list}");
    }
}
