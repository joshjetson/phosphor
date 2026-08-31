//! UI rendering: the effect chain, and the panels behind its slots.
//!
//! # Two views of one chain
//!
//! The narrow `[trk fx]` column on the left is the chain: six slots at most,
//! their types, and whether each one's bypass switch is thrown. Enter on a
//! slot opens its panel in the wide pane on the right — the same room the
//! instrument panel and the step grid use, and for the same reason: an
//! eight-band parametric does not fit in twenty-four columns and should not
//! be asked to.
//!
//! # The curve
//!
//! Drawn from the EQ's own closed-form response rather than from anything
//! this file knows about filters: [`eq_response_db`] builds the same design
//! the audio thread is running — at the same sample rate, which is why the
//! rate is threaded all the way here — and is asked for one point per column.
//! What is drawn is therefore what is heard, including every matched-design
//! correction that a hand-rolled bell drawing would have missed.
//!
//! Numbers first, curve second: the curve is what the panel gives up first
//! when the terminal is narrow, because a player can mix off the numbers and
//! cannot mix off a picture with no numbers under it.

use super::*;

use phosphor_app::fx::eq_from_natural_params;
use phosphor_app::state::{FxInstance, FxType, FxView};
use phosphor_dsp::fx::eq::{
    q_to_octaves, BandType, ParametricEq, PARAM_COUNT,
};

/// The narrowest panel that gets the response curve, in columns.
///
/// The wide layout puts eight bands side by side and a curve over them; below
/// this the bands become rows and the curve is dropped. It is the pane's
/// width, not the terminal's: the instrument column takes twenty-five of them
/// before this panel sees any.
pub(crate) const WIDE_PANEL: usize = 95;

/// Whether a pane this wide draws the wide layout.
///
/// Asked by the renderer to lay the panel out and by the key handler to know
/// which way `h`/`l` point, because the cursor moves the way the screen looks
/// and there is only one screen.
#[must_use]
pub(crate) fn is_wide(pane_width: usize) -> bool {
    pane_width >= WIDE_PANEL
}

// ── The chain ──

/// The slot list in the narrow column.
pub(super) fn render_fx_chain(frame: &mut Frame, area: Rect, nav: &NavState, focused: bool) {
    let (w, h) = (area.width as usize, area.height as usize);
    if w == 0 || h == 0 {
        return;
    }
    let chain: &[FxInstance] = nav
        .current_track()
        .map(|t| t.fx_chain.as_slice())
        .unwrap_or(&[]);

    let mut lines: Vec<Line> = Vec::new();
    if chain.is_empty() {
        lines.push(Line::from(Span::styled("  (no fx)", theme::dim())));
        lines.push(Line::from(Span::styled("  a \u{2014} add one", theme::muted())));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  works on tracks,", theme::dim())));
        lines.push(Line::from(Span::styled("  buses and master", theme::dim())));
    } else {
        let cursor = nav.clip_view.fx_cursor.min(chain.len() - 1);
        for (index, slot) in chain.iter().enumerate() {
            let here = focused && cursor == index;
            let open = nav.clip_view.fx.slot == Some(index);
            let style = if here {
                theme::amber_bright().add_modifier(Modifier::BOLD)
            } else if slot.is_active() {
                theme::normal()
            } else {
                theme::dim()
            };
            lines.push(Line::from(vec![
                Span::styled(if here { " \u{25B6} " } else { "   " }, style),
                // The switch, as a switch: a filled dot is an effect in the
                // signal path and a hollow one is an effect standing beside
                // it. The word is there too, because a glyph on its own is a
                // thing a player has to have been told about.
                Span::styled(
                    if slot.is_active() { "\u{25CF} " } else { "\u{25CB} " },
                    if slot.is_active() { style } else { theme::dim() },
                ),
                Span::styled(format!("{:<6}", slot.fx_type.label()), style),
                Span::styled(
                    if slot.is_active() { "" } else { "byp" },
                    theme::dim(),
                ),
                Span::styled(if open { "\u{25B8}" } else { "" }, theme::amber()),
            ]));
        }
        if chain.len() < phosphor_core::fx::MAX_FX_SLOTS {
            lines.push(Line::from(Span::styled("   a \u{2014} add", theme::dim())));
        }
    }

    if focused {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  enter open  b bypass", theme::dim())));
        lines.push(Line::from(Span::styled("  [ ] order  d remove", theme::dim())));
    }
    lines.truncate(h);
    frame.render_widget(Paragraph::new(lines), area);
}

