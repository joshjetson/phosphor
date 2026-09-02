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
use phosphor_dsp::fx::delay::{
    natural_param as delay_param, synced_seconds, uses as delay_uses, HEAD_LABELS, SYNC_LABELS,
    PARAM_COUNT as DELAY_PARAMS, PARAM_DIVISION, PARAM_FEEDBACK, PARAM_FREEZE, PARAM_HEADS,
    PARAM_HIGH_CUT_HZ as DELAY_HIGH_CUT, PARAM_LOW_CUT_HZ as DELAY_LOW_CUT, PARAM_MODE,
    PARAM_OFFSET, PARAM_ROUTING, PARAM_SYNC, PARAM_TIME_MODE, PARAM_TIME_MS,
};
use phosphor_dsp::fx::tape::{
    auto_makeup_db, azimuth_hz, bump_hz, flutter_percent, hiss_dbfs, loss_hz,
    natural_param as tape_param, uses as tape_uses, wow_percent,
    PARAM_AUTO_MAKEUP as TAPE_AUTO_MAKEUP, PARAM_AZIMUTH_DEG as TAPE_AZIMUTH,
    PARAM_BUMP_DB as TAPE_BUMP_DB, PARAM_COUNT as TAPE_PARAMS, PARAM_FLUTTER as TAPE_FLUTTER,
    PARAM_HISS as TAPE_HISS, PARAM_SPEED as TAPE_SPEED, PARAM_TRIM_DB as TAPE_TRIM,
    PARAM_WOW as TAPE_WOW,
};
use phosphor_dsp::fx::reverb::{
    natural_param as reverb_param, Algorithm, PARAM_ALGORITHM, PARAM_COUNT as REVERB_PARAMS,
    PARAM_DAMP_HZ, PARAM_DECAY_S, PARAM_LOW_CUT_HZ, PARAM_MOD_RATE_HZ, PARAM_PREDELAY_MS,
    PARAM_SIZE,
};
use phosphor_dsp::fx::compressor::{
    auto_makeup_for, auto_release_of, character_name, matches_character,
    natural_param as comp_param, ratio_label, sense_of, uses as comp_param_uses, AutoRelease,
    PARAM_ATTACK_MS, PARAM_AUTO_MAKEUP, PARAM_AUTO_RELEASE, PARAM_CHARACTER,
    PARAM_COUNT as COMP_PARAMS, PARAM_KNEE_DB, PARAM_MAKEUP_DB, PARAM_MIX as COMP_MIX,
    PARAM_RATIO, PARAM_RELEASE_MS, PARAM_SC_HPF_HZ, PARAM_SENSE, PARAM_THRESHOLD_DB,
    SC_HPF_MIN_HZ,
};

