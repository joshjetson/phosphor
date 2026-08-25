//! UI rendering: the step grid.
//!
//! # What is on the screen
//!
//! Four bands, top to bottom, and `j`/`k` walks them:
//!
//! * **grid** — one lane's sixteen (or thirty-two) steps, with the hits from
//!   every other lane behind them as ghosts, so a kick can be written against
//!   a hat that is not currently being edited;
//! * **step** — the controls belonging to the step under the cursor: one pitch
//!   control, chord, voicing, gate;
//! * **pattern** — length, rate, swing, the velocities, mode and key;
//! * **slots** — the eight patterns, what is queued, and the chain.
//!
//! Everything here reads. Not one function in this file changes a pattern —
//! the keys do that, and only by naming a [`SeqOp`](phosphor_app::sequencer::ops::SeqOp).
//!
//! # Fitting
//!
//! The band layout is not fixed: sections are laid out in priority order and
//! the ones that do not fit are left out, so the view works at the eight rows
//! an 80×24 terminal leaves it and spreads into the twenty a large one gives.
//! The per-lane mini-map is the first thing dropped and the last thing added.

use super::*;

use phosphor_app::sequencer::{chords, SequencerState, DEFAULT_DRUM_LABELS};
use phosphor_core::pattern::{
    Chord, Lane, Mode, PatternBlock, Rate, Step, SwitchQuant, Voicing, LANES, MAX_STEPS, SLOTS,
    STEP_COUNTS,
};

/// How many steps a display row holds before wrapping to the next.
const ROW_STEPS: usize = 16;

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

/// Width of the label column that the grid rows and the mini-map share.
const LABEL_W: usize = 5;

/// Put text into a pre-allocated row of cells.
fn write_text(row: &mut [(char, Style)], x: usize, text: &str, style: Style) {
    for (offset, ch) in text.chars().enumerate() {
        if let Some(cell) = row.get_mut(x + offset) {
            *cell = (ch, style);
        }
    }
}

/// How many cells one step gets. Three when there is room — enough to draw a
/// tie's tail — and two when there is not.
fn cell_width(seq: &Seq) -> usize {
    let per_row = seq.steps().min(ROW_STEPS);
    if LABEL_W + per_row * 3 <= seq.width {
        3
    } else {
        2
    }
}