// ── The panel ──

/// One effect's panel, in the wide pane.
pub(super) fn render_fx_panel(frame: &mut Frame, area: Rect, nav: &NavState) {
    let (w, h) = (area.width as usize, area.height as usize);
    if w == 0 || h == 0 {
        return;
    }
    let Some(track) = nav.current_track() else { return };
    let Some(index) = nav.clip_view.fx.slot else {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  no effect open \u{2014} enter on a slot in [trk fx]",
                theme::dim(),
            )),
            area,
        );
        return;
    };
    let Some(slot) = track.fx_chain.get(index) else { return };

    match slot.fx_type {
        FxType::Eq => render_eq(frame, area, nav, slot, index),
        other => {
            // Every panel lands here first. Saying which effect has none yet
            // is the whole of what "the menu does not lie" costs.
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        format!("  {} \u{2014} slot {}", other.label(), index + 1),
                        theme::normal().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "  this effect has no panel yet",
                        theme::dim(),
                    )),
                ]),
                area,
            );
        }
    }
}

// ── The eight-band parametric ──

/// Decibels the curve spans, above and below unity.
const CURVE_DB: f64 = 18.0;
/// The frequency window the curve draws, in hertz.
const CURVE_LOW_HZ: f64 = 20.0;
const CURVE_HIGH_HZ: f64 = 20_000.0;
/// Rows the curve needs before it is worth drawing at all.
const CURVE_MIN_ROWS: usize = 5;
/// How far from flat the selected band has to be before its own trace is
/// drawn over the composite. Below this it is doing nothing there, and
/// lighting the flat line either side of a bell picks out nothing at all.
const SOLO_FLOOR_DB: f64 = 0.25;

/// A frequency, the way an EQ says it: `30`, `2.5k`, `18k` — never `2487`.
fn hz_label(hz: f32) -> String {
    let hz = f64::from(hz);
    if hz >= 10_000.0 {
        format!("{:.0}k", hz / 1000.0)
    } else if hz >= 1000.0 {
        let k = hz / 1000.0;
        if (k - k.round()).abs() < 0.05 {
            format!("{k:.0}k")
        } else {
            format!("{k:.1}k")
        }
    } else {
        format!("{hz:.0}")
    }
}

/// The controls of one band, as text, and whether each one does anything on
/// this band type.
///
/// The greying is the band type's own answer — [`BandType::uses_gain`] and
/// friends — so a control that reads as inert here is inert in the filter
/// too, and the key handler refuses to move it for the same reason.
struct BandCells {
    text: [String; FxView::CONTROLS],
    live: [bool; FxView::CONTROLS],
}

fn band_cells(params: &[f32], band: usize) -> BandCells {
    let at = |control: usize| params.get(band * FxView::CONTROLS + control).copied().unwrap_or(0.0);
    let ty = BandType::from_index(at(0) as usize);
    let on = at(5) >= 0.5;
    BandCells {
        text: [
            ty.short_name().to_string(),
            hz_label(at(1)),
            if ty.uses_gain() { format!("{:+.1}", at(2)) } else { "\u{2014}".into() },
            if ty.uses_q() { format!("{:.2}", at(3)) } else { "\u{2014}".into() },
            if ty.uses_slope() { format!("{:.0}", at(4)) } else { "\u{2014}".into() },
            if on { "\u{25CF}".into() } else { "\u{00b7}".into() },
        ],
        live: [true, true, ty.uses_gain(), ty.uses_q(), ty.uses_slope(), true],
    }
}

