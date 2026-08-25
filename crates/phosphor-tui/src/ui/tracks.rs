//! UI rendering: tracks.

use super::*;

/// Width of the VU bar, in cells. Three is what the track header has room
/// for, and it is the number the scale below is chosen around.
const VU_CELLS: usize = 3;

/// Bottom of the VU's range, −30 dBFS.
///
/// Picked for the number of cells there are rather than by convention. Three
/// cells over 30 dB is 10 dB each, which puts the boundaries where the useful
/// distinctions are: three cells means the track is within 10 dB of full
/// scale, two means it is in the −20..−10 window that ordinary playing
/// occupies, one means there is signal but it is quiet. The −60 dBFS a drawn
/// meter usually spans would spend two of its three cells on levels below
/// anything the instruments produce, and read fully lit the whole time
/// someone is playing.
const VU_FLOOR_DB: f32 = -30.0;

/// How many cells of the VU a peak fills.
///
/// The meter reads the track buffer before the fader, so it shows what the
/// instrument produced. It used to be linear — `(cells as f32 * level)` —
/// which was survivable when the instruments ran into a hard clip at full
/// scale, and is not now that they are trimmed for headroom: an ordinary
/// 0.25 peak filled `3 * 0.25 = 0` cells and the meter was dark exactly when
/// it had something to say. Hearing is logarithmic and so is this.
///
/// Rounded up rather than down so any audible signal lights at least one
/// cell; the alternative wastes a third of a three-cell meter on the gap
/// between "silent" and "the first cell is worth drawing".
fn vu_cells(level: f32) -> usize {
    // NaN takes the same path as silence. This value is written by the audio
    // thread and read here without synchronisation beyond the atomic itself,
    // so it is handled rather than assumed away; the float-to-integer cast
    // below saturates rather than trapping, so nothing here can panic.
    if level.is_nan() || level <= 0.0 {
        return 0;
    }
    let fraction = 1.0 - (20.0 * level.log10()) / VU_FLOOR_DB;
    if fraction <= 0.0 {
        return 0;
    }
    ((fraction * VU_CELLS as f32).ceil() as usize).min(VU_CELLS)
}

/// The fader as dB relative to unity, in exactly three characters.
///
/// A fader is a dB control, so it reads as one: `  0` at unity, ` +6` at the
/// top of the travel, ` -2` where a new track starts, `-oo` at the bottom.
/// The `v` label the field used to carry is spent on the sign, which is the
/// better use of the character — every value is signed, so the leading
/// column says "relative level" on its own.
///
/// What it replaced was `volume * 99` in two characters. That was only ever
/// meaningful for a 0..1 fader, and once the travel went to +6 dB it would
/// have rendered 198 into a two-character field.
fn fader_label(volume: f32) -> String {
    if volume <= 0.0 {
        return "-oo".to_string();
    }
    let text = match (20.0 * volume.log10()).round() as i32 {
        0 => "0".to_string(),
        db if db <= -100 => "-oo".to_string(),
        db => format!("{db:+}"),
    };
    format!("{text:>3}")
}

pub(super) fn render_tracks(frame: &mut Frame, area: Rect, nav: &NavState, snap: &TransportSnapshot) {
    let vis = nav.visible_tracks();

    if nav.can_scroll_up() {
        frame.render_widget(
            Paragraph::new(Span::styled("\u{25B2} more", theme::dim())).alignment(Alignment::Center),
            Rect::new(area.x, area.y, HEADER_W, 1));
    }

    let solo_on = nav.tracks.iter().any(|t| t.soloed);

    for (vi, track) in vis.iter().enumerate() {
        let ai = nav.track_scroll + vi;
        let y = area.y + vi as u16 * TRACK_H;
        if y + TRACK_H > area.y + area.height { break; }

        let cur = nav.focused_pane == Pane::Tracks && nav.track_cursor == ai;
        let sel = cur && nav.track_selected;
        let dim = track.muted || (solo_on && !track.soloed);
        let (vu_l, _) = track.vu_levels();

        let ctx = TrackCtx {
            track, index: ai, is_cursor: cur, is_selected: sel,
            is_dimmed: dim, vu_level: if dim { 0.0 } else { vu_l }, nav,
        };

        let r = Rect::new(area.x, y, area.width, TRACK_H);
        render_track_row(frame, r, &ctx, snap);
    }

    if nav.can_scroll_down() {
        let y = area.y + area.height - 1;
        frame.render_widget(
            Paragraph::new(Span::styled("\u{25BC} more", theme::dim())).alignment(Alignment::Center),
            Rect::new(area.x, y, HEADER_W, 1));
    }
}

