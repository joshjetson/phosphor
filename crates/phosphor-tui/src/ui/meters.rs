//! The gain-reduction meter, drawn once and used everywhere.
//!
//! Two things reduce gain in this mixer — the compressor in an insert slot and
//! the safety limiter on the master — and they are drawn by the same code from
//! the same two atomics. A second implementation would drift, and a meter that
//! means one thing on one panel and something slightly different on another is
//! worse than no meter.
//!
//! # It reads downward, and it reads right to left
//!
//! Zero is the right-hand end, where an idle compressor sits, and the bar
//! grows *leftward* as gain comes off. That is the way every hardware
//! gain-reduction meter has ever been drawn, and it is the opposite of a level
//! meter — which is the point: one of them going up is good news and the other
//! going up is the compressor working harder.
//!
//! # The scale is not linear, and the first six decibels are half of it
//!
//! ```text
//!  0 dB  ────────────────────────────────────────────►  right edge
//! −6 dB  ───────────────────►                            half way
//! −20 dB ►                                               left edge
//! ```
//!
//! A linear 0..−20 scale spends its first half on 0..−10 dB, which is where
//! nothing interesting happens, and squeezes the whole difference between "two
//! decibels of glue" and "six decibels of glue" into a fifth of the bar. Those
//! four decibels are the entire working range of a bus compressor. So the
//! first 6 dB gets half the width and the remaining 14 dB gets the other half:
//! the region a mix decision is made in is the region the eye is given.
//!
//! # The cell is the transient
//!
//! The bar shows what is coming off now, after a 300 ms visual release; the
//! cell shows the worst moment in the last second and a half. Both come out of
//! the audio thread already ballistic — see `phosphor_core::fx::GrBallistics`
//! — because a UI on a redraw timer never sees the two-millisecond events that
//! a gain-reduction meter exists to show.

use super::*;

/// The bottom of the scale, −20 dB.
pub(crate) const GR_SPAN_DB: f32 = 20.0;

/// Where the scale bends: the first 6 dB gets half the width.
pub(crate) const GR_KNEE_DB: f32 = 6.0;

/// How much of the bar a reduction fills, from 0 at unity to 1 at the floor.
#[must_use]
pub(crate) fn gr_fill(db: f32) -> f32 {
    if db.is_nan() {
        return 0.0;
    }
    let reduction = (-db).clamp(0.0, GR_SPAN_DB);
    if reduction <= GR_KNEE_DB {
        0.5 * reduction / GR_KNEE_DB
    } else {
        0.5 + 0.5 * (reduction - GR_KNEE_DB) / (GR_SPAN_DB - GR_KNEE_DB)
    }
}

/// How many cells of a bar `width` wide a reduction lights.
///
/// Rounded up, so any reduction the meter is willing to name at all lights at
/// least one cell — the alternative wastes the bottom of the bar on the gap
/// between "something happened" and "enough happened to round to a cell".
#[must_use]
pub(crate) fn gr_cells(db: f32, width: usize) -> usize {
    let fill = gr_fill(db);
    if fill <= 0.0 || width == 0 {
        return 0;
    }
    ((fill * width as f32).ceil() as usize).min(width)
}

/// The peak cell, out ahead of the bar.
const GR_CELL: &str = "\u{2590}";
/// One lit cell of the bar.
const GR_LIT: &str = "\u{2588}";
/// One unlit cell.
const GR_DARK: &str = "\u{00b7}";

/// The bar, as characters. `width` cells, zero at the right.
///
/// The one place the layout is decided: the styled version below reads its
/// glyphs from here rather than repeating the arithmetic, so a test that
/// checks this string is checking what gets drawn. A meter whose only
/// description is a screenshot is a meter nobody checks.
#[must_use]
pub(crate) fn gr_bar_text(current_db: f32, peak_db: f32, width: usize) -> String {
    let lit = gr_cells(current_db, width);
    let held = gr_cells(peak_db, width);
    let mut out = String::with_capacity(width * 3);
    for x in 0..width {
        let from_right = width - x;
        out.push_str(if held > lit && from_right == held {
            GR_CELL
        } else if from_right <= lit {
            GR_LIT
        } else {
            GR_DARK
        });
    }
    out
}

/// The number, in one decimal place and five columns: `  0.0`, ` -6.2`,
/// `-18.4`.
#[must_use]
pub(crate) fn gr_readout(db: f32) -> String {
    let db = if db.is_finite() { db } else { 0.0 };
    // −0.04 rounds to `-0.0`, which reads as a fault rather than as silence.
    let db = if db > -0.05 { 0.0 } else { db };
    format!("{db:>5.1}")
}

