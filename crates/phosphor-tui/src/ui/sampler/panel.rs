//! The pad panel: the pad's controls, the selected layer's under them, what
//! the pad was recorded from, and the list of what is on it.
//!
//! The knobs are the house's — [`super::super::knobs`] draws them, the step
//! grid draws the same ones, and what each says is
//! [`PadKnob`](phosphor_app::sampler::knobs::PadKnob)'s answer so that the
//! panel and the keys cannot disagree about what a control is called or
//! what it reads.

use super::*;

use phosphor_app::format::db_text;
use phosphor_app::sampler::{MAX_LAYERS, MAX_PHRASES};

use super::super::knobs::{window, Knob, Panel};

fn knob_of(map: &Map, pad: &PadState, knob: PadKnob) -> Knob {
    let (row, zone) = (map.row(), map.zone());
    Knob::new(knob.label(), knob.value(pad, row, zone), knob.frac(pad, row, zone))
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
                &format!(" {} \u{00b7} {} sounds", map.state.edit_title(), map.row_count()),
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
        // The heading names what the controls under it belong to, because a
        // phrase's three and a layer's six are different controls with some
        // of the same words on them.
        let title = match map.row() {
            Some(PadRow::Phrase(phrase)) => phrase.name.clone(),
            _ => format!("layer {}", map.layer_cursor() + 1),
        };
        let (layer_rows, layer_cursor_row) =
            panel(cursor.saturating_sub(PadKnob::PAD_CONTROLS), on_layer)
                .rows(&title, &layer_knobs);
        if on_layer {
            cursor_row = rows.len() + layer_cursor_row;
        }
        rows.extend(layer_rows);
    }

    rows.extend(source_line(pad, width));
    rows.extend(sound_list(map, pad, width));
    window(rows, height, cursor_row)
}

/// What this pad was last recorded from, when it has been.
///
/// The pad remembers an instrument and its whole panel — that is what makes
/// `i` a one-press road back to the sound a take was made with — and until
/// now nothing on the screen admitted it. The line says which instrument,
/// and which key goes there, because a memory nobody can see is a memory
/// nobody uses. In keys mode it is the zone's, which is what `pad` already
/// is here.
///
/// `None` on a pad that remembers nothing, and on a pane too narrow to say
/// the whole of "source: X": half the sentence raises a question instead of
/// answering one, and the key that opens the picker is on the bottom bar
/// either way.
fn source_line(pad: &PadState, width: usize) -> Option<Line<'static>> {
    let source = pad.source.as_ref()?;
    let head = format!("{:w$}{SOURCE_MARK} source: ", "", w = INDENT);
    let name = source.instrument.label();
    if head.chars().count() + name.chars().count() > width {
        return None;
    }
    let mut row = Row::new(width);
    row.push(head, theme::dim());
    row.push(name, theme::amber());
    // The key, dropped before the name it belongs to — and it is worth the
    // columns, because "records more" is the difference between a label and
    // an offer.
    row.push_widest(&[" \u{00b7} i records more", " \u{00b7} i"], theme::dim());
    Some(row.line())
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
/// plays, what it is turned up to, and whether it is muted, missing, or a
/// phrase with no instrument to play it.
///
/// One list, layers first and phrases after, because that is the list
/// `[`/`]`, `1`-`8`, `m` and `d` all walk. A phrase in a list of its own
/// would be a second cursor for the player to keep track of.
fn sound_list(map: &Map, pad: &PadState, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![Span::styled(
        format!("{:>w$} ", "sounds", w = INDENT - 1),
        theme::dim(),
    )])];
    if pad.rows() == 0 {
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
    for index in 0..pad.rows() {
        let Some(sound) = pad.row(index) else { continue };
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
        let (muted, seconds, level) = match sound {
            PadRow::Layer(l) => (l.mute, l.seconds(), db_text(l.gain)),
            // A percentage, not decibels: a phrase's control scales the
            // velocities it plays rather than the level that comes out.
            PadRow::Phrase(p) => {
                (p.mute, p.seconds(map.rate), format!("{}%", (p.gain * 100.0).round() as i32))
            }
        };
        row.push(
            format!("{:<w$}", clip_text(sound.name(), name_w), w = name_w),
            if muted { theme::dim() } else { theme::normal() },
        );
        row.push(format!("{:>7}", format!("{seconds:.2}s")), theme::dim());
        row.push(format!("{level:>8}"), theme::dim());
        lines.push(tail_of(row, sound, map).line());
    }
    // What is left of the bed, so a player stacking a kit knows when to
    // stop, and the three keys that act on the row under the cursor. The
    // phrase count only when there are phrases: a bed nobody is using is a
    // number nobody needs.
    let beds = if pad.phrases.is_empty() {
        format!("{} of {MAX_LAYERS}", pad.layers.len())
    } else {
        format!("{} of {MAX_LAYERS} \u{00b7} {} of {MAX_PHRASES} phr", pad.layers.len(), pad.phrases.len())
    };
    lines.push(Line::from(Span::styled(
        clip_text(
            &format!("{:w$}{beds} \u{00b7} [ ] picks \u{00b7} m mutes \u{00b7} d removes", "", w = INDENT),
            width,
        ),
        theme::dim(),
    )));
    lines
}