pub(super) fn render_track_row(frame: &mut Frame, area: Rect, ctx: &TrackCtx, snap: &TransportSnapshot) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(HEADER_W), Constraint::Length(1), Constraint::Min(4)])
        .split(area);

    render_header(frame, cols[0], ctx);

    let sep: Vec<Line> = (0..area.height)
        .map(|_| Line::from(Span::styled("\u{2502}", theme::border_style())))
        .collect();
    frame.render_widget(Paragraph::new(sep), cols[1]);

    render_clips(frame, cols[2], ctx, snap);
}

// ── Track Header ──

pub(super) fn render_header(frame: &mut Frame, area: Rect, ctx: &TrackCtx) {
    let TrackCtx { track, index, is_cursor: cur, is_selected: sel, is_dimmed: dim, vu_level, nav, .. } = *ctx;
    let tc = theme::track_color(track.color_index);
    let id = (b'A' + index as u8) as char;
    let is_special = matches!(track.kind, TrackKind::SendA | TrackKind::SendB | TrackKind::Master);

    // Accent bar style
    let ac = if sel { "\u{2588}" } else { "\u{2590}" };
    let ac_s = if cur || sel { Style::default().fg(tc).bg(theme::bg_val()) }
        else { Style::default().fg(theme::dim_color(tc, if dim { 15 } else { 30 })).bg(theme::bg_val()) };
    let id_s = Style::default().fg(theme::dim_color(tc, if dim { 20 } else { 40 })).bg(theme::bg_val());

    // VU — horizontal bar on row 1
    let vu_filled = vu_cells(vu_level);

    // Record arm dot
    let arm_s = if track.armed {
        Style::default().fg(theme::rec_active_val()).bg(theme::bg_val())
    } else {
        theme::dim()
    };

    // Row 0: [accent][ID] [fx][v] [r]
    let mut r0: Vec<Span> = vec![
        Span::styled(ac, ac_s),
        Span::styled(format!("{id}"), id_s),
        Span::styled(" ", theme::bg()),
    ];
    if !is_special {
        let fx_f = sel && nav.track_element == TrackElement::Fx;
        let v_f = sel && nav.track_element == TrackElement::Volume;
        r0.push(Span::styled("fx", theme::btn_style(!track.fx_chain.is_empty(), fx_f, tc)));
        r0.push(Span::styled(" ", theme::bg()));
        r0.push(Span::styled(fader_label(track.volume), theme::btn_style(nav.element_locked && v_f, v_f, tc)));
        r0.push(Span::styled(if track.armed { " \u{25CF}" } else { "  " }, arm_s));
    }

    // Row 1: [accent]  [m][s] [VU]
    let m_f = sel && nav.track_element == TrackElement::Mute;
    let s_f = sel && nav.track_element == TrackElement::Solo;
    let solo_s = if track.soloed {
        Style::default().fg(theme::solo_active_fg())
            .bg(if s_f { theme::solo_focused_bg() } else { theme::solo_active_bg() })
            .add_modifier(Modifier::BOLD)
    } else {
        theme::btn_style(false, s_f, tc)
    };

    let vu_bar: String =
        "\u{2588}".repeat(vu_filled) + &"\u{2591}".repeat(VU_CELLS - vu_filled);
    let vu_s = Style::default()
        .fg(theme::dim_color(tc, if dim { 20 } else { 55 }))
        .bg(theme::piano_black_bg());

    let mut r1: Vec<Span> = vec![
        Span::styled(ac, ac_s),
        Span::styled("  ", theme::bg()),
        Span::styled("m", theme::btn_style(track.muted, m_f, tc)),
        Span::styled(" ", theme::bg()),
        Span::styled("s", solo_s),
        Span::styled(" ", theme::bg()),
        Span::styled(vu_bar, vu_s),
    ];

    // A pattern running on this track, and — the one that matters — a pattern
    // running on a track that also has clips on it. Those are two copies of
    // the same part playing, which sounds like a badly tuned instrument
    // rather than like a mistake anyone made, so it is said out loud on the
    // row itself.
    if let Some(sequencer) = track.sequencer.as_deref() {
        if sequencer.is_playing() {
            let doubled = !track.clips.is_empty();
            r1.push(Span::styled(
                if doubled { " \u{203C}" } else { " \u{25B6}" },
                if doubled {
                    theme::amber_bright().add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::dim_color(tc, 60)).bg(theme::bg_val())
                },
            ));
        }
    }

    // Row 2: divider line across header
    let r2 = Line::from(vec![
        Span::styled(ac, ac_s),
        Span::styled("\u{2500}".repeat(HEADER_W as usize - 1), theme::border_style()),
    ]);

    let lines = vec![
        Line::from(r0),
        Line::from(r1),
        r2,
    ];

    frame.render_widget(Paragraph::new(lines), area);
}

