//! UI rendering: the pad map.
//!
//! # What is on the screen
//!
//! The keyboard, because that is what a pad is: eighty-eight of them, one
//! per key, drawn as a keyboard with a caret over the one being edited. A
//! key with sounds on it is lit in the track's colour and carries the
//! number of layers on its face; a key holding a sound whose file has gone
//! missing is red. Playing a key moves the caret, so on a controller the
//! band is also the selector.
//!
//! Under it, two columns: every filled pad as a row — what it is called,
//! what is on it, how it triggers ([`list`]) — and the panel for the pad
//! under the caret, in the same knobs the step grid uses, with the layer
//! list below it ([`panel`]).
//!
//! Nothing here changes a pad. The keys do that, through the sampler's
//! ops, which are also the only thing that tells the engine.
//!
//! # Fitting
//!
//! The band is six rows — seven in keys mode, where the rule under the
//! keyboard braces each zone — and wants a hundred and eight columns for
//! the whole bed; below that it shows the white keys that fit and scrolls
//! to keep the caret in sight. Below twelve rows it goes entirely; how tall
//! it actually came out is read from what [`band_lines`] returned, so the
//! rule cannot push the panel off the bottom of the pane. The panel is what
//! the keys are typing into, and a view that shows the keyboard and not the
//! control being turned has given up the wrong thing.

use super::*;

use phosphor_app::sampler::knobs::PadKnob;
use phosphor_app::sampler::{
    MapMode, PadRow, PadState, SamplerState, Zone, NUM_PADS, PAD_BASE_NOTE,
};

use super::keyboard::{self, KeyPaint, INK};

mod list;
mod panel;
mod strip;
mod zones;

use list::pad_list;
use panel::panel_lines;
use strip::strip_lines;
use zones::{rule_line, zone_list};

/// The lowest and highest key on the bed.
const LOW: u8 = PAD_BASE_NOTE;
const HIGH: u8 = PAD_BASE_NOTE + 87;

/// The mark a zone's root key wears on the band.
const ROOT_MARK: char = '\u{25C6}';

/// Below this many rows the band is dropped for the panel's sake.
const BAND_MIN_H: usize = 12;

/// Below this many columns a keyboard is too few keys to read as one.
const BAND_MIN_W: usize = 24;

/// The width the filled-pad list takes when there is room for two columns.
const LIST_W: u16 = 38;

/// Below this width the two columns become one.
const WIDE_MIN: u16 = LIST_W + 40;

/// The column the panel's knobs start in, under their heading.
const INDENT: usize = 9;

/// The narrowest the knob panel is drawn at: the heading column plus the
/// widest knob the pad has (`release ◔ 10.00 s`). Below it the panel is one
/// line saying which pad the keys are on — a knob cut in half by the right
/// edge is worse than no knob at all.
const MIN_PANEL_W: usize = INDENT + 20;

/// Everything the sections read, gathered once.
struct Map<'a> {
    state: &'a SamplerState,
    view: &'a SamplerView,
    /// The clip view is focused and this is the tab it is showing.
    focused: bool,
    colour: Color,
    /// Source mode, when this track is in it — the banner's subject.
    source: Option<&'a phosphor_app::sampler::capture::SourceMode>,
    /// The engine's rate. A phrase's length is frames, so the seconds the
    /// list prints are the seconds it will actually play for here.
    rate: f32,
}

impl Map<'_> {
    /// The sound the panel is drawing: the pad under the caret, or the
    /// zone's own in keys mode. `None` only in keys mode, on a key no zone
    /// covers — where the panel says what to press instead of drawing
    /// controls for a sound that is not there.
    fn pad(&self) -> Option<&PadState> {
        self.state.edited()
    }

    /// The zone the caret is standing in, when keys mode is on.
    fn zone(&self) -> Option<&Zone> {
        match self.state.mode {
            MapMode::Keys => self.state.cursor_zone().map(|i| &self.state.zones[i]),
            MapMode::Pads => None,
        }
    }

    /// How many rows the sound list has: the layers and the phrases as one
    /// list, which is what `[`/`]` walks.
    fn row_count(&self) -> usize {
        self.pad().map_or(0, PadState::rows)
    }

    /// The sound the row cursor is on, when there is one.
    fn row(&self) -> Option<PadRow<'_>> {
        self.pad()?.row(self.layer_cursor())
    }

    /// The sound cursor, pulled inside what this pad actually holds. The
    /// keys clamp it too; this is the frame's own guard, because what is
    /// under the cursor can change without a key being pressed — playing a
    /// note moves it.
    fn layer_cursor(&self) -> usize {
        self.view.layer.min(self.row_count().saturating_sub(1))
    }

    fn knobs(&self) -> &'static [PadKnob] {
        PadKnob::visible(self.row().map(PadRow::kind), self.state.mode)
    }

    /// Which control the cursor is on, inside the list this pad offers.
    fn knob_cursor(&self) -> usize {
        self.view.knob.min(self.knobs().len().saturating_sub(1))
    }

    fn heading(&self) -> Style {
        if self.focused {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else {
            theme::dim()
        }
    }
}

