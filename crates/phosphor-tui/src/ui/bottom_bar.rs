//! UI rendering: bottom bar.

use super::*;

pub(super) fn render_bottom_bar(
    frame: &mut Frame,
    area: Rect,
    nav: &NavState,
    status: Option<&str>,
) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(16), Constraint::Min(20), Constraint::Length(42)])
        .split(area);

    let in_grid = nav.focused_pane == Pane::ClipView
        && nav.clip_view.clip_tab == ClipTab::Sequencer
        && nav.clip_view.focus == ClipViewFocus::PianoRoll;
    let in_pads = nav.focused_pane == Pane::ClipView
        && nav.clip_view.clip_tab == ClipTab::Pads
        && nav.clip_view.focus == ClipViewFocus::PianoRoll;
    let in_source = nav
        .sampler_source
        .as_deref()
        .is_some_and(|mode| mode.track_idx == nav.track_cursor);
    let in_keys_mode = nav
        .current_track()
        .and_then(|t| t.sampler.as_deref())
        .is_some_and(|s| s.mode == phosphor_app::sampler::MapMode::Keys);
    // **Key listen takes the mode tag.** It is the one switch in the box that
    // changes what comes out of the speakers rather than what the mix does
    // with it, and a player who has forgotten it is on will spend the next
    // five minutes wondering why a track sounds like the kick. So it blinks,
    // it says which track, and it says how to put it back.
    let (mt, ms) = if nav.key_listen.is_some() {
        (
            "-- KEY LISTEN --",
            if super::meters::blink_on() {
                Style::default()
                    .fg(theme::rec_active_val())
                    .bg(theme::bg_val())
                    .add_modifier(Modifier::BOLD)
            } else {
                theme::dim()
            },
        )
    } else if nav.loop_editor.active {
        ("-- LOOP --", Style::default().fg(Color::Rgb(80, 180, 80)).bg(theme::bg_val()))
    } else if nav.focused_pane == Pane::Transport && nav.transport_ui.editing {
        ("-- EDIT --", theme::amber_bright())
    } else if nav.focused_pane == Pane::Transport {
        ("-- TRANSPORT --", theme::amber_bright())
    } else if in_source && nav.focused_pane == Pane::ClipView {
        // Source mode takes the tag for key listen's reason: the track is
        // playing a synth instead of its sampler, which is a change to what
        // comes out of the speakers rather than to what the keys do. So it
        // says so from any tab of the clip view, and not only from the pad
        // map where the keys for it live.
        if nav.sampler_source.as_deref().is_some_and(|m| m.is_armed()) {
            (
                "-- TAKE --",
                if super::meters::blink_on() {
                    Style::default()
                        .fg(theme::rec_active_val())
                        .bg(theme::bg_val())
                        .add_modifier(Modifier::BOLD)
                } else {
                    theme::dim()
                },
            )
        } else {
            ("-- SOURCE --", theme::amber_bright())
        }
    } else if nav.focused_pane == Pane::ClipView
        && nav.clip_view.clip_tab == ClipTab::Fx
        && nav.clip_view.focus == ClipViewFocus::PianoRoll
    {
        if nav.clip_view.fx.locked { ("-- HOLD --", theme::amber_bright()) } else { ("-- FX --", theme::amber_bright()) }
    } else if in_grid {
        // The step grid is a mode of its own: saying SELECT over a drum
        // machine describes the track list underneath it, not the thing the
        // keys are actually driving.
        if nav.clip_view.sequencer.locked {
            ("-- HOLD --", theme::amber_bright())
        } else {
            ("-- STEP --", theme::amber_bright())
        }
    } else if in_pads {
        // And so is the pad map, for the same reason. The trim strip is its
        // own mode again: it takes every key the map takes and means
        // something else by all of them — and so, one level up, does keys
        // mode, where the same keys address zones instead of pads.
        if nav.clip_view.sampler.trim.is_some() {
            ("-- TRIM --", theme::amber_bright())
        } else if nav.clip_view.sampler.root_learn {
            // Blinking, like every other tag that means "the box is
            // waiting for you to play something".
            (
                "-- ROOT? --",
                if super::meters::blink_on() {
                    Style::default()
                        .fg(theme::rec_active_val())
                        .bg(theme::bg_val())
                        .add_modifier(Modifier::BOLD)
                } else {
                    theme::dim()
                },
            )
        } else if nav.clip_view.sampler.locked {
            ("-- HOLD --", theme::amber_bright())
        } else if in_keys_mode {
            ("-- KEYS --", theme::amber_bright())
        } else {
            ("-- PADS --", theme::amber_bright())
        }
    } else if nav.track_selected {
        ("-- SELECT --", theme::amber())
    } else {
        ("-- NORMAL --", theme::normal())
    };
    frame.render_widget(Paragraph::new(Span::styled(format!(" {mt} "), ms)), cols[0]);

    // A status message takes the rest of the bar for as long as it lives —
    // the hints and the pane list, both of which are always the same for a
    // given mode and can be read again a moment later. A message saying a
    // preset was refused, or which file a session saved to, is only worth
    // anything at the moment it happens, and the hint column alone is 22
    // characters on an 80-wide terminal: not enough to say why.
    //
    // Except while a clip number is being typed: those digits are live input
    // feedback and the message is at most a few seconds old.
    // ...and it says the rest of it where the hints would be, because the one
    // thing a player needs at that moment is the way out.
    if let Some(name) = nav.key_listen_track_name() {
        let rest = Rect::new(
            cols[1].x,
            area.y,
            area.right().saturating_sub(cols[1].x),
            area.height,
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!("{name} is playing its sidechain key"),
                    if super::meters::blink_on() {
                        Style::default().fg(theme::rec_active_val()).bg(theme::bg_val())
                    } else {
                        theme::muted()
                    },
                ),
                Span::styled(
                    "  \u{00b7} esc puts it back \u{00b7} clears on stop",
                    theme::dim(),
                ),
            ])),
            rest,
        );
        return;
    }

    if let Some(msg) = status.filter(|_| nav.number_buf.display().is_empty()) {
        let rest = Rect::new(
            cols[1].x,
            area.y,
            area.right().saturating_sub(cols[1].x),
            area.height,
        );
        frame.render_widget(
            Paragraph::new(Span::styled(msg.to_string(), theme::amber_bright())),
            rest,
        );
        return;
    }

    let d = "\u{00B7}";
    let keys: Vec<(&str, &str)> = if nav.loop_editor.active {
        let toggle = if nav.loop_editor.enabled { "off" } else { "on" };
        vec![
            ("hl","start"),("H/L","end"),("jk","slide"),("g","grid"),
            ("y","lift"),("x","cut"),("p/P","stamp/layer"),("enter", toggle),("esc","done"),
        ]
    } else {
        match nav.focused_pane {
            Pane::Transport if nav.transport_ui.editing => vec![("hl","adjust"),("enter","done"),("esc","done")],
            Pane::Transport => vec![("hl","nav"),("enter","sel"),("+/-","bpm"),("tab","pane")],
            Pane::Tracks if nav.track_selected => vec![("hl","clip"),("m","mute"),("s","solo"),("r","arm"),("R","rec"),("esc","back")],
            Pane::Tracks => vec![("jk","track"),("enter","sel"),("m","mute"),("s","solo"),("r","arm"),("R","rec")],
            // The effect chain, and the panel behind a slot.
            Pane::ClipView if nav.clip_view.focus == ClipViewFocus::FxPanel
                && nav.clip_view.fx_panel_tab == FxPanelTab::TrackFx =>
                vec![("jk","slot"),("enter","open"),("b","byp"),("[]","order"),
                     ("d","remove"),("a","add")],
            Pane::ClipView if nav.clip_view.clip_tab == ClipTab::Fx
                && nav.clip_view.focus == ClipViewFocus::PianoRoll
                && nav.clip_view.fx.locked =>
                vec![("hl","adjust"),("H/L","stride"),("esc","release")],
            // The compressor's panel is a column of knobs with two routing
            // rows under them, so `j`/`k` picks and `h`/`l` turns — which is
            // the opposite of what the EQ's grid wants out of the same keys.
            Pane::ClipView if nav.clip_view.clip_tab == ClipTab::Fx
                && nav.clip_view.focus == ClipViewFocus::PianoRoll
                && nav.open_fx_type() == Some(FxType::Compressor) =>
                vec![("jk","knob"),("hl","adjust"),("H/L","stride"),
                     ("enter","hold"),("b","byp"),("esc","back")],
            Pane::ClipView if nav.clip_view.clip_tab == ClipTab::Fx
                && nav.clip_view.focus == ClipViewFocus::PianoRoll =>
                if nav.clip_view.fx.wide {
                    vec![("hl","band"),("jk","control"),("enter","hold"),
                         ("n","on/off"),("1-8","band"),("esc","back")]
                } else {
                    vec![("jk","band"),("hl","control"),("enter","hold"),
                         ("n","on/off"),("1-8","band"),("esc","back")]
                },

            // The step grid, band by band. A locked knob says only what it
            // can do, because it is the only thing that can be done.
            Pane::ClipView if nav.clip_view.clip_tab == ClipTab::Sequencer
                && nav.clip_view.focus == ClipViewFocus::PianoRoll
                && nav.clip_view.sequencer.locked =>
                vec![("hl","turn"),("H/L","stride"),("esc","release")],
            Pane::ClipView if nav.clip_view.clip_tab == ClipTab::Sequencer
                && nav.clip_view.focus == ClipViewFocus::PianoRoll =>
                match nav.clip_view.sequencer.band {
                    // The rows are sounds on a kit and voices on a keyboard,
                    // and j/k walks them either way.
                    // Trimmed to what fits beside the pane-jump hints on a
                    // 120-column terminal — a hint bar that runs into its
                    // neighbour reads as "b·boun", which hints at nothing.
                    SeqBand::Grid if nav.current_track()
                        .and_then(|t| t.sequencer.as_deref())
                        .is_some_and(|s| !s.pattern().lanes[0].is_pitched()) => vec![
                        ("hl","step"),("jk","sound"),("n","hit"),("enter","edit"),
                        ("a","acc"),("t","play"),
                    ],
                    SeqBand::Grid => vec![
                        ("hl","step"),("jk","row"),("n","hit"),("enter","edit"),
                        ("a","acc"),("t","play"),
                    ],
                    SeqBand::Step => vec![
                        ("hl","knob"),("enter","hold"),("jk","band"),("_","tie"),
                        ("n","hit"),("a","accent"),
                    ],
                    SeqBand::Pattern => vec![
                        ("hl","knob"),("enter","hold"),("jk","band"),("[]","lane"),
                    ],
                    SeqBand::Slots => vec![
                        ("hl","slot"),("enter","queue"),("c","chain"),("y/p","copy"),
                        ("X","clear"),("jk","band"),
                    ],
                },
            // Source mode has three keys and that is the whole list, which
            // is the point: everything else is a performance. Only while
            // the pad map has the keys, though — the tag above says the
            // mode is on from any tab, but on the FX tab it is the FX
            // panel's keys that answer.
            Pane::ClipView if in_source && in_pads =>
                vec![("r", if nav.sampler_source.as_deref().is_some_and(|m| m.is_armed()) {
                    "end take"
                } else {
                    "record"
                }), ("i","instrument"), ("esc","sampler")],
            // The trim strip. `h`/`l` take an edge rather than a pad, which
            // is the one thing about this mode a player has to know.
            Pane::ClipView if nav.clip_view.clip_tab == ClipTab::Pads
                && nav.clip_view.focus == ClipViewFocus::PianoRoll
                && nav.clip_view.sampler.trim.is_some() =>
                vec![("hl","start"),("H/L","end"),("jk","unit"),("z","snap"),
                     ("r","rev"),("t","loop"),("esc","back")],
            // A held span is the loop brace, so it says so: `H`/`L` are the
            // other edge here, not a stride, and a player who read the
            // knob's line would move the wrong end.
            Pane::ClipView if nav.clip_view.clip_tab == ClipTab::Pads
                && nav.clip_view.focus == ClipViewFocus::PianoRoll
                && nav.clip_view.sampler.locked
                && in_keys_mode
                && nav.clip_view.sampler.knob == 0 =>
                vec![("hl","low edge"),("H/L","high edge"),("esc","release")],
            // The pad map. `h`/`l` walk the keyboard until a knob is held,
            // which is the one thing about this tab a player has to know.
            Pane::ClipView if nav.clip_view.clip_tab == ClipTab::Pads
                && nav.clip_view.focus == ClipViewFocus::PianoRoll
                && nav.clip_view.sampler.locked =>
                vec![("hl","turn"),("H/L","stride"),("esc","release")],
            // Keys mode: the four keys a pad map has no word for, and `K`
            // to put it back.
            Pane::ClipView if nav.clip_view.clip_tab == ClipTab::Pads
                && nav.clip_view.focus == ClipViewFocus::PianoRoll
                && in_keys_mode =>
                vec![("hl","key"),("jk","knob"),("w/o/s","zone"),("D","drop"),
                     ("a","load"),("R","root"),("K","pads")],
            Pane::ClipView if nav.clip_view.clip_tab == ClipTab::Pads
                && nav.clip_view.focus == ClipViewFocus::PianoRoll =>
                vec![("hl","pad"),("jk","knob"),("[]","layer"),("i","source"),
                     ("a","load"),("t","trim"),("K","keys")],
            // Note editing: proximity nav, selection, and the velocity ride.
            Pane::ClipView if nav.clip_view.piano_roll.edit_mode =>
                vec![("hjkl","note"),("enter","sel"),(",.","vel"),("<>","vel\u{00b1}"),
                     ("m","mute"),("d","del"),("esc","exit")],
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
