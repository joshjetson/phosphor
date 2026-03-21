//! UI rendering — phosphor solarized-dark aesthetic.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use phosphor_core::project::TrackKind;
use phosphor_core::transport::{self, Transport, TransportSnapshot};

use crate::state::*;
use crate::theme;

const HEADER_W: u16 = 16;
const TRACK_H: u16 = 5;
const VISIBLE_BARS: usize = 16;
const FX_PANEL_W: u16 = 24;
const CLIP_MEASURES: usize = 32;

/// Rendering context for a single track row. Bundles all the per-track
/// state needed by render_header and render_clips so we don't pass 9 args.
struct TrackCtx<'a> {
    track: &'a TrackState,
    index: usize,
    is_cursor: bool,
    is_selected: bool,
    is_dimmed: bool,
    vu_level: f32,
    nav: &'a NavState,
}

pub fn render(
    frame: &mut Frame,
    transport: &TransportSnapshot,
    nav: &NavState,
) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(theme::bg()), area);

    let tracks_h = (MAX_VISIBLE_TRACKS as u16) * TRACK_H;

    let mut constraints = vec![
        Constraint::Length(1),       // top bar
        Constraint::Length(1),       // separator
        Constraint::Length(1),       // ruler
        Constraint::Length(tracks_h), // tracks
    ];

    if nav.clip_view_visible {
        constraints.push(Constraint::Length(1)); // clip view tabs/label
        constraints.push(Constraint::Min(8));    // clip view content
    } else {
        constraints.push(Constraint::Min(0));    // spacer
    }

    constraints.push(Constraint::Length(1)); // separator
    constraints.push(Constraint::Length(1)); // bottom bar

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut ci = 0;
    render_top_bar(frame, chunks[ci], nav, transport); ci += 1;
    render_sep(frame, chunks[ci]); ci += 1;
    render_ruler(frame, chunks[ci], nav, transport); ci += 1;
    render_tracks(frame, chunks[ci], nav, transport); ci += 1;

    if nav.clip_view_visible {
        render_clip_view_tabs(frame, chunks[ci], nav); ci += 1;
        render_clip_view(frame, chunks[ci], nav); ci += 1;
    } else {
        ci += 1;
    }

    render_sep(frame, chunks[ci]); ci += 1;
    render_bottom_bar(frame, chunks[ci], nav);

    // Overlays
    if nav.instrument_modal.open {
        render_instrument_modal(frame, nav);
    } else if nav.space_menu.open {
        render_space_menu(frame, nav);
    } else if nav.fx_menu.open {
        render_fx_menu(frame, nav);
    }
}

// ── Top Bar ──

fn render_top_bar(frame: &mut Frame, area: Rect, nav: &NavState, snap: &TransportSnapshot) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(12), Constraint::Min(20), Constraint::Length(30)])
        .split(area);

    frame.render_widget(Paragraph::new(Span::styled(" phosphor", theme::branding())), cols[0]);

    let seq = if snap.playing { Span::styled("seq:on", theme::normal()) } else { Span::styled("seq:off", theme::dim()) };
    let rec = if snap.recording { Span::styled("\u{25CF} rec", theme::rec_active()) } else { Span::styled("\u{25CF} rec", theme::rec_dim()) };
    let loop_focused = nav.loop_editor.active;
    let loop_enabled = nav.loop_editor.enabled;
    let lp = if loop_focused {
        // Editing markers — show current range, bold
        let label = if loop_enabled { "loop" } else { "loop?" };
        Span::styled(
            format!("{label}[{}]", nav.loop_editor.display()),
            theme::amber_bright().add_modifier(Modifier::BOLD),
        )
    } else if loop_enabled {
        // Loop is on
        Span::styled(
            format!("loop:{}", nav.loop_editor.display()),
            theme::amber(),
        )
    } else {
        Span::styled("loop:off", theme::dim())
    };

    let met = if snap.metronome {
        Span::styled("\u{266A}", theme::amber()) // musical note symbol
    } else {
        Span::styled("\u{266A}", theme::dim())
    };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            seq, Span::styled("  bpm:", theme::normal()),
            Span::styled(format!("{:.0}", snap.tempo_bpm), theme::amber_bright()),
            Span::styled("  4/4  ", theme::normal()), rec,
            Span::styled("  ", theme::bg()), lp,
            Span::styled("  ", theme::bg()), met,
        ])).alignment(Alignment::Center), cols[1]);

    let pos = transport::ticks_to_position_string(snap.position_ticks, Transport::PPQ);
    let secs = snap.position_ticks as f64 * 60.0 / (snap.tempo_bpm * Transport::PPQ as f64);
    let bar = snap.position_ticks / (Transport::PPQ * 4) + 1;
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!("bar {} \u{00B7} {:02}:{:05.2} \u{00B7} {}", bar, (secs/60.0) as u32, secs%60.0, pos),
            theme::muted())).alignment(Alignment::Right), cols[2]);
}

