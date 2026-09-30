//! Finding where the hits start.
//!
//! Two passes, split by cost. [`OnsetCurve::analyse`] walks every frame
//! once, through the band's filters, and keeps only numbers per 5 ms hop:
//! how loud each hop is, and how far it has risen over the hops before it.
//! That is the expensive part and it runs once per band. [`OnsetCurve::pick`]
//! then reads those numbers at a sensitivity, which is cheap enough to run
//! on every press of the key that turns it.
//!
//! A hop is a hit when three things hold:
//!
//! 1. it **rises** far enough over the quietest of the hops just before it —
//!    a hit is a jump in level, and the tail of the last one is not;
//! 2. its **weight** — the average level of the 50 ms after the top of its
//!    rise — is within reach of the heaviest hit in the band. Weight rather
//!    than the loudest instant, because that is what the ear hears as loud:
//!    a kick's 1 ms beater click leaks into every band with a sharp peak, and
//!    is over before its rise has topped out, while a snare's crack is still
//!    sounding eighty milliseconds later;
//! 3. it is the biggest rise within a flam's width either side.
//!
//! Sensitivity moves the first two together: more of it asks for less rise
//! and reaches further down.
//!
//! The hop tells us *which* 5 ms the hit is in; a cut needs the frame. The
//! attack is found on the unfiltered recording — a filter smears the edge
//! it is listening for by milliseconds — as the steepest jump in level
//! within the hops the rise began in, and the cut goes a little before it.

use phosphor_plugin::sample::SamplePcm;

use super::bands::{hop_energies, mono_at};
use super::markers::{ms_to_frames, Marker, MIN_GAP_MS};
use super::Band;

/// How finely the level is followed.
pub const HOP_MS: f32 = 5.0;

/// The rise is measured against the quietest of this many hops before —
/// 20 ms, long enough to see past the ripple of a low note, short enough
/// that the tail of a hit a sixteenth ago is not the reference.
const LOOKBACK: usize = 4;

/// Level is judged over this many hops — 20 ms, one whole cycle of the
/// lowest kick.
const SMOOTH_HOPS: usize = 4;

/// How long after its start a hit's weight is judged over.
const WEIGHT_MS: f32 = 50.0;

/// A cut sits this far ahead of the attack it marks, so the attack is never
/// clipped. Inside the engine's 2 ms edge fade, which then opens exactly as
/// the hit arrives.
const PRE_ROLL_MS: f32 = 1.5;

/// Below this, a hop is silence: nothing in it can be a hit, however far it
/// rose. Mean energy in dB, roughly −66 dBFS as a level.
const FLOOR_DB: f32 = -66.0;

/// The level of a hop with nothing in it at all.
const SILENT_DB: f32 = -120.0;

/// Where [`OnsetCurve::pick`] starts a player off.
pub const DEFAULT_SENSITIVITY: f32 = 0.5;

/// What a sensitivity asks for: `(least rise, reach below the heaviest)`, in
/// dB. At the bottom, only jumps of 15 dB within 6 dB of the heaviest hit;
/// at the top, 3 dB jumps as far as 40 dB down, which is the ghost notes.
///
/// Geometric between the ends, so the middle sits where the decisions are:
/// 6.7 dB of rise and 15.5 dB of reach. That reach is what keeps each band
/// to its own drum — measured on a break built to be hard, what leaks into
/// a band sits 18 to 27 dB under the drum the band is for — while every
/// kick and snare in the full recording stays in it.
pub fn thresholds(sensitivity: f32) -> (f32, f32) {
    let s = sensitivity.clamp(0.0, 1.0);
    (15.0 * (3.0f32 / 15.0).powf(s), 6.0 * (40.0f32 / 6.0).powf(s))
}

fn to_db(energy: f32) -> f32 {
    if energy > 0.0 {
        (10.0 * energy.log10()).max(SILENT_DB)
    } else {
        SILENT_DB
    }
}

/// One band of one region, read once and picked from as often as the
/// sensitivity changes.
#[derive(Debug, Clone)]
pub struct OnsetCurve {
    start: u64,
    hop: u64,
    /// A flam's width in hops: no two hits closer.
    gap: usize,
    /// Level of each hop, dB, over the last [`SMOOTH_HOPS`].
    level: Vec<f32>,
    /// How far each hop stands above the quietest of the [`LOOKBACK`] before
    /// it, dB, never negative. Before the region is silence.
    rise: Vec<f32>,
    /// Mean level over the [`WEIGHT_MS`] from each hop on, dB. Read through
    /// [`Self::body`].
    weight: Vec<f32>,
    /// The heaviest hit in the band — what "within reach" is measured from.
    heaviest: f32,
}

