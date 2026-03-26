//! UI rendering — phosphor solarized-dark aesthetic.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use phosphor_core::project::TrackKind;
use phosphor_core::transport::{self, Transport, TransportSnapshot};

use crate::state::*;
use crate::theme;

mod bottom_bar;
use bottom_bar::*;
mod clip_view;
use clip_view::*;
mod overlays;
use overlays::*;
mod top_bar;
use top_bar::*;
mod tracks;
use tracks::*;

const HEADER_W: u16 = 12;
const TRACK_H: u16 = 3;
const VISIBLE_BARS: usize = 16;
const FX_PANEL_W: u16 = 24;

/// Rendering context for a single track row. Bundles all the per-track
/// state needed by render_header and render_clips so we don't pass 9 args.
struct TrackCtx<'a> {
    track: &'a TrackState,
    index: usize,
    is_cursor: bool,
    is_selected: bool,
    is_dimmed: bool,
    vu_level: f32,
    nav: &'a NavState,
}

pub fn render(
    frame: &mut Frame,
    transport: &TransportSnapshot,
    nav: &NavState,
) {
    let area = frame.area();
    crate::debug_log::log("RENDER", &format!(
        "area={}x{} tracks={} clip_view={} pane={:?}",
        area.width, area.height, nav.tracks.len(), nav.clip_view_visible, nav.focused_pane,
    ));
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(theme::bg()), area);

    let actual_track_count = nav.visible_tracks().len().min(MAX_VISIBLE_TRACKS);
    let tracks_h = (actual_track_count as u16) * TRACK_H;

    let mut constraints = vec![
        Constraint::Length(1),       // top bar (buffer 1)
        Constraint::Length(1),       // ruler
        Constraint::Length(tracks_h), // tracks (buffer 2) — sized to actual content
    ];

    if nav.clip_view_visible {
        constraints.push(Constraint::Length(1)); // clip view tabs (buffer 3 header)
        constraints.push(Constraint::Min(8));    // clip view content (buffer 3)
    } else {
        constraints.push(Constraint::Min(0));
    }

    constraints.push(Constraint::Length(1)); // bottom bar

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut ci = 0;
    render_top_bar(frame, chunks[ci], nav, transport); ci += 1;
    render_ruler(frame, chunks[ci], nav, transport); ci += 1;
    render_tracks(frame, chunks[ci], nav, transport); ci += 1;

    if nav.clip_view_visible {
        render_clip_view_tabs(frame, chunks[ci], nav); ci += 1;
        render_clip_view(frame, chunks[ci], nav, transport); ci += 1;
    } else {
        ci += 1;
    }
    render_bottom_bar(frame, chunks[ci], nav);

    // Overlays
    if nav.confirm_modal.open {
        render_confirm_modal(frame, nav);
    } else if nav.input_modal.open {
        render_input_modal(frame, nav);
    } else if nav.instrument_modal.open {
        render_instrument_modal(frame, nav);
    } else if nav.space_menu.open {
        render_space_menu(frame, nav);
    } else if nav.fx_menu.open {
        render_fx_menu(frame, nav);
    }
}

// ── Top Bar ──

