//! UI rendering: tracks.

use super::*;

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
    let vu_w = 3usize;
    let vu_filled = (vu_w as f32 * vu_level) as usize;

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
        r0.push(Span::styled(format!("v{:<2}", (track.volume * 99.0) as u8), theme::btn_style(false, v_f, tc)));
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

    let vu_filled = vu_filled.min(vu_w);
    let vu_bar: String = "\u{2588}".repeat(vu_filled) + &"\u{2591}".repeat(vu_w - vu_filled);
    let vu_s = Style::default()
        .fg(theme::dim_color(tc, if dim { 20 } else { 55 }))
        .bg(theme::piano_black_bg());

    let r1: Vec<Span> = vec![
        Span::styled(ac, ac_s),
        Span::styled("  ", theme::bg()),
        Span::styled("m", theme::btn_style(track.muted, m_f, tc)),
        Span::styled(" ", theme::bg()),
        Span::styled("s", solo_s),
        Span::styled(" ", theme::bg()),
        Span::styled(vu_bar, vu_s),
    ];

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

// ── Clip View (FX Panel left + Piano Roll right) ──