impl OnsetCurve {
    /// Read `start..end` of `pcm` through `band`. O(frames), once.
    pub fn analyse(pcm: &SamplePcm, start: u64, end: u64, band: Band) -> Self {
        let hop = ms_to_frames(HOP_MS, pcm.sample_rate).max(1);
        let energies = hop_energies(pcm, start, end, band, hop);
        // Level is the mean of the last [`SMOOTH_HOPS`], not of one: a 50 Hz
        // kick measured 5 ms at a time ripples ±6 dB with its own waveform,
        // and every crest of the ripple reads as a new hit.
        let level: Vec<f32> = (0..energies.len())
            .map(|n| {
                let window = &energies[(n + 1).saturating_sub(SMOOTH_HOPS)..=n];
                to_db(window.iter().sum::<f32>() / window.len() as f32)
            })
            .collect();

        let rise = (0..level.len())
            .map(|n| {
                let before = level[n.saturating_sub(LOOKBACK)..n]
                    .iter()
                    .copied()
                    .fold(if n < LOOKBACK { SILENT_DB } else { f32::MAX }, f32::min);
                (level[n] - before).max(0.0)
            })
            .collect();

        // A running sum, so every hop's window costs the same one add and
        // one subtract however long the window is.
        let span = ((WEIGHT_MS / HOP_MS) as usize).max(1);
        let mut weight = Vec::with_capacity(energies.len());
        let mut sum: f64 = energies.iter().take(span).map(|&e| f64::from(e)).sum();
        for n in 0..energies.len() {
            let width = span.min(energies.len() - n);
            weight.push(to_db((sum / width as f64) as f32));
            sum -= f64::from(energies[n]);
            if let Some(&next) = energies.get(n + span) {
                sum += f64::from(next);
            }
        }

        let gap = ((ms_to_frames(MIN_GAP_MS, pcm.sample_rate) / hop) as usize).max(1);
        let mut curve = Self { start, hop, gap, level, rise, weight, heaviest: SILENT_DB };
        // Weighed the way every hit is, at the top of its rise, and over
        // every rise the most sensitive setting would call a hit — so the
        // reference cannot move when the sensitivity does.
        let (least_rise, _) = thresholds(1.0);
        curve.heaviest = curve.peaks(least_rise).map(|n| curve.body(n)).fold(SILENT_DB, f32::max);
        curve
    }

    /// The weight of the hit whose rise tops out at hop `n`: the level of
    /// what is still sounding from the hop after on.
    ///
    /// One hop after, because for a sound shorter than a hop the top of the
    /// rise *is* the sound — a kick's beater click tops out on the hop it
    /// happens in, and weighing from there would give a millisecond of tick
    /// the weight of whatever follows it. Every drum is still sounding 5 ms
    /// in; a click never is.
    fn body(&self, n: usize) -> f32 {
        self.weight.get(n + 1).copied().unwrap_or(SILENT_DB)
    }

