//! Synthesized drums at frames the test knows, so a detector can be graded
//! against the truth rather than against itself.
//!
//! The voices are built to be the hard version of each drum: the kick has a
//! beater click that reaches the snare band, the snare has a 190 Hz body that
//! reaches the kick band, and the hats are noise. A band that picks the right
//! hits out of this picks them out of the easy cases too.

use std::f64::consts::FRAC_1_SQRT_2;

use phosphor_dsp::filter::{Svf, SvfCoeffs};
use phosphor_plugin::sample::SamplePcm;

pub const SR: f32 = 48_000.0;

/// A steady sine, for the filter tests.
pub fn sine(hz: f32, seconds: f32, rate: f32) -> SamplePcm {
    let n = (seconds * rate) as usize;
    let data = (0..n)
        .map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / rate).sin())
        .collect();
    SamplePcm { data, channels: 1, sample_rate: rate }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drum {
    Kick,
    Snare,
    Hat,
}

/// Deterministic white noise in -1..1: the same break every run.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// Add one drum into `out` starting at `at`, scaled by `gain`.
fn strike(out: &mut [f32], at: usize, drum: Drum, gain: f32, rate: f32, noise: &mut Noise) {
    let sr = f64::from(rate);
    // The snare's wires darkened above 6 kHz; the hat is two sections of
    // high pass at 7 kHz, which is where a real hat's energy lives.
    let wires = SvfCoeffs::new(6_000.0, FRAC_1_SQRT_2, sr);
    let shimmer = SvfCoeffs::new(7_000.0, FRAC_1_SQRT_2, sr);
    let (mut f1, mut f2) = (Svf::new(), Svf::new());
    let mut phase = 0.0f32;
    for (i, sample) in out.iter_mut().enumerate().skip(at) {
        let t = (i - at) as f32 / rate;
        let s = match drum {
            Drum::Kick => {
                if t > 0.4 {
                    break;
                }
                // Pitch falls 160 → 50 Hz over 30 ms, the shape every kick has.
                let hz = 50.0 + 110.0 * (-t / 0.012).exp();
                phase += std::f32::consts::TAU * hz / rate;
                // The beater: a millisecond of noise, which is what reaches
                // the snare's band.
                let click = if t < 0.001 { 0.2 * noise.next() } else { 0.0 };
                0.9 * phase.sin() * (-t / 0.15).exp() + click
            }
            Drum::Snare => {
                if t > 0.3 {
                    break;
                }
                phase += std::f32::consts::TAU * 190.0 / rate;
                let wire = f1.tick_with(f64::from(noise.next()), wires).0 as f32;
                0.35 * phase.sin() * (-t / 0.05).exp() + 0.6 * wire * (-t / 0.08).exp()
            }
            Drum::Hat => {
                if t > 0.08 {
                    break;
                }
                let once = f1.tick_with(f64::from(noise.next()), shimmer).2;
                0.4 * f2.tick_with(once, shimmer).2 as f32 * (-t / 0.02).exp()
            }
        };
        *sample += s * gain;
    }
}

/// A one-bar boom-bap pattern at 90 BPM, sixteenth steps: which drums hit on
/// which step. Steps 3, 10 and 14 are hats alone, 7 and 10 carry a kick
/// the snare never shares, and the snares land on the backbeat.
pub const PATTERN: &[(usize, &[Drum])] = &[
    (0, &[Drum::Kick, Drum::Hat]),
    (2, &[Drum::Hat]),
    (4, &[Drum::Snare, Drum::Hat]),
    (6, &[Drum::Hat]),
    (7, &[Drum::Kick]),
    (8, &[Drum::Hat]),
    (10, &[Drum::Kick, Drum::Hat]),
    (12, &[Drum::Snare, Drum::Hat]),
    (14, &[Drum::Hat]),
];

/// Frames per sixteenth at 90 BPM.
pub fn step_frames(rate: f32) -> usize {
    (rate * 60.0 / 90.0 / 4.0) as usize
}

/// Silence before the first hit, so a detector has to find the start rather
/// than being handed it at frame zero.
pub fn lead_frames(rate: f32) -> usize {
    (rate * 0.1) as usize
}

/// The pattern rendered, with the frame each step lands on. One bar plus a
/// tail for the last hits to ring out.
pub fn boom_bap(rate: f32) -> (SamplePcm, Vec<(usize, &'static [Drum])>) {
    let step = step_frames(rate);
    let lead = lead_frames(rate);
    let mut out = vec![0.0f32; lead + step * 16 + (rate * 0.4) as usize];
    let mut noise = Noise(0x1234_5678);
    let mut hits = Vec::new();
    for &(index, drums) in PATTERN {
        let at = lead + index * step;
        for &drum in drums {
            strike(&mut out, at, drum, 1.0, rate, &mut noise);
        }
        hits.push((at, drums));
    }
    (SamplePcm { data: out, channels: 1, sample_rate: rate }, hits)
}

/// The frames of the steps that carry `drum`.
pub fn frames_with(hits: &[(usize, &[Drum])], drum: Drum) -> Vec<usize> {
    hits.iter().filter(|(_, d)| d.contains(&drum)).map(|&(f, _)| f).collect()
}

/// A single drum hit at `at` in a buffer of `len` frames, at `gain`.
pub fn one_hit(drum: Drum, at: usize, len: usize, gain: f32, rate: f32) -> Vec<f32> {
    let mut out = vec![0.0f32; len];
    strike(&mut out, at, drum, gain, rate, &mut Noise(0x9e37_79b9));
    out
}
