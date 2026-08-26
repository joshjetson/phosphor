//! UI rendering: the step grid.
//!
//! # What is on the screen
//!
//! A drum machine's face. Eight lanes, one row each, sixteen buttons across —
//! the kick above the snare above the hats — with the step numbers over the
//! top and a light that chases across every lane as the pattern plays.
//!
//! Below it, three bands of controls that `j`/`k` walks down into:
//!
//! * **step** — what the step under the cursor plays: one pitch control,
//!   chord, voicing, gate. On a kit it is the lane's panel instead, because a
//!   drum step says only *when*;
//! * **pattern** — the child instrument, length, rate, swing, the velocities,
//!   mode and key;
//! * **slots** — the eight patterns, what is queued, and the chain.
//!
//! Everything here reads. Not one function in this file changes a pattern —
//! the keys do that, and only by naming a [`SeqOp`](phosphor_app::sequencer::ops::SeqOp).
//!
//! # What this replaced, and why
//!
//! One lane at a time, with the other seven behind it as dimmed ghosts and a
//! one-glyph-per-step map underneath. The first person to use it wrote one
//! step and watched three marks appear — the step, its ghost, and its dot on
//! the map — and could not tell which of them they had asked for. Lanes as
//! rows are what a step sequencer looks like; a hit is now exactly one mark,
//! on the row named after the sound it plays.
//!
//! # Fitting
//!
//! Eight rows of lanes and a ruler is eleven of the terminal's lines, and an
//! eighty-by-twenty-four terminal with four tracks on it has eight. So the
//! sections are laid out in priority order: the lanes and the band with the
//! cursor on it are never dropped, and below eleven rows the grid falls back
//! to the lane being written with the sound strip above it standing in for
//! the rows. The steps themselves never shrink below two columns — a pattern
//! is paged rather than squeezed.

use super::*;

use phosphor_app::sequencer::{chords, SequencerState, DEFAULT_DRUM_LABELS};
use phosphor_core::pattern::{
    Chord, Lane, Mode, PatternBlock, Rate, Step, SwitchQuant, Voicing, LANES, MAX_STEPS, SLOTS,
    STEP_COUNTS,
};

/// The dial. Five positions is what one cell can say honestly, and the value
/// is printed beside it, so the glyph is for reading the panel at a glance
/// rather than for reading the number off.
fn knob_char(frac: f64) -> char {
    const RAMP: [char; 5] = ['\u{25CB}', '\u{25D4}', '\u{25D1}', '\u{25D5}', '\u{25CF}'];
    let index = (frac.clamp(0.0, 1.0) * 4.0).round() as usize;
    RAMP[index.min(4)]
}

/// One control on a panel.
struct Knob {
    label: &'static str,
    value: String,
    /// Where the dial is pointing, 0..=1.
    frac: f64,
}

impl Knob {
    fn new(label: &'static str, value: impl Into<String>, frac: f64) -> Self {
        Self { label, value: value.into(), frac }
    }

    /// One of a list, by position — how every discrete control here reads.
    fn at(label: &'static str, value: impl Into<String>, index: usize, count: usize) -> Self {
        let frac = if count > 1 { index as f64 / (count - 1) as f64 } else { 0.0 };
        Self::new(label, value, frac)
    }

    fn toggle(label: &'static str, on: bool) -> Self {
        Self::new(label, if on { "on" } else { "off" }, if on { 1.0 } else { 0.0 })
    }

    /// What [`Seq::knob_spans`] will draw: `" label ◔ value "`.
    ///
    /// Counted rather than measured, and the count has to be exact — a knob
    /// that is one cell wider than the wrapper thinks runs off the right of
    /// the panel, where a `Paragraph` cuts it in half.
    fn width(&self) -> usize {
        self.label.chars().count() + self.value.chars().count() + 5
    }
}

/// The short name of a drum voice, for a lane pinned to one.
///
/// General MIDI percussion, abbreviated to two characters, because the lane
/// strip is eight of these across and `BD SD CH OH` is what the front panel
/// of the machine this is imitating says. Anything outside the map reads as
/// its note number, which is still enough to tell two lanes apart.
fn drum_label(note: u8) -> String {
    const NAMES: [(u8, &str); 26] = [
        (35, "BD"), (36, "BD"), (37, "RS"), (38, "SD"), (39, "CP"), (40, "SD"),
        (41, "LT"), (42, "CH"), (43, "LT"), (44, "PH"), (45, "MT"), (46, "OH"),
        (47, "MT"), (48, "HT"), (49, "CR"), (50, "HT"), (51, "RD"), (52, "CY"),
        (53, "RB"), (54, "TM"), (55, "SP"), (56, "CB"), (57, "CR"), (59, "RD"),
        (60, "BG"), (75, "CL"),
    ];
    NAMES
        .iter()
        .find(|(n, _)| *n == note)
        .map_or_else(|| format!("{note}"), |(_, name)| (*name).to_string())
}

/// What a lane is called on the strip.
fn lane_label(state: &SequencerState, index: usize) -> String {
    let lane = &state.pattern().lanes[index];
    if lane.is_pitched() {
        format!("L{}", index + 1)
    } else if lane.note == phosphor_app::sequencer::DEFAULT_DRUM_LANES[index] {
        DEFAULT_DRUM_LABELS[index].to_string()
    } else {
        drum_label(lane.note)
    }
}

/// The letter a slot is called by.
fn slot_letter(slot: u8) -> char {
    (b'A' + slot.min(SLOTS as u8 - 1)) as char
}

