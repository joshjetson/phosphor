//! UI rendering: top bar.

use super::*;

pub(super) fn render_top_bar(frame: &mut Frame, area: Rect, nav: &NavState, snap: &TransportSnapshot) {
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
    let hi = theme::transport_hi_bg();

    // BPM
    let bpm_sel = tp && te == TransportElement::Bpm;
    let bpm_bg = if bpm_sel { hi } else { theme::bg_val() };
    let bpm_fg = if editing && bpm_sel {
        theme::playhead_fg()
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
            .bg(if rec_sel { hi } else { theme::bg_val() }))
    } else {
        Span::styled("\u{25CF} rec", Style::default()
            .fg(if rec_sel { theme::NORMAL } else { theme::REC_DIM })
            .bg(if rec_sel { hi } else { theme::bg_val() }))
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
            Style::default().fg(theme::AMBER).bg(if loop_sel { hi } else { theme::bg_val() }),
        )
    } else {
        Span::styled("loop:off", Style::default()
            .fg(if loop_sel { theme::NORMAL } else { theme::DIM })
            .bg(if loop_sel { hi } else { theme::bg_val() }))
    };

    // Metronome
    let met_sel = tp && te == TransportElement::Metronome;
    let met = Span::styled("\u{266A}", Style::default()
        .fg(if snap.metronome { theme::AMBER } else { theme::DIM })
        .bg(if met_sel { hi } else { theme::bg_val() }));

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

pub(super) fn render_ruler(frame: &mut Frame, area: Rect, nav: &NavState, snap: &TransportSnapshot) {
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
            Style::default().fg(Color::Rgb(80, 180, 80)).bg(theme::bg_val()).add_modifier(Modifier::BOLD)
        } else if loop_focused && bar_num == loop_end - 1 {
            Style::default().fg(Color::Rgb(180, 80, 80)).bg(theme::bg_val()).add_modifier(Modifier::BOLD)
        } else if in_loop {
            Style::default().fg(Color::Rgb(50, 100, 110)).bg(theme::bg_val())
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