/// The last column of a sound's row: what is wrong with it, or what it is.
///
/// A layer that is both muted and missing says the worse of the two — a
/// mute is a decision, a missing file is a repair — and a phrase with no
/// child instrument says that first for the same reason. A phrase that is
/// fine says `phr`, because in a mixed list the kind of a row is the thing
/// the columns above it cannot show.
fn tail_of(mut row: Row, sound: PadRow<'_>, map: &Map) -> Row {
    let alarm = Style::default().fg(theme::rec_active_val()).bg(theme::bg_val());
    match sound {
        PadRow::Layer(l) if l.pcm.is_none() => row.push_widest(&[" missing", " !"], alarm),
        PadRow::Layer(l) if l.mute => row.push(" \u{25CF}", theme::amber()),
        PadRow::Layer(_) => {}
        // Only reachable on a hand-edited session: landing a phrase always
        // sets the child. Surfaced anyway, because the engine's answer to a
        // phrase with no child is silence and nothing else.
        PadRow::Phrase(_) if map.state.child.is_none() => {
            row.push_widest(&[" no instrument", " no inst", " !"], alarm);
        }
        PadRow::Phrase(p) => {
            row.push(" phr", theme::dim());
            if p.mute {
                row.push(" \u{25CF}", theme::amber());
            }
        }
    }
    row
}

#[cfg(test)]
mod tests {
    use super::super::tests::{kit, map, text, zoned};
    use super::*;
    use phosphor_app::sampler::{PadSource, SamplerState, MAX_LAYERS};
    use phosphor_app::state::InstrumentType;

    fn source_of(instrument: InstrumentType) -> PadSource {
        PadSource { instrument, params: vec![0.5; 8] }
    }

    /// A pad that remembers an instrument says so on the panel, with the
    /// key that goes back to it.
    ///
    /// The defect this pins: the pad kept the instrument and the whole panel
    /// it was recorded with, and nothing on the screen ever admitted it — so
    /// the one-press road back to the sound was a road only the code knew
    /// about.
    #[test]
    fn a_pad_that_remembers_an_instrument_says_so() {
        let mut state = kit();
        let view = SamplerView::new();
        let plain = text(&panel_lines(&map(&state, &view), 57, 40));
        assert!(!plain.contains("source"), "a pad with no memory claimed one:\n{plain}");

        state.pads[state.cursor].source = Some(source_of(InstrumentType::DX7));
        let shown = text(&panel_lines(&map(&state, &view), 57, 40));
        assert!(shown.contains("source: DX7"), "the panel does not name the source:\n{shown}");
        assert!(shown.contains("i records more"), "no way back to it:\n{shown}");
        assert!(shown.contains(SOURCE_MARK), "the line wears no mark:\n{shown}");
    }

    /// In keys mode the source belongs to the zone, which is what the panel
    /// is drawing controls for — reading the pad under the caret instead
    /// would name whatever was recorded on that key before the zone existed.
    #[test]
    fn in_keys_mode_the_source_line_is_the_zones() {
        let mut state = zoned();
        let view = SamplerView::new();
        // The pad under the caret remembers one thing, the zone another.
        state.pads[state.cursor].source = Some(source_of(InstrumentType::Rhodes));
        state.zones[0].pad.source = Some(source_of(InstrumentType::Juno60));
        let shown = text(&panel_lines(&map(&state, &view), 70, 40));
        assert!(shown.contains("source: Juno-60"), "the zone's source is not shown:\n{shown}");
        assert!(!shown.contains("Rhodes"), "the pad under the zone got a vote:\n{shown}");
    }