use super::meters::{gr_meter_spans, GR_PANEL_WIDTH};

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
    let midi: &[crate::state::MidiFxInstance] = nav
        .current_track()
        .map(|t| t.midi_fx.as_slice())
        .unwrap_or(&[]);

    let mut lines: Vec<Line> = Vec::new();
    if chain.is_empty() && midi.is_empty() {
        lines.push(Line::from(Span::styled("  (no fx)", theme::dim())));
        lines.push(Line::from(Span::styled("  a \u{2014} add one", theme::muted())));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  works on tracks,", theme::dim())));
        lines.push(Line::from(Span::styled("  buses and master", theme::dim())));
    } else {
        let total = midi.len() + chain.len();
        let cursor = nav.clip_view.fx_cursor.min(total - 1);
        // The MIDI rack leads, drawn the same way with a marker: these run
        // on the notes, before the instrument.
        for (index, slot) in midi.iter().enumerate() {
            let here = focused && cursor == index;
            let open = nav.clip_view.fx.midi_slot == Some(index);
            let style = if here {
                theme::amber_bright().add_modifier(Modifier::BOLD)
            } else if slot.is_active() {
                theme::normal()
            } else {
                theme::dim()
            };
            lines.push(Line::from(vec![
                Span::styled(if here { " \u{25B6} " } else { "   " }, style),
                Span::styled(
                    if slot.is_active() { "\u{25CF} " } else { "\u{25CB} " },
                    if slot.is_active() { style } else { theme::dim() },
                ),
                Span::styled(format!("{:<6}", slot.fx_type.label()), style),
                Span::styled(
                    if slot.is_active() { "midi" } else { "byp" },
                    theme::dim(),
                ),
                Span::styled(if open { "\u{25B8}" } else { "" }, theme::amber()),
            ]));
        }
        for (index, slot) in chain.iter().enumerate() {
            let index_all = midi.len() + index;
            let here = focused && cursor == index_all;
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


/// A MIDI effect's panel: the knob list, one row per control, the value in
/// its own words where the number names a thing rather than measures one.
fn render_midi_fx_panel(
    frame: &mut Frame,
    area: Rect,
    nav: &NavState,
    track: &crate::state::TrackState,
    slot: usize,
) {
    let Some(instance) = track.midi_fx.get(slot) else { return };
    let fx_type = instance.fx_type;
    let cursor = nav.clip_view.fx.band;
    let locked = nav.clip_view.fx.locked;

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(format!("  {} ", fx_type.label()), theme::amber_bright().add_modifier(Modifier::BOLD)),
        Span::styled("\u{00b7} midi \u{00b7} plays live and on playback", theme::dim()),
        Span::styled(if instance.bypass { "  \u{00b7} bypassed" } else { "" }, theme::dim()),
    ]));
    lines.push(Line::from(""));

    for (row, info) in fx_type.params().iter().enumerate() {
        let here = cursor == row;
        let value = instance.params.get(row).copied().unwrap_or(info.default);
        let shown = fx_type.value_text(row, value);
        let style = if here && locked {
            theme::amber_bright().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else if here {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else {
            theme::normal()
        };
        lines.push(Line::from(vec![
            Span::styled(if here { " \u{25B6} " } else { "   " }, style),
            Span::styled(format!("{:<8}", info.name), style),
            Span::styled(shown, style),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  j/k knob \u{00b7} h/l adjust \u{00b7} b bypass \u{00b7} esc back",
        theme::dim(),
    )));

    lines.truncate(area.height as usize);
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
    if let Some(mslot) = nav.clip_view.fx.midi_slot {
        render_midi_fx_panel(frame, area, nav, track, mslot);
        return;
    }
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

    // **Exhaustive, and it is the point.** There is no catch-all arm any
    // more: the day a sixth effect joins the menu this match stops compiling
    // until it has a panel, which is a better guarantee than a fallback that
    // apologises at run time.
    match slot.fx_type {
        FxType::Eq => render_eq(frame, area, nav, slot, index),
        FxType::Compressor => render_comp(frame, area, nav, slot, index),
        FxType::Reverb => render_reverb(frame, area, nav, slot, index),
        FxType::Delay => render_delay(frame, area, nav, slot, index),
        FxType::Tape => render_tape(frame, area, nav, slot, index),
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

// ── The reverb ──

/// The reverb's twelve controls, as a column of knobs.
///
/// No curve. A reverb's response is a decay in *time*, and the honest picture
/// of one is an energy-decay curve that would need an impulse response to
/// draw and a second of audio to update — which is a meter, not a control.
/// So the panel is the numbers, and the numbers are the ones the effect
/// declares: name, value and unit, straight from
/// [`phosphor_dsp::fx::reverb::natural_param`], so a range that moves cannot
/// leave a stale copy here.
fn render_reverb(frame: &mut Frame, area: Rect, nav: &NavState, slot: &FxInstance, index: usize) {
    let (w, h) = (area.width as usize, area.height as usize);
    let view = &nav.clip_view.fx;
    let focused = nav.focused_pane == Pane::ClipView
        && nav.clip_view.focus == ClipViewFocus::PianoRoll
        && nav.clip_view.clip_tab == ClipTab::Fx;
    let params = &slot.params;
    let algorithm = reverb_algorithm(params);
    let cursor = view.band.min(REVERB_PARAMS - 1);

    let mut lines: Vec<Line> = vec![reverb_readout(params, cursor, index)];
    if slot.bypass {
        lines.push(Line::from(Span::styled(
            "  bypassed \u{2014} b puts it back in the signal path",
            theme::dim(),
        )));
    }
    lines.push(Line::from(""));

    // Two columns where there is room, one where there is not. The list is
    // twelve entries either way: what changes is how many rows it costs.
    let columns = if is_wide(w) && h >= 8 { 2 } else { 1 };
    let rows = REVERB_PARAMS.div_ceil(columns);
    let visible = h.saturating_sub(lines.len() + 1).max(1);
    let first_row = if columns == 1 {
        cursor.saturating_sub(visible.saturating_sub(1)).min(rows.saturating_sub(1))
    } else {
        0
    };

    for row in first_row..rows {
        if lines.len() + 1 > h {
            break;
        }
        let mut spans: Vec<Span> = Vec::new();
        for column in 0..columns {
            let control = column * rows + row;
            if control >= REVERB_PARAMS {
                continue;
            }
            let here = cursor == control;
            let live = algorithm.uses(control);
            let name = reverb_param(control).map_or("", |p| p.name);
            spans.push(Span::styled(
                format!("{}{name:<7}", if here { "\u{25B8}" } else { " " }),
                if here && focused { theme::amber_bright() } else { theme::dim() },
            ));
            spans.push(Span::styled(
                format!("{:<11}", reverb_value(params, control)),
                cell_style(here, view.locked, live, focused),
            ));
            if column + 1 < columns {
                spans.push(Span::styled("  ", theme::dim()));
            }
        }
        lines.push(Line::from(spans));
    }

    if lines.len() < h {
        lines.push(Line::from(Span::styled(
            if view.locked {
                "  held \u{00b7} h/l adjusts \u{00b7} H/L strides \u{00b7} esc lets go"
            } else {
                "  j/k picks \u{00b7} h/l adjusts \u{00b7} H/L strides \u{00b7} enter holds"
            },
            theme::dim(),
        )));
    }

    lines.truncate(h);
    frame.render_widget(Paragraph::new(lines), area);
}

/// The algorithm a parameter vector names.
pub(crate) fn reverb_algorithm(params: &[f32]) -> Algorithm {
    Algorithm::from_index(
        params.get(PARAM_ALGORITHM).copied().unwrap_or(0.0).round().max(0.0) as usize,
    )
}

/// One control, in the unit a person reads it in.
pub(crate) fn reverb_value(params: &[f32], control: usize) -> String {
    let value = params.get(control).copied().unwrap_or(0.0);
    match control {
        PARAM_ALGORITHM => reverb_algorithm(params).label().to_string(),
        PARAM_PREDELAY_MS => format!("{value:.0} ms"),
        // Two decimals under a second and one above it: the difference
        // between 0.45 and 0.50 is audible and the difference between 8.0 and
        // 8.05 is not.
        PARAM_DECAY_S => {
            if value < 1.0 {
                format!("{value:.2} s")
            } else {
                format!("{value:.1} s")
            }
        }
        PARAM_SIZE => format!("{value:.2}"),
        PARAM_DAMP_HZ | PARAM_LOW_CUT_HZ => format!("{} Hz", hz_label(value)),
        PARAM_MOD_RATE_HZ => format!("{value:.2} Hz"),
        _ => format!("{value:.0}%"),
    }
}

/// The line that always survives: what the control under the cursor is, in
/// full, with its travel.
fn reverb_readout(params: &[f32], control: usize, slot_index: usize) -> Line<'static> {
    let algorithm = reverb_algorithm(params);
    let mut spans = vec![
        Span::styled(" rvb ", theme::amber_bright().add_modifier(Modifier::BOLD)),
        Span::styled(format!("slot {} ", slot_index + 1), theme::dim()),
        Span::styled(format!("\u{00b7} {} ", algorithm.label()), theme::muted()),
    ];
    let Some(info) = reverb_param(control) else {
        return Line::from(spans);
    };
    spans.push(Span::styled(format!("\u{00b7} {} ", info.name), theme::muted()));
    spans.push(Span::styled(
        reverb_value(params, control),
        theme::normal().add_modifier(Modifier::BOLD),
    ));
    if algorithm.uses(control) {
        spans.push(Span::styled(
            format!("  ({} .. {})", trim_number(info.min), trim_number(info.max)),
            theme::dim(),
        ));
    } else {
        // Greyed on the strip and refused by the keys, said once more here in
        // words: the spring's input stage is its dispersion chain, so there
        // are no diffuser coefficients for this control to scale.
        spans.push(Span::styled(
            format!("  \u{2014} no effect on the {}", algorithm.label()),
            theme::dim(),
        ));
    }
    Line::from(spans)
}

/// A range end, without the trailing zeros a range does not need.
fn trim_number(value: f32) -> String {
    if (value - value.round()).abs() < 0.005 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

// ── The delay ──

/// The colour a control that is doing something alarming is drawn in: a
/// feedback setting past unity, or a synced division the line had to fold in
/// half to hold. Asked of the theme rather than named here, so it follows the
/// palette like everything else does.
fn alarm_style() -> Style {
    Style::default().fg(theme::rec_active_val()).bg(theme::bg_val())
}

/// The delay's sixteen controls, as a column of knobs.
///
/// No picture. A delay's response is a comb whose teeth are half a hertz apart
/// at a musical setting, and the honest drawing of one is a hundred thousand
/// points wide — so the panel is the numbers, and the numbers are the ones the
/// effect declares.
///
/// Three of them are conditional and all three grey out rather than
/// disappearing: `div` and `time` are the two halves of one clock and only one
/// is live, `heads` belongs to the tape transport, `wander` to the bucket
/// brigade's clock. A control that vanished when a mode changed would make the
/// list jump under the cursor; a control that greys stays where it was.
fn render_delay(frame: &mut Frame, area: Rect, nav: &NavState, slot: &FxInstance, index: usize) {
    let (w, h) = (area.width as usize, area.height as usize);
    let view = &nav.clip_view.fx;
    let focused = nav.focused_pane == Pane::ClipView
        && nav.clip_view.focus == ClipViewFocus::PianoRoll
        && nav.clip_view.clip_tab == ClipTab::Fx;
    let params = &slot.params;
    let bpm = f64::from(nav.tempo_bpm);
    let cursor = view.band.min(DELAY_PARAMS - 1);

    let mut lines: Vec<Line> = vec![delay_readout(params, cursor, index, bpm)];
    if slot.bypass {
        lines.push(Line::from(Span::styled(
            "  bypassed \u{2014} b puts it back in the signal path",
            theme::dim(),
        )));
    }
    lines.push(Line::from(""));

    // Two columns where there is room, one where there is not. Sixteen entries
    // either way; what changes is how many rows they cost.
    let columns = if is_wide(w) && h >= 10 { 2 } else { 1 };
    let rows = DELAY_PARAMS.div_ceil(columns);
    let visible = h.saturating_sub(lines.len() + 1).max(1);
    let first_row = if columns == 1 {
        cursor.saturating_sub(visible.saturating_sub(1)).min(rows.saturating_sub(1))
    } else {
        0
    };

    for row in first_row..rows {
        if lines.len() + 1 > h {
            break;
        }
        let mut spans: Vec<Span> = Vec::new();
        for column in 0..columns {
            let control = column * rows + row;
            if control >= DELAY_PARAMS {
                continue;
            }
            let here = cursor == control;
            let live = delay_uses(params, control);
            let name = delay_param(control).map_or("", |p| p.name);
            spans.push(Span::styled(
                format!("{}{name:<7}", if here { "\u{25B8}" } else { " " }),
                if here && focused { theme::amber_bright() } else { theme::dim() },
            ));
            spans.push(Span::styled(
                format!("{:<15}", delay_value(params, control, bpm)),
                // Feedback past unity is drawn in the warning colour: the loop
                // is bounded there by construction, but it is also singing,
                // and a knob that is singing should look like one.
                if control == PARAM_FEEDBACK && params.get(control).copied().unwrap_or(0.0) > 100.0
                {
                    alarm_style()
                } else {
                    cell_style(here, view.locked, live, focused)
                },
            ));
            if column + 1 < columns {
                spans.push(Span::styled("  ", theme::dim()));
            }
        }
        lines.push(Line::from(spans));
    }

    if lines.len() < h {
        lines.push(Line::from(Span::styled(
            if view.locked {
                "  held \u{00b7} h/l adjusts \u{00b7} H/L strides \u{00b7} esc lets go"
            } else {
                "  j/k picks \u{00b7} h/l adjusts \u{00b7} H/L strides \u{00b7} enter holds"
            },
            theme::dim(),
        )));
    }

    lines.truncate(h);
    frame.render_widget(Paragraph::new(lines), area);
}

/// The delay time in force, in seconds, and whether the sync law had to fold
/// it to fit the line.
pub(crate) fn delay_resolved(params: &[f32], bpm: f64) -> (f64, u32) {
    if params.get(PARAM_SYNC).copied().unwrap_or(1.0) >= 0.5 {
        let division = params.get(PARAM_DIVISION).copied().unwrap_or(0.0).round().max(0.0) as usize;
        synced_seconds(division, bpm)
    } else {
        (f64::from(params.get(PARAM_TIME_MS).copied().unwrap_or(0.0)) / 1000.0, 0)
    }
}

/// A delay time, in the unit that reads best at that length.
fn ms_label(seconds: f64) -> String {
    let ms = seconds * 1000.0;
    if ms >= 1000.0 {
        format!("{:.2} s", seconds)
    } else if ms >= 100.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{ms:.1} ms")
    }
}

/// One control, in the unit a person reads it in.
pub(crate) fn delay_value(params: &[f32], control: usize, bpm: f64) -> String {
    let value = params.get(control).copied().unwrap_or(0.0);
    let index = value.round().max(0.0) as usize;
    match control {
        PARAM_MODE => phosphor_dsp::fx::delay::Mode::from_index(index).label().to_string(),
        PARAM_ROUTING => phosphor_dsp::fx::delay::Routing::from_index(index).label().to_string(),
        PARAM_SYNC | PARAM_FREEZE => if value >= 0.5 { "on" } else { "off" }.to_string(),
        // **The clamp is announced.** A whole note at 40 BPM is six seconds
        // and the line is five, so it ships as three — and a player who is not
        // told hears the grid break for no reason they can see.
        PARAM_DIVISION => {
            let label = SYNC_LABELS[index.min(SYNC_LABELS.len() - 1)];
            let (seconds, halvings) = synced_seconds(index, bpm);
            if halvings > 0 {
                format!("{label} \u{2192} {} clamped", ms_label(seconds))
            } else {
                format!("{label}  {}", ms_label(seconds))
            }
        }
        PARAM_TIME_MS => ms_label(f64::from(value) / 1000.0),
        PARAM_TIME_MODE => {
            phosphor_dsp::fx::delay::TimeMode::from_index(index).label().to_string()
        }
        // The derived repeat count is worth more on a panel than any taper
        // cleverness. Past unity there is no count, and the word for that is
        // the honest one.
        PARAM_FEEDBACK => match phosphor_dsp::fx::delay::repeats_to_silence(value) {
            Some(repeats) => format!("{value:.0}%  ~{repeats:.0} rpts"),
            None if value <= 0.0 => "0%  no repeat".to_string(),
            None => format!("{value:.0}%  sings"),
        },
        DELAY_LOW_CUT | DELAY_HIGH_CUT => format!("{} Hz", hz_label(value)),
        PARAM_OFFSET => format!("{value:+.0}%"),
        PARAM_HEADS => HEAD_LABELS[index.min(HEAD_LABELS.len() - 1)].to_string(),
        _ => format!("{value:.0}%"),
    }
}

/// The line that always survives: what the control under the cursor is, in
/// full, with its travel — and what the delay is doing overall.
fn delay_readout(params: &[f32], control: usize, slot_index: usize, bpm: f64) -> Line<'static> {
    let mode = phosphor_dsp::fx::delay::mode_of(params);
    let routing = phosphor_dsp::fx::delay::routing_of(params);
    let (seconds, halvings) = delay_resolved(params, bpm);
    let mut spans = vec![
        Span::styled(" dly ", theme::amber_bright().add_modifier(Modifier::BOLD)),
        Span::styled(format!("slot {} ", slot_index + 1), theme::dim()),
        Span::styled(
            format!("\u{00b7} {} {} \u{00b7} {} ", mode.label(), routing.label(), ms_label(seconds)),
            theme::muted(),
        ),
    ];
    if halvings > 0 {
        spans.push(Span::styled("(clamped) ", alarm_style()));
    }
    let Some(info) = delay_param(control) else {
        return Line::from(spans);
    };
    spans.push(Span::styled(format!("\u{00b7} {} ", info.name), theme::muted()));
    spans.push(Span::styled(
        delay_value(params, control, bpm),
        theme::normal().add_modifier(Modifier::BOLD),
    ));
    if delay_uses(params, control) {
        spans.push(Span::styled(
            format!("  ({} .. {})", trim_number(info.min), trim_number(info.max)),
            theme::dim(),
        ));
    } else {
        // Greyed on the list and refused by the keys, said once more here in
        // words rather than left as a cell that will not move.
        spans.push(Span::styled(format!("  \u{2014} {}", delay_why_not(params, control)), theme::dim()));
    }
    Line::from(spans)
}

/// Why a greyed control is greyed, in the words a player would use.
pub(crate) fn delay_why_not(params: &[f32], control: usize) -> String {
    let mode = phosphor_dsp::fx::delay::mode_of(params);
    match control {
        PARAM_DIVISION => "the clock is free-running".to_string(),
        PARAM_TIME_MS => "the clock is following the tempo".to_string(),
        PARAM_HEADS => format!("only the tape has three heads, not the {}", mode.label()),
        _ => format!("only the bbd has a clock to drift, not the {}", mode.label()),
    }
}

// ── The tape ──

/// The tape's twelve controls, as a column of knobs.
///
/// No picture. What a tape machine does to a signal is a *transfer curve*
/// whose interesting part is a hysteresis loop — two-valued, so it is not a
/// function of the input and cannot be drawn as one on a line — plus a
/// wobble that is a tenth of a percent deep. The panel is therefore the
/// numbers, and the numbers are the ones the effect derives rather than the
/// ones the knobs hold: `wow` reads the deviation it is asking for, `bump`
/// reads the frequency the speed puts it at, `azimth` reads the corner it is
/// taking the top off at, and `mkauto` reads the gain it has decided on.
///
/// One control greys: the output trim is the automatic makeup's manual
/// alternative and is inert while the automatic is on. Turning it anyway
/// takes the makeup back rather than refusing the key, which is the
/// compressor's idiom and the same control.
fn render_tape(frame: &mut Frame, area: Rect, nav: &NavState, slot: &FxInstance, index: usize) {
    let (w, h) = (area.width as usize, area.height as usize);
    let view = &nav.clip_view.fx;
    let focused = nav.focused_pane == Pane::ClipView
        && nav.clip_view.focus == ClipViewFocus::PianoRoll
        && nav.clip_view.clip_tab == ClipTab::Fx;
    let params = &slot.params;
    let cursor = view.band.min(TAPE_PARAMS - 1);

    let mut lines: Vec<Line> = vec![tape_readout(params, cursor, index)];
    if slot.bypass {
        lines.push(Line::from(Span::styled(
            "  bypassed \u{2014} b puts it back in the signal path",
            theme::dim(),
        )));
    }
    lines.push(Line::from(""));

    let columns = if is_wide(w) && h >= 9 { 2 } else { 1 };
    let rows = TAPE_PARAMS.div_ceil(columns);
    let visible = h.saturating_sub(lines.len() + 1).max(1);
    let first_row = if columns == 1 {
        cursor.saturating_sub(visible.saturating_sub(1)).min(rows.saturating_sub(1))
    } else {
        0
    };

    for row in first_row..rows {
        if lines.len() + 1 > h {
            break;
        }
        let mut spans: Vec<Span> = Vec::new();
        for column in 0..columns {
            let control = column * rows + row;
            if control >= TAPE_PARAMS {
                continue;
            }
            let here = cursor == control;
            let live = tape_uses(params, control);
            let name = tape_param(control).map_or("", |p| p.name);
            spans.push(Span::styled(
                format!("{}{name:<7}", if here { "\u{25B8}" } else { " " }),
                if here && focused { theme::amber_bright() } else { theme::dim() },
            ));
            spans.push(Span::styled(
                format!("{:<17}", tape_value(params, control)),
                cell_style(here, view.locked, live, focused),
            ));
            if column + 1 < columns {
                spans.push(Span::styled("  ", theme::dim()));
            }
        }
        lines.push(Line::from(spans));
    }

    if lines.len() < h {
        lines.push(Line::from(Span::styled(
            if view.locked {
                "  held \u{00b7} h/l adjusts \u{00b7} H/L strides \u{00b7} esc lets go"
            } else {
                "  j/k picks \u{00b7} h/l adjusts \u{00b7} H/L strides \u{00b7} enter holds"
            },
            theme::dim(),
        )));
    }

    lines.truncate(h);
    frame.render_widget(Paragraph::new(lines), area);
}

/// A speed deviation, in the decimals it needs and no more: 0.10% reads as
/// two, 0.013% as three.
fn deviation_label(percent: f64) -> String {
    if percent >= 0.1 {
        format!("{percent:.2}%")
    } else {
        format!("{percent:.3}%")
    }
}

/// One control, in the unit a person reads it in — and, where there is one,
/// the number it *means* next to the number it holds.
pub(crate) fn tape_value(params: &[f32], control: usize) -> String {
    let value = params.get(control).copied().unwrap_or(0.0);
    let speed = phosphor_dsp::fx::tape::speed_of(params);
    match control {
        TAPE_SPEED => speed.label().to_string(),
        // The deviation, because "wow 50%" means nothing and "0.10%" is the
        // number every specification sheet in the field is written in.
        TAPE_WOW => format!("{value:.0}%  {}", deviation_label(wow_percent(value))),
        TAPE_FLUTTER => format!("{value:.0}%  {}", deviation_label(flutter_percent(value))),
        // The centre comes from the speed, so the row says where the bump is
        // rather than making a player derive it.
        TAPE_BUMP_DB => {
            if value <= 0.0 {
                "off".to_string()
            } else {
                format!("{value:+.1} dB {} Hz", hz_label(bump_hz(speed) as f32))
            }
        }
        TAPE_AZIMUTH => {
            if value <= 0.0 {
                "true".to_string()
            } else {
                format!("{value:.2}\u{b0} {} Hz", hz_label(azimuth_hz(value, speed) as f32))
            }
        }
        TAPE_HISS => match hiss_dbfs(value) {
            None => "off".to_string(),
            Some(db) => format!("{value:.0}%  {db:.0} dBFS"),
        },
        TAPE_TRIM => format!("{value:+.1} dB"),
        // The gain it has settled on, because a switch that says "on" is a
        // switch that has not told you what it did.
        TAPE_AUTO_MAKEUP => {
            if value >= 0.5 {
                format!("auto {:+.1} dB", auto_makeup_db(params))
            } else {
                "manual".to_string()
            }
        }
        _ => format!("{value:.0}%"),
    }
}

/// The line that always survives: what the machine is doing overall, and what
/// the control under the cursor is, in full, with its travel.
fn tape_readout(params: &[f32], control: usize, slot_index: usize) -> Line<'static> {
    let speed = phosphor_dsp::fx::tape::speed_of(params);
    let mut spans = vec![
        Span::styled(" tap ", theme::amber_bright().add_modifier(Modifier::BOLD)),
        Span::styled(format!("slot {} ", slot_index + 1), theme::dim()),
        Span::styled(
            format!(
                "\u{00b7} {} \u{00b7} bump {} Hz \u{00b7} top {} Hz ",
                speed.label(),
                hz_label(bump_hz(speed) as f32),
                hz_label(loss_hz(speed) as f32)
            ),
            theme::muted(),
        ),
    ];
    let Some(info) = tape_param(control) else {
        return Line::from(spans);
    };
    spans.push(Span::styled(format!("\u{00b7} {} ", info.name), theme::muted()));
    spans.push(Span::styled(
        tape_value(params, control),
        theme::normal().add_modifier(Modifier::BOLD),
    ));
    if control == TAPE_AUTO_MAKEUP {
        // **What "level-matched" means, said where a player will read it.**
        // The makeup is the reciprocal of the medium's small-signal gain
        // with a lineup constant on top, and the constant is measured on
        // programme material: a tone at the same peak has a third of the
        // crest factor, drives the medium far harder, and comes back
        // quieter. Music matches; a sine does not, and that is not a fault.
        spans.push(Span::styled(
            "  \u{2014} matched on programme, not on a tone",
            theme::dim(),
        ));
    } else if tape_uses(params, control) {
        spans.push(Span::styled(
            format!("  ({} .. {})", trim_number(info.min), trim_number(info.max)),
            theme::dim(),
        ));
    } else {
        // Greyed on the list and said once more here in words. Turning it
        // anyway is not refused — it takes the makeup back.
        spans.push(Span::styled(format!("  \u{2014} {}", tape_why_not(control)), theme::dim()));
    }
    Line::from(spans)
}

/// Why a greyed control is greyed, in the words a player would use.
pub(crate) fn tape_why_not(control: usize) -> String {
    match control {
        TAPE_TRIM => "the makeup is automatic \u{00b7} turning this takes it back".to_string(),
        _ => "not on this machine".to_string(),
    }
}

// ── The compressor ──

/// The two rows on the compressor's panel that are not controls on the
/// effect: which track feeds the detector, and whether that signal is being
/// monitored in place of the track's own output.
///
/// They are drawn here because this is where a player looks for them, and
/// they are not parameters because neither one is the compressor's business:
/// the key is routing that the mixer resolves from a stored track identity
/// every block, and monitoring the key replaces the whole track's output,
/// which a slot cannot do.
pub(crate) const COMP_ROW_KEY: usize = COMP_PARAMS;
pub(crate) const COMP_ROW_KEY_LISTEN: usize = COMP_PARAMS + 1;

/// How many rows the panel has: the twelve controls plus those two.
pub(crate) const COMP_ROWS: usize = COMP_PARAMS + 2;

/// What a row is called.
#[must_use]
pub(crate) fn comp_row_name(row: usize) -> &'static str {
    match row {
        COMP_ROW_KEY => "key",
        COMP_ROW_KEY_LISTEN => "klistn",
        _ => comp_param(row).map_or("", |p| p.name),
    }
}