/// The top line: what this is, which slot, and what is about to happen.
fn header_line(seq: &Seq) -> Line<'static> {
    let state = seq.state;
    let mut spans: Vec<Span> = vec![Span::styled(
        " seq ",
        if seq.focused {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else {
            theme::dim()
        },
    )];

    let live = state.live_slot();
    let running = state.is_playing();
    spans.push(Span::styled(
        format!("{} slot {} ", if running { "\u{25B6}" } else { "\u{25A0}" }, slot_letter(state.selected_slot())),
        if running { theme::amber_bright() } else { theme::normal() },
    ));

    if running && live != state.selected_slot() {
        spans.push(Span::styled(
            format!("(playing {}) ", slot_letter(live)),
            theme::muted(),
        ));
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
    if running && seq.clips > 0 {
        spans.push(Span::styled(
            format!("\u{203C} {} clip{} too ", seq.clips, if seq.clips == 1 { "" } else { "s" }),
            theme::amber_bright(),
        ));
    }

    Line::from(spans)
}

/// The lane strip: eight names, the current one in brackets.
fn lane_line(seq: &Seq) -> Line<'static> {
    let state = seq.state;
    let current = state.lane_cursor();
    let mut spans: Vec<Span> = vec![Span::styled(" lane ", theme::dim())];

    for index in 0..LANES {
        let lane = &state.pattern().lanes[index];
        let label = lane_label(state, index);
        let selected = index == current;
        let audible = state.pattern().lane_audible(index);

        let style = if selected && seq.focused {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else if lane.soloed {
            Style::default().fg(theme::solo_active_fg()).bg(theme::bg_val())
        } else if !audible || lane.muted {
            theme::dim()
        } else if lane.steps.iter().any(|s| s.on) {
            Style::default().fg(seq.colour).bg(theme::bg_val())
        } else {
            theme::muted()
        };

        let text = if selected {
            format!("[{label}]")
        } else if lane.muted {
            format!(" {label}\u{00B7}")
        } else {
            format!(" {label} ")
        };
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

/// Step numbers above the grid, every fourth one.
fn ruler_line(seq: &Seq, cw: usize) -> Line<'static> {
    let mut row = vec![(' ', theme::bg()); seq.width];
    let cursor = seq.state.step_cursor();
    let per_row = seq.steps().min(ROW_STEPS);
    for index in 0..per_row {
        if index % 4 != 0 && index != cursor {
            continue;
        }
        let style = if index == cursor && seq.on(SeqBand::Grid) {
            theme::amber_bright().add_modifier(Modifier::BOLD)
        } else if index == cursor {
            theme::amber()
        } else {
            theme::dim()
        };
        write_text(&mut row, LABEL_W + index * cw, &format!("{}", index + 1), style);
    }
    grid_to_lines(vec![row]).remove(0)
}

/// The step row (or two) for the lane being edited.
///
/// The hits from the other seven lanes are behind them, dimmed. Editing one
/// lane at a time is what makes a step grid usable in a terminal, and a lane
/// edited with no sight of the others is how a kick ends up on the same step
/// as a crash for the whole of a pattern.
fn grid_lines(seq: &Seq, cw: usize) -> Vec<Line<'static>> {
    let state = seq.state;
    let steps = seq.steps();
    let lane = seq.lane();
    let cursor = state.step_cursor();
    let rows = steps.div_ceil(ROW_STEPS).max(1);
    let mut grid: Vec<Vec<(char, Style)>> = Vec::new();

    for row_index in 0..rows {
        let mut row = vec![(' ', theme::bg()); seq.width];
        if row_index == 0 {
            let label = lane_label(state, state.lane_cursor());
            let style = if seq.on(SeqBand::Grid) {
                theme::amber_bright().add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(seq.colour).bg(theme::bg_val())
            };
            write_text(&mut row, 0, &format!("{label:>4} "), style);
        } else {
            // A second row of sixteen is still the same lane, so the label
            // column says where it starts instead: the numbers above the
            // columns are the first row's.
            write_text(
                &mut row,
                0,
                &format!("{:>4} ", row_index * ROW_STEPS + 1),
                theme::dim(),
            );
        }

        let start = row_index * ROW_STEPS;
        for offset in 0..ROW_STEPS.min(steps.saturating_sub(start)) {
            let index = start + offset;
            let step = lane.steps[index];
            let ghost = !step.on
                && (0..LANES).any(|other| {
                    other != state.lane_cursor()
                        && state.pattern().lanes[other].steps[index].on
                        && state.pattern().lane_audible(other)
                });

            let here = index == cursor;
            let playing = seq.playhead == Some(index);
            let bg = if here && seq.on(SeqBand::Grid) {
                theme::col_row_bg()
            } else if here {
                theme::col_highlight_bg()
            } else if playing {
                theme::playhead_bg()
            } else {
                theme::bg_val()
            };

            let (glyph, fg) = if step.on && step.accent {
                ('\u{25C9}', theme::amber_bright_val())
            } else if step.on {
                ('\u{25CF}', seq.colour)
            } else if ghost {
                ('\u{25E6}', theme::dim_color(seq.colour, 35))
            } else if playing {
                ('\u{2502}', theme::playhead_fg())
            } else if index % 4 == 0 {
                ('\u{250A}', theme::grid_major())
            } else {
                ('\u{00B7}', theme::grid_minor())
            };

            let x = LABEL_W + offset * cw;
            let style = Style::default().fg(fg).bg(bg).add_modifier(
                if step.on { Modifier::BOLD } else { Modifier::empty() },
            );
            if let Some(cell) = row.get_mut(x) {
                *cell = (glyph, style);
            }
            // A tie's tail runs into the cells the step's own width leaves,
            // so "hold this until the next hit" is visible on the grid rather
            // than only in the step's panel.
            let tail = if step.on && step.gate == Step::TIE { '\u{254C}' } else { ' ' };
            let tail_style = Style::default().fg(theme::dim_color(seq.colour, 45)).bg(bg);
            for fill in 1..cw {
                if let Some(cell) = row.get_mut(x + fill) {
                    *cell = (tail, tail_style);
                }
            }
        }
        grid.push(row);
    }
    grid_to_lines(grid)
}

/// One glyph per step per lane: the whole pattern at a glance.
fn minimap_lines(seq: &Seq) -> Vec<Line<'static>> {
    let state = seq.state;
    let steps = seq.steps();
    let mut grid: Vec<Vec<(char, Style)>> = Vec::new();

    for index in 0..LANES {
        let lane = &state.pattern().lanes[index];
        let current = index == state.lane_cursor();
        let audible = state.pattern().lane_audible(index);
        let mut row = vec![(' ', theme::bg()); seq.width];

        let label_style = if current && seq.focused {
            theme::amber_bright()
        } else if audible {
            theme::muted()
        } else {
            theme::dim()
        };
        write_text(&mut row, 0, &format!("{:>4} ", lane_label(state, index)), label_style);

        for step_index in 0..steps.min(seq.width.saturating_sub(LABEL_W)) {
            let step = lane.steps[step_index];
            let playing = seq.playhead == Some(step_index);
            let bg = if playing { theme::playhead_bg() } else { theme::bg_val() };
            let (glyph, fg) = if !step.on {
                ('\u{00B7}', theme::grid_minor())
            } else if !audible {
                ('\u{25CB}', theme::dim_color(seq.colour, 25))
            } else if step.accent {
                ('\u{25C9}', theme::amber_bright_val())
            } else {
                ('\u{25CF}', if current { seq.colour } else { theme::dim_color(seq.colour, 60) })
            };
            row[LABEL_W + step_index] = (glyph, Style::default().fg(fg).bg(bg));
        }
        grid.push(row);
    }
    grid_to_lines(grid)
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
fn readout_line(seq: &Seq) -> Line<'static> {
    let state = seq.state;
    let pattern = state.pattern();
    let step = *state.step();
    let lane = seq.lane();

    let mark = if step.on { "\u{25B8}" } else { " " };
    let mut spans: Vec<Span> = vec![Span::styled(
        format!("{:>w$} ", mark, w = LABEL_W + 3),
        theme::dim(),
    )];

    if !lane.is_pitched() {
        spans.push(Span::styled(
            format!("{} \u{00B7} note {}", drum_label(lane.note), lane.note),
            if step.on { theme::normal() } else { theme::dim() },
        ));
        if step.accent {
            spans.push(Span::styled(
                format!("  accent {}", pattern.accent_vel),
                theme::amber_bright(),
            ));
        } else if step.on {
            spans.push(Span::styled(format!("  vel {}", pattern.base_vel), theme::muted()));
        }
        return Line::from(spans);
    }

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
/// The sections are laid out by priority rather than by a fixed geometry: the
/// grid and the band with the cursor on it are never dropped, the mini-map is
/// only drawn when there is room left over, and the panel being used scrolls
/// to the row the cursor is on rather than being cut off at the bottom.
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

    let seq = Seq {
        state,
        view: &nav.clip_view.sequencer,
        focused: nav.focused_pane == Pane::ClipView
            && nav.clip_view.focus == ClipViewFocus::PianoRoll
            && nav.clip_view.clip_tab == ClipTab::Sequencer,
        width,
        // The marker belongs to the pattern that is sounding. Drawing it on a
        // slot that is only being looked at would be a playhead running
        // through a pattern nobody can hear.
        playhead: nav
            .sequencer_playhead(nav.track_cursor)
            .filter(|_| state.live_slot() == state.selected_slot())
            .map(|step| step.min(MAX_STEPS - 1)),
        position: snap.position_ticks,
        colour: theme::track_color(track.color_index),
        child: track.instrument_type,
        clips: track.clips.len(),
    };

    let cw = cell_width(&seq);
    let grid = grid_lines(&seq, cw);
    let (step_panel, step_cursor_row) = step_rows(&seq);
    let (pattern_panel, pattern_cursor_row) = pattern_rows(&seq);

    // header, lanes, ruler, grid, mini-map, step, readout, pattern, slots
    const HEADER: usize = 0;
    const LANES_ROW: usize = 1;
    const RULER: usize = 2;
    const GRID: usize = 3;
    const MINIMAP: usize = 4;
    const STEP: usize = 5;
    const READOUT: usize = 6;
    const PATTERN: usize = 7;
    const SLOTS: usize = 8;

    let mut show = [1usize, 1, 0, grid.len(), 0, 1, 1, 1, 1];
    let sum = |show: &[usize; 9]| show.iter().sum::<usize>();

    // Too little room: give up the sections a player can do without, in the
    // order they can be done without, and never the band being used.
    let band = seq.band();
    for &index in &[READOUT, SLOTS, PATTERN, LANES_ROW, STEP] {
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

    // Room to spare: the rest of the panel being used first, then the step
    // numbers, then the other panel, then the whole pattern lane by lane.
    let mut extras: Vec<(usize, usize)> = Vec::new();
    if band == SeqBand::Step {
        extras.push((STEP, step_panel.len()));
    } else if band == SeqBand::Pattern {
        extras.push((PATTERN, pattern_panel.len()));
    }
    extras.push((RULER, 1));
    extras.push((STEP, step_panel.len()));
    extras.push((PATTERN, pattern_panel.len()));
    extras.push((MINIMAP, LANES));
    for (index, wanted) in extras {
        if show[index] >= wanted || (show[index] == 0 && index != RULER && index != MINIMAP) {
            continue;
        }
        let spare = height.saturating_sub(sum(&show));
        // A knob panel takes what it can get and scrolls to the cursor; the
        // step numbers and the mini-map are only worth drawing whole.
        let granted = if index == STEP || index == PATTERN {
            spare.min(wanted - show[index])
        } else if wanted - show[index] <= spare {
            wanted - show[index]
        } else {
            0
        };
        show[index] += granted;
    }

    let mut lines: Vec<Line> = Vec::new();
    if show[HEADER] > 0 {
        lines.push(header_line(&seq));
    }
    if show[LANES_ROW] > 0 {
        lines.push(lane_line(&seq));
    }
    if show[RULER] > 0 {
        lines.push(ruler_line(&seq, cw));
    }
    lines.extend(grid.into_iter().take(show[GRID]));
    if show[MINIMAP] > 0 {
        lines.extend(minimap_lines(&seq).into_iter().take(show[MINIMAP]));
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

    /// Steps get three cells where there is room for the tie tails and two
    /// where there is not, and never fewer — a step grid with no gap between
    /// its steps is unreadable.
    #[test]
    fn steps_are_three_cells_wide_when_they_fit() {
        let state = SequencerState::new(InstrumentType::DrumRack);
        let view = SequencerView::new();
        let mut seq = panel(&state, &view);
        seq.width = 53;
        assert_eq!(cell_width(&seq), 3);
        seq.width = 52;
        assert_eq!(cell_width(&seq), 2);
        seq.width = 10;
        assert_eq!(cell_width(&seq), 2);
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
