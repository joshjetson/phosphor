//! The practice room's screen: the drill list, and the drill itself —
//! a keyboard with the fingering written on the keys, a target lane in
//! the typing-tutor shape (what to play above, how you played below),
//! and the numbers that matter (tempo, streak, bias, spread, evenness).

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use phosphor_app::practice::judge::{HitState, Mode, Verdict};
use phosphor_app::practice::{Family, Hand, TargetNote, NOTE_NAMES};

use crate::state::NavState;
use crate::theme;

pub(super) fn render_practice(frame: &mut Frame, area: Rect, nav: &NavState) {
    let room = &nav.practice;
    if room.run.is_some() {
        render_run(frame, area, nav);
    } else {
        render_browse(frame, area, nav);
    }
}

fn render_browse(frame: &mut Frame, area: Rect, nav: &NavState) {
    let room = &nav.practice;
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("  fingers ", theme::amber_bright().add_modifier(Modifier::BOLD)),
        Span::styled("\u{00b7} technique drills, judged from your playing", theme::dim()),
    ]));
    lines.push(Line::from(""));

    let selected = room.selected();
    for (i, family) in Family::ALL.iter().enumerate() {
        let here = room.cursor == i;
        let style = if here {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else {
            theme::normal()
        };
        // The record shown is for the *current* key/hands variant of the
        // row under the cursor, and the family's plain form elsewhere.
        let probe = if here {
            selected.id.clone()
        } else {
            let hands = if family.handed() { room.hands } else { phosphor_app::practice::Hands::Left };
            phosphor_app::practice::build(*family, room.key(), hands).id
        };
        let best = room.record_for(&probe);
        let best_str = if best > 0 {
            format!("\u{2669}={best}")
        } else {
            "\u{2014}".to_string()
        };
        lines.push(Line::from(vec![
            Span::styled(if here { " \u{25B6} " } else { "   " }, style),
            Span::styled(format!("L{} ", family.level()), theme::dim()),
            Span::styled(format!("{:<28}", family.title()), style),
            Span::styled(format!("{best_str:>8}"), if best > 0 { theme::amber() } else { theme::dim() }),
        ]));
    }

    lines.push(Line::from(""));
    let family = room.family();
    let mut sel = vec![Span::styled("   ", theme::dim())];
    if family.keyed() {
        sel.push(Span::styled(
            format!("key {} \u{00b7} ", NOTE_NAMES[room.key() as usize]),
            theme::amber(),
        ));
    }
    if family.handed() {
        sel.push(Span::styled(format!("{} \u{00b7} ", room.hands.label()), theme::amber()));
    }
    sel.push(Span::styled(
        format!(
            "\u{2669}={} \u{00b7} {} \u{00b7} click {}",
            room.start_bpm(),
            room.mode.label(),
            room.click.label()
        ),
        theme::amber(),
    ));
    lines.push(Line::from(sel));
    lines.push(Line::from(Span::styled(
        format!("   {}", family.coach()),
        theme::dim(),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "   j/k drill \u{00b7} < > key \u{00b7} h hands \u{00b7} w wait/flow \u{00b7} c click \u{00b7} [ ] tempo",
        theme::dim(),
    )));
    lines.push(Line::from(Span::styled(
        "   enter \u{2014} start \u{00b7} esc \u{2014} leave \u{00b7} your controller sounds the selected track",
        theme::dim(),
    )));

    lines.truncate(area.height as usize);
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_run(frame: &mut Frame, area: Rect, nav: &NavState) {
    let room = &nav.practice;
    let Some(run) = &room.run else { return };
    let mut lines: Vec<Line> = Vec::new();

    // ── Header ──
    let streak_dots: String =
        (0..3u32).map(|i| if i < run.streak { '\u{25cf}' } else { '\u{25cb}' }).collect();
    lines.push(Line::from(vec![
        Span::styled(format!("  {} ", run.exercise.title), theme::amber_bright().add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(
                "\u{00b7} \u{2669}={} \u{00b7} {} \u{00b7} click {} \u{00b7} rep {} \u{00b7} {streak_dots}",
                room.start_bpm(),
                room.mode.label(),
                room.click.label(),
                run.rep,
            ),
            theme::amber(),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        format!("  {}", Family::ALL[room.cursor.min(Family::ALL.len() - 1)].coach()),
        theme::dim(),
    )));
    lines.push(Line::from(""));

    // ── Target lane: fingers above, notes below, verdicts trailing ──
    let targets = run.judge.targets();
    let focus = lane_focus(&run.judge, targets);
    let lo = focus.saturating_sub(3);
    let hi = (lo + 16).min(targets.len());
    let mut finger_row = vec![Span::styled("  ", theme::dim())];
    let mut note_row = vec![Span::styled("  ", theme::dim())];
    for i in lo..hi {
        let t = &targets[i];
        let status = run.judge.status(i);
        let fg_style = match status {
            HitState::Hit(dev) => Style::default().fg(verdict_color(dev, run.judge.window_ms)),
            HitState::Missed => Style::default().fg(theme::rec_active_val()),
            HitState::Pending => {
                let here = i == focus;
                if here {
                    theme::amber_bright().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                } else if t.hand == Hand::Left {
                    Style::default().fg(Color::Rgb(120, 170, 220))
                } else {
                    theme::normal()
                }
            }
        };
        let name = note_label(t.note);
        let finger = if t.finger > 0 { t.finger.to_string() } else { "\u{00b7}".into() };
        finger_row.push(Span::styled(format!("{finger:^5}"), fg_style));
        note_row.push(Span::styled(format!("{name:^5}"), fg_style));
    }
    lines.push(Line::from(finger_row));
    lines.push(Line::from(note_row));
    lines.push(Line::from(""));

    // ── The keyboard ──
    let (klo, khi) = keyboard_range(targets);
    let width = area.width as usize;
    for line in keyboard_band(klo, khi, width, run, focus) {
        lines.push(line);
    }
    lines.push(Line::from(""));

    // ── Feedback ──
    let mut fb = vec![Span::styled("  ", theme::dim())];
    if let Some(v) = run.judge.last_verdict {
        let (text, color) = match v {
            Verdict::Perfect(d) => (format!("\u{25cf} {d:+.0}ms"), Color::Rgb(120, 220, 120)),
            Verdict::Good(d) => (format!("\u{25cb} {d:+.0}ms"), Color::Rgb(200, 220, 120)),
            Verdict::Early(d) => (format!("\u{25c2} early {d:+.0}ms"), Color::Rgb(230, 170, 90)),
            Verdict::Late(d) => (format!("\u{25b8} late {d:+.0}ms"), Color::Rgb(200, 140, 220)),
            Verdict::Wrong(n) => (format!("\u{00d7} {}", note_label(n)), theme::rec_active_val()),
        };
        fb.push(Span::styled(text, Style::default().fg(color).add_modifier(Modifier::BOLD)));
    }
    if let Some(r) = run.last_report {
        let verdict = if r.clean { "CLEAN" } else { "again" };
        fb.push(Span::styled(
            format!(
                "   last rep: {}/{} \u{00b7} {} wrong \u{00b7} bias {:+.0}ms \u{00b7} spread {:.0}ms \u{00b7} even {:.1}% \u{00b7} {}",
                r.hit, r.total, r.wrong, r.bias_ms, r.spread_ms, r.ioi_cv, verdict
            ),
            if r.clean { theme::amber() } else { theme::dim() },
        ));
    }
    lines.push(Line::from(fb));
    lines.push(Line::from(Span::styled(
        "  enter/esc \u{2014} stop \u{00b7} [ ] tempo \u{00b7} w mode \u{00b7} c click \u{00b7} < > key",
        theme::dim(),
    )));

    lines.truncate(area.height as usize);
    frame.render_widget(Paragraph::new(lines), area);
}

/// Which target the lane should center on.
fn lane_focus(judge: &phosphor_app::practice::judge::Judge, targets: &[TargetNote]) -> usize {
    match judge.mode {
        Mode::Wait => judge.cursor.min(targets.len().saturating_sub(1)),
        Mode::Flow => targets
            .iter()
            .enumerate()
            .find(|(i, _)| matches!(judge.status(*i), HitState::Pending))
            .map_or(targets.len().saturating_sub(1), |(i, _)| i),
    }
}

fn verdict_color(dev_ms: f32, window: f32) -> Color {
    if dev_ms.abs() <= window * 0.25 {
        Color::Rgb(120, 220, 120)
    } else if dev_ms < 0.0 {
        Color::Rgb(230, 170, 90)
    } else {
        Color::Rgb(200, 140, 220)
    }
}

fn note_label(note: u8) -> String {
    format!("{}{}", NOTE_NAMES[(note % 12) as usize], i32::from(note) / 12 - 1)
}

fn keyboard_range(targets: &[TargetNote]) -> (u8, u8) {
    let lo = targets.iter().map(|t| t.note).min().unwrap_or(48);
    let hi = targets.iter().map(|t| t.note).max().unwrap_or(72);
    // Pad to octave boundaries with a little air.
    let lo = (lo.saturating_sub(2) / 12) * 12;
    let hi = ((hi + 13) / 12) * 12;
    (lo.max(21), hi.min(108))
}

const WHITE_PCS: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];

fn is_black(note: u8) -> bool {
    matches!(note % 12, 1 | 3 | 6 | 8 | 10)
}

/// White-key index of a note from `lo` (counting white keys only).
fn white_index(lo: u8, note: u8) -> usize {
    (lo..note).filter(|n| !is_black(*n)).count()
}

/// The keyboard band: five rows. The upper three carry the black keys,
/// the lower two the white; targets carry their finger number on the key,
/// pressed keys flip green (right) or red (wrong).
fn keyboard_band(
    lo: u8,
    hi: u8,
    width: usize,
    run: &phosphor_app::practice::Run,
    focus: usize,
) -> Vec<Line<'static>> {
    let targets = run.judge.targets();
    // The keys the player should have down *now*: the wait group, or the
    // pending flow targets nearest the focus tick.
    let focus_tick = targets.get(focus).map(|t| t.tick).unwrap_or(0);
    let mut wanted: Vec<(u8, u8, Hand)> = Vec::new();
    for (i, t) in targets.iter().enumerate() {
        if t.tick == focus_tick && matches!(run.judge.status(i), HitState::Pending) {
            wanted.push((t.note, t.finger, t.hand));
        }
    }

    let white_total = white_index(lo, hi) + 1;
    let cell = 2usize;
    let usable = width.saturating_sub(4);
    let white_shown = (usable / cell).min(white_total);
    let bg_white = theme::piano_white_bg();
    let bg_white_lit = Color::Rgb(240, 240, 230);
    let bg_black = Color::Rgb(20, 20, 24);
    let green = Color::Rgb(60, 160, 80);
    let red = theme::rec_active_val();
    let amber = theme::amber_bright_val();
    let lh_blue = Color::Rgb(70, 130, 200);

    let key_color = |note: u8, upper: bool| -> (Color, Option<char>) {
        let down = run.down.contains(&note);
        let want = wanted.iter().find(|(n, _, _)| *n == note);
        match (want, down) {
            (Some(_), true) => (green, None),
            (Some(&(_, finger, hand)), false) => {
                let base = if hand == Hand::Left { lh_blue } else { amber };
                let digit = if finger > 0 {
                    char::from_digit(u32::from(finger), 10)
                } else {
                    None
                };
                (base, digit)
            }
            (None, true) => {
                // A key down that nothing asked for: red, unless the judge
                // counts it a recent hit (chords release slowly).
                let recently_ok = targets
                    .iter()
                    .enumerate()
                    .any(|(i, t)| t.note == note && matches!(run.judge.status(i), HitState::Hit(_)));
                if recently_ok {
                    (green, None)
                } else {
                    (red, None)
                }
            }
            (None, false) => {
                if upper {
                    (bg_black, None)
                } else {
                    (bg_white, None)
                }
            }
        }
    };
    let _ = bg_white_lit;

    let mut rows: Vec<Vec<Span<'static>>> = vec![vec![Span::styled("  ", theme::dim())]; 5];
    let mut white = lo;
    // Walk white keys left to right; consult the black key above each.
    let mut shown = 0usize;
    while shown < white_shown && white <= hi {
        if is_black(white) {
            white += 1;
            continue;
        }
        let w = white;
        // Rows 3-4: the white key body.
        let (wc, wdigit) = key_color(w, false);
        let wtext = |digit: Option<char>| -> String {
            match digit {
                Some(d) => format!("{d} "),
                None => "  ".to_string(),
            }
        };
        rows[3].push(Span::styled(
            wtext(None),
            Style::default().bg(wc),
        ));
        rows[4].push(Span::styled(
            wtext(wdigit).to_string(),
            Style::default().bg(wc).fg(Color::Rgb(10, 10, 10)).add_modifier(Modifier::BOLD),
        ));
        // Rows 0-2: black key between this white and the next, drawn on
        // this cell's right half, or the white key's continuation.
        let black = w + 1;
        let has_black = black <= hi && is_black(black) && WHITE_PCS.contains(&(w % 12)) && !matches!(w % 12, 4 | 11);
        for row in 0..3 {
            if has_black {
                let (bc, bdigit) = key_color(black, true);
                let digit = if row == 1 { bdigit } else { None };
                let text = match digit {
                    Some(d) => format!("{d}"),
                    None => " ".to_string(),
                };
                rows[row].push(Span::styled(
                    " ".to_string(),
                    Style::default().bg(key_color(w, false).0),
                ));
                rows[row].push(Span::styled(
                    text,
                    Style::default().bg(bc).fg(Color::Rgb(230, 230, 230)).add_modifier(Modifier::BOLD),
                ));
            } else {
                rows[row].push(Span::styled(
                    "  ".to_string(),
                    Style::default().bg(key_color(w, false).0),
                ));
            }
        }
        white += 1;
        shown += 1;
    }
    rows.into_iter().map(Line::from).collect()
}
