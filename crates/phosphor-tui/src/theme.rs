//! Phosphor themes. All colors in one place.
//! Switch themes via the space menu settings.

use ratatui::style::{Color, Modifier, Style};
use std::sync::atomic::{AtomicU8, Ordering};

// ── Theme selection ──

static ACTIVE_THEME: AtomicU8 = AtomicU8::new(0);

pub const THEME_COUNT: usize = 9;
pub const THEME_NAMES: [&str; THEME_COUNT] = [
    "Phosphor",    // original solarized-dark
    "SpaceVim",    // spacevim-inspired dark
    "Gruvbox",     // gruvbox dark
    "Midnight",    // deep blue midnight
    "Dracula",     // dracula purple
    "Nord",        // nord polar night
    "Jellybean",   // jellybean vim colorscheme
    "Catppuccin",  // catppuccin mocha
    "SpaceVim2",   // authentic SpaceVim colorscheme
];

pub fn current_theme() -> usize {
    ACTIVE_THEME.load(Ordering::Relaxed) as usize
}

pub fn set_theme(index: usize) {
    ACTIVE_THEME.store((index % THEME_COUNT) as u8, Ordering::Relaxed);
    save_preference();
}

pub fn next_theme() {
    let cur = current_theme();
    set_theme((cur + 1) % THEME_COUNT);
}

/// Load theme preference from ~/.phosphor/config.json on startup.
pub fn load_preference() {
    if let Some(home) = dirs_path() {
        let config = home.join("config.json");
        if let Ok(data) = std::fs::read_to_string(&config) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(idx) = val.get("theme").and_then(|v| v.as_u64()) {
                    ACTIVE_THEME.store((idx as usize % THEME_COUNT) as u8, Ordering::Relaxed);
                }
            }
        }
    }
}

/// Save theme preference to ~/.phosphor/config.json.
fn save_preference() {
    if let Some(dir) = dirs_path() {
        let _ = std::fs::create_dir_all(&dir);
        let config = dir.join("config.json");
        let val = serde_json::json!({ "theme": current_theme() });
        let _ = std::fs::write(&config, serde_json::to_string_pretty(&val).unwrap_or_default());
    }
}

fn dirs_path() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join(".phosphor"))
}

pub fn theme_name() -> &'static str {
    THEME_NAMES[current_theme()]
}

// ── Theme palette ──

#[allow(dead_code)]
struct Palette {
    bg: Color,
    border: Color,
    dim: Color,
    muted: Color,
    normal: Color,
    bright: Color,
    highlight: Color,
    amber: Color,
    amber_bright: Color,
    rec_dim: Color,
    rec_active: Color,
    track_colors: &'static [Color],
    // Button style overrides
    btn_active_bg: Color,
    btn_focused_bg: Color,
    btn_default_fg: Color,
    btn_default_bg: Color,
}

// ── Phosphor (original solarized-dark) ──

const PHOSPHOR_TRACKS: [Color; 8] = [
    Color::Rgb(56, 148, 142),  // teal
    Color::Rgb(108, 130, 60),  // olive green
    Color::Rgb(140, 100, 50),  // amber/brown
    Color::Rgb(80, 110, 160),  // steel blue
    Color::Rgb(130, 80, 130),  // muted purple
    Color::Rgb(60, 140, 100),  // sea green
    Color::Rgb(150, 90, 70),   // terracotta
    Color::Rgb(70, 120, 140),  // slate cyan
];

const PHOSPHOR: Palette = Palette {
    bg: Color::Rgb(11, 29, 40),
    border: Color::Rgb(15, 37, 53),
    dim: Color::Rgb(22, 46, 60),
    muted: Color::Rgb(26, 58, 76),
    normal: Color::Rgb(37, 78, 98),
    bright: Color::Rgb(56, 113, 127),
    highlight: Color::Rgb(88, 160, 176),
    amber: Color::Rgb(114, 88, 14),
    amber_bright: Color::Rgb(150, 116, 20),
    rec_dim: Color::Rgb(90, 26, 26),
    rec_active: Color::Rgb(180, 50, 50),
    track_colors: &PHOSPHOR_TRACKS,
    btn_active_bg: Color::Rgb(12, 22, 32),
    btn_focused_bg: Color::Rgb(25, 40, 50),
    btn_default_fg: Color::Rgb(18, 50, 72),
    btn_default_bg: Color::Rgb(7, 17, 28),
};