/// Everything the sections read, gathered once.
struct Seq<'a> {
    state: &'a SequencerState,
    view: &'a SequencerView,
    /// The clip view is focused and this is the tab it is showing.
    focused: bool,
    width: usize,
    /// Which step the audio thread is on, when this track's pattern is running
    /// *and* the slot being looked at is the one playing.
    playhead: Option<usize>,
    /// Transport position, for the countdown to a queued switch.
    position: i64,
    colour: Color,
    /// The instrument the sequencer drives, for the child knob.
    child: Option<InstrumentType>,
    /// How many lanes get a row. Eight is the machine's face; fewer is a
    /// terminal too short for it, and the rows scroll under the cursor.
    lane_window: usize,
    /// How many clips are sitting on the track — a running pattern and a clip
    /// of the same part is the doubled-notes trap.
    clips: usize,
}

impl Seq<'_> {
    fn steps(&self) -> usize {
        self.state.pattern().step_count()
    }

    fn band(&self) -> SeqBand {
        self.view.band
    }

    /// Whether a band has the cursor *and* the view has the keyboard.
    fn on(&self, band: SeqBand) -> bool {
        self.focused && self.view.band == band
    }

    fn lane(&self) -> &Lane {
        self.state.lane()
    }

    /// Whether this is a kit, in which case a lane is a sound and the grid is
    /// eight of them stacked up.
    fn is_kit(&self) -> bool {
        !self.state.pattern().lanes[0].is_pitched()
    }

    /// The lanes that get a row, in the order they are drawn.
    ///
    /// All eight, on a kit and on a keyboard alike. On a kit that is the
    /// machine's face — a step written on the snare row is visibly on the
    /// snare. On a melodic pattern the same eight rows are how a chord gets
    /// layered: a seventh on one row and the ninth above it on the next,
    /// which is a thing the engine has always played and the view used to
    /// hide. Fewer rows only when the terminal is too short for eight, and
    /// then they scroll under the cursor.
    fn lane_rows(&self) -> Vec<usize> {
        let window = self.lane_window.clamp(1, LANES);
        // The cursor stays inside the window, roughly in the middle of it, so
        // that walking down the kit scrolls the rows rather than losing them.
        let start = self
            .state
            .lane_cursor()
            .saturating_sub(window / 2)
            .min(LANES - window);
        (start..start + window).collect()
    }

    /// Whether the strip has to stand in for rows that are not on the screen.
    fn shows_every_lane(&self) -> bool {
        self.lane_window >= LANES
    }

    /// Whether the pattern under the editor has anything on it at all.
    fn is_empty(&self) -> bool {
        self.state
            .pattern()
            .lanes
            .iter()
            .all(|lane| lane.steps.iter().all(|step| !step.on))
    }

    fn step(&self) -> &Step {
        self.state.step()
    }

    /// The style a knob's parts take, given where the cursor is.
    fn knob_spans(&self, knob: &Knob, index: usize, band: SeqBand) -> Vec<Span<'static>> {
        let selected = self.on(band) && self.view.knob == index;
        let locked = selected && self.view.locked;

        let label_style = if selected {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else if self.focused {
            theme::muted()
        } else {
            theme::dim()
        };
        let dial_style = if selected {
            Style::default().fg(theme::amber_bright_val()).bg(theme::bg_val())
        } else {
            Style::default().fg(self.colour).bg(theme::bg_val())
        };
        // Locked reads as inverse video in the theme's own colours rather than
        // as a colour of its own: every palette has a background and an amber,
        // and swapping them is legible in all nine.
        let value_style = if locked {
            Style::default()
                .fg(theme::bg_val())
                .bg(theme::amber_bright_val())
                .add_modifier(Modifier::BOLD)
        } else if selected {
            theme::amber_bright()
        } else if self.focused {
            theme::normal()
        } else {
            theme::dim()
        };

        vec![
            Span::styled(format!(" {} ", knob.label), label_style),
            Span::styled(knob_char(knob.frac).to_string(), dial_style),
            Span::styled(format!(" {}", knob.value), value_style),
            Span::styled(" ", theme::bg()),
        ]
    }

    /// A panel of knobs, wrapped to the width it has, under a title in the
    /// same column the grid's lane labels use.
    ///
    /// Answers which of the rows the cursor ended up on, so that a panel too
    /// tall for the space it has can be scrolled to the row being used rather
    /// than cut off at the bottom.
    fn knob_rows(
        &self,
        title: &str,
        knobs: &[Knob],
        band: SeqBand,
    ) -> (Vec<Line<'static>>, usize) {
        const INDENT: usize = LABEL_W + 4;
        let heading = if self.on(band) {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else if self.focused {
            theme::muted()
        } else {
            theme::dim()
        };

        let mut rows: Vec<Line> = Vec::new();
        let mut cursor_row = 0;
        let mut spans: Vec<Span> =
            vec![Span::styled(format!("{:>w$} ", title, w = INDENT - 1), heading)];
        let mut used = INDENT;

        for (index, knob) in knobs.iter().enumerate() {
            if used + knob.width() > self.width && used > INDENT {
                rows.push(Line::from(std::mem::take(&mut spans)));
                spans.push(Span::styled(" ".repeat(INDENT), theme::bg()));
                used = INDENT;
            }
            if index == self.view.knob {
                cursor_row = rows.len();
            }
            used += knob.width();
            spans.extend(self.knob_spans(knob, index, band));
        }
        rows.push(Line::from(spans));
        (rows, cursor_row)
    }
}

// ── Sections ──

/// The lane label column: a cursor mark, two characters of name, and a mute
/// or solo mark. Narrow, because every column it takes is a column the steps
/// do not get.
const LABEL_W: usize = 4;

/// Put text into a pre-allocated row of cells.
fn write_text(row: &mut [(char, Style)], x: usize, text: &str, style: Style) {
    for (offset, ch) in text.chars().enumerate() {
        if let Some(cell) = row.get_mut(x + offset) {
            *cell = (ch, style);
        }
    }
}

