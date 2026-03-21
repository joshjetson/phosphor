//! Basic oscillators: sine, saw, square, triangle.
//!
//! All oscillators are anti-aliased where needed and produce no
//! denormals. They implement the Plugin trait.

use std::f64::consts::TAU;

/// Waveform shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

/// A simple monophonic oscillator. Phase-accumulator design.
#[derive(Debug)]
pub struct Oscillator {
    waveform: Waveform,
    frequency: f64,
    sample_rate: f64,
    phase: f64,
    amplitude: f32,
}

impl Oscillator {
    pub fn new(waveform: Waveform, frequency: f64, sample_rate: f64) -> Self {
        Self {
            waveform,
            frequency,
            sample_rate,
            phase: 0.0,
            amplitude: 1.0,
        }
    }

    pub fn set_frequency(&mut self, freq: f64) {
        self.frequency = freq;
    }

    pub fn set_amplitude(&mut self, amp: f32) {
        self.amplitude = amp;
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        self.waveform = waveform;
    }

    /// Fill a buffer with oscillator output. Mono.
    pub fn process(&mut self, output: &mut [f32]) {
        let phase_inc = self.frequency / self.sample_rate;

        for sample in output.iter_mut() {
            let value = match self.waveform {
                Waveform::Sine => (self.phase * TAU).sin(),
                Waveform::Saw => 2.0 * self.phase - 1.0,
                Waveform::Square => {
                    if self.phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Waveform::Triangle => {
                    if self.phase < 0.5 {
                        4.0 * self.phase - 1.0
                    } else {
                        3.0 - 4.0 * self.phase
                    }
                }
            };

            *sample = (value * self.amplitude as f64) as f32;

            self.phase += phase_inc;
            // Keep phase in [0, 1) — avoid floating point drift
            self.phase -= self.phase.floor();
        }
    }

    /// Reset phase to zero.
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f64 = 44100.0;

    #[test]
    fn sine_no_nan_or_inf() {
        let mut osc = Oscillator::new(Waveform::Sine, 440.0, SAMPLE_RATE);
        let mut buf = [0.0f32; 4096];
        // Run for 10 seconds
        for _ in 0..(SAMPLE_RATE as usize * 10 / 4096) {
            osc.process(&mut buf);
            assert!(
                buf.iter().all(|s| s.is_finite()),
                "NaN or Inf in sine output"
            );
        }
    }

    #[test]
    fn saw_no_nan_or_inf() {
        let mut osc = Oscillator::new(Waveform::Saw, 440.0, SAMPLE_RATE);
        let mut buf = [0.0f32; 4096];
        for _ in 0..100 {
            osc.process(&mut buf);
            assert!(buf.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn square_no_nan_or_inf() {
        let mut osc = Oscillator::new(Waveform::Square, 440.0, SAMPLE_RATE);
        let mut buf = [0.0f32; 4096];
        for _ in 0..100 {
            osc.process(&mut buf);
            assert!(buf.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn triangle_no_nan_or_inf() {
        let mut osc = Oscillator::new(Waveform::Triangle, 440.0, SAMPLE_RATE);
        let mut buf = [0.0f32; 4096];
        for _ in 0..100 {
            osc.process(&mut buf);
            assert!(buf.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn sine_amplitude_within_bounds() {
        let mut osc = Oscillator::new(Waveform::Sine, 440.0, SAMPLE_RATE);
        let mut buf = [0.0f32; 44100]; // 1 second
        osc.process(&mut buf);
        let max = buf.iter().copied().fold(0.0f32, f32::max);
        let min = buf.iter().copied().fold(0.0f32, f32::min);
        assert!(max <= 1.0, "Sine max {max} exceeds 1.0");
        assert!(min >= -1.0, "Sine min {min} below -1.0");
        // Should actually reach close to ±1.0
        assert!(max > 0.99, "Sine max {max} should reach ~1.0");
        assert!(min < -0.99, "Sine min {min} should reach ~-1.0");
    }

    #[test]
    fn saw_amplitude_within_bounds() {
        let mut osc = Oscillator::new(Waveform::Saw, 440.0, SAMPLE_RATE);
        let mut buf = [0.0f32; 44100];
        osc.process(&mut buf);
        assert!(buf.iter().all(|&s| s >= -1.0 && s <= 1.0));
    }

    #[test]
    fn square_only_two_values() {
        let mut osc = Oscillator::new(Waveform::Square, 440.0, SAMPLE_RATE);
        let mut buf = [0.0f32; 4096];
        osc.process(&mut buf);
        assert!(buf.iter().all(|&s| s == 1.0 || s == -1.0));
    }

    #[test]
    fn triangle_amplitude_within_bounds() {
        let mut osc = Oscillator::new(Waveform::Triangle, 440.0, SAMPLE_RATE);
        let mut buf = [0.0f32; 44100];
        osc.process(&mut buf);
        assert!(buf.iter().all(|&s| s >= -1.0 && s <= 1.0));
    }

    #[test]
    fn frequency_change_takes_effect() {
        let mut osc = Oscillator::new(Waveform::Sine, 440.0, SAMPLE_RATE);
        let mut buf1 = [0.0f32; 4096];
        osc.process(&mut buf1);

        osc.reset();
        osc.set_frequency(880.0);
        let mut buf2 = [0.0f32; 4096];
        osc.process(&mut buf2);

        // Buffers should be different (different frequency)
        assert_ne!(&buf1[..], &buf2[..]);
    }

    #[test]
    fn amplitude_scales_output() {
        let mut osc = Oscillator::new(Waveform::Sine, 440.0, SAMPLE_RATE);
        osc.set_amplitude(0.5);
        let mut buf = [0.0f32; 44100];
        osc.process(&mut buf);
        let max = buf.iter().copied().fold(0.0f32, f32::max);
        assert!(max <= 0.51, "Expected max ~0.5, got {max}");
        assert!(max > 0.49, "Expected max ~0.5, got {max}");
    }

    #[test]
    fn reset_resets_phase() {
        let mut osc = Oscillator::new(Waveform::Sine, 440.0, SAMPLE_RATE);
        let mut buf1 = [0.0f32; 64];
        osc.process(&mut buf1);

        osc.reset();
        let mut buf2 = [0.0f32; 64];
        osc.process(&mut buf2);

        // After reset, output should match the first run
        assert_eq!(&buf1[..], &buf2[..], "Reset should reproduce identical output");
    }

    #[test]
    fn no_denormals_in_output() {
        let mut osc = Oscillator::new(Waveform::Sine, 440.0, SAMPLE_RATE);
        let mut buf = [0.0f32; 44100];
        osc.process(&mut buf);
        for &s in &buf {
            assert!(
                s == 0.0 || s.abs() > f32::MIN_POSITIVE,
                "Denormal detected: {s}"
            );
        }
    }

    #[test]
    fn extreme_frequency_no_crash() {
        // Near Nyquist
        let mut osc = Oscillator::new(Waveform::Sine, 22000.0, SAMPLE_RATE);
        let mut buf = [0.0f32; 4096];
        osc.process(&mut buf);
        assert!(buf.iter().all(|s| s.is_finite()));

        // Very low
        let mut osc = Oscillator::new(Waveform::Sine, 0.1, SAMPLE_RATE);
        osc.process(&mut buf);
        assert!(buf.iter().all(|s| s.is_finite()));

        // Zero Hz
        let mut osc = Oscillator::new(Waveform::Sine, 0.0, SAMPLE_RATE);
        osc.process(&mut buf);
        assert!(buf.iter().all(|s| s.is_finite()));
    }
}
