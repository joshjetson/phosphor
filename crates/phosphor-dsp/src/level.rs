//! Output level control shared by the built-in instruments.
//!
//! Two separate jobs, deliberately kept apart:
//!
//! * **Headroom.** Each instrument multiplies its voice sum by a fixed
//!   `OUTPUT_TRIM` before it leaves `process()`. The trims are measured
//!   values — see the constant in each synth — picked so that *ordinary
//!   playing* (the preset the instrument loads with, a triad at velocity 100)
//!   peaks near −12 dBFS, and so that the five keyboards sit at the same
//!   typical loudness. The drum rack is matched on peak instead of RMS, for
//!   the reason given at its own constant. The trim is a constant: nothing
//!   here divides by the number of sounding voices, because a divisor that
//!   moves as notes are released pumps audibly.
//!
//!   The trims are deliberately *not* sized on the loudest input the
//!   instruments accept. Sizing to the rarest case — an eight-note chord at
//!   velocity 127 on the hottest patch in the bank — costs every note the
//!   user actually plays 3.6 dB, which is what made the instruments too
//!   quiet to use without the OS volume control.
//!
//! * **Bounding.** Whatever is left after the trim goes through
//!   [`soft_saturate`], which is the identity below the knee and bends
//!   smoothly towards ±1 above it. It replaces the `clamp(-1.0, 1.0)` that
//!   used to sit at each output stage: a hard clamp turns a loud chord into
//!   a square wave, and the odd harmonics that produces are what actually
//!   damages tweeters.
//!
//! Neither of these is the safety guarantee — that is the master limiter in
//! the mixer. What these two buy is that the limiter never has to act on a
//! single track: the loudest thing any instrument can produce leaves
//! `process()` under the limiter's ceiling, so gain reduction on the master
//! bus only ever means "several loud tracks at once", never "one instrument
//! playing a chord".

/// Magnitude below which [`soft_saturate`] returns its input untouched.
///
/// −2.5 dBFS. The headroom trims put ordinary playing far under this, so for
/// almost everything the saturator is a wire. Two voicings in the whole
/// project cross it — the DX7's "Timpani" attack and a full drum kit struck
/// on one sample, both at velocity 127 — and they cross by about 1.3 dB,
/// which is the bounding stage doing its job rather than a mis-sized trim.
///
/// Chosen against the master limiter's −1 dBFS ceiling rather than against
/// full scale: an input of 1.07 maps to exactly 0.891 here, so any instrument
/// that stays under 1.07 before the saturator cannot make the limiter act on
/// its own. Every preset in every bank does.
pub const SATURATION_KNEE: f32 = 0.75;

/// Width of the bending region, from the knee up to the ±1 asymptote.
const SATURATION_RANGE: f32 = 1.0 - SATURATION_KNEE;

/// The f32 immediately below full scale, `1 - 2^-24`.
///
/// The curve is bounded by 1 in exact arithmetic, but `0.75 + 0.25 * t`
/// rounds up to exactly 1.0 once `t` is within half an ulp of 1, which
/// happens for inputs past roughly 1e6. Pinning here keeps "output is never
/// at full scale" true in f32 as well as on paper, so a 1.0 appearing
/// anywhere downstream is always a real fault rather than the saturator
/// touching its asymptote.
const BELOW_FULL_SCALE: f32 = 1.0 - f32::EPSILON / 2.0;

/// Soft saturator: transparent below the knee, asymptotically bounded by ±1.
///
/// Above the knee the excess is passed through `u / (1 + u)`, so with
/// `k = SATURATION_KNEE` and `r = 1 - k`:
///
/// ```text
/// |x| <= k :  y = x
/// |x| >  k :  y = k + r * u / (1 + u),   u = (|x| - k) / r
/// ```
///
/// Properties that matter here:
///
/// * **Bit-transparent below the knee.** The branch returns `x` itself, so a
///   signal that never reaches the knee is unchanged sample for sample —
///   no timbre change to anything that was not already clipping.
/// * **C1-continuous at the knee.** `d/dx [k + r·u/(1+u)] = 1/(1+u)^2`,
///   which is exactly 1 at `u = 0`, matching the identity branch's slope.
/// * **Monotonic.** `u/(1+u)` is strictly increasing on `u >= 0`, so the
///   curve never folds back — a fold would generate the same kind of
///   high-order harmonics as the clamp it replaces.
/// * **Bounded.** `u/(1+u) < 1` for all finite `u >= 0`, so `|y| < 1`
///   strictly; the output approaches full scale but cannot reach it.
///
/// A rational curve rather than `tanh` because it is a divide instead of a
/// libm call, and the difference between the two is inaudible this close to
/// the asymptote.
///
/// Non-finite input returns silence. A NaN that reaches the audio device is
/// a full-scale noise burst, so it is stopped at the first stage that can
/// see it rather than passed on.
#[inline]
#[must_use]
pub fn soft_saturate(x: f32) -> f32 {
    let mag = x.abs();
    if mag <= SATURATION_KNEE {
        return x;
    }
    if !mag.is_finite() {
        return 0.0;
    }
    let u = (mag - SATURATION_KNEE) / SATURATION_RANGE;
    let y = SATURATION_KNEE + SATURATION_RANGE * (u / (1.0 + u));
    y.min(BELOW_FULL_SCALE).copysign(x)
}

