//! The filled-pad list: the kit as a list of what is on it.
//!
//! One row per pad that differs from a fresh one, which includes a pad
//! whose settings were changed and whose sound was then taken off — the
//! seat is still taken, and a list that hid it would hide the reason the
//! pad does not behave like its neighbours.

use super::*;

/// One row per pad with something on it, and a line of instruction when
/// there is nothing at all.
pub(super) fn pad_list(map: &Map, width: usize, height: usize) -> Vec<Line<'static>> {
    let filled: Vec<usize> = map.state.occupied_pads().collect();
    let mut head = Row::new(width);
    head.push("  pads", map.heading());
    head.push(
        if filled.is_empty() {
            " \u{00b7} empty".to_string()
        } else {
            format!(" \u{00b7} {} filled", filled.len())
        },
        theme::dim(),
    );
    let mut lines = vec![head.line()];

    if filled.is_empty() {
        for line in ["  play a key to pick a pad, then a", "  to load a sound onto it"] {
            lines.push(Line::from(Span::styled(clip_text(line, width), theme::dim())));
        }
        return lines;
    }

    // Scroll to the pad under the caret: a kit big enough to fill the list
    // is a kit where the pad being edited is the one that has to be visible.
    let rows = height.saturating_sub(1);
    let here = filled.iter().position(|p| *p == map.state.cursor).unwrap_or(0);
    let start = (here + 1).saturating_sub(rows).min(filled.len().saturating_sub(rows));
    for &pad in filled.iter().skip(start).take(rows.max(1)) {
        lines.push(pad_row(map, pad, width));
    }
    lines
}

fn pad_row(map: &Map, pad: usize, width: usize) -> Line<'static> {
    let state = &map.state.pads[pad];
    let here = pad == map.state.cursor;
    let label_style = if here && map.focused {
        theme::amber_bright().add_modifier(Modifier::BOLD)
    } else if here {
        theme::amber()
    } else {
        theme::normal()
    };
    let count = state.layers.len();
    // One layer is named; a stack says how many, because the first of eight
    // names is not what the pad is.
    let what = match count {
        0 => "\u{2014}".to_string(),
        1 => state.layers[0].name.clone(),
        n => format!("{n} layers"),
    };

    // The columns in the order they matter, each one taken only if it fits.
    // A narrow list loses the poly and the trigger before it loses the name
    // of the pad, and never runs off its own right edge.
    let mut row = Row::new(width);
    row.push(if here { " \u{25B8} " } else { "   " }, label_style);
    row.push(format!("{:<4}", SamplerState::pad_label(pad)), label_style);
    let name_w = row.left().saturating_sub(17).clamp(4, 14);
    row.push(format!("{:<w$}", clip_text(&what, name_w), w = name_w), theme::normal());
    row.push(format!("{count:>2} "), theme::dim());
    row.push(format!("{:<9}", PadKnob::Trig.value(state, None)), theme::dim());
    row.push(format!("p{}", state.config.poly), theme::dim());
    if state.config.choke > 0 {
        row.push(format!(" c{}", state.config.choke), theme::dim());
    }
    if state.has_missing() {
        row.push(" !", Style::default().fg(theme::rec_active_val()).bg(theme::bg_val()));
    }
    row.line()
}

#[cfg(test)]
mod tests {
    use super::super::tests::{kit, map, text};
    use super::*;
    use phosphor_app::sampler::SamplerState;

    /// An empty kit says what to do about it rather than showing a blank
    /// column.
    #[test]
    fn an_empty_kit_tells_the_player_what_to_press() {
        let state = SamplerState::new();
        let view = SamplerView::new();
        let list = text(&pad_list(&map(&state, &view), 38, 10));
        assert!(list.contains("empty"), "{list}");
        assert!(list.contains("play a key"), "{list}");
    }

    /// A pad with sounds on it names them, counts them, says how it
    /// triggers, and says out loud that one of them has lost its file.
    #[test]
    fn a_filled_pad_reads_like_a_pad() {
        let state = kit();
        let view = SamplerView::new();
        let list = text(&pad_list(&map(&state, &view), 38, 10));
        assert!(list.contains("C3"), "{list}");
        assert!(list.contains("2 layers"), "{list}");
        assert!(list.contains("one-shot"), "{list}");
        assert!(list.contains('!'), "a missing file is not marked:\n{list}");
    }

    /// A kit longer than the list scrolls to the pad under the caret: a
    /// pad being edited off the bottom of its own list is a cursor nobody
    /// can see.
    #[test]
    fn a_long_kit_scrolls_to_the_pad_being_edited() {
        let mut state = kit();
        for pad in 0..40 {
            state.pads[pad].config.choke = 1;
        }
        state.cursor = 39;
        let view = SamplerView::new();
        let list = text(&pad_list(&map(&state, &view), 38, 6));
        assert!(
            list.contains(&SamplerState::pad_label(39)),
            "the pad under the caret is off the list:\n{list}",
        );
        assert_eq!(list.lines().count(), 6, "the list outgrew its space:\n{list}");
    }
}
