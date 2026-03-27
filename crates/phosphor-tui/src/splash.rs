//! Splash screen with ASCII art and loading bar.

use std::io;
use std::time::Duration;

use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Terminal;

// ── Colors ──

const BG: Color = Color::Rgb(6, 6, 12);             // deep dark blue-black

// Aquamarine palette
const AQUA: Color = Color::Rgb(80, 220, 210);        // main aquamarine
const AQUA_BRIGHT: Color = Color::Rgb(140, 255, 245); // bright aqua
const AQUA_DIM: Color = Color::Rgb(25, 70, 68);       // dim aqua

// Violet palette
const VIOLET: Color = Color::Rgb(160, 100, 240);      // main violet
const VIOLET_BRIGHT: Color = Color::Rgb(200, 150, 255); // bright violet
const VIOLET_DIM: Color = Color::Rgb(50, 30, 80);      // dim violet

const BAR_BG: Color = Color::Rgb(12, 14, 22);         // dark bar background
const TAG: Color = Color::Rgb(100, 110, 130);          // muted cool tagline
const VER: Color = Color::Rgb(50, 55, 70);             // version

// ── Dot-matrix letters (5 rows each, using ● and space) ──

const DOT: &str = "\u{25CF}"; // ●

fn letter_p() -> [&'static str; 5] {
    ["####", "#  #", "####", "#   ", "#   "]
}
fn letter_h() -> [&'static str; 5] {
    ["#  #", "#  #", "####", "#  #", "#  #"]
}
fn letter_o() -> [&'static str; 5] {
    [" ## ", "#  #", "#  #", "#  #", " ## "]
}
fn letter_s() -> [&'static str; 5] {
    [" ###", "#   ", " ## ", "   #", "### "]
}
fn letter_r() -> [&'static str; 5] {
    ["### ", "#  #", "### ", "#  #", "#  #"]
}

fn render_word() -> Vec<String> {
    let letters = [
        letter_p(), letter_h(), letter_o(), letter_s(),
        letter_p(), letter_h(), letter_o(), letter_r(),
    ];

    let mut lines = vec![String::new(); 5];
    for (li, letter) in letters.iter().enumerate() {
        for row in 0..5 {
            if li > 0 { lines[row].push_str("  "); } // spacing between letters
            for ch in letter[row].chars() {
                if ch == '#' {
                    lines[row].push_str(DOT);
                } else {
                    lines[row].push(' ');
                }
            }
        }
    }
    lines
}

/// Interpolate between two colors.
fn lerp_color(a: Color, b: Color, t: f64) -> Color {
    let (ar, ag, ab) = color_rgb(a);
    let (br, bg, bb) = color_rgb(b);
    Color::Rgb(
        (ar as f64 + (br as f64 - ar as f64) * t) as u8,
        (ag as f64 + (bg as f64 - ag as f64) * t) as u8,
        (ab as f64 + (bb as f64 - ab as f64) * t) as u8,
    )
}

fn color_rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    }
}

const TAGLINE: &str = "where the terminal makes music";
const STAGES: &[&str] = &[
    "initializing audio engine",
    "scanning midi ports",
    "loading synthesizers",
    "warming up oscillators",
    "calibrating filters",
    "ready",
];

