//! The plain state-variable filter: one two-pole section, all three outputs.
//!
//! The topology-preserving (trapezoidal) form — stable at any cutoff below
//! Nyquist, and free of the zipper a naive biquad makes when its corner
//! moves. This is the unvoiced one: no clamps, no saturation, no denormal
//! floors. Instruments that need a character of their own keep their own
//! (the TEO-5's SEM morph, the compressor's sidechain with its floors); the
//! drum rack and the sampler's chop analysis want exactly this and share it.

use std::f64::consts::PI;

/// The coefficients one corner and one Q name.
///
/// Worth holding when the corner stands still: `tan` per sample is most of
/// what a fixed filter costs, and an analysis pass over ten minutes of audio
/// runs it tens of millions of times.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvfCoeffs {
    k: f64,
    a1: f64,
    a2: f64,
    a3: f64,
}

impl SvfCoeffs {
    /// `cutoff` and `sr` in Hz; `q` of 0.707 is the flat Butterworth corner.
    #[must_use]
    pub fn new(cutoff: f64, q: f64, sr: f64) -> Self {
        let g = (PI * cutoff / sr).tan();
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        Self { k, a1, a2, a3 }
    }
}

/// One two-pole section's memory.
#[derive(Debug, Clone, Copy, Default)]
pub struct Svf {
    ic1eq: f64,
    ic2eq: f64,
}

impl Svf {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One sample through fixed coefficients: `(low, band, high)`.
    #[inline]
    pub fn tick_with(&mut self, x: f64, c: SvfCoeffs) -> (f64, f64, f64) {
        let v3 = x - self.ic2eq;
        let v1 = c.a1 * self.ic1eq + c.a2 * v3;
        let v2 = self.ic2eq + c.a2 * self.ic1eq + c.a3 * v3;

        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        (v2, v1, x - c.k * v1 - v2)
    }

    /// One sample with the corner named afresh — for a corner that moves.
    #[inline]
    pub fn tick(&mut self, x: f64, cutoff: f64, q: f64, sr: f64) -> (f64, f64, f64) {
        self.tick_with(x, SvfCoeffs::new(cutoff, q, sr))
    }

    pub fn bandpass(&mut self, x: f64, cutoff: f64, q: f64, sr: f64) -> f64 {
        self.tick(x, cutoff, q, sr).1
    }

    pub fn lowpass(&mut self, x: f64, cutoff: f64, q: f64, sr: f64) -> f64 {
        self.tick(x, cutoff, q, sr).0
    }

    pub fn highpass(&mut self, x: f64, cutoff: f64, q: f64, sr: f64) -> f64 {
        self.tick(x, cutoff, q, sr).2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    /// Steady-state peak of a sine at `hz` through one output.
    fn gain_at(hz: f64, pick: fn((f64, f64, f64)) -> f64) -> f64 {
        let c = SvfCoeffs::new(1_000.0, std::f64::consts::FRAC_1_SQRT_2, SR);
        let mut f = Svf::new();
        let n = (SR * 0.5) as usize;
        let mut peak = 0.0f64;
        for i in 0..n {
            let x = (std::f64::consts::TAU * hz * i as f64 / SR).sin();
            let y = pick(f.tick_with(x, c));
            if i > n / 2 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn each_output_passes_its_own_band() {
        let low = |t: (f64, f64, f64)| t.0;
        let high = |t: (f64, f64, f64)| t.2;
        assert!(gain_at(100.0, low) > 0.95, "the low pass cut its pass band");
        assert!(gain_at(10_000.0, low) < 0.02, "the low pass let the top through");
        assert!(gain_at(10_000.0, high) > 0.95, "the high pass cut its pass band");
        assert!(gain_at(100.0, high) < 0.02, "the high pass let the bottom through");
        let corner = gain_at(1_000.0, low);
        assert!((corner - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.02, "corner at {corner}");
    }

    /// Held coefficients and fresh ones are the same filter, bit for bit —
    /// the drum rack moved onto this without its sound moving at all.
    #[test]
    fn held_and_fresh_coefficients_agree_exactly() {
        let c = SvfCoeffs::new(2_345.0, 3.0, SR);
        let (mut a, mut b) = (Svf::new(), Svf::new());
        for i in 0..2_000 {
            let x = ((i * 7919) % 101) as f64 / 50.0 - 1.0;
            assert_eq!(a.tick_with(x, c), b.tick(x, 2_345.0, 3.0, SR));
        }
    }
}