/// Whether a row does anything at these settings.
///
/// Three of them are conditional, and all three grey out rather than
/// disappearing — a control that vanished when a switch moved would make the
/// list jump under the cursor.
///
/// * `makeup` while `mkauto` is on, and `releas` while the automatic release
///   is running: an automatic has taken the control over. **Turning a greyed
///   control takes it back** — the key handler switches the automatic off and
///   seeds the knob with the value it was already producing, so the control
///   never jumps and nothing on this panel is ever simply dead.
/// * `key` and `klistn` on a bus or on the master: those strips are not
///   tracks, they have no place in the track list, and there is nothing for a
///   key to name.
#[must_use]
pub(crate) fn comp_row_live(nav: &NavState, params: &[f32], row: usize) -> bool {
    match row {
        COMP_ROW_KEY | COMP_ROW_KEY_LISTEN => nav
            .current_track()
            .is_some_and(|t| t.kind == TrackKind::Instrument || t.kind == TrackKind::Audio),
        _ => comp_param_uses(params, row),
    }
}

/// Why a greyed row is greyed, in the words a player would use.
#[must_use]
pub(crate) fn comp_why_not(row: usize) -> &'static str {
    match row {
        PARAM_MAKEUP_DB => "the automatic makeup has it \u{2014} turn it to take it back",
        PARAM_RELEASE_MS => "the automatic release has it \u{2014} turn it to take it back",
        _ => "a bus has no key",
    }
}

