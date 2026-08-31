//! Gain reduction, as a number a meter can draw.
//!
//! Built once and used by everything that reduces gain: the master limiter
//! today, the compressor when it lands. Both need the same two answers —
//! "how much is it taking off right now" and "what is the worst it took" —
//! and both have to compute them **on the audio thread**.
//!
//! That last part is the whole design. A UI that samples an atomic on a
//! redraw timer sees one instant in every sixteen milliseconds; gain
//! reduction on a snare hit is two milliseconds long. The meter would show
//! nothing at all on the transients that matter and a random subset of the
//! ones that do not. So the audio thread — which sees every sample — keeps
//! the ballistics: it takes the worst reduction in the block, applies the
//! visual release and the peak hold, and publishes a value that is already
//! ready to draw. The UI reads two atomics and paints them.

use std::sync::atomic::{AtomicU32, Ordering};

/// The lowest reduction the ballistics will represent.
///
/// The meter's scale stops at −20 dB, so this is only here to keep a gain of
/// zero — a muted stage, a fader at the bottom — from arriving as negative
/// infinity and poisoning the one-pole below.
pub const GR_FLOOR_DB: f32 = -60.0;

/// How long the display takes to give back reduction it is showing, as a
/// one-pole time constant.
///
/// 300 ms is the usual visual release for a gain-reduction meter: fast
/// enough to follow a compressor working, slow enough that the eye reads a
/// level rather than a flicker. It is a *time*, so the decay is the same at
/// every sample rate and — because a one-pole composes,
/// `e^(-a/τ)·e^(-b/τ) = e^(-(a+b)/τ)` — at every block size.
pub const GR_RELEASE_SECONDS: f32 = 0.3;

/// How long the peak cell holds before it falls back to the bar.
pub const GR_PEAK_HOLD_SECONDS: f32 = 1.5;

/// Below this much reduction the meter reads nothing at all.
///
/// A hundredth of a decibel is two orders of magnitude under the readout's
/// one decimal place, and it is where a one-pole release spends the rest of
/// its life: without a floor, a limiter that engaged once would leave the
/// meter lit forever.
pub const GR_IDLE_DB: f32 = 0.01;

/// What the UI reads: two decibel values, published by the audio thread.
///
/// Both are ≤ 0, and both are already display-ready — no smoothing, no
/// conversion and no ballistics are left for the reader to do.
#[derive(Debug)]
pub struct GrMeter {
    /// The bar: current reduction after the visual release.
    current: AtomicU32,
    /// The cell: the worst reduction inside the hold window.
    peak: AtomicU32,
}

impl Default for GrMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl GrMeter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            current: AtomicU32::new(0.0f32.to_bits()),
            peak: AtomicU32::new(0.0f32.to_bits()),
        }
    }

    /// `(current, peak)` in decibels of reduction, both ≤ 0.
    #[must_use]
    pub fn get(&self) -> (f32, f32) {
        (
            f32::from_bits(self.current.load(Ordering::Relaxed)),
            f32::from_bits(self.peak.load(Ordering::Relaxed)),
        )
    }

    /// Current reduction in decibels, ≤ 0.
    #[must_use]
    pub fn current_db(&self) -> f32 {
        self.get().0
    }

    /// Whether anything is being taken off at all — the threshold below which
    /// a meter should draw nothing rather than a sliver.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.current_db() <= -0.1
    }

    fn publish(&self, current: f32, peak: f32) {
        self.current.store(current.to_bits(), Ordering::Relaxed);
        self.peak.store(peak.to_bits(), Ordering::Relaxed);
    }

    /// Back to rest. The panic path, and anything that drops a tail.
    pub fn reset(&self) {
        self.publish(0.0, 0.0);
    }
}

/// The audio thread's half: the ballistics that turn a block of gain into a
/// value a meter can draw.
///
/// Three floats of state, no allocation, no branches that can panic.
#[derive(Debug, Clone, Copy)]
pub struct GrBallistics {
    /// What the bar is showing, in dB, ≤ 0.
    shown_db: f32,
    /// What the cell is showing, in dB, ≤ `shown_db`.
    peak_db: f32,
    /// Seconds of hold left on the cell.
    hold_left: f32,
}

