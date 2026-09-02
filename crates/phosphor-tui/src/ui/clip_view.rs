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

    // Right tabs (step grid / inst config / piano / settings). The grid is
    // only a tab on a track that has a sequencer on it, and it comes first
    // because on those tracks it is the tab being worked in.
    let has_sequencer = nav.current_track().is_some_and(|t| t.sequencer.is_some());
    let mut tabs: Vec<ClipTab> = Vec::new();
    if has_sequencer {
        tabs.push(ClipTab::Sequencer);
    }
    // An effect's panel is a tab only while a slot is open in it: a tab for a
    // panel with no effect behind it is a tab that shows nothing.
    if nav.clip_view.fx.slot.is_some() {
        tabs.push(ClipTab::Fx);
    }
    tabs.extend(ClipTab::ALL.iter().copied());

    for tab in &tabs {
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

    // Which band the step grid's cursor is in, and whether a knob is being
    // held — the same thing the piano roll says about its edit mode, and for
    // the same reason: a locked control that does not announce itself is a
    // keyboard that has stopped working.
    if nav.clip_view.clip_tab == ClipTab::Sequencer && has_sequencer {
        let view = &nav.clip_view.sequencer;
        if let Some(track) = nav.current_track() {
            spans.push(Span::styled(
                format!(" {} \u{00B7} seq", track.name.to_uppercase()),
                theme::normal(),
            ));
        }
        spans.push(Span::styled(
            format!(" [SEQ:{}{}]", view.band.label(), if view.locked { " hold" } else { "" }),
            Style::default().fg(theme::amber_val()).add_modifier(Modifier::BOLD),
        ));
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
        ClipTab::Sequencer => render_sequencer(frame, cols[2], nav, snap),
        ClipTab::Fx => fx::render_fx_panel(frame, cols[2], nav),
    }
}