// ── SpaceVim ──

const SPACEVIM_TRACKS: [Color; 8] = [
    Color::Rgb(65, 166, 181),  // cyan
    Color::Rgb(180, 142, 60),  // gold
    Color::Rgb(130, 170, 100), // green
    Color::Rgb(100, 130, 200), // blue
    Color::Rgb(190, 100, 140), // pink
    Color::Rgb(80, 180, 130),  // mint
    Color::Rgb(200, 120, 80),  // orange
    Color::Rgb(140, 110, 190), // purple
];

const SPACEVIM: Palette = Palette {
    bg: Color::Rgb(30, 30, 40),
    border: Color::Rgb(50, 50, 65),
    dim: Color::Rgb(65, 65, 80),
    muted: Color::Rgb(90, 90, 110),
    normal: Color::Rgb(130, 130, 150),
    bright: Color::Rgb(180, 180, 200),
    highlight: Color::Rgb(210, 210, 230),
    amber: Color::Rgb(200, 155, 50),
    amber_bright: Color::Rgb(230, 185, 70),
    rec_dim: Color::Rgb(100, 30, 30),
    rec_active: Color::Rgb(200, 60, 60),
    track_colors: &SPACEVIM_TRACKS,
    btn_active_bg: Color::Rgb(40, 40, 55),
    btn_focused_bg: Color::Rgb(50, 50, 70),
    btn_default_fg: Color::Rgb(70, 70, 90),
    btn_default_bg: Color::Rgb(25, 25, 35),
};

// ── Gruvbox Dark ──

const GRUVBOX_TRACKS: [Color; 8] = [
    Color::Rgb(104, 157, 106), // green
    Color::Rgb(214, 153, 62),  // orange
    Color::Rgb(69, 133, 136),  // aqua
    Color::Rgb(177, 98, 134),  // purple
    Color::Rgb(215, 186, 125), // yellow
    Color::Rgb(131, 165, 152), // teal
    Color::Rgb(204, 36, 29),   // red
    Color::Rgb(152, 151, 26),  // lime
];

const GRUVBOX: Palette = Palette {
    bg: Color::Rgb(40, 40, 40),
    border: Color::Rgb(60, 56, 54),
    dim: Color::Rgb(80, 73, 69),
    muted: Color::Rgb(102, 92, 84),
    normal: Color::Rgb(168, 153, 132),
    bright: Color::Rgb(213, 196, 161),
    highlight: Color::Rgb(235, 219, 178),
    amber: Color::Rgb(214, 153, 62),
    amber_bright: Color::Rgb(250, 189, 47),
    rec_dim: Color::Rgb(120, 30, 30),
    rec_active: Color::Rgb(204, 36, 29),
    track_colors: &GRUVBOX_TRACKS,
    btn_active_bg: Color::Rgb(50, 48, 47),
    btn_focused_bg: Color::Rgb(60, 56, 54),
    btn_default_fg: Color::Rgb(102, 92, 84),
    btn_default_bg: Color::Rgb(32, 32, 32),
};

// ── Midnight ──

const MIDNIGHT_TRACKS: [Color; 8] = [
    Color::Rgb(80, 140, 200),  // bright blue
    Color::Rgb(100, 180, 160), // seafoam
    Color::Rgb(180, 130, 80),  // warm gold
    Color::Rgb(150, 100, 180), // violet
    Color::Rgb(80, 170, 120),  // emerald
    Color::Rgb(200, 100, 100), // coral
    Color::Rgb(120, 160, 80),  // lime
    Color::Rgb(160, 120, 160), // mauve
];

