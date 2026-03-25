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

const HEADER_W: u16 = 12;
const TRACK_H: u16 = 3;
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
    crate::debug_log::log("RENDER", &format!(
        "area={}x{} tracks={} clip_view={} pane={:?}",
        area.width, area.height, nav.tracks.len(), nav.clip_view_visible, nav.focused_pane,
    ));
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(theme::bg()), area);

    let actual_track_count = nav.visible_tracks().len().min(MAX_VISIBLE_TRACKS);
    let tracks_h = (actual_track_count as u16) * TRACK_H;

    let mut constraints = vec![
        Constraint::Length(1),       // top bar (buffer 1)
        Constraint::Length(1),       // ruler
        Constraint::Length(tracks_h), // tracks (buffer 2) — sized to actual content
    ];

    if nav.clip_view_visible {
        constraints.push(Constraint::Length(1)); // clip view tabs (buffer 3 header)
        constraints.push(Constraint::Min(8));    // clip view content (buffer 3)
    } else {
        constraints.push(Constraint::Min(0));
    }

    constraints.push(Constraint::Length(1)); // bottom bar

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut ci = 0;
    render_top_bar(frame, chunks[ci], nav, transport); ci += 1;
    render_ruler(frame, chunks[ci], nav, transport); ci += 1;
    render_tracks(frame, chunks[ci], nav, transport); ci += 1;

    if nav.clip_view_visible {
        render_clip_view_tabs(frame, chunks[ci], nav); ci += 1;
        render_clip_view(frame, chunks[ci], nav); ci += 1;
    } else {
        ci += 1;
    }
    render_bottom_bar(frame, chunks[ci], nav);

    // Overlays
    if nav.input_modal.open {
        render_input_modal(frame, nav);
    } else if nav.instrument_modal.open {
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

    let buf1_style = if nav.focused_pane == Pane::Transport { theme::amber() } else { theme::dim() };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("\u{00B9}", buf1_style), // superscript 1
            Span::styled("phosphor", theme::branding()),
        ])),
        cols[0],
    );

    let tp = nav.focused_pane == Pane::Transport;
    let te = nav.transport_ui.element;
    let editing = nav.transport_ui.editing;
    let hi = Color::Rgb(30, 45, 55); // highlight background

    // BPM
    let bpm_sel = tp && te == TransportElement::Bpm;
    let bpm_bg = if bpm_sel { hi } else { theme::BG };
    let bpm_fg = if editing && bpm_sel {
        Color::Rgb(255, 200, 50)
    } else if bpm_sel {
        theme::AMBER_BRIGHT
    } else {
        theme::AMBER_BRIGHT
    };
    let bpm_label = if editing && bpm_sel { "\u{2190}bpm\u{2192}" } else { "bpm:" };

    // Record
    let rec_sel = tp && te == TransportElement::Record;
    let rec = if snap.recording {
        Span::styled("\u{25CF} rec", Style::default()
            .fg(theme::REC_ACTIVE)
            .bg(if rec_sel { hi } else { theme::BG }))
    } else {
        Span::styled("\u{25CF} rec", Style::default()
            .fg(if rec_sel { theme::NORMAL } else { theme::REC_DIM })
            .bg(if rec_sel { hi } else { theme::BG }))
    };

    // Loop
    let loop_sel = tp && te == TransportElement::Loop;
    let loop_focused = nav.loop_editor.active;
    let loop_enabled = nav.loop_editor.enabled;
    let lp = if loop_focused {
        let label = if loop_enabled { "loop" } else { "loop?" };
        Span::styled(
            format!("{label}[{}]", nav.loop_editor.display()),
            theme::amber_bright().add_modifier(Modifier::BOLD),
        )
    } else if loop_enabled {
        Span::styled(
            format!("loop:{}", nav.loop_editor.display()),
            Style::default().fg(theme::AMBER).bg(if loop_sel { hi } else { theme::BG }),
        )
    } else {
        Span::styled("loop:off", Style::default()
            .fg(if loop_sel { theme::NORMAL } else { theme::DIM })
            .bg(if loop_sel { hi } else { theme::BG }))
    };

    // Metronome
    let met_sel = tp && te == TransportElement::Metronome;
    let met = Span::styled("\u{266A}", Style::default()
        .fg(if snap.metronome { theme::AMBER } else { theme::DIM })
        .bg(if met_sel { hi } else { theme::BG }));

    // Seq
    let seq = if snap.playing { Span::styled("seq:on", theme::normal()) } else { Span::styled("seq:off", theme::dim()) };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            seq, Span::styled(format!("  {bpm_label}"), theme::normal()),
            Span::styled(format!("{:.0}", snap.tempo_bpm), Style::default().fg(bpm_fg).bg(bpm_bg)),
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

    let buf2_style = if nav.focused_pane == Pane::Tracks { theme::amber() } else { theme::dim() };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("\u{00B2}", buf2_style), // superscript 2
            Span::styled("trk", theme::dim()),
        ])),
        cols[0],
    );
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
    let id = (b'A' + index as u8) as char;
    let is_special = matches!(track.kind, TrackKind::SendA | TrackKind::SendB | TrackKind::Master);

    // Accent bar style
    let ac = if sel { "\u{2588}" } else { "\u{2590}" };
    let ac_s = if cur || sel { Style::default().fg(tc).bg(theme::BG) }
        else { Style::default().fg(theme::dim_color(tc, if dim { 15 } else { 30 })).bg(theme::BG) };
    let id_s = Style::default().fg(theme::dim_color(tc, if dim { 20 } else { 40 })).bg(theme::BG);

    // VU — horizontal bar on row 1
    let vu_w = 3usize;
    let vu_filled = (vu_w as f32 * vu_level) as usize;

    // Record arm dot
    let arm_s = if track.armed {
        Style::default().fg(Color::Rgb(180, 50, 50)).bg(theme::BG)
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
        Style::default().fg(Color::Rgb(84, 148, 46))
            .bg(if s_f { Color::Rgb(20, 38, 18) } else { Color::Rgb(10, 28, 14) })
            .add_modifier(Modifier::BOLD)
    } else {
        theme::btn_style(false, s_f, tc)
    };

    let vu_filled = vu_filled.min(vu_w);
    let vu_bar: String = "\u{2588}".repeat(vu_filled) + &"\u{2591}".repeat(vu_w - vu_filled);
    let vu_s = Style::default()
        .fg(theme::dim_color(tc, if dim { 20 } else { 55 }))
        .bg(Color::Rgb(5, 13, 22));

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