// ── Text that has to fit ──

/// A row built column by column, where a column that does not fit is not
/// drawn at all.
///
/// Half a word cut off by the right edge of the pane is worse than a
/// missing column: it reads as a bug rather than as a narrow terminal, and
/// the column it cuts is whichever one happened to be last.
struct Row {
    spans: Vec<Span<'static>>,
    width: usize,
    used: usize,
}

impl Row {
    fn new(width: usize) -> Self {
        Self { spans: Vec::new(), width, used: 0 }
    }

    /// How many cells are still free.
    fn left(&self) -> usize {
        self.width.saturating_sub(self.used)
    }

    fn push(&mut self, text: impl Into<String>, style: Style) {
        let text = text.into();
        let cells = text.chars().count();
        if cells > self.left() {
            return;
        }
        self.used += cells;
        self.spans.push(Span::styled(text, style));
    }

    /// The first of these that fits, widest first.
    ///
    /// For the marks that say something is wrong: `no instrument` in a wide
    /// pane, `!` in a narrow one. [`Row::push`] drops what does not fit,
    /// which is right for a column and wrong for a warning — a warning that
    /// vanishes at 57 columns is a warning nobody gets.
    fn push_widest(&mut self, options: &[&str], style: Style) {
        if let Some(text) = options.iter().find(|t| t.chars().count() <= self.left()) {
            self.push(*text, style);
        }
    }

    fn line(self) -> Line<'static> {
        Line::from(self.spans)
    }
}

/// Cut a name to the room it has, with an ellipsis where it was cut.
fn clip_text(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    text.chars().take(width.saturating_sub(1)).chain(['\u{2026}']).collect()
}

// ── The band ──

/// The caret over the key being edited, the keyboard under it, and — in
/// keys mode — the rule that braces each zone.
fn band_lines(map: &Map, width: usize) -> Vec<Line<'static>> {
    let cursor_note = SamplerState::note_of_pad(map.state.cursor);
    let lo = keyboard::window_lo(LOW, HIGH, width, cursor_note);

    let mut lines = vec![caret_line(map, lo, cursor_note, width)];
    lines.extend(keyboard::band(lo, HIGH, width, |note| key_paint(map, note)));
    // The rule only exists where zones do. In pads mode the row would be
    // air, and a band that changes height for nothing is a screen that
    // jumps when the mode does.
    if map.state.mode == MapMode::Keys {
        lines.push(rule_line(map, lo, width));
    }
    lines
}

/// How one key on the bed is painted.
///
/// The same rule in both modes, because it is the same question: what does
/// this key play, is anything it plays missing, and is the caret on it. In
/// keys mode the answer comes from the zones standing over the key rather
/// than from the pad under it — see
/// [`SamplerState::key_load`](phosphor_app::sampler::SamplerState::key_load).
fn key_paint(map: &Map, note: u8) -> KeyPaint {
    let Some(index) = SamplerState::pad_of_note(note) else {
        return KeyPaint::plain(note);
    };
    let (count, missing) = map.state.key_load(index);
    let paint = if index == map.state.cursor {
        KeyPaint::lit(theme::amber_bright_val())
    } else if missing {
        KeyPaint::lit(theme::rec_active_val())
    } else if count > 0 {
        KeyPaint::lit(map.colour)
    } else {
        return KeyPaint::plain(note);
    };
    // A zone's root wears the anchor rather than the count: the count is on
    // every other key of the zone, and which key the sound was recorded at
    // is the thing that cannot be read anywhere else on the band.
    if map.state.is_zone_root(index) {
        return paint.with_glyph(ROOT_MARK, INK);
    }
    // The number of sounds stacked on the key, on the key. Eight layers and
    // four phrases is twelve, which no digit can say — so past nine the key
    // wears a `+` and the list beside it gives the count. A truncated digit
    // would be a lie about a kit the player built.
    match count {
        0 => paint,
        1..=9 => match char::from_digit(count as u32, 10) {
            Some(digit) => paint.with_glyph(digit, INK),
            None => paint,
        },
        _ => paint.with_glyph('+', INK),
    }
}

