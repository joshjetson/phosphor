//! The judge: what the player played, against what the exercise asked.
//!
//! Two modes, straight from the products that got this right. **Wait**
//! (Synthesia's melody practice, Yamaha's "waiting" step): time stands
//! still until the right note lands — pitch and fingering-by-feel first,
//! rhythm later. **Flow** (every rhythm game): the exercise rolls at the
//! metronome's tempo and every onset is judged against the grid, from the
//! MIDI arrival stamp, never the render clock.
//!
//! The windows are the rhythm-game canon: ±45 ms reads tight, ±90
//! comfortable, ±135 forgiving. The evenness number is the research one —
//! the coefficient of variation of inter-onset intervals, where
//! conservatory hands measure about 7% and the audibility line sits near
//! 8%. Bias (rushing vs dragging) and spread (consistency) are reported
//! separately, the way Yamaha's drum coaches learned to.

use super::TargetNote;

/// How far from the grid an onset may land and still count, by feel tier.
pub const WINDOW_TIGHT_MS: f32 = 45.0;
pub const WINDOW_COMFORTABLE_MS: f32 = 90.0;
pub const WINDOW_FORGIVING_MS: f32 = 135.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Wait,
    Flow,
}

impl Mode {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Wait => "wait",
            Self::Flow => "flow",
        }
    }
}

/// One target's outcome.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HitState {
    Pending,
    /// Hit, with the signed deviation in ms (negative = early). Wait mode
    /// records 0.
    Hit(f32),
    Missed,
}

/// What one incoming note was judged as, for the moment-to-moment readout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    Perfect(f32),
    Good(f32),
    Late(f32),
    Early(f32),
    Wrong(u8),
}

impl Verdict {
    #[must_use]
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Perfect(_) => "\u{25cf}",
            Self::Good(_) => "\u{25cb}",
            Self::Early(_) => "\u{25c2}",
            Self::Late(_) => "\u{25b8}",
            Self::Wrong(_) => "\u{00d7}",
        }
    }
}

/// A finished rep, measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RepReport {
    pub hit: usize,
    pub total: usize,
    pub wrong: usize,
    /// Mean signed deviation — the bias. Positive drags, negative rushes.
    pub bias_ms: f32,
    /// Standard deviation of the deviations — the spread.
    pub spread_ms: f32,
    /// Coefficient of variation of inter-onset intervals, percent.
    /// Conservatory hands: ~7. Audibly uneven: >8. Meaningless in wait mode.
    pub ioi_cv: f32,
    pub clean: bool,
}

#[derive(Debug)]
pub struct Judge {
    targets: Vec<TargetNote>,
    pub mode: Mode,
    pub window_ms: f32,
    status: Vec<HitState>,
    /// Wait mode: index of the first target of the group the player is on.
    pub cursor: usize,
    /// Notes of the current wait-mode group already down.
    group_down: Vec<u8>,
    pub wrong: usize,
    /// (target index, arrival micros) for every hit, for the IOI math.
    onsets: Vec<(usize, u64)>,
    /// Flow mode: the micros at which exercise tick 0 falls.
    anchor_micros: u64,
    micros_per_tick: f64,
    pub last_verdict: Option<Verdict>,
}

impl Judge {
    #[must_use]
    pub fn new(targets: Vec<TargetNote>, mode: Mode, bpm: u32, window_ms: f32) -> Self {
        let micros_per_tick = 60_000_000.0 / (f64::from(bpm) * 960.0);
        let len = targets.len();
        Self {
            targets,
            mode,
            window_ms,
            status: vec![HitState::Pending; len],
            cursor: 0,
            group_down: Vec::new(),
            wrong: 0,
            onsets: Vec::new(),
            anchor_micros: 0,
            micros_per_tick,
            last_verdict: None,
        }
    }

    /// Arm flow mode: tick 0 of the exercise falls at `anchor_micros`.
    pub fn set_anchor(&mut self, anchor_micros: u64) {
        self.anchor_micros = anchor_micros;
    }

    #[must_use]
    pub fn targets(&self) -> &[TargetNote] {
        &self.targets
    }

    #[must_use]
    pub fn status(&self, index: usize) -> HitState {
        self.status.get(index).copied().unwrap_or(HitState::Missed)
    }

    /// The indices of the wait-mode group under the cursor — every target
    /// sharing the cursor's tick.
    #[must_use]
    pub fn current_group(&self) -> Vec<usize> {
        let Some(first) = self.targets.get(self.cursor) else { return Vec::new() };
        let tick = first.tick;
        (self.cursor..self.targets.len())
            .take_while(|&i| self.targets[i].tick == tick)
            .collect()
    }