/// The key this strip is running, as a phrase.
///
/// A deleted key track reads `Kick (missing)` rather than a bare `(missing)`:
/// the mixer has already fallen back to the internal key for this block, and
/// the player needs to know *which* track went so they can put it back or
/// point the key somewhere else.
#[must_use]
pub(crate) fn comp_key_label(nav: &NavState) -> String {
    let Some(track) = nav.current_track() else {
        return "internal".to_string();
    };
    if !matches!(track.kind, TrackKind::Instrument | TrackKind::Audio) {
        return "internal".to_string();
    }
    let Some(id) = track.key_source else {
        return "internal".to_string();
    };
    match nav.tracks.iter().find(|t| t.mixer_id == Some(id)) {
        Some(source) => source.name.clone(),
        None => match &track.key_source_name {
            Some(name) => format!("{name} (missing)"),
            None => "(missing)".to_string(),
        },
    }
}

/// One row, in the unit a person reads it in.
#[must_use]
pub(crate) fn comp_value(nav: &NavState, params: &[f32], row: usize) -> String {
    let at = |index: usize| params.get(index).copied().unwrap_or(0.0);
    match row {
        PARAM_CHARACTER => {
            // The selector names a character; the moment a control moves off
            // it, it says so. A selector that keeps naming a preset after the
            // preset has been dialled away from is a selector that lies.
            if matches_character(params) {
                character_name(params).to_string()
            } else {
                format!("{} \u{00b7}edited", character_name(params))
            }
        }
        PARAM_THRESHOLD_DB => format!("{:.1} dB", at(row)),
        PARAM_RATIO => ratio_label(at(row)),
        PARAM_KNEE_DB => {
            if at(row) <= 0.0 {
                "0 dB  hard".to_string()
            } else {
                format!("{:.0} dB  soft", at(row))
            }
        }
        PARAM_ATTACK_MS => {
            let ms = at(row);
            if ms < 1.0 {
                format!("{ms:.2} ms")
            } else {
                format!("{ms:.1} ms")
            }
        }
        PARAM_RELEASE_MS => {
            if auto_release_of(params) == AutoRelease::Off {
                let ms = at(row);
                if ms >= 1000.0 {
                    format!("{:.2} s", f64::from(ms) / 1000.0)
                } else {
                    format!("{ms:.0} ms")
                }
            } else {
                // The automatic owns it. Saying which two time constants are
                // in force is worth more than showing a number that is not.
                "\u{2014} automatic".to_string()
            }
        }
        PARAM_AUTO_RELEASE => match auto_release_of(params) {
            AutoRelease::Off => "off".to_string(),
            AutoRelease::Auto => "auto  100ms/12s".to_string(),
            AutoRelease::Auto2 => "auto 2  50ms/6s".to_string(),
        },
        PARAM_MAKEUP_DB => {
            if at(PARAM_AUTO_MAKEUP) >= 0.5 {
                let db = auto_makeup_for(
                    f64::from(at(PARAM_THRESHOLD_DB)),
                    f64::from(at(PARAM_RATIO)) / 100.0,
                );
                format!("{db:+.1} dB  auto")
            } else {
                format!("{:+.1} dB", at(row))
            }
        }
        PARAM_AUTO_MAKEUP => if at(row) >= 0.5 { "on" } else { "off" }.to_string(),
        COMP_MIX => {
            let mix = at(row);
            if mix >= 99.95 {
                "100%".to_string()
            } else {
                format!("{mix:.0}%  parallel")
            }
        }
        PARAM_SENSE => sense_of(params).label().to_string(),
        PARAM_SC_HPF_HZ => {
            if at(row) < SC_HPF_MIN_HZ {
                "off".to_string()
            } else {
                format!("{} Hz", hz_label(at(row)))
            }
        }
        COMP_ROW_KEY => comp_key_label(nav),
        COMP_ROW_KEY_LISTEN => {
            let listening = nav
                .current_track()
                .and_then(|t| t.mixer_id)
                .is_some_and(|id| nav.key_listen == Some(id));
            if listening { "LISTENING".to_string() } else { "off".to_string() }
        }
        _ => format!("{:.2}", at(row)),
    }
}

