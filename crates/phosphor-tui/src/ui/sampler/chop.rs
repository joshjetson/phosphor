//! The chop screen: the recording across the pane with every cut marked,
//! and the rows that decide where the cuts go and where the slices land.
//!
//! # What is on the screen
//!
//! ```text
//!  chop · amen · 4.20s · transient · 9 slices → C1–G#1
//!  c lands them on C1–G#1
//!        ▄▄      ▄       ▄▄     ▄       ▄▄
//!  ▄▄████████▄▄▄███▄▄▄▄▄█████▄▄███▄▄▄▄▄█████▄▄▄
//!        ▀▀      ▀       ▀▀     ▀       ▀▀
//!  ─▲──────│───────┃───────│──────│───────│────
//!  ▸ cuts         2 of 9
//!    mode         transient
//!    listen       low · kicks
//!  j/k row · h/l change · enter holds a cut · a add · d remove · c lands · esc cancels
//! ```
//!
//! The slice under the cursor is lit in the track's colour, the rest of the
//! chop is drawn plain, and whatever the source's trim cut off is dim — the
//! three things a player is choosing between. On the ruler a cut the player
//! placed or moved is heavier than one detection proposed, because those are
//! the ones a change of setting will keep.
//!
//! The line under the header is the landing, in words, before it happens:
//! where the slices go, or — in red — why they cannot. The keyboard band
//! above says the same thing in colour, key by key.

use super::*;

use phosphor_app::sampler::chop::controls::ChopRow;
use phosphor_app::state::ChopScreen;

use super::wave::{column_of, wave_rows, with_peaks};

/// The tallest the waveform is drawn here — shorter than the trim strip's,
/// because the rows under it are the controls.
const MAX_WAVE_ROWS: usize = 8;

/// The width the row labels are padded to.
const LABEL_W: usize = 13;

const FREE_KEYS: &str = " j/k row \u{00b7} h/l change \u{00b7} enter holds a cut \u{00b7} a add \
                         \u{00b7} d remove \u{00b7} p play \u{00b7} c lands \u{00b7} esc cancels";

const HELD_KEYS: &str = " held \u{00b7} h/l moves the cut \u{00b7} H/L moves the slice's end \
                         \u{00b7} j/k unit \u{00b7} esc lets go";

/// The keys the chop will land on, first and last, when it has cuts.
pub(super) fn target_keys(chop: &ChopScreen) -> Option<(usize, usize)> {
    chop.plan.keys()
}

fn header(map: &Map, chop: &ChopScreen, width: usize) -> Line<'static> {
    let plan = &chop.plan;
    let mut row = Row::new(width);
    row.push(" chop ", map.heading());
    let (start, end) = plan.region();
    let seconds = (end - start) as f32 / plan.pcm().sample_rate.max(1.0);
    let name_w = row.left().saturating_sub(44).clamp(4, 24);
    row.push(format!("\u{00b7} {} ", clip_text(&plan.source().name, name_w)), theme::normal());
    row.push(format!("\u{00b7} {seconds:.2}s "), theme::dim());
    row.push(format!("\u{00b7} {} ", plan.mode.label()), theme::amber_bright());
    let count = plan.cuts().len();
    let slices = format!("\u{00b7} {count} slice{}", if count == 1 { "" } else { "s" });
    row.push(slices, theme::normal());
    if count > 0 {
        row.push(format!(" \u{2192} {}", plan.keys_label()), theme::normal());
    }
    row.line()
}

/// Where the slices will go, or why they cannot.
fn landing_line(map: &Map, chop: &ChopScreen, width: usize) -> Line<'static> {
    let text = match chop.plan.refusal(map.state) {
        Some(reason) => {
            let red = Style::default().fg(theme::rec_active_val()).bg(theme::bg_val());
            return Line::from(Span::styled(clip_text(&format!(" \u{2715} {reason}"), width), red));
        }
        None if chop.plan.cuts().is_empty() => " no cuts yet".into(),
        None => format!(" c lands them on {}", chop.plan.keys_label()),
    };
    Line::from(Span::styled(clip_text(&text, width), theme::dim()))
}

/// The ruler under the waveform: a mark at every cut, the one under the
/// cursor on top.
fn cuts_row(map: &Map, chop: &ChopScreen, frames: u64, width: usize) -> Line<'static> {
    let (start, end) = chop.plan.region();
    let inside = (column_of(start, frames, width), column_of(end.saturating_sub(1), frames, width));
    let mut cells: Vec<(&'static str, Style)> = (0..width)
        .map(|c| {
            if c >= inside.0 && c <= inside.1 {
                ("\u{2500}", theme::dim())
            } else {
                ("\u{00b7}", theme::dim())
            }
        })
        .collect();
    let last = width.saturating_sub(1);
    for cut in chop.plan.cuts() {
        let c = column_of(cut.frame, frames, width).min(last);
        cells[c] = if cut.pinned {
            ("\u{2503}", theme::amber_bright())
        } else {
            ("\u{2502}", theme::amber())
        };
    }
    if let Some(cut) = chop.plan.cuts().get(chop.plan.selected) {
        let c = column_of(cut.frame, frames, width).min(last);
        let style = if map.focused { theme::amber_bright() } else { theme::amber() };
        cells[c] = (if chop.held { "\u{25C6}" } else { "\u{25B2}" }, style.add_modifier(Modifier::BOLD));
    }
    Line::from(cells.into_iter().map(|(g, s)| Span::styled(g, s)).collect::<Vec<_>>())
}