/// The line that always survives: what the band under the cursor is doing,
/// spelled out.
fn eq_readout(params: &[f32], view: &FxView, slot_index: usize) -> Line<'static> {
    let mut spans = vec![
        Span::styled(" eq ", theme::amber_bright().add_modifier(Modifier::BOLD)),
        Span::styled(format!("slot {} ", slot_index + 1), theme::dim()),
    ];
    if view.band >= FxView::TRIM {
        let trim = params.get(PARAM_COUNT - 1).copied().unwrap_or(0.0);
        spans.push(Span::styled("\u{00b7} output trim ", theme::muted()));
        spans.push(Span::styled(format!("{trim:+.1} dB"), theme::normal()));
        return Line::from(spans);
    }

    let at = |control: usize| {
        params
            .get(view.band * FxView::CONTROLS + control)
            .copied()
            .unwrap_or(0.0)
    };
    let ty = BandType::from_index(at(0) as usize);
    let on = at(5) >= 0.5;
    spans.push(Span::styled(
        format!("\u{00b7} band {} ", view.band + 1),
        theme::muted(),
    ));
    spans.push(Span::styled(
        format!("{} {} ", ty.short_name(), hz_label(at(1))),
        if on { theme::normal().add_modifier(Modifier::BOLD) } else { theme::dim() },
    ));
    if ty.uses_gain() {
        spans.push(Span::styled(format!("{:+.1} dB ", at(2)), theme::normal()));
    }
    if ty.uses_q() {
        // Q means nothing to most people and octaves mean something to
        // everyone who has ever swept one.
        spans.push(Span::styled(
            format!("Q {:.2} ({:.1} oct) ", at(3), q_to_octaves(f64::from(at(3)))),
            theme::muted(),
        ));
    }
    if ty.uses_slope() {
        spans.push(Span::styled(format!("{:.0} dB/oct ", at(4)), theme::muted()));
    }
    if !on {
        spans.push(Span::styled("\u{00b7} off", theme::dim()));
    }
    Line::from(spans)
}

/// The response curve, in braille.
///
/// One dot column per half-cell across a log frequency axis, and the drawing
/// is clipped rather than the data: a band pushed past ±18 dB flattens
/// against the top of the window and its number goes on reading what it is.
fn curve_lines(
    params: &[f32],
    sample_rate: f64,
    view: &FxView,
    width: usize,
    rows: usize,
) -> Vec<Line<'static>> {
    const LABEL: usize = 5;
    let plot_w = width.saturating_sub(LABEL);
    if plot_w < 8 || rows < CURVE_MIN_ROWS {
        return Vec::new();
    }
    let dots_x = plot_w * 2;
    let dots_y = rows * 4;

    // The whole EQ, and the band under the cursor on its own. Two designs
    // rather than one: the highlight is *that band's* contribution, which is
    // not something the composite curve can be asked for.
    let composite = eq_from_natural_params(params, sample_rate);
    let solo = (view.band < FxView::TRIM)
        .then(|| then_solo(params, view.band, sample_rate))
        .flatten();

    let mut cells: Vec<Vec<(char, Style)>> = (0..rows)
        .map(|_| vec![(' ', theme::bg()); width])
        .collect();

    // Gridlines first, so the curve draws over them.
    for (db, label) in [(12.0, "+12"), (6.0, " +6"), (0.0, "  0"), (-6.0, " -6"), (-12.0, "-12")] {
        let Some(row) = db_row(db, rows) else { continue };
        let style = if db == 0.0 {
            Style::default().fg(theme::grid_major()).bg(theme::bg_val())
        } else {
            Style::default().fg(theme::grid_minor()).bg(theme::bg_val())
        };
        let label_style = if db == 0.0 { theme::muted() } else { theme::dim() };
        for (i, ch) in format!("{label} ").chars().enumerate() {
            if i < LABEL {
                cells[row][i] = (ch, label_style);
            }
        }
        for x in LABEL..width {
            cells[row][x] = (if db == 0.0 { '\u{00b7}' } else { '\u{2508}' }, style);
        }
    }

    // The band's own frequency, as a vertical cursor.
    if view.band < FxView::TRIM {
        let hz = f64::from(params.get(view.band * FxView::CONTROLS + 1).copied().unwrap_or(1000.0));
        if let Some(x) = hz_column(hz, plot_w) {
            for row in cells.iter_mut() {
                let cell = &mut row[LABEL + x];
                *cell = (
                    if cell.0 == ' ' { '\u{2502}' } else { cell.0 },
                    Style::default().fg(theme::amber_val()).bg(theme::bg_val()),
                );
            }
        }
    }

    // The curve, as braille dots gathered into cells.
    let mut dots: Vec<u8> = vec![0; plot_w * rows];
    let mut lit: Vec<bool> = vec![false; plot_w * rows];
    for dx in 0..dots_x {
        let hz = curve_hz(dx, dots_x);
        plot_dot(&composite, hz, dx, dots_y, plot_w, &mut dots, &mut lit, false);
        // The band's own trace, drawn only where the band does something.
        // A band is flat outside its own region, and a highlight that
        // followed it there would light the whole curve and pick nothing
        // out — which is what "the selected band highlighted" cannot mean.
        if let Some(solo) = solo.as_ref() {
            if solo.response_db(hz).abs() > SOLO_FLOOR_DB {
                plot_dot(solo, hz, dx, dots_y, plot_w, &mut dots, &mut lit, true);
            }
        }
    }
    for (index, &bits) in dots.iter().enumerate() {
        if bits == 0 {
            continue;
        }
        let (row, column) = (index / plot_w, index % plot_w);
        let glyph = char::from_u32(0x2800 + u32::from(bits)).unwrap_or('\u{2807}');
        cells[row][LABEL + column] = (
            glyph,
            if lit[index] {
                theme::amber_bright().add_modifier(Modifier::BOLD)
            } else {
                theme::normal()
            },
        );
    }

    let mut lines = grid_to_lines(cells);

    // Decade ticks, under the plot.
    let mut ticks = vec![(' ', theme::bg()); width];
    for (hz, label) in [(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
        if let Some(x) = hz_column(hz, plot_w) {
            for (i, ch) in label.chars().enumerate() {
                let at = LABEL + x + i;
                if at < width {
                    ticks[at] = (ch, theme::dim());
                }
            }
        }
    }
    lines.extend(grid_to_lines(vec![ticks]));
    lines
}