const MIDNIGHT: Palette = Palette {
    bg: Color::Rgb(15, 15, 30),
    border: Color::Rgb(30, 30, 55),
    dim: Color::Rgb(40, 40, 70),
    muted: Color::Rgb(60, 60, 100),
    normal: Color::Rgb(100, 100, 150),
    bright: Color::Rgb(150, 150, 200),
    highlight: Color::Rgb(190, 190, 230),
    amber: Color::Rgb(130, 100, 40),
    amber_bright: Color::Rgb(170, 140, 60),
    rec_dim: Color::Rgb(80, 20, 40),
    rec_active: Color::Rgb(180, 40, 60),
    track_colors: &MIDNIGHT_TRACKS,
    btn_active_bg: Color::Rgb(25, 25, 50),
    btn_focused_bg: Color::Rgb(35, 35, 65),
    btn_default_fg: Color::Rgb(50, 50, 85),
    btn_default_bg: Color::Rgb(12, 12, 25),
};

// ── Dracula ──

const DRACULA_TRACKS: [Color; 8] = [
    Color::Rgb(139, 233, 253), // cyan
    Color::Rgb(80, 250, 123),  // green
    Color::Rgb(255, 184, 108), // orange
    Color::Rgb(189, 147, 249), // purple
    Color::Rgb(255, 121, 198), // pink
    Color::Rgb(241, 250, 140), // yellow
    Color::Rgb(255, 85, 85),   // red
    Color::Rgb(98, 114, 164),  // comment blue
];

const DRACULA: Palette = Palette {
    bg: Color::Rgb(40, 42, 54),
    border: Color::Rgb(68, 71, 90),
    dim: Color::Rgb(68, 71, 90),
    muted: Color::Rgb(98, 114, 164),
    normal: Color::Rgb(148, 150, 172),
    bright: Color::Rgb(208, 210, 224),
    highlight: Color::Rgb(248, 248, 242),
    amber: Color::Rgb(255, 184, 108),
    amber_bright: Color::Rgb(255, 214, 148),
    rec_dim: Color::Rgb(120, 30, 30),
    rec_active: Color::Rgb(255, 85, 85),
    track_colors: &DRACULA_TRACKS,
    btn_active_bg: Color::Rgb(55, 57, 72),
    btn_focused_bg: Color::Rgb(65, 67, 82),
    btn_default_fg: Color::Rgb(68, 71, 90),
    btn_default_bg: Color::Rgb(33, 34, 44),
};

// ── Nord ──

const NORD_TRACKS: [Color; 8] = [
    Color::Rgb(136, 192, 208), // frost cyan
    Color::Rgb(163, 190, 140), // aurora green
    Color::Rgb(208, 135, 112), // aurora orange
    Color::Rgb(180, 142, 173), // aurora purple
    Color::Rgb(191, 97, 106),  // aurora red
    Color::Rgb(235, 203, 139), // aurora yellow
    Color::Rgb(129, 161, 193), // frost blue
    Color::Rgb(143, 188, 187), // frost teal
];

const NORD: Palette = Palette {
    bg: Color::Rgb(46, 52, 64),
    border: Color::Rgb(59, 66, 82),
    dim: Color::Rgb(67, 76, 94),
    muted: Color::Rgb(76, 86, 106),
    normal: Color::Rgb(129, 161, 193),
    bright: Color::Rgb(216, 222, 233),
    highlight: Color::Rgb(236, 239, 244),
    amber: Color::Rgb(235, 203, 139),
    amber_bright: Color::Rgb(245, 224, 170),
    rec_dim: Color::Rgb(120, 50, 50),
    rec_active: Color::Rgb(191, 97, 106),
    track_colors: &NORD_TRACKS,
    btn_active_bg: Color::Rgb(59, 66, 82),
    btn_focused_bg: Color::Rgb(67, 76, 94),
    btn_default_fg: Color::Rgb(76, 86, 106),
    btn_default_bg: Color::Rgb(39, 43, 53),
};