/// The line that always survives: what the row under the cursor is, in full,
/// with its travel — and what the compressor is doing overall.
fn comp_readout(nav: &NavState, params: &[f32], row: usize, slot_index: usize) -> Line<'static> {
    let mut spans = vec![
        Span::styled(" cmp ", theme::amber_bright().add_modifier(Modifier::BOLD)),
        Span::styled(format!("slot {} ", slot_index + 1), theme::dim()),
        Span::styled(
            format!(
                "\u{00b7} {} {} \u{00b7} latency 0 ",
                ratio_label(params.get(PARAM_RATIO).copied().unwrap_or(0.0)),
                sense_of(params).label(),
            ),
            theme::muted(),
        ),
    ];
    spans.push(Span::styled(
        format!("\u{00b7} {} ", comp_row_name(row)),
        theme::muted(),
    ));
    spans.push(Span::styled(
        comp_value(nav, params, row),
        theme::normal().add_modifier(Modifier::BOLD),
    ));
    if !comp_row_live(nav, params, row) {
        spans.push(Span::styled(format!("  \u{2014} {}", comp_why_not(row)), theme::dim()));
    } else if let Some(info) = comp_param(row) {
        // The ratio's travel is a percentage of full limiting, and nobody
        // wants to read that: the two ends of the knob are what it means.
        let travel = if row == PARAM_RATIO {
            "  (1.0:1 .. \u{221e}:1)".to_string()
        } else {
            format!("  ({} .. {})", trim_number(info.min), trim_number(info.max))
        };
        spans.push(Span::styled(travel, theme::dim()));
    }
    Line::from(spans)
}

