//! The chop screen's rows: what each one is called, what it reads, and what
//! `h`/`l` do to it.
//!
//! The rows follow the mode, because each mode asks different questions: a
//! transient chop wants a band and a sensitivity, a grid wants bars and a
//! division, an equal chop wants a count. A row that meant nothing in the
//! mode on screen would be a key that silently does nothing.
//!
//! Every value stops at both ends rather than wrapping, the rule every
//! control in the house keeps: walking off one end and reappearing at the
//! other is how a setting gets changed by accident.

use phosphor_plugin::sample::NUM_PADS;

use super::grid::Division;
use super::land::Feel;
use super::plan::{ChopMode, ChopPlan, MAX_BARS};
use super::Band;
use crate::sampler::SamplerState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChopRow {
    /// The slices themselves: `h`/`l` walk them, Enter holds one.
    Cuts,
    Mode,
    Listen,
    Sensitivity,
    Fit,
    Bars,
    Every,
    Slices,
    From,
    Feel,
    Clip,
}

const BANDS: [Band; 4] = [Band::Full, Band::Low, Band::Mid, Band::High];
const DIVISIONS: [Division; 5] =
    [Division::Bar, Division::Half, Division::Beat, Division::Eighth, Division::Sixteenth];
const FEELS: [Feel; 2] = [Feel::Break, Feel::Melodic];

/// One sensitivity step on `h`/`l`, and one on `H`/`L`.
const SENSITIVITY_STEP: f32 = 0.05;
const SENSITIVITY_STRIDE: f32 = 0.20;

/// An octave: the stride of every count and key on this screen, because
/// "twelve more" is "one more octave of keys".
const OCTAVE: i32 = 12;

/// The rows the screen shows in `mode`, top to bottom.
pub fn rows(mode: ChopMode) -> &'static [ChopRow] {
    use ChopRow::*;
    match mode {
        ChopMode::Transient => &[Cuts, Mode, Listen, Sensitivity, Fit, From, Feel, Clip],
        ChopMode::Grid => &[Cuts, Mode, Bars, Every, From, Feel, Clip],
        ChopMode::Equal => &[Cuts, Mode, Slices, From, Feel, Clip],
    }
}

pub fn band_label(band: Band) -> &'static str {
    match band {
        Band::Full => "everything",
        Band::Low => "low \u{00b7} kicks",
        Band::Mid => "mid \u{00b7} snares",
        Band::High => "high \u{00b7} hats",
    }
}

pub fn division_label(division: Division) -> &'static str {
    match division {
        Division::Bar => "bar",
        Division::Half => "half bar",
        Division::Beat => "beat",
        Division::Eighth => "1/8",
        Division::Sixteenth => "1/16",
    }
}

pub fn feel_label(feel: Feel) -> &'static str {
    match feel {
        Feel::Break => "break \u{00b7} one-shots that cut each other",
        Feel::Melodic => "melodic \u{00b7} held keys ring together",
    }
}

/// Step through a short list, stopping at both ends.
fn step<T: Copy + PartialEq>(list: &[T], now: T, delta: i32) -> T {
    let here = list.iter().position(|v| *v == now).unwrap_or(0) as i32;
    list[(here + delta).clamp(0, list.len() as i32 - 1) as usize]
}

fn nudged(value: u32, delta: i32, lo: u32, hi: u32) -> u32 {
    (i64::from(value) + i64::from(delta)).clamp(i64::from(lo), i64::from(hi)) as u32
}

impl ChopRow {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cuts => "cuts",
            Self::Mode => "mode",
            Self::Listen => "listen",
            Self::Sensitivity => "sensitivity",
            Self::Fit => "fit",
            Self::Bars => "bars",
            Self::Every => "cut every",
            Self::Slices => "slices",
            Self::From => "from",
            Self::Feel => "feel",
            Self::Clip => "clip",
        }
    }

    /// What the row reads.
    pub fn value(self, plan: &ChopPlan) -> String {
        match self {
            Self::Cuts => match plan.cuts().len() {
                0 => "none \u{00b7} a adds one".into(),
                n => format!("{} of {n}", plan.selected + 1),
            },
            Self::Mode => plan.mode.label().into(),
            Self::Listen => band_label(plan.band).into(),
            Self::Sensitivity => format!("{:.0}%", plan.sensitivity * 100.0),
            Self::Fit => match plan.fit {
                None => "every hit".into(),
                Some(n) => format!("the {n} strongest"),
            },
            Self::Bars => plan.bars.to_string(),
            Self::Every => division_label(plan.division).into(),
            Self::Slices => plan.slices.to_string(),
            Self::From => SamplerState::pad_label(plan.first),
            Self::Feel => feel_label(plan.feel).into(),
            Self::Clip => if plan.clip {
                "yes \u{00b7} a clip replays the recording through the slices".into()
            } else {
                "no \u{00b7} l writes a clip that replays it".into()
            },
        }
    }

    /// Whether changing this row moves the cuts, so the plan must propose
    /// them again. Where the slices land and how they behave does not.
    fn recuts(self) -> bool {
        !matches!(self, Self::Cuts | Self::From | Self::Feel | Self::Clip)
    }
}