/// The geometry of one page of the step grid.
///
/// A step is a *button*, not a character: three columns of it, two of them
/// solid, with a gap every fourth step so the beats group the way they do on
/// the front panel of the machine this is imitating. Two columns per step is
/// the narrow fallback; below that the grid is paged rather than shrunk,
/// because a step you cannot see is better than sixteen you cannot read.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    /// Columns per step, including the space that separates it from the next.
    cell: usize,
    /// The first step of the page on the screen.
    first: usize,
    /// How many steps are on it.
    count: usize,
    /// How many pages the pattern is.
    pages: usize,
}

impl Geometry {
    /// The x of a step's cell, or `None` when it is on another page.
    fn x_of(&self, step: usize) -> Option<usize> {
        if step < self.first || step >= self.first + self.count {
            return None;
        }
        let offset = step - self.first;
        Some(LABEL_W + offset * self.cell + offset / BEAT * BEAT_GAP)
    }

    /// The columns one page needs.
    fn width(cell: usize, count: usize) -> usize {
        LABEL_W + count * cell + count.saturating_sub(1) / BEAT * BEAT_GAP
    }
}

/// Steps to a beat, which is what the gaps in the row count out.
const BEAT: usize = 4;

/// The gap between beats, in columns.
const BEAT_GAP: usize = 1;

/// A page of steps, at most sixteen.
const PAGE: usize = 16;

/// Work out how much of the pattern fits, and which part of it to show.
///
/// Everything is tried at full size first: sixteen steps of three columns
/// each is fifty-five, which is exactly what an eighty-column terminal has
/// left after the instrument panel. Only when the whole pattern will not fit
/// at either size does it page, and then the page is the one the cursor is
/// standing on.
fn geometry(seq: &Seq) -> Geometry {
    let steps = seq.steps().max(1);
    for cell in [3usize, 2] {
        if Geometry::width(cell, steps) <= seq.width {
            return Geometry { cell, first: 0, count: steps, pages: 1 };
        }
    }

    // Paged. The page is a bar of sixteen where that fits, and whatever does
    // fit on a terminal too narrow even for that — the steps stay the size
    // they are and the page gets shorter, rather than the other way round.
    let cell = if Geometry::width(3, PAGE.min(steps)) <= seq.width { 3 } else { 2 };
    let mut per_page = PAGE.min(steps);
    while per_page > 1 && Geometry::width(cell, per_page) > seq.width {
        per_page -= 1;
    }
    let pages = steps.div_ceil(per_page);
    let page = (seq.state.step_cursor() / per_page).min(pages - 1);
    let first = page * per_page;
    Geometry { cell, first, count: per_page.min(steps - first), pages }
}

/// The top line: what the machine is doing, in words.
///
/// A step sequencer that does not say whether it is running is a machine with
/// no transport lights on it, and "press play and nothing happened" is the
/// first thing that goes wrong for someone who has not used this one before.
fn header_line(seq: &Seq) -> Line<'static> {
    let state = seq.state;
    let mut spans: Vec<Span> = Vec::new();

    match seq.playhead {
        Some(step) => spans.push(Span::styled(
            format!(" \u{25B6} step {} of {} ", step + 1, seq.steps()),
            theme::amber_bright().add_modifier(Modifier::BOLD),
        )),
        None if state.is_playing() => spans.push(Span::styled(
            " \u{25A0} stopped \u{2014} t or SPC p plays ",
            theme::normal(),
        )),
        None => spans.push(Span::styled(
            " \u{25A0} muted \u{2014} t plays this pattern ",
            theme::muted(),
        )),
    }

    spans.push(Span::styled(" slot ", theme::dim()));
    spans.push(Span::styled(
        format!("{} ", slot_letter(state.selected_slot())),
        theme::amber().add_modifier(Modifier::BOLD),
    ));
    if let Some(child) = seq.child {
        spans.push(Span::styled(format!("\u{00B7} {} ", child.label()), theme::muted()));
    }

    let live = state.live_slot();
    if state.is_playing() && live != state.selected_slot() {
        spans.push(Span::styled(format!("(playing {}) ", slot_letter(live)), theme::muted()));
    }

    // The countdown. The same arithmetic the audio thread will do, on the
    // same inputs, so it is what will happen rather than a guess at it.
    if let Some((slot, steps)) = state.countdown(seq.position) {
        // "in 0 steps" is what the arithmetic says when the transport is
        // sitting exactly on the boundary, and it is not what a player wants
        // read out to them.
        let when = if steps <= 0 {
            "next".to_string()
        } else {
            format!("in {} step{}", steps, if steps == 1 { "" } else { "s" })
        };
        spans.push(Span::styled(
            format!("\u{2192} {} {when} ", slot_letter(slot)),
            theme::amber_bright().add_modifier(Modifier::BOLD),
        ));
    } else if state.is_chained() {
        spans.push(Span::styled("chained ", theme::muted()));
    }

    // A number half typed. Sixteen steps need two digits, so `1` has to wait
    // to see whether it is going to become `12`, and a key that has been
    // pressed and appears to have done nothing is worth showing.
    if !seq.view.digits.is_empty() {
        spans.push(Span::styled(
            format!(
                "{} {}_ ",
                if seq.band() == SeqBand::Slots { "slot" } else { "step" },
                seq.view.digits,
            ),
            theme::amber_bright(),
        ));
    }

    if state.is_step_recording() {
        spans.push(Span::styled(
            "\u{25CF} rec ",
            Style::default().fg(theme::rec_active_val()).add_modifier(Modifier::BOLD),
        ));
    }

    // The doubled-part warning, where it is looked at rather than only on the
    // track row: a pattern running under clips of its own bounce.
    if state.is_playing() && seq.clips > 0 {
        spans.push(Span::styled(
            format!("\u{203C} {} clip{} too ", seq.clips, if seq.clips == 1 { "" } else { "s" }),
            theme::amber_bright(),
        ));
    }

    Line::from(spans)
}