// ── Jellybean ──

const JELLYBEAN_TRACKS: [Color; 8] = [
    Color::Rgb(143, 191, 220), // light blue
    Color::Rgb(163, 211, 156), // light green
    Color::Rgb(226, 185, 130), // tan
    Color::Rgb(207, 106, 76),  // salmon
    Color::Rgb(178, 148, 187), // lavender
    Color::Rgb(130, 196, 183), // seafoam
    Color::Rgb(222, 165, 132), // peach
    Color::Rgb(112, 185, 186), // teal
];

const JELLYBEAN: Palette = Palette {
    bg: Color::Rgb(18, 18, 18),
    border: Color::Rgb(48, 48, 48),
    dim: Color::Rgb(68, 68, 68),
    muted: Color::Rgb(98, 98, 98),
    normal: Color::Rgb(144, 144, 144),
    bright: Color::Rgb(198, 198, 198),
    highlight: Color::Rgb(218, 218, 218),
    amber: Color::Rgb(226, 185, 130),
    amber_bright: Color::Rgb(255, 215, 160),
    rec_dim: Color::Rgb(110, 35, 35),
    rec_active: Color::Rgb(207, 106, 76),
    track_colors: &JELLYBEAN_TRACKS,
    btn_active_bg: Color::Rgb(32, 32, 32),
    btn_focused_bg: Color::Rgb(42, 42, 42),
    btn_default_fg: Color::Rgb(68, 68, 68),
    btn_default_bg: Color::Rgb(14, 14, 14),
};

// ── Catppuccin Mocha ──

const CATPPUCCIN_TRACKS: [Color; 8] = [
    Color::Rgb(137, 220, 235), // sky
    Color::Rgb(166, 227, 161), // green
    Color::Rgb(249, 226, 175), // yellow
    Color::Rgb(203, 166, 247), // mauve
    Color::Rgb(245, 194, 231), // pink
    Color::Rgb(148, 226, 213), // teal
    Color::Rgb(250, 179, 135), // peach
    Color::Rgb(116, 199, 236), // sapphire
];

const CATPPUCCIN: Palette = Palette {
    bg: Color::Rgb(30, 30, 46),
    border: Color::Rgb(49, 50, 68),
    dim: Color::Rgb(69, 71, 90),
    muted: Color::Rgb(88, 91, 112),
    normal: Color::Rgb(166, 173, 200),
    bright: Color::Rgb(205, 214, 244),
    highlight: Color::Rgb(245, 245, 255),
    amber: Color::Rgb(249, 226, 175),
    amber_bright: Color::Rgb(250, 236, 200),
    rec_dim: Color::Rgb(120, 40, 50),
    rec_active: Color::Rgb(243, 139, 168),
    track_colors: &CATPPUCCIN_TRACKS,
    btn_active_bg: Color::Rgb(40, 40, 58),
    btn_focused_bg: Color::Rgb(49, 50, 68),
    btn_default_fg: Color::Rgb(69, 71, 90),
    btn_default_bg: Color::Rgb(24, 24, 37),
};

// ── SpaceVim2 (authentic SpaceVim colorscheme from SpaceVim.vim) ──

const SPACEVIM2_TRACKS: [Color; 8] = [
    Color::Rgb(51, 102, 204),  // keyword blue (68)
    Color::Rgb(0, 153, 102),   // string green (36)
    Color::Rgb(204, 153, 0),   // boolean gold (178)
    Color::Rgb(204, 51, 153),  // function pink (169)
    Color::Rgb(153, 102, 204), // statusline purple (140)
    Color::Rgb(0, 102, 102),   // comment teal (30)
    Color::Rgb(204, 102, 0),   // warning orange (172)
    Color::Rgb(102, 153, 255), // operator blue (111)
];