impl ChopPlan {
    /// `h`/`l` (and `H`/`L`, `stride`) on a settings row. Returns whether
    /// anything changed — a press against a wall is worth no redraw of the
    /// cuts, and no audition.
    ///
    /// The cuts row is not a setting and is answered by the screen's keys.
    pub fn adjust(&mut self, row: ChopRow, delta: i32, stride: bool) -> bool {
        let before = (
            self.clip,
            self.mode,
            self.band,
            self.sensitivity,
            self.fit,
            self.bars,
            self.division,
            self.slices,
            self.first,
            self.feel,
        );
        let big = if stride { OCTAVE } else { 1 };
        match row {
            ChopRow::Cuts => return false,
            ChopRow::Mode => self.mode = step(&ChopMode::ALL, self.mode, delta),
            ChopRow::Listen => self.band = step(&BANDS, self.band, delta),
            ChopRow::Sensitivity => {
                let by = if stride { SENSITIVITY_STRIDE } else { SENSITIVITY_STEP };
                // Held in whole steps, so twenty presses up and twenty down
                // land exactly where they started.
                let steps = (self.sensitivity / SENSITIVITY_STEP).round() + (by / SENSITIVITY_STEP).round() * delta as f32;
                self.sensitivity = (steps * SENSITIVITY_STEP).clamp(0.0, 1.0);
            }
            ChopRow::Fit => self.fit = self.fit_stepped(delta * big),
            ChopRow::Bars => self.bars = nudged(self.bars, delta * if stride { 4 } else { 1 }, 1, MAX_BARS),
            ChopRow::Every => self.division = step(&DIVISIONS, self.division, delta),
            ChopRow::Slices => self.slices = nudged(self.slices, delta * big, 1, NUM_PADS as u32),
            ChopRow::From => {
                self.first = nudged(self.first as u32, delta * big, 0, NUM_PADS as u32 - 1) as usize;
            }
            ChopRow::Feel => self.feel = step(&FEELS, self.feel, delta),
            ChopRow::Clip => self.clip = step(&[false, true], self.clip, delta),
        }
        let after = (
            self.clip,
            self.mode,
            self.band,
            self.sensitivity,
            self.fit,
            self.bars,
            self.division,
            self.slices,
            self.first,
            self.feel,
        );
        let changed = before != after;
        if changed && row.recuts() {
            self.redetect();
        }
        changed
    }

