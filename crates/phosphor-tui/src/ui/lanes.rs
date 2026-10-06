//! UI rendering: the track lanes — clips on a window of the song.
//!
//! A clip is a tinted capsule two rows tall: a bright edge where it starts,
//! its number, and a miniature of its notes in braille, eight dots of pitch
//! by two of time per cell, so a bass line reads as a bass line and a drum
//! part as a drum part before anything is opened. The row under it is the
//! lane's floor: the track's name and the line between tracks.
//!
//! Everything is placed through the [`TimelineView`] the ruler also uses, so
//! the lanes, the ruler, the playhead and the loop brace can never disagree
//! about where a tick is.

use super::*;
use phosphor_app::timeline::TICKS_PER_BAR;

/// The window the lanes and the ruler draw: the player's view, turned to the
/// playhead's page while playing past its edge.
pub(super) fn lane_view(nav: &NavState, snap: &TransportSnapshot) -> TimelineView {
    nav.timeline.following(snap.position_ticks, snap.playing)
}

/// The columns `start..end` covers across `width`, at least one wide so the
/// smallest thing still shows, clipped to the screen. `None` when it is off
/// screen entirely.
pub(super) fn span_columns(view: &TimelineView, start: i64, end: i64, width: usize) -> Option<(usize, usize)> {
    if width == 0 || !view.shows(start, end) {
        return None;
    }
    let a = view.column(start, width).clamp(0, width as i64 - 1);
    let b = view.column(end, width).max(a + 1).min(width as i64);
    Some((a as usize, b as usize))
}

/// Whether the brace is something to show: while it is being edited, or
/// while the song is looping on it.
pub(super) fn brace_shown(nav: &NavState) -> bool {
    nav.loop_editor.active || nav.loop_editor.enabled
}

pub(super) fn render_clips(frame: &mut Frame, area: Rect, ctx: &TrackCtx, snap: &TransportSnapshot) {
    let TrackCtx { track, is_selected: sel, is_dimmed: dim, nav, .. } = *ctx;
    let tc = theme::track_color(track.color_index);
    let (w, h) = (area.width as usize, area.height as usize);
    if w == 0 || h == 0 {
        return;
    }
    let view = lane_view(nav, snap);
    let mut grid: Vec<Vec<(char, Style)>> = vec![vec![(' ', theme::bg()); w]; h];

    draw_gridlines(&mut grid, &view, w);

    // The brace, while it is being edited: a band down the lane, so what a
    // `y` will lift is visible on every track at once, however small.
    if nav.loop_editor.active {
        if let Some((a, b)) = span_columns(&view, nav.loop_editor.start, nav.loop_editor.end, w) {
            for row in &mut grid {
                for cell in &mut row[a..b] {
                    cell.1 = cell.1.bg(theme::loop_band());
                }
            }
        }
    }

    let body_rows = h.saturating_sub(1).max(1);
    for (ci, clip) in track.clips.iter().enumerate() {
        let focused = sel && matches!(nav.track_element, TrackElement::Clip(i) if i == ci);
        draw_clip(&mut grid, &view, clip, ClipLook::new(tc, focused, dim, clip.has_content), body_rows);
    }

    if h > 1 {
        draw_floor(&mut grid, &view, track, tc, dim, w);
    }

    // Playhead
    if snap.playing {
        let px = view.column(snap.position_ticks, w);
        if (0..w as i64).contains(&px) {
            for row in &mut grid {
                let cell = &mut row[px as usize];
                let bg = cell.1.bg.unwrap_or(theme::bg_val());
                *cell = ('\u{2502}', Style::default().fg(theme::amber_val()).bg(bg));
            }
        }
    }

    frame.render_widget(Paragraph::new(grid_to_lines(grid)), area);
}

/// Bar lines, a heavier one every four bars, and beat lines once the lanes
/// are zoomed in far enough for beats to be worth telling apart.
fn draw_gridlines(grid: &mut [Vec<(char, Style)>], view: &TimelineView, w: usize) {
    let beat = phosphor_core::transport::Transport::PPQ;
    let cells_per_beat = w as i64 * beat / view.ticks();
    let step = if cells_per_beat >= 4 { beat } else { TICKS_PER_BAR };
    let mut tick = view.first_tick();
    while tick < view.end_tick() {
        let x = view.column(tick, w);
        if x > 0 && (x as usize) < w {
            let bar = tick % TICKS_PER_BAR == 0;
            let major = bar && (tick / TICKS_PER_BAR) % 4 == 0;
            let (ch, fg) = match (major, bar) {
                (true, _) => ('\u{2502}', theme::grid_major()),
                (false, true) => ('\u{2506}', theme::grid_minor()),
                _ => ('\u{250A}', theme::grid_minor()),
            };
            for row in grid.iter_mut() {
                row[x as usize] = (ch, Style::default().fg(fg).bg(theme::bg_val()));
            }
        }
        tick += step;
    }
}

