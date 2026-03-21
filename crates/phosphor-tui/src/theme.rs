//! Phosphor solarized-dark theme. All colors in one place.

use ratatui::style::{Color, Modifier, Style};

// ── Base palette ──

/// Deep dark blue-black background.
pub const BG: Color = Color::Rgb(11, 29, 40);
/// Border / separator lines.
pub const BORDER: Color = Color::Rgb(15, 37, 53);
/// Dimmest text (barely visible hints).
pub const DIM: Color = Color::Rgb(22, 46, 60);
/// Muted text (secondary info).
pub const MUTED: Color = Color::Rgb(26, 58, 76);
/// Normal text (labels, status).
pub const NORMAL: Color = Color::Rgb(37, 78, 98);
/// Bright text (primary content).
pub const BRIGHT: Color = Color::Rgb(56, 113, 127);
/// Highlight text (important values, branding).
pub const HIGHLIGHT: Color = Color::Rgb(88, 160, 176);

// ── Accent colors ──

/// Amber accent (BPM, playhead, active values).
pub const AMBER: Color = Color::Rgb(114, 88, 14);
/// Bright amber for emphasis.
pub const AMBER_BRIGHT: Color = Color::Rgb(150, 116, 20);
/// Record red (muted).
pub const REC_DIM: Color = Color::Rgb(90, 26, 26);
/// Record red (active).
pub const REC_ACTIVE: Color = Color::Rgb(180, 50, 50);

// ── Track colors (color-coded per track) ──

pub const TRACK_COLORS: &[Color] = &[
    Color::Rgb(56, 148, 142),  // teal
    Color::Rgb(108, 130, 60),  // olive green
    Color::Rgb(140, 100, 50),  // amber/brown
    Color::Rgb(80, 110, 160),  // steel blue
    Color::Rgb(130, 80, 130),  // muted purple
    Color::Rgb(60, 140, 100),  // sea green
    Color::Rgb(150, 90, 70),   // terracotta
    Color::Rgb(70, 120, 140),  // slate cyan
];

pub fn track_color(index: usize) -> Color {
    TRACK_COLORS[index % TRACK_COLORS.len()]
}

// ── Convenience styles ──

pub fn bg() -> Style {
    Style::default().bg(BG)
}

pub fn dim() -> Style {
    Style::default().fg(DIM).bg(BG)
}

pub fn muted() -> Style {
    Style::default().fg(MUTED).bg(BG)
}

pub fn normal() -> Style {
    Style::default().fg(NORMAL).bg(BG)
}

pub fn amber() -> Style {
    Style::default().fg(AMBER).bg(BG)
}

pub fn amber_bright() -> Style {
    Style::default().fg(AMBER_BRIGHT).bg(BG)
}

pub fn branding() -> Style {
    Style::default()
        .fg(BRIGHT)
        .bg(BG)
        .add_modifier(Modifier::BOLD)
}

pub fn rec_dim() -> Style {
    Style::default().fg(REC_DIM).bg(BG)
}

pub fn rec_active() -> Style {
    Style::default().fg(REC_ACTIVE).bg(BG)
}


pub fn border_style() -> Style {
    Style::default().fg(BORDER).bg(BG)
}

// ── Color helpers ──

/// Dim a color by a percentage (0..100).
pub fn dim_color(c: Color, pct: u16) -> Color {
    Color::Rgb((tc_r(c) as u16*pct/100) as u8, (tc_g(c) as u16*pct/100) as u8, (tc_b(c) as u16*pct/100) as u8)
}

/// Extract the red channel from an RGB color.
pub fn tc_r(c: Color) -> u8 { if let Color::Rgb(r,_,_) = c { r } else { 128 } }
/// Extract the green channel from an RGB color.
pub fn tc_g(c: Color) -> u8 { if let Color::Rgb(_,g,_) = c { g } else { 128 } }
/// Extract the blue channel from an RGB color.
pub fn tc_b(c: Color) -> u8 { if let Color::Rgb(_,_,b) = c { b } else { 128 } }

/// Style for track header buttons (fx, volume, mute, etc).
pub fn btn_style(active: bool, focused: bool, tc: Color) -> Style {
    if active {
        Style::default().fg(dim_color(tc, 80))
            .bg(if focused { Color::Rgb(25,40,50) } else { Color::Rgb(12,22,32) })
            .add_modifier(Modifier::BOLD)
    } else if focused {
        Style::default().fg(NORMAL).bg(Color::Rgb(25,40,50))
    } else {
        Style::default().fg(Color::Rgb(18,50,72)).bg(Color::Rgb(7,17,28))
    }
}
