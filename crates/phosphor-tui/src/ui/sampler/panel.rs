//! The pad panel: the pad's controls, the selected layer's under them, and
//! the list of what is on the pad.
//!
//! The knobs are the house's — [`super::super::knobs`] draws them, the step
//! grid draws the same ones, and what each says is
//! [`PadKnob`](phosphor_app::sampler::knobs::PadKnob)'s answer so that the
//! panel and the keys cannot disagree about what a control is called or
//! what it reads.

use super::*;

use phosphor_app::format::db_text;
use phosphor_app::sampler::MAX_LAYERS;

use super::super::knobs::{window, Knob, Panel};

fn knob_of(map: &Map, pad: &PadState, knob: PadKnob) -> Knob {
    let (layer, zone) = (map.layer(), map.zone());
    Knob::new(
        knob.label(),
        knob.value(pad, layer, zone),
        knob.frac(pad, layer, zone),
    )
}

/// The pad's own controls, the selected layer's under them, and the layer
/// list under that — scrolled so the control under the cursor is on the
/// screen whatever height it was given.
pub(super) fn panel_lines(map: &Map, width: usize, height: usize) -> Vec<Line<'static>> {
    if height == 0 {
        return Vec::new();
    }
    // Narrower than one knob and its heading, there is no panel to draw —
    // only the line that says what the keys are on, so that a player in a
    // small terminal can still see the caret move.
    if width < MIN_PANEL_W {
        return vec![Line::from(Span::styled(
            clip_text(
                &format!(" {} \u{00b7} {} layers", map.state.edit_title(), map.layer_count()),
                width,
            ),
            map.heading(),
        ))];
    }
    // Keys mode on a key no zone covers: there is nothing to draw controls
    // for, and the useful thing to say is which three keys make one.
    let Some(pad) = map.pad() else {
        return no_zone_lines(map, width, height);
    };
    let knobs = map.knobs();
    let cursor = map.knob_cursor();
    let on_layer = cursor >= PadKnob::PAD_CONTROLS;

    let panel = |cursor: usize, active: bool| Panel {
        cursor,
        active: active && map.focused,
        locked: map.view.locked,
        focused: map.focused,
        colour: map.colour,
        width,
        indent: INDENT,
    };

    let pad_knobs: Vec<Knob> =
        knobs[..PadKnob::PAD_CONTROLS].iter().map(|k| knob_of(map, pad, *k)).collect();
    let title = map.state.edit_title();
    let (mut rows, pad_cursor_row) =
        panel(cursor.min(PadKnob::PAD_CONTROLS - 1), !on_layer).rows(&title, &pad_knobs);

    // Which row the window has to keep on the screen. The two panels have
    // their own cursors, so the one the keys are actually in decides.
    let mut cursor_row = if on_layer { rows.len() } else { pad_cursor_row };
    if knobs.len() > PadKnob::PAD_CONTROLS {
        let layer_knobs: Vec<Knob> =
            knobs[PadKnob::PAD_CONTROLS..].iter().map(|k| knob_of(map, pad, *k)).collect();
        let title = format!("layer {}", map.layer_cursor() + 1);
        let (layer_rows, layer_cursor_row) =
            panel(cursor.saturating_sub(PadKnob::PAD_CONTROLS), on_layer)
                .rows(&title, &layer_knobs);
        if on_layer {
            cursor_row = rows.len() + layer_cursor_row;
        }
        rows.extend(layer_rows);
    }

    rows.extend(layer_list(map, pad, width));
    window(rows, height, cursor_row)
}

/// The panel on a key keys mode has no zone for: where the caret is, and
/// the three keys that put a zone under it.
fn no_zone_lines(map: &Map, width: usize, height: usize) -> Vec<Line<'static>> {
    let mut rows = vec![Line::from(Span::styled(
        clip_text(
            &format!(" {} \u{00b7} no zone on this key", SamplerState::pad_label(map.state.cursor)),
            width,
        ),
        map.heading(),
    ))];
    for line in [
        "w covers the whole bed",
        "o covers this octave",
        "s splits the zone under the caret here",
    ] {
        rows.push(Line::from(Span::styled(
            clip_text(&format!("{:w$}{line}", "", w = INDENT), width),
            theme::dim(),
        )));
    }
    rows.truncate(height);
    rows
}

