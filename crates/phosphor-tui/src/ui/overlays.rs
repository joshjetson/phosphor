//! UI rendering: overlays.

use super::*;

pub(super) fn render_space_menu(frame: &mut Frame, nav: &NavState) {
    let area = frame.area();
    let mh = 10u16.min(area.height.saturating_sub(2));
    let my = area.height.saturating_sub(mh + 1);
    let menu_area = Rect::new(0, my, area.width, mh);

    frame.render_widget(Clear, menu_area);
    let block = Block::default()
        .style(Style::default().bg(theme::overlay_bg()))
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
                            Style::default().fg(theme::highlight_val()).bg(theme::overlay_bg()).add_modifier(Modifier::BOLD)
                        } else { theme::normal() };

                        spans.push(Span::styled(format!("{indicator} "), label_s));
                        spans.push(Span::styled(format!("{:<7}", key), key_s));
                        spans.push(Span::styled(format!("{:<12}", label), label_s));
                        // pad to column width
                        let used = 2 + 7 + 12;
                        if col_w > used {
                            spans.push(Span::styled(" ".repeat(col_w - used), Style::default().bg(theme::overlay_bg())));
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
                    Style::default().fg(theme::highlight_val()).bg(theme::overlay_bg()).add_modifier(Modifier::BOLD)
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

pub(super) fn render_instrument_modal(frame: &mut Frame, nav: &NavState) {
    let area = frame.area();
    let mw = 40u16;
    // 3 lines per instrument (name + desc + blank) + 3 for border/padding
    let mh = ((InstrumentType::ALL.len() as u16) * 3 + 3).min(area.height.saturating_sub(2));
    let mx = (area.width.saturating_sub(mw)) / 2;
    let my = (area.height.saturating_sub(mh)) / 2;
    let menu_area = Rect::new(mx, my, mw, mh);

    frame.render_widget(Clear, menu_area);
    let block = Block::default()
        .style(Style::default().bg(theme::overlay_bg()))
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

pub(super) fn render_confirm_modal(frame: &mut Frame, nav: &NavState) {
    let area = frame.area();
    let msg = &nav.confirm_modal.message;
    let mw = (msg.len() as u16 + 6).min(area.width.saturating_sub(4)).max(30);
    let mh = 4u16;
    let mx = (area.width.saturating_sub(mw)) / 2;
    let my = (area.height.saturating_sub(mh)) / 2;
    let menu_area = Rect::new(mx, my, mw, mh);

    frame.render_widget(Clear, menu_area);
    let block = Block::default()
        .style(Style::default().bg(theme::overlay_bg()))
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(theme::rec_active_val()))
        .title(Span::styled(" confirm ", Style::default().fg(theme::rec_active_val()).add_modifier(Modifier::BOLD)));
    frame.render_widget(block, menu_area);

    let inner = Rect::new(mx + 2, my + 1, mw - 4, mh - 2);
    let lines = vec![
        Line::from(Span::styled(msg.as_str(), theme::normal())),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn render_input_modal(frame: &mut Frame, nav: &NavState) {
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
        InputModalKind::PresetName => " name preset ",
    };
    let block = Block::default()
        .style(Style::default().bg(theme::overlay_bg()))
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(title, theme::amber_bright().add_modifier(Modifier::BOLD)));
    frame.render_widget(block, menu_area);

    let inner = Rect::new(mx + 2, my + 1, mw - 4, mh - 2);

    let prompt = match nav.input_modal.kind {
        InputModalKind::SaveAs => "filename: ",
        InputModalKind::Open => "path: ",
        InputModalKind::PresetName => "name: ",
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
            Span::styled(cursor_char, Style::default().fg(theme::overlay_bg()).bg(theme::amber_bright_val())),
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

pub(super) fn render_preset_modal(frame: &mut Frame, nav: &NavState) {
    let pm = &nav.preset_modal;
    let area = frame.area();
    let mw = 46u16.min(area.width.saturating_sub(4));

    // The save row plus one line per preset, then the hint line if there is
    // one, a blank, and the footer. Capped to the terminal, so a full bank
    // scrolls inside the modal rather than drawing off the bottom.
    let hint = pm.error.is_some() || pm.entries.is_empty();
    let content = pm.item_count() + usize::from(hint) + 2;
    // Never larger than the terminal: a modal taller than the buffer it is
    // drawn into is an index past the end of the buffer.
    let mh = (content.min(u16::MAX as usize) as u16 + 2).min(area.height.saturating_sub(2));
    let mx = (area.width.saturating_sub(mw)) / 2;
    let my = (area.height.saturating_sub(mh)) / 2;
    let menu_area = Rect::new(mx, my, mw, mh);

    frame.render_widget(Clear, menu_area);
    let title = match pm.instrument {
        Some(inst) => format!(" presets \u{2014} {} ", inst.label()),
        None => " presets ".to_string(),
    };
    let block = Block::default()
        .style(Style::default().bg(theme::overlay_bg()))
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(title, theme::amber_bright().add_modifier(Modifier::BOLD)));
    frame.render_widget(block, menu_area);

    let inner = Rect::new(mx + 2, my + 1, mw.saturating_sub(4), mh.saturating_sub(2));
    // Reserve the blank line, the footer, and the hint line if it is showing.
    let reserved = 2 + u16::from(hint);
    let list_rows = inner.height.saturating_sub(reserved).max(1) as usize;

    // Scroll the list so the cursor stays on screen in a long bank.
    let first = pm.cursor.saturating_sub(list_rows.saturating_sub(1));

    let row_style = |selected: bool| -> (Style, &'static str) {
        if selected {
            (theme::amber_bright().add_modifier(Modifier::BOLD), "\u{25B6} ")
        } else {
            (theme::normal(), "  ")
        }
    };

    let mut lines: Vec<Line> = Vec::new();
    for row in first..pm.item_count() {
        if lines.len() >= list_rows { break; }
        let (style, indicator) = row_style(pm.cursor == row);
        if row == PresetModal::SAVE_ROW {
            lines.push(Line::from(vec![
                Span::styled(indicator, style),
                Span::styled("[ save current panel ]", style),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(indicator, style),
                Span::styled(pm.entries[row - 1].as_str(), style),
            ]));
        }
    }

    if let Some(err) = &pm.error {
        lines.push(Line::from(Span::styled(
            err.as_str(),
            Style::default().fg(theme::rec_active_val()),
        )));
    } else if pm.entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no saved presets yet",
            theme::dim(),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  j/k", theme::dim()),
        Span::styled(" nav  ", theme::muted()),
        Span::styled("enter", theme::dim()),
        Span::styled(" save/load  ", theme::muted()),
        Span::styled("d", theme::dim()),
        Span::styled(" delete  ", theme::muted()),
        Span::styled("esc", theme::dim()),
        Span::styled(" close", theme::muted()),
    ]));

    frame.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn render_fx_menu(frame: &mut Frame, nav: &NavState) {
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
        .style(Style::default().bg(theme::overlay_bg()))
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

pub(super) fn render_quantize_modal(frame: &mut Frame, nav: &NavState) {
    let area = frame.area();
    let mw = 36u16.min(area.width.saturating_sub(4));
    let mh = 9u16;
    let mx = (area.width.saturating_sub(mw)) / 2;
    let my = (area.height.saturating_sub(mh)) / 2;
    let menu_area = Rect::new(mx, my, mw, mh);

    frame.render_widget(Clear, menu_area);
    let block = Block::default()
        .style(Style::default().bg(theme::overlay_bg()))
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(" quantize ", theme::amber_bright().add_modifier(Modifier::BOLD)));
    frame.render_widget(block, menu_area);

    let inner = Rect::new(mx + 2, my + 1, mw - 4, mh - 2);
    let qm = &nav.quantize_modal;

    let row_style = |idx: usize| -> (Style, &'static str) {
        if qm.cursor == idx {
            (theme::amber_bright().add_modifier(Modifier::BOLD), "\u{25B6} ")
        } else {
            (theme::normal(), "  ")
        }
    };

    let (s0, ind0) = row_style(0);
    let (s1, ind1) = row_style(1);
    let (s2, ind2) = row_style(2);

    let lines = vec![
        Line::from(vec![
            Span::styled(ind0, s0),
            Span::styled("grid      ", theme::dim()),
            Span::styled(format!("\u{25C0} {} \u{25B6}", qm.grid.label()), s0),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(ind1, s1),
            Span::styled("strength  ", theme::dim()),
            Span::styled(format!("\u{25C0} {}% \u{25B6}", qm.strength), s1),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(ind2, s2),
            Span::styled(
                "[ apply ]",
                if qm.cursor == 2 { theme::amber_bright().add_modifier(Modifier::BOLD) } else { theme::normal() },
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  j/k", theme::dim()),
            Span::styled(" nav  ", theme::muted()),
            Span::styled("h/l", theme::dim()),
            Span::styled(" adjust  ", theme::muted()),
            Span::styled("enter", theme::dim()),
            Span::styled(" apply", theme::muted()),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines), inner);
}

// ── Separator + Helpers ──