fn control_rows(map: &Map, chop: &ChopScreen, width: usize) -> Vec<Line<'static>> {
    let current = chop.current();
    chop.rows()
        .iter()
        .map(|&row| {
            let here = row == current;
            let mut line = Row::new(width);
            line.push(
                if here { " \u{25B8} " } else { "   " },
                if map.focused { theme::amber_bright() } else { theme::dim() },
            );
            line.push(format!("{:<LABEL_W$}", row.label()), if here { theme::normal() } else { theme::dim() });
            let mut value = row.value(&chop.plan);
            if row == ChopRow::Cuts && chop.held {
                value = format!("{value} \u{00b7} held \u{00b7} unit {}", chop.unit.label());
            }
            let room = line.left();
            line.push(
                clip_text(&value, room),
                if here { theme::amber_bright() } else { theme::normal() },
            );
            line.line()
        })
        .collect()
}

/// The screen, for a pane `width` by `height`. `None` when no chop is open
/// on this track.
pub(super) fn chop_lines(map: &Map, width: usize, height: usize) -> Option<Vec<Line<'static>>> {
    let chop = map.chop?;
    if width == 0 || height == 0 {
        return Some(Vec::new());
    }
    let plan = &chop.plan;
    let pcm = plan.pcm();
    let frames = pcm.frames();

    let controls = control_rows(map, chop, width);
    // Header, landing, ruler, keys, and the controls: the waveform takes
    // what they leave, never less than a row.
    let chrome = 4 + controls.len();
    let wave = height.saturating_sub(chrome).clamp(1, MAX_WAVE_ROWS);

    let (start, end) = plan.region();
    let selected = plan.selected_slice();
    let lit = Style::default().fg(map.colour).bg(theme::bg_val()).add_modifier(Modifier::BOLD);

    let mut lines = vec![header(map, chop, width), landing_line(map, chop, width)];
    lines.extend(with_peaks(pcm, width, |peaks| {
        let columns = peaks.columns.len();
        let column_range = |(a, b): (u64, u64)| {
            (column_of(a, frames, columns), column_of(b.saturating_sub(1), frames, columns))
        };
        let region = column_range((start, end));
        let slice = selected.map(column_range);
        wave_rows(peaks, wave, move |c| {
            if slice.is_some_and(|(a, b)| c >= a && c <= b) {
                lit
            } else if c >= region.0 && c <= region.1 {
                theme::normal()
            } else {
                theme::dim()
            }
        })
    }));
    lines.push(cuts_row(map, chop, frames, width));
    lines.extend(controls);
    let keys = if chop.held { HELD_KEYS } else { FREE_KEYS };
    lines.push(Line::from(Span::styled(clip_text(keys, width), theme::dim())));
    lines.truncate(height);
    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{map, text};
    use super::*;
    use phosphor_app::sampler::chop::plan::ChopPlan;
    use phosphor_app::sampler::LayerState;
    use phosphor_plugin::sample::SamplePcm;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Four hits a quarter second apart, in a second of silence: a 1 kHz
    /// tone dying away over 60 ms each. A hit and not a click — a blip
    /// shorter than a hop is what the detector is built to ignore.
    fn screen() -> ChopScreen {
        let data = (0..48_000)
            .map(|i| {
                let t = (i % 12_000) as f32 - 1_000.0;
                if t < 0.0 {
                    return 0.0;
                }
                let secs = t / 48_000.0;
                0.8 * (std::f32::consts::TAU * 1_000.0 * secs).sin() * (-secs / 0.06).exp()
            })
            .collect();
        let pcm = Arc::new(SamplePcm { data, channels: 1, sample_rate: 48_000.0 });
        let plan = ChopPlan::new(LayerState::from_wav(PathBuf::from("clicks.wav"), pcm)).unwrap();
        ChopScreen::new(0, plan)
    }

    fn draw(state: &SamplerState, chop: &ChopScreen, width: usize, height: usize) -> Vec<Line<'static>> {
        let view = SamplerView::new();
        let mut m = map(state, &view);
        m.chop = Some(chop);
        chop_lines(&m, width, height).unwrap()
    }

    #[test]
    fn the_header_says_how_many_and_where() {
        let chop = screen();
        assert_eq!(chop.plan.cuts().len(), 4, "the clicks were not found");
        let shown = text(&draw(&SamplerState::new(), &chop, 100, 20));
        assert!(shown.contains("clicks"), "{shown}");
        assert!(shown.contains("4 slices \u{2192} C1\u{2013}D#1"), "{shown}");
        assert!(shown.contains("c lands them on C1\u{2013}D#1"), "{shown}");
        assert!(shown.contains("esc cancels"), "no way out on the screen:\n{shown}");
    }

    /// The refusal is on the screen before the key that would hit it.
    #[test]
    fn a_landing_that_would_be_refused_says_so_in_red_first() {
        let chop = screen();
        let mut state = SamplerState::new();
        let pcm = Arc::new(SamplePcm { data: vec![0.1; 100], channels: 1, sample_rate: 48_000.0 });
        state.add_wav_layer(chop.plan.first + 2, PathBuf::from("snare.wav"), pcm).unwrap();
        let lines = draw(&state, &chop, 120, 20);
        let shown = text(&lines);
        assert!(shown.contains("1 of the 4 keys C1\u{2013}D#1 already holds a sound"), "{shown}");
        let refusal = &lines[1].spans[0];
        assert_eq!(refusal.style.fg, Some(theme::rec_active_val()), "the refusal is not red");
    }

    #[test]
    fn every_cut_has_a_mark_and_the_cursor_one_is_the_caret() {
        let chop = screen();
        let lines = draw(&SamplerState::new(), &chop, 80, 20);
        let ruler = lines.iter().find(|l| l.spans.iter().any(|s| s.content == "\u{25B2}")).expect("no caret");
        let marks = ruler.spans.iter().filter(|s| ["\u{2502}", "\u{2503}", "\u{25B2}"].contains(&s.content.as_ref())).count();
        assert_eq!(marks, 4);
        assert_eq!(ruler.spans.len(), 80);
    }

    #[test]
    fn the_rows_follow_the_mode_and_show_the_cursor() {
        let mut chop = screen();
        let shown = text(&draw(&SamplerState::new(), &chop, 80, 20));
        assert!(shown.contains("\u{25B8} cuts"), "{shown}");
        assert!(shown.contains("listen") && shown.contains("everything"), "{shown}");
        chop.plan.adjust(ChopRow::Mode, 2, false);
        chop.move_row(2);
        let shown = text(&draw(&SamplerState::new(), &chop, 80, 20));
        assert!(!shown.contains("listen"), "a grid row list asked for a band:\n{shown}");
        assert!(shown.contains("\u{25B8} slices"), "{shown}");
    }

    #[test]
    fn a_held_cut_says_so_and_names_its_unit() {
        let mut chop = screen();
        chop.held = true;
        let shown = text(&draw(&SamplerState::new(), &chop, 100, 20));
        assert!(shown.contains("held \u{00b7} unit 10ms"), "{shown}");
        assert!(shown.contains("H/L moves the slice's end"), "{shown}");
    }

    /// The band speaks for the landing: amber where a slice will go, red
    /// where a key is in the way, and the pad cursor not lit at all — two
    /// ambers, one a target and one not, would say the chop lands on both.
    #[test]
    fn the_band_lights_the_landing_and_nothing_else() {
        let chop = screen();
        let first = chop.plan.first;
        let mut state = SamplerState::new();
        let pcm = Arc::new(SamplePcm { data: vec![0.1; 100], channels: 1, sample_rate: 48_000.0 });
        state.add_wav_layer(first + 1, PathBuf::from("snare.wav"), pcm).unwrap();
        state.cursor = SamplerState::pad_of_note(60).unwrap();
        let view = SamplerView::new();
        let mut m = map(&state, &view);
        m.chop = Some(&chop);
        let bg = |pad: usize| super::super::key_paint(&m, SamplerState::note_of_pad(pad)).bg;
        assert_eq!(bg(first), theme::amber_bright_val(), "a free target key is not amber");
        assert_eq!(bg(first + 1), theme::rec_active_val(), "a key in the way is not red");
        assert_ne!(bg(state.cursor), theme::amber_bright_val(), "the pad cursor is lit under a chop");
        assert_ne!(bg(first + 4), theme::amber_bright_val(), "a key past the landing is lit");
    }

    /// Nothing past the right edge or the bottom, and a short pane keeps the
    /// header and the landing line — the two that say what `c` will do.
    #[test]
    fn it_fits_whatever_it_is_given() {
        let chop = screen();
        for (w, h) in [(40, 6), (80, 12), (160, 40), (10, 2)] {
            let lines = draw(&SamplerState::new(), &chop, w, h);
            assert!(lines.len() <= h, "{w}x{h}: {} rows", lines.len());
            for line in &lines {
                let cells: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
                assert!(cells <= w, "{w}x{h}: a row of {cells}");
            }
        }
        let two = text(&draw(&SamplerState::new(), &chop, 60, 2));
        assert!(two.contains("chop") && two.contains("c lands"), "{two}");
    }
}
