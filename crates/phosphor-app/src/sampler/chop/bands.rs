//! A recording heard through one band, reduced to energy per hop.
//!
//! Streamed: the filtered signal is never stored, only the energy of each
//! hop of it. Ten minutes of 48 kHz is 28.8 million frames and would be a
//! hundred-megabyte copy per band; the energies of 5 ms hops are 120,000
//! numbers.

use std::f64::consts::FRAC_1_SQRT_2;

use phosphor_dsp::filter::{Svf, SvfCoeffs};
use phosphor_plugin::sample::SamplePcm;

use super::Band;

/// Which output of a section a stage takes.
#[derive(Debug, Clone, Copy)]
enum Pass {
    Low,
    High,
}

impl Band {
    /// The filter stages, in order: three sections at each edge, 36 dB an
    /// octave. At 24 a snare's 190 Hz body reaches the kick band only 25 dB
    /// under the kick, which is inside the reach of a sensitivity a player
    /// would leave on; and the low corner sits at 100 Hz rather than the 150
    /// a kick might suggest, because the snare's body lives just above it.
    fn stages(self) -> &'static [(Pass, f64)] {
        const LOW: f64 = 100.0;
        const MID_LOW: f64 = 300.0;
        const MID_HIGH: f64 = 5_000.0;
        const HIGH: f64 = 7_000.0;
        match self {
            Band::Full => &[],
            Band::Low => &[(Pass::Low, LOW), (Pass::Low, LOW), (Pass::Low, LOW)],
            Band::Mid => &[
                (Pass::High, MID_LOW),
                (Pass::High, MID_LOW),
                (Pass::High, MID_LOW),
                (Pass::Low, MID_HIGH),
            ],
            Band::High => &[(Pass::High, HIGH), (Pass::High, HIGH), (Pass::High, HIGH)],
        }
    }
}

/// One frame as a single line: the channels averaged.
#[inline]
pub fn mono_at(pcm: &SamplePcm, frame: u64) -> f32 {
    let channels = usize::from(pcm.channels.max(1));
    let base = frame as usize * channels;
    pcm.data
        .get(base..base + channels)
        .map_or(0.0, |f| f.iter().sum::<f32>() / channels as f32)
}

/// Mean energy of each `hop` frames of `start..end`, heard through `band`.
///
/// The last hop is whatever is left of the region, averaged over what it
/// has — a short tail is not quieter than it sounds.
pub fn hop_energies(pcm: &SamplePcm, start: u64, end: u64, band: Band, hop: u64) -> Vec<f32> {
    let hop = hop.max(1);
    let end = end.min(pcm.frames());
    if start >= end {
        return Vec::new();
    }
    let rate = f64::from(pcm.sample_rate.max(1.0));
    let stages: Vec<(Pass, SvfCoeffs)> = band
        .stages()
        .iter()
        // A corner at or past Nyquist is a filter with nothing to do; a
        // tan() that close to π/2 is a filter that explodes.
        .map(|&(pass, hz)| (pass, SvfCoeffs::new(hz.min(rate * 0.45), FRAC_1_SQRT_2, rate)))
        .collect();
    let mut filters = vec![Svf::new(); stages.len()];

    let mut energies = Vec::with_capacity(((end - start) / hop + 1) as usize);
    let (mut sum, mut count) = (0.0f64, 0u64);
    for frame in start..end {
        let mut x = f64::from(mono_at(pcm, frame));
        for (filter, &(pass, coeffs)) in filters.iter_mut().zip(&stages) {
            let (low, _, high) = filter.tick_with(x, coeffs);
            x = match pass {
                Pass::Low => low,
                Pass::High => high,
            };
        }
        sum += x * x;
        count += 1;
        if count == hop {
            energies.push((sum / hop as f64) as f32);
            (sum, count) = (0.0, 0);
        }
    }
    if count > 0 {
        energies.push((sum / count as f64) as f32);
    }
    energies
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::chop::testkit::{sine, SR};

    fn energy(band: Band, hz: f32) -> f32 {
        let pcm = sine(hz, 0.5, SR);
        let e = hop_energies(&pcm, 0, pcm.frames(), band, 480);
        // Past the filters' settling.
        e[e.len() / 2..].iter().sum::<f32>() / (e.len() - e.len() / 2) as f32
    }

    #[test]
    fn each_band_hears_its_own_drum_and_not_the_others() {
        let full = energy(Band::Full, 50.0);
        assert!(energy(Band::Low, 50.0) > full * 0.8, "the low band lost the kick");
        assert!(energy(Band::Low, 2_000.0) < full * 0.001, "the low band heard the snare's crack");
        assert!(energy(Band::Mid, 1_500.0) > full * 0.8, "the mid band lost the snare");
        assert!(energy(Band::Mid, 50.0) < full * 0.01, "the mid band heard the kick");
        assert!(energy(Band::High, 12_000.0) > full * 0.8, "the high band lost the hat");
        assert!(energy(Band::High, 1_500.0) < full * 0.001, "the high band heard the snare");
    }

    #[test]
    fn a_sine_at_half_scale_has_the_energy_it_should() {
        // A sine of amplitude a has mean square a²/2.
        let e = energy(Band::Full, 440.0);
        assert!((e - 0.125).abs() < 0.002, "{e}");
    }

    #[test]
    fn the_hops_cover_the_region_and_a_short_tail_counts_whole() {
        let pcm = sine(440.0, 0.1, SR); // 4,800 frames
        let e = hop_energies(&pcm, 100, 1_150, Band::Full, 100);
        assert_eq!(e.len(), 11, "ten whole hops and the tail of fifty");
        assert!(e[10] > 0.08, "the tail was averaged over frames it does not have");
        assert!(hop_energies(&pcm, 500, 500, Band::Full, 100).is_empty());
        assert!(hop_energies(&pcm, 4_000, 99_999, Band::Full, 100).len() == 8);
    }

    #[test]
    fn stereo_is_heard_as_the_average_of_its_sides() {
        let pcm = SamplePcm { data: vec![1.0, 0.0, 0.5, 0.5], channels: 2, sample_rate: SR };
        assert_eq!(mono_at(&pcm, 0), 0.5);
        assert_eq!(mono_at(&pcm, 1), 0.5);
        assert_eq!(mono_at(&pcm, 9), 0.0, "a frame past the end is silence, not a panic");
    }
}