/// The bar and its number, styled.
///
/// `label` is drawn ahead of it when there is one — `gr` on the compressor's
/// panel, `lim` in the top bar — and the whole thing greys out when nothing is
/// being taken off, so a meter at rest does not compete for the eye with one
/// that is working.
#[must_use]
pub(crate) fn gr_meter_spans(
    label: &str,
    current_db: f32,
    peak_db: f32,
    width: usize,
) -> Vec<Span<'static>> {
    let working = current_db <= -0.05;

    let mut spans = Vec::with_capacity(width + 4);
    if !label.is_empty() {
        spans.push(Span::styled(
            format!("{label} "),
            if working { theme::muted() } else { theme::dim() },
        ));
    }
    spans.push(Span::styled("\u{2595}", theme::dim()));
    for glyph in gr_bar_text(current_db, peak_db, width).chars() {
        let (text, style) = match glyph {
            '\u{2590}' => (GR_CELL, theme::amber_bright().add_modifier(Modifier::BOLD)),
            '\u{2588}' => (GR_LIT, theme::amber()),
            _ => (GR_DARK, theme::dim()),
        };
        spans.push(Span::styled(text, style));
    }
    spans.push(Span::styled("\u{258F}", theme::dim()));
    spans.push(Span::styled(
        format!(" {} dB", gr_readout(current_db)),
        if working {
            theme::normal().add_modifier(Modifier::BOLD)
        } else {
            theme::dim()
        },
    ));
    spans
}

/// Whether a warning indicator is on this half-second.
///
/// One clock for every blinking thing on the screen, taken from process start
/// rather than threaded through the renderer, so two indicators can never be
/// out of phase with each other. Half a second on, half a second off: fast
/// enough to catch the eye and slow enough not to be a strobe.
#[must_use]
pub(crate) fn blink_on() -> bool {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    (start.elapsed().as_millis() / 500) % 2 == 0
}

/// How wide the compressor panel's bar is drawn.
pub(crate) const GR_PANEL_WIDTH: usize = 24;

/// ...and the top bar's, where every column is spoken for.
pub(crate) const GR_COMPACT_WIDTH: usize = 8;

#[cfg(test)]
mod tests {
    use super::*;

    /// **The first six decibels are half the bar.** The whole reason the
    /// scale is not linear: two decibels of glue and six decibels of glue have
    /// to look different, and on a linear 0..−20 scale they do not.
    #[test]
    fn the_first_six_decibels_get_half_the_width() {
        assert!((gr_fill(0.0) - 0.0).abs() < 1.0e-6);
        assert!((gr_fill(-6.0) - 0.5).abs() < 1.0e-6);
        assert!((gr_fill(-20.0) - 1.0).abs() < 1.0e-6);
        assert!((gr_fill(-13.0) - 0.75).abs() < 1.0e-6, "halfway up the top half");
        // Past the floor it stops rather than running off the end.
        assert!((gr_fill(-60.0) - 1.0).abs() < 1.0e-6);
        // A NaN out of a stage that diverged reads as nothing.
        assert_eq!(gr_fill(f32::NAN), 0.0);

        // The scale really is expanded where the work happens: three decibels
        // of reduction is a quarter of the bar, and it would be an eighth of a
        // linear one.
        assert!(gr_fill(-3.0) > 3.0 / GR_SPAN_DB);
    }

    /// The bar grows leftward from a right-hand zero, and any reduction worth
    /// naming lights at least one cell.
    #[test]
    fn the_bar_reads_downward_from_the_right() {
        assert_eq!(gr_bar_text(0.0, 0.0, 8), "\u{00b7}".repeat(8));
        assert_eq!(gr_cells(-0.3, 24), 1, "a third of a decibel lit nothing");
        assert_eq!(gr_cells(-6.0, 24), 12, "six decibels is not half the bar");
        assert_eq!(gr_cells(-20.0, 24), 24);
        assert_eq!(gr_cells(-40.0, 24), 24, "the bar ran off the end");
        assert_eq!(gr_cells(-6.0, 0), 0);

        let bar = gr_bar_text(-6.0, -6.0, 8);
        assert_eq!(bar.chars().count(), 8);
        assert_eq!(&bar, "\u{00b7}\u{00b7}\u{00b7}\u{00b7}\u{2588}\u{2588}\u{2588}\u{2588}");
    }