pub(super) fn render_fx_panel(frame: &mut Frame, area: Rect, nav: &NavState) {
    let h = area.height as usize;
    let w = area.width as usize;
    if h == 0 || w == 0 { return; }

    let focused = nav.focused_pane == Pane::ClipView && nav.clip_view.focus == ClipViewFocus::FxPanel;

    let mut lines: Vec<Line> = Vec::new();

    // Synth tab: the same parameters the [inst] tab draws, in the width this
    // column has. How each one reads is `params`' answer, not this panel's —
    // two panels showing the same control differently is how one of them
    // ends up wrong.
    if nav.clip_view.fx_panel_tab == FxPanelTab::Synth {
        let track = nav.tracks.get(nav.track_cursor);
        let values = track.map(|t| &t.synth_params).cloned().unwrap_or_default();

        if values.is_empty() {
            lines.push(Line::from(Span::styled("  (no instrument)", theme::dim())));
        } else {
            let instrument = track.and_then(|t| t.instrument_type);
            let names = params::names(instrument);
            let count = values.len().min(names.len());

            let visible_rows = h.saturating_sub(2);
            let cursor = nav.clip_view.synth_param_cursor;
            let scroll_offset = if cursor >= visible_rows {
                cursor - visible_rows + 1
            } else {
                0
            };

            for (i, &val) in values[..count].iter().enumerate().skip(scroll_offset).take(visible_rows) {
                let is_cur = focused && cursor == i;
                let name = names.get(i).copied().unwrap_or("?");

                let indicator = if is_cur { "\u{25B6}" } else { " " };
                let name_s = if is_cur { theme::amber_bright().add_modifier(Modifier::BOLD) } else { theme::normal() };
                let dim_s = if is_cur { theme::amber() } else { theme::dim() };

                if let Some(label) = params::discrete_label(instrument, &values, i) {
                    lines.push(Line::from(vec![
                        Span::styled(format!(" {indicator} "), name_s),
                        Span::styled(format!("{name:<8}"), name_s),
                        Span::styled(format!(" {label}"), dim_s),
                    ]));
                } else {
                    let bar_w = (w.saturating_sub(14)).min(10);
                    lines.push(Line::from(vec![
                        Span::styled(format!(" {indicator} "), name_s),
                        Span::styled(format!("{name:<8}"), name_s),
                        Span::styled(params::bar(val, bar_w), if is_cur { theme::amber() } else { theme::muted() }),
                        Span::styled(format!(" {}", params::value_text(instrument, &values, i)), dim_s),
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
        // TrackFx tab: the chain. Its own module, because a slot list and
        // the panels behind the slots are one feature and belong together.
        lines.truncate(h);
        if !lines.is_empty() {
            frame.render_widget(Paragraph::new(lines), area);
        }
        fx::render_fx_chain(frame, area, nav, focused);
        return;
    }

    lines.truncate(h);
    frame.render_widget(Paragraph::new(lines), area);
}

/// The instrument's real panel, in the room the right pane affords.
///
/// # What this replaced
///
/// A mock-up. The tab drew `LFO rate / depth / wave / target`, `Filter type /
/// cutoff / reso` and so on — four sections of plausible-looking controls at
/// a hard-coded `0%`, wired to nothing, answering `j`/`k` and `h`/`l` with
/// silence. The real panel, the patch selector included, was in the narrow
/// column on the left; a player who pressed Tab to reach their instrument
/// found the fake one and typed into it.
///
/// It is the same panel as the left strip and the same cursor: what changes
/// is the width. Eighty-four controls do not fit in a column twenty-four
/// wide, and they do fit in three columns of a hundred, which is the whole
/// reason this tab is worth having.
pub(super) fn render_inst_config(frame: &mut Frame, area: Rect, nav: &NavState) {
    let (w, h) = (area.width as usize, area.height as usize);
    if w == 0 || h == 0 { return; }

    let focused = nav.focused_pane == Pane::ClipView
        && nav.clip_view.focus == ClipViewFocus::PianoRoll
        && nav.clip_view.clip_tab == ClipTab::InstConfig;

    let Some(track) = nav.tracks.get(nav.track_cursor) else {
        frame.render_widget(Paragraph::new(Span::styled("  select a track", theme::dim())), area);
        return;
    };
    let instrument = track.instrument_type;
    let values = &track.synth_params;
    let names = params::names(instrument);
    let count = values.len().min(names.len());
    if count == 0 {
        frame.render_widget(
            Paragraph::new(Span::styled("  this track has no instrument on it", theme::dim())),
            area,
        );
        return;
    }

    // One control per cell, filled down each column and then across, so the
    // panel reads in the order the instrument lists it.
    let cell_w = INST_CELL_W.min(w);
    let columns = (w / cell_w).max(1);
    let rows = h.saturating_sub(1).max(1);
    let per_page = columns * rows;
    let cursor = nav.clip_view.synth_param_cursor.min(count.saturating_sub(1));
    let pages = count.div_ceil(per_page);
    let page = cursor / per_page;
    let first = page * per_page;

    let mut lines: Vec<Line> = vec![inst_header(track, instrument, count, page, pages, focused)];

    for row in 0..rows {
        let mut spans: Vec<Span> = Vec::new();
        for column in 0..columns {
            let index = first + column * rows + row;
            if index >= count {
                continue;
            }
            spans.extend(inst_cell(
                instrument,
                values,
                index,
                names.get(index).copied().unwrap_or("?"),
                focused && index == cursor,
                cell_w,
            ));
        }
        if spans.is_empty() {
            break;
        }
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// How wide one control's cell is: the name, its value, and a space to keep
/// two columns of them apart.
///
/// Twenty-six is chosen so that the pane an eighty-column terminal leaves —
/// fifty-five — holds two of them rather than one, and a hundred-and-twenty
/// column one holds three. Every selector label in the project fits the
/// fourteen columns that leaves for a value; the panel tests pin that.
const INST_CELL_W: usize = 26;

/// The line over the panel: whose controls these are, and which page of them.
fn inst_header(
    track: &TrackState,
    instrument: Option<InstrumentType>,
    count: usize,
    page: usize,
    pages: usize,
    focused: bool,
) -> Line<'static> {
    let name = instrument.map_or("no instrument", InstrumentType::label);
    let mut spans = vec![
        Span::styled(
            format!(" {name} "),
            if focused {
                theme::amber_bright().add_modifier(Modifier::BOLD)
            } else {
                theme::normal().add_modifier(Modifier::BOLD)
            },
        ),
        Span::styled(
            format!("\u{00B7} {} controls ", count),
            theme::dim(),
        ),
    ];
    if pages > 1 {
        spans.push(Span::styled(format!("\u{00B7} page {}/{pages} ", page + 1), theme::muted()));
    }
    spans.push(Span::styled(
        format!("\u{00B7} {} ", track.name.to_lowercase()),
        theme::dim(),
    ));
    Line::from(spans)
}

/// One control: `▶ cutoff   ██████░░░░ 61%`, or a selector's word instead of
/// the bar.
fn inst_cell(
    instrument: Option<InstrumentType>,
    values: &[f32],
    index: usize,
    name: &str,
    selected: bool,
    cell_w: usize,
) -> Vec<Span<'static>> {
    let name_style = if selected {
        theme::amber_bright().add_modifier(Modifier::BOLD)
    } else {
        theme::normal()
    };
    let value_style = if selected { theme::amber() } else { theme::dim() };
    let indicator = if selected { "\u{25B6}" } else { " " };

    // `name` is 8 wide on every instrument — the panels all pin it — leaving
    // the rest of the cell for the value.
    const NAME_W: usize = 8;
    const GUTTER: usize = 3; // indicator and its spaces
    let value_w = cell_w.saturating_sub(NAME_W + GUTTER + 1);

    let mut spans = vec![
        Span::styled(format!("{indicator} "), name_style),
        Span::styled(format!("{name:<NAME_W$} "), name_style),
    ];

    if let Some(label) = params::discrete_label(instrument, values, index) {
        let text: String = label.chars().take(value_w).collect();
        spans.push(Span::styled(
            format!("{text:<value_w$}"),
            if selected {
                theme::amber_bright()
            } else {
                theme::muted()
            },
        ));
    } else {
        let reading = params::value_text(instrument, values, index);
        let bar_w = value_w.saturating_sub(reading.chars().count() + 1).min(12);
        let value = values.get(index).copied().unwrap_or(0.0);
        spans.push(Span::styled(
            params::bar(value, bar_w),
            if selected { theme::amber() } else { theme::muted() },
        ));
        let pad = value_w.saturating_sub(bar_w + reading.chars().count() + 1);
        spans.push(Span::styled(
            format!(" {reading}{:pad$}", "", pad = pad),
            value_style,
        ));
    }
    spans.push(Span::styled(" ", theme::bg()));
    spans
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
    let clip_len = clip.map(|c| c.length_ticks.max(1)).unwrap_or(1);
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

    // column_count is grid-scaled (e.g. 32 for 16 beats at 1/8 grid)
    let column_count = pr.column_count.max(1);

    // Visible columns limited by screen width (min 3 chars per column)
    let max_visible = (note_w / 3).max(1).min(column_count);
    let scroll_offset = pr.scroll_x.min(column_count.saturating_sub(max_visible));
    let visible_cols = max_visible.min(column_count - scroll_offset);
    // Use the full note_w for column width calculation to avoid a gap at the right
    // where notes render but no column grid exists. Integer col_w * visible_cols
    // must equal note_w, so we shrink note_w to the largest multiple.
    let col_w = if note_w > 0 && visible_cols > 0 { note_w / visible_cols } else { 1 };
    let note_w = col_w * visible_cols; // trim to exact column boundary

    let mut lines: Vec<Line> = Vec::new();

    // Column number header row (only when in column/row mode)
    if in_col_mode && h > 1 {
        let mut hdr_spans: Vec<Span> = Vec::new();
        // Show recording indicator — or, in edit mode, the cursor note's
        // velocity, which is otherwise a byte with no face.
        if snap.recording {
            hdr_spans.push(Span::styled(" \u{25CF}REC", Style::default().fg(theme::rec_active_val()).add_modifier(Modifier::BOLD)));
            hdr_spans.push(Span::styled(" ", theme::bg()));
        } else if pr.edit_mode {
            let vel = notes.get(pr.edit_cursor).map(|n| n.velocity);
            let label = match vel {
                Some(v) => format!("v{v:>4}"),
                None => "     ".to_string(),
            };
            hdr_spans.push(Span::styled(label, theme::amber()));
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

    // The automation lane, when open, takes a strip off the bottom: one
    // label row and a few bar rows. It shares the column grid above it to
    // the cell, so a point drawn in a column sits under the notes in it.
    let lane_open = pr.automation_open && clip.is_some();
    let lane_h = if lane_open { LANE_ROWS.min(h.saturating_sub(2)) } else { 0 };

    let rows_before_lane = h.saturating_sub(lane_h);
    let rows_for_notes = if in_col_mode && rows_before_lane > 1 {
        rows_before_lane - 1
    } else {
        rows_before_lane
    };

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
        let total_subs = (total_beats as f64 * subs_per_beat).round() as usize;
        let scroll_beat_frac = if column_count > 0 { scroll_offset as f64 / column_count as f64 } else { 0.0 };
        let visible_beat_frac = if column_count > 0 { visible_cols as f64 / column_count as f64 } else { 1.0 };
        for s in 1..total_subs {
            let abs_frac = s as f64 / total_subs as f64;
            let vis_frac = (abs_frac - scroll_beat_frac) / visible_beat_frac;
            if vis_frac <= 0.0 || vis_frac >= 1.0 { continue; }
            // Draw grid line 1 cell before the column boundary so the thin
            // stroke visually aligns with the left edge of notes at this position
            let x = ((vis_frac * note_w as f64) as usize).saturating_sub(1);
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
        let scroll_frac = if column_count > 0 { scroll_offset as f64 / column_count as f64 } else { 0.0 };
        let visible_frac = if column_count > 0 { visible_cols as f64 / column_count as f64 } else { 1.0 };
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
                } else if n.muted {
                    // A muted note is still on the page but out of the mix:
                    // darker than the softest sounding note can ever be, so
                    // the eye separates "quiet" from "silenced".
                    base_note_style.fg(theme::dim_color(tc, 25)).add_modifier(Modifier::CROSSED_OUT)
                } else {
                    // Velocity is the note's brightness: a ghost note reads
                    // faint, an accent reads hot, and a clip's dynamics are
                    // visible at a glance instead of hiding in a byte. The
                    // floor keeps the quietest note findable on screen.
                    let brightness = 45 + (n.velocity as u16 * 55) / 127;
                    base_note_style.fg(theme::dim_color(tc, brightness))
                };
                // Map note position from clip-space to visible-window-space
                let rel_start = (n.start_frac(clip_len) - scroll_frac) / visible_frac;
                let rel_end =
                    ((n.end_tick() as f64 / clip_len as f64) - scroll_frac) / visible_frac;
                if rel_end <= 0.0 || rel_start >= 1.0 { continue; } // off-screen
                let sx = (rel_start.max(0.0) * note_w as f64) as usize;
                let ex = (rel_end * note_w as f64) as usize;
                let ex = ex.max(sx + 1).min(note_w);
                let note_len = ex - sx;
                for (j, cell) in gr.iter_mut().take(ex).skip(sx).enumerate() {
                    if j == 0 || (note_len > 2 && j == note_len - 1) {
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

    if lane_open {
        append_automation_lane(
            &mut lines, clip.unwrap(), pr, key_w, note_w, col_w,
            visible_cols, scroll_offset, lane_h, focused, snap,
        );
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// How many rows the automation lane's bars get, below its one label row.
const LANE_ROWS: usize = 6;

/// Draw the automation lane under the note grid: a label row naming the
/// stream, then bar rows whose height is the controller value in each
/// column. Column geometry is passed in from the note grid so the two are
/// aligned to the cell.
#[allow(clippy::too_many_arguments)]
fn append_automation_lane(
    lines: &mut Vec<Line<'static>>,
    clip: &crate::state::Clip,
    pr: &crate::state::PianoRollState,
    key_w: usize,
    note_w: usize,
    col_w: usize,
    visible_cols: usize,
    scroll_offset: usize,
    lane_h: usize,
    focused: bool,
    snap: &TransportSnapshot,
) {
    let streams = clip.control_streams();
    if streams.is_empty() || lane_h == 0 {
        return;
    }
    let stream = streams[pr.automation_lane.min(streams.len() - 1)];
    let active = focused && pr.automation_focus;
    let col_count = pr.column_count.max(1);
    let bar_rows = lane_h.saturating_sub(1).max(1);

    // Label row: the stream name, and a hint at which of several it is.
    let name_style = if active {
        theme::amber_bright().add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    };
    let lane_hint = if streams.len() > 1 {
        format!("auto {} ({}/{})", stream.label(), pr.automation_lane.min(streams.len() - 1) + 1, streams.len())
    } else {
        format!("auto {}", stream.label())
    };
    lines.push(Line::from(vec![
        Span::styled(format!("{lane_hint:>kw$} ", kw = key_w.saturating_sub(1)), name_style),
        Span::styled("\u{2502}", theme::border_style()),
        Span::styled(
            if active { " jk draw \u{00b7} [ ] lane \u{00b7} d clear".to_string() } else { String::new() },
            theme::dim(),
        ),
    ]));

    // Per-column value, held from the last point — the same rule playback
    // and editing use, so the bars show what the instrument will hear.
    let values: Vec<Option<u8>> = (0..visible_cols)
        .map(|c| clip.control_value_at_column(stream, c + scroll_offset, col_count))
        .collect();

    let cursor_vis = pr.column.checked_sub(scroll_offset).filter(|&c| c < visible_cols);
    // Playhead column, for the same vertical line the note grid draws.
    let play_col = if snap.playing && clip.length_ticks > 0 {
        let pos = snap.position_ticks - clip.start_tick;
        if pos >= 0 && pos < clip.length_ticks {
            let c = ((pos * col_count as i64) / clip.length_ticks) as usize;
            c.checked_sub(scroll_offset).filter(|&c| c < visible_cols)
        } else {
            None
        }
    } else {
        None
    };

    for r in 0..bar_rows {
        // Top row is the highest value band; bottom row the lowest.
        let band_hi = 127 - (r * 128 / bar_rows) as u8;
        let band_lo = 127u8.saturating_sub(((r + 1) * 128 / bar_rows) as u8);
        let mut cells: Vec<(char, Style)> = Vec::with_capacity(note_w);
        for c in 0..visible_cols {
            let is_cursor_col = cursor_vis == Some(c);
            let is_play_col = play_col == Some(c);
            let filled = values[c].is_some_and(|v| v >= band_lo);
            let at_top = values[c].is_some_and(|v| v >= band_lo && v <= band_hi);
            let (ch, fg) = if at_top {
                ('\u{2584}', theme::amber_bright_val()) // the value's own band: a cap
            } else if filled {
                ('\u{2588}', theme::amber_val())
            } else {
                (' ', theme::dim_val())
            };
            let bg = if is_play_col {
                theme::playhead_bg()
            } else if is_cursor_col && active {
                theme::col_highlight_bg()
            } else {
                theme::bg_val()
            };
            for _ in 0..col_w {
                cells.push((ch, Style::default().fg(fg).bg(bg)));
            }
        }
        // Pad to the note width so the lane's background reaches the edge.
        while cells.len() < note_w {
            cells.push((' ', Style::default().bg(theme::bg_val())));
        }

        let mut spans: Vec<Span> = Vec::with_capacity(note_w + 2);
        spans.push(Span::styled(" ".repeat(key_w), theme::bg()));
        spans.push(Span::styled("\u{2502}", theme::border_style()));
        // Merge equal cells into runs.
        let mut text = String::new();
        let mut cur = Style::default().bg(theme::bg_val());
        for (ch, s) in cells {
            if s == cur {
                text.push(ch);
            } else {
                if !text.is_empty() { spans.push(Span::styled(std::mem::take(&mut text), cur)); }
                cur = s;
                text.push(ch);
            }
        }
        if !text.is_empty() { spans.push(Span::styled(text, cur)); }
        lines.push(Line::from(spans));
    }
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
        ("Rec quant", pr.record_quantize.map_or("off".to_string(), |g| g.label().to_string())),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The DX7's voice names come from the ROM and run to ten characters —
    /// longer than any other instrument's patch names, and long enough to fall
    /// off the end of the panel if the layout is ever tightened.
    #[test]
    fn every_dx7_voice_name_fits_the_fx_panel() {
        // A selector row is " \u{25B6} " plus the parameter name padded to 8,
        // plus a space; whatever is left is the label's.
        const LABEL_COLUMN: usize = 12;
        let room = FX_PANEL_W as usize - LABEL_COLUMN;

        for voice in 0..phosphor_dsp::dx7::VOICE_COUNT {
            let name = phosphor_dsp::dx7::voice_name(voice);
            assert!(name.chars().count() <= room,
                "voice {voice} {name:?} needs {} of the {room} columns the panel leaves",
                name.chars().count());
        }
        for bank in phosphor_dsp::dx7::BANK_NAMES {
            assert!(bank.chars().count() <= room, "bank {bank:?} does not fit");
        }
        // The parameter names have to fit their own column too, or the label
        // is pushed right and the assertion above stops meaning anything.
        for name in phosphor_dsp::dx7::PARAM_NAMES {
            assert!(name.chars().count() <= 8, "parameter name {name:?} overflows its column");
        }
    }

    /// The Juno's factory names run to twenty characters — MYSTERIOUS
    /// INVENTION — and a player refers to a patch by its number, so the panel
    /// label is the number and as much of the name as fits. This is the test
    /// that keeps the abbreviating honest: every label carries its number and
    /// none of them runs off the end.
    #[test]
    fn every_juno_patch_label_fits_the_fx_panel() {
        const LABEL_COLUMN: usize = 12;
        let room = FX_PANEL_W as usize - LABEL_COLUMN;

        for index in 0..phosphor_dsp::juno::PATCH_COUNT {
            let label = phosphor_dsp::juno::PATCH_LABELS[index];
            assert!(label.chars().count() <= room,
                "patch {index} {label:?} needs {} of the {room} columns the panel leaves",
                label.chars().count());
            let number = phosphor_dsp::juno::PATCH_NUMBERS[index];
            assert!(label.starts_with(number),
                "patch {index} {label:?} does not lead with its number {number:?}");
        }
        // Every other switch on the panel shares that column.
        for index in 0..phosphor_dsp::juno::PARAM_COUNT {
            if let Some(label) = phosphor_dsp::juno::discrete_label(index, 0.5) {
                assert!(label.chars().count() <= room, "switch label {label:?} does not fit");
            }
            let name = phosphor_dsp::juno::PARAM_NAMES[index];
            assert!(name.chars().count() <= 8, "parameter name {name:?} overflows its column");
        }
    }

    /// The Rhodes has no factory patch numbers to lead with — the instrument
    /// has no patch memory at all — so its bank is named after the pianos it
    /// is, and every one of those names has to fit the column on its own.
    #[test]
    fn every_rhodes_patch_name_fits_the_fx_panel() {
        const LABEL_COLUMN: usize = 12;
        let room = FX_PANEL_W as usize - LABEL_COLUMN;

        for index in 0..phosphor_dsp::rhodes::PATCH_COUNT {
            let name = phosphor_dsp::rhodes::PATCH_NAMES[index];
            assert!(name.chars().count() <= room,
                "patch {index} {name:?} needs {} of the {room} columns the panel leaves",
                name.chars().count());
        }
        for index in 0..phosphor_dsp::rhodes::PARAM_COUNT {
            for position in [0.0, 0.3, 0.6, 1.0] {
                if let Some(label) = phosphor_dsp::rhodes::discrete_label(index, position) {
                    assert!(label.chars().count() <= room,
                        "switch label {label:?} does not fit");
                }
            }
            let name = phosphor_dsp::rhodes::PARAM_NAMES[index];
            assert!(name.chars().count() <= 8, "parameter name {name:?} overflows its column");
        }
    }

    /// The Little Phatty's bank is a hundred patches of our own, named rather
    /// than numbered for the same reason the Rhodes' is: the numbers on the
    /// hardware belong to Moog's factory set, which is not what this is.
    #[test]
    fn every_little_phatty_patch_name_fits_the_fx_panel() {
        const LABEL_COLUMN: usize = 12;
        let room = FX_PANEL_W as usize - LABEL_COLUMN;

        for index in 0..phosphor_dsp::phatty::PATCH_COUNT {
            let name = phosphor_dsp::phatty::PATCH_NAMES[index];
            assert!(name.chars().count() <= room,
                "patch {index} {name:?} needs {} of the {room} columns the panel leaves",
                name.chars().count());
        }
        // Forty-one controls, eighteen of them selectors, and every one of
        // those prints a word in the same column.
        for index in 0..phosphor_dsp::phatty::PARAM_COUNT {
            for position in [0.0, 0.2, 0.4, 0.6, 0.8, 1.0] {
                if let Some(label) = phosphor_dsp::phatty::discrete_label(index, position) {
                    assert!(label.chars().count() <= room,
                        "switch label {label:?} does not fit");
                }
            }
            let name = phosphor_dsp::phatty::PARAM_NAMES[index];
            assert!(name.chars().count() <= 8, "parameter name {name:?} overflows its column");
        }
    }

    /// The phosphor synth carries 229 patches across four sets, which is more
    /// than any other instrument here and the reason its labels lead with a
    /// slot code: at that size a name on its own does not tell a player where
    /// in the bank they are standing. The microKORG set's names are Korg's
    /// own and run to eighteen characters — Techstep Ring Bass — so the label
    /// is `A42 TechRing` and the full name survives in `PATCH_NAMES`.
    ///
    /// Its panel is also the widest in the project: 67 controls, 25 of them
    /// selectors, and every one of those prints a word in the same column.
    #[test]
    fn every_phosphor_panel_label_fits_the_fx_panel() {
        const LABEL_COLUMN: usize = 12;
        let room = FX_PANEL_W as usize - LABEL_COLUMN;

        for index in 0..phosphor_dsp::synth::PATCH_COUNT {
            let label = phosphor_dsp::synth::PATCH_LABELS[index];
            assert!(label.chars().count() <= room,
                "patch {index} {label:?} needs {} of the {room} columns the panel leaves",
                label.chars().count());
            let slot = phosphor_dsp::synth::PATCH_SLOTS[index];
            let short: String = slot.chars().filter(|c| *c != '.').collect();
            assert!(label.starts_with(&short),
                "patch {index} {label:?} does not lead with its slot {slot:?}");
            assert!(label.chars().count() > short.chars().count() + 1,
                "patch {index} {label:?} is a slot with no name after it");
            assert!(!phosphor_dsp::synth::PATCH_NAMES[index].is_empty(),
                "patch {index} has no full name");
        }
        for name in phosphor_dsp::synth::BANK_NAMES {
            assert!(name.chars().count() <= room, "set name {name:?} does not fit");
        }
        for index in 0..phosphor_dsp::synth::PARAM_COUNT {
            for position in [0.0, 0.3, 0.6, 1.0] {
                if let Some(label) = phosphor_dsp::synth::discrete_label(index, position) {
                    assert!(label.chars().count() <= room,
                        "switch label {label:?} does not fit");
                }
            }
            let name = phosphor_dsp::synth::PARAM_NAMES[index];
            assert!(name.chars().count() <= 8, "parameter name {name:?} overflows its column");
        }
    }

    /// The Jupiter's panel grew from sixteen controls to thirty-two, seven of
    /// them switches that print a word rather than a bar. Every one of those
    /// words, and every parameter name, has to fit the column it is given.
    ///
    /// Its factory names run to twenty characters — MUSIC OF THE SPHERES —
    /// and a player refers to a patch by its number, so the panel label is the
    /// number and as much of the name as fits, exactly as on the Juno. This is
    /// what keeps the abbreviating honest: every label carries its number,
    /// none of them runs off the end, and the full name survives in
    /// `PATCH_NAMES` for anything with room to print it.
    #[test]
    fn every_jupiter_panel_label_fits_the_fx_panel() {
        const LABEL_COLUMN: usize = 12;
        let room = FX_PANEL_W as usize - LABEL_COLUMN;

        for index in 0..phosphor_dsp::jupiter::PATCH_COUNT {
            let label = phosphor_dsp::jupiter::PATCH_LABELS[index];
            assert!(label.chars().count() <= room,
                "patch {index} {label:?} needs {} of the {room} columns the panel leaves",
                label.chars().count());
            let number = phosphor_dsp::jupiter::PATCH_NUMBERS[index];
            assert!(label.starts_with(number),
                "patch {index} {label:?} does not lead with its number {number:?}");
            // A label that is only its number would fit and say nothing, and
            // the full name has to survive somewhere for a caller with room
            // to print it. Twenty characters is the longest Roland gave any
            // of them: MUSIC OF THE SPHERES, which the label shortens to
            // SPHERES rather than to a run of initials.
            assert!(label.len() > number.len() + 1,
                "patch {index} {label:?} is a number with no name after it");
            let name = phosphor_dsp::jupiter::PATCH_NAMES[index];
            assert!(!name.is_empty() && name.chars().count() <= 20,
                "patch {index} name {name:?} is not a factory name");
        }
        for index in 0..phosphor_dsp::jupiter::PARAM_COUNT {
            for position in [0.0, 0.3, 0.6, 1.0] {
                if let Some(label) = phosphor_dsp::jupiter::discrete_label(index, position) {
                    assert!(label.chars().count() <= room,
                        "switch label {label:?} does not fit");
                }
            }
            let name = phosphor_dsp::jupiter::PARAM_NAMES[index];
            assert!(name.chars().count() <= 8, "parameter name {name:?} overflows its column");
        }
    }

    /// The TEO-5 is the widest panel in the rack — 147 controls, 88 of them
    /// selectors, 48 of which are the modulation matrix — and its sixty-five
    /// destination names and 256 program names all have to fit the same
    /// twelve columns.
    #[test]
    fn every_teo_five_panel_label_fits_the_fx_panel() {
        use phosphor_dsp::teo5;
        const LABEL_COLUMN: usize = 12;
        let room = FX_PANEL_W as usize - LABEL_COLUMN;

        for index in 0..teo5::PROGRAM_COUNT {
            let label = teo5::program_label(index);
            assert!(
                label.chars().count() <= room,
                "program {index} {label:?} needs {} of the {room} columns the panel leaves",
                label.chars().count()
            );
            let name = teo5::program_name(index);
            assert!(!name.is_empty() && name.chars().count() <= 20,
                "program {index} name {name:?} is not a factory name");
            assert!(name.starts_with(label),
                "program {index} label {label:?} is not the front of its name {name:?}");
        }

        let mut params = vec![0.0f32; teo5::PARAM_COUNT];
        for index in 0..teo5::PARAM_COUNT {
            for position in [0.0f32, 0.3, 0.6, 1.0] {
                params[index] = position;
                if let Some(label) = teo5::discrete_label(&params, index) {
                    assert!(label.chars().count() <= room,
                        "switch label {label:?} does not fit");
                }
            }
            params[index] = 0.0;
            let name = teo5::PARAM_NAMES[index];
            assert!(name.chars().count() <= 8, "parameter name {name:?} overflows its column");
        }
    }

    /// The Prophet-6 is the widest panel in the rack — 84 controls, 44 of
    /// them selectors — and the only one whose preset names come out of a ROM
    /// rather than being written by hand, so nothing stops one of Sequential's
    /// twenty-character names from running off the end except this.
    #[test]
    fn every_prophet_six_panel_label_fits_the_fx_panel() {
        use phosphor_dsp::prophet6;
        const LABEL_COLUMN: usize = 12;
        let room = FX_PANEL_W as usize - LABEL_COLUMN;

        for index in 0..prophet6::PROGRAM_COUNT {
            let label = prophet6::program_label(index);
            assert!(
                label.chars().count() <= room,
                "program {index} {label:?} needs {} of the {room} columns the panel leaves",
                label.chars().count()
            );
            let name = prophet6::program_name(index);
            assert!(!name.is_empty() && name.chars().count() <= 20,
                "program {index} name {name:?} is not a factory name");
            assert!(name.starts_with(label),
                "program {index} label {label:?} is not the front of its name {name:?}");
        }

        let mut params = vec![0.0f32; prophet6::PARAM_COUNT];
        for index in 0..prophet6::PARAM_COUNT {
            for position in [0.0f32, 0.3, 0.6, 1.0] {
                params[index] = position;
                if let Some(label) = prophet6::discrete_label(&params, index) {
                    assert!(label.chars().count() <= room,
                        "switch label {label:?} does not fit");
                }
            }
            params[index] = 0.0;
            let name = prophet6::PARAM_NAMES[index];
            assert!(name.chars().count() <= 8, "parameter name {name:?} overflows its column");
        }
    }
}