/// Every lane on one line, for the terminal that has no room to give each of
/// them a row of their own.
///
/// The rows are the lane list when they fit. This is what stands in for them
/// when they do not: which sounds the kit has, which one is being written,
/// and which of them have anything on them at all.
fn lane_strip(seq: &Seq) -> Line<'static> {
    let state = seq.state;
    let current = state.lane_cursor();
    // "sound" on a kit, where a lane is a drum voice; "voice" on a melodic
    // pattern, where a lane is one of eight notes that can sound at once.
    let mut spans: Vec<Span> =
        vec![Span::styled(if seq.is_kit() { " sound " } else { " voice " }, theme::dim())];

    for index in 0..LANES {
        let lane = &state.pattern().lanes[index];
        let label = lane_label(state, index);
        let selected = index == current;
        let used = lane.steps.iter().any(|s| s.on);

        let style = if selected && seq.focused {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else if lane.soloed {
            Style::default().fg(theme::solo_active_fg()).bg(theme::bg_val())
        } else if lane.muted || !state.pattern().lane_audible(index) {
            theme::dim()
        } else if used {
            Style::default().fg(seq.colour).bg(theme::bg_val())
        } else {
            theme::muted()
        };
        spans.push(Span::styled(
            if selected { format!("[{label}]") } else { format!(" {label} ") },
            style,
        ));
    }
    Line::from(spans)
}

/// The step numbers over the grid, and the running light in the same column
/// as the lanes below it.
fn ruler_line(seq: &Seq, geometry: Geometry) -> Line<'static> {
    let mut row = vec![(' ', theme::bg()); seq.width];
    let cursor = seq.state.step_cursor();

    if geometry.pages > 1 {
        let page = geometry.first / geometry.count.max(1) + 1;
        write_text(&mut row, 0, &format!("{page}/{}", geometry.pages), theme::dim());
    }

    for offset in 0..geometry.count {
        let step = geometry.first + offset;
        let Some(x) = geometry.x_of(step) else { continue };
        let beat = step % BEAT == 0;
        let lit = seq.playhead == Some(step);
        let style = if lit {
            light()
        } else if step == cursor && seq.on(SeqBand::Grid) {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else if beat {
            theme::normal().add_modifier(Modifier::BOLD)
        } else {
            theme::dim()
        };
        // The light takes the whole cell first, so the number sits inside a
        // lit block rather than beside one.
        if lit {
            for column in x..x + geometry.cell {
                if let Some(cell) = row.get_mut(column) {
                    *cell = (' ', light());
                }
            }
        }
        // Right-aligned in the solid columns of the cell, so the number sits
        // over the button rather than beside it.
        let text = format!("{:>w$}", step + 1, w = geometry.cell.saturating_sub(1).max(1));
        write_text(&mut row, x, &text, style);
    }
    grid_to_lines(vec![row]).remove(0)
}

/// The running light: the column the pattern is on, inverted.
///
/// Every cell in it, on every lane and on the ruler above them, so the light
/// reads as one bar sweeping across the machine rather than as eight separate
/// marks. This is the thing a step sequencer is recognised by from across a
/// room, and the reason it is the theme's brightest colour on its darkest.
fn light() -> Style {
    Style::default()
        .fg(theme::bg_val())
        .bg(theme::playhead_fg())
        .add_modifier(Modifier::BOLD)
}

/// The grid itself: one row per lane, all of them at once.
///
/// This is the whole point of the view. A drum machine's face is its lanes
/// side by side — the kick against the hat against the snare — and one lane
/// at a time with the others hinted at is what this replaced: a player who
/// wrote one step saw three marks appear and could not tell which of them
/// they had asked for.
fn grid_lines(seq: &Seq, geometry: Geometry) -> Vec<Line<'static>> {
    let state = seq.state;
    let pattern = state.pattern();
    let current = state.lane_cursor();
    let mut grid: Vec<Vec<(char, Style)>> = Vec::new();

    for &lane_index in seq.lane_rows().iter() {
        let lane = &pattern.lanes[lane_index];
        let here = lane_index == current;
        let audible = pattern.lane_audible(lane_index);
        let mut row = vec![(' ', theme::bg()); seq.width];

        // The row's own background, so the lane being written stands out from
        // the seven that are not.
        let row_bg = if here && seq.on(SeqBand::Grid) {
            theme::col_highlight_bg()
        } else {
            theme::bg_val()
        };
        for cell in row.iter_mut() {
            cell.1 = cell.1.bg(row_bg);
        }

        let label_style = if here && seq.focused {
            theme::amber_bright().add_modifier(Modifier::BOLD).bg(row_bg)
        } else if lane.soloed {
            Style::default().fg(theme::solo_active_fg()).bg(row_bg)
        } else if lane.muted || !audible {
            theme::dim().bg(row_bg)
        } else {
            Style::default().fg(theme::dim_color(seq.colour, 70)).bg(row_bg)
        };
        let mark = if here { '\u{25B8}' } else { ' ' };
        let state_mark = if lane.soloed {
            's'
        } else if lane.muted {
            'm'
        } else {
            ' '
        };
        write_text(
            &mut row,
            0,
            &format!("{mark}{:>2}{state_mark}", lane_label(state, lane_index)),
            label_style,
        );

        for offset in 0..geometry.count {
            let step = geometry.first + offset;
            let Some(x) = geometry.x_of(step) else { continue };
            let value = lane.steps[step];
            let cursor_here = here && step == state.step_cursor();

            let (glyph, fg) = if value.on && value.accent {
                ('\u{2588}', theme::amber_bright_val())
            } else if value.on {
                ('\u{2593}', if audible { seq.colour } else { theme::dim_color(seq.colour, 40) })
            } else {
                ('\u{2591}', theme::grid_minor())
            };
            let bg = if cursor_here {
                if seq.on(SeqBand::Grid) { theme::col_row_bg() } else { theme::col_highlight_bg() }
            } else {
                row_bg
            };
            let style = Style::default().fg(fg).bg(bg).add_modifier(
                if value.on { Modifier::BOLD } else { Modifier::empty() },
            );

            for column in 0..geometry.cell.saturating_sub(1) {
                if let Some(cell) = row.get_mut(x + column) {
                    *cell = (glyph, style);
                }
            }
            // The separator column carries a tie's tail, which is how "hold
            // this until the next hit" is read off the grid rather than out
            // of the step's panel.
            if let Some(cell) = row.get_mut(x + geometry.cell - 1) {
                let tie = value.on && value.gate == Step::TIE;
                *cell = (
                    if tie { '\u{2500}' } else { ' ' },
                    Style::default().fg(theme::dim_color(seq.colour, 45)).bg(bg),
                );
            }

            // ...and the light goes over everything, on every lane at once.
            if seq.playhead == Some(step) {
                for column in x..x + geometry.cell {
                    if let Some(cell) = row.get_mut(column) {
                        *cell = (cell.0, light());
                    }
                }
            }
        }
        grid.push(row);
    }
    grid_to_lines(grid)
}