/// The frequency at a dot column, log-spaced across the window.
fn curve_hz(dot_x: usize, dots_x: usize) -> f64 {
    let t = dot_x as f64 / (dots_x.max(2) - 1) as f64;
    CURVE_LOW_HZ * (CURVE_HIGH_HZ / CURVE_LOW_HZ).powf(t)
}

/// Which cell column a frequency falls in, or `None` when it is outside the
/// window the curve draws.
fn hz_column(hz: f64, plot_w: usize) -> Option<usize> {
    if !(CURVE_LOW_HZ..=CURVE_HIGH_HZ).contains(&hz) || plot_w == 0 {
        return None;
    }
    let t = (hz / CURVE_LOW_HZ).ln() / (CURVE_HIGH_HZ / CURVE_LOW_HZ).ln();
    Some(((t * (plot_w - 1) as f64).round() as usize).min(plot_w - 1))
}

/// The text row a gridline sits on.
fn db_row(db: f64, rows: usize) -> Option<usize> {
    let t = (CURVE_DB - db) / (2.0 * CURVE_DB);
    let dot = (t * (rows * 4 - 1) as f64).round() as usize;
    (dot < rows * 4).then_some(dot / 4)
}

/// Set the braille dot for one column of one curve.
#[allow(clippy::too_many_arguments)]
fn plot_dot(
    eq: &ParametricEq,
    hz: f64,
    dot_x: usize,
    dots_y: usize,
    plot_w: usize,
    dots: &mut [u8],
    lit: &mut [bool],
    highlight: bool,
) {
    let db = eq.response_db(hz);
    if !db.is_finite() {
        return;
    }
    // Clipped, not clamped away: a band driven past the window flattens on
    // the edge of the drawing and its number keeps saying what it is.
    let t = ((CURVE_DB - db) / (2.0 * CURVE_DB)).clamp(0.0, 1.0);
    let dot_y = ((t * (dots_y - 1) as f64).round() as usize).min(dots_y - 1);
    let (column, row) = (dot_x / 2, dot_y / 4);
    let index = row * plot_w + column;
    if index >= dots.len() {
        return;
    }
    let (dx, dy) = (dot_x % 2, dot_y % 4);
    let bit = if dy < 3 { dx * 3 + dy } else { 6 + dx };
    dots[index] |= 1 << bit;
    if highlight {
        lit[index] = true;
    }
}