// ── Ruler ──

fn render_ruler(frame: &mut Frame, area: Rect, nav: &NavState, snap: &TransportSnapshot) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(HEADER_W), Constraint::Length(1), Constraint::Min(4)])
        .split(area);

    frame.render_widget(Paragraph::new(Span::styled("  trk", theme::dim())).alignment(Alignment::Center), cols[0]);
    frame.render_widget(Paragraph::new(Span::styled("\u{2502}", theme::border_style())), cols[1]);

    let w = cols[2].width as usize;
    let bw = if w > 0 { w / VISIBLE_BARS } else { return };
    if bw == 0 { return; }

    let ph = snap.position_ticks as f64 / (Transport::PPQ * 4) as f64;
    let loop_start = nav.loop_editor.start_bar as usize;
    let loop_end = nav.loop_editor.end_bar as usize; // exclusive
    let loop_focused = nav.loop_editor.active;
    let loop_enabled = nav.loop_editor.enabled;

    let spans: Vec<Span> = (0..VISIBLE_BARS).map(|b| {
        let bar_num = b + 1; // 1-based
        let is_ph = snap.playing && ph >= b as f64 && ph < (b + 1) as f64;
        let in_loop = (loop_enabled || loop_focused) && bar_num >= loop_start && bar_num < loop_end;

        let s = if is_ph {
            theme::amber()
        } else if loop_focused && bar_num == loop_start {
            Style::default().fg(Color::Rgb(80, 180, 80)).bg(theme::BG).add_modifier(Modifier::BOLD)
        } else if loop_focused && bar_num == loop_end - 1 {
            Style::default().fg(Color::Rgb(180, 80, 80)).bg(theme::BG).add_modifier(Modifier::BOLD)
        } else if in_loop {
            Style::default().fg(Color::Rgb(50, 100, 110)).bg(theme::BG)
        } else if b % 4 == 0 {
            theme::normal()
        } else {
            theme::dim()
        };

        Span::styled(format!("{:<w$}", bar_num, w = bw), s)
    }).collect();
    frame.render_widget(Paragraph::new(Line::from(spans)), cols[2]);
}

// ── Tracks ──

fn render_tracks(frame: &mut Frame, area: Rect, nav: &NavState, snap: &TransportSnapshot) {
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

        let dy = y + TRACK_H - 1;
        if dy < area.y + area.height {
            frame.render_widget(
                Paragraph::new(Span::styled("\u{2500}".repeat(area.width as usize), theme::border_style())),
                Rect::new(area.x, dy, area.width, 1));
        }
    }

    if nav.can_scroll_down() {
        let y = area.y + area.height - 1;
        frame.render_widget(
            Paragraph::new(Span::styled("\u{25BC} more", theme::dim())).alignment(Alignment::Center),
            Rect::new(area.x, y, HEADER_W, 1));
    }
}

