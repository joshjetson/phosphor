//! The trim strip: one layer's waveform, with the region that plays lit and
//! the rest of the file left dark.
//!
//! # What is on the screen
//!
//! ```text
//!  C3 · kick · 0.250s · unit 10ms · snap on · 0.010s → 0.240s
//!        ▄▄                      ▄
//!  ▄▄███████▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄████▄▄··················
//!        ▀▀                      ▀
//!  ·····[························]···················
//!  h/l start · H/L end · w hug · j/k unit · z snap · r rev · t loop · esc back
//! ```
//!
//! The whole file, always — there is no zoom, and there is deliberately no
//! zoom. A trim is a decision about where a sound begins and ends *relative
//! to itself*, and a view that scrolled would answer "where in the window"
//! instead. The consequence is that the markers move by a fraction of a
//! column at the fine units, which is why the header prints the numbers: the
//! picture is for finding the transient, the numbers are for reading the
//! answer, and the audition is for checking it.
//!
//! The height is spent on the waveform because that is what there is to look
//! at. Everything else is one row each, and on a pane too short for all of
//! them the waveform is what shrinks.
//!
//! The picture itself — the reduction, its cache, the half blocks and the
//! ruler — is [`super::wave`]'s, because the pad panel draws the same
//! waveform three rows tall and two decimators would be two pictures of one
//! sound.

use super::*;

use phosphor_app::sampler::trim::{NudgeUnit, TrimEdge};
use phosphor_app::sampler::LayerState;

use super::wave::{marker_row, wave_rows, with_peaks};

/// The tallest the waveform is drawn. Past a dozen rows a peak picture stops
/// telling a player anything they did not already know, and the pad list
/// underneath is worth more than the extra amplitude.
const MAX_WAVE_ROWS: usize = 12;

/// Rows the strip spends on words: the header, the marker ruler, the keys.
const CHROME_ROWS: usize = 3;

/// The strip's own keys, on the strip. The bottom bar says the same thing in
/// its own shorthand; this is the row a player reads without looking away
/// from the waveform.
const KEYS: &str = " h/l start \u{00b7} H/L end \u{00b7} w hug \u{00b7} j/k unit \
                    \u{00b7} z snap \u{00b7} r rev \u{00b7} t loop \u{00b7} esc back";

/// The header: which layer, how long it plays, and every switch the keys
/// can throw, in the order the keys are laid out.
fn header(
    map: &Map,
    layer: &LayerState,
    unit: NudgeUnit,
    snap: bool,
    width: usize,
) -> Line<'static> {
    let mut row = Row::new(width);
    row.push(
        format!(" {} ", SamplerState::pad_label(map.state.cursor)),
        map.heading(),
    );
    let name_w = row.left().saturating_sub(40).clamp(4, 20);
    row.push(clip_text(&layer.name, name_w), theme::normal());
    row.push(format!(" \u{00b7} {:.3}s", layer.seconds()), theme::dim());
    row.push(format!(" \u{00b7} unit {}", unit.label()), theme::amber_bright());
    row.push(
        format!(" \u{00b7} snap {}", if snap { "on" } else { "off" }),
        if snap { theme::amber() } else { theme::dim() },
    );
    row.push(
        format!(
            " \u{00b7} {:.3}s \u{2192} {:.3}s",
            layer.edge_seconds(TrimEdge::Start),
            layer.edge_seconds(TrimEdge::End),
        ),
        theme::normal(),
    );
    if layer.reverse {
        row.push(" \u{00b7} rev", theme::amber());
    }
    row.line()
}