// ── Clip Area ──

pub(super) fn render_clips(frame: &mut Frame, area: Rect, ctx: &TrackCtx, snap: &TransportSnapshot) {
    let TrackCtx { track, is_selected: sel, is_dimmed: dim, nav, .. } = *ctx;
    let tc = theme::track_color(track.color_index);
    let (w, h) = (area.width as usize, area.height as usize);
    if w == 0 || h == 0 { return; }
    let bw = w / VISIBLE_BARS;
    if bw == 0 { return; }
    crate::debug_log::log("CLIP_GRID", &format!("w={w} h={h} alloc={}bytes", w * h * std::mem::size_of::<(char, Style)>()));

    let mut grid: Vec<Vec<(char, Style)>> = vec![vec![(' ', theme::bg()); w]; h];

    // Gridlines
    for b in 1..VISIBLE_BARS {
        let x = b * bw;
        if x < w {
            let major = b % 4 == 0;
            let s = Style::default()
                .fg(if major { theme::grid_major() } else { theme::grid_minor() })
                .bg(theme::bg_val());
            let ch = if major { '\u{2502}' } else { '\u{2506}' };
            for row in &mut grid { row[x] = (ch, s); }
        }
    }

    // Clips — positioned by their start_tick relative to the timeline
    let ticks_per_bar = Transport::PPQ * 4;
    let total_visible_ticks = (VISIBLE_BARS as i64) * ticks_per_bar;

    for (ci, clip) in track.clips.iter().enumerate() {
        let focused = sel && matches!(nav.track_element, TrackElement::Clip(i) if i == ci);
        // Position start and end independently so they snap to the same grid as bar lines
        let clip_end_tick = clip.start_tick + clip.length_ticks;
        let cx = (clip.start_tick as usize * w) / total_visible_ticks as usize;
        let ce = (clip_end_tick as usize * w) / total_visible_ticks as usize;
        let ce = ce.max(cx + 1).min(w);
        if cx >= w { break; }

        let bg = theme::bg_val();
        let cbg = if focused {
            // Blend track color into bg at 18%
            Color::Rgb(
                (theme::tc_r(tc) as u16 * 18 / 100 + theme::tc_r(bg) as u16) as u8,
                (theme::tc_g(tc) as u16 * 18 / 100 + theme::tc_g(bg) as u16) as u8,
                (theme::tc_b(tc) as u16 * 18 / 100 + theme::tc_b(bg) as u16) as u8,
            )
        } else if clip.has_content {
            Color::Rgb(
                (theme::tc_r(tc) as u16 * 8 / 100 + theme::tc_r(bg) as u16) as u8,
                (theme::tc_g(tc) as u16 * 8 / 100 + theme::tc_g(bg) as u16) as u8,
                (theme::tc_b(tc) as u16 * 8 / 100 + theme::tc_b(bg) as u16) as u8,
            )
        } else { theme::bg_val() };
        let cfg = if dim { theme::dim_color(tc,18) } else if focused { tc } else if clip.has_content { theme::dim_color(tc,55) } else { theme::dim_color(tc,20) };

        if clip.has_content {
            let afg = if dim { theme::dim_color(tc,25) } else if focused { tc } else { theme::dim_color(tc,65) };
            for x in cx..ce { grid[0][x] = ('\u{2580}', Style::default().fg(afg).bg(cbg)); }
        }

        // Clip body: empty block rendering for all clips
        let body_style = Style::default().fg(theme::dim_color(tc, 15)).bg(cbg);
        for row in grid.iter_mut().take(h.saturating_sub(1)).skip(1) {
            if let Some(cells) = row.get_mut(cx..ce) {
                let len = cells.len();
                for (j, cell) in cells.iter_mut().enumerate() {
                    let edge = j == 0 || j == len - 1;
                    *cell = (if edge { '\u{2502}' } else { ' ' }, body_style);
                }
            }
        }

        for x in cx..ce {
            if grid[h-1][x].0 == ' ' {
                grid[h-1][x] = ('\u{2581}', Style::default().fg(if clip.has_content { cfg } else { theme::dim_color(tc,12) }).bg(cbg));
            }
        }

        // Clip number
        let ns = format!("{}", clip.number);
        let n_s = Style::default().fg(if focused { theme::amber_bright_val() } else { theme::dim_color(tc, if dim { 20 } else { 40 }) }).bg(cbg);
        for (i, ch) in ns.chars().enumerate() {
            let x = cx+i+1;
            if x < ce && 1 < h { grid[1][x] = (ch, n_s); }
        }
    }

    // Bottom row: track name in first bar, divider line for remaining bars
    let last_row = h - 1;
    let div_s = theme::border_style();

    // Divider from bar 2 onward
    for x in bw..w {
        grid[last_row][x] = ('\u{2500}', div_s);
    }

    // Track name in first bar — lowercase, no bold, subtler presence
    let name = track.name.to_lowercase();
    let name_s = Style::default()
        .fg(if dim { theme::dim_color(tc, 30) } else { theme::dim_color(tc, 65) })
        .bg(theme::bg_val());
    for (i, ch) in name.chars().enumerate() {
        let x = i + 1;
        if x < bw && x < w {
            grid[last_row][x] = (ch, name_s);
        }
    }

    // Playhead
    if snap.playing {
        let ph = snap.position_ticks as f64 / (Transport::PPQ * 4) as f64;
        let px = (ph * bw as f64) as usize;
        if px < w {
            for row in &mut grid {
                let bg = row[px].1.bg.unwrap_or(theme::bg_val());
                row[px] = ('\u{2502}', Style::default().fg(theme::amber_val()).bg(bg));
            }
        }
    }

    let lines: Vec<Line> = grid_to_lines(grid);
    frame.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defect this replaced: a linear three-cell meter reads zero for
    /// every level the instruments actually produce. With ordinary playing
    /// peaking near 0.25, `(3.0 * 0.25) as usize` is 0 — a meter that is dark
    /// whenever there is something to show.
    #[test]
    fn vu_is_lit_at_the_levels_the_instruments_produce() {
        // Ordinary playing, -12 dBFS.
        assert!(vu_cells(0.25) >= 2, "ordinary playing read {} cells", vu_cells(0.25));
        // A quiet single note, around -24 dBFS.
        assert!(vu_cells(0.063) >= 1, "a quiet note read no cells");
        // The loudest thing in the project, -1 dBFS.
        assert_eq!(vu_cells(0.88), VU_CELLS);
    }

    /// Three cells, three bands, ten dB each. The point of the scale is that
    /// the cells mean different things; a meter that is always full or always
    /// dark carries no information.
    #[test]
    fn vu_bands_are_ten_db_apart() {
        // Just inside each band, so rounding at the boundary cannot flip it.
        assert_eq!(vu_cells(0.4), 3); // -8 dBFS
        assert_eq!(vu_cells(0.126), 2); // -18 dBFS
        assert_eq!(vu_cells(0.04), 1); // -28 dBFS
        assert_eq!(vu_cells(0.02), 0); // -34 dBFS, under the floor
    }

    #[test]
    fn vu_is_monotonic_and_never_overflows_the_bar() {
        let mut previous = 0;
        let mut level = 0.0001f32;
        while level < 4.0 {
            let cells = vu_cells(level);
            assert!(cells >= previous, "meter went down at {level}");
            assert!(cells <= VU_CELLS, "meter drew {cells} cells into {VU_CELLS}");
            previous = cells;
            level *= 1.01;
        }
    }

    /// The meter reads a value written by the audio thread, so it has to
    /// survive anything that thread can put there without panicking. The
    /// subtraction in the bar below it would underflow if this returned more
    /// cells than there are.
    #[test]
    fn vu_handles_silence_and_nonsense() {
        assert_eq!(vu_cells(0.0), 0);
        assert_eq!(vu_cells(-1.0), 0);
        assert_eq!(vu_cells(f32::NAN), 0);
        assert_eq!(vu_cells(f32::INFINITY), VU_CELLS);
        assert_eq!(vu_cells(f32::NEG_INFINITY), 0);
        assert_eq!(vu_cells(f32::MIN_POSITIVE), 0);
    }

    /// Exactly three characters at every fader position, because the header
    /// row has no slack: one character more and the record-arm dot is pushed
    /// off the end of the track header.
    #[test]
    fn fader_label_is_always_three_characters() {
        let mut volume = 0.0f32;
        while volume <= 2.0 {
            let label = fader_label(volume);
            assert_eq!(label.chars().count(), 3, "{volume} rendered as {label:?}");
            volume += 0.001;
        }
    }

    #[test]
    fn fader_label_reads_as_db() {
        assert_eq!(fader_label(2.0), " +6");
        assert_eq!(fader_label(1.413), " +3");
        assert_eq!(fader_label(1.0), "  0");
        assert_eq!(fader_label(0.708), " -3");
        assert_eq!(fader_label(0.251), "-12");
        assert_eq!(fader_label(0.0), "-oo");
    }
}

// ── Clip View (FX Panel left + Piano Roll right) ──