fn caret_line(map: &Map, lo: u8, cursor_note: u8, width: usize) -> Line<'static> {
    let column = keyboard::column_of(lo, cursor_note);
    if column >= width {
        return Line::from(Span::styled(" ", theme::bg()));
    }
    Line::from(vec![
        Span::styled(" ".repeat(column), theme::bg()),
        Span::styled(
            "\u{25BC}",
            if map.focused { theme::amber_bright() } else { theme::dim() },
        ),
    ])
}

// ── The banner ──

/// The one line that says the track is not playing its sampler.
///
/// It is worth a whole row of the pane because the mode changes what the
/// keys do, what the track sounds like, and what a note played on the
/// controller *is* — and a player who cannot see it on is a player whose
/// pads have gone quiet for no visible reason.
fn source_banner(map: &Map, width: usize) -> Option<Line<'static>> {
    let mode = map.source?;
    let red = Style::default().fg(theme::rec_active_val()).bg(theme::bg_val());
    let pad = SamplerState::pad_label(mode.pad);
    // What `r` will land, always on the banner and never only in a flash:
    // it is the difference between a pad that costs megabytes and one that
    // costs an instrument, and a player who cannot see which is armed finds
    // out after the performance.
    let take = format!("take: {}", mode.take.label());
    let (mark, text, style) = match mode.capture.as_ref() {
        None => (
            "\u{25B8}",
            format!(
                "source \u{00b7} {} \u{00b7} pad {pad} \u{00b7} {take} \u{00b7} p swaps \u{00b7} r records \u{00b7} esc puts the sampler back",
                mode.instrument.label(),
            ),
            theme::amber_bright(),
        ),
        Some(capture) if !capture.open_at(phosphor_midi::clock::now_micros()) => (
            "\u{25CF}",
            format!("armed \u{00b7} pad {pad} \u{00b7} {take} \u{00b7} it opens at the next bar line"),
            red,
        ),
        Some(capture) => (
            "\u{25CF}",
            format!(
                "recording \u{00b7} pad {pad} \u{00b7} {take} \u{00b7} {} note{} \u{00b7} {} \u{00b7} r ends it",
                capture.note_count(),
                if capture.note_count() == 1 { "" } else { "s" },
                if capture.is_bars() { "whole bars" } else { "free" },
            ),
            red,
        ),
    };
    let mut row = Row::new(width);
    row.push(format!(" {mark} "), style);
    row.push(clip_text(&text, width.saturating_sub(3)), style);
    Some(row.line())
}

// ── The view ──