/// What to press, for a pattern with nothing on it yet.
///
/// One quiet line, and only while the pattern is empty: the screen has to
/// answer "what do I do now" on its own, because the person reading it has
/// not been told and there is nothing else on the grid to look at.
fn coaching_line(seq: &Seq) -> Line<'static> {
    // A kit's rows are sounds; a keyboard's are voices to layer a chord
    // across. Same key, and the word says which machine this is.
    let rows = if seq.is_kit() { "pick a sound" } else { "pick a row" };
    Line::from(vec![
        Span::styled(format!("{:>w$} ", "", w = LABEL_W), theme::bg()),
        Span::styled("n", theme::amber_bright().add_modifier(Modifier::BOLD)),
        Span::styled(" \u{2014} write a step   ", theme::muted()),
        Span::styled("j/k", theme::amber_bright().add_modifier(Modifier::BOLD)),
        Span::styled(format!(" \u{2014} {rows}   "), theme::muted()),
        Span::styled("enter", theme::amber_bright().add_modifier(Modifier::BOLD)),
        Span::styled(" \u{2014} edit it   ", theme::muted()),
        Span::styled("t", theme::amber_bright().add_modifier(Modifier::BOLD)),
        Span::styled(" \u{2014} play", theme::muted()),
    ])
}

// ── Panels ──

/// A mode's name in three characters, for a knob that sits in a row of nine.
/// The full word is on the readout line under it.
fn mode_short(mode: Mode) -> &'static str {
    match mode {
        Mode::Chromatic => "off",
        Mode::Ionian => "ion",
        Mode::Dorian => "dor",
        Mode::Phrygian => "phr",
        Mode::Lydian => "lyd",
        Mode::Mixolydian => "mix",
        Mode::Aeolian => "aeo",
        Mode::Locrian => "loc",
    }
}

/// The value and dial position of one control.
fn knob_of(seq: &Seq, knob: SeqKnob) -> Knob {
    let pattern = seq.state.pattern();
    let step = *seq.step();
    let lane = seq.lane();
    let label = knob.label();

    match knob {
        SeqKnob::Child => {
            let choices: Vec<_> = crate::app::sequencer_keys::child_choices().collect();
            let at = seq
                .child
                .and_then(|c| choices.iter().position(|&x| x == c))
                .unwrap_or(0);
            let name = seq.child.map_or("—", InstrumentType::label);
            Knob::at(label, name, at, choices.len())
        }
        SeqKnob::Pitch => {
            // One control. The storage is an octave and a key; what the
            // player is given is a note, and a degree beside it when the
            // pattern is in a mode.
            let root = step.root();
            let value = match chords::degree_label(pattern.mode, pattern.tonic, root) {
                Some(degree) => format!("{degree}\u{00B7}{}", chords::note_label(root)),
                None => chords::note_label(root),
            };
            Knob::new(label, value, f64::from(root) / 127.0)
        }
        SeqKnob::Chord => Knob::at(
            label,
            chords::chord_name(step.chord_kind()),
            step.chord_kind().index() as usize,
            Chord::ALL.len(),
        ),
        SeqKnob::Voicing => Knob::at(
            label,
            step.voicing_kind().label(),
            step.voicing_kind().index() as usize,
            Voicing::ALL.len(),
        ),
        SeqKnob::RootBelow => Knob::toggle(label, step.root_below()),
        SeqKnob::Gate => {
            if step.gate == Step::TIE {
                Knob::new(label, "tie", 1.0)
            } else {
                Knob::new(
                    label,
                    format!("{}%", step.gate),
                    f64::from(step.gate) / f64::from(Step::MAX_GATE),
                )
            }
        }
        SeqKnob::Voice => Knob::new(
            label,
            format!("{} {}", drum_label(lane.note), lane.note),
            f64::from(lane.note) / 127.0,
        ),
        SeqKnob::Mute => Knob::toggle(label, lane.muted),
        SeqKnob::Solo => Knob::toggle(label, lane.soloed),
        SeqKnob::Length => Knob::at(
            label,
            format!("{}", pattern.step_count()),
            STEP_COUNTS.iter().position(|&c| c == pattern.steps).unwrap_or(3),
            STEP_COUNTS.len(),
        ),
        SeqKnob::Rate => Knob::at(
            label,
            pattern.rate.label(),
            pattern.rate.index() as usize,
            Rate::ALL.len(),
        ),
        SeqKnob::Swing => Knob::new(
            label,
            format!("{}%", pattern.swing),
            f64::from(pattern.swing - PatternBlock::MIN_SWING)
                / f64::from(PatternBlock::MAX_SWING - PatternBlock::MIN_SWING),
        ),
        SeqKnob::DefaultGate => Knob::new(
            label,
            format!("{}%", pattern.default_gate),
            f64::from(pattern.default_gate) / f64::from(Step::MAX_GATE),
        ),
        SeqKnob::BaseVelocity => {
            Knob::new(label, format!("{}", pattern.base_vel), f64::from(pattern.base_vel) / 127.0)
        }
        SeqKnob::AccentVelocity => Knob::new(
            label,
            format!("{}", pattern.accent_vel),
            f64::from(pattern.accent_vel) / 127.0,
        ),
        SeqKnob::Mode => Knob::at(
            label,
            mode_short(pattern.mode),
            pattern.mode.index() as usize,
            Mode::ALL.len(),
        ),
        SeqKnob::Tonic => Knob::at(
            label,
            chords::note_name(pattern.tonic),
            pattern.tonic as usize,
            12,
        ),
        SeqKnob::Switch => Knob::at(
            label,
            seq.state.switch_quant().label(),
            seq.state.switch_quant().index() as usize,
            SwitchQuant::ALL.len(),
        ),
    }
}

