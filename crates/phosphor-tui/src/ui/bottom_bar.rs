//! UI rendering: bottom bar.

use super::*;

pub(super) fn render_bottom_bar(frame: &mut Frame, area: Rect, nav: &NavState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(16), Constraint::Min(20), Constraint::Length(42)])
        .split(area);

    let (mt, ms) = if nav.loop_editor.active {
        ("-- LOOP --", Style::default().fg(Color::Rgb(80, 180, 80)).bg(theme::bg_val()))
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
                vec![("hl","col"),("H/L","highlight"),("d","del hl"),("1-9","jump"),("enter","sel")],
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

pub(super) fn grid_to_lines(grid: Vec<Vec<(char, Style)>>) -> Vec<Line<'static>> {
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

pub(super) fn midi_note_name(n: u8) -> String {
    const N: [&str;12] = ["C","C#","D","D#","E","F","F#","G","G#","A","A#","B"];
    format!("{}{}", N[n as usize%12], (n as i8/12)-1)
}

pub(super) fn is_black_key(n: u8) -> bool { matches!(n%12, 1|3|6|8|10) }
