//! One buffer as a picture, at two sizes.
//!
//! The reduction, the half-block drawing and the marker ruler are here
//! because two views want them: the trim strip ([`super::strip`]), which
//! takes the whole pane, and the compact picture under the pad panel
//! ([`mini_lines`]), which takes three rows of it. They are the same
//! waveform seen from different distances — a second decimator would be two
//! pictures of one sound that disagree the day either is touched.
//!
//! # Why the peaks are cached
//!
//! One column is the loudest and quietest sample in its own slice of the
//! file, so drawing either view reads the whole buffer once. For a drum hit
//! that is nothing; for a ten-minute stereo take it is fifty megabytes, and
//! both views redraw on a timer. So the reduction is kept, keyed by the
//! buffer it came from and the width it was taken at — and it survives every
//! nudge, because a trim moves the markers and never the waveform.
//!
//! One entry is enough because one view is on the screen at a time: the
//! strip takes the body of the pane and returns before the panel is laid
//! out, so the compact picture is never drawn in the same frame.

use super::*;

use std::cell::RefCell;
use std::sync::Arc;

use phosphor_plugin::sample::SamplePcm;

/// A floor under the normalising peak, so that a buffer of silence draws a
/// flat line rather than dividing by nothing and painting a wall.
pub(super) const QUIET_FLOOR: f32 = 1e-4;

/// Rows the compact picture spends: two of waveform and the ruler under
/// them.
///
/// Two is enough to read an envelope against the zero line and no more than
/// the pad panel can spare — the controls above it are what the keys are
/// typing into. The ruler is not optional: without the markers the picture
/// says what the recording is and not what plays.
pub(super) const MINI_ROWS: usize = 3;

/// The narrowest picture worth drawing under the panel. Below a dozen
/// columns a waveform is a smear, and the markers either side of it would
/// be most of the row.
const MINI_MIN_PICTURE: usize = 12;

/// One layer's waveform reduced to one pair of peaks per column.
pub(super) struct Peaks {
    /// The buffer these came from, held rather than pointed at: an `Arc`
    /// cannot be freed and its address handed to a different recording while
    /// the cache is still comparing against it.
    pcm: Arc<SamplePcm>,
    width: usize,
    /// Quietest and loudest sample in each column's slice of the file.
    pub(super) columns: Vec<(f32, f32)>,
    /// The loudest sample anywhere, which the drawing normalises by: a take
    /// recorded at −20 dBFS is still a waveform, and one drawn at a twentieth
    /// of the height is a flat line with a rumour in it.
    pub(super) peak: f32,
}

