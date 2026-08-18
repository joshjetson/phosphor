//! The physics the three acoustic kits share.
//!
//! Fifteen machines in this rack and every one of them is a machine: a
//! circuit, a converter, or a recipe table. This is the other half — real
//! drums, modelled, and the modelling is different in kind rather than in
//! tuning.
//!
//! # Why an 808 cannot be retuned into a kick drum
//!
//! An 808's bass drum is one bridged-T resonator: a single decaying sinusoid
//! at 49.4 Hz with a click on the front of it. An acoustic bass drum is **two
//! membranes coupled through the air trapped inside the shell**, and that
//! coupling is most of the sound. The batter head is struck; the air in the
//! shell is compressed; the resonant head moves; the air pushes back. The
//! result is that the drum has *two* low modes where a single head has one,
//! and the interval between them is set by how much air spring there is —
//! which is what a drummer changes when they tune the front head, cut a port
//! in it, or drop a pillow inside.
//!
//! Two modes a few Hz apart beat. That envelope ripple is the "boom-woomp" of
//! a real kick and it is unreachable from any one resonator, at any tuning.
//! [`couple`] is the eigenproblem that produces it and
//! `the_two_heads_split_the_kick_into_two_modes` measures it.
//!
//! # Membranes
//!
//! An ideal circular membrane's modes are the zeros of the Bessel functions,
//! in the ratios
//!
//! ```text
//! 1 : 1.593 : 2.136 : 2.295 : 2.653 : 2.917 : 3.155 : 3.500
//! ```
//!
//! for (0,1) (1,1) (2,1) (0,2) (3,1) (1,2) (4,1) (2,2). **A real drum does not
//! have these.** The head does not vibrate in a vacuum: it drags a layer of
//! air with it, and that added mass lowers every mode. It does not lower them
//! equally — the modes that displace net volume, the axisymmetric (0,m) pair,
//! drag the most air, and every mode drags less of it as the mode order rises
//! and adjacent lobes start cancelling each other's near field. So the loading
//! pulls the *bottom* of the series down hardest and the ratios come out
//! stretched. That is the same mechanism that makes a timpani nearly
//! harmonic — its ideal 1 : 1.34 : 1.66 : 1.98 is measured at roughly
//! 1 : 1.5 : 2 : 2.44 — and it is why the ideal set on its own sounds like a
//! synthesizer imitating a drum. [`AIR_LOAD`] is the shape factor per mode.
//!
//! Then there are two heads, at two tensions, so the whole series appears
//! twice: a batter group at `f_b · r_n` and a resonant group at `f_r · r_n`.
//! Two nearly evenly spaced groups, and — apart from the lowest pair, which
//! the cavity splits — no coupling between them, because a mode with `n ≥ 1`
//! has as much head going up as coming down and changes the enclosed volume
//! by nothing at all. This is Rossing, Bork, Zhao and Fystrom's picture of a
//! snare drum, arrived at from the mechanism rather than transcribed.
//!
//! Last, a struck head is momentarily at higher tension than a resting one,
//! so it starts sharp and falls — [`Drum::drop`], a few percent over ten or
//! twenty milliseconds on a kick, much more on a tom, and the thing a tom
//! sounds wrong without.
//!
//! # The snare's wires
//!
//! Twenty strands of coiled wire lying against the bottom head, held by a
//! strainer. They do not vibrate with the head; they **bounce on it**. Contact
//! is intermittent, and the intermittency is the whole character: a soft hit
//! gives a dense even buzz, a hard hit throws the wires clear so they go quiet
//! for a moment and come back — the choke — and after the drum has stopped the
//! wires are still settling, which is the sound a snare leaves in a room.
//!
//! [`Wires`] is a bouncing-mass contact model, three groups of strands with
//! different masses and gaps so they never sync up. It is a few flops, it is
//! driven one way only by the resonant head — nothing goes back into the
//! membrane, so no loop exists to run away — and it is what makes this a
//! snare rather than a tom with a noise burst on it.
//!
//! # Cymbals
//!
//! From *Real-Time Modal Synthesis of Crash Cymbals with Nonlinear Dynamics*
//! (DAFx-19), which is the paper that made this tractable in real time. Three
//! ideas, all of them here:
//!
//! 1. **Complex resonators.** Each mode is one complex one-pole, so its state
//!    carries amplitude and phase together and costs one complex multiply per
//!    sample. [`ModalBank`] is that, and it is what every voice in these three
//!    kits is built from — no `sin` in any inner loop anywhere in this file.
//! 2. **Frequency gating.** The modes above [`Plate::gate_from`] are simply
//!    not there below a strike energy threshold. A cymbal hit harder does not
//!    get louder, it *blooms*: new content appears.
//! 3. **Modal coupling.** A mode's previous complex output, scaled down and
//!    injected into a higher mode's input beside the exciter term. That is the
//!    energy cascade that makes a crash shimmer instead of decaying, and a
//!    ride ping instead of ticking.
//!
//! Bow, bell and edge are one modal system struck in three places — see
//! [`Strike::at`] — which is also how one plate gives a ride all three of its
//! voices. A hi-hat is two cymbals in one bank, the lower half of it the top
//! plate and the upper half the bottom, and closing it does the three things
//! closing it really does: it damps both plates, it dumps the top one's energy
//! into the bottom, and it takes the low modes away — a low mode needs the
//! whole plate free to move and a clamp is exactly what stops that, which is
//! why a closed hat is *brighter* than an open one and not merely shorter. The
//! coupling span on a hat is half the bank, so mode `k + 20` reads mode `k`:
//! the same mechanism as the cymbal's cascade, pointed at the other plate.
//!
//! # Sources
//!
//! * Rossing, Bork, Zhao & Fystrom, *Acoustics of snare drums*, JASA 92 (1992)
//! * Rossing, *Science of Percussion Instruments*, World Scientific (2000)
//! * *Real-Time Modal Synthesis of Crash Cymbals with Nonlinear Dynamics*,
//!   DAFx-19
//!
//! Where a number below is derived, the comment says what from. Where it is
//! chosen — and most of the voicing is chosen, because a kit is a set of
//! choices a drummer made — the comment says that instead.

use super::super::*;

// ══════════════════════════════════════════════════════════════════════════════
// The modal bank
// ══════════════════════════════════════════════════════════════════════════════

/// How many modes one voice runs. Fixed and bounded, as everything in the
/// audio path is: a cymbal uses all of them, a hi-hat twenty per plate, a
/// two-headed drum thirteen and a conga eight.
///
/// Forty is a compromise and worth naming as one. A real cymbal has hundreds
/// of modes — a flat plate's modal density is asymptotically *constant* in
/// frequency, so the count grows linearly with bandwidth and a sixteen-inch
/// crash has several hundred below 16 kHz. Forty is what fits in the budget,
/// and the consequence is audible: these cymbals are thinner than the real
/// thing in the top two octaves, where the gaps between the modelled modes
/// are widest. It is also the number that decides the per-voice cost, since a
/// cymbal is the most expensive voice in the rack.
pub(crate) const MODES: usize = 40;

/// A bank of complex one-pole resonators.
///
/// Each mode is `z[n] = p·z[n−1] + u[n]` with `p = r·e^{jω/sr}`. One complex
/// multiply per mode per sample; the state carries amplitude and phase
/// together, so nothing here needs a `sin` after the coefficients are set, and
/// the output is the quadrature part, which starts at zero and rises the way
/// an impulse response does.
///
/// `w` and `r` are kept alongside the coefficients they were built from so
/// that [`ModalBank::retune`] can rebuild them when the attack pitch drop
/// moves the whole bank — which happens at a control rate, not per sample.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ModalBank {
    re: [f64; MODES],
    im: [f64; MODES],
    /// `r·cos ω` and `r·sin ω`, the rotation with the decay already in it.
    pre: [f64; MODES],
    pim: [f64; MODES],
    /// Radians per sample.
    w: [f64; MODES],
    /// Per-sample decay factor.
    r: [f64; MODES],
    /// How hard the exciter drives this mode.
    drive: [f64; MODES],
    /// How much of it reaches the microphone.
    gain: [f64; MODES],
    /// How much of it reaches the far head — what the snare wires ride on.
    /// Zero everywhere on a drum that has no wires under it.
    far: [f64; MODES],
    /// The DAFx-19 coupling coefficient into this mode from mode `k − span`.
    kappa: [f64; MODES],
    /// How far down the bank a coupled mode reaches for its partner.
    span: usize,
    /// How many modes are live. Everything above this is skipped.
    live: usize,
}