fn render_track_row(frame: &mut Frame, area: Rect, ctx: &TrackCtx, snap: &TransportSnapshot) {
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

fn render_header(frame: &mut Frame, area: Rect, ctx: &TrackCtx) {
    let TrackCtx { track, index, is_cursor: cur, is_selected: sel, is_dimmed: dim, vu_level, nav, .. } = *ctx;
    let tc = theme::track_color(track.color_index);
    let h = area.height as usize;
    let id = (b'A' + index as u8) as char;
    let nm: Vec<char> = track.name.to_uppercase().chars().collect();
    let is_special = matches!(track.kind, TrackKind::SendA | TrackKind::SendB | TrackKind::Master);

    let mut lines: Vec<Line> = Vec::new();

    for row in 0..h {
        let mut sp: Vec<Span> = Vec::new();

        // Accent bar
        let ac = if sel { "\u{2588}" } else { "\u{2590}" };
        let as_ = if cur || sel { Style::default().fg(tc).bg(theme::BG) }
            else { Style::default().fg(theme::dim_color(tc, if dim { 15 } else { 30 })).bg(theme::BG) };
        sp.push(Span::styled(ac, as_));

        // ID
        sp.push(Span::styled(
            if row == 0 { format!("{id}") } else { " ".into() },
            Style::default().fg(theme::dim_color(tc, if dim { 20 } else { 40 })).bg(theme::BG)));

        // Name vertical
        let ns = if cur {
            Style::default().fg(if dim { theme::dim_color(tc, 40) } else { tc }).bg(theme::BG).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::dim_color(tc, if dim { 25 } else { 60 })).bg(theme::BG)
        };
        let nstart = h.saturating_sub(nm.len()) / 2;
        if row >= nstart && row < nstart + nm.len() {
            sp.push(Span::styled(format!(" {} ", nm[row - nstart]), ns));
        } else {
            sp.push(Span::styled("   ", theme::bg()));
        }

        // VU
        let vu = vu_level as f64;
        let filled = ((h as f64) * vu) as usize;
        let fb = h - 1 - row;
        let (vc, vs) = if fb < filled {
            ("\u{2588}", Style::default().fg(theme::dim_color(tc, if dim { 20 } else { 55 })).bg(Color::Rgb(5,13,22)))
        } else {
            ("\u{2591}", Style::default().fg(Color::Rgb(12,24,36)).bg(Color::Rgb(5,13,22)))
        };
        sp.push(Span::styled(vc, vs));
        sp.push(Span::styled(" ", theme::bg()));

        // Buttons column: fx, v, m, s, r (one per row, skip if special track)
        let btn = match row {
            0 if !is_special => {
                let f = sel && nav.track_element == TrackElement::Fx;
                let s = if !track.fx_chain.is_empty() {
                    let count = track.fx_chain.len();
                    (format!("fx{count}"), theme::btn_style(true, f, tc))
                } else {
                    ("fx ".into(), theme::btn_style(false, f, tc))
                };
                Some(s)
            }
            1 if !is_special => {
                let f = sel && nav.track_element == TrackElement::Volume;
                let vol_pct = (track.volume * 100.0) as u8;
                Some((format!("v{vol_pct}"), theme::btn_style(false, f, tc)))
            }
            2 => {
                let f = sel && nav.track_element == TrackElement::Mute;
                Some(("[m]".into(), theme::btn_style(track.muted, f, tc)))
            }
            3 => {
                let f = sel && nav.track_element == TrackElement::Solo;
                let s = if track.soloed {
                    Style::default().fg(Color::Rgb(84,148,46))
                        .bg(if f { Color::Rgb(20,38,18) } else { Color::Rgb(10,28,14) })
                        .add_modifier(Modifier::BOLD)
                } else { theme::btn_style(false, f, tc) };
                Some(("[s]".into(), s))
            }
            4 if !is_special => {
                let f = sel && nav.track_element == TrackElement::RecordArm;
                let s = if track.armed {
                    Style::default().fg(Color::Rgb(180,50,50))
                        .bg(if f { Color::Rgb(35,20,20) } else { theme::BG })
                        .add_modifier(Modifier::BOLD)
                } else { theme::btn_style(false, f, tc) };
                let t = if track.armed { "\u{25CF}r " } else { " r " };
                Some((t.into(), s))
            }
            _ => None,
        };

        if let Some((text, style)) = btn {
            sp.push(Span::styled(text, style));
        }

        // Route indicator on row 0 for special tracks
        if is_special && row == 0 {
            let label = match track.kind {
                TrackKind::SendA => "snd",
                TrackKind::SendB => "snd",
                TrackKind::Master => "mst",
                _ => "",
            };
            sp.push(Span::styled(label, theme::dim()));
        }

        lines.push(Line::from(sp));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

// ── Clip Area ──

fn render_clips(frame: &mut Frame, area: Rect, ctx: &TrackCtx, snap: &TransportSnapshot) {
    let TrackCtx { track, is_selected: sel, is_dimmed: dim, nav, .. } = *ctx;
    let tc = theme::track_color(track.color_index);
    let (w, h) = (area.width as usize, area.height as usize);
    if w == 0 || h == 0 { return; }
    let bw = w / VISIBLE_BARS;
    if bw == 0 { return; }

    let mut grid: Vec<Vec<(char, Style)>> = vec![vec![(' ', theme::bg()); w]; h];

    // Gridlines
    for b in 1..VISIBLE_BARS {
        let x = b * bw;
        if x < w {
            let major = b % 4 == 0;
            let s = Style::default()
                .fg(if major { Color::Rgb(13,32,50) } else { Color::Rgb(9,21,34) })
                .bg(theme::BG);
            let ch = if major { '\u{2502}' } else { '\u{2506}' };
            for row in &mut grid { row[x] = (ch, s); }
        }
    }

    // Clips
    for (ci, clip) in track.clips.iter().enumerate() {
        let focused = sel && matches!(nav.track_element, TrackElement::Clip(i) if i == ci);
        let cx: usize = track.clips[..ci].iter().map(|c| c.width as usize).sum();
        let cw = clip.width as usize;
        let ce = (cx + cw).min(w);
        if cx >= w { break; }

        let cbg = if focused { Color::Rgb((theme::tc_r(tc) as u16*18/100+10) as u8, (theme::tc_g(tc) as u16*18/100+12) as u8, (theme::tc_b(tc) as u16*18/100+15) as u8) }
            else if clip.has_content { Color::Rgb((theme::tc_r(tc) as u16*8/100+8) as u8, (theme::tc_g(tc) as u16*8/100+10) as u8, (theme::tc_b(tc) as u16*8/100+13) as u8) }
            else { theme::BG };
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
        let n_s = Style::default().fg(if focused { theme::AMBER_BRIGHT } else { theme::dim_color(tc, if dim { 20 } else { 40 }) }).bg(cbg);
        for (i, ch) in ns.chars().enumerate() {
            let x = cx+i+1;
            if x < ce && 1 < h { grid[1][x] = (ch, n_s); }
        }
    }

    // Playhead
    if snap.playing {
        let ph = snap.position_ticks as f64 / (Transport::PPQ * 4) as f64;
        let px = (ph * bw as f64) as usize;
        if px < w {
            for row in &mut grid {
                let bg = row[px].1.bg.unwrap_or(theme::BG);
                row[px] = ('\u{2502}', Style::default().fg(Color::Rgb(96,74,10)).bg(bg));
            }
        }
    }

    let lines: Vec<Line> = grid_to_lines(grid);
    frame.render_widget(Paragraph::new(lines), area);
}

// ── Clip View (FX Panel left + Piano Roll right) ──

fn render_clip_view_tabs(frame: &mut Frame, area: Rect, nav: &NavState) {
    let focused = nav.focused_pane == Pane::ClipView;

    // Left tabs (FX panel)
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled(" \u{00B2} ", if focused { theme::amber_bright() } else { theme::normal() }));

    for tab in [FxPanelTab::TrackFx, FxPanelTab::Synth] {
        let active = nav.clip_view.fx_panel_tab == tab && nav.clip_view.focus == ClipViewFocus::FxPanel;
        let s = if active { theme::amber_bright().add_modifier(Modifier::BOLD) }
            else if focused { theme::normal() }
            else { theme::dim() };
        spans.push(Span::styled(format!("[{}]", tab.label()), s));
        spans.push(Span::styled(" ", theme::bg()));
    }

    spans.push(Span::styled(" \u{2502} ", theme::border_style()));

    // Right tabs (clip/piano)
    for tab in [ClipTab::PianoRoll, ClipTab::Automation] {
        let active = nav.clip_view.clip_tab == tab && nav.clip_view.focus == ClipViewFocus::PianoRoll;
        let s = if active { theme::amber_bright().add_modifier(Modifier::BOLD) }
            else if focused { theme::normal() }
            else { theme::dim() };
        spans.push(Span::styled(format!("[{}]", tab.label()), s));
        spans.push(Span::styled(" ", theme::bg()));
    }

    if let Some(t) = nav.active_clip_track() {
        if let Some(c) = nav.active_clip() {
            spans.push(Span::styled(
                format!(" {} \u{00B7} clip {}", t.name.to_uppercase(), c.number),
                theme::muted()));
        }
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_clip_view(frame: &mut Frame, area: Rect, nav: &NavState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(FX_PANEL_W), // FX panel
            Constraint::Length(1),          // separator
            Constraint::Min(10),           // piano roll / clip content
        ])
        .split(area);

    render_fx_panel(frame, cols[0], nav);

    let sep: Vec<Line> = (0..area.height)
        .map(|_| Line::from(Span::styled("\u{2502}", theme::border_style())))
        .collect();
    frame.render_widget(Paragraph::new(sep), cols[1]);

    render_piano_roll(frame, cols[2], nav);
}

fn render_fx_panel(frame: &mut Frame, area: Rect, nav: &NavState) {
    let h = area.height as usize;
    let w = area.width as usize;
    if h == 0 || w == 0 { return; }

    let focused = nav.focused_pane == Pane::ClipView && nav.clip_view.focus == ClipViewFocus::FxPanel;

    let mut lines: Vec<Line> = Vec::new();

    // Synth tab: show synth parameters with knob-style controls
    if nav.clip_view.fx_panel_tab == FxPanelTab::Synth {
        let track = nav.tracks.get(nav.track_cursor);
        let params = track.map(|t| &t.synth_params).cloned().unwrap_or_default();

        if params.is_empty() {
            lines.push(Line::from(Span::styled("  (no synth)", theme::dim())));
        } else {
            use phosphor_dsp::synth::{PARAM_NAMES, P_WAVEFORM, PARAM_COUNT};

            // Scroll: keep cursor visible within the panel height
            let param_count = params.len().min(PARAM_COUNT);
            let visible_rows = h.saturating_sub(2); // leave room for controls hint
            let cursor = nav.clip_view.synth_param_cursor;
            let scroll_offset = if cursor >= visible_rows {
                cursor - visible_rows + 1
            } else {
                0
            };

            for (i, &val) in params[..param_count].iter().enumerate().skip(scroll_offset).take(visible_rows) {
                let is_cur = focused && nav.clip_view.synth_param_cursor == i;
                let name = PARAM_NAMES.get(i).copied().unwrap_or("?");

                let indicator = if is_cur { "\u{25B6}" } else { " " };
                let name_s = if is_cur { theme::amber_bright().add_modifier(Modifier::BOLD) } else { theme::normal() };
                let dim_s = if is_cur { theme::amber() } else { theme::dim() };

                // Special display for waveform selector
                if i == P_WAVEFORM {
                    let wf = match (val * 4.0) as u8 {
                        0 => "sine", 1 => "saw", 2 => "square", _ => "tri",
                    };
                    lines.push(Line::from(vec![
                        Span::styled(format!(" {indicator} "), name_s),
                        Span::styled(format!("{name:<8}"), name_s),
                        Span::styled(format!(" {wf}"), dim_s),
                    ]));
                } else {
                    // Bar display
                    let bar_w = (w.saturating_sub(14)).min(10);
                    let filled = (val * bar_w as f32) as usize;
                    let bar: String = "\u{2588}".repeat(filled)
                        + &"\u{2591}".repeat(bar_w.saturating_sub(filled));

                    // Format value nicely
                    let display_val = match i {
                        7 | 8 | 10 => format!("{:.0}ms", val * 2000.0), // attack/decay/release
                        _ => format!("{:.0}%", val * 100.0),
                    };

                    lines.push(Line::from(vec![
                        Span::styled(format!(" {indicator} "), name_s),
                        Span::styled(format!("{name:<8}"), name_s),
                        Span::styled(bar, if is_cur { theme::amber() } else { theme::muted() }),
                        Span::styled(format!(" {display_val}"), dim_s),
                    ]));
                }
            }

            // Controls hint
            if focused {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("  h/l", theme::dim()),
                    Span::styled(" adjust  ", theme::muted()),
                    Span::styled("jk", theme::dim()),
                    Span::styled(" select", theme::muted()),
                ]));
            }
        }
    } else {
        // TrackFx tab: show FX chain
        let fx_chain: &[FxInstance] = nav.active_clip_track()
            .map(|t| t.fx_chain.as_slice())
            .unwrap_or(&[]);

        if fx_chain.is_empty() {
            lines.push(Line::from(Span::styled("  (no fx)", theme::dim())));
            lines.push(Line::from(Span::styled("  enter on [fx]", theme::dim())));
            lines.push(Line::from(Span::styled("  to add", theme::dim())));
        } else {
            for (i, fx) in fx_chain.iter().enumerate() {
                let is_cur = focused && nav.clip_view.fx_cursor == i;
                let s = if is_cur { theme::amber_bright().add_modifier(Modifier::BOLD) }
                    else if fx.enabled { theme::normal() }
                    else { theme::dim() };

                let indicator = if is_cur { "\u{25B6}" } else { " " };
                let enabled = if fx.enabled { "\u{25CF}" } else { "\u{25CB}" };
                lines.push(Line::from(vec![
                    Span::styled(format!(" {indicator} {enabled} "), s),
                    Span::styled(fx.fx_type.label(), s),
                ]));

                if is_cur {
                    for (name, val) in &fx.params {
                        let bar_w = 8;
                        let filled = (val * bar_w as f32) as usize;
                        let bar: String = "\u{2588}".repeat(filled) + &"\u{2591}".repeat(bar_w - filled);
                        lines.push(Line::from(vec![
                            Span::styled(format!("     {name:<6}"), theme::dim()),
                            Span::styled(bar, theme::muted()),
                            Span::styled(format!(" {:.0}%", val * 100.0), theme::dim()),
                        ]));
                    }
                }
            }
        }
    }

    lines.truncate(h);
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_piano_roll(frame: &mut Frame, area: Rect, nav: &NavState) {
    let (w, h) = (area.width as usize, area.height as usize);
    if w == 0 || h == 0 { return; }

    let track = match nav.active_clip_track() {
        Some(t) => t,
        None => {
            frame.render_widget(Paragraph::new(Span::styled("  select a track", theme::dim())), area);
            return;
        }
    };
    let clip = nav.active_clip();
    let notes = clip.map(|c| c.notes.as_slice()).unwrap_or(&[]);
    let tc = theme::track_color(track.color_index);

    let pr = &nav.clip_view.piano_roll;
    let focused = nav.focused_pane == Pane::ClipView && nav.clip_view.focus == ClipViewFocus::PianoRoll;
    let key_w = 6usize;
    let note_w = w.saturating_sub(key_w + 1);

    let mut lines: Vec<Line> = Vec::new();

    for row in 0..h {
        let note_i = pr.view_bottom_note as i16 + (h as i16 - 1 - row as i16);
        if !(0..=127).contains(&note_i) {
            lines.push(Line::from(Span::styled(" ".repeat(w), theme::bg())));
            continue;
        }
        let note = note_i as u8;
        let is_cur = focused && note == pr.cursor_note;
        let black = is_black_key(note);

        let key_bg = if is_cur { Color::Rgb(25,45,55) } else if black { Color::Rgb(6,14,22) } else { Color::Rgb(12,26,38) };
        let key_fg = if is_cur { theme::AMBER_BRIGHT } else if note % 12 == 0 { theme::NORMAL } else { theme::DIM };

        let mut spans: Vec<Span> = Vec::new();
        spans.push(Span::styled(format!("{:>5} ", midi_note_name(note)), Style::default().fg(key_fg).bg(key_bg)));
        spans.push(Span::styled("\u{2502}",
            if note % 12 == 0 { Style::default().fg(Color::Rgb(18,42,56)).bg(theme::BG) }
            else { theme::border_style() }));

        let row_bg = if is_cur { Color::Rgb(18,35,45) } else if black { Color::Rgb(7,16,25) } else { Color::Rgb(8,18,28) };
        let mut gr = vec![(' ', Style::default().fg(theme::DIM).bg(row_bg)); note_w];

        // Beat gridlines
        let beat_w = if note_w > 16 { note_w / CLIP_MEASURES.min(16) } else { 1 };
        for b in 1..CLIP_MEASURES.min(16) {
            let x = b * beat_w;
            if x < note_w {
                gr[x] = (if b%4==0 { '\u{2502}' } else { '\u{2506}' },
                    Style::default().fg(if b%4==0 { Color::Rgb(16,36,50) } else { Color::Rgb(10,24,36) }).bg(row_bg));
            }
        }

        // Draw MIDI notes from the active clip
        let note_style = Style::default().fg(tc).bg(
            if is_cur { Color::Rgb(25, 50, 60) } else { row_bg }
        ).add_modifier(Modifier::BOLD);
        for n in notes {
            if n.note == note {
                let sx = (n.start_frac * note_w as f64) as usize;
                let ex = ((n.start_frac + n.duration_frac) * note_w as f64) as usize;
                let ex = ex.max(sx + 1).min(note_w); // at least 1 cell wide
                for cell in gr.iter_mut().take(ex).skip(sx) {
                    *cell = ('\u{2588}', note_style);
                }
            }
        }

        // Merge grid cells into spans
        let mut text = String::new();
        let mut cur_s = Style::default().fg(theme::DIM).bg(row_bg);
        for (ch, s) in gr {
            if s == cur_s { text.push(ch); }
            else {
                if !text.is_empty() { spans.push(Span::styled(std::mem::take(&mut text), cur_s)); }
                cur_s = s; text.push(ch);
            }
        }
        if !text.is_empty() { spans.push(Span::styled(text, cur_s)); }

        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

// ── FX Menu Overlay ──

// ── Space Menu Overlay ──

fn render_space_menu(frame: &mut Frame, nav: &NavState) {
    let area = frame.area();
    let mh = 10u16.min(area.height.saturating_sub(2));
    let my = area.height.saturating_sub(mh + 1);
    let menu_area = Rect::new(0, my, area.width, mh);

    frame.render_widget(Clear, menu_area);
    let block = Block::default()
        .style(Style::default().bg(Color::Rgb(8, 18, 28)))
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(" space ", theme::amber_bright().add_modifier(Modifier::BOLD)));
    frame.render_widget(block, menu_area);

    let inner = Rect::new(1, my + 1, area.width.saturating_sub(2), mh.saturating_sub(2));

    let tab_line = Line::from(vec![
        Span::styled(" [actions] ",
            if nav.space_menu.section == SpaceMenuSection::Actions { theme::amber_bright().add_modifier(Modifier::BOLD) } else { theme::dim() }),
        Span::styled(" [help] ",
            if nav.space_menu.section == SpaceMenuSection::Help { theme::amber_bright().add_modifier(Modifier::BOLD) } else { theme::dim() }),
        Span::styled("  tab\u{2192}switch  esc\u{2192}close  +/-\u{2192}bpm", theme::dim()),
    ]);
    frame.render_widget(Paragraph::new(tab_line), Rect::new(inner.x, inner.y, inner.width, 1));

    let list_area = Rect::new(inner.x, inner.y + 1, inner.width, inner.height.saturating_sub(1));

    match nav.space_menu.section {
        SpaceMenuSection::Actions => {
            // Render in columns: fill top-to-bottom, left-to-right
            let items = SPACE_ACTIONS;
            let col_w = 30usize; // width per column
            let rows = list_area.height as usize;
            let cols = if rows > 0 { (items.len() + rows - 1) / rows } else { 1 };

            let mut lines: Vec<Line> = Vec::new();
            for row in 0..rows {
                let mut spans: Vec<Span> = Vec::new();
                for col in 0..cols {
                    let idx = col * rows + row;
                    if idx < items.len() {
                        let (key, label, _desc) = items[idx];
                        let is_cur = nav.space_menu.cursor == idx;
                        let indicator = if is_cur { "\u{25B6}" } else { " " };
                        let key_s = if is_cur { theme::amber_bright().add_modifier(Modifier::BOLD) } else { theme::amber() };
                        let label_s = if is_cur {
                            Style::default().fg(theme::HIGHLIGHT).bg(Color::Rgb(8, 18, 28)).add_modifier(Modifier::BOLD)
                        } else { theme::normal() };

                        spans.push(Span::styled(format!("{indicator} "), label_s));
                        spans.push(Span::styled(format!("{:<7}", key), key_s));
                        spans.push(Span::styled(format!("{:<12}", label), label_s));
                        // pad to column width
                        let used = 2 + 7 + 12;
                        if col_w > used {
                            spans.push(Span::styled(" ".repeat(col_w - used), Style::default().bg(Color::Rgb(8, 18, 28))));
                        }
                    }
                }
                if !spans.is_empty() {
                    lines.push(Line::from(spans));
                }
            }
            frame.render_widget(Paragraph::new(lines), list_area);
        }
        SpaceMenuSection::Help => {
            let mut lines: Vec<Line> = Vec::new();
            for (i, (title, desc)) in HELP_TOPICS.iter().enumerate() {
                let is_cur = nav.space_menu.cursor == i;
                let indicator = if is_cur { "\u{25B6} " } else { "  " };
                let s = if is_cur {
                    Style::default().fg(theme::HIGHLIGHT).bg(Color::Rgb(8, 18, 28)).add_modifier(Modifier::BOLD)
                } else { theme::normal() };

                lines.push(Line::from(vec![
                    Span::styled(indicator, s),
                    Span::styled(format!("{:<14}", title), s),
                    Span::styled(*desc, theme::dim()),
                ]));
            }
            frame.render_widget(Paragraph::new(lines), list_area);
        }
    }
}

fn render_instrument_modal(frame: &mut Frame, nav: &NavState) {
    let area = frame.area();
    let mw = 40u16;
    let mh = 10u16;
    let mx = (area.width.saturating_sub(mw)) / 2;
    let my = (area.height.saturating_sub(mh)) / 2;
    let menu_area = Rect::new(mx, my, mw, mh);

    frame.render_widget(Clear, menu_area);
    let block = Block::default()
        .style(Style::default().bg(Color::Rgb(10, 22, 34)))
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(" add instrument ", theme::amber_bright().add_modifier(Modifier::BOLD)));
    frame.render_widget(block, menu_area);

    let inner = Rect::new(mx + 2, my + 2, mw - 4, mh - 3);

    let mut lines: Vec<Line> = Vec::new();
    for (i, inst) in InstrumentType::ALL.iter().enumerate() {
        let is_cur = nav.instrument_modal.cursor == i;
        let indicator = if is_cur { "\u{25B6} " } else { "  " };
        let name_s = if is_cur {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else {
            theme::normal()
        };

        lines.push(Line::from(vec![
            Span::styled(indicator, name_s),
            Span::styled(inst.label(), name_s),
        ]));
        lines.push(Line::from(vec![
            Span::styled("    ", theme::bg()),
            Span::styled(inst.description(), theme::dim()),
        ]));
        lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_fx_menu(frame: &mut Frame, nav: &NavState) {
    let area = frame.area();
    // Position menu near center
    let mw = 28u16;
    let mh = 10u16;
    let mx = (area.width.saturating_sub(mw)) / 2;
    let my = (area.height.saturating_sub(mh)) / 2;
    let menu_area = Rect::new(mx, my, mw, mh);

    // Background
    frame.render_widget(Clear, menu_area);
    let block = Block::default()
        .style(Style::default().bg(Color::Rgb(10, 22, 34)))
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(" add fx ", theme::amber_bright()));
    frame.render_widget(block, menu_area);

    let inner = Rect::new(mx + 1, my + 1, mw - 2, mh - 2);

    let items: Vec<(&str, bool)> = FxType::ALL.iter().map(|f| (f.label(), false)).collect();

    let mut lines: Vec<Line> = Vec::new();
    for (i, (label, active)) in items.iter().enumerate() {
        let is_cur = nav.fx_menu.cursor == i;
        let indicator = if is_cur { "\u{25B6} " } else { "  " };
        let check = if *active { "\u{25CF} " } else { "  " };
        let s = if is_cur { theme::amber_bright().add_modifier(Modifier::BOLD) } else { theme::normal() };
        lines.push(Line::from(vec![
            Span::styled(indicator, s),
            Span::styled(check, if *active { theme::amber() } else { theme::dim() }),
            Span::styled(*label, s),
        ]));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

// ── Separator + Helpers ──

fn render_sep(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(Span::styled("\u{2500}".repeat(area.width as usize), theme::border_style())),
        area);
}

fn render_bottom_bar(frame: &mut Frame, area: Rect, nav: &NavState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(16), Constraint::Min(20), Constraint::Length(42)])
        .split(area);

    let (mt, ms) = if nav.loop_editor.active {
        ("-- LOOP --", Style::default().fg(Color::Rgb(80, 180, 80)).bg(theme::BG))
    } else if nav.focused_pane == Pane::Transport {
        ("-- TRANSPORT --", theme::amber_bright())
    } else if nav.track_selected {
        ("-- SELECT --", theme::amber())
    } else {
        ("-- NORMAL --", theme::normal())
    };
    frame.render_widget(Paragraph::new(Span::styled(format!(" {mt} "), ms)), cols[0]);

    let d = "\u{00B7}";
    let keys: Vec<(&str, &str)> = if nav.loop_editor.active {
        let toggle = if nav.loop_editor.enabled { "off" } else { "on" };
        vec![("hl","start"),("H/L","end"),("enter", toggle),("esc","done")]
    } else {
        match nav.focused_pane {
            Pane::Transport => vec![("tab","next"),("spc","menu"),("+/-","bpm")],
            Pane::Tracks if nav.track_selected => vec![("hl","clip"),("m","mute"),("s","solo"),("r","arm"),("R","rec"),("esc","back")],
            Pane::Tracks => vec![("jk","track"),("enter","sel"),("m","mute"),("s","solo"),("r","arm"),("R","rec")],
            Pane::ClipView => vec![("jk","nav"),("hl","panel"),("tab","tabs"),("esc","back")],
        }
    };
    let ks: Vec<Span> = keys.iter().flat_map(|(k,v)| vec![
        Span::styled(*k, theme::dim()),
        Span::styled(format!("{d}{v}  "), theme::muted()),
    ]).collect();
    frame.render_widget(Paragraph::new(Line::from(ks)), cols[1]);

    let mut right: Vec<Span> = Vec::new();
    for p in [Pane::Transport, Pane::Tracks, Pane::ClipView] {
        let a = nav.focused_pane == p;
        let s = if a { theme::amber_bright().add_modifier(Modifier::BOLD) } else { theme::dim() };
        right.push(Span::styled(format!("spc+{}", p.number()), s));
        right.push(Span::styled(format!("{d}{}  ", p.label()), if a { theme::amber() } else { theme::muted() }));
    }
    let nb = nav.number_buf.display();
    if !nb.is_empty() {
        right.push(Span::styled("clip:", theme::dim()));
        right.push(Span::styled(nb.to_string(), theme::amber_bright().add_modifier(Modifier::BOLD)));
        right.push(Span::styled("_ ", theme::amber()));
    }
    right.push(Span::styled(":q", theme::dim()));
    frame.render_widget(Paragraph::new(Line::from(right)).alignment(Alignment::Right), cols[2]);
}

fn grid_to_lines(grid: Vec<Vec<(char, Style)>>) -> Vec<Line<'static>> {
    grid.into_iter().map(|row| {
        let mut spans: Vec<Span> = Vec::new();
        let mut text = String::new();
        let mut cs = theme::bg();
        for (ch, s) in row {
            if s == cs { text.push(ch); }
            else {
                if !text.is_empty() { spans.push(Span::styled(std::mem::take(&mut text), cs)); }
                cs = s; text.push(ch);
            }
        }
        if !text.is_empty() { spans.push(Span::styled(text, cs)); }
        Line::from(spans)
    }).collect()
}

fn midi_note_name(n: u8) -> String {
    const N: [&str;12] = ["C","C#","D","D#","E","F","F#","G","G#","A","A#","B"];
    format!("{}{}", N[n as usize%12], (n as i8/12)-1)
}

fn is_black_key(n: u8) -> bool { matches!(n%12, 1|3|6|8|10) }
