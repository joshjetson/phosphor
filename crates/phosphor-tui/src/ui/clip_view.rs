//! UI rendering: clip view.

use super::*;

pub(super) fn render_clip_view_tabs(frame: &mut Frame, area: Rect, nav: &NavState) {
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
        let total_clips = t.clips.len();
        if let Some(c) = nav.active_clip() {
            let clip_num = nav.clip_view_target.map(|(_, ci)| ci + 1).unwrap_or(c.number);
            spans.push(Span::styled(
                format!(" {} \u{00B7} clip {}/{}", t.name.to_uppercase(), clip_num, total_clips),
                theme::normal()));
            if nav.clip_view.piano_roll.edit_mode {
                let sub = match nav.clip_view.piano_roll.edit_sub {
                    crate::state::EditSubMode::Navigate => "nav",
                    crate::state::EditSubMode::Selecting => "sel",
                    crate::state::EditSubMode::Moving => "mov",
                };
                spans.push(Span::styled(
                    format!(" [EDIT:{}]", sub),
                    Style::default().fg(theme::amber_val()).add_modifier(Modifier::BOLD)));
            }
        }
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub(super) fn render_clip_view(frame: &mut Frame, area: Rect, nav: &NavState, snap: &TransportSnapshot) {
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
        ClipTab::Settings => render_settings(frame, cols[2], nav),
        ClipTab::PianoRoll => render_piano_roll(frame, cols[2], nav, snap),
    }
}