impl ModalBank {
    pub(crate) const fn new() -> Self {
        Self {
            re: [0.0; MODES],
            im: [0.0; MODES],
            pre: [0.0; MODES],
            pim: [0.0; MODES],
            w: [0.0; MODES],
            r: [0.0; MODES],
            drive: [0.0; MODES],
            gain: [0.0; MODES],
            far: [0.0; MODES],
            kappa: [0.0; MODES],
            span: 4,
            live: 0,
        }
    }

    /// Every array, not just the state: a mode left over from the last hit
    /// with a coefficient from that hit and a drive of zero is a mode waiting
    /// for a voicing that skips it to leave it half-set.
    pub(crate) fn clear(&mut self) {
        *self = Self::new();
    }

    /// One mode: where it sits, how long it rings, how hard the strike reaches
    /// it, and how much of it reaches the two microphones.
    ///
    /// `ring` is the −20 dB time, which is how every decay figure in this rack
    /// is quoted. Total by construction — a frequency past Nyquist folds, so
    /// it is clamped rather than trusted, and a ring time of zero would be a
    /// division, so it has a floor.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set(
        &mut self,
        k: usize,
        hz: f64,
        ring: f64,
        drive: f64,
        gain: f64,
        far: f64,
        sr: f64,
    ) {
        if k >= MODES {
            return;
        }
        let hz = hz.clamp(1.0, sr * 0.47);
        let tau = (ring.max(0.0005) / DECAY_REFERENCE).max(1e-5);
        let w = TAU * hz / sr;
        let r = (-1.0 / (tau * sr)).exp();
        self.w[k] = w;
        self.r[k] = r;
        self.pre[k] = r * w.cos();
        self.pim[k] = r * w.sin();
        self.drive[k] = drive;
        self.gain[k] = gain;
        self.far[k] = far;
        if k + 1 > self.live {
            self.live = k + 1;
        }
    }

    /// The DAFx-19 cascade: mode `k` takes `amount` of mode `k − span`'s
    /// previous complex output, injected beside the exciter term.
    ///
    /// `amount` is what the transfer is worth, and the scaling here is what
    /// makes it mean that. A resonator at `ω_k` driven by another at `ω_j`
    /// settles at `κ·A / |e^{jω_j} − e^{jω_k}|`, and that denominator is
    /// `2·|sin(Δω/2)|` — which goes to zero if the two modes ever land on top
    /// of each other, and would then be a gain of tens of thousands. Dividing
    /// it out on the way in leaves the transfer at exactly `amount` times the
    /// source mode's amplitude whatever the two frequencies turn out to be,
    /// so a voicing cannot make the cascade explode by accident.
    ///
    /// The chain only ever runs upward — mode `k` reads `k − span`, which
    /// reads `k − 2·span` — so there is no cycle in it and it cannot
    /// oscillate. What it *can* do is multiply: `n` links in series carry
    /// `amount^n`, so an `amount` above one would grow without bound down a
    /// long enough bank. Every voicing in these three kits is well under one
    /// and `the_cascade_carries_what_it_says_and_no_more` holds them there.
    ///
    /// This was first written scaled by `1 − r` instead, which is also bounded
    /// but is bounded by the *damping*: on a mode that rings for a second the
    /// transfer came out at two parts in a thousand, and the cascade did
    /// nothing at all. A crash measured with no energy above 6 kHz.
    pub(crate) fn couple(&mut self, span: usize, amount: f64) {
        let span = span.max(1);
        self.span = span;
        for k in 0..MODES {
            self.kappa[k] = if k >= span {
                amount * 2.0 * ((self.w[k] - self.w[k - span]) * 0.5).sin().abs()
            } else {
                0.0
            };
        }
    }

    /// Move the lowest `count` modes by `ratio`, which is the attack pitch
    /// drop.
    ///
    /// Called at a control rate — every [`RETUNE_INTERVAL`] samples for the
    /// first few time constants of the drop — because a `sin` and a `cos` per
    /// mode per sample is exactly what the complex resonator exists in order
    /// not to need.
    pub(crate) fn retune(&mut self, ratio: f64, count: usize) {
        for k in 0..self.live.min(count) {
            let w = self.w[k] * ratio;
            self.pre[k] = self.r[k] * w.cos();
            self.pim[k] = self.r[k] * w.sin();
        }
    }

    /// One sample of exciter in; the near microphone and the far head out.
    ///
    /// Runs the bank downwards so that a coupled mode reads its partner's
    /// output from the *previous* sample rather than this one, which is the
    /// paper's implementation note and costs nothing to arrange.
    #[inline]
    pub(crate) fn tick(&mut self, x: f64) -> (f64, f64) {
        let n = self.live.min(MODES);
        let span = self.span;
        let mut near = 0.0;
        let mut far = 0.0;
        for k in (0..n).rev() {
            let (ure, uim) = if k >= span {
                let a = self.kappa[k];
                (x * self.drive[k] + a * self.re[k - span], a * self.im[k - span])
            } else {
                (x * self.drive[k], 0.0)
            };
            let re = self.pre[k] * self.re[k] - self.pim[k] * self.im[k] + ure;
            let im = self.pre[k] * self.im[k] + self.pim[k] * self.re[k] + uim;
            self.re[k] = re;
            self.im[k] = im;
            near += self.gain[k] * im;
            far += self.far[k] * im;
        }
        (near, far)
    }

}

/// How often the bank is rebuilt while the attack pitch drop is moving it.
/// 32 samples is 0.7 ms at 44.1 kHz — far finer than the ear resolves a glide,
/// and 1/32 of the cost of doing it per sample.
pub(crate) const RETUNE_INTERVAL: u64 = 32;

// ══════════════════════════════════════════════════════════════════════════════
// Membrane modes
// ══════════════════════════════════════════════════════════════════════════════

/// Zeros of the Bessel functions, which are the ideal circular membrane's
/// modes: (0,1) (1,1) (2,1) (0,2) (3,1) (1,2) (4,1) (2,2).
pub(crate) const BESSEL_ZERO: [f64; 8] =
    [2.405, 3.832, 5.136, 5.520, 6.380, 7.016, 7.588, 8.417];

/// The angular order `n` of each of those, which is what decides both how much
/// air the mode drags and whether a strike off the centre reaches it at all.
pub(crate) const BESSEL_ORDER: [u32; 8] = [0, 1, 2, 0, 3, 1, 4, 2];

/// How much of the air load each mode carries, relative to the (0,1).
///
/// Chosen, not measured, and the shape is the argument for it: the (0,m) modes
/// displace net volume and drag the most air; a mode with `n ≥ 1` has as much
/// head rising as falling, so its near field short-circuits and it drags much
/// less; and the loading falls further as the lobes get smaller and closer
/// together. What the table has to get right is the *order*, because that is
/// what stretches the ideal ratios, and the order is not in doubt.
pub(crate) const AIR_LOAD: [f64; 8] = [1.00, 0.48, 0.31, 0.42, 0.22, 0.24, 0.17, 0.18];

/// Bessel function of the first kind, from the series, for `x` inside the
/// range this file uses it over — the largest argument here is 8.417.
///
/// `J_n(x) = Σ_k (−1)^k (x/2)^{n+2k} / (k! (n+k)!)`, accumulated as a running
/// term so there is no factorial to overflow. Twenty-four terms is convergence
/// to the last bit of an `f64` at these arguments. Called once per hit, not
/// per sample.
pub(crate) fn bessel_j(n: u32, x: f64) -> f64 {
    let h = x * 0.5;
    let mut term = 1.0;
    for i in 1..=n {
        term *= h / f64::from(i);
    }
    let mut sum = term;
    let h2 = h * h;
    for k in 1..24u32 {
        term *= -h2 / (f64::from(k) * f64::from(n + k));
        sum += term;
    }
    sum
}

/// Where mode `k` of a membrane sits once the air has loaded it, as a ratio to
/// the ideal fundamental.
///
/// `load = 1/√(1 + μ·s_k)` — added mass lowers a frequency by the square root
/// of the mass ratio — and `μ` is how much air the drum drags, which is
/// larger for a big shell and a slack head.
pub(crate) fn loaded_ratio(k: usize, mu: f64) -> f64 {
    BESSEL_ZERO[k] / BESSEL_ZERO[0] / (1.0 + mu * AIR_LOAD[k]).sqrt()
}