thread_local! {
    /// One entry, because one view is drawn at a time.
    static PEAKS: RefCell<Option<Peaks>> = const { RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// How many times this thread has walked a buffer. A test asserts on it
    /// to prove that two views of one recording cost one reduction — which
    /// is the whole reason the cache is shared rather than copied.
    static REDUCTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Reductions done on this thread so far.
#[cfg(test)]
pub(super) fn reductions() -> usize {
    REDUCTIONS.with(std::cell::Cell::get)
}

impl Peaks {
    fn reduce(pcm: &Arc<SamplePcm>, width: usize) -> Self {
        #[cfg(test)]
        REDUCTIONS.with(|count| count.set(count.get() + 1));
        let frames = pcm.frames().max(1);
        let channels = usize::from(pcm.channels.max(1));
        let mut columns = Vec::with_capacity(width);
        let mut peak = QUIET_FLOOR;
        for column in 0..width {
            // Integer arithmetic on u64 so a long file cannot lose frames to
            // f32's mantissa: at 48 kHz a ten-minute take is 28.8 million
            // frames, which is already past 2^24.
            let lo = (column as u64 * frames / width as u64) as usize;
            let hi = (((column as u64 + 1) * frames / width as u64) as usize).max(lo + 1);
            let mut span = (0.0f32, 0.0f32);
            for frame in lo..hi.min(frames as usize) {
                // The first channel: a stereo file's two sides look alike at
                // this resolution, and summing them would draw a hole
                // wherever they disagree.
                let s = pcm.data.get(frame * channels).copied().unwrap_or(0.0);
                span.0 = span.0.min(s);
                span.1 = span.1.max(s);
                peak = peak.max(s.abs());
            }
            columns.push(span);
        }
        Self { pcm: Arc::clone(pcm), width, columns, peak }
    }

    fn is_for(&self, pcm: &Arc<SamplePcm>, width: usize) -> bool {
        self.width == width && Arc::ptr_eq(&self.pcm, pcm)
    }
}

/// Run `draw` against this buffer's peaks at this width, reducing it first
/// if the cache is holding somebody else's.
///
/// The one door to a reduction. Both views come through it, so a strip and
/// the picture under the panel showing the same buffer at the same width
/// cost one walk of it between them.
pub(super) fn with_peaks<T>(
    pcm: &Arc<SamplePcm>,
    width: usize,
    draw: impl FnOnce(&Peaks) -> T,
) -> T {
    PEAKS.with(|cell| {
        let mut slot = cell.borrow_mut();
        if !slot.as_ref().is_some_and(|p| p.is_for(pcm, width)) {
            *slot = Some(Peaks::reduce(pcm, width));
        }
        draw(slot.as_ref().expect("the reduction was just put there"))
    })
}

/// The column a frame falls in. `frames` is never zero here — a layer with
/// no frames has no region and never reaches either view.
fn column_of(frame: u64, frames: u64, width: usize) -> usize {
    ((frame.min(frames) * width as u64) / frames.max(1)) as usize
    // The exclusive end lands one past the last column; the caller clamps.
}

/// The waveform itself, `rows` tall.
///
/// Half blocks either side of the centre line, so the picture has twice the
/// vertical resolution a terminal row would otherwise give it, and the
/// centre row is always drawn: a silent passage is a line through the middle
/// rather than a gap, which is the difference between "quiet here" and "the
/// strip stopped drawing".
pub(super) fn wave_rows(
    peaks: &Peaks,
    rows: usize,
    region: (u64, u64),
    frames: u64,
    lit: Style,
    cut: Style,
) -> Vec<Line<'static>> {
    let width = peaks.columns.len();
    let centre = rows / 2;
    let half = (rows as f32 / 2.0).max(1.0);
    let scale = 1.0 / peaks.peak.max(QUIET_FLOOR);
    let start_col = column_of(region.0, frames, width);
    // The end is exclusive; the column holding the last playing frame is the
    // last lit one.
    let end_col = column_of(region.1.saturating_sub(1), frames, width);

    (0..rows)
        .map(|row| {
            let spans = (0..width)
                .map(|column| {
                    let (low, high) = peaks.columns[column];
                    let style = if column >= start_col && column <= end_col { lit } else { cut };
                    let reach = if row < centre {
                        (high * scale).max(0.0) * half - (centre - row) as f32
                    } else if row > centre {
                        (-low * scale).max(0.0) * half - (row - centre) as f32
                    } else {
                        // The centre row is the zero line and is always there.
                        1.0
                    };
                    let glyph = if reach >= 0.0 {
                        "\u{2588}"
                    } else if reach >= -0.5 {
                        // The half nearest the centre line, so the shape
                        // grows outward from it rather than floating.
                        if row < centre { "\u{2584}" } else { "\u{2580}" }
                    } else {
                        " "
                    };
                    Span::styled(glyph, style)
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

/// The ruler under the waveform, carrying the two markers.
pub(super) fn marker_row(
    region: (u64, u64),
    frames: u64,
    width: usize,
    lit: Style,
    cut: Style,
    mark: Style,
) -> Line<'static> {
    let start = column_of(region.0, frames, width).min(width.saturating_sub(1));
    let mut end = column_of(region.1.saturating_sub(1), frames, width).min(width.saturating_sub(1));
    // Two markers in one column would be one marker. Given a choice the end
    // moves, because the start is where the sound begins and is the one a
    // player is usually looking at.
    if end == start && width > 1 {
        end = if start + 1 < width { start + 1 } else { start - 1 };
    }
    let (left, right) = (start.min(end), start.max(end));

    let spans = (0..width)
        .map(|column| {
            if column == start {
                Span::styled("[", mark)
            } else if column == end {
                Span::styled("]", mark)
            } else if column > left && column < right {
                Span::styled("\u{2500}", lit)
            } else {
                Span::styled("\u{00b7}", cut)
            }
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

// ── The compact picture under the panel ──

/// The sound under the cursor, drawn small, for the rows under the knobs.
///
/// The owner's ask: a look at the recording without leaving the controls.
/// It is informational and nothing else — `t` is still the editor, and no
/// key on the pad map addresses it — so it carries exactly what a glance
/// needs: the whole buffer, the region that plays lit and the cut ends dark,
/// the two markers under them, and the word `rev` where the strip's header
/// puts it.
///
/// `None` when there is nothing to draw one of: a phrase row (notes, not a
/// waveform), a layer whose file has gone, or a pane too narrow to make a
/// picture out of. Those rows say what they already say.
///
/// The picture is indented to the column the knobs start in, so it lines up
/// under them rather than floating in the margin, and the gutter it leaves
/// is where the labels go — [`super::knobs::Panel::rows`]' own layout.
pub(super) fn mini_lines(map: &Map, width: usize) -> Option<Vec<Line<'static>>> {
    if width < INDENT + MINI_MIN_PICTURE {
        return None;
    }
    let PadRow::Layer(layer) = map.row()? else { return None };
    let pcm = layer.pcm.as_ref()?;
    let region = layer.region()?;
    let picture = width - INDENT;

    let lit = Style::default().fg(map.colour).bg(theme::bg_val());
    let cut = theme::dim();
    let mark = if map.focused { theme::amber_bright() } else { theme::amber() };

    let frames = pcm.frames();
    let mut rows = with_peaks(pcm, picture, |peaks| {
        wave_rows(peaks, MINI_ROWS - 1, region, frames, lit, cut)
    });
    rows.push(marker_row(region, frames, picture, lit, cut, mark));

    // The gutter, row by row: what the picture is, and — where the strip's
    // header would say it — that the region plays backwards.
    let labels: [(&str, Style); MINI_ROWS] = [
        ("wave", theme::dim()),
        (if layer.reverse { "rev" } else { "" }, theme::amber()),
        ("", theme::dim()),
    ];
    Some(
        rows.into_iter()
            .zip(labels)
            .map(|(row, (label, style))| {
                let mut spans =
                    vec![Span::styled(format!("{label:>w$} ", w = INDENT - 1), style)];
                spans.extend(row.spans);
                Line::from(spans)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::super::tests::{kit, map, ramp_kit};
    use super::*;

    /// How wide the picture is, given the pane the panel column has.
    fn picture_of(width: usize) -> usize {
        width - INDENT
    }

    /// The columns of one row, as characters. The glyphs are multi-byte, so
    /// a byte offset into the text is not a column.
    fn cells(line: &Line<'static>) -> Vec<char> {
        line.spans.iter().flat_map(|s| s.content.chars()).collect()
    }

    fn row_text(line: &Line<'static>) -> String {
        cells(line).into_iter().collect()
    }

    /// A layer with audio gets a picture: two rows of waveform and a ruler
    /// with both markers on it, indented under the knobs.
    #[test]
    fn an_audio_layer_draws_a_picture_with_its_markers() {
        let state = ramp_kit(44_100);
        let view = SamplerView::new();
        let lines = mini_lines(&map(&state, &view), 60).expect("no picture for a wav layer");
        assert_eq!(lines.len(), MINI_ROWS);
        for line in &lines {
            assert_eq!(cells(line).len(), 60, "the picture is not the width it was given");
        }
        // The gutter carries the word, and the picture starts where the
        // knobs do.
        let head = row_text(&lines[0]);
        assert!(head.starts_with("    wave "), "{head:?}");
        let ruler = cells(&lines[MINI_ROWS - 1]);
        assert!(ruler.contains(&'['), "no start marker: {ruler:?}");
        assert!(ruler.contains(&']'), "no end marker: {ruler:?}");
        // ...and the markers are inside the picture, not in the gutter.
        assert!(ruler.iter().position(|c| *c == '[').unwrap() >= INDENT);
    }

    /// The markers follow a trim, and the region between them is lit while
    /// the cut ends are not — the strip's own rule, at a third the height.
    #[test]
    fn the_region_is_lit_and_the_markers_follow_a_trim() {
        let mut state = ramp_kit(44_100);
        let pad = state.cursor;
        state.pads[pad].layers[0].start_frame = 11_025; // a quarter in
        state.pads[pad].layers[0].end_frame = 33_075; // three quarters
        let view = SamplerView::new();
        let width = INDENT + 80;
        let lines = mini_lines(&map(&state, &view), width).unwrap();

        let ruler = cells(&lines[MINI_ROWS - 1]);
        assert_eq!(ruler.iter().position(|c| *c == '['), Some(INDENT + 20));
        assert_eq!(ruler.iter().position(|c| *c == ']'), Some(INDENT + 59));

        // The centre row of the picture: lit inside the region, dim outside.
        let wave = &lines[MINI_ROWS - 2];
        assert_eq!(wave.spans.len(), picture_of(width) + 1, "the gutter is not one span");
        let column = |i: usize| wave.spans[i + 1].style;
        assert_eq!(column(0), theme::dim(), "a cut column is lit");
        assert_ne!(column(40), theme::dim(), "a playing column is dim");
        assert_eq!(column(79), theme::dim(), "the tail is lit");
    }

    /// A reversed layer says so where the strip's header says it.
    #[test]
    fn a_reversed_layer_wears_the_word_the_strip_uses() {
        let mut state = ramp_kit(44_100);
        let view = SamplerView::new();
        let plain = row_text(&mini_lines(&map(&state, &view), 60).unwrap()[1]);
        assert!(!plain.starts_with("     rev"), "{plain:?}");

        state.pads[state.cursor].layers[0].reverse = true;
        let shown = row_text(&mini_lines(&map(&state, &view), 60).unwrap()[1]);
        assert!(shown.starts_with("     rev "), "the picture does not say rev: {shown:?}");
    }

    /// A phrase row and a layer whose file has gone have no picture. Both
    /// already say what they are in the list; an empty waveform would read
    /// as the picture having broken.
    #[test]
    fn a_phrase_and_a_missing_file_have_no_picture() {
        let mut state = kit(); // layer 1's file is gone
        let mut view = SamplerView::new();
        view.layer = 1;
        assert!(mini_lines(&map(&state, &view), 60).is_none(), "a missing file drew a picture");

        let events = std::sync::Arc::from(vec![phosphor_plugin::sample::PhraseEvent {
            frame: 0,
            status: 0x90,
            data1: 60,
            data2: 100,
        }]);
        let pad = state.cursor;
        state.pads[pad].add_phrase(events, 44_100, 0.0, "pad").unwrap();
        view.layer = 2; // the phrase, after the kit's two layers
        assert!(mini_lines(&map(&state, &view), 60).is_none(), "a phrase drew a waveform");

        // ...and an empty pad has nothing to draw at all.
        state.cursor = 0;
        view.layer = 0;
        assert!(mini_lines(&map(&state, &view), 60).is_none());
    }

    /// Every width from nothing to a wall, and nothing runs past its own
    /// right edge or panics reaching for a column that is not there.
    #[test]
    fn the_picture_fits_whatever_width_it_is_given() {
        let state = ramp_kit(4_410);
        let view = SamplerView::new();
        for width in 0..200usize {
            let Some(lines) = mini_lines(&map(&state, &view), width) else { continue };
            assert!(width >= INDENT + MINI_MIN_PICTURE, "a picture at {width} columns");
            assert_eq!(lines.len(), MINI_ROWS);
            for line in &lines {
                assert_eq!(cells(line).len(), width, "a row of {width} columns overran");
            }
        }
    }

    /// Two views of one buffer cost one walk of it.
    ///
    /// The point of sharing the reduction rather than copying it: the strip
    /// and the picture under the panel go through one door
    /// ([`with_peaks`]), keyed by the buffer and the width, so a recording
    /// drawn at one width is decimated once however many views ask for it.
    #[test]
    fn one_reduction_serves_both_views() {
        let state = ramp_kit(44_100);
        let view = SamplerView::new();
        let mut open = SamplerView::new();
        open.trim = Some(crate::state::TrimView::default());
        let (panel, strip) = (map(&state, &view), map(&state, &open));
        // The picture under the panel at this pane width, and the strip at
        // the width that makes a picture of exactly the same columns.
        let panel_w = INDENT + 80;
        let before = reductions();
        let _ = mini_lines(&panel, panel_w).expect("a picture");
        assert_eq!(reductions(), before + 1, "the first draw did not reduce");
        let _ = super::super::strip::strip_lines(&strip, 80, 14).expect("a strip");
        assert_eq!(reductions(), before + 1, "the strip reduced a buffer already in the cache");
        // ...and drawing the small one again still costs nothing.
        let _ = mini_lines(&panel, panel_w).expect("a picture");
        assert_eq!(reductions(), before + 1, "the picture reduced twice");

        // A different width is a different reduction, which is the cache
        // being keyed rather than merely present.
        let _ = mini_lines(&panel, panel_w + 1).expect("a picture");
        assert_eq!(reductions(), before + 2);
    }
}