pub(super) fn render_fx_panel(frame: &mut Frame, area: Rect, nav: &NavState) {
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

pub(super) fn render_inst_config(frame: &mut Frame, area: Rect, nav: &NavState) {
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

pub(super) fn render_piano_roll(frame: &mut Frame, area: Rect, nav: &NavState, snap: &TransportSnapshot) {
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

    // Column geometry — based on actual clip length
    // Each column = 1 beat (quarter note = PPQ ticks)
    let ppq = phosphor_core::transport::Transport::PPQ;
    let total_beats = if let Some(c) = clip {
        ((c.length_ticks as f64) / ppq as f64).ceil() as usize
    } else {
        16
    }.max(1);

    // Visible columns limited by screen width (min 3 chars per column)
    let max_visible = (note_w / 3).max(1).min(total_beats);
    let scroll_offset = pr.scroll_x.min(total_beats.saturating_sub(max_visible));
    let visible_cols = max_visible.min(total_beats - scroll_offset);
    // Use the full note_w for column width calculation to avoid a gap at the right
    // where notes render but no column grid exists. Integer col_w * visible_cols
    // must equal note_w, so we shrink note_w to the largest multiple.
    let col_w = if note_w > 0 && visible_cols > 0 { note_w / visible_cols } else { 1 };
    let note_w = col_w * visible_cols; // trim to exact column boundary

    let mut lines: Vec<Line> = Vec::new();

    // Column number header row (only when in column/row mode)
    if in_col_mode && h > 1 {
        let mut hdr_spans: Vec<Span> = Vec::new();
        // Show recording indicator
        if snap.recording {
            hdr_spans.push(Span::styled(" \u{25CF}REC", Style::default().fg(theme::REC_ACTIVE).add_modifier(Modifier::BOLD)));
            hdr_spans.push(Span::styled(" ", theme::bg()));
        } else {
            hdr_spans.push(Span::styled("      ", theme::bg()));
        }
        hdr_spans.push(Span::styled("\u{2502}", theme::border_style()));
        for c in 0..visible_cols {
            let abs_col = c + scroll_offset; // absolute column index
            let col_num = abs_col + 1; // 1-based display
            let is_sel = abs_col == pr.column;
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

        let row_highlighted = pr.is_row_highlighted(note);

        let key_bg = if row_highlighted && is_cur {
            theme::selection_cursor_bg()
        } else if row_highlighted {
            theme::selection_bg()
        } else if is_cur {
            theme::piano_cursor_bg()
        } else if black {
            theme::piano_black_bg()
        } else {
            theme::piano_white_bg()
        };
        let key_fg = if is_cur { theme::amber_bright_val() } else if note % 12 == 0 { theme::normal_val() } else { theme::dim_val() };

        let mut spans: Vec<Span> = Vec::new();
        spans.push(Span::styled(format!("{:>5} ", midi_note_name(note)), Style::default().fg(key_fg).bg(key_bg)));
        spans.push(Span::styled("\u{2502}",
            if note % 12 == 0 { Style::default().fg(theme::grid_major()).bg(theme::bg_val()) }
            else { theme::border_style() }));
        let row_bg = if row_highlighted && is_cur {
            theme::selection_cursor_bg()
        } else if row_highlighted {
            theme::selection_bg()
        } else if is_cur {
            theme::piano_cursor_bg()
        } else if black {
            theme::piano_black_bg()
        } else {
            theme::piano_white_bg()
        };
        let mut gr = vec![(' ', Style::default().fg(theme::dim_color_val()).bg(row_bg)); note_w];

        // Gridlines at grid resolution subdivisions
        let subs_per_beat = pr.grid.subdivisions_per_beat();
        let total_subs = (total_beats as f64 * subs_per_beat) as usize;
        let scroll_beat_frac = if total_beats > 0 { scroll_offset as f64 / total_beats as f64 } else { 0.0 };
        let visible_beat_frac = if total_beats > 0 { visible_cols as f64 / total_beats as f64 } else { 1.0 };
        for s in 1..total_subs {
            let abs_frac = s as f64 / total_subs as f64;
            let vis_frac = (abs_frac - scroll_beat_frac) / visible_beat_frac;
            if vis_frac <= 0.0 || vis_frac >= 1.0 { continue; }
            let x = (vis_frac * note_w as f64) as usize;
            if x >= note_w { continue; }
            let beat_idx = (s as f64 / subs_per_beat) as usize;
            let is_beat = (s as f64 % subs_per_beat).abs() < 0.01;
            let is_bar = is_beat && beat_idx % 4 == 0;
            let (ch, fg) = if is_bar {
                ('\u{2502}', theme::grid_major())
            } else if is_beat {
                ('\u{2506}', theme::grid_minor())
            } else {
                ('\u{00B7}', theme::dim_color(theme::grid_minor(), 40))
            };
            gr[x] = (ch, Style::default().fg(fg).bg(row_bg));
        }

        // Highlight range (Shift+h/l selection) — adjusted for scroll
        if let Some((hl_start, hl_end)) = pr.highlight_range() {
            let vis_start = hl_start.saturating_sub(scroll_offset);
            let vis_end = (hl_end + 1).saturating_sub(scroll_offset);
            let hl_x_start = vis_start * col_w;
            let hl_x_end = (vis_end * col_w).min(note_w);
            let hl_bg = theme::selection_bg();
            for x in hl_x_start..hl_x_end {
                let (ch, old_s) = gr[x];
                let fg = old_s.fg.unwrap_or(theme::dim_val());
                gr[x] = (ch, Style::default().fg(fg).bg(hl_bg));
            }
        }

        // Column highlight (current column cursor) — adjusted for scroll
        if in_col_mode && pr.column >= scroll_offset && pr.column < scroll_offset + visible_cols {
            let vis_col = pr.column - scroll_offset;
            let col_start = vis_col * col_w;
            let col_end = (col_start + col_w).min(note_w);
            let col_bg = if in_row_mode && is_cur {
                theme::col_row_bg()
            } else if pr.is_highlighted(pr.column) {
                theme::selection_cursor_bg()
            } else {
                theme::col_highlight_bg()
            };
            for x in col_start..col_end {
                let (ch, old_s) = gr[x];
                let fg = old_s.fg.unwrap_or(theme::dim_val());
                gr[x] = (ch, Style::default().fg(fg).bg(col_bg));
            }
        }

        // Draw MIDI notes from the active clip — adjusted for scroll window
        let base_note_style = Style::default().fg(tc).bg(
            if is_cur { theme::piano_cursor_bg() } else { row_bg }
        ).add_modifier(Modifier::BOLD);
        // Scroll window as fraction of clip
        let scroll_frac = if total_beats > 0 { scroll_offset as f64 / total_beats as f64 } else { 0.0 };
        let visible_frac = if total_beats > 0 { visible_cols as f64 / total_beats as f64 } else { 1.0 };
        let in_edit = pr.edit_mode;
        for (ni, n) in notes.iter().enumerate() {
            if n.note == note {
                // Determine style based on edit mode state
                let note_style = if in_edit && ni == pr.edit_cursor {
                    // Edit cursor — bright highlight
                    Style::default().fg(Color::Rgb(255, 255, 255)).bg(theme::amber_val()).add_modifier(Modifier::BOLD)
                } else if in_edit && pr.edit_selected.contains(&ni) {
                    // Selected note — tinted highlight
                    Style::default().fg(Color::Rgb(255, 255, 200)).bg(Color::Rgb(80, 60, 20)).add_modifier(Modifier::BOLD)
                } else {
                    base_note_style
                };
                // Map note position from clip-space to visible-window-space
                let rel_start = (n.start_frac - scroll_frac) / visible_frac;
                let rel_end = (n.start_frac + n.duration_frac - scroll_frac) / visible_frac;
                if rel_end <= 0.0 || rel_start >= 1.0 { continue; } // off-screen
                let sx = (rel_start.max(0.0) * note_w as f64) as usize;
                let ex = (rel_end * note_w as f64) as usize;
                let ex = ex.max(sx + 1).min(note_w);
                let note_len = ex - sx;
                for (j, cell) in gr.iter_mut().take(ex).skip(sx).enumerate() {
                    if j == 0 || (note_len > 2 && j == note_len - 1) {
                        // Border: use row background as foreground so the edge
                        // visually separates adjacent notes in any theme
                        *cell = ('\u{2502}', Style::default().fg(row_bg).bg(note_style.fg.unwrap_or(tc)));
                    } else {
                        *cell = ('\u{2588}', note_style);
                    }
                }
            }
        }

        // Playhead — vertical line showing current transport position
        if snap.playing {
            if let Some(clip) = clip {
                if clip.length_ticks > 0 {
                    let pos = snap.position_ticks;
                    let clip_start = clip.start_tick;
                    let clip_end = clip_start + clip.length_ticks;
                    if pos >= clip_start && pos < clip_end {
                        let frac = (pos - clip_start) as f64 / clip.length_ticks as f64;
                        // Map to visible window
                        let rel = (frac - scroll_frac) / visible_frac;
                        if rel >= 0.0 && rel < 1.0 {
                            let x = (rel * note_w as f64) as usize;
                            if x < note_w {
                                let (ch, _) = gr[x];
                                gr[x] = (ch, Style::default().fg(theme::playhead_fg()).bg(theme::playhead_bg()));
                            }
                        }
                    }
                }
            }
        }

        // Merge grid cells into spans
        let mut text = String::new();
        let mut cur_s = Style::default().fg(theme::dim_val()).bg(row_bg);
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

// ── Settings Panel ──

fn render_settings(frame: &mut Frame, area: Rect, nav: &NavState) {
    let focused = nav.focused_pane == Pane::ClipView && nav.clip_view.focus == ClipViewFocus::PianoRoll;
    let pr = &nav.clip_view.piano_roll;
    let cursor = pr.settings_cursor;

    let items: Vec<(&str, String)> = vec![
        ("Grid", pr.grid.label().to_string()),
        ("Snap", if pr.snap_enabled { "on".into() } else { "off".into() }),
        ("Velocity", format!("{}", pr.default_velocity)),
    ];

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        "  Piano Roll Settings",
        if focused { theme::amber_bright().add_modifier(Modifier::BOLD) } else { theme::dim() },
    )));
    lines.push(Line::from(""));

    for (i, (label, value)) in items.iter().enumerate() {
        let is_cur = focused && i == cursor;
        let label_style = if is_cur { theme::amber_bright() } else { theme::normal() };
        let value_style = if is_cur {
            Style::default().fg(Color::Rgb(255, 255, 255)).bg(theme::amber_val())
        } else {
            theme::muted()
        };
        let arrow = if is_cur { "\u{25B8} " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(arrow, label_style),
            Span::styled(format!("{:<10}", label), label_style),
            Span::styled(format!(" {}", value), value_style),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  h/l to adjust, j/k to navigate", theme::dim())));
    if focused {
        lines.push(Line::from(Span::styled(
            format!("  Edit mode: Space+E ({})", if pr.edit_mode { "active" } else { "off" }),
            theme::muted(),
        )));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

// ── FX Menu Overlay ──

// ── Space Menu Overlay ──