/// The two coupled (0,1) modes of a drum with two heads.
///
/// The heads are two oscillators sharing one air spring. Taking both
/// displacements positive *into* the shell, the enclosed volume changes with
/// `x_b + x_r`, so the cavity pushes back on both:
///
/// ```text
/// ẍ_b = −ω_b² x_b − a(x_b + x_r)      a = k·ω_b²
/// ẍ_r = −ω_r² x_r − b(x_b + x_r)      b = k·ω_r²
/// ```
///
/// which is a 2×2 eigenproblem with `T = (1+k)(ω_b² + ω_r²)` and
/// `D = ω_b²ω_r²(1 + 2k)`. Both roots are real and positive. With the heads at
/// the same tension they come out at `ω` and `ω√(1+2k)`: the lower mode is the
/// two heads moving the same way in space, which changes the enclosed volume
/// by nothing and therefore does not feel the air spring at all, and the upper
/// one is the two heads moving towards each other, which is the only motion
/// the air resists.
///
/// Returns `(hz, batter share)` for each, with the batter share taken from the
/// normalised eigenvector: how much of that mode is the head the microphone is
/// pointed at, and how much is the one the wires lie against.
pub(crate) fn couple(batter_hz: f64, reso_hz: f64, k: f64) -> [(f64, f64); 2] {
    let wb = batter_hz * batter_hz;
    let wr = reso_hz * reso_hz;
    let a = k * wb;
    let t = (1.0 + k) * (wb + wr);
    let d = wb * wr * (1.0 + 2.0 * k);
    let disc = (t * t - 4.0 * d).max(0.0).sqrt();
    let mut out = [(0.0, 0.0); 2];
    for (i, lambda) in [(t - disc) * 0.5, (t + disc) * 0.5].into_iter().enumerate() {
        // (ω_b² + a − λ)·x_b + a·x_r = 0
        let ratio = if a.abs() > 1e-9 { (lambda - wb - a) / a } else { 0.0 };
        let norm = (1.0 + ratio * ratio).sqrt();
        out[i] = (lambda.max(0.0).sqrt(), 1.0 / norm);
    }
    out
}

// ══════════════════════════════════════════════════════════════════════════════
// The pieces of a kit
// ══════════════════════════════════════════════════════════════════════════════

/// One drum: a shell, one or two heads, and whatever is lying against the
/// bottom of it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Drum {
    /// Batter-head (0,1) before the air gets to it, Hz. This is the tension
    /// the drummer set, not the pitch the drum sounds at.
    pub(crate) batter: f64,
    /// Resonant head, same. Zero for a conga, which has one head and an open
    /// bottom, so there is no cavity and nothing to couple to.
    pub(crate) reso: f64,
    /// The cavity air spring, as a fraction of `ω_b²`. A sealed shell with two
    /// tight heads is around 0.4; a port takes it down; a pillow against the
    /// front head takes it most of the way to nothing, which is what a
    /// heavily damped studio kick is.
    pub(crate) air_spring: f64,
    /// How much air the heads drag, which is what lowers and stretches the
    /// mode series. Bigger shells and slacker heads drag more.
    pub(crate) air_load: f64,
    /// Ring time of the lowest mode, seconds to −20 dB, before any muffling.
    pub(crate) ring: f64,
    /// How much faster mode `k` decays than mode 0, per unit of frequency
    /// ratio. Heads lose their high modes first; a coated or double-ply head
    /// loses them much faster than a clear single-ply one.
    pub(crate) tilt: f64,
    /// How far above its resting pitch the head starts, and how long it takes
    /// to fall — a struck head is momentarily at higher tension.
    pub(crate) drop: f64,
    pub(crate) drop_tau: f64,
    /// The shell itself: a lightly damped resonance under everything, and the
    /// whole of a cross-stick.
    pub(crate) shell_hz: f64,
    pub(crate) shell_mix: f64,
    /// How much of the resonant head reaches the microphone. A kick miked at
    /// the port hears a lot of it; a tom miked over the batter hears very
    /// little.
    pub(crate) reso_mic: f64,
    /// Wires against the bottom head, 0 for a drum without them.
    pub(crate) wires: f64,
    /// Where this drum sits in the kit.
    pub(crate) out: f64,
}

/// One cymbal, or one pair of them clamped together.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Plate {
    /// The lowest mode, Hz. A big plate is low.
    pub(crate) lowest: f64,
    /// Chladni's law for a flat circular plate puts the modes at `(m+2n)²`,
    /// which is an exponent of 2 on the mode index. A cymbal is not flat — the
    /// dome and the taper stiffen it unevenly — and the measured effect of
    /// that curvature is to pull the exponent down towards 1. So this is
    /// between the two, and it is the number that sets **mode density**: lower
    /// is denser, which is a bigger and thinner cymbal.
    pub(crate) spread: f64,
    /// How far the modes are pushed off the rule, as a fraction. A cymbal is
    /// not a mathematical plate; a china is barely one at all.
    pub(crate) scatter: f64,
    /// Ring time of the lowest mode, seconds to −20 dB.
    pub(crate) ring: f64,
    /// How much faster the high modes go, as the exponent in `τ ∝ f^−tilt`.
    ///
    /// A power law rather than the linear one the heads use: a plate's losses
    /// are radiation and internal damping, both of which rise with frequency
    /// but nothing like as fast as a drumhead's do. At 0.4 the top of a
    /// cymbal's series rings about a quarter as long as the bottom, which is
    /// what keeps a crash sizzling for a second instead of going dark in a
    /// tenth of one.
    pub(crate) tilt: f64,
    /// The first mode that is only there in a hard strike. Everything from
    /// here up is gated by the energy of the hit, which is what makes a cymbal
    /// bloom rather than just get louder.
    pub(crate) gate_from: usize,
    /// The strike energy at which the gated modes start to appear, and the
    /// energy at which they are all the way in.
    pub(crate) gate_open: f64,
    pub(crate) gate_full: f64,
    /// The DAFx-19 cascade coefficient, and how far down the bank each mode
    /// reaches for the one that feeds it.
    pub(crate) cascade: f64,
    pub(crate) cascade_span: usize,
    /// How many modes this plate runs. A hi-hat is two plates of twenty.
    pub(crate) modes: usize,
    pub(crate) out: f64,
}

/// A struck bar with no membrane anywhere in it: the cowbell.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bar {
    pub(crate) hz: f64,
    pub(crate) ring: f64,
    pub(crate) out: f64,
}

/// The downward expander the studio kit puts across everything, which is the
/// one thing on that kit that is not a drum.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Gate {
    /// Where it starts closing, as a fraction of the hit's own peak. Relative
    /// rather than absolute so that a quiet hit is gated the same way a loud
    /// one is, which is what a gate keyed off the close mic does.
    pub(crate) threshold: f64,
    /// How fast it shuts, seconds to −60 dB.
    pub(crate) release: f64,
}

/// A port cut in the front head, which turns the shell into a Helmholtz
/// resonator: the air in the hole is a mass, the air in the shell is a spring,
/// and the pair has a resonance of its own under the drum. It also bleeds the
/// cavity, which is why a ported kick has less of the two-head split than a
/// sealed one — that is carried by [`Drum::air_spring`], and this is the
/// resonance the hole adds back.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Port {
    pub(crate) hz: f64,
    pub(crate) q: f64,
    pub(crate) mix: f64,
}

/// Everything a drummer brought to the session.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Kit {
    pub(crate) kick: Drum,
    pub(crate) snare: Drum,
    /// Floor, mid and high, low to high.
    pub(crate) toms: [Drum; 3],
    /// Low, mid and high, and single-headed — an open shell has no cavity, so
    /// none of the two-head machinery above runs on these.
    pub(crate) congas: [Drum; 3],
    pub(crate) ride: Plate,
    /// Two crashes, differing in size and therefore in mode density.
    pub(crate) crash: [Plate; 2],
    pub(crate) splash: Plate,
    pub(crate) china: Plate,
    /// One pair: the lower half of the bank is the top plate and the upper
    /// half the bottom one.
    pub(crate) hat: Plate,
    pub(crate) cowbell: Bar,
    /// A gate across the kit, for the one kit that has one.
    pub(crate) gate: Option<Gate>,
    /// A port in the front head of the kick, for the one kit that has one.
    pub(crate) port: Option<Port>,
    /// Whether the snare articulations that call for it are played with
    /// brushes, which are a different excitation and not a filter setting.
    pub(crate) brushes: bool,
    /// Trim for the whole kit.
    pub(crate) out: f64,
}

// ══════════════════════════════════════════════════════════════════════════════
// Articulations — one table, read by the synthesis and by the routing
// ══════════════════════════════════════════════════════════════════════════════