/// The controls belonging to the step under the cursor — or to the lane,
/// when the lane is a drum voice and the step only says *when*.
fn step_rows(seq: &Seq) -> (Vec<Line<'static>>, usize) {
    let knobs: Vec<Knob> = crate::app::sequencer_keys::step_knobs(seq.state)
        .iter()
        .map(|&knob| knob_of(seq, knob))
        .collect();
    let title = if seq.lane().is_pitched() {
        format!("step {}", seq.state.step_cursor() + 1)
    } else {
        format!("lane {}", lane_label(seq.state, seq.state.lane_cursor()))
    };
    seq.knob_rows(&title, &knobs, SeqBand::Step)
}

/// What the step under the cursor is playing, spelled out.
///
/// The chord's name and its notes, always on the screen: a player should not
/// have to open anything to find out what `min7` in the second inversion
/// actually sounds like.
///
/// Only drawn on a melodic pattern. On a kit the row a step is on is already
/// named after the sound it plays, which is the same question answered by
/// the grid itself.
fn readout_line(seq: &Seq) -> Line<'static> {
    let state = seq.state;
    let pattern = state.pattern();
    let step = *state.step();

    let mark = if step.on { "\u{25B8}" } else { " " };
    let mut spans: Vec<Span> = vec![Span::styled(
        format!("{:>w$} ", mark, w = LABEL_W + 3),
        theme::dim(),
    )];

    let root = step.root();
    spans.push(Span::styled(
        chords::readout(
            root,
            step.chord_kind(),
            step.voicing_kind(),
            step.root_below(),
            pattern.mode,
            pattern.tonic,
        ),
        if step.on { theme::normal() } else { theme::dim() },
    ));
    if pattern.mode != Mode::Chromatic {
        spans.push(Span::styled(
            format!("   {} {}", pattern.mode.label(), chords::note_name(pattern.tonic)),
            theme::muted(),
        ));
    }
    if step.accent {
        spans.push(Span::styled("  accent", theme::amber_bright()));
    }
    Line::from(spans)
}

/// The pattern's own settings.
fn pattern_rows(seq: &Seq) -> (Vec<Line<'static>>, usize) {
    let knobs: Vec<Knob> = crate::app::sequencer_keys::PATTERN_KNOBS
        .iter()
        .map(|&knob| knob_of(seq, knob))
        .collect();
    seq.knob_rows("pattern", &knobs, SeqBand::Pattern)
}