/// The strip, for a pane `width` by `height`.
///
/// `None` when there is nothing to draw one of — an empty pad, or a layer
/// whose file has gone missing since the strip was opened, which the keys
/// refuse but a redraw must survive.
pub(super) fn strip_lines(map: &Map, width: usize, height: usize) -> Option<Vec<Line<'static>>> {
    let view = map.view.trim?;
    // A phrase has no waveform, so the strip has nothing to draw for one —
    // the keys refuse to open it, and this is the redraw's own guard for a
    // cursor that walked onto one while it was open.
    let PadRow::Layer(layer) = map.row()? else { return None };
    let pcm = layer.pcm.as_ref()?;
    let region = layer.region()?;
    let frames = pcm.frames();
    if width == 0 || height == 0 {
        return Some(Vec::new());
    }

    let lit = Style::default().fg(map.colour).bg(theme::bg_val());
    let cut = theme::dim();
    let mark = if map.focused { theme::amber_bright() } else { theme::amber() };

    let mut lines = vec![header(map, layer, view.unit, view.snap, width)];
    // The waveform takes what the words leave, and the words go one at a
    // time when even that is not enough: on a two-row pane a header and a
    // single row of waveform beats three rows of chrome and nothing to look
    // at.
    let wave = height.saturating_sub(CHROME_ROWS).clamp(1, MAX_WAVE_ROWS);
    if height > 1 {
        lines.extend(with_peaks(pcm, width, |peaks| {
            wave_rows(peaks, wave.min(height - 1), region, frames, lit, cut)
        }));
    }
    if height > lines.len() {
        lines.push(marker_row(region, frames, width, lit, cut, mark));
    }
    if height > lines.len() {
        lines.push(Line::from(Span::styled(clip_text(KEYS, width), theme::dim())));
    }
    lines.truncate(height);
    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{kit, map, ramp_kit, text};
    use super::*;
    use phosphor_app::sampler::SamplerState;
    use phosphor_plugin::sample::SamplePcm;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn open(unit: NudgeUnit, snap: bool) -> SamplerView {
        let mut view = SamplerView::new();
        view.trim = Some(crate::state::TrimView { unit, snap, looping: false });
        view
    }

    /// The header says every switch the keys can throw, so a player never
    /// has to press one to find out where it is.
    #[test]
    fn the_header_names_the_layer_the_unit_the_snap_and_the_markers() {
        let state = ramp_kit(44_100);
        let view = open(NudgeUnit::TenMs, true);
        let shown = text(&strip_lines(&map(&state, &view), 90, 14).unwrap());
        assert!(shown.contains("C3"), "{shown}");
        assert!(shown.contains("ramp"), "{shown}");
        assert!(shown.contains("unit 10ms"), "{shown}");
        assert!(shown.contains("snap on"), "{shown}");
        assert!(shown.contains("0.000s \u{2192} 1.000s"), "no trim times:\n{shown}");
        assert!(shown.contains("esc back"), "no way out on the screen:\n{shown}");

        // ...and it follows the switches rather than describing them once.
        let view = open(NudgeUnit::Bar, false);
        let shown = text(&strip_lines(&map(&state, &view), 90, 14).unwrap());
        assert!(shown.contains("unit bar"), "{shown}");
        assert!(shown.contains("snap off"), "{shown}");
    }

    /// The marker ruler, as columns. The glyphs around the markers are
    /// multi-byte, so a byte offset into this string is not a column.
    fn ruler(lines: &[Line<'static>]) -> Vec<char> {
        // Second from the bottom: the last row is the key hints.
        lines[lines.len() - 2]
            .spans
            .iter()
            .flat_map(|s| s.content.chars().collect::<Vec<_>>())
            .collect()
    }

    fn column_of_marker(ruler: &[char], marker: char) -> Option<usize> {
        ruler.iter().position(|c| *c == marker)
    }

    /// The markers sit over the frames they name, and the region between
    /// them is lit while the rest of the file is not.
    #[test]
    fn the_region_is_lit_and_the_markers_sit_over_it() {
        let mut state = ramp_kit(44_100);
        let pad = state.cursor;
        state.pads[pad].layers[0].start_frame = 11_025; // a quarter in
        state.pads[pad].layers[0].end_frame = 33_075; // three quarters
        let view = open(NudgeUnit::TenMs, true);
        let lines = strip_lines(&map(&state, &view), 80, 14).unwrap();

        let ruler = ruler(&lines);
        assert_eq!(ruler.len(), 80, "the ruler is not the pane's width");
        let open_at = column_of_marker(&ruler, '[').expect("no start marker");
        let close_at = column_of_marker(&ruler, ']').expect("no end marker");
        assert_eq!(open_at, 20, "the start marker is at column {open_at}, not a quarter in");
        assert_eq!(close_at, 59, "the end marker is at column {close_at}");

        // The waveform's own columns agree: lit inside, dim outside. The
        // centre row is always drawn, so it is the one to read.
        let wave = &lines[1 + (lines.len() - 3) / 2];
        assert_eq!(wave.spans.len(), 80);
        assert_eq!(wave.spans[0].style, theme::dim(), "a cut column is lit");
        assert_ne!(wave.spans[40].style, theme::dim(), "a playing column is dim");
        assert_eq!(wave.spans[79].style, theme::dim(), "the tail is lit");
    }

    /// A trim moves the markers and never the waveform — which is also what
    /// lets the reduction be cached across a nudge run.
    #[test]
    fn nudging_an_edge_redraws_the_markers_and_not_the_picture() {
        let mut state = ramp_kit(44_100);
        let view = open(NudgeUnit::TenMs, true);
        let wave_of = |state: &SamplerState| -> Vec<String> {
            let lines = strip_lines(&map(state, &view), 80, 14).unwrap();
            lines[1..lines.len() - 2]
                .iter()
                .map(|l| l.spans.iter().map(|s| s.content.to_string()).collect())
                .collect()
        };
        let before = wave_of(&state);
        let pad = state.cursor;
        state.pads[pad].layers[0].start_frame = 22_050;
        let after = wave_of(&state);
        assert_eq!(before, after, "the waveform moved when only a marker should have");

        let lines = strip_lines(&map(&state, &view), 80, 14).unwrap();
        assert_eq!(
            column_of_marker(&ruler(&lines), '['),
            Some(40),
            "the marker did not follow the trim",
        );
    }

    /// Nothing runs past the right edge, nothing runs past the bottom, and
    /// a pane too small for the chrome still draws something.
    #[test]
    fn the_strip_fits_whatever_it_is_given() {
        let state = ramp_kit(4_410);
        let view = open(NudgeUnit::OneMs, false);
        for width in [1usize, 4, 20, 57, 120, 400] {
            for height in [1usize, 2, 3, 4, 10, 40] {
                let lines = strip_lines(&map(&state, &view), width, height).unwrap();
                assert!(lines.len() <= height, "{width}x{height}: {} rows", lines.len());
                assert!(!lines.is_empty(), "{width}x{height}: nothing drawn");
                for line in &lines {
                    let cells: usize =
                        line.spans.iter().map(|s| s.content.chars().count()).sum();
                    assert!(cells <= width, "{width}x{height}: a {cells}-cell row");
                }
            }
        }
    }

    /// A one-frame region is legal in the engine and has to be drawable: the
    /// two markers cannot share a column, and nothing panics reaching for
    /// the one either side.
    #[test]
    fn a_region_of_one_frame_still_shows_two_markers() {
        let mut state = ramp_kit(44_100);
        let pad = state.cursor;
        state.pads[pad].layers[0].start_frame = 100;
        state.pads[pad].layers[0].end_frame = 101;
        let view = open(NudgeUnit::Sample, true);
        for width in [1usize, 2, 3, 80] {
            let lines = strip_lines(&map(&state, &view), width, 10).unwrap();
            let marks = ruler(&lines);
            if width > 1 {
                assert!(marks.contains(&'['), "{width}: no start marker in {marks:?}");
                assert!(marks.contains(&']'), "{width}: no end marker in {marks:?}");
            }
        }

        // ...and the same at the very end of the file, where the marker
        // would otherwise be drawn one column off the right edge.
        state.pads[pad].layers[0].start_frame = 44_098;
        state.pads[pad].layers[0].end_frame = 44_100;
        let lines = strip_lines(&map(&state, &view), 80, 10).unwrap();
        let marks = ruler(&lines);
        assert_eq!(marks.len(), 80);
        assert!(marks.contains(&']'), "{marks:?}");
        assert!(marks.contains(&'['), "{marks:?}");
    }

    /// A quiet recording is drawn as a waveform, not as a flat line: the
    /// picture normalises to the file's own peak.
    #[test]
    fn a_quiet_recording_still_has_a_shape() {
        let mut state = SamplerState::new();
        let data: Vec<f32> = (0..4_410)
            .map(|i| 0.01 * (core::f32::consts::TAU * 20.0 * i as f32 / 4_410.0).sin())
            .collect();
        let pcm = Arc::new(SamplePcm { data, channels: 1, sample_rate: 44_100.0 });
        let pad = SamplerState::pad_of_note(60).unwrap();
        state.add_wav_layer(pad, PathBuf::from("quiet.wav"), pcm).unwrap();
        state.cursor = pad;
        let view = open(NudgeUnit::OneMs, true);
        let shown = text(&strip_lines(&map(&state, &view), 80, 14).unwrap());
        let tall = shown.lines().filter(|l| l.contains('\u{2588}')).count();
        assert!(tall > 3, "a −40 dB recording drew {tall} rows of waveform:\n{shown}");
    }

    /// Silence is a line through the middle, not a wall and not a gap.
    #[test]
    fn a_silent_buffer_draws_a_centre_line() {
        let state = kit(); // C3 holds a second of zeros
        let view = open(NudgeUnit::TenMs, true);
        let lines = strip_lines(&map(&state, &view), 40, 12).unwrap();
        let wave: Vec<String> = lines[1..lines.len() - 2]
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.to_string()).collect())
            .collect();
        let full = wave.iter().filter(|r| r.contains('\u{2588}')).count();
        assert_eq!(full, 1, "silence drew {full} rows:\n{}", wave.join("\n"));
    }

    /// A layer with no audio behind it has no strip. The keys refuse to open
    /// one, and a redraw after the file went missing must agree.
    #[test]
    fn a_missing_layer_has_no_strip_to_draw() {
        let mut state = kit();
        let view = {
            let mut v = open(NudgeUnit::TenMs, true);
            v.layer = 1; // the layer whose file is gone
            v
        };
        assert!(strip_lines(&map(&state, &view), 80, 14).is_none());

        // ...and neither has an empty pad.
        state.cursor = 0;
        assert!(strip_lines(&map(&state, &view), 80, 14).is_none());
    }
}