/// Every way these kits can be struck.
///
/// The names are the drummer's, not the machine's, because on an acoustic kit
/// the difference between two sounds is usually where and how you hit the same
/// piece of metal or skin rather than which circuit fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Articulation {
    KickFelt,
    KickWood,
    KickDeep,
    KickMuted,
    SnareCentre,
    SnareEdge,
    SnareGhost,
    SnareBrush,
    SnareRimshot,
    SnareFlam,
    CrossStick,
    TomHigh,
    TomMid,
    TomFloor,
    CongaHigh,
    CongaMid,
    CongaLow,
    CongaSlap,
    HatClosed,
    HatClosedEdge,
    HatPedal,
    HatOpen,
    HatOpenLoose,
    HatHalf,
    RideBow,
    RideBell,
    RideEdge,
    CrashThin,
    CrashDark,
    China,
    Splash,
    CowbellMid,
    CowbellHigh,
    CowbellLow,
}

impl Articulation {
    /// The panel strip this articulation is played from.
    ///
    /// One conga per tom strip, by pitch, which is what the rest of this rack
    /// does and what the panel's own documentation says: the tom and the conga
    /// at the same pitch are one board behind one TUNING knob.
    pub(crate) fn strip(self) -> Instrument {
        match self {
            Self::KickFelt | Self::KickWood | Self::KickDeep | Self::KickMuted => Instrument::Bd,
            Self::SnareCentre
            | Self::SnareEdge
            | Self::SnareGhost
            | Self::SnareBrush
            | Self::SnareRimshot
            | Self::SnareFlam => Instrument::Sd,
            Self::CrossStick => Instrument::Rim,
            Self::TomHigh | Self::CongaHigh | Self::CongaSlap => Instrument::HighTom,
            Self::TomMid | Self::CongaMid => Instrument::MidTom,
            Self::TomFloor | Self::CongaLow => Instrument::LowTom,
            Self::HatClosed | Self::HatClosedEdge | Self::HatPedal => Instrument::ClosedHat,
            Self::HatOpen | Self::HatOpenLoose | Self::HatHalf => Instrument::OpenHat,
            Self::RideBow | Self::RideBell | Self::RideEdge => Instrument::Ride,
            Self::CrashThin | Self::CrashDark | Self::China | Self::Splash => Instrument::Cymbal,
            Self::CowbellMid | Self::CowbellHigh | Self::CowbellLow => Instrument::Cowbell,
        }
    }
}

/// The whole kit, one articulation per key, in the order a drummer would read
/// it off a chart: kick, snare, rims, toms, congas, hats, ride, crashes, bell.
///
/// This is what notes 76 and up play. The General MIDI percussion map has
/// thirty-four slots between 35 and 75 and this kit has thirty-four
/// articulations, but they are not the same thirty-four: GM has a hand clap,
/// a vibraslap, two whistles and two guiros, and has no slot at all for a
/// half-open hat, a ride edge, a brush, a ghost note, a conga slap or a second
/// crash. So the GM range is mapped to the nearest thing this kit really has —
/// see [`articulation`] — and the whole articulation set is laid out again from
/// 76 up, one per key, so that nothing is only reachable by approximation.
/// Above the end of the table it wraps, so every note in the map speaks.
pub(crate) const LAYOUT: [Articulation; 34] = [
    Articulation::KickFelt,
    Articulation::KickWood,
    Articulation::KickDeep,
    Articulation::KickMuted,
    Articulation::SnareCentre,
    Articulation::SnareEdge,
    Articulation::SnareGhost,
    Articulation::SnareBrush,
    Articulation::SnareRimshot,
    Articulation::SnareFlam,
    Articulation::CrossStick,
    Articulation::TomHigh,
    Articulation::TomMid,
    Articulation::TomFloor,
    Articulation::CongaHigh,
    Articulation::CongaMid,
    Articulation::CongaLow,
    Articulation::CongaSlap,
    Articulation::HatClosed,
    Articulation::HatClosedEdge,
    Articulation::HatPedal,
    Articulation::HatOpen,
    Articulation::HatOpenLoose,
    Articulation::HatHalf,
    Articulation::RideBow,
    Articulation::RideBell,
    Articulation::RideEdge,
    Articulation::CrashThin,
    Articulation::CrashDark,
    Articulation::China,
    Articulation::Splash,
    Articulation::CowbellMid,
    Articulation::CowbellHigh,
    Articulation::CowbellLow,
];

/// Which articulation a note plays.
///
/// **The decisions this table is**, in the places the General MIDI map has no
/// slot for something a drummer plays:
///
/// * **37 Side Stick is the cross-stick**, which is what that GM name means and
///   what the stick laid across the head and struck on the rim actually is. It
///   is nearly all shell and almost no head.
/// * **40 Electric Snare is the rimshot.** There is no electric snare on an
///   acoustic kit, and 40 is where a part puts its loud backbeat, which on a
///   real kit is a rimshot — stick across head and rim together.
/// * **39 Hand Clap is a flam.** These kits have no clap in them. A part that
///   layers 38 and 39 wants two attacks on the backbeat, and two strokes 24 ms
///   apart is what a drummer gives it.
/// * **62 Mute Hi Conga is the slap.** The map's three conga slots are mute,
///   open and low; the slap is the fourth articulation every conga part uses
///   and the map has nowhere for it. The mute slot is the only closed-hand
///   conga in the map, and slap is the closed-hand articulation a part means.
/// * **The bongos and timbales fold onto the congas**, high and mid, because
///   they are the same thing physically: a single head over an open shell.
/// * **Everything shaken, scraped or blown is played on the kit.** There is no
///   tambourine, cabasa, maracas, vibraslap, guiro or whistle in a jazz, funk
///   or studio drum kit, so those notes land on the nearest thing there is —
///   the shakers on a hat edge, the scraped ones on a half-open hat, which is
///   the kit's own rattle, and the whistles on the ride bell, which is its
///   only sustained pitched metal. This is the same decision `kit_606` makes
///   for the voices a TR-606 does not have, and for the same reason: switching
///   a finished part to this kit should play it on what is here, not delete
///   half of it.
///
/// The consequence for the panel is that the CLAP fader is dead on all three
/// kits, and it is dead because nothing is played from it — see
/// `a_dead_fader_is_one_with_nothing_behind_it`.
pub(crate) fn articulation(sound: DrumSound) -> Articulation {
    use Articulation as A;
    use DrumSound as S;
    match sound {
        // The sub-kick range is the kick at other tunings and other beaters:
        // the lowest of it is the big drum, the top of it the tight one.
        S::SubKick(mult) => {
            if mult < 0.70 {
                A::KickDeep
            } else if mult < 0.90 {
                A::KickMuted
            } else {
                A::KickFelt
            }
        }
        S::Kick => A::KickWood,
        S::Snare => A::SnareCentre,
        S::SnareAlt => A::SnareRimshot,
        S::Clap => A::SnareFlam,
        S::Rimshot | S::Clave => A::CrossStick,
        S::LowTom => A::TomFloor,
        S::MidTom => A::TomMid,
        S::HighTom => A::TomHigh,
        // GM 62, Mute Hi Conga, is the map's one closed-hand conga and is the
        // only slot a slap can go in. It is the one conga carried at 350 Hz.
        S::Conga(f) if (f - 350.0).abs() < 1.0 => A::CongaSlap,
        S::Conga(f) | S::Bongo(f) | S::Timbale(f) => {
            if f < 260.0 {
                A::CongaLow
            } else if f < 380.0 {
                A::CongaMid
            } else {
                A::CongaHigh
            }
        }
        S::ClosedHat => A::HatClosed,
        S::PedalHat => A::HatPedal,
        S::OpenHat => A::HatOpen,
        S::Ride => A::RideBow,
        S::RideBell => A::RideBell,
        S::Crash => A::CrashThin,
        S::Cymbal => A::China,
        S::Splash => A::Splash,
        S::Cowbell => A::CowbellMid,
        S::Agogo(f) => {
            if f > 750.0 {
                A::CowbellHigh
            } else {
                A::CowbellLow
            }
        }
        // Shaken: the shortest brightest thing on the kit.
        S::Tambourine | S::Maracas | S::Cabasa => A::HatClosedEdge,
        // Scraped and rattled: the kit's own rattle is a half-open hat.
        S::Vibraslap | S::Guiro(_) => A::HatHalf,
        // Blown: the only pitched metal that sustains.
        S::Whistle(_) => A::RideBell,
        // Notes 76 and up: the whole kit again, one articulation per key.
        S::FxNoise(v) => {
            let step = (v * 51.0).round().max(0.0) as usize;
            LAYOUT[step % LAYOUT.len()]
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// How each articulation strikes what it is played on
// ══════════════════════════════════════════════════════════════════════════════

/// Which piece of the kit an articulation is played on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Piece {
    Kick,
    Snare,
    Tom(usize),
    Conga(usize),
    Ride,
    Crash(usize),
    Splash,
    China,
    Hat,
    Cowbell,
}

/// What the stick does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// An ordinary stroke.
    Stick,
    /// Head and rim together: the rim adds a high, hard, quickly damped
    /// partial that the head never has on its own.
    Rimshot,
    /// The stick laid flat across the head with its shoulder on the rim, so
    /// almost nothing reaches the membrane and the shell is the whole sound.
    CrossStick,
    /// Two strokes, the grace note ahead of the main one.
    Flam,
    /// Wire brushes: contact spread over an area and over time rather than a
    /// point impulse, which is why a brush has no attack transient to speak of
    /// and lives almost entirely in the high modes.
    Brush,
    /// A flat hand at the edge of a conga: short, hard, and mostly the drum's
    /// upper modes.
    Slap,
}

/// One articulation, fully described: which piece, where on it, with what, and
/// what the other hand is doing to it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Strike {
    pub(crate) on: Piece,
    /// Where the strike lands, as a fraction of the radius. Zero is the dead
    /// centre of a head and the dome of a cymbal; one is the rim and the edge.
    ///
    /// On a membrane this is not a tone control, it is the mode shape: mode
    /// (n,m) is driven by `J_n(j_{n,m}·at)`, so a strike in the exact centre
    /// reaches the two axisymmetric modes and *nothing else*, which is why a
    /// kick beater in the middle of the head gives a pure low thump and the
    /// same drum struck halfway out gives a tom.
    pub(crate) at: f64,
    /// How long the stick, beater or hand is in contact, seconds. This is what
    /// decides how far up the mode series the strike reaches: a wood tip at
    /// half a millisecond drives everything, a felt beater at four
    /// milliseconds rolls off above a few hundred Hz.
    pub(crate) contact: f64,
    /// Relative force.
    pub(crate) force: f64,
    /// Ring time multiplier: a hand on the head, a muffling ring, a choke.
    pub(crate) ring: f64,
    /// Extra tilt on the high modes, which is what a damping ring does — it
    /// sits at the edge, where the high modes have their antinodes.
    pub(crate) damp: f64,
    /// How hard the hi-hat is clamped, 0 fully open and 1 fully shut. Ignored
    /// on everything that is not a hat.
    pub(crate) clamp: f64,
    pub(crate) kind: Kind,
}