/// The selected band on its own, for the highlighted trace — every other band
/// switched off in a copy of the parameters.
fn then_solo(params: &[f32], band: usize, sample_rate: f64) -> Option<ParametricEq> {
    let mut solo: Vec<f32> = params.to_vec();
    solo.resize(PARAM_COUNT, 0.0);
    let on = solo[band * FxView::CONTROLS + 5] >= 0.5;
    if !on {
        return None;
    }
    for other in 0..FxView::TRIM {
        if other != band {
            solo[other * FxView::CONTROLS + 5] = 0.0;
        }
    }
    solo[PARAM_COUNT - 1] = 0.0;
    Some(eq_from_natural_params(&solo, sample_rate))
}

/// The EQ's panel: the readout, the curve if there is room for it, and the
/// eight bands.
///
/// # What goes when there is no room
///
/// The curve first, then the slope row, then Q, then the frequencies. Never
/// the gain column and never the readout line: a player can mix with numbers
/// and no picture, and cannot mix with a picture and no numbers.
fn render_eq(frame: &mut Frame, area: Rect, nav: &NavState, slot: &FxInstance, index: usize) {
    let (w, h) = (area.width as usize, area.height as usize);
    let view = &nav.clip_view.fx;
    let focused = nav.focused_pane == Pane::ClipView
        && nav.clip_view.focus == ClipViewFocus::PianoRoll
        && nav.clip_view.clip_tab == ClipTab::Fx;
    let params = &slot.params;
    let rate = f64::from(nav.sample_rate.max(8_000));

    let mut lines: Vec<Line> = vec![eq_readout(params, view, index)];
    if slot.bypass {
        lines.push(Line::from(Span::styled(
            "  bypassed \u{2014} b puts it back in the signal path",
            theme::dim(),
        )));
    }

    if is_wide(w) {
        // Rows the strip needs: the band numbers, then one per control, then
        // the trim.
        let strip = 1 + FxView::CONTROLS + 1;
        let spare = h.saturating_sub(lines.len() + strip);
        if spare > CURVE_MIN_ROWS {
            lines.extend(curve_lines(params, rate, view, w, spare.saturating_sub(1).min(10)));
        }
        lines.extend(wide_strip(params, view, w, focused));
    } else {
        lines.extend(narrow_strip(params, view, w, h.saturating_sub(lines.len()), focused));
    }

    lines.truncate(h);
    frame.render_widget(Paragraph::new(lines), area);
}

/// The control names, in the EQ's own order.
const CONTROL_NAMES: [&str; FxView::CONTROLS] = ["type", "freq", "gain", "q", "slope", "on"];

/// The style one cell of the strip takes.
fn cell_style(selected: bool, locked: bool, live: bool, focused: bool) -> Style {
    if selected && locked {
        // Held reads as inverse video in the theme's own colours, exactly as
        // a held knob does everywhere else in the application.
        Style::default()
            .fg(theme::bg_val())
            .bg(theme::amber_bright_val())
            .add_modifier(Modifier::BOLD)
    } else if selected && focused {
        theme::amber_bright().add_modifier(Modifier::BOLD)
    } else if !live {
        // A control this band type does not use. Greyed here and refused by
        // the keys, which is the same fact said twice on purpose.
        theme::dim()
    } else {
        theme::normal()
    }
}