/// The compressor's panel: fourteen rows and a gain-reduction meter that is
/// the whole point of looking at it.
///
/// No picture of the static curve. A compressor's curve is two straight lines
/// and a parabola between them, and drawing it costs the rows that say what
/// the ballistics are doing — which is the half of a compressor a curve cannot
/// show at all. The meter is the picture.
fn render_comp(frame: &mut Frame, area: Rect, nav: &NavState, slot: &FxInstance, index: usize) {
    let (w, h) = (area.width as usize, area.height as usize);
    let view = &nav.clip_view.fx;
    let focused = nav.focused_pane == Pane::ClipView
        && nav.clip_view.focus == ClipViewFocus::PianoRoll
        && nav.clip_view.clip_tab == ClipTab::Fx;
    let params = &slot.params;
    let cursor = view.band.min(COMP_ROWS - 1);

    let mut lines: Vec<Line> = vec![comp_readout(nav, params, cursor, index)];

    // The meter, live off the audio thread's two atomics.
    let (current, peak) = slot.gr.as_ref().map_or((0.0, 0.0), |m| m.get());
    let mut meter = vec![Span::styled("  ", theme::bg())];
    meter.extend(gr_meter_spans("gr", current, peak, GR_PANEL_WIDTH));
    if slot.bypass {
        meter.push(Span::styled("   bypassed \u{2014} b puts it back", theme::dim()));
    }
    lines.push(Line::from(meter));
    lines.push(Line::from(""));

    // Two columns where there is room, one where there is not. Fourteen rows
    // either way; what changes is how many lines they cost.
    let columns = if is_wide(w) && h >= 10 { 2 } else { 1 };
    let rows = COMP_ROWS.div_ceil(columns);
    let visible = h.saturating_sub(lines.len() + 1).max(1);
    let first_row = if columns == 1 {
        cursor.saturating_sub(visible.saturating_sub(1)).min(rows.saturating_sub(1))
    } else {
        0
    };

    for row in first_row..rows {
        if lines.len() + 1 > h {
            break;
        }
        let mut spans: Vec<Span> = Vec::new();
        for column in 0..columns {
            let control = column * rows + row;
            if control >= COMP_ROWS {
                continue;
            }
            let here = cursor == control;
            let live = comp_row_live(nav, params, control);
            spans.push(Span::styled(
                format!("{}{:<7}", if here { "\u{25B8}" } else { " " }, comp_row_name(control)),
                if here && focused { theme::amber_bright() } else { theme::dim() },
            ));
            let listening = control == COMP_ROW_KEY_LISTEN
                && nav
                    .current_track()
                    .and_then(|t| t.mixer_id)
                    .is_some_and(|id| nav.key_listen == Some(id));
            spans.push(Span::styled(
                format!("{:<17}", comp_value(nav, params, control)),
                if listening {
                    // The one control on this panel that changes what comes
                    // out of the speakers rather than what the compressor
                    // does with it. It is drawn in the warning colour for the
                    // same reason the record light is.
                    alarm_style()
                } else {
                    cell_style(here, view.locked, live, focused)
                },
            ));
            if column + 1 < columns {
                spans.push(Span::styled("  ", theme::dim()));
            }
        }
        lines.push(Line::from(spans));
    }

    if lines.len() < h {
        lines.push(Line::from(Span::styled(
            if view.locked {
                "  held \u{00b7} h/l adjusts \u{00b7} H/L strides \u{00b7} esc lets go"
            } else {
                "  j/k picks \u{00b7} h/l adjusts \u{00b7} H/L strides \u{00b7} enter holds"
            },
            theme::dim(),
        )));
    }

    lines.truncate(h);
    frame.render_widget(Paragraph::new(lines), area);
}