/// Every sound on the pad or in the zone: what it is called, how long it
/// plays, what it is turned up to, and whether it is muted or missing.
fn layer_list(map: &Map, pad: &PadState, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![Span::styled(
        format!("{:>w$} ", "layers", w = INDENT - 1),
        theme::dim(),
    )])];
    if pad.layers.is_empty() {
        lines.push(Line::from(Span::styled(
            clip_text(
                &format!(
                    "{:w$}a loads a sound onto {}",
                    "",
                    match map.state.mode {
                        MapMode::Keys => "this zone",
                        MapMode::Pads => "this pad",
                    },
                    w = INDENT,
                ),
                width,
            ),
            theme::dim(),
        )));
        return lines;
    }

    let cursor = map.layer_cursor();
    // The word when there is room for it, the mark when there is not — the
    // same `!` the pad list uses, so one shorthand covers both.
    let long_marks = width >= INDENT + 33;
    let name_w =
        width.saturating_sub(INDENT + 17 + if long_marks { 8 } else { 2 }).clamp(4, 18);
    for (index, layer) in pad.layers.iter().enumerate() {
        let here = index == cursor;
        let mark_style = if here && map.focused {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else if here {
            theme::amber()
        } else {
            theme::dim()
        };
        let mut row = Row::new(width);
        // The mark sits against the number it points at, not out at the
        // left margin where it would be pointing at the heading column.
        row.push(
            format!("{:>w$}", if here { "\u{25B8} " } else { "" }, w = INDENT),
            mark_style,
        );
        row.push(format!("{} ", index + 1), theme::dim());
        row.push(
            format!("{:<w$}", clip_text(&layer.name, name_w), w = name_w),
            if layer.mute { theme::dim() } else { theme::normal() },
        );
        row.push(format!("{:>7}", format!("{:.2}s", layer.seconds())), theme::dim());
        row.push(format!("{:>8}", db_text(layer.gain)), theme::dim());
        // A layer that is both muted and missing says the worse of the two:
        // a mute is a decision, a missing file is a repair.
        if layer.pcm.is_none() {
            row.push(
                if long_marks { " missing" } else { " !" },
                Style::default().fg(theme::rec_active_val()).bg(theme::bg_val()),
            );
        } else if layer.mute {
            row.push(" \u{25CF}", theme::amber());
        }
        lines.push(row.line());
    }
    // What is left of the bed, so a player stacking a kit knows when to
    // stop, and the three keys that act on the row under the cursor.
    lines.push(Line::from(Span::styled(
        clip_text(
            &format!(
                "{:w$}{} of {MAX_LAYERS} \u{00b7} [ ] picks \u{00b7} m mutes \u{00b7} d removes",
                "",
                pad.layers.len(),
                w = INDENT,
            ),
            width,
        ),
        theme::dim(),
    )));
    lines
}

#[cfg(test)]
mod tests {
    use super::super::tests::{kit, map, text};
    use super::*;
    use phosphor_app::sampler::{SamplerState, MAX_LAYERS};

    /// An empty pad offers its own controls and nothing else — a gain knob
    /// for a sound that is not there is a control that answers keys and
    /// changes nothing.
    #[test]
    fn an_empty_pad_has_no_layer_controls() {
        let state = SamplerState::new();
        let view = SamplerView::new();
        let panel = text(&panel_lines(&map(&state, &view), 57, 20));
        assert!(panel.contains("a loads a sound"), "{panel}");
        assert!(panel.contains("keytrk"), "the pad's own controls are missing:\n{panel}");
        assert!(!panel.contains("rev"), "an empty pad offered a layer control:\n{panel}");
    }

    /// The panel names the pad, names every sound on it, times them, and
    /// says which one has lost its file.
    #[test]
    fn the_panel_reads_the_pad_and_its_layers() {
        let state = kit();
        let view = SamplerView::new();
        let panel = text(&panel_lines(&map(&state, &view), 57, 30));
        assert!(panel.contains("pad C3"), "{panel}");
        assert!(panel.contains("one-shot"), "{panel}");
        assert!(panel.contains("kick"), "{panel}");
        assert!(panel.contains("clap"), "{panel}");
        assert!(panel.contains("missing"), "{panel}");
        assert!(panel.contains("1.00s"), "a layer's length is not shown:\n{panel}");
    }

    /// The panel scrolls to the control being turned: a cursor on the last
    /// layer knob is on the screen even when the panel is many times taller
    /// than the space it has.
    #[test]
    fn the_panel_scrolls_to_the_control_under_the_cursor() {
        let state = kit();
        let mut view = SamplerView::new();
        for knob in 0..PadKnob::ALL.len() {
            view.knob = knob;
            let shown = text(&panel_lines(&map(&state, &view), 57, 3));
            let label = PadKnob::ALL[knob].label();
            assert!(
                shown.contains(label),
                "the cursor is on {label} and it is off the screen:\n{shown}",
            );
        }
    }

    /// A pad cursor that walked onto a pad with fewer layers does not take
    /// the renderer with it.
    #[test]
    fn a_stale_layer_cursor_does_not_index_past_the_pad() {
        let mut state = kit();
        state.cursor = 0; // an empty pad
        let mut view = SamplerView::new();
        view.layer = MAX_LAYERS + 4;
        view.knob = PadKnob::ALL.len() - 1;
        let map = map(&state, &view);
        assert!(map.layer().is_none());
        let _ = text(&panel_lines(&map, 57, 20));
    }
}