/// Show the splash screen. Does NOT leave alternate screen — the caller (App::run)
/// re-enters its own alternate screen, so we stay in ours to avoid flicker.
pub fn show_splash(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> anyhow::Result<()> {
    let word_lines = render_word();

    let total_steps = 50;
    for step in 0..=total_steps {
        let progress = step as f64 / total_steps as f64;
        let stage_idx = ((progress * (STAGES.len() - 1) as f64) as usize).min(STAGES.len() - 1);
        let stage = STAGES[stage_idx];
        // Shimmer phase — oscillates between aqua and violet
        let shimmer = (step as f64 * 0.3).sin() * 0.5 + 0.5; // 0..1

        terminal.draw(|frame| {
            let area = frame.area();
            let cx = area.width / 2;
            let cy = area.height / 2;

            // Background — fill entire screen
            let bg_block = ratatui::widgets::Block::default()
                .style(Style::default().bg(BG));
            frame.render_widget(bg_block, area);

            // ASCII art — centered
            let art_start_y = cy.saturating_sub(6);
            for (i, line) in word_lines.iter().enumerate() {
                let y = art_start_y + i as u16;
                if y >= area.height { break; }

                let chars: Vec<char> = line.chars().collect();
                let total_chars = chars.len();
                let lit_up = (progress * total_chars as f64) as usize;

                let mut spans = Vec::new();
                for (ci, ch) in chars.iter().enumerate() {
                    let color = if *ch == '\u{25CF}' {
                        if ci < lit_up {
                            // Each dot gets its own shimmer phase based on position
                            let dot_phase = ((ci as f64 * 0.15) + (step as f64 * 0.25)).sin() * 0.5 + 0.5;
                            if ci + 3 > lit_up {
                                // Leading edge — extra bright
                                lerp_color(AQUA_BRIGHT, VIOLET_BRIGHT, dot_phase)
                            } else {
                                // Lit body — shimmer between aqua and violet
                                lerp_color(AQUA, VIOLET, dot_phase)
                            }
                        } else {
                            // Unlit — dim shimmer
                            lerp_color(AQUA_DIM, VIOLET_DIM, shimmer)
                        }
                    } else {
                        BG
                    };
                    spans.push(Span::styled(
                        ch.to_string(),
                        Style::default().fg(color).bg(BG),
                    ));
                }

                let line_w = total_chars as u16;
                let x = cx.saturating_sub(line_w / 2);
                let r = Rect::new(x, y, line_w.min(area.width.saturating_sub(x)), 1);
                frame.render_widget(Paragraph::new(Line::from(spans)), r);
            }

            // Tagline — shimmers subtly
            let tag_y = art_start_y + 6;
            if tag_y < area.height {
                let tag_color = lerp_color(TAG, Color::Rgb(120, 130, 160), shimmer * 0.4);
                let tag_style = Style::default().fg(tag_color).bg(BG).add_modifier(Modifier::ITALIC);
                let tag_w = TAGLINE.len() as u16;
                let tag_x = cx.saturating_sub(tag_w / 2);
                let r = Rect::new(tag_x, tag_y, tag_w.min(area.width.saturating_sub(tag_x)), 1);
                frame.render_widget(
                    Paragraph::new(Span::styled(TAGLINE, tag_style)),
                    r,
                );
            }

            // Loading bar
            let bar_y = art_start_y + 8;
            if bar_y < area.height {
                let bar_w = 32u16;
                let bar_x = cx.saturating_sub(bar_w / 2 + 1);
                let filled = (progress * bar_w as f64) as u16;

                let mut bar_spans = Vec::new();
                let bracket_color = lerp_color(AQUA_DIM, VIOLET_DIM, shimmer);
                bar_spans.push(Span::styled("[", Style::default().fg(bracket_color).bg(BG)));
                for i in 0..bar_w {
                    if i < filled {
                        let bar_phase = ((i as f64 * 0.2) + (step as f64 * 0.3)).sin() * 0.5 + 0.5;
                        let bar_color = lerp_color(AQUA, VIOLET, bar_phase);
                        bar_spans.push(Span::styled(
                            "\u{2588}",
                            Style::default().fg(bar_color).bg(BAR_BG),
                        ));
                    } else {
                        bar_spans.push(Span::styled(
                            "\u{2500}",
                            Style::default().fg(Color::Rgb(20, 22, 35)).bg(BAR_BG),
                        ));
                    }
                }
                bar_spans.push(Span::styled("]", Style::default().fg(bracket_color).bg(BG)));

                let r = Rect::new(bar_x, bar_y, bar_w + 2, 1);
                frame.render_widget(Paragraph::new(Line::from(bar_spans)), r);
            }

            // Stage text
            let stage_y = art_start_y + 10;
            if stage_y < area.height {
                let dots = ".".repeat((step % 4) as usize);
                let text = format!("{}{}", stage, dots);
                let text_w = text.len() as u16;
                let stage_x = cx.saturating_sub(text_w / 2);
                let stage_color = lerp_color(AQUA_DIM, VIOLET_DIM, shimmer);
                let r = Rect::new(stage_x, stage_y, text_w.min(area.width.saturating_sub(stage_x)), 1);
                frame.render_widget(
                    Paragraph::new(Span::styled(text, Style::default().fg(stage_color).bg(BG))),
                    r,
                );
            }

            // Version — bottom right
            let ver = format!("v{}", env!("CARGO_PKG_VERSION"));
            let ver_x = area.width.saturating_sub(ver.len() as u16 + 1);
            let ver_y = area.height.saturating_sub(1);
            frame.render_widget(
                Paragraph::new(Span::styled(&ver, Style::default().fg(VER).bg(BG))),
                Rect::new(ver_x, ver_y, ver.len() as u16, 1),
            );
        })?;

        std::thread::sleep(Duration::from_millis(65));
    }

    // Hold on the completed screen — let it breathe
    // Do a few more shimmer frames with full bar
    for step in 0..15 {
        let shimmer = (step as f64 * 0.3 + 15.0).sin() * 0.5 + 0.5;
        terminal.draw(|frame| {
            let area = frame.area();
            let cx = area.width / 2;
            let cy = area.height / 2;

            let bg_block = ratatui::widgets::Block::default()
                .style(Style::default().bg(BG));
            frame.render_widget(bg_block, area);

            let art_start_y = cy.saturating_sub(6);
            for (i, line) in word_lines.iter().enumerate() {
                let y = art_start_y + i as u16;
                if y >= area.height { break; }
                let chars: Vec<char> = line.chars().collect();
                let total_chars = chars.len();
                let mut spans = Vec::new();
                for (ci, ch) in chars.iter().enumerate() {
                    let color = if *ch == '\u{25CF}' {
                        let dot_phase = ((ci as f64 * 0.15) + ((50 + step) as f64 * 0.25)).sin() * 0.5 + 0.5;
                        lerp_color(AQUA, VIOLET, dot_phase)
                    } else {
                        BG
                    };
                    spans.push(Span::styled(ch.to_string(), Style::default().fg(color).bg(BG)));
                }
                let line_w = total_chars as u16;
                let x = cx.saturating_sub(line_w / 2);
                let r = Rect::new(x, y, line_w.min(area.width.saturating_sub(x)), 1);
                frame.render_widget(Paragraph::new(Line::from(spans)), r);
            }

            let tag_y = art_start_y + 6;
            if tag_y < area.height {
                let tag_color = lerp_color(TAG, Color::Rgb(120, 130, 160), shimmer * 0.4);
                let tag_w = TAGLINE.len() as u16;
                let tag_x = cx.saturating_sub(tag_w / 2);
                let r = Rect::new(tag_x, tag_y, tag_w.min(area.width.saturating_sub(tag_x)), 1);
                frame.render_widget(
                    Paragraph::new(Span::styled(TAGLINE, Style::default().fg(tag_color).bg(BG).add_modifier(Modifier::ITALIC))),
                    r,
                );
            }

            let bar_y = art_start_y + 8;
            if bar_y < area.height {
                let bar_w = 32u16;
                let bar_x = cx.saturating_sub(bar_w / 2 + 1);
                let bracket_color = lerp_color(AQUA_DIM, VIOLET_DIM, shimmer);
                let mut bar_spans = Vec::new();
                bar_spans.push(Span::styled("[", Style::default().fg(bracket_color).bg(BG)));
                for i in 0..bar_w {
                    let bar_phase = ((i as f64 * 0.2) + ((50 + step) as f64 * 0.3)).sin() * 0.5 + 0.5;
                    let bar_color = lerp_color(AQUA, VIOLET, bar_phase);
                    bar_spans.push(Span::styled("\u{2588}", Style::default().fg(bar_color).bg(BAR_BG)));
                }
                bar_spans.push(Span::styled("]", Style::default().fg(bracket_color).bg(BG)));
                let r = Rect::new(bar_x, bar_y, bar_w + 2, 1);
                frame.render_widget(Paragraph::new(Line::from(bar_spans)), r);
            }

            let stage_y = art_start_y + 10;
            if stage_y < area.height {
                let ready_color = lerp_color(AQUA, VIOLET, shimmer);
                let text_w = 5u16; // "ready"
                let stage_x = cx.saturating_sub(text_w / 2);
                let r = Rect::new(stage_x, stage_y, text_w, 1);
                frame.render_widget(
                    Paragraph::new(Span::styled("ready", Style::default().fg(ready_color).bg(BG))),
                    r,
                );
            }

            let ver = format!("v{}", env!("CARGO_PKG_VERSION"));
            let ver_x = area.width.saturating_sub(ver.len() as u16 + 1);
            let ver_y = area.height.saturating_sub(1);
            frame.render_widget(
                Paragraph::new(Span::styled(&ver, Style::default().fg(VER).bg(BG))),
                Rect::new(ver_x, ver_y, ver.len() as u16, 1),
            );
        })?;
        std::thread::sleep(Duration::from_millis(80));
    }

    Ok(())
}