const SPACEVIM2: Palette = Palette {
    bg: Color::Rgb(38, 38, 38),              // 235
    border: Color::Rgb(48, 48, 48),           // 236
    dim: Color::Rgb(68, 68, 68),              // 238
    muted: Color::Rgb(78, 78, 78),            // 239
    normal: Color::Rgb(178, 178, 178),        // 249 (Normal fg)
    bright: Color::Rgb(204, 204, 204),        // bright text
    highlight: Color::Rgb(230, 230, 230),     // highlight
    amber: Color::Rgb(204, 153, 0),           // 178 (Boolean/StorageClass gold)
    amber_bright: Color::Rgb(230, 180, 30),   // brighter gold
    rec_dim: Color::Rgb(120, 25, 25),
    rec_active: Color::Rgb(204, 0, 0),        // 160 (Error red)
    track_colors: &SPACEVIM2_TRACKS,
    btn_active_bg: Color::Rgb(48, 48, 48),    // 236
    btn_focused_bg: Color::Rgb(58, 58, 58),   // 237
    btn_default_fg: Color::Rgb(68, 68, 68),   // 238
    btn_default_bg: Color::Rgb(28, 28, 28),   // 234
};

// ── Palette accessor ──

fn palette() -> &'static Palette {
    match current_theme() {
        1 => &SPACEVIM,
        2 => &GRUVBOX,
        3 => &MIDNIGHT,
        4 => &DRACULA,
        5 => &NORD,
        6 => &JELLYBEAN,
        7 => &CATPPUCCIN,
        8 => &SPACEVIM2,
        _ => &PHOSPHOR,
    }
}

/// Get the BG color for the active theme (for inline Color::Rgb replacement).
pub fn bg_val() -> Color { palette().bg }
/// Get the DIM color for the active theme.
pub fn dim_color_val() -> Color { palette().dim }
/// Get the AMBER color for the active theme.
pub fn amber_val() -> Color { palette().amber }

// Old const names kept for backward compat — code that uses these directly
// gets the Phosphor theme colors. Style functions above are theme-aware.
#[allow(dead_code)] pub const BG: Color = Color::Rgb(11, 29, 40);
#[allow(dead_code)] pub const BORDER: Color = Color::Rgb(15, 37, 53);
pub const DIM: Color = Color::Rgb(22, 46, 60);
#[allow(dead_code)] pub const MUTED: Color = Color::Rgb(26, 58, 76);
pub const NORMAL: Color = Color::Rgb(37, 78, 98);
#[allow(dead_code)] pub const BRIGHT: Color = Color::Rgb(56, 113, 127);
pub const HIGHLIGHT: Color = Color::Rgb(88, 160, 176);
pub const AMBER: Color = Color::Rgb(114, 88, 14);
pub const AMBER_BRIGHT: Color = Color::Rgb(150, 116, 20);
pub const REC_DIM: Color = Color::Rgb(90, 26, 26);
pub const REC_ACTIVE: Color = Color::Rgb(180, 50, 50);

pub fn track_color(index: usize) -> Color {
    let p = palette();
    p.track_colors[index % p.track_colors.len()]
}

// ── Convenience styles (now theme-aware) ──

pub fn bg() -> Style {
    Style::default().bg(palette().bg)
}

pub fn dim() -> Style {
    let p = palette();
    Style::default().fg(p.dim).bg(p.bg)
}

pub fn muted() -> Style {
    let p = palette();
    Style::default().fg(p.muted).bg(p.bg)
}

pub fn normal() -> Style {
    let p = palette();
    Style::default().fg(p.normal).bg(p.bg)
}

pub fn amber() -> Style {
    let p = palette();
    Style::default().fg(p.amber).bg(p.bg)
}

pub fn amber_bright() -> Style {
    let p = palette();
    Style::default().fg(p.amber_bright).bg(p.bg)
}

pub fn branding() -> Style {
    let p = palette();
    Style::default()
        .fg(p.bright)
        .bg(p.bg)
        .add_modifier(Modifier::BOLD)
}

pub fn border_style() -> Style {
    let p = palette();
    Style::default().fg(p.border).bg(p.bg)
}