    /// The fit, walked: down from "every hit" starts one under what was
    /// found, and up past what was found is "every hit" again. Never zero —
    /// a chop that keeps nothing is a key press that erased the screen.
    fn fit_stepped(&self, delta: i32) -> Option<usize> {
        let found = self.proposed().max(1);
        let now = self.fit.unwrap_or(found) as i32;
        let next = (now + delta).max(1) as usize;
        (next < found).then_some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::chop::testkit::{boom_bap, SR};
    use crate::sampler::LayerState;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn plan() -> ChopPlan {
        let pcm = Arc::new(boom_bap(SR).0);
        ChopPlan::new(LayerState::from_wav(PathBuf::from("break.wav"), pcm)).unwrap()
    }

    #[test]
    fn each_mode_asks_only_its_own_questions() {
        assert!(rows(ChopMode::Transient).contains(&ChopRow::Listen));
        assert!(!rows(ChopMode::Grid).contains(&ChopRow::Listen));
        assert!(rows(ChopMode::Grid).contains(&ChopRow::Every));
        assert!(rows(ChopMode::Equal).contains(&ChopRow::Slices));
        for mode in ChopMode::ALL {
            let r = rows(mode);
            assert_eq!(r[0], ChopRow::Cuts, "the cuts are always the first row");
            assert!(r.contains(&ChopRow::From) && r.contains(&ChopRow::Feel));
            assert_eq!(r.last(), Some(&ChopRow::Clip), "the clip row is the last in every mode");
        }
    }

    #[test]
    fn values_stop_at_both_ends() {
        let mut plan = plan();
        assert!(!plan.adjust(ChopRow::Mode, -1, false), "walked off the top of the modes");
        assert!(plan.adjust(ChopRow::Mode, 5, false));
        assert_eq!(plan.mode, ChopMode::Equal);
        plan.first = 0;
        assert!(!plan.adjust(ChopRow::From, -1, false));
        plan.first = NUM_PADS - 1;
        assert!(!plan.adjust(ChopRow::From, 1, true));
        assert!(!plan.adjust(ChopRow::Cuts, 1, false), "the cuts row is not a setting");
    }

    #[test]
    fn sensitivity_walks_in_whole_steps_and_comes_back_exactly() {
        let mut plan = plan();
        let start = plan.sensitivity;
        for _ in 0..7 {
            plan.adjust(ChopRow::Sensitivity, 1, false);
        }
        for _ in 0..7 {
            plan.adjust(ChopRow::Sensitivity, -1, false);
        }
        assert_eq!(plan.sensitivity, start);
        plan.adjust(ChopRow::Sensitivity, 1, true);
        assert!((plan.sensitivity - (start + 0.2)).abs() < 1e-6);
        for _ in 0..30 {
            plan.adjust(ChopRow::Sensitivity, 1, true);
        }
        assert_eq!(plan.sensitivity, 1.0);
        assert_eq!(ChopRow::Sensitivity.value(&plan), "100%");
    }

    #[test]
    fn turning_the_sensitivity_up_finds_more() {
        let mut plan = plan();
        let before = plan.cuts().len();
        plan.adjust(ChopRow::Sensitivity, 1, true);
        plan.adjust(ChopRow::Sensitivity, 1, true);
        plan.adjust(ChopRow::Sensitivity, 1, true);
        assert!(plan.cuts().len() > before, "the cuts did not follow the knob");
    }

    /// The owner's "bigger slices on one octave": fit walked down to twelve
    /// is an octave's worth of cuts.
    #[test]
    fn fit_walks_down_from_every_hit_and_back_up_to_it() {
        let mut plan = plan();
        plan.sensitivity = 1.0;
        plan.redetect();
        let found = plan.cuts().len();
        assert!(found > 3);
        plan.adjust(ChopRow::Fit, -1, false);
        assert_eq!(plan.fit, Some(found - 1));
        assert_eq!(plan.cuts().len(), found - 1);
        assert_eq!(ChopRow::Fit.value(&plan), format!("the {} strongest", found - 1));
        for _ in 0..100 {
            plan.adjust(ChopRow::Fit, -1, false);
        }
        assert_eq!(plan.fit, Some(1), "a fit reached zero");
        for _ in 0..100 {
            plan.adjust(ChopRow::Fit, 1, false);
        }
        assert_eq!(plan.fit, None);
        assert_eq!(plan.cuts().len(), found);
    }

    #[test]
    fn where_and_how_it_lands_never_moves_a_cut() {
        let mut plan = plan();
        let cuts = plan.cuts().to_vec();
        plan.nudge_cut(96, false);
        let held = plan.cuts().to_vec();
        assert_ne!(cuts, held);
        plan.adjust(ChopRow::From, 1, true);
        plan.adjust(ChopRow::Feel, 1, false);
        assert_eq!(plan.cuts(), &held[..]);
        assert_eq!(ChopRow::From.value(&plan), "C3");
    }

    #[test]
    fn the_rows_read_in_words() {
        let mut plan = plan();
        assert_eq!(ChopRow::Listen.value(&plan), "everything");
        plan.adjust(ChopRow::Listen, 1, false);
        assert_eq!(ChopRow::Listen.value(&plan), "low \u{00b7} kicks");
        assert!(ChopRow::Cuts.value(&plan).starts_with("1 of "));
        assert!(ChopRow::Feel.value(&plan).starts_with("break"));
    }
}