pub(super) fn render_pads(frame: &mut Frame, area: Rect, nav: &NavState) {
    let (width, height) = (area.width as usize, area.height as usize);
    if width == 0 || height == 0 {
        return;
    }
    let Some(track) = nav.current_track() else { return };
    let Some(state) = track.sampler.as_deref() else {
        frame.render_widget(
            Paragraph::new(Span::styled("  no sampler on this track", theme::dim())),
            area,
        );
        return;
    };

    let map = Map {
        state,
        view: &nav.clip_view.sampler,
        focused: nav.focused_pane == Pane::ClipView
            && nav.clip_view.focus == ClipViewFocus::PianoRoll
            && nav.clip_view.clip_tab == ClipTab::Pads,
        colour: theme::track_color(track.color_index),
        source: nav
            .sampler_source
            .as_deref()
            .filter(|mode| mode.track_idx == nav.track_cursor),
        rate: nav.sample_rate as f32,
    };

    let mut body = area;
    if height >= BAND_MIN_H && width >= BAND_MIN_W {
        let lines = band_lines(&map, width);
        let rows = (lines.len() as u16).min(area.height);
        let mut top = area;
        top.height = rows;
        frame.render_widget(Paragraph::new(lines), top);
        body.y += rows;
        body.height -= rows;
    }
    if body.height == 0 {
        return;
    }

    // The banner takes the row under the keyboard, above everything else:
    // it is the answer to "why is nothing on the pads making a sound".
    if let Some(line) = source_banner(&map, body.width as usize) {
        let mut row = body;
        row.height = 1;
        frame.render_widget(Paragraph::new(vec![line]), row);
        body.y += 1;
        body.height -= 1;
        if body.height == 0 {
            return;
        }
    }

    // The trim strip takes the body and leaves the band. It is worth the
    // whole width — a waveform in half a pane is half a waveform — and the
    // keyboard above it stays because a player who has lost track of which
    // pad they are trimming has lost the plot entirely.
    if let Some(lines) = strip_lines(&map, body.width as usize, body.height as usize) {
        frame.render_widget(Paragraph::new(lines), body);
        return;
    }

    if body.width >= WIDE_MIN {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(LIST_W), Constraint::Min(20)])
            .split(body);
        frame.render_widget(
            Paragraph::new(side_list(&map, cols[0].width as usize, cols[0].height as usize)),
            cols[0],
        );
        frame.render_widget(
            Paragraph::new(panel_lines(&map, cols[1].width as usize, cols[1].height as usize)),
            cols[1],
        );
        return;
    }

    // One column: the panel first, because it is what the keys are typing
    // into, and the pad list under it with whatever is left. Two rows are
    // kept back for the list — its heading and the pad under the caret —
    // so that a narrow terminal still says what is on the kit. The panel
    // scrolls to the control being turned, so walking down to a layer's
    // knobs brings the layer list into view with them.
    let body_h = body.height as usize;
    let body_w = body.width as usize;
    let mut lines = panel_lines(&map, body_w, body_h.saturating_sub(2).max(1));
    let left = body_h.saturating_sub(lines.len());
    if left > 1 {
        lines.extend(side_list(&map, body_w, left));
    }
    lines.truncate(body_h);
    frame.render_widget(Paragraph::new(lines), body);
}

