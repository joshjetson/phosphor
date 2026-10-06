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
    // Bright either way: which one is selected is said by the background
    // cell, not by the ink.
    let bpm_fg = if editing && bpm_sel {
        theme::playhead_fg()
    } else {
        theme::amber_bright_val()
    };
    let bpm_label = if editing && bpm_sel { "\u{2190}bpm\u{2192}" } else { "bpm:" };

    // Record
    let rec_sel = tp && te == TransportElement::Record;
    let rec = if snap.recording {
        Span::styled("\u{25CF} rec", Style::default()
            .fg(theme::rec_active_val())
            .bg(if rec_sel { hi } else { theme::bg_val() }))
    } else {
        Span::styled("\u{25CF} rec", Style::default()
            .fg(if rec_sel { theme::normal_val() } else { theme::rec_dim_val() })
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
            Style::default().fg(theme::amber_val()).bg(if loop_sel { hi } else { theme::bg_val() }),
        )
    } else {
        Span::styled("loop:off", Style::default()
            .fg(if loop_sel { theme::normal_val() } else { theme::dim_val() })
            .bg(if loop_sel { hi } else { theme::bg_val() }))
    };

    // Metronome — or the drill click, when the practice room has the
    // metronome on loan. The loaned click free-runs at its own tempo and
    // pattern by design, and by ear that is indistinguishable from "the
    // whole application has the wrong tempo"; naming it here is what keeps
    // that from ever being a mystery again.
    let met_sel = tp && te == TransportElement::Metronome;
    let met = if let Some((bpm, pattern)) = nav.practice.engine_click {
        let word = if pattern == 1 {
            format!("\u{266A}drill@{bpm}\u{00b7}2&4")
        } else {
            format!("\u{266A}drill@{bpm}")
        };
        Span::styled(word, Style::default()
            .fg(theme::rec_active_val())
            .bg(if met_sel { hi } else { theme::bg_val() }))
    } else {
        Span::styled("\u{266A}".to_string(), Style::default()
            .fg(if snap.metronome { theme::amber_val() } else { theme::dim_val() })
            .bg(if met_sel { hi } else { theme::bg_val() }))
    };

    // The audio thread missing its deadlines. Never decoration: when this
    // is on the screen, playback is genuinely running slower than the
    // song, and the debug log says so in sentences.
    let struggling = nav.audio_struggling.then(|| {
        Span::styled(
            " \u{26A0}audio",
            Style::default().fg(theme::rec_active_val()).bg(theme::bg_val()),
        )
    });

    // The safety limiter, when it is working. Silent otherwise: a readout
    // that is always on screen showing 0.0 teaches the eye to ignore it, and
    // this one only matters on the rare block where the mix went over.
    //
    // Drawn through the same widget the compressor's panel uses, in its
    // compact width. The number used to be spelled out here by hand, which
    // meant the master's reduction and a track's were two different pictures
    // of the same quantity — and the one in the top bar had no bar at all.
    let (reduction, lim_peak) = nav.limiter_gr.get();
    let lim: Vec<Span> = if nav.limiter_gr.is_active() {
        let mut spans = vec![Span::styled("  ", theme::bg())];
        spans.extend(super::meters::gr_meter_spans(
            "lim",
            reduction,
            lim_peak,
            super::meters::GR_COMPACT_WIDTH,
        ));
        spans
    } else {
        Vec::new()
    };

    // Seq
    // "seq" predates the step sequencer; with a real sequencer in the rack a
    // top-bar "seq:on" that means "the transport is rolling" reads as a lie.
    let seq = if snap.playing { Span::styled("play:on", theme::normal()) } else { Span::styled("play:off", theme::dim()) };

    // Count-in: the setting when idle, the countdown itself — beats left,
    // loud and bold — while the bars click down.
    let cnt_sel = tp && te == TransportElement::CountIn;
    let cnt = if snap.count_in_remaining > 0 {
        let beats = (snap.count_in_remaining + Transport::PPQ - 1) / Transport::PPQ;
        Span::styled(
            format!("count \u{00B7} {beats}"),
            theme::amber_bright().add_modifier(Modifier::BOLD),
        )
    } else if snap.count_in_bars > 0 {
        Span::styled(
            format!("cnt:{}", snap.count_in_bars),
            Style::default()
                .fg(theme::amber_val())
                .bg(if cnt_sel { hi } else { theme::bg_val() }),
        )
    } else {
        Span::styled("cnt:off", Style::default()
            .fg(if cnt_sel { theme::normal_val() } else { theme::dim_val() })
            .bg(if cnt_sel { hi } else { theme::bg_val() }))
    };

    // Take mode: dub layers passes up, new clears the range first. While
    // recording it shows the running pass count instead — the stack's
    // visible depth. Silent in the default state: a cell that always says
    // "dub" is noise, and the bar's width belongs to the limiter readout.
    let mode_sel = tp && te == TransportElement::RecordMode;
    let take = if snap.recording && nav.take_count > 0 {
        Some(Span::styled(
            format!("take {}", nav.take_count),
            theme::amber_bright().add_modifier(Modifier::BOLD),
        ))
    } else if nav.record_replace || mode_sel {
        Some(Span::styled(
            if nav.record_replace { "take:new" } else { "take:dub" },
            Style::default()
                .fg(if nav.record_replace { theme::amber_val() } else { theme::normal_val() })
                .bg(if mode_sel { hi } else { theme::bg_val() }),
        ))
    } else {
        None
    };

    let mut middle = vec![
        seq,
        Span::styled(format!("  {bpm_label}"), theme::normal()),
        Span::styled(format!("{:.0}", snap.tempo_bpm), Style::default().fg(bpm_fg).bg(bpm_bg)),
        Span::styled("  4/4  ", theme::normal()),
        rec,
        Span::styled("  ", theme::bg()),
        lp,
        Span::styled("  ", theme::bg()),
        met,
        Span::styled("  ", theme::bg()),
        cnt,
    ];
    if let Some(take) = take {
        middle.push(Span::styled("  ", theme::bg()));
        middle.push(take);
    }
    if let Some(warn) = struggling {
        middle.push(warn);
    }
    middle.extend(lim);
    frame.render_widget(
        Paragraph::new(Line::from(middle)).alignment(Alignment::Center),
        cols[1],
    );

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
    if w == 0 {
        return;
    }
    let view = lane_view(nav, snap);
    let bar_ticks = phosphor_app::timeline::TICKS_PER_BAR;
    let mut cells: Vec<(char, Style)> = vec![(' ', theme::dim()); w];

    // Bar numbers, as many as fit: every bar when there is room, every
    // second or fourth (and so on) when the lanes are zoomed out, and the
    // beats too when zoomed far enough in to aim at one.
    let cells_per_bar = (w as i64 * bar_ticks / view.ticks()).max(1);
    let widest = format!("{}", view.first_bar + view.bars).len() as i64 + 1;
    let every = [1i64, 2, 4, 8, 16, 32, 64, 128]
        .into_iter()
        .find(|&n| n * cells_per_bar >= widest)
        .unwrap_or(128);
    let beat = Transport::PPQ;
    let show_beats = cells_per_bar >= 16;
    let mut tick = view.first_tick();
    while tick < view.end_tick() {
        let x = view.column(tick, w);
        let bar = tick / bar_ticks;
        let on_bar = tick % bar_ticks == 0;
        let label = if on_bar && bar % every == 0 {
            Some(((bar + 1).to_string(), if bar % 4 == 0 { theme::normal() } else { theme::dim() }))
        } else if !on_bar && show_beats {
            Some((format!("{}", (tick % bar_ticks) / beat + 1), theme::dim()))
        } else {
            None
        };
        if let (Some((text, style)), true) = (label, (0..w as i64).contains(&x)) {
            for (i, ch) in text.chars().enumerate() {
                if let Some(cell) = cells.get_mut(x as usize + i) {
                    *cell = (ch, cell.1.patch(style).bg(cell.1.bg.unwrap_or(theme::bg_val())));
                }
            }
        }
        tick += if show_beats { beat } else { bar_ticks };
    }

    // The brace, exactly where it is: a band across the ruler with its two
    // edges marked, a column wide at the least, so a thirty-second shows.
    // Drawn over the numbers, which stay readable on it.
    let loop_focused = nav.loop_editor.active;
    if brace_shown(nav) {
        let (start, end) = (nav.loop_editor.start, nav.loop_editor.end);
        if let Some((a, b)) = span_columns(&view, start, end, w) {
            for cell in &mut cells[a..b] {
                let fg = if cell.0 == ' ' { Color::Rgb(50, 100, 110) } else { theme::normal_val() };
                cell.1 = Style::default().fg(fg).bg(theme::loop_band());
            }
            if loop_focused {
                let edge = |c: Color| Style::default().fg(c).bg(theme::loop_band()).add_modifier(Modifier::BOLD);
                if view.column(start, w) >= 0 {
                    cells[a] = ('\u{258F}', edge(Color::Rgb(80, 180, 80)));
                }
                if view.column(end, w) <= w as i64 {
                    cells[b - 1] = ('\u{2595}', edge(Color::Rgb(180, 80, 80)));
                }
            }
        }
    }

    if snap.playing {
        let px = view.column(snap.position_ticks, w);
        if (0..w as i64).contains(&px) {
            let cell = &mut cells[px as usize];
            *cell = (if cell.0 == ' ' { '\u{25BC}' } else { cell.0 }, theme::amber());
        }
    }

    let spans: Vec<Span> = cells.into_iter().map(|(ch, st)| Span::styled(ch.to_string(), st)).collect();
    frame.render_widget(Paragraph::new(Line::from(spans)), cols[2]);
}

// ── Tracks ──