    /// Whether every target is resolved.
    #[must_use]
    pub fn done(&self) -> bool {
        match self.mode {
            Mode::Wait => self.cursor >= self.targets.len(),
            Mode::Flow => self.status.iter().all(|s| !matches!(s, HitState::Pending)),
        }
    }

    /// A note arrived. `micros` is its receipt stamp.
    pub fn note_on(&mut self, note: u8, micros: u64) {
        match self.mode {
            Mode::Wait => self.wait_note(note, micros),
            Mode::Flow => self.flow_note(note, micros),
        }
    }

    fn wait_note(&mut self, note: u8, micros: u64) {
        let group = self.current_group();
        if group.is_empty() {
            return;
        }
        let wanted = group
            .iter()
            .find(|&&i| self.targets[i].note == note && !self.group_down.contains(&note));
        match wanted {
            Some(&i) => {
                self.group_down.push(note);
                self.status[i] = HitState::Hit(0.0);
                self.onsets.push((i, micros));
                self.last_verdict = Some(Verdict::Perfect(0.0));
                if self.group_down.len() >= group.len() {
                    self.cursor = group.last().map_or(self.targets.len(), |&l| l + 1);
                    self.group_down.clear();
                }
            }
            None => {
                self.wrong += 1;
                self.last_verdict = Some(Verdict::Wrong(note));
            }
        }
    }

    fn flow_note(&mut self, note: u8, micros: u64) {
        // The nearest pending target of this pitch inside the window.
        let mut best: Option<(usize, f32)> = None;
        for (i, t) in self.targets.iter().enumerate() {
            if t.note != note || !matches!(self.status[i], HitState::Pending) {
                continue;
            }
            let expected = self.anchor_micros as f64 + t.tick as f64 * self.micros_per_tick;
            let dev_ms = (micros as f64 - expected) as f32 / 1000.0;
            if dev_ms.abs() <= self.window_ms
                && best.is_none_or(|(_, b)| dev_ms.abs() < b.abs())
            {
                best = Some((i, dev_ms));
            }
        }
        match best {
            Some((i, dev)) => {
                self.status[i] = HitState::Hit(dev);
                self.onsets.push((i, micros));
                self.last_verdict = Some(if dev.abs() <= self.window_ms * 0.25 {
                    Verdict::Perfect(dev)
                } else if dev.abs() <= self.window_ms * 0.6 {
                    Verdict::Good(dev)
                } else if dev < 0.0 {
                    Verdict::Early(dev)
                } else {
                    Verdict::Late(dev)
                });
            }
            None => {
                self.wrong += 1;
                self.last_verdict = Some(Verdict::Wrong(note));
            }
        }
    }

    /// Flow housekeeping: targets whose window has closed unplayed become
    /// misses. `now_micros` is the current clock.
    pub fn expire(&mut self, now_micros: u64) {
        if self.mode != Mode::Flow {
            return;
        }
        for (i, t) in self.targets.iter().enumerate() {
            if !matches!(self.status[i], HitState::Pending) {
                continue;
            }
            let expected = self.anchor_micros as f64 + t.tick as f64 * self.micros_per_tick;
            if now_micros as f64 > expected + f64::from(self.window_ms) * 1000.0 {
                self.status[i] = HitState::Missed;
            }
        }
    }