    /// The hops at the top of a rise of at least `least_rise`: loud enough
    /// to be sound, and the biggest rise within a flam either side — the
    /// earlier of two equal ones, so the same audio always cuts the same way.
    fn peaks(&self, least_rise: f32) -> impl Iterator<Item = usize> + '_ {
        (0..self.rise.len()).filter(move |&n| {
            let rise = self.rise[n];
            if rise < least_rise || self.level[n] < FLOOR_DB {
                return false;
            }
            let lo = n.saturating_sub(self.gap);
            let hi = (n + self.gap + 1).min(self.rise.len());
            !(lo..hi).any(|m| self.rise[m] > rise || (self.rise[m] == rise && m < n))
        })
    }

    /// The hits at `sensitivity` (0..=1): the hop each one's rise began in,
    /// and its weight. O(hops × flam width).
    ///
    /// Weighed from the top of the rise, which is the sound that follows
    /// the attack rather than the attack itself. That is the difference
    /// between a kick's beater click — a millisecond, over before the top,
    /// and heard in every band — and the snare the mid band is for.
    fn hits(&self, sensitivity: f32) -> Vec<(usize, f32)> {
        let (least_rise, reach) = thresholds(sensitivity);
        let floor = FLOOR_DB.max(self.heaviest - reach);
        self.peaks(least_rise)
            .filter(|&n| self.body(n) >= floor)
            .map(|n| {
                // Back to where the rise began: a hit climbs over two or
                // three hops, and the attack is at the bottom of the climb.
                let mut first = n;
                while first > 0
                    && n - first < LOOKBACK
                    && self.rise[first - 1] >= least_rise * 0.5
                {
                    first -= 1;
                }
                (first, self.body(n))
            })
            .collect()
    }

    /// The cuts at `sensitivity`, each placed just ahead of its attack.
    pub fn pick(&self, pcm: &SamplePcm, sensitivity: f32) -> Vec<Marker> {
        let pre_roll = ms_to_frames(PRE_ROLL_MS, pcm.sample_rate);
        self.hits(sensitivity)
            .into_iter()
            .map(|(first, weight)| {
                let attack = self.attack(pcm, first);
                let frame = attack.saturating_sub(pre_roll).max(self.start);
                let cut = Marker::at(pcm, frame, weight, false);
                // A snap may not carry a cut out of the region it was found
                // in: the marker list would drop it as outside, and a region
                // that opens on sound would lose its first slice.
                if cut.frame < self.start {
                    Marker { frame: self.start, ..cut }
                } else {
                    cut
                }
            })
            .collect()
    }

    /// The attack inside hop `n`, give or take one hop: the start of the
    /// steepest jump in level on the unfiltered recording.
    fn attack(&self, pcm: &SamplePcm, n: usize) -> u64 {
        let from = self.start + (n.saturating_sub(1) as u64) * self.hop;
        let to = (self.start + (n as u64 + 1) * self.hop).min(pcm.frames());
        // Sub-hops an eighth of a hop long, about 0.6 ms at any rate.
        let sub = (self.hop / 8).max(1);
        let peak = |at: u64| -> f32 {
            (at..(at + sub).min(pcm.frames())).fold(0.0f32, |m, f| m.max(mono_at(pcm, f).abs()))
        };
        let peaks: Vec<(u64, f32)> =
            (from..to).step_by(sub as usize).map(|at| (at, peak(at))).collect();
        let loudest = peaks.iter().fold(0.0f32, |m, &(_, p)| m.max(p));
        let mut best = (from + self.hop, 0.0f32);
        let mut before = from.checked_sub(sub).map_or(0.0, peak);
        for &(at, p) in &peaks {
            // Only sub-hops that reach a quarter of the loudest count: in
            // near-silence the ratio between two tiny numbers is noise.
            if p >= loudest * 0.25 {
                let jump = p / before.max(1e-6);
                if jump > best.1 {
                    best = (at, jump);
                }
            }
            before = p;
        }
        best.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::chop::testkit::{boom_bap, frames_with, one_hit, Drum, SR};

    const RATES: [f32; 2] = [44_100.0, 48_000.0];

    fn cuts(pcm: &SamplePcm, band: Band, sensitivity: f32) -> Vec<u64> {
        OnsetCurve::analyse(pcm, 0, pcm.frames(), band)
            .pick(pcm, sensitivity)
            .iter()
            .map(|m| m.frame)
            .collect()
    }

    /// Every cut lands just before the hit it is for — never after it, which
    /// would clip the attack, and never more than 4 ms early — and there is
    /// exactly one cut per hit.
    fn assert_cuts_at(found: &[u64], truth: &[usize], rate: f32, what: &str) {
        let early = ms_to_frames(4.0, rate);
        assert_eq!(found.len(), truth.len(), "{what} at {rate}: cut at {found:?}, hits at {truth:?}");
        for (&cut, &hit) in found.iter().zip(truth) {
            let hit = hit as u64;
            assert!(cut < hit && hit - cut <= early, "{what} at {rate}: the hit at {hit} was cut at {cut}");
        }
    }

    /// The feature, in one test: a break cut at the kicks, at the snares and
    /// at the hats, each band hearing only its own drum at the default.
    #[test]
    fn each_band_cuts_at_its_own_drum_at_the_default_sensitivity() {
        for rate in RATES {
            let (pcm, hits) = boom_bap(rate);
            let s = DEFAULT_SENSITIVITY;
            assert_cuts_at(&cuts(&pcm, Band::Low, s), &frames_with(&hits, Drum::Kick), rate, "low");
            assert_cuts_at(&cuts(&pcm, Band::Mid, s), &frames_with(&hits, Drum::Snare), rate, "mid");
            assert_cuts_at(&cuts(&pcm, Band::High, s), &frames_with(&hits, Drum::Hat), rate, "high");
        }
    }

    /// The whole recording at the default cuts at the big hits — every kick
    /// and snare — and turned all the way up reaches the hats that play
    /// alone as well.
    #[test]
    fn the_full_recording_finds_the_big_hits_then_the_quiet_ones() {
        for rate in RATES {
            let (pcm, hits) = boom_bap(rate);
            let big: Vec<usize> = hits
                .iter()
                .filter(|(_, d)| d.contains(&Drum::Kick) || d.contains(&Drum::Snare))
                .map(|&(f, _)| f)
                .collect();
            assert_cuts_at(&cuts(&pcm, Band::Full, DEFAULT_SENSITIVITY), &big, rate, "full");
            let all = cuts(&pcm, Band::Full, 1.0);
            assert!(all.len() > big.len(), "all the way up found nothing more: {all:?}");
            for alone in frames_with(&hits, Drum::Hat).into_iter().filter(|f| !big.contains(f)) {
                // The one under the kick's tail is masked for real; the rest
                // stand clear of anything else.
                let masked = hits.iter().any(|&(f, d)| d.contains(&Drum::Kick) && f < alone && alone - f < (rate * 0.2) as usize);
                if !masked {
                    assert!(all.iter().any(|&c| c < alone as u64 && alone as u64 - c <= ms_to_frames(4.0, rate)), "the hat at {alone} was not found");
                }
            }
        }
    }

    #[test]
    fn more_sensitivity_never_finds_fewer_hits() {
        let (pcm, _) = boom_bap(SR);
        for band in [Band::Full, Band::Low, Band::Mid, Band::High] {
            let curve = OnsetCurve::analyse(&pcm, 0, pcm.frames(), band);
            let counts: Vec<usize> = (0..=10).map(|i| curve.pick(&pcm, i as f32 / 10.0).len()).collect();
            assert!(counts.windows(2).all(|w| w[0] <= w[1]), "{band:?}: {counts:?}");
        }
    }

    /// A snare played softly, 20 dB under the backbeat: a ghost note. Left
    /// out until the player reaches for it.
    #[test]
    fn a_ghost_note_waits_for_the_sensitivity_to_reach_it() {
        let len = (SR * 1.5) as usize;
        let (loud, ghost) = ((SR * 0.2) as usize, (SR * 0.7) as usize);
        let mut data = one_hit(Drum::Snare, loud, len, 1.0, SR);
        for (d, g) in data.iter_mut().zip(one_hit(Drum::Snare, ghost, len, 0.1, SR)) {
            *d += g;
        }
        let pcm = SamplePcm { data, channels: 1, sample_rate: SR };
        assert_cuts_at(&cuts(&pcm, Band::Mid, DEFAULT_SENSITIVITY), &[loud], SR, "default");
        assert_cuts_at(&cuts(&pcm, Band::Mid, 1.0), &[loud, ghost], SR, "all the way up");
    }

    #[test]
    fn silence_and_nothing_make_no_cuts() {
        let silence = SamplePcm { data: vec![0.0; 48_000], channels: 1, sample_rate: SR };
        assert!(cuts(&silence, Band::Full, 1.0).is_empty());
        let (pcm, _) = boom_bap(SR);
        let curve = OnsetCurve::analyse(&pcm, 500, 500, Band::Full);
        assert!(curve.pick(&pcm, 1.0).is_empty());
    }

    /// A region chopped from a buffer's middle speaks in the buffer's frames
    /// and never cuts before its own start. One that opens on sound opens
    /// with a cut, because what is sounding there belongs in the first slice
    /// rather than in none.
    #[test]
    fn a_region_is_cut_in_buffer_frames() {
        let (pcm, hits) = boom_bap(SR);
        let snares = frames_with(&hits, Drum::Snare);
        // From just after the first snare's attack: its tail is in the region
        // but its hit is not.
        let start = snares[0] as u64 + 100;
        let curve = OnsetCurve::analyse(&pcm, start, pcm.frames(), Band::Mid);
        let found: Vec<u64> = curve.pick(&pcm, DEFAULT_SENSITIVITY).iter().map(|m| m.frame).collect();
        assert!(found.iter().all(|&f| f >= start), "cut before the region: {found:?}");
        assert!(found[0] - start <= ms_to_frames(1.0, SR), "the sound it opens on was left out: {found:?}");
        assert_cuts_at(&found[1..], &snares[1..], SR, "the region");
    }
}