/// Eight bands as columns, one control per row.
fn wide_strip(params: &[f32], view: &FxView, width: usize, focused: bool) -> Vec<Line<'static>> {
    const LABEL: usize = 6;
    let cells: Vec<BandCells> = (0..FxView::TRIM).map(|b| band_cells(params, b)).collect();
    let columns = FxView::TRIM + 1; // the eight bands and the trim
    let cell_w = ((width.saturating_sub(LABEL)) / columns).clamp(5, 9);

    let mut rows: Vec<Line> = Vec::new();
    let mut header = vec![Span::styled(format!("{:<LABEL$}", "band"), theme::dim())];
    for band in 0..FxView::TRIM {
        let here = view.band == band;
        header.push(Span::styled(
            format!("{:<cell_w$}", band + 1),
            if here && focused { theme::amber_bright().add_modifier(Modifier::BOLD) } else { theme::dim() },
        ));
    }
    header.push(Span::styled(
        "trim".to_string(),
        if view.band >= FxView::TRIM && focused {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else {
            theme::dim()
        },
    ));
    rows.push(Line::from(header));

    for (control, name) in CONTROL_NAMES.iter().enumerate() {
        let mut line = vec![Span::styled(format!("{name:<LABEL$}"), theme::dim())];
        for (band, cell) in cells.iter().enumerate() {
            let selected = view.band == band && view.control == control;
            line.push(Span::styled(
                format!("{:<cell_w$}", cell.text[control]),
                cell_style(selected, view.locked, cell.live[control], focused),
            ));
        }
        // The trim sits on the gain row: it is a gain, and giving it a row of
        // its own would spend one on a single number.
        if control == 2 {
            let trim = params.get(PARAM_COUNT - 1).copied().unwrap_or(0.0);
            line.push(Span::styled(
                format!("{trim:+.1}"),
                cell_style(view.band >= FxView::TRIM, view.locked, true, focused),
            ));
        }
        rows.push(Line::from(line));
    }
    rows
}

/// Eight bands as rows, controls across. What an eighty-column terminal gets.
fn narrow_strip(
    params: &[f32],
    view: &FxView,
    width: usize,
    height: usize,
    focused: bool,
) -> Vec<Line<'static>> {
    // Columns go in the order the contract sets: slope first, then Q, then
    // freq. The gain column never goes.
    let mut shown: Vec<usize> = vec![0, 1, 2, 3, 4, 5];
    let cell_w = 7usize;
    let label = 4usize;
    while label + shown.len() * cell_w > width && shown.len() > 3 {
        // 4 = slope, 3 = q, 1 = freq
        let drop = if shown.contains(&4) {
            4
        } else if shown.contains(&3) {
            3
        } else {
            1
        };
        shown.retain(|&c| c != drop);
    }

    let mut rows: Vec<Line> = Vec::new();
    let mut header = vec![Span::styled(format!("{:<label$}", ""), theme::dim())];
    for &control in &shown {
        header.push(Span::styled(
            format!("{:<cell_w$}", CONTROL_NAMES[control]),
            theme::dim(),
        ));
    }
    rows.push(Line::from(header));

    // The band under the cursor is always on the screen, whatever else is
    // not: the rows scroll under it rather than being cut off.
    let visible = height.saturating_sub(2).max(1);
    let first = view.band.saturating_sub(visible.saturating_sub(1)).min(FxView::TRIM);
    for band in first..=FxView::TRIM.min(first + visible.saturating_sub(1)) {
        if band >= FxView::TRIM {
            let trim = params.get(PARAM_COUNT - 1).copied().unwrap_or(0.0);
            rows.push(Line::from(vec![
                Span::styled("trim".to_string(), theme::dim()),
                Span::styled(
                    format!("{trim:+.1}"),
                    cell_style(view.band >= FxView::TRIM, view.locked, true, focused),
                ),
            ]));
            continue;
        }
        let cell = band_cells(params, band);
        let here = view.band == band;
        let mut line = vec![Span::styled(
            format!("{:<label$}", format!("{}{}", if here { "\u{25B8}" } else { " " }, band + 1)),
            if here && focused { theme::amber_bright() } else { theme::dim() },
        )];
        for &control in &shown {
            line.push(Span::styled(
                format!("{:<cell_w$}", cell.text[control]),
                cell_style(here && view.control == control, view.locked, cell.live[control], focused),
            ));
        }
        rows.push(Line::from(line));
    }
    rows
}