fn render_clips(frame: &mut Frame, area: Rect, ctx: &TrackCtx, snap: &TransportSnapshot) {
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
                .fg(if major { Color::Rgb(13,32,50) } else { Color::Rgb(9,21,34) })
                .bg(theme::BG);
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
        .bg(theme::BG);
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
    let buf3_style = if focused { theme::amber_bright() } else { theme::dim() };
    spans.push(Span::styled("\u{00B3}", buf3_style)); // superscript 3
    spans.push(Span::styled(" ", theme::bg()));

    for tab in [FxPanelTab::TrackFx, FxPanelTab::Synth] {
        let active = nav.clip_view.fx_panel_tab == tab && nav.clip_view.focus == ClipViewFocus::FxPanel;
        let s = if active { theme::amber_bright().add_modifier(Modifier::BOLD) }
            else if focused { theme::normal() }
            else { theme::dim() };
        spans.push(Span::styled(format!("[{}]", tab.label()), s));
        spans.push(Span::styled(" ", theme::bg()));
    }

    spans.push(Span::styled(" \u{2502} ", theme::border_style()));

    // Right tabs (inst config / piano / auto)
    for tab in ClipTab::ALL {
        let active = nav.clip_view.clip_tab == *tab && nav.clip_view.focus == ClipViewFocus::PianoRoll;
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

    match nav.clip_view.clip_tab {
        ClipTab::InstConfig => render_inst_config(frame, cols[2], nav),
        _ => render_piano_roll(frame, cols[2], nav),
    }
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
            lines.push(Line::from(Span::styled("  (no instrument)", theme::dim())));
        } else {
            let instrument_type = track.and_then(|t| t.instrument_type);
            let is_drum = instrument_type == Some(InstrumentType::DrumRack);
            let is_dx7 = instrument_type == Some(InstrumentType::DX7);
            let is_jupiter = instrument_type == Some(InstrumentType::Jupiter8);
            let is_odyssey = instrument_type == Some(InstrumentType::Odyssey);
            let is_juno = instrument_type == Some(InstrumentType::Juno60);
            let param_names: &[&str] = if is_drum {
                &phosphor_dsp::drum_rack::PARAM_NAMES
            } else if is_dx7 {
                &phosphor_dsp::dx7::PARAM_NAMES
            } else if is_jupiter {
                &phosphor_dsp::jupiter::PARAM_NAMES
            } else if is_odyssey {
                &phosphor_dsp::odyssey::PARAM_NAMES
            } else if is_juno {
                &phosphor_dsp::juno::PARAM_NAMES
            } else {
                &phosphor_dsp::synth::PARAM_NAMES
            };
            let param_count = params.len().min(param_names.len());

            let visible_rows = h.saturating_sub(2);
            let cursor = nav.clip_view.synth_param_cursor;
            let scroll_offset = if cursor >= visible_rows {
                cursor - visible_rows + 1
            } else {
                0
            };

            for (i, &val) in params[..param_count].iter().enumerate().skip(scroll_offset).take(visible_rows) {
                let is_cur = focused && nav.clip_view.synth_param_cursor == i;
                let name = param_names.get(i).copied().unwrap_or("?");

                let indicator = if is_cur { "\u{25B6}" } else { " " };
                let name_s = if is_cur { theme::amber_bright().add_modifier(Modifier::BOLD) } else { theme::normal() };
                let dim_s = if is_cur { theme::amber() } else { theme::dim() };

                // Discrete selector (waveform, kit, patch, mode, etc.)
                let discrete_label = if is_jupiter {
                    phosphor_dsp::jupiter::discrete_label(i, val)
                } else if is_odyssey {
                    phosphor_dsp::odyssey::discrete_label(i, val)
                } else if is_juno {
                    phosphor_dsp::juno::discrete_label(i, val)
                } else if i == 0 {
                    // Index 0 is always a discrete selector for non-Jupiter instruments
                    Some(if is_drum {
                        match (val * 10.0) as u8 {
                            0 => "808", 1 => "909", 2 => "707", 3 => "606", 4 => "777",
                            5 => "tsty-1", 6 => "tsty-2", 7 => "tsty-3", 8 => "tsty-4", _ => "tsty-5",
                        }
                    } else if is_dx7 {
                        let idx = (val * (phosphor_dsp::dx7::PATCH_COUNT as f32 - 0.01)) as usize;
                        phosphor_dsp::dx7::PATCH_NAMES[idx.min(phosphor_dsp::dx7::PATCH_COUNT - 1)]
                    } else {
                        match (val * 4.0) as u8 {
                            0 => "sine", 1 => "saw", 2 => "square", _ => "tri",
                        }
                    })
                } else {
                    None
                };
                if let Some(label) = discrete_label {
                    lines.push(Line::from(vec![
                        Span::styled(format!(" {indicator} "), name_s),
                        Span::styled(format!("{name:<8}"), name_s),
                        Span::styled(format!(" {label}"), dim_s),
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
                        let filled = ((val * bar_w as f32) as usize).min(bar_w);
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

fn render_inst_config(frame: &mut Frame, area: Rect, nav: &NavState) {
    let (w, h) = (area.width as usize, area.height as usize);
    if w == 0 || h == 0 { return; }

    let focused = nav.focused_pane == Pane::ClipView
        && nav.clip_view.focus == ClipViewFocus::PianoRoll
        && nav.clip_view.clip_tab == ClipTab::InstConfig;

    let track = match nav.active_clip_track().or_else(|| nav.current_track()) {
        Some(t) => t,
        None => {
            frame.render_widget(Paragraph::new(Span::styled("  select a track", theme::dim())), area);
            return;
        }
    };

    let inst_label = track.instrument_type.map(|i| i.label()).unwrap_or("—");
    let mut lines: Vec<Line> = Vec::new();

    // Header
    lines.push(Line::from(vec![
        Span::styled(format!("  {inst_label}"), theme::amber_bright().add_modifier(Modifier::BOLD)),
        Span::styled(" instrument config", theme::dim()),
    ]));
    lines.push(Line::from(""));

    // Sections — placeholder structure for future parameters
    let sections = [
        ("LFO", &["rate", "depth", "wave", "target"][..]),
        ("Filter", &["type", "cutoff", "reso", "env amt"]),
        ("Envelope", &["attack", "decay", "sustain", "release"]),
        ("Pitch", &["bend range", "portamento", "detune"]),
    ];

    let cursor = nav.clip_view.inst_config_cursor;
    let mut param_idx = 0;

    for (section_name, params) in &sections {
        lines.push(Line::from(Span::styled(
            format!("  {section_name}"),
            theme::normal().add_modifier(Modifier::BOLD),
        )));

        for &param_name in *params {
            let is_cur = focused && cursor == param_idx;
            let indicator = if is_cur { "\u{25B6}" } else { " " };
            let name_s = if is_cur { theme::amber_bright().add_modifier(Modifier::BOLD) } else { theme::normal() };
            let dim_s = if is_cur { theme::amber() } else { theme::dim() };

            let bar_w = (w.saturating_sub(20)).min(12);
            let val = 0.0f32; // placeholder — will be wired to real params
            let filled = ((val * bar_w as f32) as usize).min(bar_w);
            let bar: String = "\u{2588}".repeat(filled)
                + &"\u{2591}".repeat(bar_w - filled);

            lines.push(Line::from(vec![
                Span::styled(format!("   {indicator} "), name_s),
                Span::styled(format!("{param_name:<12}"), name_s),
                Span::styled(bar, if is_cur { theme::amber() } else { theme::muted() }),
                Span::styled(format!(" {:.0}%", val * 100.0), dim_s),
            ]));

            param_idx += 1;
        }
        lines.push(Line::from(""));
    }

    // Controls hint
    if focused {
        lines.push(Line::from(vec![
            Span::styled("  jk", theme::dim()),
            Span::styled(" select  ", theme::muted()),
            Span::styled("hl", theme::dim()),
            Span::styled(" adjust  ", theme::muted()),
            Span::styled("tab", theme::dim()),
            Span::styled(" next panel", theme::muted()),
        ]));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_piano_roll(frame: &mut Frame, area: Rect, nav: &NavState) {
    let (w, h) = (area.width as usize, area.height as usize);
    if w == 0 || h == 0 { return; }
    crate::debug_log::log("PIANO", &format!("w={w} h={h} note_w={}", w.saturating_sub(7)));

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
    let in_col_mode = focused; // columns always visible when piano roll is focused
    let in_row_mode = focused && pr.focus == PianoRollFocus::Row;
    let key_w = 6usize;
    let note_w = w.saturating_sub(key_w + 1);

    // Column geometry
    let col_count = CLIP_MEASURES.min(16);
    let col_w = if note_w > 0 && col_count > 0 { note_w / col_count } else { 1 };

    let mut lines: Vec<Line> = Vec::new();

    // Column number header row (only when in column/row mode)
    if in_col_mode && h > 1 {
        let mut hdr_spans: Vec<Span> = Vec::new();
        hdr_spans.push(Span::styled("      ", theme::bg())); // key label width
        hdr_spans.push(Span::styled("\u{2502}", theme::border_style()));
        for c in 0..col_count {
            let col_num = c + 1;
            let is_sel = c == pr.column;
            let s = if is_sel {
                theme::amber_bright().add_modifier(Modifier::BOLD)
            } else {
                theme::dim()
            };
            hdr_spans.push(Span::styled(format!("{:<w$}", col_num, w = col_w), s));
        }
        lines.push(Line::from(hdr_spans));
    }

    let rows_for_notes = if in_col_mode && h > 1 { h - 1 } else { h };

    for row in 0..rows_for_notes {
        let note_i = pr.view_bottom_note as i16 + (rows_for_notes as i16 - 1 - row as i16);
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
        for b in 1..col_count {
            let x = b * col_w;
            if x < note_w {
                gr[x] = (if b%4==0 { '\u{2502}' } else { '\u{2506}' },
                    Style::default().fg(if b%4==0 { Color::Rgb(16,36,50) } else { Color::Rgb(10,24,36) }).bg(row_bg));
            }
        }

        // Column highlight
        if in_col_mode {
            let col_start = pr.column * col_w;
            let col_end = (col_start + col_w).min(note_w);
            let col_bg = if in_row_mode && is_cur {
                Color::Rgb(30, 55, 65) // row+column intersection
            } else {
                Color::Rgb(14, 28, 38) // column highlight
            };
            for x in col_start..col_end {
                let (ch, old_s) = gr[x];
                let fg = old_s.fg.unwrap_or(theme::DIM);
                gr[x] = (ch, Style::default().fg(fg).bg(col_bg));
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
                let ex = ex.max(sx + 1).min(note_w);
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
    // 3 lines per instrument (name + desc + blank) + 3 for border/padding
    let mh = ((InstrumentType::ALL.len() as u16) * 3 + 3).min(area.height.saturating_sub(2));
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

fn render_input_modal(frame: &mut Frame, nav: &NavState) {
    let area = frame.area();
    let mw = 50u16.min(area.width.saturating_sub(4));
    let mh = 5u16;
    let mx = (area.width.saturating_sub(mw)) / 2;
    let my = (area.height.saturating_sub(mh)) / 2;
    let menu_area = Rect::new(mx, my, mw, mh);

    frame.render_widget(Clear, menu_area);

    let title = match nav.input_modal.kind {
        InputModalKind::SaveAs => " save project ",
        InputModalKind::Open => " open project ",
    };
    let block = Block::default()
        .style(Style::default().bg(Color::Rgb(10, 22, 34)))
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(title, theme::amber_bright().add_modifier(Modifier::BOLD)));
    frame.render_widget(block, menu_area);

    let inner = Rect::new(mx + 2, my + 1, mw - 4, mh - 2);

    let prompt = match nav.input_modal.kind {
        InputModalKind::SaveAs => "filename: ",
        InputModalKind::Open => "path: ",
    };

    let buf = nav.input_modal.value();
    let cursor_pos = nav.input_modal.cursor;
    let (before, after) = buf.split_at(cursor_pos.min(buf.len()));
    let cursor_char = if after.is_empty() { "\u{2588}" } else { &after[..1] };
    let rest = if after.len() > 1 { &after[1..] } else { "" };

    let lines = vec![
        Line::from(vec![
            Span::styled(prompt, theme::dim()),
            Span::styled(before, theme::amber_bright().add_modifier(Modifier::BOLD)),
            Span::styled(cursor_char, Style::default().fg(Color::Rgb(8, 18, 28)).bg(theme::AMBER_BRIGHT)),
            Span::styled(rest, theme::amber_bright().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  enter", theme::dim()),
            Span::styled(" confirm  ", theme::muted()),
            Span::styled("esc", theme::dim()),
            Span::styled(" cancel", theme::muted()),
        ]),
    ];

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


fn render_bottom_bar(frame: &mut Frame, area: Rect, nav: &NavState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(16), Constraint::Min(20), Constraint::Length(42)])
        .split(area);

    let (mt, ms) = if nav.loop_editor.active {
        ("-- LOOP --", Style::default().fg(Color::Rgb(80, 180, 80)).bg(theme::BG))
    } else if nav.focused_pane == Pane::Transport && nav.transport_ui.editing {
        ("-- EDIT --", theme::amber_bright())
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
            Pane::Transport if nav.transport_ui.editing => vec![("hl","adjust"),("enter","done"),("esc","done")],
            Pane::Transport => vec![("hl","nav"),("enter","sel"),("+/-","bpm"),("tab","pane")],
            Pane::Tracks if nav.track_selected => vec![("hl","clip"),("m","mute"),("s","solo"),("r","arm"),("R","rec"),("esc","back")],
            Pane::Tracks => vec![("jk","track"),("enter","sel"),("m","mute"),("s","solo"),("r","arm"),("R","rec")],
            Pane::ClipView if nav.clip_view.focus == ClipViewFocus::PianoRoll
                && nav.clip_view.clip_tab == ClipTab::InstConfig =>
                vec![("jk","select"),("hl","adjust"),("tab","next"),("esc","back")],
            Pane::ClipView if nav.clip_view.focus == ClipViewFocus::PianoRoll
                && nav.clip_view.piano_roll.focus == PianoRollFocus::Row =>
                vec![("hl","left\u{2194}"),("H/L","right\u{2194}"),("jk","note"),("n","draw"),("esc","col")],
            Pane::ClipView if nav.clip_view.focus == ClipViewFocus::PianoRoll
                && nav.clip_view.piano_roll.focus == PianoRollFocus::Selected =>
                vec![("hl","left\u{2194}"),("H/L","right\u{2194}"),("jk","\u{2193}row"),("esc","nav")],
            Pane::ClipView if nav.clip_view.focus == ClipViewFocus::PianoRoll =>
                vec![("hl","col"),("1-9","jump"),("enter","sel"),("esc","back")],
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