/// The column beside the panel: what is on the kit, in whichever unit the
/// mode is in. Pads have a pad list; zones have a zone list, because in
/// keys mode a list of eighty-eight keys would be a list of the same sound
/// twenty-four times.
fn side_list(map: &Map, width: usize, height: usize) -> Vec<Line<'static>> {
    match map.state.mode {
        MapMode::Pads => pad_list(map, width, height),
        MapMode::Keys => zone_list(map, width, height),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use phosphor_plugin::sample::SamplePcm;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// A kit with two sounds on C3, the second of which has lost its file.
    /// One second of audio, so a length reads as `1.00s`.
    pub(in crate::ui::sampler) fn kit() -> SamplerState {
        let mut state = SamplerState::new();
        let pcm = Arc::new(SamplePcm {
            data: vec![0.0; 44_100],
            channels: 1,
            sample_rate: 44_100.0,
        });
        let pad = SamplerState::pad_of_note(60).unwrap();
        state.add_wav_layer(pad, PathBuf::from("kick.wav"), Arc::clone(&pcm)).unwrap();
        state.add_wav_layer(pad, PathBuf::from("clap.wav"), pcm).unwrap();
        state.pads[pad].layers[1].pcm = None;
        state.cursor = pad;
        state
    }

    /// The same kit in keys mode, with one zone over C2-B3 rooted at C3 —
    /// two octaves of a piano, which is what a zone is for.
    pub(in crate::ui::sampler) fn zoned() -> SamplerState {
        let mut state = kit();
        state.mode = phosphor_app::sampler::MapMode::Keys;
        let pcm = Arc::new(SamplePcm {
            data: vec![0.0; 44_100],
            channels: 1,
            sample_rate: 44_100.0,
        });
        let mut pad = PadState::empty(60);
        pad.add_wav(PathBuf::from("piano.wav"), pcm, "zone").unwrap();
        state.zones.push(phosphor_app::sampler::Zone::new(
            SamplerState::pad_of_note(48).unwrap(),
            SamplerState::pad_of_note(71).unwrap(),
            pad,
        ));
        state.cursor = SamplerState::pad_of_note(60).unwrap();
        state
    }

    pub(in crate::ui::sampler) fn map<'a>(
        state: &'a SamplerState,
        view: &'a SamplerView,
    ) -> Map<'a> {
        Map {
            state,
            view,
            focused: true,
            colour: theme::track_color(0),
            source: None,
            rate: 44_100.0,
        }
    }

    pub(in crate::ui::sampler) fn text(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.to_string()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn cells(line: &Line<'static>) -> usize {
        line.spans.iter().map(|s| s.content.chars().count()).sum()
    }

    /// No row of any section runs past the width it was given — the defect
    /// that cuts a knob in half at the right edge of the pane.
    #[test]
    fn nothing_runs_past_the_right_edge() {
        // Both modes, because keys mode draws two sections the pad map does
        // not — the rule under the band and the zone list — and a row one
        // cell too long is cut in half by the edge of the pane either way.
        for state in [kit(), zoned()] {
            let view = SamplerView::new();
            let map = map(&state, &view);
            for width in [1usize, 8, 20, 29, 38, 57, 95, 200] {
                for line in band_lines(&map, width) {
                    assert!(cells(&line) <= width.max(1), "band row in {width} columns");
                }
                for line in side_list(&map, width, 10) {
                    assert!(
                        cells(&line) <= width,
                        "a {}-cell list row in {width} columns: {:?}",
                        cells(&line),
                        text(&[line.clone()]),
                    );
                }
                for line in panel_lines(&map, width, 20) {
                    assert!(
                        cells(&line) <= width.max(MIN_PANEL_W),
                        "a {}-cell panel row in {width} columns: {:?}",
                        cells(&line),
                        text(&[line.clone()]),
                    );
                }
            }
        }
    }

    /// Keys mode costs one row more than pads mode and no more than that,
    /// and the row it costs is the rule.
    #[test]
    fn the_rule_is_the_only_row_keys_mode_adds() {
        let view = SamplerView::new();
        let pads = band_lines(&map(&kit(), &view), 120).len();
        let keys = zoned();
        assert_eq!(band_lines(&map(&keys, &view), 120).len(), pads + 1);
    }

    /// The caret is over the key it names, wherever the band has scrolled
    /// to — including both ends of the bed, where the window is hard
    /// against the wall.
    #[test]
    fn the_caret_sits_over_the_pad_it_names() {
        let mut state = kit();
        let view = SamplerView::new();
        for note in [21u8, 60, 61, 84, 108] {
            state.cursor = SamplerState::pad_of_note(note).unwrap();
            let map = map(&state, &view);
            for width in [30usize, 60, 108, 120] {
                let lines = band_lines(&map, width);
                let caret = cells(&lines[0]).saturating_sub(1);
                let lo = keyboard::window_lo(LOW, HIGH, width, note);
                assert_eq!(
                    caret,
                    keyboard::column_of(lo, note),
                    "note {note} at width {width}",
                );
                assert!(caret < width, "the caret is off the right edge");
            }
        }
    }

    /// The band paints what is on the bed: the pad under the caret in the
    /// theme's amber, a pad with a missing file in red, a filled pad in the
    /// track's colour, and an empty one in neither.
    #[test]
    fn the_band_paints_what_is_on_the_bed() {
        let mut state = kit();
        // A second pad, filled and whole, and a third left empty.
        let pcm =
            Arc::new(SamplePcm { data: vec![0.0; 10], channels: 1, sample_rate: 44_100.0 });
        let filled = SamplerState::pad_of_note(64).unwrap();
        state.add_wav_layer(filled, PathBuf::from("hat.wav"), pcm).unwrap();
        let view = SamplerView::new();

        // The last of the five rows is white-key body all the way across,
        // so a white key's colour is read there.
        fn colour_at(state: &SamplerState, view: &SamplerView, note: u8) -> Color {
            let lines = band_lines(&map(state, view), 200);
            let lo = keyboard::window_lo(LOW, HIGH, 200, note);
            let column = keyboard::column_of(lo, note);
            let mut seen = 0usize;
            for span in &lines[keyboard::ROWS].spans {
                let cells = span.content.chars().count();
                if seen + cells > column {
                    return span.style.bg.expect("a key with no colour");
                }
                seen += cells;
            }
            panic!("note {note} is not on the band");
        }

        assert_eq!(
            colour_at(&state, &view, 60),
            theme::amber_bright_val(),
            "the caret pad is not lit",
        );
        // With the caret elsewhere, the same pad shows what is wrong with it.
        state.cursor = 0;
        assert_eq!(
            colour_at(&state, &view, 60),
            theme::rec_active_val(),
            "a missing file is not red",
        );
        assert_eq!(
            colour_at(&state, &view, 64),
            theme::track_color(0),
            "a filled pad is not lit",
        );
        assert_eq!(
            colour_at(&state, &view, 65),
            theme::piano_white_bg(),
            "an empty pad is lit",
        );
    }
}
