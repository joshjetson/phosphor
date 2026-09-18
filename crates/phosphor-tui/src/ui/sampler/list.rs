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
    // What the kit is costing. Dropped by `Row::push` on a narrow pane,
    // which is right: the pads themselves come first.
    if let Some(held) = held_label(map.state) {
        head.push(held, theme::muted());
    }
    // What the glyph on those rows means, and only where a row wears one: a
    // legend for a mark nobody's kit carries is a column spent on nothing.
    //
    // Last, and so the first thing a narrow list drops — after the memory
    // line rather than before it. A mark is explained again on the panel
    // beside this list, in the sentence that names the instrument; how much
    // audio the kit is holding is said here and nowhere else.
    if filled.iter().any(|p| map.state.pads[*p].source.is_some()) {
        head.push(format!(" \u{00b7} {SOURCE_MARK} source"), theme::dim());
    }
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
    // One sound is named; a stack says how many, because the first of eight
    // names is not what the pad is. Phrases count: a pad carrying nothing
    // but a performance is not an empty pad.
    let count = state.rows();
    let what = state.sound_label();

    // The columns in the order they matter, each one taken only if it fits.
    // A narrow list loses the poly and the trigger before it loses the name
    // of the pad, and never runs off its own right edge.
    let mut row = Row::new(width);
    row.push(if here { " \u{25B8} " } else { "   " }, label_style);
    row.push(format!("{:<4}", SamplerState::pad_label(pad)), label_style);
    let name_w = row.left().saturating_sub(17).clamp(4, 14);
    row.push(format!("{:<w$}", clip_text(&what, name_w), w = name_w), theme::normal());
    row.push(format!("{count:>2} "), theme::dim());
    row.push(format!("{:<9}", PadKnob::Trig.value(state, None, None)), theme::dim());
    row.push(format!("p{}", state.config.poly), theme::dim());
    if state.config.choke > 0 {
        row.push(format!(" c{}", state.config.choke), theme::dim());
    }
    if state.has_missing() {
        row.push(" !", Style::default().fg(theme::rec_active_val()).bg(theme::bg_val()));
    }
    // Last, so it is the first column a narrow list drops: a pad that
    // remembers an instrument is a convenience, and a pad whose file has
    // gone is a repair. The head above says what the mark means.
    if state.source.is_some() {
        row.push(format!(" {SOURCE_MARK}"), theme::amber());
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

    /// The kit says what it is costing, in a unit that is not a lie about a
    /// small kit — and an empty one says nothing at all rather than `0 MB`.
    #[test]
    fn the_list_head_says_how_much_audio_is_held() {
        let state = SamplerState::new();
        let view = SamplerView::new();
        let list = text(&pad_list(&map(&state, &view), 60, 10));
        assert!(!list.contains("held"), "an empty kit claimed to hold something:\n{list}");

        // 44 100 mono frames of f32 — 173 kB, which must not read as 0.0 MB.
        let state = kit();
        let list = text(&pad_list(&map(&state, &view), 60, 10));
        assert!(list.contains("173 kB held"), "the memory line is wrong:\n{list}");

        // And a kit big enough to matter reads in megabytes.
        let mut big = SamplerState::new();
        let pcm = std::sync::Arc::new(phosphor_plugin::sample::SamplePcm {
            data: vec![0.0; 3_000_000],
            channels: 2,
            sample_rate: 44_100.0,
        });
        big.add_wav_layer(0, std::path::PathBuf::from("take.wav"), pcm).unwrap();
        let list = text(&pad_list(&map(&big, &view), 60, 10));
        assert!(list.contains("11.4 MB held"), "the megabyte line is wrong:\n{list}");

        // A narrow pane drops it rather than running off its own edge.
        let narrow = text(&pad_list(&map(&big, &view), 24, 10));
        assert!(narrow.lines().all(|l| l.chars().count() <= 24), "the head overran:\n{narrow}");
    }

    /// A pad that remembers an instrument wears a mark, and the head says
    /// what the mark means — but only when something is wearing one.
    #[test]
    fn a_pad_carrying_a_source_is_marked_and_the_mark_is_explained() {
        let mut state = kit();
        let view = SamplerView::new();
        let plain = text(&pad_list(&map(&state, &view), 38, 10));
        assert!(!plain.contains(SOURCE_MARK), "an unrecorded kit wears the mark:\n{plain}");
        assert!(!plain.contains("\u{00b7} \u{25CE} source"), "a legend for nothing:\n{plain}");

        state.pads[state.cursor].source = Some(phosphor_app::sampler::PadSource {
            instrument: phosphor_app::state::InstrumentType::DX7,
            params: vec![0.5; 4],
        });
        let shown = text(&pad_list(&map(&state, &view), 60, 10));
        let head = shown.lines().next().unwrap();
        assert!(head.contains("source"), "the mark is not explained:\n{shown}");
        let row = shown.lines().find(|l| l.contains("C3")).expect("no C3 row");
        assert!(row.contains(SOURCE_MARK), "the pad is not marked: {row}");

        // The legend goes before the memory line does. How much audio the
        // kit is holding is said here and nowhere else; the mark is
        // explained again on the panel beside this list.
        let narrow = text(&pad_list(&map(&state, &view), 38, 10));
        let head = narrow.lines().next().unwrap();
        assert!(head.contains("kB held"), "the legend cost the memory line: {head}");
        assert!(!head.contains("\u{25CE} source"), "the head overran its own width: {head}");
    }

    /// The mark is the first column a narrow list gives up: a pad that
    /// remembers an instrument is a convenience, and a pad whose file has
    /// gone is a repair.
    #[test]
    fn the_source_mark_goes_before_the_missing_warning() {
        let mut state = kit(); // C3's second layer has lost its file
        state.pads[state.cursor].source = Some(phosphor_app::sampler::PadSource {
            instrument: phosphor_app::state::InstrumentType::DX7,
            params: vec![0.5; 4],
        });
        let view = SamplerView::new();
        let mut dropped = false;
        for width in 1..60usize {
            let shown = text(&pad_list(&map(&state, &view), width, 10));
            for line in shown.lines() {
                assert!(line.chars().count() <= width, "a {width}-column list overran: {line}");
            }
            let Some(row) = shown.lines().find(|l| l.contains("C3")) else { continue };
            if !row.contains(SOURCE_MARK) {
                dropped = true;
                continue;
            }
            assert!(row.contains('!'), "the mark outlived the warning at {width}: {row}");
        }
        assert!(dropped, "the mark never dropped, so nothing was under pressure");
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