/// The colours a clip is drawn in, worked out once per clip.
struct ClipLook {
    /// The capsule's fill.
    fill: Color,
    /// The edge where the clip starts.
    edge: Color,
    /// The clip's number.
    label: Style,
    /// The notes in the miniature.
    ink: Color,
}

impl ClipLook {
    fn new(tc: Color, focused: bool, dim: bool, has_notes: bool) -> Self {
        let fill_pct = match (focused, has_notes) {
            (true, _) => 26,
            (false, true) => 13,
            (false, false) => 6,
        };
        let fill = theme::clip_tint(tc, if dim { fill_pct / 2 } else { fill_pct });
        let edge = if dim {
            theme::dim_color(tc, 30)
        } else if focused {
            tc
        } else {
            theme::dim_color(tc, 75)
        };
        let label = if focused {
            Style::default().fg(theme::amber_bright_val()).bg(fill).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(if dim { theme::dim_color(tc, 35) } else { theme::dim_color(tc, 85) }).bg(fill)
        };
        let ink = if dim {
            theme::dim_color(tc, 40)
        } else if focused {
            tc
        } else {
            theme::dim_color(tc, 90)
        };
        Self { fill, edge, label, ink }
    }
}

fn draw_clip(grid: &mut [Vec<(char, Style)>], view: &TimelineView, clip: &Clip, look: ClipLook, rows: usize) {
    let w = grid.first().map_or(0, Vec::len);
    let end = clip.start_tick + clip.length_ticks;
    let Some((a, b)) = span_columns(view, clip.start_tick, end, w) else { return };
    let starts_on_screen = view.column(clip.start_tick, w) >= 0;

    let fill = Style::default().fg(look.ink).bg(look.fill);
    for row in grid.iter_mut().take(rows) {
        for cell in &mut row[a..b] {
            *cell = (' ', fill);
        }
    }

    let dots = note_dots(view, clip, w, rows);
    for (r, row) in grid.iter_mut().take(rows).enumerate() {
        for (x, cell) in row.iter_mut().enumerate().take(b).skip(a) {
            let bits = dots[r][x];
            if bits != 0 {
                *cell = (char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' '), fill);
            }
        }
    }

    // The start edge and the number go on last, over the miniature: they are
    // what tells one clip from the next when two sit end to end.
    if starts_on_screen {
        for row in grid.iter_mut().take(rows) {
            row[a] = ('\u{258E}', Style::default().fg(look.edge).bg(look.fill));
        }
        let number = clip.number.to_string();
        if b - a > number.len() + 1 {
            for (i, ch) in number.chars().enumerate() {
                grid[0][a + 1 + i] = (ch, look.label);
            }
        }
    }
}

/// The braille dots of a clip's notes: for each body row and screen column,
/// the eight-dot pattern to draw there. Pitch runs up the rows — the clip's
/// own lowest note at the bottom, its highest at the top — and time across,
/// two dots to a cell.
fn note_dots(view: &TimelineView, clip: &Clip, w: usize, rows: usize) -> Vec<Vec<u8>> {
    let mut dots = vec![vec![0u8; w]; rows];
    let visible: Vec<&phosphor_core::clip::NoteSnapshot> =
        clip.notes.iter().filter(|n| n.start_tick < clip.length_ticks).collect();
    let (Some(low), Some(high)) =
        (visible.iter().map(|n| n.note).min(), visible.iter().map(|n| n.note).max())
    else {
        return dots;
    };
    let levels = rows * 4;
    let span = usize::from(high - low);
    // A clip that uses fewer pitches than there are dot rows is centred
    // rather than stretched, so a one-note drum part sits on one line.
    let pad = levels.saturating_sub(span + 1) / 2;
    for note in visible {
        let from = usize::from(note.note - low);
        let level = if span < levels { pad + from } else { from * (levels - 1) / span.max(1) };
        let dot_row = levels - 1 - level.min(levels - 1);
        let start = clip.start_tick + note.start_tick;
        let stop = clip.start_tick + (note.start_tick + note.duration_ticks).min(clip.length_ticks);
        // Half-cell time resolution; a gap before the next note so repeated
        // hits read as separate dots, not one line.
        let x0 = view.column(start, w * 2);
        let x1 = (view.column(stop, w * 2) - 1).max(x0 + 1);
        for half in x0..x1 {
            if !(0..(w * 2) as i64).contains(&half) {
                continue;
            }
            let (cell, right) = ((half / 2) as usize, half % 2 == 1);
            let (r, sub) = (dot_row / 4, dot_row % 4);
            dots[r][cell] |= braille_bit(sub, right);
        }
    }
    dots
}