// ── Derived colors for UI elements ──

/// Piano roll row backgrounds (cursor, black key, white key).
pub fn piano_cursor_bg() -> Color { lighten(palette().bg, 15) }
pub fn piano_black_bg() -> Color { lighten(palette().bg, 2) }
pub fn piano_white_bg() -> Color { lighten(palette().bg, 5) }

/// Column highlight background.
pub fn col_highlight_bg() -> Color { lighten(palette().bg, 8) }
/// Row+column intersection.
pub fn col_row_bg() -> Color { lighten(palette().bg, 18) }
/// Highlight selection (Shift+h/l).
pub fn selection_bg() -> Color {
    let p = palette();
    // Warm tint of amber on bg
    blend(p.bg, p.amber, 30)
}
/// Selection + cursor overlap.
pub fn selection_cursor_bg() -> Color {
    let p = palette();
    blend(p.bg, p.amber, 45)
}

/// Gridline colors (major / minor).
pub fn grid_major() -> Color { lighten(palette().bg, 10) }
pub fn grid_minor() -> Color { lighten(palette().bg, 6) }

/// Modal/overlay background.
pub fn overlay_bg() -> Color { lighten(palette().bg, 4) }

/// Transport highlight background.
pub fn transport_hi_bg() -> Color { lighten(palette().bg, 12) }

/// Solo active style.
pub fn solo_active_fg() -> Color { Color::Rgb(84, 148, 46) }
pub fn solo_active_bg() -> Color { lighten(palette().bg, 6) }
pub fn solo_focused_bg() -> Color { lighten(palette().bg, 10) }

/// Playhead style.
pub fn playhead_fg() -> Color { Color::Rgb(255, 200, 50) }
pub fn playhead_bg() -> Color { blend(palette().bg, Color::Rgb(255, 200, 50), 20) }

/// Helper: lighten a color by adding a flat amount to each channel.
fn lighten(c: Color, amt: u8) -> Color {
    Color::Rgb(
        tc_r(c).saturating_add(amt),
        tc_g(c).saturating_add(amt),
        tc_b(c).saturating_add(amt),
    )
}

/// Helper: blend two colors. pct=0 → all c1, pct=100 → all c2.
fn blend(c1: Color, c2: Color, pct: u16) -> Color {
    let r = (tc_r(c1) as u16 * (100 - pct) + tc_r(c2) as u16 * pct) / 100;
    let g = (tc_g(c1) as u16 * (100 - pct) + tc_g(c2) as u16 * pct) / 100;
    let b = (tc_b(c1) as u16 * (100 - pct) + tc_b(c2) as u16 * pct) / 100;
    Color::Rgb(r as u8, g as u8, b as u8)
}

// ── Color helpers ──

/// Dim a color by a percentage (0..100).
pub fn dim_color(c: Color, pct: u16) -> Color {
    Color::Rgb((tc_r(c) as u16*pct/100) as u8, (tc_g(c) as u16*pct/100) as u8, (tc_b(c) as u16*pct/100) as u8)
}

pub fn tc_r(c: Color) -> u8 { if let Color::Rgb(r,_,_) = c { r } else { 128 } }
pub fn tc_g(c: Color) -> u8 { if let Color::Rgb(_,g,_) = c { g } else { 128 } }
pub fn tc_b(c: Color) -> u8 { if let Color::Rgb(_,_,b) = c { b } else { 128 } }

/// Style for track header buttons (fx, volume, mute, etc).
pub fn btn_style(active: bool, focused: bool, tc: Color) -> Style {
    let p = palette();
    if active {
        Style::default().fg(dim_color(tc, 80))
            .bg(if focused { p.btn_focused_bg } else { p.btn_active_bg })
            .add_modifier(Modifier::BOLD)
    } else if focused {
        Style::default().fg(p.normal).bg(p.btn_focused_bg)
    } else {
        Style::default().fg(p.btn_default_fg).bg(p.btn_default_bg)
    }
}