impl Default for GrBallistics {
    fn default() -> Self {
        Self::new()
    }
}

impl GrBallistics {
    #[must_use]
    pub fn new() -> Self {
        Self {
            shown_db: 0.0,
            peak_db: 0.0,
            hold_left: 0.0,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    #[must_use]
    pub fn current_db(&self) -> f32 {
        self.shown_db
    }

    #[must_use]
    pub fn peak_db(&self) -> f32 {
        self.peak_db
    }

    /// Fold one block into the display value.
    ///
    /// `block_min_gain` is the smallest linear gain the stage applied
    /// anywhere in the block — the worst moment, not the average, because a
    /// meter that averaged would never show a transient at all.
    ///
    /// Attack is instant: reduction that happened is shown on the block it
    /// happened in. Release is the one-pole above. Both ends of that are
    /// deliberate — a meter that lags the attack lies about what the stage
    /// did, and one that snaps back on release flickers.
    pub fn observe(&mut self, block_min_gain: f32, frames: usize, sample_rate: f32) {
        let reduction = if block_min_gain.is_finite() && block_min_gain > 0.0 {
            let db = (20.0 * block_min_gain.log10()).clamp(GR_FLOOR_DB, 0.0);
            // A hundredth of a decibel is not gain reduction, it is a
            // one-pole release that is still asymptotically approaching
            // unity. Without this floor a limiter that engaged once leaves
            // the meter permanently lit at −0.0006 dB, because its gain
            // never exactly reaches one again.
            if db > -GR_IDLE_DB {
                0.0
            } else {
                db
            }
        } else if block_min_gain >= 1.0 {
            0.0
        } else {
            // A gain of zero or a NaN out of a diverging stage. Neither is a
            // reduction the meter can name, and the floor is the honest
            // answer for the first.
            GR_FLOOR_DB
        };

        let dt = if sample_rate > 0.0 {
            frames as f32 / sample_rate
        } else {
            0.0
        };

        if reduction < self.shown_db {
            self.shown_db = reduction;
        } else {
            // `shown *= e^(-dt/τ)` written as a one-pole step, which is the
            // same thing and composes exactly across block sizes.
            let decay = (-dt / GR_RELEASE_SECONDS).exp();
            self.shown_db *= decay;
            if self.shown_db > -GR_IDLE_DB {
                self.shown_db = 0.0;
            }
        }

        if self.shown_db < self.peak_db {
            self.peak_db = self.shown_db;
            self.hold_left = GR_PEAK_HOLD_SECONDS;
        } else {
            self.hold_left -= dt;
            if self.hold_left <= 0.0 {
                self.peak_db = self.shown_db;
                self.hold_left = 0.0;
            }
        }
    }

    /// Fold one block in and hand the result to the UI.
    pub fn publish(&mut self, meter: &GrMeter, block_min_gain: f32, frames: usize, sample_rate: f32) {
        self.observe(block_min_gain, frames, sample_rate);
        meter.publish(self.shown_db, self.peak_db);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reduction that happened inside the block is shown for that block. A
    /// meter that smoothed the attack would miss every transient, which is
    /// the only thing anyone looks at a gain-reduction meter for.
    #[test]
    fn the_attack_is_instant() {
        let mut gr = GrBallistics::new();
        gr.observe(0.5, 64, 44_100.0); // −6.02 dB
        assert!(
            (gr.current_db() - (-6.0206)).abs() < 0.01,
            "shown {} dB after a 6 dB reduction",
            gr.current_db()
        );
        assert!((gr.peak_db() - gr.current_db()).abs() < 1.0e-6);
    }

    /// The release is a time constant, and the same one however the audio is
    /// chopped up. This is the property a UI-side timer cannot have.
    #[test]
    fn the_release_does_not_depend_on_block_size_or_rate() {
        let mut results = Vec::new();
        for (rate, frames) in [(44_100.0f32, 32usize), (44_100.0, 512), (96_000.0, 64), (48_000.0, 1024)] {
            let mut gr = GrBallistics::new();
            gr.observe(0.5, frames, rate);
            let start = gr.current_db();

            // Exactly one time constant of quiet.
            let blocks = (GR_RELEASE_SECONDS * rate / frames as f32).round() as usize;
            for _ in 0..blocks {
                gr.observe(1.0, frames, rate);
            }
            results.push(gr.current_db() / start);
        }
        for ratio in &results {
            assert!(
                (ratio - std::f32::consts::E.recip()).abs() < 0.02,
                "one time constant left {ratio:.4} of the reduction, not 1/e"
            );
        }
        let spread = results
            .iter()
            .fold(0.0f32, |worst, r| worst.max((r - results[0]).abs()));
        assert!(spread < 0.01, "block size and rate changed the decay by {spread:.4}");
    }

    /// The cell holds the worst moment for a second and a half, then falls
    /// back to the bar. Without the hold, a two-millisecond transient is a
    /// value nobody ever sees.
    #[test]
    fn the_peak_cell_holds_then_falls_back() {
        let rate = 44_100.0f32;
        let frames = 64usize;
        let mut gr = GrBallistics::new();
        gr.observe(0.25, frames, rate); // −12 dB
        let worst = gr.peak_db();
        assert!((worst - (-12.04)).abs() < 0.05);

        // Half the hold window of silence: the bar has fallen a long way, the
        // cell has not moved.
        let blocks = (0.75 * rate / frames as f32) as usize;
        for _ in 0..blocks {
            gr.observe(1.0, frames, rate);
        }
        assert!(gr.current_db() > worst + 6.0, "the bar did not release");
        assert_eq!(gr.peak_db(), worst, "the cell moved inside the hold window");

        // Past the window: the cell rejoins the bar.
        for _ in 0..blocks * 2 {
            gr.observe(1.0, frames, rate);
        }
        assert!(
            (gr.peak_db() - gr.current_db()).abs() < 1.0e-6,
            "the cell never fell back: {} vs {}",
            gr.peak_db(),
            gr.current_db()
        );
    }

    /// A stage at unity publishes nothing, and one that has fully released
    /// comes back to exactly zero rather than to a hundredth of a decibel
    /// that keeps a meter lit forever.
    #[test]
    fn unity_reads_zero_and_release_reaches_it() {
        let mut gr = GrBallistics::new();
        gr.observe(1.0, 128, 48_000.0);
        assert_eq!(gr.current_db(), 0.0);
        assert_eq!(gr.peak_db(), 0.0);

        gr.observe(0.9, 128, 48_000.0);
        for _ in 0..2_000 {
            gr.observe(1.0, 128, 48_000.0);
        }
        assert_eq!(gr.current_db(), 0.0, "the bar never came all the way back");
        assert_eq!(gr.peak_db(), 0.0);
    }

    /// Silence and NaN are not reductions the meter can name; neither may
    /// leave it stuck or reading a value that is not a number.
    #[test]
    fn a_dead_or_diverging_stage_reads_the_floor() {
        let mut gr = GrBallistics::new();
        gr.observe(0.0, 64, 44_100.0);
        assert_eq!(gr.current_db(), GR_FLOOR_DB);

        let mut gr = GrBallistics::new();
        gr.observe(f32::NAN, 64, 44_100.0);
        assert!(gr.current_db().is_finite(), "a NaN reached the meter");
        assert_eq!(gr.current_db(), GR_FLOOR_DB);
    }

    /// What the UI reads is what the audio thread computed, and a reset puts
    /// both back to rest.
    #[test]
    fn publishing_reaches_the_meter() {
        let meter = GrMeter::new();
        assert_eq!(meter.get(), (0.0, 0.0));
        assert!(!meter.is_active());

        let mut gr = GrBallistics::new();
        gr.publish(&meter, 0.5, 64, 44_100.0);
        let (current, peak) = meter.get();
        assert!((current - (-6.0206)).abs() < 0.01);
        assert_eq!(current, peak);
        assert!(meter.is_active());

        meter.reset();
        assert_eq!(meter.get(), (0.0, 0.0));
        gr.reset();
        assert_eq!(gr.current_db(), 0.0);
    }
}