/// The input [`soft_saturate`] would have needed to produce `y`.
///
/// Not part of the audio path — this is a measurement aid, and it exists
/// because "how hard is the bounding stage working?" is the question that
/// decides whether a headroom trim is right. Given a peak read off an
/// instrument's output, this recovers the trimmed voice sum behind it, and
/// the ratio of the two is the gain reduction the saturator applied.
///
/// Well defined because [`soft_saturate`] is strictly monotonic. Inverting
/// `t = u / (1 + u)` gives `u = t / (1 - t)`, so with `k = SATURATION_KNEE`,
/// `r = 1 - k` and `t = (y - k) / r` the input is `k + r * u`. Below the knee
/// the forward map is the identity, so this is too.
///
/// `y` at or above full scale has no finite preimage and returns infinity;
/// the saturator never produces such a value, so seeing one means the
/// measurement did not come from a saturated output.
#[inline]
#[must_use]
pub fn saturation_input(y: f32) -> f32 {
    let mag = y.abs();
    if mag <= SATURATION_KNEE {
        return y;
    }
    let t = (mag - SATURATION_KNEE) / SATURATION_RANGE;
    if t >= 1.0 {
        return f32::INFINITY.copysign(y);
    }
    (SATURATION_KNEE + SATURATION_RANGE * (t / (1.0 - t))).copysign(y)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Below the knee the saturator must return the input bit for bit —
    /// not "close to", identical. This is what lets it be dropped into an
    /// output stage without altering any patch that was not already clipping.
    #[test]
    fn transparent_below_knee_bit_identical() {
        // Walk the f32 bit patterns from 0 up to the knee, both signs, at a
        // stride that still visits every exponent and a spread of mantissas.
        let top = SATURATION_KNEE.to_bits();
        let mut bits = 0u32;
        let mut checked = 0u64;
        while bits <= top {
            for x in [f32::from_bits(bits), -f32::from_bits(bits)] {
                let y = soft_saturate(x);
                assert_eq!(
                    y.to_bits(),
                    x.to_bits(),
                    "saturator altered {x:e} below the knee"
                );
                checked += 1;
            }
            bits += 1_009; // prime stride: no aliasing with mantissa layout
        }
        assert!(checked > 2_000_000, "sweep was too sparse: {checked}");
        // And the endpoints exactly.
        assert_eq!(soft_saturate(SATURATION_KNEE).to_bits(), SATURATION_KNEE.to_bits());
        assert_eq!(soft_saturate(-SATURATION_KNEE).to_bits(), (-SATURATION_KNEE).to_bits());
        assert_eq!(soft_saturate(0.0).to_bits(), 0.0f32.to_bits());
        assert_eq!(soft_saturate(-0.0).to_bits(), (-0.0f32).to_bits());
    }

    #[test]
    fn bounded_above_knee() {
        let mut x = SATURATION_KNEE;
        while x < 1.0e6 {
            let y = soft_saturate(x);
            assert!(y < 1.0, "saturate({x}) = {y} reached full scale");
            assert!(soft_saturate(-x) > -1.0);
            x *= 1.0009;
        }
        assert!(soft_saturate(f32::MAX) < 1.0);
        assert!(soft_saturate(-f32::MAX) > -1.0);
    }

    #[test]
    fn monotonic() {
        let mut prev = f32::NEG_INFINITY;
        let mut x = -4.0f32;
        while x <= 4.0 {
            let y = soft_saturate(x);
            assert!(y >= prev, "not monotonic at {x}: {y} < {prev}");
            prev = y;
            x += 0.0001;
        }
    }

    /// C1 at the knee: the slope approaching from below is 1, and the slope
    /// leaving from above is 1 as well, so there is no corner to hear.
    #[test]
    fn slope_continuous_at_knee() {
        let h = 1.0e-4f32;
        let below = (soft_saturate(SATURATION_KNEE) - soft_saturate(SATURATION_KNEE - h)) / h;
        let above = (soft_saturate(SATURATION_KNEE + h) - soft_saturate(SATURATION_KNEE)) / h;
        assert!((below - 1.0).abs() < 1.0e-3, "slope below knee = {below}");
        assert!((above - 1.0).abs() < 1.0e-2, "slope above knee = {above}");
        assert!((below - above).abs() < 1.0e-2, "corner at the knee: {below} vs {above}");
    }

    #[test]
    fn non_finite_becomes_silence() {
        assert_eq!(soft_saturate(f32::NAN), 0.0);
        assert_eq!(soft_saturate(f32::INFINITY), 0.0);
        assert_eq!(soft_saturate(f32::NEG_INFINITY), 0.0);
    }

    /// The inverse has to be an inverse, or every gain-reduction figure it
    /// produces is wrong — including the ones the headroom test asserts on.
    #[test]
    fn saturation_input_round_trips() {
        let mut x = -8.0f32;
        while x <= 8.0 {
            let round_trip = saturation_input(soft_saturate(x));
            let tolerance = 1.0e-3 * x.abs().max(1.0);
            assert!(
                (round_trip - x).abs() < tolerance,
                "round trip of {x} came back as {round_trip}"
            );
            x += 0.0011;
        }
        // Below the knee it is the identity in both directions.
        assert_eq!(saturation_input(0.5).to_bits(), 0.5f32.to_bits());
        assert_eq!(saturation_input(-0.5).to_bits(), (-0.5f32).to_bits());
        assert_eq!(saturation_input(SATURATION_KNEE).to_bits(), SATURATION_KNEE.to_bits());
        // Full scale is not in the range of the saturator.
        assert_eq!(saturation_input(1.0), f32::INFINITY);
        assert_eq!(saturation_input(-1.0), f32::NEG_INFINITY);
    }
}