    /// The peak cell sits out ahead of the bar, and rejoins it when it falls
    /// back. Without the cell a two-millisecond transient is a number nobody
    /// ever sees.
    #[test]
    fn the_peak_cell_stands_ahead_of_the_bar() {
        let bar = gr_bar_text(-3.0, -12.0, 12);
        let cells: Vec<char> = bar.chars().collect();
        assert_eq!(cells.len(), 12);
        let peak_at = 12 - gr_cells(-12.0, 12);
        assert_eq!(cells[peak_at], '\u{2590}', "the cell is not at the peak position");
        assert!(cells[11] == '\u{2588}', "the bar does not reach the zero end");
        assert!(cells[peak_at + 1] == '\u{00b7}', "the gap between cell and bar is filled");

        // Level with the bar, the cell disappears into it rather than
        // punching a hole in the fill.
        let bar = gr_bar_text(-6.0, -6.0, 12);
        assert!(!bar.contains('\u{2590}'));
    }

    /// **The widget, drawn.** Five states of the panel's 24-cell bar, written
    /// out so that a change to the scale, the glyphs or the peak cell has to
    /// be made here on purpose rather than noticed on screen a week later.
    ///
    /// ```text
    ///   0.0 dB   gr ▕························▏   0.0 dB
    ///  −1.5 dB   gr ▕·····················███▏  −1.5 dB
    ///  −3.0 dB   gr ▕··················██████▏  −3.0 dB   (a quarter, for three decibels)
    ///  −6.0 dB   gr ▕············████████████▏  −6.0 dB   (half the bar, six decibels)
    /// −20.0 dB   gr ▕████████████████████████▏ −20.0 dB
    /// ```
    #[test]
    fn the_bar_is_drawn_like_this() {
        const W: usize = GR_PANEL_WIDTH;
        let dot = "\u{00b7}";
        let lit = "\u{2588}";

        assert_eq!(gr_bar_text(0.0, 0.0, W), dot.repeat(24));
        // A decibel and a half is three cells of twenty-four, because the
        // first six decibels are half the bar.
        assert_eq!(gr_bar_text(-1.5, -1.5, W), format!("{}{}", dot.repeat(21), lit.repeat(3)));
        assert_eq!(gr_bar_text(-6.0, -6.0, W), format!("{}{}", dot.repeat(12), lit.repeat(12)));
        assert_eq!(gr_bar_text(-20.0, -20.0, W), lit.repeat(24));
        // ...and the cell out ahead of the bar, where a transient left it.
        assert_eq!(
            gr_bar_text(-3.0, -8.0, W),
            format!("{}\u{2590}{}{}", dot.repeat(10), dot.repeat(7), lit.repeat(6))
        );

        // The whole widget, label and number and rails.
        let drawn: String = gr_meter_spans("gr", -6.0, -6.0, W)
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(
            drawn,
            format!("gr \u{2595}{}{}\u{258F}  -6.0 dB", dot.repeat(12), lit.repeat(12))
        );
    }

    /// One decimal place, and a meter at rest reads `0.0` rather than `-0.0`.
    #[test]
    fn the_readout_is_one_decimal_place() {
        assert_eq!(gr_readout(0.0), "  0.0");
        assert_eq!(gr_readout(-0.01), "  0.0");
        assert_eq!(gr_readout(-6.25), " -6.2");
        assert_eq!(gr_readout(-18.44), "-18.4");
        assert_eq!(gr_readout(f32::NAN), "  0.0");
        assert_eq!(gr_readout(f32::NEG_INFINITY), "  0.0");
    }

    /// The styled version reads its glyphs from the text one, and wraps them
    /// in the rails, the label and the number.
    #[test]
    fn the_styled_bar_matches_the_text_one() {
        let spans = gr_meter_spans("gr", -6.0, -12.0, 12);
        let drawn: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(drawn.starts_with("gr \u{2595}"), "{drawn}");
        assert!(drawn.ends_with("-6.0 dB"), "{drawn}");
        // The label, the left rail, then one span per cell.
        let bar: String = spans[2..2 + 12].iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(bar, gr_bar_text(-6.0, -12.0, 12));

        // No label, no leading space.
        let spans = gr_meter_spans("", 0.0, 0.0, 8);
        let drawn: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(drawn.starts_with('\u{2595}'), "{drawn}");
    }
}