const STROKE: Strike = Strike {
    on: Piece::Snare,
    at: 0.4,
    contact: 0.0009,
    force: 1.0,
    ring: 1.0,
    damp: 0.0,
    clamp: 0.0,
    kind: Kind::Stick,
};

/// How each articulation is played.
///
/// The three kits share this table and differ in the [`Kit`] the strikes land
/// on, which is the right split: a rimshot is a rimshot on any kit, and what
/// makes a funk rimshot different from a jazz one is the drum.
pub(crate) fn strike_of(a: Articulation) -> Strike {
    use Articulation as A;
    match a {
        // ── Kicks. Four, and the difference between them is the beater and
        // what is inside the shell, not the tuning alone.
        A::KickFelt => Strike {
            on: Piece::Kick,
            at: 0.0,
            contact: 0.0042,
            ..STROKE
        },
        A::KickWood => Strike {
            on: Piece::Kick,
            at: 0.10,
            contact: 0.0012,
            force: 1.05,
            ..STROKE
        },
        A::KickDeep => Strike {
            on: Piece::Kick,
            at: 0.0,
            contact: 0.0055,
            force: 1.1,
            ring: 1.45,
            ..STROKE
        },
        A::KickMuted => Strike {
            on: Piece::Kick,
            at: 0.06,
            contact: 0.0022,
            ring: 0.42,
            damp: 1.3,
            ..STROKE
        },
        // ── Snares.
        A::SnareCentre => Strike { on: Piece::Snare, at: 0.38, ..STROKE },
        A::SnareEdge => Strike {
            on: Piece::Snare,
            at: 0.82,
            contact: 0.0007,
            force: 0.9,
            ring: 0.8,
            ..STROKE
        },
        A::SnareGhost => Strike {
            on: Piece::Snare,
            at: 0.55,
            contact: 0.0011,
            force: 0.28,
            ..STROKE
        },
        A::SnareBrush => Strike {
            on: Piece::Snare,
            at: 0.5,
            contact: 0.012,
            force: 0.6,
            kind: Kind::Brush,
            ..STROKE
        },
        A::SnareRimshot => Strike {
            on: Piece::Snare,
            at: 0.72,
            contact: 0.0005,
            force: 1.3,
            kind: Kind::Rimshot,
            ..STROKE
        },
        A::SnareFlam => Strike { on: Piece::Snare, at: 0.42, kind: Kind::Flam, ..STROKE },
        A::CrossStick => Strike {
            on: Piece::Snare,
            at: 0.95,
            contact: 0.0006,
            force: 0.75,
            kind: Kind::CrossStick,
            ..STROKE
        },
        // ── Toms. Struck between the centre and the edge, which is where a
        // tom is played and what gives it its (1,1) mode.
        A::TomHigh => Strike { on: Piece::Tom(2), at: 0.45, contact: 0.0011, ..STROKE },
        A::TomMid => Strike { on: Piece::Tom(1), at: 0.45, contact: 0.0012, ..STROKE },
        A::TomFloor => Strike { on: Piece::Tom(0), at: 0.42, contact: 0.0014, ..STROKE },
        // ── Congas. A hand, not a stick, so the contact is long — and the
        // slap is the same hand held stiff at the edge, which is a tenth of
        // the contact time and most of the reason it cracks.
        A::CongaHigh => Strike {
            on: Piece::Conga(2),
            at: 0.62,
            contact: 0.0035,
            ..STROKE
        },
        A::CongaMid => Strike {
            on: Piece::Conga(1),
            at: 0.62,
            contact: 0.0038,
            ..STROKE
        },
        A::CongaLow => Strike {
            on: Piece::Conga(0),
            at: 0.58,
            contact: 0.0045,
            ..STROKE
        },
        // A flat stiff hand at the very edge. Nothing reaches the drum's
        // lowest mode from there — J_0 has all but died by 0.88 of the
        // radius — so it takes force to be heard, which is what a slap is.
        A::CongaSlap => Strike {
            on: Piece::Conga(2),
            at: 0.88,
            contact: 0.0004,
            force: 1.7,
            ring: 0.45,
            damp: 0.5,
            kind: Kind::Slap,
            ..STROKE
        },
        // ── Hats. One pair of cymbals, and the articulation is how hard the
        // pedal is down.
        A::HatClosed => Strike {
            on: Piece::Hat,
            at: 0.72,
            contact: 0.00005,
            clamp: 1.0,
            ..STROKE
        },
        A::HatClosedEdge => Strike {
            on: Piece::Hat,
            at: 0.96,
            contact: 0.00004,
            force: 1.1,
            clamp: 0.9,
            ..STROKE
        },
        // Not a stick on a plate: the two plates hitting each other, edge to
        // edge, which is a short hard contact with a lot of low air in it.
        A::HatPedal => Strike {
            on: Piece::Hat,
            at: 0.9,
            contact: 0.00016,
            force: 0.85,
            clamp: 1.0,
            ..STROKE
        },
        A::HatOpen => Strike {
            on: Piece::Hat,
            at: 0.74,
            contact: 0.00005,
            clamp: 0.0,
            ..STROKE
        },
        A::HatOpenLoose => Strike {
            on: Piece::Hat,
            at: 0.9,
            contact: 0.00004,
            force: 1.1,
            ring: 1.35,
            clamp: 0.0,
            ..STROKE
        },
        A::HatHalf => Strike {
            on: Piece::Hat,
            at: 0.8,
            contact: 0.00005,
            clamp: 0.42,
            ..STROKE
        },
        // ── Ride. One plate, three places on it.
        A::RideBow => Strike { on: Piece::Ride, at: 0.6, contact: 0.00006, ..STROKE },
        A::RideBell => Strike {
            on: Piece::Ride,
            at: 0.12,
            contact: 0.00008,
            force: 2.6,
            ..STROKE
        },
        A::RideEdge => Strike {
            on: Piece::Ride,
            at: 0.98,
            contact: 0.00009,
            force: 1.2,
            ring: 1.2,
            ..STROKE
        },
        // ── Crashes and the rest of the metal.
        A::CrashThin => Strike { on: Piece::Crash(0), at: 0.95, contact: 0.00006, ..STROKE },
        A::CrashDark => Strike { on: Piece::Crash(1), at: 0.92, contact: 0.00008, ..STROKE },
        A::China => Strike { on: Piece::China, at: 0.97, contact: 0.00006, force: 1.1, ..STROKE },
        A::Splash => Strike { on: Piece::Splash, at: 0.9, contact: 0.00005, ..STROKE },
        A::CowbellMid => Strike { on: Piece::Cowbell, at: 0.5, contact: 0.0004, ..STROKE },
        A::CowbellHigh => Strike {
            on: Piece::Cowbell,
            at: 0.3,
            contact: 0.00035,
            ring: 0.8,
            ..STROKE
        },
        A::CowbellLow => Strike {
            on: Piece::Cowbell,
            at: 0.7,
            contact: 0.0005,
            ring: 1.2,
            ..STROKE
        },
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// The snare's wires
// ══════════════════════════════════════════════════════════════════════════════

/// How many groups of strands are modelled separately. Three, with different
/// masses and different gaps, so that they never land together and the rattle
/// does not turn into a tone.
pub(crate) const WIRE_GROUPS: usize = 3;

/// Twenty strands of coiled wire lying on the bottom head, as three bouncing
/// masses.
///
/// Tightening the strainer makes them rattle *less*, not more, and that is the
/// direction a snare drum goes: pulling the strands hard against the head is
/// what stops them leaving it, so they stay in contact instead of bouncing and
/// the drum cracks rather than sizzles. Over-tighten a real snare and it
/// chokes; that is this, at the end of the SNAPPY knob.
///
/// Each group falls under a restoring force, lands on the head wherever the
/// head happens to be, and bounces. Nothing here is an envelope: the sound is
/// the impacts, and everything the wires do musically falls out of when the
/// impacts happen.
///
/// * A **soft hit** barely lifts them, so they land often and evenly: a dense
///   fine buzz.
/// * A **hard hit** throws them clear. They are in the air while the drum is
///   at its loudest and come back afterwards, which is the *choke* — the
///   thing that makes a hard backbeat crack rather than sizzle.
/// * The head stops before they do, so they are still landing after the drum
///   has gone: the **ring on**.
///
/// One way only. The wires read the resonant head and never write to it, so
/// this cannot form a loop and no setting of any knob can make it run away —
/// which matters more here than the small amount of damping the wires really
/// do add to the head, and that is carried as a fixed loss in the voicing
/// instead.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Wires {
    /// Height of each group above the head, in the same units the head's
    /// displacement is measured in.
    y: [f64; WIRE_GROUPS],
    /// Vertical velocity.
    v: [f64; WIRE_GROUPS],
    /// The head's position last sample, for its velocity.
    last_head: f64,
    /// The restoring acceleration this strainer setting gives.
    gravity: f64,
    /// How far the gaps close up as the strainer is tightened.
    gap: f64,
    /// The speed a strand lands at when it simply falls from its rest gap,
    /// which is what an impact is measured against. Without it the impacts
    /// are in head-velocity units and the strands are a hundred times louder
    /// than the drum.
    reference: f64,
    /// Energy in the strands, kicked by each impact and decaying between them.
    energy: f64,
    /// The impact this sample, before it is turned into sound. Read by the
    /// tests that count contacts.
    pub(crate) impact: f64,
    /// How many impacts there have been. Free, and the plainest evidence that
    /// the contact really is intermittent.
    pub(crate) contacts: u32,
}

/// Restoring acceleration on a strand — the strainer pulling it back against
/// the head, which is far stronger than gravity — in head-displacement units
/// per second squared.
///
/// This is the number that sets both timescales the strands have. A strand
/// resting at its gap `h` is back on the head in `2√(2h/g)`, which at the gaps
/// below is two to four milliseconds and is the buzz rate of a snare played
/// softly; and a strand thrown at velocity `v` by a hard stroke is airborne for
/// `2v/g`, which at a backbeat's head velocity is ten to twenty milliseconds
/// and is the choke.
const WIRE_G: f64 = 60_000.0;

/// How much of a bounce comes back. Coiled wire against a plastic head is not
/// elastic.
const WIRE_RESTITUTION: f64 = 0.42;

/// Below this the strand is resting on the head rather than landing on it,
/// measured as a multiple of the velocity one sample of the restoring force
/// adds. Any lower and a strand that has come to rest re-collides on every
/// sample and the buzz becomes a tone at half the sample rate; any higher and
/// it stops buzzing while it still should be. Expressed against the sample
/// period rather than as a constant so that the rate the host runs at cannot
/// change how long the strands ring.
const WIRE_FLOOR: f64 = 3.0;

/// How fast the energy from one impact decays. 1.4 ms, which is short enough
/// that impacts stay separate ticks rather than smearing into a wash.
const WIRE_TAU: f64 = 0.0014;

/// The gap each group rests at, and how strongly the head drives it. Different
/// on purpose: strands under different tension across a twenty-strand set, so
/// that they never land together and the rattle stays a rattle.
const WIRE_GAP: [f64; WIRE_GROUPS] = [0.045, 0.076, 0.118];
const WIRE_DRIVE: [f64; WIRE_GROUPS] = [1.0, 0.72, 0.55];

impl Wires {
    pub(crate) const fn new() -> Self {
        Self {
            y: [0.0; WIRE_GROUPS],
            v: [0.0; WIRE_GROUPS],
            last_head: 0.0,
            gravity: WIRE_G,
            gap: 1.0,
            reference: 1.0,
            energy: 0.0,
            impact: 0.0,
            contacts: 0,
        }
    }

    /// Set the strainer and put the strands back on the head.
    ///
    /// `tension` is the SNAPPY knob: tighter strands sit closer to the head
    /// and are pulled back to it harder, which is more contacts per unit time
    /// and a shorter choke after a hard stroke.
    pub(crate) fn arm(&mut self, tension: f64) {
        let tension = tension.clamp(0.0, 1.0);
        *self = Self::new();
        self.y = WIRE_GAP;
        self.gravity = WIRE_G * (0.55 + tension * 1.4);
        self.gap = 1.0 - tension * 0.55;
        self.reference = (2.0 * self.gravity * WIRE_GAP[1] * self.gap).sqrt();
    }

    /// One sample. `head` is the resonant head's displacement; the return is
    /// the strands' contribution before it is filtered.
    #[inline]
    pub(crate) fn tick(&mut self, head: f64, dt: f64) -> f64 {
        let head_v = (head - self.last_head) / dt;
        self.last_head = head;
        self.impact = 0.0;
        let gravity = self.gravity;
        let floor = gravity * dt * WIRE_FLOOR;
        for i in 0..WIRE_GROUPS {
            let head_local = head_v * WIRE_DRIVE[i];
            self.v[i] -= gravity * dt;
            self.y[i] += self.v[i] * dt;
            let rest = head * WIRE_DRIVE[i] + WIRE_GAP[i] * self.gap;
            if self.y[i] <= rest {
                let relative = self.v[i] - head_local;
                self.y[i] = rest;
                if relative < -floor {
                    self.impact -= relative / self.reference;
                    self.contacts += 1;
                    self.v[i] = head_local - WIRE_RESTITUTION * relative;
                } else {
                    // Resting on the head, not landing on it. It rides the
                    // head from here until the head throws it clear again.
                    self.v[i] = head_local;
                }
            }
        }
        // Each impact kicks the strands; between impacts the kick decays. The
        // ticks stay separate, which is what a buzz is.
        self.energy = self.energy * (-dt / WIRE_TAU).exp() + self.impact * 0.9;
        self.energy
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Cymbal mode layout
// ══════════════════════════════════════════════════════════════════════════════

/// How far each mode of a plate sits off the rule that places it, as a
/// fraction. Fixed rather than random, so a cymbal is the same cymbal on every
/// hit and two kits' cymbals differ because their plates differ.
///
/// A cymbal is not a mathematical plate: it is spun, hammered and lathed, and
/// every one of those breaks the symmetry the rule assumes. Without this the
/// modes fall on a smooth curve and the result rings like a bell rather than
/// crashing like a cymbal.
pub(crate) const SCATTER: [f64; MODES] = [
    0.000, 0.031, -0.024, 0.047, -0.038, 0.019, 0.055, -0.041, 0.028, -0.017, 0.062, -0.033,
    0.021, 0.049, -0.052, 0.036, -0.026, 0.058, -0.019, 0.043, -0.047, 0.024, 0.051, -0.035,
    0.018, -0.056, 0.039, -0.022, 0.060, -0.030, 0.045, -0.048, 0.026, 0.053, -0.037, 0.020,
    -0.044, 0.057, -0.028, 0.034,
];

/// Where mode `k` of a plate sits, as a ratio to the lowest.
///
/// Counting *every* mode of a flat plate rather than one family of them, the
/// modal density is constant in frequency — the modes are evenly spaced, which
/// is an exponent of 1 here. A cymbal is not flat: the dome and the taper
/// stiffen it unevenly and push the count up with frequency, which raises the
/// exponent a little above 1. It is the number that decides **mode density**,
/// and density is what makes a big thin crash a wash and a small thick splash
/// a chirp.
///
/// This used to be near 2, which is Chladni's law for one family — `f ∝
/// (m+2n)²` — applied as though it placed the whole set. It does not, and the
/// cost of the mistake was that a crash had four modes above 6 kHz instead of
/// twenty: measured, *nothing* above 6 kHz survived a hit, and the cymbals
/// were all wash and no metal.
pub(crate) fn plate_ratio(k: usize, spread: f64, scatter: f64) -> f64 {
    let base = ((k + 1) as f64).powf(spread);
    base * (1.0 + scatter * SCATTER[k.min(MODES - 1)])
}

/// How far in the gated modes are at this strike energy.
///
/// Below `open` they are not there at all — not quiet, *absent*, which is the
/// DAFx-19 paper's point and the difference between a cymbal blooming and a
/// cymbal getting louder. Between `open` and `full` they come in over a short
/// ramp, so the transition is not a step.
pub(crate) fn gate(energy: f64, open: f64, full: f64) -> f64 {
    if energy <= open {
        0.0
    } else if energy >= full {
        1.0
    } else {
        let x = (energy - open) / (full - open).max(1e-6);
        // Smoothstep: no corner at either end.
        x * x * (3.0 - 2.0 * x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 44_100.0;

    /// The series this file starts from, checked against the definition rather
    /// than against itself: `J_n(j_{n,m}) = 0` is what a Bessel zero *is*.
    #[test]
    fn the_bessel_zeros_are_zeros_of_the_bessel_functions() {
        for (k, &zero) in BESSEL_ZERO.iter().enumerate() {
            let at = bessel_j(BESSEL_ORDER[k], zero);
            assert!(at.abs() < 5e-4, "J_{}({zero}) = {at}", BESSEL_ORDER[k]);
        }
        // ...and the ratios they give are the ideal membrane's, which is the
        // set the module docs quote.
        const IDEAL: [f64; 8] = [1.0, 1.593, 2.136, 2.295, 2.653, 2.917, 3.155, 3.500];
        for (k, &want) in IDEAL.iter().enumerate() {
            let got = BESSEL_ZERO[k] / BESSEL_ZERO[0];
            assert!((got - want).abs() < 0.002, "mode {k}: {got} against {want}");
        }
    }

    /// A beater in the dead centre of a head reaches the two axisymmetric
    /// modes and *nothing else*, because every other mode has as much head
    /// rising as falling at the middle and the two cancel exactly.
    ///
    /// This is why a kick with the beater in the middle of the head is a pure
    /// low thump and the same drum struck halfway out is a tom — not a filter
    /// setting, a different set of modes.
    #[test]
    fn a_strike_in_the_centre_reaches_only_the_axisymmetric_modes() {
        for k in 0..8 {
            let centre = bessel_j(BESSEL_ORDER[k], BESSEL_ZERO[k] * 0.0);
            if BESSEL_ORDER[k] == 0 {
                assert!((centre - 1.0).abs() < 1e-12, "mode {k} at the centre is {centre}");
            } else {
                assert!(centre.abs() < 1e-12, "mode {k} at the centre is {centre}");
            }
            // Halfway out, everything answers.
            let half = bessel_j(BESSEL_ORDER[k], BESSEL_ZERO[k] * 0.5);
            assert!(half.abs() > 0.02, "mode {k} halfway out is {half}");
        }
    }

    /// Air loading lowers every mode and lowers the bottom of the series
    /// hardest, so the ideal ratios come out **stretched**.
    ///
    /// The claim the module docs make, checked as an ordering rather than
    /// against a measurement: with no air there is no change, and with air
    /// every ratio above the fundamental has moved up.
    #[test]
    fn air_loading_stretches_the_ideal_ratios() {
        let mu = 0.75;
        for (k, &zero) in BESSEL_ZERO.iter().enumerate() {
            let ideal = zero / BESSEL_ZERO[0];
            assert!((loaded_ratio(k, 0.0) - ideal).abs() < 1e-12, "no air moved mode {k}");
            if k == 0 {
                continue;
            }
            let loaded = loaded_ratio(k, mu) / loaded_ratio(0, mu);
            assert!(loaded > ideal * 1.02, "mode {k}: {loaded:.3} against the ideal {ideal:.3}");
        }
        // The fundamental itself is pulled *down*, hard — this is the "air
        // loading significantly lowers the modal frequencies" of the
        // literature, and at this loading it is three semitones.
        let drop = loaded_ratio(0, mu);
        assert!((0.74..0.78).contains(&drop), "the (0,1) mode landed at {drop:.3} of its ideal");
    }

    /// The two heads are the eigenproblem this file says they are.
    ///
    /// Checked against the closed form for the case that has one: two heads at
    /// the same tension come out at `ω` and `ω√(1+2k)` — the lower mode is the
    /// pair moving the same way in space, which changes the enclosed volume by
    /// nothing and therefore never feels the air spring at all.
    #[test]
    fn the_two_heads_are_the_eigenproblem_this_file_says() {
        for k in [0.05f64, 0.2, 0.42, 0.9] {
            let [(lo, lo_batter), (hi, hi_batter)] = couple(100.0, 100.0, k);
            assert!((lo - 100.0).abs() < 1e-9, "the volume-preserving mode moved to {lo}");
            let want = 100.0 * (1.0 + 2.0 * k).sqrt();
            assert!((hi - want).abs() < 1e-9, "the air-spring mode is {hi}, want {want}");
            // Equal tensions, so each mode is half of each head.
            for share in [lo_batter, hi_batter] {
                assert!((share - 0.5f64.sqrt()).abs() < 1e-9, "head share {share}");
            }
        }
        // With the heads at different tensions the cancellation is not exact,
        // so the air spring lifts *both* modes — the lower one a little and
        // the upper one a lot — and widens the gap between them. What holds
        // whatever the tunings are is that neither mode ends up below the
        // lower head and the split only ever opens as the cavity stiffens.
        let mut last = 0.0;
        for k in [0.0f64, 0.1, 0.3, 0.6] {
            let [(lo, _), (hi, _)] = couple(59.0, 66.5, k);
            assert!(lo >= 59.0 - 1e-9, "k={k}: the lower mode fell to {lo:.1}");
            assert!(hi >= 66.5 - 1e-9 && hi > lo, "k={k}: {lo:.1} {hi:.1}");
            let split = hi / lo;
            assert!(split > last, "k={k} did not widen the split: {split:.3}");
            last = split;
        }
        // With no cavity at all the pair is just the two heads.
        let [(lo, _), (hi, _)] = couple(59.0, 66.5, 0.0);
        assert!((lo - 59.0).abs() < 1e-9 && (hi - 66.5).abs() < 1e-9);
        // A single-headed drum has one mode and no partner to split with.
        let [(lo, share), _] = couple(200.0, 200.0, 0.0);
        assert!((lo - 200.0).abs() < 1e-9 && share > 0.0);
    }

    /// The strands bounce; they do not buzz at one rate.
    ///
    /// Three things a static noise burst cannot do, all of them measured on
    /// the contact counter rather than on the sound:
    ///
    /// * a **soft** stroke gives a dense, even rattle — contacts in every
    ///   window while the head is moving;
    /// * a **hard** stroke throws the strands clear, so the drum is at its
    ///   loudest with the wires *off* it, and they come back afterwards. That
    ///   is the choke, and it is why a hard backbeat cracks;
    /// * and a hard stroke is still landing strands long after the head has
    ///   gone, where a soft one has settled. That is the ring on.
    #[test]
    fn the_strands_bounce_rather_than_buzzing_at_one_rate() {
        const BOUNDS: [f64; 6] = [0.0, 0.005, 0.020, 0.060, 0.150, 0.400];
        let run = |amp: f64| {
            let mut w = Wires::new();
            w.arm(0.5);
            let mut counts = [0u32; 5];
            let mut band = 0;
            for n in 0..(0.4 * SR) as usize {
                let t = n as f64 / SR;
                while band + 1 < 5 && t >= BOUNDS[band + 1] {
                    band += 1;
                }
                // A head ringing at 190 Hz and dying over 90 ms, which is what
                // the resonant head of a snare does.
                let head = amp * (-t / 0.09).exp() * (TAU * 190.0 * t).sin();
                let before = w.contacts;
                w.tick(head, 1.0 / SR);
                counts[band] += w.contacts - before;
            }
            counts
        };

        let soft = run(0.15);
        let hard = run(0.70);

        // Soft: contacts all the way through the drum's ring, and the strands
        // are on the head as much at 10 ms as at 40.
        assert!(soft[1] >= 8, "a soft stroke rattled {} times in 5-20 ms", soft[1]);
        assert!(soft[2] >= 8, "a soft stroke rattled {} times in 20-60 ms", soft[2]);
        assert!(
            soft[1] * 2 >= soft[2],
            "a soft stroke choked: {} in 5-20 ms against {} in 20-60",
            soft[1],
            soft[2]
        );

        // Hard: the strands are in the air over the loudest part of the hit.
        assert!(hard[1] <= 3, "a hard stroke did not choke: {} contacts in 5-20 ms", hard[1]);
        assert!(
            hard[3] >= 8 * hard[1].max(1),
            "the strands never came back: {} in 5-20 ms, {} in 60-150",
            hard[1],
            hard[3]
        );
        // ...and they are still landing when the drum has stopped. At 150 ms
        // the head is five time constants down.
        assert!(hard[4] >= 20, "the strands stopped with the drum: {} after 150 ms", hard[4]);
        assert_eq!(soft[4], 0, "a soft stroke should have settled by 150 ms");
    }

    /// A tight strainer rattles *less* than a slack one, and stops sooner.
    ///
    /// This is the direction the model came out and it is the direction a
    /// snare drum goes: pulling the strands hard against the head is what
    /// stops them leaving it, so they stay in contact instead of bouncing and
    /// the drum cracks rather than sizzles. Over-tighten a real snare and it
    /// chokes; that is this, at the end of the knob.
    ///
    /// SNAPPY is therefore two controls in one, which is what the strainer
    /// really is: it sets the strands' level, in `build_membrane`, and it sets
    /// how freely they bounce, here.
    #[test]
    fn a_tighter_strainer_rattles_less_freely() {
        let run = |tension: f64| {
            let mut w = Wires::new();
            w.arm(tension);
            let mut last = 0.0;
            for n in 0..(0.4 * SR) as usize {
                let t = n as f64 / SR;
                let before = w.contacts;
                w.tick(0.30 * (-t / 0.09).exp() * (TAU * 190.0 * t).sin(), 1.0 / SR);
                if w.contacts > before {
                    last = t;
                }
            }
            (w.contacts, last)
        };
        let (loose, loose_last) = run(0.0);
        let (tight, tight_last) = run(1.0);
        assert!(loose > tight * 3 / 2, "loose {loose} contacts, tight {tight}");
        assert!(
            loose_last > tight_last * 1.3,
            "loose settled at {loose_last:.3} s, tight at {tight_last:.3} s"
        );
    }

    /// The DAFx-19 cascade carries what [`ModalBank::couple`] says it does,
    /// and its chain gain is the per-link figure raised to the number of links.
    #[test]
    fn the_cascade_carries_what_it_says_and_no_more() {
        let transfer = |amount: f64| {
            let mut b = ModalBank::new();
            b.set(0, 300.0, 2.0, 1.0, 1.0, 0.0, SR);
            // Driven by nothing but its partner.
            b.set(1, 900.0, 2.0, 0.0, 1.0, 0.0, SR);
            b.couple(1, amount);
            let (mut source, mut coupled) = (0.0f64, 0.0f64);
            for n in 0..(0.5 * SR) as usize {
                b.tick(if n == 0 { 1.0 } else { 0.0 });
                source = source.max(b.im[0].abs());
                coupled = coupled.max(b.im[1].abs());
            }
            coupled / source
        };
        assert_eq!(transfer(0.0), 0.0, "an uncoupled bank moved energy anyway");
        // Linear in the coefficient, and the constant is the transient
        // overshoot of a resonator driven off its own frequency: the steady
        // state is `amount` and the first swing is about 1.6 times it.
        let a = transfer(0.2);
        let b = transfer(0.5);
        assert!((a / 0.2 - 1.586).abs() < 0.05, "0.2 transferred {a:.4}");
        assert!((b / 0.5 - 1.586).abs() < 0.05, "0.5 transferred {b:.4}");
        // ...and it is a chain, so two links carry the square of one.
        let mut bank = ModalBank::new();
        bank.set(0, 300.0, 2.0, 1.0, 1.0, 0.0, SR);
        bank.set(1, 900.0, 2.0, 0.0, 1.0, 0.0, SR);
        bank.set(2, 1500.0, 2.0, 0.0, 1.0, 0.0, SR);
        bank.couple(1, 0.4);
        let (mut first, mut second) = (0.0f64, 0.0f64);
        for n in 0..(0.5 * SR) as usize {
            bank.tick(if n == 0 { 1.0 } else { 0.0 });
            first = first.max(bank.im[1].abs());
            second = second.max(bank.im[2].abs());
        }
        assert!(second < first, "the second link carried more than the first");
        assert!(second > first * 0.2, "the chain died at the second link");
    }

    /// Every plate in every acoustic kit couples below unity, which is what
    /// keeps the chain above shrinking rather than growing.
    #[test]
    fn no_voicing_couples_at_or_above_unity() {
        for kit in [&super::super::kit_jazz::KIT, &super::super::kit_funk::KIT, &super::super::kit_studio::KIT] {
            for p in [&kit.ride, &kit.crash[0], &kit.crash[1], &kit.splash, &kit.china, &kit.hat] {
                assert!(p.cascade < 1.0, "a plate couples at {}", p.cascade);
                assert!(p.cascade_span >= 1);
            }
        }
    }

    /// A gated mode below the threshold is *absent*, not quiet — which is what
    /// makes a cymbal bloom rather than get louder.
    #[test]
    fn the_gate_is_absence_and_not_attenuation() {
        assert_eq!(gate(0.0, 0.3, 0.8), 0.0);
        assert_eq!(gate(0.3, 0.3, 0.8), 0.0);
        assert_eq!(gate(0.8, 0.3, 0.8), 1.0);
        assert_eq!(gate(1.0, 0.3, 0.8), 1.0);
        // Smooth across the ramp, so there is no step to click on.
        let mut last = 0.0;
        let mut x = 0.30;
        while x <= 0.80 {
            let g = gate(x, 0.3, 0.8);
            assert!(g >= last && (0.0..=1.0).contains(&g));
            last = g;
            x += 0.01;
        }
        assert!((gate(0.55, 0.3, 0.8) - 0.5).abs() < 1e-9, "the ramp is not centred");
    }

    /// Every articulation is reachable, lands on a strip, and is played from
    /// somewhere in the note map.
    #[test]
    fn the_articulation_table_is_complete_and_reachable() {
        use crate::drum_rack::note_to_sound;
        let mut seen = [false; LAYOUT.len()];
        for note in 0u8..128 {
            let a = articulation(note_to_sound(note));
            let index = LAYOUT.iter().position(|&x| x == a).expect("not in the layout");
            seen[index] = true;
        }
        for (i, &hit) in seen.iter().enumerate() {
            assert!(hit, "{:?} is not reachable from any note", LAYOUT[i]);
        }
        // Notes 76 and up are the whole kit again, one articulation per key.
        for (i, &a) in LAYOUT.iter().enumerate() {
            let note = 76 + i as u8;
            assert_eq!(articulation(note_to_sound(note)), a, "note {note}");
        }
        // ...and above the end of the table it wraps rather than falling off.
        assert_eq!(
            articulation(note_to_sound(76 + LAYOUT.len() as u8)),
            LAYOUT[0],
            "the layout did not wrap"
        );
        assert_eq!(articulation(note_to_sound(127)), LAYOUT[(127 - 76) % LAYOUT.len()]);
    }
}