/// The eight slots, what is playing, what is queued, and the chain.
fn slots_line(seq: &Seq) -> Line<'static> {
    let state = seq.state;
    let focused = seq.on(SeqBand::Slots);
    let mut spans: Vec<Span> = vec![Span::styled(
        format!("{:>w$} ", "slots", w = LABEL_W + 3),
        if focused {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else if seq.focused {
            theme::muted()
        } else {
            theme::dim()
        },
    )];

    for slot in 0..SLOTS as u8 {
        let selected = slot == state.selected_slot();
        let live = slot == state.live_slot() && state.is_playing();
        let queued = state.queued_slot() == Some(slot);
        let used = state.pattern_at(slot as usize).lanes.iter().any(|l| l.steps.iter().any(|s| s.on));

        let mut style = if live {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else if queued {
            theme::amber()
        } else if used {
            Style::default().fg(seq.colour).bg(theme::bg_val())
        } else {
            theme::dim()
        };
        if selected {
            style = style.bg(if focused { theme::col_row_bg() } else { theme::col_highlight_bg() });
        }
        let mark = if live {
            '\u{25B6}'
        } else if queued {
            '\u{00BB}'
        } else if selected {
            '\u{25B8}'
        } else {
            ' '
        };
        spans.push(Span::styled(format!("{mark}{}", slot_letter(slot)), style));
        spans.push(Span::styled(" ", theme::bg()));
    }

    // Where a queued switch will land, beside the queueing itself. The knob
    // that sets it is on the pattern panel; this is the reading of it, and it
    // belongs next to the slot that is waiting on it.
    spans.push(Span::styled(
        format!(" on {} ", seq.state.switch_quant().label()),
        theme::muted(),
    ));

    let chain = state.chain();
    if chain.is_empty() {
        spans.push(Span::styled(" chain \u{2014}", theme::dim()));
    } else {
        spans.push(Span::styled(" chain ", theme::dim()));
        for entry in chain {
            let text = if entry.repeats > 1 {
                format!("{}\u{00D7}{} ", slot_letter(entry.slot), entry.repeats)
            } else {
                format!("{} ", slot_letter(entry.slot))
            };
            spans.push(Span::styled(text, theme::normal()));
        }
    }
    Line::from(spans)
}

// ── Assembly ──

/// Draw the step grid.
///
/// The lanes come first and are never dropped: a drum machine that is not
/// showing its lanes is not showing anything. Everything else is fitted
/// around them in priority order, and the band with the cursor on it is
/// always drawn — a panel too tall for its space scrolls to the row being
/// used rather than being cut off at the bottom.
pub(super) fn render_sequencer(
    frame: &mut Frame,
    area: Rect,
    nav: &NavState,
    snap: &TransportSnapshot,
) {
    let (width, height) = (area.width as usize, area.height as usize);
    if width == 0 || height == 0 {
        return;
    }

    let Some(track) = nav.current_track() else { return };
    let Some(state) = track.sequencer.as_deref() else {
        frame.render_widget(
            Paragraph::new(Span::styled("  no sequencer on this track", theme::dim())),
            area,
        );
        return;
    };

    let mut seq = Seq {
        state,
        view: &nav.clip_view.sequencer,
        focused: nav.focused_pane == Pane::ClipView
            && nav.clip_view.focus == ClipViewFocus::PianoRoll
            && nav.clip_view.clip_tab == ClipTab::Sequencer,
        width,
        // The light belongs to the pattern that is sounding. Drawing it on a
        // slot that is only being looked at would be a chase running through
        // a pattern nobody can hear.
        playhead: nav
            .sequencer_playhead(nav.track_cursor)
            .filter(|_| state.is_playing() && state.live_slot() == state.selected_slot())
            .map(|step| step.min(MAX_STEPS - 1)),
        position: snap.position_ticks,
        colour: theme::track_color(track.color_index),
        child: track.instrument_type,
        lane_window: LANES,
        clips: track.clips.len(),
    };
    // Eight rows for eight sounds as soon as there is room for them: that is
    // the face of the machine, and with it there is no need for the strip.
    // Below that the rows scroll — three lanes of a kit still shows the kick
    // against the hat, which one lane never can.
    //
    // The four are the header, the ruler, the step panel and the slots; the
    // fifth, when the rows do not all fit, is the strip that stands in for
    // them, and the sixth is the chord readout a melodic pattern carries.
    let fixed = if seq.is_kit() { 5 } else { 6 };
    seq.lane_window = if height >= LANES + fixed - 1 {
        LANES
    } else {
        height.saturating_sub(fixed).clamp(1, LANES)
    };
    let seq = seq;

    let geometry = geometry(&seq);
    let grid = grid_lines(&seq, geometry);
    let (step_panel, step_cursor_row) = step_rows(&seq);
    let (pattern_panel, pattern_cursor_row) = pattern_rows(&seq);

    // header, strip, ruler, grid, coaching, step, readout, pattern, slots
    const HEADER: usize = 0;
    const STRIP: usize = 1;
    const RULER: usize = 2;
    const GRID: usize = 3;
    const COACH: usize = 4;
    const STEP: usize = 5;
    const READOUT: usize = 6;
    const PATTERN: usize = 7;
    const SLOTS: usize = 8;

    let coaching = seq.is_empty();
    let readout = !seq.is_kit();
    let mut show = [
        1,
        usize::from(!seq.shows_every_lane()),
        1,
        grid.len(),
        usize::from(coaching),
        1,
        usize::from(readout),
        1,
        1,
    ];
    let sum = |show: &[usize; 9]| show.iter().sum::<usize>();

    // Too little room: give up the sections a player can do without, in the
    // order they can be done without, and never the band being used.
    //
    // The pattern's settings go first — they are set once and read rarely.
    // The chord readout outlives them, because on a keyboard it is the only
    // thing that says what a step is actually playing. The ruler goes last
    // of all: numbered steps are half of what makes a grid readable as
    // positions rather than as a wall of blocks.
    let band = seq.band();
    for &index in &[PATTERN, READOUT, COACH, SLOTS, STEP, STRIP, RULER] {
        if sum(&show) <= height {
            break;
        }
        let essential = (index == STEP && band == SeqBand::Step)
            || (index == PATTERN && band == SeqBand::Pattern)
            || (index == SLOTS && band == SeqBand::Slots);
        if !essential {
            show[index] = 0;
        }
    }

    // Room to spare: the rest of the panel being used first, then the other.
    let mut extras: Vec<(usize, usize)> = Vec::new();
    if band == SeqBand::Step {
        extras.push((STEP, step_panel.len()));
    } else if band == SeqBand::Pattern {
        extras.push((PATTERN, pattern_panel.len()));
    }
    extras.push((STEP, step_panel.len()));
    extras.push((PATTERN, pattern_panel.len()));
    for (index, wanted) in extras {
        if show[index] == 0 || show[index] >= wanted {
            continue;
        }
        let spare = height.saturating_sub(sum(&show));
        show[index] += spare.min(wanted - show[index]);
    }

    let mut lines: Vec<Line> = Vec::new();
    if show[HEADER] > 0 {
        lines.push(header_line(&seq));
    }
    if show[STRIP] > 0 {
        lines.push(lane_strip(&seq));
    }
    if show[RULER] > 0 {
        lines.push(ruler_line(&seq, geometry));
    }
    lines.extend(grid.into_iter().take(show[GRID]));
    if show[COACH] > 0 {
        lines.push(coaching_line(&seq));
    }
    lines.extend(window(step_panel, show[STEP], step_cursor_row));
    if show[READOUT] > 0 {
        lines.push(readout_line(&seq));
    }
    lines.extend(window(pattern_panel, show[PATTERN], pattern_cursor_row));
    if show[SLOTS] > 0 {
        lines.push(slots_line(&seq));
    }

    lines.truncate(height);
    frame.render_widget(Paragraph::new(lines), area);
}

/// `count` rows of a panel, chosen so that the row the cursor is on is one of
/// them. A knob under the cursor and off the bottom of the screen is a
/// control that answers keys nobody can see.
fn window(rows: Vec<Line<'static>>, count: usize, cursor_row: usize) -> Vec<Line<'static>> {
    if count == 0 {
        return Vec::new();
    }
    if count >= rows.len() {
        return rows;
    }
    let start = (cursor_row + 1).saturating_sub(count).min(rows.len() - count);
    rows.into_iter().skip(start).take(count).collect()
}


#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_app::state::InstrumentType;

    fn panel<'a>(state: &'a SequencerState, view: &'a SequencerView) -> Seq<'a> {
        Seq {
            state,
            view,
            focused: true,
            width: 120,
            playhead: None,
            position: 0,
            colour: theme::track_color(0),
            child: None,
            lane_window: LANES,
            clips: 0,
        }
    }

    /// The defect this catches, exactly: a knob whose declared width is one
    /// cell short of what it draws wraps a column too late and gets cut in
    /// half by the right edge of the panel.
    #[test]
    fn a_knob_is_as_wide_as_it_says_it_is() {
        let view = SequencerView::new();
        for child in [InstrumentType::DrumRack, InstrumentType::Juno60] {
            let state = SequencerState::new(child);
            let seq = panel(&state, &view);
            let every: Vec<SeqKnob> = crate::app::sequencer_keys::step_knobs(&state)
                .iter()
                .chain(crate::app::sequencer_keys::PATTERN_KNOBS.iter())
                .copied()
                .collect();
            for knob in every {
                let drawn = knob_of(&seq, knob);
                let cells: usize = seq
                    .knob_spans(&drawn, 0, SeqBand::Step)
                    .iter()
                    .map(|span| span.content.chars().count())
                    .sum();
                assert_eq!(cells, drawn.width(), "{} draws {cells} cells", drawn.label);
            }
        }
    }

    /// The dial reads its whole travel, and anything a float can be does not
    /// take it off the end of the ramp.
    #[test]
    fn the_dial_covers_its_travel_and_survives_nonsense() {
        assert_eq!(knob_char(0.0), '\u{25CB}');
        assert_eq!(knob_char(1.0), '\u{25CF}');
        assert_ne!(knob_char(0.5), knob_char(0.0));
        for frac in [-1.0, 2.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let _ = knob_char(frac);
        }
    }

    /// A step is a button, and a button that has shrunk to one character is
    /// not one. Below the width that fits them the pattern pages instead —
    /// sixteen steps of three columns is fifty-five, which is exactly what an
    /// eighty-column terminal has left after the instrument panel.
    #[test]
    fn steps_are_buttons_and_a_long_pattern_pages_rather_than_shrinking() {
        let state = SequencerState::new(InstrumentType::DrumRack);
        let view = SequencerView::new();
        let mut seq = panel(&state, &view);

        seq.width = 55;
        let full = geometry(&seq);
        assert_eq!((full.cell, full.count, full.pages), (3, 16, 1), "80 columns is a full grid");
        assert_eq!(full.x_of(0), Some(LABEL_W));
        assert_eq!(full.x_of(4), Some(LABEL_W + 13), "the beats are not grouped");

        seq.width = 54;
        assert_eq!(geometry(&seq).cell, 2, "a narrower panel goes to two columns");

        seq.width = 20;
        let tight = geometry(&seq);
        assert!(tight.cell >= 2, "a step was squeezed below two columns");
        assert!(tight.pages > 1, "a grid that cannot fit was not paged");
        assert!(
            Geometry::width(tight.cell, tight.count) <= 20,
            "a page wider than the panel it is drawn in",
        );
    }

    /// A paged pattern shows the page the cursor is standing on, so walking
    /// past step sixteen turns over rather than walking off the screen.
    #[test]
    fn the_page_follows_the_cursor() {
        use phosphor_app::sequencer::ops::{dispatch, SeqOp};

        let mut track = phosphor_app::state::TrackState::new(
            "seq",
            0,
            false,
            phosphor_core::project::TrackKind::Instrument,
            Vec::new(),
        );
        track.sequencer = Some(Box::new(SequencerState::new(InstrumentType::DrumRack)));
        dispatch(&mut track, SeqOp::CycleLength(2)); // 16 → 32
        dispatch(&mut track, SeqOp::SelectStep(20));

        let state = track.sequencer.as_deref().unwrap();
        let view = SequencerView::new();
        let mut seq = panel(state, &view);
        seq.width = 55;

        let geometry = geometry(&seq);
        assert_eq!(geometry.pages, 2);
        assert_eq!(geometry.first, 16, "the page with the cursor on it is not the one shown");
        assert!(geometry.x_of(20).is_some());
        assert!(geometry.x_of(4).is_none(), "a step from the other page was drawn");
    }

    /// A panel taller than its space shows the row the cursor is on, wherever
    /// in the panel that row is.
    #[test]
    fn a_windowed_panel_keeps_the_cursor_on_the_screen() {
        let rows: Vec<Line<'static>> = (0..5)
            .map(|index| Line::from(Span::raw(format!("row {index}"))))
            .collect();
        for cursor in 0..5 {
            let shown = window(rows.clone(), 2, cursor);
            assert_eq!(shown.len(), 2);
            let text: Vec<String> =
                shown.iter().map(|line| line.spans[0].content.to_string()).collect();
            assert!(
                text.contains(&format!("row {cursor}")),
                "row {cursor} fell off the screen: {text:?}",
            );
        }
        assert_eq!(window(rows.clone(), 9, 0).len(), 5, "a panel that fits is not windowed");
        assert!(window(rows, 0, 0).is_empty());
    }
}