/// The bit for one dot of a braille cell: `row` 0-3 from the top, left or
/// right column.
fn braille_bit(row: usize, right: bool) -> u8 {
    match (row, right) {
        (0, false) => 0x01,
        (1, false) => 0x02,
        (2, false) => 0x04,
        (3, false) => 0x40,
        (0, true) => 0x08,
        (1, true) => 0x10,
        (2, true) => 0x20,
        _ => 0x80,
    }
}

/// The lane's last row: the line between tracks, a soft lip under every
/// clip, and the track's name at the left.
fn draw_floor(grid: &mut [Vec<(char, Style)>], view: &TimelineView, track: &TrackState, tc: Color, dim: bool, w: usize) {
    let last = grid.len() - 1;
    let line = theme::border_style();
    for cell in &mut grid[last] {
        *cell = ('\u{2500}', line);
    }
    for clip in &track.clips {
        if let Some((a, b)) = span_columns(view, clip.start_tick, clip.start_tick + clip.length_ticks, w) {
            let lip = theme::clip_tint(tc, if dim { 6 } else { 13 });
            for cell in &mut grid[last][a..b] {
                *cell = ('\u{2594}', Style::default().fg(lip).bg(theme::bg_val()));
            }
        }
    }

    // A send bus is named by what is in it: `rvb` reads as the thing the
    // player is sending to, where `snd a` reads as a routing matrix. An
    // empty bus keeps its letter — see `phosphor_app::fx::bus_label`.
    let name = match track.send_slot() {
        Some(slot) => phosphor_app::fx::bus_label(&track.fx_chain, slot).to_string(),
        None => track.name.to_lowercase(),
    };
    let name_s = Style::default()
        .fg(if dim { theme::dim_color(tc, 30) } else { theme::dim_color(tc, 65) })
        .bg(theme::bg_val());
    grid[last][0] = (' ', theme::bg());
    for (i, ch) in name.chars().enumerate().take(w.saturating_sub(2)) {
        grid[last][i + 1] = (ch, name_s);
    }
    if let Some(cell) = grid[last].get_mut(name.chars().count() + 1) {
        *cell = (' ', theme::bg());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip_of(notes: &[(u8, i64, i64)], length: i64) -> Clip {
        let notes = notes
            .iter()
            .map(|&(note, start_tick, duration_ticks)| phosphor_core::clip::NoteSnapshot {
                note,
                velocity: 100,
                start_tick,
                duration_ticks,
                muted: false,
            })
            .collect();
        Clip::of_notes(0, length, notes)
    }

    /// A rising line of notes draws as dots that rise: the lowest note on the
    /// bottom dot row, the highest on the top one.
    #[test]
    fn the_miniature_puts_low_notes_low_and_high_notes_high() {
        let view = TimelineView { first_bar: 0, bars: 1 };
        let beat = phosphor_core::transport::Transport::PPQ;
        let clip = clip_of(&[(40, 0, beat / 2), (80, 3 * beat, beat / 2)], TICKS_PER_BAR);
        let dots = note_dots(&view, &clip, 16, 2);
        // First beat, bottom row's bottom dots; last beat, top row's top dots.
        assert!(dots[1][0] & 0x40 != 0, "the low note is not on the bottom: {:?}", dots);
        assert!(dots[0][12] & 0x01 != 0, "the high note is not on the top: {:?}", dots);
        assert_eq!(dots[0][0], 0);
        assert_eq!(dots[1][12], 0);
    }

    /// One pitch played four times is four separate marks on one line, not
    /// a single bar — a drum part has to read as hits.
    #[test]
    fn repeated_hits_stay_separate() {
        let view = TimelineView { first_bar: 0, bars: 1 };
        let beat = phosphor_core::transport::Transport::PPQ;
        let hits: Vec<(u8, i64, i64)> = (0..4).map(|i| (36, i * beat, beat)).collect();
        let clip = clip_of(&hits, TICKS_PER_BAR);
        let dots = note_dots(&view, &clip, 8, 2);
        // Each beat is two cells; the hit lights the first and the left half
        // of the second, and leaves the right half dark before the next.
        const RIGHT: u8 = 0x08 | 0x10 | 0x20 | 0x80;
        for beat in 0..4 {
            assert!(dots[0][2 * beat] | dots[1][2 * beat] != 0, "hit {beat} is missing");
            assert_eq!((dots[0][2 * beat + 1] | dots[1][2 * beat + 1]) & RIGHT, 0, "hit {beat} ran into the next");
        }
    }

    #[test]
    fn a_tiny_span_still_takes_a_column_and_off_screen_takes_none() {
        let view = TimelineView::default();
        assert_eq!(span_columns(&view, 0, 1, 160), Some((0, 1)));
        assert_eq!(span_columns(&view, 20 * TICKS_PER_BAR, 21 * TICKS_PER_BAR, 160), None);
        let scrolled = TimelineView { first_bar: 20, bars: 16 };
        assert_eq!(span_columns(&scrolled, 20 * TICKS_PER_BAR, 21 * TICKS_PER_BAR, 160), Some((0, 10)));
    }
}