    /// The line gives its columns up in order rather than running off the
    /// right edge or leaving half a sentence behind.
    #[test]
    fn the_source_line_drops_its_columns_before_it_overruns() {
        let mut state = kit();
        // The longest name there is, so the widths below are the worst case.
        state.pads[state.cursor].source = Some(source_of(InstrumentType::Synth));
        let view = SamplerView::new();
        let mut seen_short = false;
        for width in MIN_PANEL_W..60 {
            let shown = text(&panel_lines(&map(&state, &view), width, 40));
            for line in shown.lines() {
                assert!(line.chars().count() <= width, "a {width}-column panel overran: {line}");
            }
            let Some(line) = shown.lines().find(|l| l.contains("source:")) else {
                // Too narrow for the whole of "source: X" — and then the
                // line is absent rather than cut in half.
                continue;
            };
            assert!(line.contains("Phosphor Synth"), "the name was cut off: {line}");
            if !line.contains("i records more") {
                seen_short = true;
            }
        }
        assert!(seen_short, "the key hint never dropped, so nothing was under pressure");
    }

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

    /// A phrase row reads like a phrase: its name, how long it plays at the
    /// engine's rate, the velocity scale as a percentage, and `phr` — which
    /// is the only place in the list the kind of a row can be read.
    #[test]
    fn a_phrase_row_says_what_it_is_and_how_long() {
        let mut state = kit();
        let events = std::sync::Arc::from(vec![phosphor_plugin::sample::PhraseEvent {
            frame: 0,
            status: 0x90,
            data1: 60,
            data2: 100,
        }]);
        let pad = state.cursor;
        state.pads[pad].add_phrase(events, 88_200, 0.0, "pad").unwrap();
        state.child = Some(phosphor_app::sampler::PadSource {
            instrument: phosphor_app::state::InstrumentType::DX7,
            params: vec![0.5],
        });
        let mut view = SamplerView::new();
        view.layer = 2; // the phrase, after the kit's two layers

        let panel = text(&panel_lines(&map(&state, &view), 57, 40));
        assert!(panel.contains("phrase 1"), "{panel}");
        assert!(panel.contains("phr"), "the row's kind is not on it:\n{panel}");
        // 88 200 frames at the map's 44.1 kHz is two seconds.
        assert!(panel.contains("2.00s"), "the length is not shown:\n{panel}");
        assert!(panel.contains("100%"), "the velocity scale is not a percentage:\n{panel}");
        assert!(panel.contains("2 of 4 phr") || panel.contains("1 of 4 phr"), "{panel}");
        // The panel's second group is the phrase's own three controls.
        assert!(panel.contains("vel"), "the phrase's control is missing:\n{panel}");
        assert!(panel.contains("keytrk"), "{panel}");
        assert!(!panel.contains(" rev"), "a phrase row offered reverse:\n{panel}");
        assert!(!panel.contains("tune"), "a phrase row offered tune:\n{panel}");
    }

    /// A sampler with no child cannot sound a phrase at all — the engine's
    /// answer is silence and nothing else — so the row says so. Only a
    /// hand-edited session can reach this; landing a phrase always sets one.
    #[test]
    fn a_phrase_with_no_child_says_no_instrument() {
        let mut state = kit();
        let events = std::sync::Arc::from(vec![phosphor_plugin::sample::PhraseEvent {
            frame: 0,
            status: 0x90,
            data1: 60,
            data2: 100,
        }]);
        let pad = state.cursor;
        state.pads[pad].add_phrase(events, 1_000, 0.0, "pad").unwrap();
        assert!(state.child.is_none());
        let view = SamplerView::new();
        let panel = text(&panel_lines(&map(&state, &view), 57, 40));
        assert!(panel.contains("no inst"), "the dead phrase is not marked:\n{panel}");
        assert!(!panel.contains("% phr"), "a dead phrase was marked as a live one:\n{panel}");
        // Wide enough for the whole sentence, it gets the whole sentence.
        let wide = text(&panel_lines(&map(&state, &view), 90, 40));
        assert!(wide.contains("no instrument"), "the warning was cut short:\n{wide}");
    }

    /// A pad cursor that walked onto a pad with fewer sounds does not take
    /// the renderer with it.
    #[test]
    fn a_stale_layer_cursor_does_not_index_past_the_pad() {
        let mut state = kit();
        state.cursor = 0; // an empty pad
        let mut view = SamplerView::new();
        view.layer = MAX_LAYERS + 4;
        view.knob = PadKnob::ALL.len() - 1;
        let map = map(&state, &view);
        assert!(map.row().is_none());
        let _ = text(&panel_lines(&map, 57, 20));
    }
}