    /// Measure the rep. `clean_window_ms` is the pass band for the bias.
    #[must_use]
    pub fn report(&self, clean_window_ms: f32) -> RepReport {
        let total = self.targets.len();
        let hit = self.status.iter().filter(|s| matches!(s, HitState::Hit(_))).count();
        let devs: Vec<f32> = self
            .status
            .iter()
            .filter_map(|s| if let HitState::Hit(d) = s { Some(*d) } else { None })
            .collect();
        let bias = if devs.is_empty() { 0.0 } else { devs.iter().sum::<f32>() / devs.len() as f32 };
        let spread = if devs.len() < 2 {
            0.0
        } else {
            let var =
                devs.iter().map(|d| (d - bias) * (d - bias)).sum::<f32>() / (devs.len() - 1) as f32;
            var.sqrt()
        };

        // Evenness: CV of consecutive-hit inter-onset intervals, over
        // uniformly spaced stretches only (unequal target spacing would
        // read as unevenness the player did not produce).
        let mut iois: Vec<f32> = Vec::new();
        let mut sorted = self.onsets.clone();
        sorted.sort_by_key(|&(i, _)| self.targets[i].tick);
        for pair in sorted.windows(2) {
            let (ia, ta) = pair[0];
            let (ib, tb) = pair[1];
            let dt = self.targets[ib].tick - self.targets[ia].tick;
            if dt <= 0 {
                continue;
            }
            // Only uniform eighth/quarter spacing feeds the evenness number.
            let expected = dt as f64 * self.micros_per_tick;
            let actual = tb.saturating_sub(ta) as f64;
            if actual > 0.0 && (dt == 480 || dt == 960) {
                iois.push((actual / expected) as f32);
            }
        }
        let ioi_cv = if iois.len() < 3 {
            0.0
        } else {
            let mean = iois.iter().sum::<f32>() / iois.len() as f32;
            let var = iois.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>()
                / (iois.len() - 1) as f32;
            (var.sqrt() / mean) * 100.0
        };

        let clean = match self.mode {
            Mode::Wait => hit == total && self.wrong == 0,
            Mode::Flow => {
                hit == total && self.wrong == 0 && bias.abs() <= clean_window_ms && spread <= clean_window_ms
            }
        };
        RepReport { hit, total, wrong: self.wrong, bias_ms: bias, spread_ms: spread, ioi_cv, clean }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::practice::{build, Family, Hand, Hands};

    fn micros_at(bpm: u32, tick: i64) -> u64 {
        (tick as f64 * 60_000_000.0 / (f64::from(bpm) * 960.0)) as u64
    }

    /// Wait mode: right notes advance, wrong notes count and do not; a
    /// chord group needs every note down before the cursor moves.
    #[test]
    fn wait_mode_holds_the_door() {
        let ex = build(Family::Shell251, 0, Hands::Left);
        let first_group: Vec<u8> =
            ex.notes.iter().take_while(|n| n.tick == ex.notes[0].tick).map(|n| n.note).collect();
        assert_eq!(first_group.len(), 3, "a shell is three voices");
        let mut judge = Judge::new(ex.notes.clone(), Mode::Wait, 60, WINDOW_COMFORTABLE_MS);

        judge.note_on(1, 0); // wrong
        assert_eq!(judge.wrong, 1);
        assert_eq!(judge.cursor, 0, "a wrong note moved the cursor");

        judge.note_on(first_group[0], 10);
        judge.note_on(first_group[1], 20);
        assert_eq!(judge.cursor, 0, "the cursor moved before the chord was whole");
        judge.note_on(first_group[2], 30);
        assert_eq!(judge.cursor, 3, "the finished chord did not advance");
    }

    /// Flow mode: deviations are measured from the anchor, verdicts tier
    /// by fractions of the window, and a note outside it is wrong.
    #[test]
    fn flow_mode_measures_the_grid() {
        let ex = build(Family::MajorScale, 0, Hands::Right);
        let bpm = 60;
        let mut judge = Judge::new(ex.notes.clone(), Mode::Flow, bpm, WINDOW_COMFORTABLE_MS);
        judge.set_anchor(1_000_000);

        // First note (tick 0) 10 ms late: Perfect at a 90 ms window.
        judge.note_on(ex.notes[0].note, 1_000_000 + 10_000);
        assert!(matches!(judge.last_verdict, Some(Verdict::Perfect(d)) if (d - 10.0).abs() < 0.5));

        // Second note 70 ms early: Early.
        let t1 = 1_000_000 + micros_at(bpm, ex.notes[1].tick);
        judge.note_on(ex.notes[1].note, t1 - 70_000);
        assert!(matches!(judge.last_verdict, Some(Verdict::Early(_))));

        // A pitch nowhere near a pending target: wrong.
        judge.note_on(119, t1);
        assert_eq!(judge.wrong, 1);

        // Unplayed targets expire once the clock passes their window.
        let last_tick = ex.notes.last().unwrap().tick;
        judge.expire(1_000_000 + micros_at(bpm, last_tick) + 200_000);
        assert!(judge.done(), "expiry did not resolve the rep");
        let report = judge.report(45.0);
        assert_eq!(report.hit, 2);
        assert!(!report.clean);
    }

    /// A metronomic run reads clean: full hits, near-zero bias and spread,
    /// evenness far under the audibility line.
    #[test]
    fn a_perfect_run_reads_clean() {
        let ex = build(Family::MajorScale, 0, Hands::Right);
        let bpm = 96;
        let mut judge = Judge::new(ex.notes.clone(), Mode::Flow, bpm, WINDOW_COMFORTABLE_MS);
        judge.set_anchor(500_000);
        for t in &ex.notes {
            judge.note_on(t.note, 500_000 + micros_at(bpm, t.tick));
        }
        let report = judge.report(45.0);
        assert!(report.clean, "a perfect run failed: {report:?}");
        assert!(report.bias_ms.abs() < 1.0);
        assert!(report.ioi_cv < 1.0, "perfect spacing read uneven: {}", report.ioi_cv);
    }

    /// A rushing player reads as bias, an uneven one as spread — the two
    /// faults the report keeps apart.
    #[test]
    fn bias_and_spread_are_told_apart() {
        let ex = build(Family::MajorScale, 0, Hands::Right);
        let bpm = 96;
        // Rushing: every note 40 ms early, perfectly consistently.
        let mut rushing = Judge::new(ex.notes.clone(), Mode::Flow, bpm, WINDOW_COMFORTABLE_MS);
        rushing.set_anchor(1_000_000);
        for t in &ex.notes {
            rushing.note_on(t.note, 1_000_000 + micros_at(bpm, t.tick) - 40_000);
        }
        let r = rushing.report(45.0);
        assert!(r.bias_ms < -35.0, "rushing did not read as negative bias: {r:?}");
        assert!(r.spread_ms < 5.0, "consistent rushing read as spread: {r:?}");

        // Uneven: alternately 30 ms early and late — no bias, all spread.
        let mut uneven = Judge::new(ex.notes.clone(), Mode::Flow, bpm, WINDOW_COMFORTABLE_MS);
        uneven.set_anchor(1_000_000);
        for (k, t) in ex.notes.iter().enumerate() {
            let jitter: i64 = if k % 2 == 0 { -30_000 } else { 30_000 };
            uneven.note_on(t.note, (1_000_000 + micros_at(bpm, t.tick)) .saturating_add_signed(jitter));
        }
        let u = uneven.report(45.0);
        assert!(u.bias_ms.abs() < 5.0, "alternating jitter read as bias: {u:?}");
        assert!(u.spread_ms > 25.0, "alternating jitter did not read as spread: {u:?}");
        assert!(u.ioi_cv > 8.0, "audible unevenness read under the line: {}", u.ioi_cv);
    }

    /// The curriculum's fingering data holds its invariants: no thumb on a
    /// black key in any major or minor scale, in any key, either hand.
    #[test]
    fn no_scale_puts_the_thumb_on_a_black_key() {
        for family in [Family::MajorScale, Family::MinorScale] {
            for key in 0..12u8 {
                for hands in [Hands::Right, Hands::Left] {
                    let ex = build(family, key, hands);
                    for n in &ex.notes {
                        let black = matches!(n.note % 12, 1 | 3 | 6 | 8 | 10);
                        assert!(
                            !(black && n.finger == 1),
                            "{}: thumb on a black key (note {}, {:?})",
                            ex.id,
                            n.note,
                            n.hand
                        );
                    }
                }
            }
        }
    }

    /// Hands-together scales keep both hands on the same tick everywhere —
    /// the display and the judge both lean on it.
    #[test]
    fn hands_together_lines_up() {
        let ex = build(Family::MajorScale, 7, Hands::Together);
        let rh: Vec<i64> =
            ex.notes.iter().filter(|n| n.hand == Hand::Right).map(|n| n.tick).collect();
        let lh: Vec<i64> =
            ex.notes.iter().filter(|n| n.hand == Hand::Left).map(|n| n.tick).collect();
        assert_eq!(rh, lh, "the hands drifted apart");
    }

    /// Every exercise in the whole curriculum builds, is non-empty, sits
    /// inside MIDI range, and spans whole bars.
    #[test]
    fn the_whole_curriculum_builds() {
        for family in Family::ALL {
            let keys: Vec<u8> = if family.keyed() { (0..12).collect() } else { vec![0] };
            for key in keys {
                let hands: Vec<Hands> = if family.handed() {
                    Hands::ALL.to_vec()
                } else {
                    vec![Hands::Left]
                };
                for h in hands {
                    let ex = build(family, key, h);
                    assert!(!ex.notes.is_empty(), "{} is empty", ex.id);
                    assert!(ex.loop_ticks % (960 * 4) == 0, "{} is not whole bars", ex.id);
                    for n in &ex.notes {
                        assert!((21..=108).contains(&n.note), "{} leaves the keyboard: {}", ex.id, n.note);
                        assert!(n.tick >= 0 && n.tick < ex.loop_ticks, "{} overflows its loop", ex.id);
                    }
                }
            }
        }
    }
}
