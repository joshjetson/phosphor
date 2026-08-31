//! Eight-band parametric EQ.
//!
//! Every band can be any of eight types, every knob position has a
//! closed-form magnitude response, and [`ParametricEq::response_db`] returns
//! that response for the curve the terminal draws. "Does this EQ work" is a
//! numeric question here, not a listening one, and the test module at the
//! bottom of this file answers it against values generated from the
//! reference implementation rather than from this code.
//!
//! # Why the coefficients are not the cookbook's
//!
//! The RBJ Audio EQ Cookbook is the default answer for audio biquads and its
//! formulas are correct. Its problem is structural: it uses the bilinear
//! transform, which maps the analog `s = ∞` onto `z = −1`, squeezing every
//! analog frequency from the corner up to infinity into the interval between
//! the corner and Nyquist. Near Nyquist that squeeze is brutal.
//!
//! Ask for a bell at 16 kHz, +12 dB, Q 2, at 44.1 kHz. The analog filter it
//! is meant to be has its half-gain (+6 dB) points at 12 492 Hz and
//! 20 492 Hz — 0.714 octaves wide, Q 2.000. RBJ gives you 14 579 Hz to
//! 17 212 Hz: **0.240 octaves, effective Q 6.02**. The band is three times
//! narrower than the number on the knob says. What is *not* wrong is the
//! peak: RBJ hits +12.000 dB at exactly 16 000 Hz. That is why cramping
//! survives casual testing — a single-point check passes and the whole error
//! is in the shape.
//!
//! The same disease elsewhere, all at 44.1 kHz, as maximum error against the
//! analog prototype the knob is promising:
//!
//! | case | RBJ | what this module does |
//! |---|---|---|
//! | high shelf 16 kHz +12 dB | 3.63 dB, transition starts at 11 827 Hz instead of 8 252 | 0.37 dB, starts at 8 148 |
//! | lowpass 15 kHz Q 0.707, at 20 kHz | −22.91 dB where analog is −6.19 | −5.18 dB |
//! | bell 20 kHz +12 dB Q 3 | 8.76 dB | 0.82 dB |
//! | highpass 80 Hz Q 0.707 | 0.0002 dB | 0.0000 dB |
//!
//! That last row is the honest counterpoint: below about `f₀/f_s = 0.005`
//! the cookbook is perfect. Cramping is exclusively a high-frequency
//! problem, and a rumble filter does not care. A 16 kHz air band cares
//! enormously.
//!
//! So the bells, shelves, high-pass and low-pass here are **Vicanek matched
//! designs**: the poles come from impulse invariance (`z = e^s`), which by
//! construction does not warp the pole frequency or narrow the resonance
//! near Nyquist, and the numerator is then solved for directly. Orfanidis
//! solves the peaking case at least as well but has a hard feasibility
//! constraint — `f₀ < (1 − 1/(2Q))·f_Nyquist`, which makes a Q 0.5 bell
//! undesignable at *any* frequency and a Q 0.7 bell undesignable above
//! 6.3 kHz at 44.1 kHz. A wide "add some air" bell is the single most common
//! move in mixing, so that is disqualifying. The matched design has no
//! feasibility constraint at all.
//!
//! The one exception is the **notch, which is RBJ**. A notch is defined by an
//! exact zero on the unit circle; there is no published matched notch, and
//! deriving one from `1 − matched bandpass` fails badly — the depth collapses
//! to −7.6 dB at 8 kHz because the matched bandpass's deep skirts are not
//! accurate to one part in 10⁴ and subtracting from 1 amplifies exactly that
//! error. The RBJ notch's null is exact everywhere; only its *width* cramps,
//! and width is not what a notch is for. The all-pass is RBJ for the same
//! kind of reason: it has unity magnitude by construction at every frequency,
//! so there is nothing to cramp.
//!
//! # Why the arithmetic is `f64`, coefficients and state both
//!
//! Ask a 20 Hz, Q 8 bell for +12 dB at 192 kHz. Compute its coefficients
//! correctly, round them to `f32`, and the filter delivers **+5.75 dB** — not
//! noise, the wrong filter, off by half, before a single sample is processed.
//! A biquad's DC gain is `(b0+b1+b2)/(1+a1+a2)` and for a low-frequency band
//! both sums are `O(ω₀²)` formed by cancelling three numbers of magnitude
//! near 1. At 20 Hz / 96 kHz, `1+a1+a2 ≈ 1.7e-6`: about 19 bits destroyed.
//! `f32` has 24 mantissa bits, so five survive; `f64` has 53, so 34 do.
//!
//! This is a *different* problem from cramping and the two want opposite
//! remedies. Cramping is a design error that lives at the top of the spectrum
//! and gets better as the sample rate rises. Low-frequency conditioning is a
//! word-length error that lives at the bottom and gets *worse* as the sample
//! rate rises — oversampling, the folk cure for cramping, actively makes it
//! worse. Matched designs up top, `f64` down low.
//!
//! Splitting the damage: `f32` coefficients alone cost −47.7 dB of accuracy
//! on a 20 Hz Q 4 bell at 96 kHz; `f32` state alone costs −98.6 dB. It is the
//! coefficients that kill you, and the state is then free.
//!
//! # Direct Form I, and how the coefficients move
//!
//! `y[n] = b0·x[n] + b1·x[n−1] + b2·x[n−2] − a1·y[n−1] − a2·y[n−2]`, four
//! state words per section per channel. The folklore says transposed Direct
//! Form II for floating point; the folklore attributes that to Julius Smith,
//! who in fact says structure choice "is usually not critical" above 32 bits.
//! The real source is ARM's CMSIS-DSP, and its stated reason is TDF-II's
//! smaller *state*, not better accuracy. Two words per section is not worth
//! anything here, and DF-I buys something real: its state is literally past
//! inputs and past outputs — signal history with no dependence on the
//! coefficients — so when coefficients change mid-stream it applies the new
//! difference equation to the true history. TDF-II's state words are partial
//! sums computed with the *old* coefficients and are inconsistent with the
//! new ones the instant you swap. Measured, that is worth 15–26 dB less click
//! energy on Q and gain moves.
//!
//! Coefficients are recomputed once per [`CONTROL_BLOCK`] samples from
//! parameters smoothed with a 15 ms one-pole, and linearly interpolated
//! across the block. Both halves matter, and the interpolation is the cheap
//! half: with it, a 32-sample block is acoustically identical to recomputing
//! every sample (13.5 dB of click energy either way) and even a 512-sample
//! block is within 1 dB; without it, a 512-sample block is as bad as no
//! smoothing at all.
//!
//! **The parameter smoother is load-bearing, not polish.** Staying inside the
//! biquad stability triangle is not a proof of anything: frozen-coefficient
//! stability at every instant does not imply stability of the time-varying
//! system, and direct forms are known to diverge under *periodic* coefficient
//! interpolation even when every interpolated point is verified stable. What
//! prevents that here is the smoother — it turns any input, including an
//! adversarial square wave, into a slow monotone approach, so the
//! coefficients never oscillate at audio rate. Do not shorten it to make
//! parameters feel snappier, and do not let a preset load bypass it. There is
//! a divergence guard behind it anyway, one compare per band per block, which
//! turns a theoretical blow-up into an inaudible 0.7 ms mute.
//!
//! # Denormals
//!
//! An IIR filter's state decays exponentially after the input goes silent and
//! passes through the subnormal range on the way, where some hardware takes
//! 10–100× longer per operation. A 60 Hz Q 4 bell at `f64` enters that range
//! 30 seconds after silence and sits in it for 1.5 s; a 20 Hz Q 8 bell sits
//! in it for 9 seconds. Apple Silicon handles subnormals at full speed, but a
//! Cortex-A72 does not and x86 pays about 31×.
//!
//! The usual fix is to set the FTZ/DAZ bits, and in Rust that is a trap:
//! `_mm_setcsr` is deprecated, has no DAZ counterpart at all, and its own
//! documentation says modifying the denormals-are-zero flags is *immediate
//! undefined behaviour* because the optimiser assumes the default state — and
//! that this applies even if the register is restored afterwards. The
//! sanctioned escape is inline assembly. This module does not take it: it
//! flushes explicitly instead, once per control block per band, testing
//! `|x1| + |x2| + |y1| + |y2| < 1e-30` and zeroing all four if so. Four `abs`
//! and one compare per band per block, portable, no `unsafe`, correct on any
//! platform. The threshold sits far below any audible signal and far above
//! the `f64` subnormal range, so it never truncates anything real. A host
//! that has already set FTZ for the callback thread loses nothing by this.
//!
//! # What is deliberately not here
//!
//! Constant-Q only (no gain/Q interaction), linked stereo (no per-band M/S),
//! no auto-gain, no linear phase, no oversampling, and no spectrum analyzer.
//! The last one is a separate feature with its own FFT, ring buffer and tilt
//! convention. Oversampling is absent on purpose: the whole point of the
//! matched designs is that you do not need it.

use std::f64::consts::PI;

// ---------------------------------------------------------------------------
// Sizes and tuning constants
// ---------------------------------------------------------------------------

/// Bands in one EQ instance.
///
/// Four bells is the working minimum for corrective work and high-pass,
/// low-pass and two shelves are the standard frame around them. Eight is that
/// frame with every band free to be any type.
pub const BAND_COUNT: usize = 8;

/// Samples between coefficient recomputations.
///
/// The coefficients are linearly interpolated across the block, which is what
/// makes the block size nearly irrelevant to the sound; 32 is small enough
/// that it is indistinguishable from per-sample recomputation and large
/// enough that twenty EQ instances in a session cost 1/32 of the
/// transcendental work they otherwise would.
pub const CONTROL_BLOCK: usize = 32;

/// Biquad sections a single band can occupy: one, or two for 24 dB/oct.
const MAX_SECTIONS: usize = 2;

const INV_CONTROL_BLOCK: f64 = 1.0 / CONTROL_BLOCK as f64;

/// Time constant of the per-parameter one-pole smoother, in seconds.
///
/// Past about 15 ms the click energy stops improving — 13.5 dB at 10 ms
/// against 11.9 dB at 30 ms, for three times the lag. Fast enough that a
/// knob move feels immediate, slow enough that what is left is the legitimate
/// content of a filter sweep rather than a click.
const SMOOTH_TAU_S: f64 = 0.015;

/// Distance in normalised parameter units at which a smoother snaps to its
/// target instead of approaching it geometrically forever.
///
/// Two jobs. It stops the smoothers from generating their own denormals, and
/// it guarantees that a band parked at exactly 0 dB really does reach exactly
/// 0 dB, which is what lets the identity fast path in [`ParametricEq::process`]
/// engage. In normalised units 1e-9 is 3.6e-8 dB of gain or 8e-9 relative in
/// frequency.
const SMOOTH_SNAP: f64 = 1e-9;

/// Length in samples of the crossfade applied to discrete parameter changes.
///
/// Public because it is observable behaviour rather than a private tuning
/// number: it is how long a type, slope or enable change takes to complete.
///
/// Type, slope and enable cannot be smoothed — there is no path between
/// "bell" and "notch" — so the old and new filters run in parallel for
/// 1.5 ms and the output crossfades between them along a smoothstep curve.
pub const XFADE_LEN: u32 = 64;

/// Sum-of-state magnitude below which a section's state is flushed to zero.
const DENORMAL_FLOOR: f64 = 1e-30;

/// State magnitude above which a section is assumed to have diverged.
const DIVERGENCE_CEILING: f64 = 1e9;

// ---------------------------------------------------------------------------
// Parameter ranges and their 0..1 mapping laws
// ---------------------------------------------------------------------------

/// Lowest band frequency, in Hz.
pub const FREQ_MIN_HZ: f64 = 10.0;
/// Highest band frequency, in Hz.
///
/// The display reads 20 Hz – 20 kHz but the parameter goes past both ends:
/// shelves and high/low-pass genuinely want corners outside the audible band
/// (a 24 kHz high shelf is a real air move) and the matched two-pole shelf is
/// *designed* to work with its corner above Nyquist.
pub const FREQ_MAX_HZ: f64 = 30_000.0;
const FREQ_SPAN: f64 = FREQ_MAX_HZ / FREQ_MIN_HZ; // 3000

/// Band gain limit, in dB, either direction.
///
/// The convention survey runs API 550 at ±12, Neve 1073 at ±16/18, SSL at
/// ±15/20 and Pro-Q at ±30. ±18 covers every musical move; the case that
/// wants more is a deep surgical dip, which is what the notch type is for and
/// it has no depth limit. In a terminal the knob has finite travel, and ±18
/// at 0.1 dB steps reads cleanly at 360 steps where ±30 buys nothing but a
/// coarser knob.
pub const GAIN_MAX_DB: f64 = 18.0;

/// Lowest band Q. 0.1 is 6.67 octaves wide.
pub const Q_MIN: f64 = 0.1;
/// Highest band Q. 40 is 0.036 octaves wide.
pub const Q_MAX: f64 = 40.0;
const Q_SPAN: f64 = Q_MAX / Q_MIN; // 400

/// Output trim limit, in dB, either direction.
pub const TRIM_MAX_DB: f64 = 24.0;

/// Butterworth Q — the default for shelves, high-pass and low-pass.
pub const BUTTERWORTH_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// Section Qs for a 24 dB/oct (4-pole) Butterworth cascade.
///
/// The general rule is `Q_k = 1/(2·cos(π(2k+1)/(2·order)))`.
const BUTTERWORTH_Q4: [f64; 2] = [0.541_196_100_146_197, 1.306_562_964_876_377_3];

/// Band frequency for a normalised 0..1 parameter: `f = 10 · 3000^p`.
///
/// Exponential, so one step of the knob is a constant musical interval
/// anywhere on the dial. Anchors: 20 Hz is p = 0.08657, 1 kHz is p = 0.57519,
/// 20 kHz is p = 0.94936.
#[must_use]
pub fn freq_hz_from_norm(p: f64) -> f64 {
    FREQ_MIN_HZ * FREQ_SPAN.powf(p.clamp(0.0, 1.0))
}

/// Inverse of [`freq_hz_from_norm`].
#[must_use]
pub fn norm_from_freq_hz(hz: f64) -> f64 {
    (hz.clamp(FREQ_MIN_HZ, FREQ_MAX_HZ) / FREQ_MIN_HZ).ln() / FREQ_SPAN.ln()
}

/// Band gain in dB for a normalised 0..1 parameter: `G = (2p − 1)·18`.
///
/// Linear, and centred so that p = 0.5 is exactly 0 dB — bit-exactly, which
/// the identity fast path depends on — and one 1/360 step is exactly 0.1 dB.
#[must_use]
pub fn gain_db_from_norm(p: f64) -> f64 {
    (2.0 * p.clamp(0.0, 1.0) - 1.0) * GAIN_MAX_DB
}

/// Inverse of [`gain_db_from_norm`].
#[must_use]
pub fn norm_from_gain_db(db: f64) -> f64 {
    (db.clamp(-GAIN_MAX_DB, GAIN_MAX_DB) / GAIN_MAX_DB).mul_add(0.5, 0.5)
}

/// Band Q for a normalised 0..1 parameter: `Q = 0.1 · 400^p`.
///
/// Anchors: Q 0.707 is p = 0.32647, Q 2 is exactly p = 0.5.
#[must_use]
pub fn q_from_norm(p: f64) -> f64 {
    Q_MIN * Q_SPAN.powf(p.clamp(0.0, 1.0))
}

/// Inverse of [`q_from_norm`].
#[must_use]
pub fn norm_from_q(q: f64) -> f64 {
    (q.clamp(Q_MIN, Q_MAX) / Q_MIN).ln() / Q_SPAN.ln()
}

/// Output trim in dB for a normalised 0..1 parameter: `(2p − 1)·24`.
#[must_use]
pub fn trim_db_from_norm(p: f64) -> f64 {
    (2.0 * p.clamp(0.0, 1.0) - 1.0) * TRIM_MAX_DB
}

/// Inverse of [`trim_db_from_norm`].
#[must_use]
pub fn norm_from_trim_db(db: f64) -> f64 {
    (db.clamp(-TRIM_MAX_DB, TRIM_MAX_DB) / TRIM_MAX_DB).mul_add(0.5, 0.5)
}

/// Half-gain bandwidth in octaves for a Q.
///
/// The octave number is the one that means something to a person; the Q is
/// the one that appears in the formulas. Display both. Q 0.707 is 1.90
/// octaves, Q 1.414 is exactly 1.00, Q 40 is 0.036.
#[must_use]
pub fn q_to_octaves(q: f64) -> f64 {
    (2.0 / std::f64::consts::LN_2) * (1.0 / (2.0 * q)).asinh()
}

/// Inverse of [`q_to_octaves`].
#[must_use]
pub fn octaves_to_q(octaves: f64) -> f64 {
    1.0 / (2.0 * ((std::f64::consts::LN_2 / 2.0) * octaves).sinh())
}

/// ISO 1/6-octave nominal centre frequencies, 10 Hz to 30 kHz.
///
/// The R20 preferred-number series: its ratio is 10^(1/20) = 1.1220 against a
/// true sixth of an octave at 1.1225, and these are the numbers a person
/// expects to read. Frequency knobs walk this grid so the readout is always
/// "2.5k" and never "2487".
pub const ISO_SIXTH_OCTAVE_HZ: [f64; 71] = [
    10.0, 11.2, 12.5, 14.0, 16.0, 18.0, 20.0, 22.4, 25.0, 28.0, 31.5, 35.5, 40.0, 45.0, 50.0, 56.0,
    63.0, 71.0, 80.0, 90.0, 100.0, 112.0, 125.0, 140.0, 160.0, 180.0, 200.0, 224.0, 250.0, 280.0,
    315.0, 355.0, 400.0, 450.0, 500.0, 560.0, 630.0, 710.0, 800.0, 900.0, 1000.0, 1120.0, 1250.0,
    1400.0, 1600.0, 1800.0, 2000.0, 2240.0, 2500.0, 2800.0, 3150.0, 3550.0, 4000.0, 4500.0, 5000.0,
    5600.0, 6300.0, 7100.0, 8000.0, 9000.0, 10000.0, 11200.0, 12500.0, 14000.0, 16000.0, 18000.0,
    20000.0, 22400.0, 25000.0, 28000.0, 30000.0,
];

/// How far off a grid point a value can be and still count as sitting on it.
///
/// A part per million, and it is not arbitrary: every one of these knobs
/// stores its value as an `f32`, which carries about six parts in a hundred
/// million, and two of the centres — 11.2 and 22.4 Hz — are not exactly
/// representable in one. Round 22.4 through an `f32` and it comes back a
/// hair *below* the table's own entry, so a "strictly greater" test with a
/// tolerance of `1e-9` finds 22.4 again and the knob sticks there forever.
/// The grid points are twelve percent apart, so a millionth cannot reach the
/// next one.
const ISO_ON_GRID: f64 = 1.0e-6;

/// The next [`ISO_SIXTH_OCTAVE_HZ`] centre strictly above `hz`.
///
/// Saturates at the top of the range. Starting off-grid — the factory
/// defaults are round numbers like 120 Hz rather than grid points — steps
/// onto the grid rather than snapping first, so one keypress never moves the
/// frequency backwards.
#[must_use]
pub fn iso_step_up(hz: f64) -> f64 {
    for &f in &ISO_SIXTH_OCTAVE_HZ {
        if f > hz * (1.0 + ISO_ON_GRID) {
            return f;
        }
    }
    FREQ_MAX_HZ
}

/// The next [`ISO_SIXTH_OCTAVE_HZ`] centre strictly below `hz`.
#[must_use]
pub fn iso_step_down(hz: f64) -> f64 {
    for &f in ISO_SIXTH_OCTAVE_HZ.iter().rev() {
        if f < hz * (1.0 - ISO_ON_GRID) {
            return f;
        }
    }
    FREQ_MIN_HZ
}

/// The [`ISO_SIXTH_OCTAVE_HZ`] centre nearest `hz`, measured in log distance.
#[must_use]
pub fn iso_snap(hz: f64) -> f64 {
    let target = hz.clamp(FREQ_MIN_HZ, FREQ_MAX_HZ).ln();
    let mut best = ISO_SIXTH_OCTAVE_HZ[0];
    let mut best_d = f64::INFINITY;
    for &f in &ISO_SIXTH_OCTAVE_HZ {
        let d = (f.ln() - target).abs();
        if d < best_d {
            best_d = d;
            best = f;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// Band type and slope
// ---------------------------------------------------------------------------

/// What a band does.
///
/// The discriminants are the session's on-disk encoding and the order the
/// normalised `type` parameter walks, so they are stable: append only.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[repr(u8)]
pub enum BandType {
    /// Peaking bell. Matched design, cuts by reciprocal of the boost.
    #[default]
    Bell = 0,
    /// Low shelf, matched. 12 dB/oct Butterworth or 6 dB/oct one-pole.
    LowShelf = 1,
    /// High shelf, matched. 12 dB/oct Butterworth or 6 dB/oct one-pole.
    HighShelf = 2,
    /// High-pass, matched. 12 or 24 dB/oct.
    HighPass = 3,
    /// Low-pass, matched. 12 or 24 dB/oct.
    LowPass = 4,
    /// Notch. RBJ — the only cookbook filter in here, and §"Why the
    /// coefficients are not the cookbook's" says why.
    Notch = 5,
    /// Band-pass, matched, unity gain at the centre.
    BandPass = 6,
    /// All-pass, RBJ. Unity magnitude by construction.
    AllPass = 7,
}

impl BandType {
    /// Every type, in parameter order.
    pub const ALL: [Self; 8] = [
        Self::Bell,
        Self::LowShelf,
        Self::HighShelf,
        Self::HighPass,
        Self::LowPass,
        Self::Notch,
        Self::BandPass,
        Self::AllPass,
    ];

    /// The type at `index`, clamped into range.
    #[must_use]
    pub fn from_index(index: usize) -> Self {
        Self::ALL[index.min(Self::ALL.len() - 1)]
    }

    /// Position in [`BandType::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Whether the band's gain control does anything.
    ///
    /// Greyed out for high-pass, low-pass, notch, band-pass and all-pass.
    #[must_use]
    pub const fn uses_gain(self) -> bool {
        matches!(self, Self::Bell | Self::LowShelf | Self::HighShelf)
    }

    /// Whether the band's Q control does anything.
    ///
    /// The matched shelves are Butterworth by construction at 12 dB/oct and
    /// have no resonance at all at 6 dB/oct, so Q is inert for both — the
    /// same rule Pro-Q applies to its first-order shelves, extended one slope
    /// further because a resonant shelf has no published matched design and
    /// is not worth cramping one band type to get.
    #[must_use]
    pub const fn uses_q(self) -> bool {
        !matches!(self, Self::LowShelf | Self::HighShelf)
    }

    /// Whether the band offers a slope choice, and which one.
    #[must_use]
    pub const fn uses_slope(self) -> bool {
        matches!(
            self,
            Self::LowShelf | Self::HighShelf | Self::HighPass | Self::LowPass
        )
    }

    /// Short label for a terminal cell.
    #[must_use]
    pub const fn short_name(self) -> &'static str {
        match self {
            Self::Bell => "bell",
            Self::LowShelf => "loshf",
            Self::HighShelf => "hishf",
            Self::HighPass => "hpf",
            Self::LowPass => "lpf",
            Self::Notch => "notch",
            Self::BandPass => "bpf",
            Self::AllPass => "apf",
        }
    }
}

/// Filter slope, where the band type offers a choice.
///
/// Shelves take [`Slope::Db6`] or [`Slope::Db12`]; high-pass and low-pass take
/// [`Slope::Db12`] or [`Slope::Db24`]. Anything else ignores it. There is no
/// steeper shelf because cascading identical matched shelves barely changes
/// the slope — one octave below a 10 kHz +12 dB shelf the response is
/// +0.94 dB at one section and +0.77 dB at three — and the construction that
/// *would* work is a higher-order Butterworth shelf, which is a different
/// design problem.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[repr(u8)]
pub enum Slope {
    /// 6 dB/oct, one pole. Shelves only.
    Db6 = 0,
    /// 12 dB/oct, one biquad. The default everywhere.
    #[default]
    Db12 = 1,
    /// 24 dB/oct, two cascaded biquads. High-pass and low-pass only.
    Db24 = 2,
}

impl Slope {
    /// Decibels per octave.
    #[must_use]
    pub const fn db_per_octave(self) -> u8 {
        match self {
            Self::Db6 => 6,
            Self::Db12 => 12,
            Self::Db24 => 24,
        }
    }

    /// The slopes a given band type offers, in order. Empty if it offers none.
    #[must_use]
    pub const fn choices_for(ty: BandType) -> &'static [Self] {
        match ty {
            BandType::LowShelf | BandType::HighShelf => &[Self::Db6, Self::Db12],
            BandType::HighPass | BandType::LowPass => &[Self::Db12, Self::Db24],
            _ => &[],
        }
    }

    /// The slope `ty` actually gets when asked for `self`, which is `self` if
    /// it is one of the type's choices and the type's default otherwise.
    #[must_use]
    fn resolve(self, ty: BandType) -> Self {
        let choices = Self::choices_for(ty);
        if choices.is_empty() || choices.contains(&self) {
            self
        } else {
            Self::Db12
        }
    }
}

// ---------------------------------------------------------------------------
// The biquad and its state
// ---------------------------------------------------------------------------

/// One biquad section, `a0` already normalised to 1.
///
/// `H(z) = (b0 + b1 z⁻¹ + b2 z⁻²) / (1 + a1 z⁻¹ + a2 z⁻²)`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    /// `H(z) ≡ 1`. A wire.
    const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// The section that is a wire *with these poles*.
    ///
    /// `b = a` gives `H ≡ 1` exactly. Used where a design degenerates at unity
    /// gain: keeping the poles rather than collapsing to [`Biquad::IDENTITY`]
    /// means the coefficients stay continuous as the gain crosses zero, which
    /// matters because they are interpolated across it.
    const fn wire_with_poles(a1: f64, a2: f64) -> Self {
        Self {
            b0: 1.0,
            b1: a1,
            b2: a2,
            a1,
            a2,
        }
    }

    /// Whether this section is exactly a wire, bit for bit.
    ///
    /// Exact comparison is the point: the designs force `b0 = 1, b1 = a1,
    /// b2 = a2` at unity gain rather than arriving there through a `sqrt`
    /// chain that lands one ulp away, so this predicate is reliable and the
    /// caller can skip the arithmetic entirely and copy instead.
    fn is_wire(&self) -> bool {
        self.b0 == 1.0 && self.b1 == self.a1 && self.b2 == self.a2
    }

    /// Linear interpolation of all five coefficients, `t` in 0..1.
    ///
    /// All five, not just the denominator: it is changes in the coefficients
    /// that set the *zeros* that produce audible artifacts, while the
    /// transient from moving the poles is perceptually pleasant. A "smooth
    /// only `a1`/`a2`" optimisation would be exactly backwards.
    #[inline]
    fn lerp(&self, to: &Self, t: f64) -> Self {
        Self {
            b0: t.mul_add(to.b0 - self.b0, self.b0),
            b1: t.mul_add(to.b1 - self.b1, self.b1),
            b2: t.mul_add(to.b2 - self.b2, self.b2),
            a1: t.mul_add(to.a1 - self.a1, self.a1),
            a2: t.mul_add(to.a2 - self.a2, self.a2),
        }
    }

    /// `|H(ω)|²` by Vicanek's φ-form, which needs one `sin` and no complex
    /// arithmetic and cancels well.
    ///
    /// Returns a negative-or-zero numerator as `None`, which is a notch
    /// sitting on its own zero.
    fn mag_squared(&self, phi0: f64, phi1: f64, phi2: f64) -> Option<f64> {
        let a0 = (1.0 + self.a1 + self.a2).powi(2);
        let a1t = (1.0 - self.a1 + self.a2).powi(2);
        let a2t = -4.0 * self.a2;
        let b0 = (self.b0 + self.b1 + self.b2).powi(2);
        let b1t = (self.b0 - self.b1 + self.b2).powi(2);
        let b2t = -4.0 * self.b0 * self.b2;
        let num = b2t.mul_add(phi2, b0.mul_add(phi0, b1t * phi1));
        let den = a2t.mul_add(phi2, a0.mul_add(phi0, a1t * phi1));
        if num <= 0.0 || den <= 0.0 {
            None
        } else {
            Some(num / den)
        }
    }
}

/// Direct Form I state: two past inputs and two past outputs.
///
/// Signal history, with no dependence on the coefficients — which is the
/// whole reason for choosing this topology.
#[derive(Clone, Copy, Debug, Default)]
struct BiquadState {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl BiquadState {
    #[inline]
    fn tick(&mut self, c: &Biquad, x: f64) -> f64 {
        let y = c.b2.mul_add(
            self.x2,
            c.b0.mul_add(x, c.b1 * self.x1) - c.a1.mul_add(self.y1, c.a2 * self.y2),
        );
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    /// Advance the state as if this section were a wire fed `buf`.
    ///
    /// The identity fast path skips the arithmetic but must not skip the
    /// history, or the filter would start from stale state the moment the
    /// user nudges the gain off zero.
    fn absorb(&mut self, buf: &[f64]) {
        match buf.len() {
            0 => {}
            1 => {
                self.x2 = self.x1;
                self.x1 = buf[0];
            }
            n => {
                self.x2 = buf[n - 2];
                self.x1 = buf[n - 1];
            }
        }
        self.y1 = self.x1;
        self.y2 = self.x2;
    }

    fn clear(&mut self) {
        *self = Self::default();
    }

    /// Zero the state if it has decayed into the subnormal danger zone, or if
    /// it has diverged. One call per section per control block.
    fn guard(&mut self) {
        let magnitude = self.x1.abs() + self.x2.abs() + self.y1.abs() + self.y2.abs();
        if magnitude < DENORMAL_FLOOR || !magnitude.is_finite() || magnitude > DIVERGENCE_CEILING {
            self.clear();
        }
    }
}

/// The one to four biquads a band compiles down to.
#[derive(Clone, Copy, Debug)]
struct Sections {
    c: [Biquad; MAX_SECTIONS],
    n: usize,
}

impl Sections {
    const WIRE: Self = Self {
        c: [Biquad::IDENTITY; MAX_SECTIONS],
        n: 1,
    };

    fn one(c: Biquad) -> Self {
        Self {
            c: [c, Biquad::IDENTITY],
            n: 1,
        }
    }

    fn two(a: Biquad, b: Biquad) -> Self {
        Self { c: [a, b], n: 2 }
    }

    fn is_wire(&self) -> bool {
        self.c[..self.n].iter().all(Biquad::is_wire)
    }

    fn same_as(&self, other: &Self) -> bool {
        self.n == other.n && self.c[..self.n] == other.c[..other.n]
    }
}

// ---------------------------------------------------------------------------
// Coefficient design
// ---------------------------------------------------------------------------

/// Impulse-invariant pole mapping: `z = e^s` applied to `s² + 2qω₀s + ω₀²`.
///
/// This is the step that does not cramp. The bilinear transform maps
/// `s = ∞` to `z = −1` and squeezes everything above `f₀` into the interval
/// up to Nyquist; `z = e^s` places the poles at their actual frequency and
/// leaves the resonance width alone. `q > 1` — which a deep cut reaches,
/// since the bell's pole damping is `1/(2Q√G)` — moves the pole pair onto the
/// real axis and takes the `cosh` branch.
fn poles_impulse_invariant(w0: f64, q: f64) -> (f64, f64) {
    let decay = (-q * w0).exp();
    let a1 = if q <= 1.0 {
        -2.0 * decay * ((1.0 - q * q).max(0.0).sqrt() * w0).cos()
    } else {
        -2.0 * decay * ((q * q - 1.0).max(0.0).sqrt() * w0).cosh()
    };
    (a1, (-2.0 * q * w0).exp())
}

/// `φ0, φ1, φ2` at an angular frequency, the frequency-dependent half of the
/// φ-form.
fn phis(w: f64) -> (f64, f64, f64) {
    let s = (w * 0.5).sin();
    let phi1 = s * s;
    let phi0 = 1.0 - phi1;
    (phi0, phi1, 4.0 * phi0 * phi1)
}

/// `A0, A1, A2`, the denominator half of the φ-form.
fn denom_terms(a1: f64, a2: f64) -> (f64, f64, f64) {
    (
        (1.0 + a1 + a2).powi(2),
        (1.0 - a1 + a2).powi(2),
        -4.0 * a2,
    )
}

/// Recover a minimum-phase numerator from `B0, B1, B2`.
///
/// Vicanek imposes `b0 > |b2|` and `b0 + b2 > |b1|` on the numerator, which
/// keeps the zeros inside the unit circle. That is what makes the reciprocal
/// trick in [`design_bell`] legal: the reciprocal filter's poles are this
/// filter's zeros.
///
/// Every `sqrt` argument is clamped at zero. Where the clamp bites — very low
/// `f₀`, where `A0·φ0 + A1·φ1 + A2·φ2` is `O(ω₀⁴)` assembled from terms of
/// `O(ω₀²)` — the true value really is zero to within rounding, so the clamp
/// is exact rather than a fudge.
fn numerator_from_b(b0t: f64, b1t: f64, b2t: f64) -> (f64, f64, f64) {
    let r0 = b0t.max(0.0).sqrt();
    let r1 = b1t.max(0.0).sqrt();
    let w = 0.5 * (r0 + r1);
    let b0 = 0.5 * (w + w.mul_add(w, b2t).max(0.0).sqrt());
    let b1 = 0.5 * (r0 - r1);
    let b2 = if b0 == 0.0 { 0.0 } else { -b2t / (4.0 * b0) };
    (b0, b1, b2)
}

/// Matched peaking bell, Vicanek §4.4, boost only.
///
/// The analog prototype is
/// `H(s) = (ω₀² + sω₀√G/Q + s²)/(ω₀² + sω₀/(√G·Q) + s²)`, whose pole damping
/// is `q = 1/(2Q√G)` — note the `√G`: the prototype's pole Q is `√G·Q`, not
/// `Q`.
///
/// `B2` uses the algebraically simplified form. The paper computes
/// `B2 = (R1 − R2·φ1 − B0)/(4φ1²)`, and `R1 − R2·φ1` expands — using
/// `φ0 + φ1 = 1` and `φ2 = 4φ0φ1` — to exactly `G²(A0 + 4A2φ1²)`, so
/// `B2 = A0(G² − 1)/(4φ1²) + G²A2` with no subtraction of nearly-equal
/// quantities. The two agree to 1.6e-15 relative at 16 kHz / 44.1 kHz and
/// diverge to 1.1e-7 at 10 Hz / 192 kHz, where this form is the accurate one.
fn design_bell_boost(w0: f64, gain: f64, q_user: f64) -> Biquad {
    let (a1, a2) = poles_impulse_invariant(w0, 1.0 / (2.0 * q_user * gain.sqrt()));
    if gain == 1.0 {
        return Biquad::wire_with_poles(a1, a2);
    }
    let (t0, t1, t2) = denom_terms(a1, a2);
    let (phi0, phi1, _) = phis(w0);
    let g2 = gain * gain;

    let b0t = t0;
    let r2 = (4.0 * (phi0 - phi1)).mul_add(t2, t1 - t0) * g2;
    let b2t = (t0 * (g2 - 1.0)).mul_add(1.0 / (4.0 * phi1 * phi1), g2 * t2);
    let b1t = (4.0 * (phi1 - phi0)).mul_add(b2t, r2 + b0t);

    let (b0, b1, b2) = numerator_from_b(b0t, b1t, b2t);
    Biquad { b0, b1, b2, a1, a2 }
}

/// Matched peaking bell with reciprocal cuts.
///
/// A cut is not designed; it is the boost filter inverted. The analog
/// prototype satisfies `H_{1/G}(s) = 1/H_G(s)` exactly, so the correct cut
/// literally *is* the reciprocal of the correct boost, and Vicanek's
/// minimum-phase numerator makes the inversion stable.
///
/// It wins on both axes at once. Against the analog target at 16 kHz Q 2 the
/// error is 6.60 dB for RBJ, 1.79 dB for a directly designed matched cut, and
/// 0.668 dB for the reciprocal — identical to the boost error, because the dB
/// curve is exactly negated. And boosting then cutting by the same amount
/// leaves a residual of 6.2e-15 dB, against 1.126 dB for the directly
/// designed cut. Users notice that; RBJ engineered its Q definition
/// specifically to get it, and this gets it without giving up accuracy.
fn design_bell(w0: f64, gain_db: f64, q_user: f64) -> Biquad {
    if gain_db >= 0.0 {
        return design_bell_boost(w0, db_to_linear(gain_db), q_user);
    }
    let b = design_bell_boost(w0, db_to_linear(-gain_db), q_user);
    Biquad {
        b0: 1.0 / b.b0,
        b1: b.a1 / b.b0,
        b2: b.a2 / b.b0,
        a1: b.b1 / b.b0,
        a2: b.b2 / b.b0,
    }
}

/// Matched low-pass, Vicanek §4.1. `b2 = 0` by construction.
fn design_lowpass(w0: f64, q_user: f64) -> Biquad {
    let (a1, a2) = poles_impulse_invariant(w0, 1.0 / (2.0 * q_user));
    let (t0, t1, t2) = denom_terms(a1, a2);
    let (phi0, phi1, phi2) = phis(w0);
    let r1 = t2.mul_add(phi2, t0.mul_add(phi0, t1 * phi1)) * q_user * q_user;
    let b0t = t0;
    let b1t = ((r1 - b0t * phi0) / phi1).max(0.0);
    let r0 = b0t.max(0.0).sqrt();
    let b0 = 0.5 * (r0 + b1t.sqrt());
    Biquad {
        b0,
        b1: r0 - b0,
        b2: 0.0,
        a1,
        a2,
    }
}

/// Matched high-pass, Vicanek §4.2. Double zero at DC.
fn design_highpass(w0: f64, q_user: f64) -> Biquad {
    let (a1, a2) = poles_impulse_invariant(w0, 1.0 / (2.0 * q_user));
    let (t0, t1, t2) = denom_terms(a1, a2);
    let (phi0, phi1, phi2) = phis(w0);
    let b0 = q_user * t2.mul_add(phi2, t0.mul_add(phi0, t1 * phi1)).max(0.0).sqrt() / (4.0 * phi1);
    Biquad {
        b0,
        b1: -2.0 * b0,
        b2: b0,
        a1,
        a2,
    }
}

/// Matched band-pass, Vicanek §4.3. Single zero at DC, unity gain at `f₀`.
///
/// `B2` uses the same cancellation-free identity as the bell:
/// `R1 − R2·φ1 = A0 + 4A2φ1²`.
fn design_bandpass(w0: f64, q_user: f64) -> Biquad {
    let (a1, a2) = poles_impulse_invariant(w0, 1.0 / (2.0 * q_user));
    let (t0, t1, t2) = denom_terms(a1, a2);
    let (phi0, phi1, _) = phis(w0);
    let b2t = t0.mul_add(1.0 / (4.0 * phi1 * phi1), t2);
    let r2 = (4.0 * (phi0 - phi1)).mul_add(t2, t1 - t0);
    let b1t = (4.0 * (phi1 - phi0)).mul_add(b2t, r2);
    let b1 = -0.5 * b1t.max(0.0).sqrt();
    let b0 = 0.5 * (b1.mul_add(b1, b2t).max(0.0).sqrt() - b1);
    Biquad {
        b0,
        b1,
        b2: -b0 - b1,
        a1,
        a2,
    }
}

/// Matched one-pole shelf, Vicanek 2019. 6 dB/oct.
///
/// `fc` is in units of Nyquist. The matching point is `f_m = 0.9` of Nyquist.
/// Returned as a biquad with `b2 = a2 = 0`.
///
/// The paper's eq. (11) prints `b0 = (1 + a)/(1 + b)`, where `a` means the
/// coefficient `a1` rather than the intermediate `α`.
fn design_shelf_1pole(fc: f64, gain: f64, high: bool) -> Biquad {
    // A high shelf of gain g; a low shelf is the high shelf of 1/g scaled by g.
    // At gain 1 the two intermediates below are bitwise equal, so `b0` is
    // exactly 1 and `b1` exactly `a1`: the natural result is already a wire
    // with the right pole and needs no special case.
    let g = if high { gain } else { 1.0 / gain };
    const FM: f64 = 0.9;
    let phi_m = 1.0 - (PI * FM).cos();
    let k = 2.0 / (PI * PI);
    let alpha = k.mul_add(1.0 / (FM * FM) + 1.0 / (g * fc * fc), -(1.0 / phi_m));
    let beta = k.mul_add(1.0 / (FM * FM) + g / (fc * fc), -(1.0 / phi_m));
    let a1 = -alpha / (1.0 + alpha + 2.0f64.mul_add(alpha, 1.0).max(0.0).sqrt());
    let bb = -beta / (1.0 + beta + 2.0f64.mul_add(beta, 1.0).max(0.0).sqrt());
    let mut b0 = (1.0 + a1) / (1.0 + bb);
    let mut b1 = bb * b0;
    if !high {
        b0 *= gain;
        b1 *= gain;
    }
    Biquad {
        b0,
        b1,
        b2: 0.0,
        a1,
        a2: 0.0,
    }
}

/// Matched two-pole Butterworth shelf, Vicanek 2024 (rev. Dec 2025).
///
/// The default shelf. It matches the analog second-order Butterworth shelf
/// `H(s) = (1 + √2·g·s + g²s²)/(1 + √2·s/g + s²/g²)` with `g = G^{1/4}` by
/// imposing unity and maximal flatness at DC, the exact gain at Nyquist, and
/// equality at two frequencies chosen to stay below Nyquist even when the
/// corner is above it. `fc` is in units of Nyquist, so a corner above Nyquist
/// is legal and useful.
///
/// The payoff is shape, not endpoints. A +12 dB high shelf at 16 kHz first
/// reaches +1 dB at 8 252 Hz in the analog prototype and at 8 148 Hz here;
/// RBJ's does not get there until 11 827 Hz — half an octave late, then it
/// has to climb steeply to make up the gain. That is what a cramped shelf
/// sounds like.
///
/// The unity-gain guard is not optional: at `gain = 1` the paper's linear
/// system is singular, because `hny − h1` and `hny − h2` are both zero. The
/// paper substitutes 1.00001. This function goes further and returns a true
/// wire *with the guarded design's poles*, so the response is exactly flat
/// rather than 8.7e-6 dB of shelf, and the coefficients stay continuous
/// across zero for the interpolator.
fn design_shelf_2pole(fc: f64, gain: f64, high: bool) -> Biquad {
    let flat = (1.0 - gain).abs() < 1e-6;
    let requested = if high { gain } else { 1.0 / gain };
    let g = if flat { 1.00001 } else { requested };
    let invg = 1.0 / g;

    let fc2 = fc * fc;
    let fc4 = fc2 * fc2;
    let hny = (fc4 + g) / (fc4 + invg);

    let f1 = fc / 1.543f64.mul_add(fc2, 0.160).sqrt();
    let f14 = f1 * f1 * f1 * f1;
    let h1 = f14.mul_add(g, fc4) / f14.mul_add(invg, fc4);
    let phi1 = (PI * 0.5 * f1).sin().powi(2);

    let f2 = fc / 3.806f64.mul_add(fc2, 0.947).sqrt();
    let f24 = f2 * f2 * f2 * f2;
    let h2 = f24.mul_add(g, fc4) / f24.mul_add(invg, fc4);
    let phi2 = (PI * 0.5 * f2).sin().powi(2);

    let d1 = (h1 - 1.0) * (1.0 - phi1);
    let c11 = -phi1 * d1;
    let c12 = phi1 * phi1 * (hny - h1);
    let d2 = (h2 - 1.0) * (1.0 - phi2);
    let c21 = -phi2 * d2;
    let c22 = phi2 * phi2 * (hny - h2);

    let alpha1 = c12.mul_add(-d2, c22 * d1) / c12.mul_add(-c21, c11 * c22);
    let aa1 = c11.mul_add(-alpha1, d1) / c12;
    let bb1 = hny * aa1;
    let aa2 = 0.25 * (alpha1 - aa1);
    let bb2 = 0.25 * (alpha1 - bb1);

    let v = 0.5 * (1.0 + aa1.max(0.0).sqrt());
    let w = 0.5 * (1.0 + bb1.max(0.0).sqrt());
    let a0 = 0.5 * (v + v.mul_add(v, aa2).max(0.0).sqrt());
    let inv_a0 = 1.0 / a0;
    let a1 = (1.0 - v) * inv_a0;
    let a2 = -0.25 * aa2 * inv_a0 * inv_a0;

    if flat {
        return Biquad::wire_with_poles(a1, a2);
    }

    if high {
        let b0 = 0.5 * (w + w.mul_add(w, bb2).max(0.0).sqrt()) * inv_a0;
        Biquad {
            b0,
            b1: (1.0 - w) * inv_a0,
            b2: (-0.25 * bb2 / b0) * inv_a0 * inv_a0,
            a1,
            a2,
        }
    } else {
        let g_inv_a0 = invg * inv_a0;
        let b0u = 0.5 * (w + w.mul_add(w, bb2).max(0.0).sqrt());
        Biquad {
            b0: b0u * g_inv_a0,
            b1: (1.0 - w) * g_inv_a0,
            b2: (-0.25 * bb2 / b0u) * g_inv_a0,
            a1,
            a2,
        }
    }
}

/// RBJ notch. The one cookbook filter in this module.
///
/// A notch is defined by an exact zero on the unit circle at `ω₀` — infinite
/// attenuation — and no matched design provides one. The identity
/// `H_notch = 1 − H_bandpass` applied to the matched bandpass does not
/// recover it: the depth collapses to −49 dB at 60 Hz and −7.6 dB at 8 kHz,
/// because the matched bandpass's deep skirts are not accurate to one part in
/// 10⁴ and subtracting from 1 amplifies exactly that error. Recorded here so
/// nobody tries it again.
///
/// The limitation to document in the UI: the null is exact at every
/// frequency, but the notch *width* cramps at high `f₀` — 8.4 dB of shape
/// error at 15 kHz. That is acceptable because a notch is for hum, resonance
/// and feedback, where depth at a known frequency is the entire job. A user
/// who wants a correctly shaped deep dip should use a bell at −18 dB.
fn design_notch(w0: f64, q_user: f64) -> Biquad {
    let alpha = w0.sin() / (2.0 * q_user);
    let cos_w0 = w0.cos();
    let a0 = 1.0 + alpha;
    Biquad {
        b0: 1.0 / a0,
        b1: -2.0 * cos_w0 / a0,
        b2: 1.0 / a0,
        a1: -2.0 * cos_w0 / a0,
        a2: (1.0 - alpha) / a0,
    }
}

/// RBJ all-pass. Unity magnitude at every frequency by construction, so there
/// is nothing for a matched design to improve — cramping here would only be a
/// phase question and nobody is checking.
fn design_allpass(w0: f64, q_user: f64) -> Biquad {
    let alpha = w0.sin() / (2.0 * q_user);
    let cos_w0 = w0.cos();
    let a0 = 1.0 + alpha;
    Biquad {
        b0: (1.0 - alpha) / a0,
        b1: -2.0 * cos_w0 / a0,
        b2: 1.0,
        a1: -2.0 * cos_w0 / a0,
        a2: (1.0 - alpha) / a0,
    }
}

#[inline]
fn db_to_linear(db: f64) -> f64 {
    if db == 0.0 {
        1.0
    } else {
        10.0f64.powf(db / 20.0)
    }
}

/// Compile one band's settings into the sections that realise it.
///
/// `freq_hz` is clamped to 0.49·f_s for everything except the shelves. The
/// shelves genuinely work with a corner above Nyquist — the two-pole matched
/// shelf is designed for it — but the impulse-invariant pole map aliases
/// above Nyquist, so a bell, notch, band-pass, all-pass, high-pass or
/// low-pass asked for 30 kHz at 44.1 kHz gets 21.6 kHz instead.
fn design(
    ty: BandType,
    freq_hz: f64,
    gain_db: f64,
    q_user: f64,
    slope: Slope,
    sample_rate: f64,
) -> Sections {
    let nyquist = 0.5 * sample_rate;
    let clamped = freq_hz.clamp(FREQ_MIN_HZ, (0.49 * sample_rate).max(FREQ_MIN_HZ));
    let w0 = 2.0 * PI * clamped / sample_rate;
    let q = q_user.clamp(Q_MIN, Q_MAX);

    match ty {
        BandType::Bell => Sections::one(design_bell(w0, gain_db, q)),
        BandType::LowShelf | BandType::HighShelf => {
            let high = ty == BandType::HighShelf;
            // Shelf corners are expressed in units of Nyquist and are not
            // clamped: a 24 kHz shelf at 44.1 kHz is a real air move.
            let fc = freq_hz.max(FREQ_MIN_HZ) / nyquist;
            let gain = db_to_linear(gain_db);
            Sections::one(if slope == Slope::Db6 {
                design_shelf_1pole(fc, gain, high)
            } else {
                design_shelf_2pole(fc, gain, high)
            })
        }
        BandType::HighPass | BandType::LowPass => {
            let make = |q: f64| {
                if ty == BandType::HighPass {
                    design_highpass(w0, q)
                } else {
                    design_lowpass(w0, q)
                }
            };
            if slope == Slope::Db24 {
                // The user's Q scales the Butterworth pair rather than being
                // ignored, so the control still does something at 24 dB/oct;
                // at the default Q the sections are exactly Butterworth.
                let scale = q / BUTTERWORTH_Q;
                Sections::two(
                    make((BUTTERWORTH_Q4[0] * scale).clamp(Q_MIN, Q_MAX)),
                    make((BUTTERWORTH_Q4[1] * scale).clamp(Q_MIN, Q_MAX)),
                )
            } else {
                Sections::one(make(q))
            }
        }
        BandType::Notch => Sections::one(design_notch(w0, q)),
        BandType::BandPass => Sections::one(design_bandpass(w0, q)),
        BandType::AllPass => Sections::one(design_allpass(w0, q)),
    }
}

// ---------------------------------------------------------------------------
// Parameter smoothing
// ---------------------------------------------------------------------------

/// One-pole smoother for a normalised 0..1 parameter, advanced once per
/// control block.
///
/// Smoothing happens in normalised space, which means frequency and Q are
/// smoothed in the log domain — a sweep from 500 Hz to 5 kHz covers each
/// octave at the same rate — and gain is smoothed in dB. It also means there
/// is exactly one place where a parameter's law is applied, after the
/// smoother rather than before it.
#[derive(Clone, Copy, Debug)]
struct Smoother {
    target: f64,
    value: f64,
}

impl Smoother {
    const fn new(v: f64) -> Self {
        Self {
            target: v,
            value: v,
        }
    }

    fn set(&mut self, t: f64) {
        self.target = t.clamp(0.0, 1.0);
    }

    /// Jump to the target without smoothing. For construction, sample-rate
    /// changes and `reset` — never for a knob move.
    fn snap(&mut self) {
        self.value = self.target;
    }

    fn settled(&self) -> bool {
        self.value == self.target
    }

    /// One control block of decay toward the target. `alpha` is the
    /// block-length pole, `exp(−N/(τ·f_s))`.
    fn advance(&mut self, alpha: f64) -> f64 {
        let delta = self.target - self.value;
        if delta.abs() < SMOOTH_SNAP {
            self.value = self.target;
        } else {
            self.value = delta.mul_add(-alpha, self.target);
        }
        self.value
    }
}

// ---------------------------------------------------------------------------
// A band
// ---------------------------------------------------------------------------

/// What the output of a band is being crossfaded between, if anything.
///
/// Type, slope and enable are discrete: there is no path between "bell" and
/// "notch" to smooth along, so the old and new filters run in parallel for
/// [`XFADE_LEN`] samples and the output crosses over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fade {
    None,
    /// Band switching on: dry to wet.
    FromDry,
    /// Band switching off: wet to dry, then the band goes inactive and its
    /// state is zeroed, which is also what kills its denormals.
    ToDry,
    /// Type or slope changed: old filter to new filter.
    Swap,
}

#[derive(Clone, Copy, Debug)]
struct Band {
    ty: BandType,
    slope: Slope,
    enabled: bool,
    freq: Smoother,
    gain: Smoother,
    q: Smoother,

    /// Coefficients in force at the end of the current control block.
    cur: Sections,
    /// Coefficients in force at the end of the previous block; the block
    /// interpolates from these to `cur`.
    prev: Sections,
    /// True when `prev` and `cur` differ, i.e. the block must interpolate.
    ramping: bool,
    /// Set when a parameter has moved and cleared once the smoothers have
    /// settled and one final design has been computed.
    needs_design: bool,

    state: [[BiquadState; MAX_SECTIONS]; 2],

    /// Whether the band is contributing to the output at all. Lags `enabled`
    /// by the length of a crossfade.
    active: bool,
    fade: Fade,
    fade_pos: u32,
    old: Sections,
    old_state: [[BiquadState; MAX_SECTIONS]; 2],
}

impl Band {
    fn new(ty: BandType, freq_hz: f64, q: f64, enabled: bool, sample_rate: f64) -> Self {
        let mut band = Self {
            ty,
            slope: Slope::Db12,
            enabled,
            freq: Smoother::new(norm_from_freq_hz(freq_hz)),
            gain: Smoother::new(norm_from_gain_db(0.0)),
            q: Smoother::new(norm_from_q(q)),
            cur: Sections::WIRE,
            prev: Sections::WIRE,
            ramping: false,
            needs_design: false,
            state: [[BiquadState::default(); MAX_SECTIONS]; 2],
            active: enabled,
            fade: Fade::None,
            fade_pos: 0,
            old: Sections::WIRE,
            old_state: [[BiquadState::default(); MAX_SECTIONS]; 2],
        };
        band.redesign_now(sample_rate);
        band
    }

    fn freq_hz(&self) -> f64 {
        freq_hz_from_norm(self.freq.target)
    }
    fn gain_db(&self) -> f64 {
        gain_db_from_norm(self.gain.target)
    }
    fn q_value(&self) -> f64 {
        q_from_norm(self.q.target)
    }

    /// Snap the smoothers to their targets and compute the coefficients they
    /// imply, with no ramp. Construction, `reset` and sample-rate changes.
    fn redesign_now(&mut self, sample_rate: f64) {
        self.freq.snap();
        self.gain.snap();
        self.q.snap();
        self.cur = design(
            self.ty,
            freq_hz_from_norm(self.freq.value),
            gain_db_from_norm(self.gain.value),
            q_from_norm(self.q.value),
            self.slope,
            sample_rate,
        );
        self.prev = self.cur;
        self.ramping = false;
        self.needs_design = false;
    }

    /// A discrete parameter changed. Park the running filter in the fade slot
    /// and let the new one take over across [`XFADE_LEN`] samples.
    fn begin_swap(&mut self, sample_rate: f64) {
        let previous_sections = self.cur.n;
        if self.active {
            self.old = self.cur;
            self.old_state = self.state;
            self.fade = Fade::Swap;
            self.fade_pos = 0;
        }
        // The new filter starts at its own coefficients rather than
        // interpolating from the old type's, which would spend a block
        // passing through filters that are neither. Any continuous parameter
        // still in flight keeps smoothing from where it is.
        self.cur = design(
            self.ty,
            freq_hz_from_norm(self.freq.value),
            gain_db_from_norm(self.gain.value),
            q_from_norm(self.q.value),
            self.slope,
            sample_rate,
        );
        self.prev = self.cur;
        self.ramping = false;
        // A band going from 12 to 24 dB/oct grows a second section, which must
        // not start from whatever the last 24 dB/oct configuration left in
        // that slot. The old state stays intact in `old_state` for the
        // crossfade to finish with.
        for ch in &mut self.state {
            for st in &mut ch[previous_sections..] {
                st.clear();
            }
        }
    }

    fn set_enabled(&mut self, on: bool, sample_rate: f64) {
        if on == self.enabled {
            return;
        }
        self.enabled = on;
        if on {
            // The band was silent, so there is nothing to smooth away from:
            // start it at the settings it is meant to have and fade it in.
            for s in &mut self.state {
                for st in s {
                    st.clear();
                }
            }
            self.redesign_now(sample_rate);
            self.active = true;
            self.fade = Fade::FromDry;
            self.fade_pos = 0;
        } else if self.active {
            self.fade = Fade::ToDry;
            self.fade_pos = 0;
        }
    }

    fn clear_state(&mut self) {
        for s in &mut self.state {
            for st in s {
                st.clear();
            }
        }
        for s in &mut self.old_state {
            for st in s {
                st.clear();
            }
        }
    }

    fn reset(&mut self, sample_rate: f64) {
        self.clear_state();
        self.fade = Fade::None;
        self.fade_pos = 0;
        self.active = self.enabled;
        self.redesign_now(sample_rate);
    }

    /// Once per control block: guard the state, then move the coefficients.
    fn control_tick(&mut self, alpha: f64, sample_rate: f64) {
        if !self.active {
            return;
        }
        for ch in &mut self.state {
            for st in &mut ch[..self.cur.n] {
                st.guard();
            }
        }
        self.prev = self.cur;
        self.ramping = false;
        if self.needs_design {
            let f = self.freq.advance(alpha);
            let g = self.gain.advance(alpha);
            let q = self.q.advance(alpha);
            self.cur = design(
                self.ty,
                freq_hz_from_norm(f),
                gain_db_from_norm(g),
                q_from_norm(q),
                self.slope,
                sample_rate,
            );
            self.ramping = !self.prev.same_as(&self.cur);
            if self.freq.settled() && self.gain.settled() && self.q.settled() {
                self.needs_design = false;
            }
        }
    }

    /// Filter one chunk in place. `block_pos` is the chunk's offset within the
    /// control block, which is what indexes the coefficient ramp.
    fn process_chunk(&mut self, l: &mut [f64], r: &mut [f64], block_pos: usize) {
        if !self.active {
            return;
        }
        if self.fade != Fade::None {
            self.process_fading(l, r, block_pos);
        } else if !self.ramping && self.cur.is_wire() {
            // Exactly 0 dB on a bell or shelf: the design degenerates to
            // `b = a`, so the filter is a wire and the samples pass through
            // untouched rather than through five multiplies that would only
            // add rounding noise. The state still tracks the signal, so
            // nudging the gain off zero starts from the true history.
            let n = self.cur.n;
            for st in &mut self.state[0][..n] {
                st.absorb(l);
            }
            for st in &mut self.state[1][..n] {
                st.absorb(r);
            }
        } else if self.ramping {
            for (i, (xl, xr)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                let t = (block_pos + i + 1) as f64 * INV_CONTROL_BLOCK;
                let mut yl = *xl;
                let mut yr = *xr;
                for s in 0..self.cur.n {
                    let c = self.prev.c[s].lerp(&self.cur.c[s], t);
                    yl = self.state[0][s].tick(&c, yl);
                    yr = self.state[1][s].tick(&c, yr);
                }
                *xl = yl;
                *xr = yr;
            }
        } else {
            for s in 0..self.cur.n {
                let c = self.cur.c[s];
                let (left, right) = self.state.split_at_mut(1);
                let (sl, sr) = (&mut left[0][s], &mut right[0][s]);
                for (xl, xr) in l.iter_mut().zip(r.iter_mut()) {
                    *xl = sl.tick(&c, *xl);
                    *xr = sr.tick(&c, *xr);
                }
            }
        }
    }

    /// The general path: coefficient ramp and crossfade at once. Runs for at
    /// most [`XFADE_LEN`] samples after a discrete change.
    fn process_fading(&mut self, l: &mut [f64], r: &mut [f64], block_pos: usize) {
        let inv_fade = 1.0 / f64::from(XFADE_LEN);
        for (i, (xl, xr)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            if self.fade == Fade::None {
                // The crossfade finished part-way through this chunk; the rest
                // of it is ordinary filtering.
                let t = if self.ramping {
                    (block_pos + i + 1) as f64 * INV_CONTROL_BLOCK
                } else {
                    1.0
                };
                let mut yl = *xl;
                let mut yr = *xr;
                for s in 0..self.cur.n {
                    let c = self.prev.c[s].lerp(&self.cur.c[s], t);
                    yl = self.state[0][s].tick(&c, yl);
                    yr = self.state[1][s].tick(&c, yr);
                }
                *xl = yl;
                *xr = yr;
                continue;
            }

            let dry_l = *xl;
            let dry_r = *xr;
            let t = if self.ramping {
                (block_pos + i + 1) as f64 * INV_CONTROL_BLOCK
            } else {
                1.0
            };
            let mut wet_l = dry_l;
            let mut wet_r = dry_r;
            for s in 0..self.cur.n {
                let c = self.prev.c[s].lerp(&self.cur.c[s], t);
                wet_l = self.state[0][s].tick(&c, wet_l);
                wet_r = self.state[1][s].tick(&c, wet_r);
            }

            // Smoothstep rather than a straight line. A linear crossfade has
            // a slope discontinuity at each end, and slope discontinuities are
            // exactly what the sideband metric measures: switching a +12 dB
            // shelf on costs 17.8 dB of high-frequency injection with a linear
            // ramp and 4.8 dB with this one, and a band type change goes from
            // 12.3 dB to 1.5. One multiply-add per sample, for 64 samples,
            // once per discrete change.
            let lin = f64::from(self.fade_pos) * inv_fade;
            let w = lin * lin * 2.0f64.mul_add(-lin, 3.0);
            match self.fade {
                Fade::FromDry => {
                    *xl = w.mul_add(wet_l - dry_l, dry_l);
                    *xr = w.mul_add(wet_r - dry_r, dry_r);
                }
                Fade::ToDry => {
                    *xl = w.mul_add(dry_l - wet_l, wet_l);
                    *xr = w.mul_add(dry_r - wet_r, wet_r);
                }
                Fade::Swap => {
                    let mut old_l = dry_l;
                    let mut old_r = dry_r;
                    for s in 0..self.old.n {
                        let c = self.old.c[s];
                        old_l = self.old_state[0][s].tick(&c, old_l);
                        old_r = self.old_state[1][s].tick(&c, old_r);
                    }
                    *xl = w.mul_add(wet_l - old_l, old_l);
                    *xr = w.mul_add(wet_r - old_r, old_r);
                }
                Fade::None => unreachable!(),
            }

            self.fade_pos += 1;
            if self.fade_pos >= XFADE_LEN {
                let ending = self.fade;
                self.fade = Fade::None;
                self.fade_pos = 0;
                if ending == Fade::ToDry {
                    self.active = false;
                    self.clear_state();
                    // Everything after this sample is already the dry signal.
                    return;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Flat parameter addressing
// ---------------------------------------------------------------------------

/// The per-band controls, in the order they occupy the flat parameter space.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(usize)]
pub enum BandParam {
    /// [`BandType`], as an index over [`BandType::ALL`].
    Type = 0,
    /// Corner or centre frequency. See [`freq_hz_from_norm`].
    Freq = 1,
    /// Gain. See [`gain_db_from_norm`]. Inert for types where
    /// [`BandType::uses_gain`] is false.
    Gain = 2,
    /// Q. See [`q_from_norm`]. Inert where [`BandType::uses_q`] is false.
    Q = 3,
    /// [`Slope`], as an index over [`Slope::choices_for`].
    Slope = 4,
    /// Band enable, `>= 0.5` for on.
    Enabled = 5,
}

impl BandParam {
    /// Every per-band control, in order.
    pub const ALL: [Self; 6] = [
        Self::Type,
        Self::Freq,
        Self::Gain,
        Self::Q,
        Self::Slope,
        Self::Enabled,
    ];

    /// This control's index in the flat parameter space, for `band`.
    #[must_use]
    pub const fn index(self, band: usize) -> usize {
        band * PARAMS_PER_BAND + self as usize
    }

    /// Name for a parameter list. Terse because it shares a row with a value.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Freq => "freq",
            Self::Gain => "gain",
            Self::Q => "q",
            Self::Slope => "slope",
            Self::Enabled => "on",
        }
    }
}

/// Controls per band in the flat parameter space.
pub const PARAMS_PER_BAND: usize = 6;

/// Flat index of the instance-wide output trim. See [`trim_db_from_norm`].
pub const PARAM_OUTPUT_TRIM: usize = BAND_COUNT * PARAMS_PER_BAND;

/// Size of the flat parameter space: eight bands of six, plus output trim.
pub const PARAM_COUNT: usize = PARAM_OUTPUT_TRIM + 1;

/// Which band and control a flat parameter index addresses, or `None` for the
/// output trim and for anything out of range.
#[must_use]
pub fn param_address(index: usize) -> Option<(usize, BandParam)> {
    if index >= PARAM_OUTPUT_TRIM {
        return None;
    }
    Some((
        index / PARAMS_PER_BAND,
        BandParam::ALL[index % PARAMS_PER_BAND],
    ))
}

/// Name of the control a flat parameter index addresses.
///
/// The band number is not in the string: a caller that needs `"band 3 freq"`
/// has the band index from [`param_address`] already and can format it
/// without this function allocating on its behalf.
#[must_use]
pub fn param_name(index: usize) -> &'static str {
    match param_address(index) {
        None => "trim",
        Some((_, p)) => p.name(),
    }
}

// ---------------------------------------------------------------------------
// The same flat space, in natural units
// ---------------------------------------------------------------------------
//
// The 0..1 surface above is the right shape for a preset file and for a
// generic automation lane, where every control has to look the same. It is
// the wrong thing for a *session* to store. A file written today would hold
// 0.5 for "2.5 kHz", and the day the frequency law's top end moves from
// 30 kHz to 40 kHz every saved session silently re-points — which is exactly
// the defect the instruments' `discrete` table exists to work around.
//
// So the host surface is the number on the knob: hertz, decibels, dB per
// octave. The two meet only here, through the laws at the top of this
// module, and nothing above the EQ ever has to know what 0.5 means.

/// One flat parameter as a host sees it, in the unit the control actually
/// has.
///
/// `&'static str` for both strings: a panel reads these while it is drawing,
/// sixty times a second, and a `String` per parameter per frame is an
/// allocation storm for text that never changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NaturalParam {
    /// The control's name, without its band number — see [`param_name`] for
    /// why the band is the caller's to format.
    pub name: &'static str,
    /// `"Hz"`, `"dB"`, `"dB/oct"`, or empty for the two counted controls.
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    /// The factory setting, which for the per-band controls differs by band.
    pub default: f32,
}

/// What flat parameter `index` is, in natural units, or `None` past the end
/// of the parameter space.
///
/// The defaults come from the same table [`ParametricEq::new`] builds from,
/// so a band whose factory frequency moves cannot leave a stale copy here.
#[must_use]
pub fn natural_param(index: usize) -> Option<NaturalParam> {
    let Some((band, which)) = param_address(index) else {
        return (index == PARAM_OUTPUT_TRIM).then_some(NaturalParam {
            name: "trim",
            unit: "dB",
            min: -TRIM_MAX_DB as f32,
            max: TRIM_MAX_DB as f32,
            default: 0.0,
        });
    };
    let (ty, hz, q, on) = DEFAULT_BANDS[band];
    let name = which.name();
    Some(match which {
        // Counted controls: the value is a position in a list, and the list
        // is [`BandType::ALL`] and 0/1 respectively. Setting one rounds.
        BandParam::Type => NaturalParam {
            name,
            unit: "",
            min: 0.0,
            max: (BandType::ALL.len() - 1) as f32,
            default: ty.index() as f32,
        },
        BandParam::Freq => NaturalParam {
            name,
            unit: "Hz",
            min: FREQ_MIN_HZ as f32,
            max: FREQ_MAX_HZ as f32,
            default: hz as f32,
        },
        BandParam::Gain => NaturalParam {
            name,
            unit: "dB",
            min: -GAIN_MAX_DB as f32,
            max: GAIN_MAX_DB as f32,
            default: 0.0,
        },
        BandParam::Q => NaturalParam {
            name,
            unit: "",
            min: Q_MIN as f32,
            max: Q_MAX as f32,
            default: q as f32,
        },
        // The unit is what the control means, and the travel is every slope
        // any type offers. Which of the three a *given* band will accept is
        // its type's business — see [`Slope::choices_for`] — and asking for
        // one it does not offer lands on the nearest one it does.
        BandParam::Slope => NaturalParam {
            name,
            unit: "dB/oct",
            min: f32::from(Slope::Db6.db_per_octave()),
            max: f32::from(Slope::Db24.db_per_octave()),
            default: f32::from(Slope::Db12.db_per_octave()),
        },
        BandParam::Enabled => NaturalParam {
            name,
            unit: "",
            min: 0.0,
            max: 1.0,
            default: f32::from(u8::from(on)),
        },
    })
}

/// The factory settings as a flat natural-unit vector.
///
/// What a host stores for an EQ nobody has touched, and what
/// [`ParametricEq::new`] answers control for control.
#[must_use]
pub fn default_natural_params() -> [f32; PARAM_COUNT] {
    let mut out = [0.0f32; PARAM_COUNT];
    for (index, value) in out.iter_mut().enumerate() {
        *value = natural_param(index).map_or(0.0, |p| p.default);
    }
    out
}

/// An EQ built from a flat natural-unit vector, settled.
///
/// Every smoother is on its target and every coefficient designed, so
/// [`ParametricEq::response_db`] reads the curve those numbers describe
/// rather than one that is 15 ms behind them. That is what a UI wants and the
/// opposite of what the running instance wants, which is why this builds a
/// second EQ instead of reaching into the one making the sound: the audio
/// thread's EQ is not readable from the UI thread and must not become so.
///
/// Extra values past [`PARAM_COUNT`] are ignored and missing ones keep their
/// factory setting, so a session written by a build with fewer bands loads.
#[must_use]
pub fn eq_from_natural_params(params: &[f32], sample_rate: f64) -> ParametricEq {
    let mut eq = ParametricEq::new(sample_rate);
    for (index, &value) in params.iter().enumerate().take(PARAM_COUNT) {
        eq.set_param_natural(index, value);
    }
    eq.reset();
    eq
}

/// The magnitude response in dB, at one frequency, of the EQ a flat
/// natural-unit vector describes.
///
/// The one-shot form, for a readout or a test. **Drawing a curve should build
/// the mirror once** with [`eq_from_natural_params`] and call
/// [`ParametricEq::response_db`] per point: this function designs eight bands
/// twice per call, which is nothing at one point and eighty times too much at
/// a hundred and sixty of them.
///
/// The sample rate is a parameter because the answer depends on it — a 16 kHz
/// bell at 44.1 kHz is not the filter it is at 96 kHz, and a curve drawn at a
/// rate the engine is not running at is a drawing of a different EQ.
#[must_use]
pub fn eq_response_db(params: &[f32], sample_rate: f64, freq_hz: f64) -> f64 {
    eq_from_natural_params(params, sample_rate).response_db(freq_hz)
}

/// The slope nearest `db_per_octave` among those `ty` offers, or `None` for a
/// type that offers none.
fn nearest_slope(ty: BandType, db_per_octave: f64) -> Option<Slope> {
    Slope::choices_for(ty).iter().copied().min_by(|a, b| {
        let distance = |s: Slope| (f64::from(s.db_per_octave()) - db_per_octave).abs();
        distance(*a).total_cmp(&distance(*b))
    })
}

// ---------------------------------------------------------------------------
// The EQ
// ---------------------------------------------------------------------------

/// An eight-band parametric EQ, stereo, zero latency.
///
/// Construct with [`ParametricEq::new`], drive with the typed setters or the
/// flat [`ParametricEq::set_param`], run with [`ParametricEq::process`], and
/// draw with [`ParametricEq::response_db`].
///
/// Stereo is linked: one set of coefficients per band, two independent state
/// sets. That is right for almost all EQ use and it keeps the null test
/// trivial. Per-band mid/side is a later feature and needs a third state set,
/// a channel-target control per band, and a decision about which domain the
/// instance runs in.
///
/// A fresh instance with nothing touched is a **bit-exact wire**: the two
/// filters that would colour the signal are off, and the six that are on sit
/// at exactly 0 dB, where the matched designs degenerate to `b = a` and
/// [`ParametricEq::process`] copies rather than filters.
#[derive(Clone, Debug)]
pub struct ParametricEq {
    sample_rate: f64,
    /// Block-length smoother pole, `exp(−N/(τ·f_s))`.
    block_alpha: f64,
    bands: [Band; BAND_COUNT],
    trim: Smoother,
    trim_prev: f64,
    trim_cur: f64,
    trim_moving: bool,
    /// Position within the current control block, 0..[`CONTROL_BLOCK`].
    block_pos: usize,
}

/// The factory band layout.
///
/// Four bells over a shelf frame, with the high-pass and low-pass parked off
/// so that inserting an EQ and touching nothing changes nothing. Frequencies
/// are the round numbers a mixer expects to read; the knob walks the
/// [`ISO_SIXTH_OCTAVE_HZ`] grid from wherever it starts.
const DEFAULT_BANDS: [(BandType, f64, f64, bool); BAND_COUNT] = [
    (BandType::HighPass, 30.0, BUTTERWORTH_Q, false),
    (BandType::LowShelf, 120.0, BUTTERWORTH_Q, true),
    (BandType::Bell, 300.0, 1.0, true),
    (BandType::Bell, 800.0, 1.0, true),
    (BandType::Bell, 2500.0, 1.0, true),
    (BandType::Bell, 6000.0, 1.0, true),
    (BandType::HighShelf, 10000.0, BUTTERWORTH_Q, true),
    (BandType::LowPass, 18000.0, BUTTERWORTH_Q, false),
];

impl ParametricEq {
    /// A new EQ at `sample_rate`, with the factory band layout, every gain at
    /// 0 dB and the trim at unity.
    #[must_use]
    pub fn new(sample_rate: f64) -> Self {
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let mut bands = [Band::new(BandType::Bell, 1000.0, 1.0, false, sr); BAND_COUNT];
        for (band, &(ty, hz, q, on)) in bands.iter_mut().zip(DEFAULT_BANDS.iter()) {
            *band = Band::new(ty, hz, q, on, sr);
        }
        Self {
            sample_rate: sr,
            block_alpha: block_alpha(sr),
            bands,
            trim: Smoother::new(norm_from_trim_db(0.0)),
            trim_prev: 1.0,
            trim_cur: 1.0,
            trim_moving: false,
            block_pos: 0,
        }
    }

    /// The sample rate the coefficients are designed for.
    #[must_use]
    pub const fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Processing latency, in samples. Always zero; this is minimum-phase IIR
    /// and it is meant to stay that way.
    #[must_use]
    pub const fn latency_samples(&self) -> usize {
        0
    }

    /// Redesign for a new sample rate and clear all state.
    ///
    /// A rate change means the device changed, so there is nothing worth
    /// crossfading and every coefficient is wrong until it is recomputed.
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        if !sample_rate.is_finite() || sample_rate <= 0.0 || sample_rate == self.sample_rate {
            return;
        }
        self.sample_rate = sample_rate;
        self.block_alpha = block_alpha(sample_rate);
        self.reset();
    }

    /// Clear all filter state, cancel crossfades, and snap every smoother to
    /// its target.
    ///
    /// For transport stops, seeks and preset loads — anywhere the signal is
    /// discontinuous anyway, so there is nothing to protect and everything to
    /// gain from starting the next block at the settings the user asked for.
    pub fn reset(&mut self) {
        let sr = self.sample_rate;
        for band in &mut self.bands {
            band.reset(sr);
        }
        self.trim.snap();
        self.trim_cur = db_to_linear(trim_db_from_norm(self.trim.value));
        self.trim_prev = self.trim_cur;
        self.trim_moving = false;
        self.block_pos = 0;
    }

    // -- typed parameter access ---------------------------------------------

    /// Set a band's type. Crossfades over [`XFADE_LEN`] samples if the band is
    /// audible.
    pub fn set_band_type(&mut self, band: usize, ty: BandType) {
        let sr = self.sample_rate;
        let Some(b) = self.bands.get_mut(band) else {
            debug_assert!(false, "band index {band} out of range");
            return;
        };
        if b.ty == ty {
            return;
        }
        b.ty = ty;
        b.slope = b.slope.resolve(ty);
        b.begin_swap(sr);
    }

    /// A band's type.
    #[must_use]
    pub fn band_type(&self, band: usize) -> BandType {
        self.bands.get(band).map_or(BandType::Bell, |b| b.ty)
    }

    /// Set a band's corner or centre frequency in Hz, clamped to
    /// [`FREQ_MIN_HZ`]..=[`FREQ_MAX_HZ`]. Smoothed.
    pub fn set_band_freq_hz(&mut self, band: usize, hz: f64) {
        self.set_band_norm(band, BandParam::Freq, norm_from_freq_hz(hz));
    }

    /// A band's frequency in Hz, as asked for rather than as currently
    /// smoothed.
    #[must_use]
    pub fn band_freq_hz(&self, band: usize) -> f64 {
        self.bands.get(band).map_or(0.0, Band::freq_hz)
    }

    /// Set a band's gain in dB, clamped to ±[`GAIN_MAX_DB`]. Smoothed.
    pub fn set_band_gain_db(&mut self, band: usize, db: f64) {
        self.set_band_norm(band, BandParam::Gain, norm_from_gain_db(db));
    }

    /// A band's gain in dB.
    #[must_use]
    pub fn band_gain_db(&self, band: usize) -> f64 {
        self.bands.get(band).map_or(0.0, Band::gain_db)
    }

    /// Set a band's Q, clamped to [`Q_MIN`]..=[`Q_MAX`]. Smoothed.
    pub fn set_band_q(&mut self, band: usize, q: f64) {
        self.set_band_norm(band, BandParam::Q, norm_from_q(q));
    }

    /// A band's Q.
    #[must_use]
    pub fn band_q(&self, band: usize) -> f64 {
        self.bands.get(band).map_or(0.0, Band::q_value)
    }

    /// Set a band's slope. Ignored if the type does not offer that slope.
    pub fn set_band_slope(&mut self, band: usize, slope: Slope) {
        let sr = self.sample_rate;
        let Some(b) = self.bands.get_mut(band) else {
            debug_assert!(false, "band index {band} out of range");
            return;
        };
        let resolved = slope.resolve(b.ty);
        if resolved == b.slope {
            return;
        }
        b.slope = resolved;
        b.begin_swap(sr);
    }

    /// A band's slope.
    #[must_use]
    pub fn band_slope(&self, band: usize) -> Slope {
        self.bands.get(band).map_or(Slope::Db12, |b| b.slope)
    }

    /// Enable or disable a band. Crossfades over [`XFADE_LEN`] samples; a
    /// disabled band is skipped entirely and its state zeroed.
    pub fn set_band_enabled(&mut self, band: usize, on: bool) {
        let sr = self.sample_rate;
        let Some(b) = self.bands.get_mut(band) else {
            debug_assert!(false, "band index {band} out of range");
            return;
        };
        b.set_enabled(on, sr);
    }

    /// Whether a band is enabled.
    #[must_use]
    pub fn band_enabled(&self, band: usize) -> bool {
        self.bands.get(band).is_some_and(|b| b.enabled)
    }

    /// Set the output trim in dB, clamped to ±[`TRIM_MAX_DB`]. Smoothed and
    /// interpolated per sample, like everything else.
    pub fn set_output_trim_db(&mut self, db: f64) {
        self.trim.set(norm_from_trim_db(db));
        self.trim_moving = true;
    }

    /// The output trim in dB.
    #[must_use]
    pub fn output_trim_db(&self) -> f64 {
        trim_db_from_norm(self.trim.target)
    }

    fn set_band_norm(&mut self, band: usize, which: BandParam, value: f64) {
        let Some(b) = self.bands.get_mut(band) else {
            debug_assert!(false, "band index {band} out of range");
            return;
        };
        let s = match which {
            BandParam::Freq => &mut b.freq,
            BandParam::Gain => &mut b.gain,
            BandParam::Q => &mut b.q,
            _ => return,
        };
        if s.target == value {
            return;
        }
        s.set(value);
        b.needs_design = true;
    }

    // -- flat parameter access ----------------------------------------------

    /// Set a parameter by its flat index, normalised 0..1.
    ///
    /// The index space is `band * PARAMS_PER_BAND + BandParam`, with
    /// [`PARAM_OUTPUT_TRIM`] last; out-of-range indices are ignored. This is
    /// the surface a generic host or a preset file drives; the typed setters
    /// are the surface the UI drives.
    pub fn set_param(&mut self, index: usize, value: f32) {
        let v = f64::from(value).clamp(0.0, 1.0);
        let Some((band, which)) = param_address(index) else {
            if index == PARAM_OUTPUT_TRIM {
                self.trim.set(v);
                self.trim_moving = true;
            }
            return;
        };
        match which {
            BandParam::Type => {
                let n = BandType::ALL.len() - 1;
                self.set_band_type(band, BandType::from_index((v * n as f64).round() as usize));
            }
            BandParam::Freq | BandParam::Gain | BandParam::Q => {
                self.set_band_norm(band, which, v);
            }
            BandParam::Slope => {
                let choices = Slope::choices_for(self.band_type(band));
                if !choices.is_empty() {
                    let n = choices.len() - 1;
                    self.set_band_slope(band, choices[(v * n as f64).round() as usize]);
                }
            }
            BandParam::Enabled => self.set_band_enabled(band, v >= 0.5),
        }
    }

    /// Read a parameter by its flat index, normalised 0..1.
    ///
    /// Returns the value that was asked for, not the value the smoother has
    /// reached. Out-of-range indices read 0.
    #[must_use]
    pub fn param(&self, index: usize) -> f32 {
        let Some((band, which)) = param_address(index) else {
            return if index == PARAM_OUTPUT_TRIM {
                self.trim.target as f32
            } else {
                0.0
            };
        };
        let Some(b) = self.bands.get(band) else {
            return 0.0;
        };
        let v = match which {
            BandParam::Type => {
                b.ty.index() as f64 / (BandType::ALL.len() - 1) as f64
            }
            BandParam::Freq => b.freq.target,
            BandParam::Gain => b.gain.target,
            BandParam::Q => b.q.target,
            BandParam::Slope => {
                let choices = Slope::choices_for(b.ty);
                match choices.iter().position(|&s| s == b.slope) {
                    Some(i) if choices.len() > 1 => i as f64 / (choices.len() - 1) as f64,
                    _ => 0.0,
                }
            }
            BandParam::Enabled => f64::from(u8::from(b.enabled)),
        };
        v as f32
    }

    // -- natural parameter access -------------------------------------------

    /// Set a parameter by its flat index, in the control's own unit.
    ///
    /// Hertz, decibels and dB per octave; the two counted controls take a
    /// position and round to it. Out-of-range *values* are clamped by the
    /// typed setters this calls, an out-of-range *index* is ignored, and a
    /// NaN moves nothing.
    ///
    /// This is the surface a session and a host drive. The 0..1
    /// [`ParametricEq::set_param`] is the surface a preset file and an
    /// automation lane drive, and neither is a translation of the other: they
    /// are two views of one set of controls, and the laws at the top of this
    /// module are where they meet.
    ///
    /// Applied in index order a whole vector lands correctly, because `type`
    /// comes before `slope` — a slope is only meaningful against the type
    /// that offers it.
    pub fn set_param_natural(&mut self, index: usize, value: f32) {
        if value.is_nan() {
            return;
        }
        let v = f64::from(value);
        let Some((band, which)) = param_address(index) else {
            if index == PARAM_OUTPUT_TRIM {
                self.set_output_trim_db(v);
            }
            return;
        };
        match which {
            // `as usize` saturates at both ends, so a negative or absurd
            // position is 0 or `usize::MAX`, and `from_index` clamps.
            BandParam::Type => self.set_band_type(band, BandType::from_index(v.round() as usize)),
            BandParam::Freq => self.set_band_freq_hz(band, v),
            BandParam::Gain => self.set_band_gain_db(band, v),
            BandParam::Q => self.set_band_q(band, v),
            BandParam::Slope => {
                if let Some(slope) = nearest_slope(self.band_type(band), v) {
                    self.set_band_slope(band, slope);
                }
            }
            BandParam::Enabled => self.set_band_enabled(band, v >= 0.5),
        }
    }

    /// Read a parameter by its flat index, in the control's own unit.
    ///
    /// The value that was asked for rather than the one the smoother has
    /// reached, so a host that writes a control and reads it back gets what
    /// it wrote. Out-of-range indices read 0.
    #[must_use]
    pub fn param_natural(&self, index: usize) -> f32 {
        let Some((band, which)) = param_address(index) else {
            return if index == PARAM_OUTPUT_TRIM {
                self.output_trim_db() as f32
            } else {
                0.0
            };
        };
        let v = match which {
            BandParam::Type => self.band_type(band).index() as f64,
            BandParam::Freq => self.band_freq_hz(band),
            BandParam::Gain => self.band_gain_db(band),
            BandParam::Q => self.band_q(band),
            // A type with no slope choice still has one stored, and reads it:
            // a control that is inert is not a control that has no value.
            BandParam::Slope => f64::from(self.band_slope(band).db_per_octave()),
            BandParam::Enabled => f64::from(u8::from(self.band_enabled(band))),
        };
        v as f32
    }

    // -- audio ---------------------------------------------------------------

    /// Filter a stereo buffer in place.
    ///
    /// Allocates nothing, locks nothing and logs nothing. Both slices are
    /// processed to the length of the shorter one.
    ///
    /// Internally the samples are widened to `f64` once on entry and narrowed
    /// once on exit, in chunks of at most [`CONTROL_BLOCK`] samples on the
    /// stack. Coefficients are recomputed at each control-block boundary and
    /// interpolated across the block.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        debug_assert_eq!(left.len(), right.len(), "stereo buffers must match");
        let n = left.len().min(right.len());
        let mut scratch_l = [0.0f64; CONTROL_BLOCK];
        let mut scratch_r = [0.0f64; CONTROL_BLOCK];
        let mut off = 0;

        while off < n {
            if self.block_pos == 0 {
                self.control_tick();
            }
            let take = (CONTROL_BLOCK - self.block_pos).min(n - off);
            let (sl, sr) = (&mut scratch_l[..take], &mut scratch_r[..take]);
            for (i, (dl, dr)) in sl.iter_mut().zip(sr.iter_mut()).enumerate() {
                *dl = f64::from(left[off + i]);
                *dr = f64::from(right[off + i]);
            }

            for band in &mut self.bands {
                band.process_chunk(sl, sr, self.block_pos);
            }
            self.apply_trim(sl, sr, self.block_pos);

            for (i, (dl, dr)) in sl.iter().zip(sr.iter()).enumerate() {
                left[off + i] = *dl as f32;
                right[off + i] = *dr as f32;
            }

            self.block_pos = (self.block_pos + take) % CONTROL_BLOCK;
            off += take;
        }
    }

    /// Filter one stereo frame.
    ///
    /// A convenience over [`ParametricEq::process`] with the same semantics
    /// and the same control-block timing; a caller that has a buffer should
    /// pass the buffer, which amortises the per-chunk setup over more
    /// samples.
    pub fn process_sample(&mut self, left: f32, right: f32) -> (f32, f32) {
        let mut l = [left];
        let mut r = [right];
        self.process(&mut l, &mut r);
        (l[0], r[0])
    }

    fn apply_trim(&mut self, l: &mut [f64], r: &mut [f64], block_pos: usize) {
        if self.trim_prev == self.trim_cur {
            let g = self.trim_cur;
            if g == 1.0 {
                return;
            }
            for (xl, xr) in l.iter_mut().zip(r.iter_mut()) {
                *xl *= g;
                *xr *= g;
            }
            return;
        }
        let (from, to) = (self.trim_prev, self.trim_cur);
        for (i, (xl, xr)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            let t = (block_pos + i + 1) as f64 * INV_CONTROL_BLOCK;
            let g = t.mul_add(to - from, from);
            *xl *= g;
            *xr *= g;
        }
    }

    fn control_tick(&mut self) {
        let (alpha, sr) = (self.block_alpha, self.sample_rate);
        for band in &mut self.bands {
            band.control_tick(alpha, sr);
        }
        self.trim_prev = self.trim_cur;
        if self.trim_moving {
            let p = self.trim.advance(alpha);
            self.trim_cur = db_to_linear(trim_db_from_norm(p));
            if self.trim.settled() {
                self.trim_moving = false;
            }
        }
    }

    // -- the curve -----------------------------------------------------------

    /// Magnitude response at `freq_hz`, in dB, including the output trim.
    ///
    /// Closed form: one `sin` and about ten flops per band, no FFT and no
    /// audio-thread work. Drawing an 80-column braille curve is 160 of these
    /// for the whole eight-band response, which is cheap enough to recompute
    /// every frame instead of caching and invalidating.
    ///
    /// This reports what the filters are *currently doing*, not what they have
    /// been asked to do — during a 15 ms parameter move the two differ, which
    /// is what makes the drawn curve animate with the sound instead of
    /// jumping ahead of it. In steady state it matches the rendered response
    /// exactly, which is the property the tests assert.
    ///
    /// Returns [`f64::NEG_INFINITY`] at the exact centre of a notch, where the
    /// response really is zero.
    #[must_use]
    pub fn response_db(&self, freq_hz: f64) -> f64 {
        let (phi0, phi1, phi2) = phis(2.0 * PI * freq_hz / self.sample_rate);
        let mut power = 1.0;
        for band in &self.bands {
            if !band.active {
                continue;
            }
            for c in &band.cur.c[..band.cur.n] {
                match c.mag_squared(phi0, phi1, phi2) {
                    Some(p) => power *= p,
                    None => return f64::NEG_INFINITY,
                }
            }
        }
        10.0f64.mul_add(power.log10(), 20.0 * self.trim_cur.log10())
    }

    /// Magnitude response of one band alone, in dB, excluding the output trim.
    ///
    /// For drawing the selected band highlighted against the composite curve.
    /// A disabled band reads 0 dB, which is what it contributes.
    #[must_use]
    pub fn band_response_db(&self, band: usize, freq_hz: f64) -> f64 {
        let Some(b) = self.bands.get(band) else {
            return 0.0;
        };
        if !b.active {
            return 0.0;
        }
        let (phi0, phi1, phi2) = phis(2.0 * PI * freq_hz / self.sample_rate);
        let mut power = 1.0;
        for c in &b.cur.c[..b.cur.n] {
            match c.mag_squared(phi0, phi1, phi2) {
                Some(p) => power *= p,
                None => return f64::NEG_INFINITY,
            }
        }
        10.0 * power.log10()
    }
}

/// The one-pole smoother's block-length pole, `exp(−N/(τ·f_s))`.
fn block_alpha(sample_rate: f64) -> f64 {
    (-(CONTROL_BLOCK as f64) / (SMOOTH_TAU_S * sample_rate)).exp()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// Every hard-coded number below was generated by the reference implementation
// in the design brief — pure-`f64` Python transcribed from the primary papers
// — and never by this module. Generating goldens from the code under test
// proves only that it agrees with itself. Each block cites the table it came
// from.
//
// Tolerances are aggregate, never bit patterns: CI runs three platforms and
// `sin`, `cos`, `exp` and `sqrt` differ between libm implementations at the
// last ulp or two, which propagates through a coefficient computation
// containing `exp(−qω₀)` and a `sqrt` chain.

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f64 = 44100.0;

    // -- helpers ------------------------------------------------------------

    /// An EQ with exactly one band enabled, smoothers snapped, at `fs`.
    fn single(ty: BandType, hz: f64, gain_db: f64, q: f64, slope: Slope, fs: f64) -> ParametricEq {
        let mut eq = ParametricEq::new(fs);
        for b in 0..BAND_COUNT {
            eq.set_band_enabled(b, false);
        }
        eq.set_band_type(0, ty);
        eq.set_band_slope(0, slope);
        eq.set_band_freq_hz(0, hz);
        eq.set_band_gain_db(0, gain_db);
        eq.set_band_q(0, q);
        eq.set_band_enabled(0, true);
        eq.reset();
        eq
    }

    fn bell(hz: f64, gain_db: f64, q: f64, fs: f64) -> ParametricEq {
        single(BandType::Bell, hz, gain_db, q, Slope::Db12, fs)
    }

    fn coeffs(eq: &ParametricEq, band: usize) -> Biquad {
        eq.bands[band].cur.c[0]
    }

    /// Mixed absolute/relative comparison. The absolute floor lets a golden
    /// value of exactly zero — a matched lowpass's `b2`, a one-pole's `a2` —
    /// be checked with the same tolerance as everything else.
    #[track_caller]
    fn close(actual: f64, expected: f64, tol: f64, what: &str) {
        let scale = expected.abs().max(1.0);
        let err = (actual - expected).abs() / scale;
        assert!(
            err <= tol,
            "{what}: got {actual:.17e}, want {expected:.17e} (err {err:.3e} > {tol:.1e})"
        );
    }

    #[track_caller]
    fn close_biquad(got: Biquad, want: [f64; 5], tol: f64, what: &str) {
        close(got.b0, want[0], tol, &format!("{what} b0"));
        close(got.b1, want[1], tol, &format!("{what} b1"));
        close(got.b2, want[2], tol, &format!("{what} b2"));
        close(got.a1, want[3], tol, &format!("{what} a1"));
        close(got.a2, want[4], tol, &format!("{what} a2"));
    }

    /// `|H(f)|` in dB by direct complex evaluation, the slow reference for the
    /// φ-form used in `response_db`.
    fn complex_mag_db(c: Biquad, f: f64, fs: f64) -> f64 {
        let w = 2.0 * PI * f / fs;
        let (cos1, sin1) = ((-w).cos(), (-w).sin());
        let (cos2, sin2) = ((-2.0 * w).cos(), (-2.0 * w).sin());
        let nr = c.b0 + c.b1 * cos1 + c.b2 * cos2;
        let ni = c.b1 * sin1 + c.b2 * sin2;
        let dr = 1.0 + c.a1 * cos1 + c.a2 * cos2;
        let di = c.a1 * sin1 + c.a2 * sin2;
        10.0 * (((nr * nr) + (ni * ni)) / ((dr * dr) + (di * di))).log10()
    }

    // -- analog prototypes, the continuous-time targets ---------------------

    fn analog_bell_db(f: f64, f0: f64, gain_db: f64, q: f64) -> f64 {
        let root_g = 10f64.powf(gain_db / 40.0);
        let (w0, w) = (2.0 * PI * f0, 2.0 * PI * f);
        let real = w0 * w0 - w * w;
        let num_i = w * w0 * root_g / q;
        let den_i = w * w0 / (root_g * q);
        10.0 * ((real.mul_add(real, num_i * num_i)) / (real.mul_add(real, den_i * den_i))).log10()
    }

    fn analog_lowpass_db(f: f64, f0: f64, q: f64) -> f64 {
        let (w0, w) = (2.0 * PI * f0, 2.0 * PI * f);
        let real = w0 * w0 - w * w;
        let imag = w * w0 / q;
        10.0 * ((w0 * w0 * w0 * w0) / real.mul_add(real, imag * imag)).log10()
    }

    fn analog_highpass_db(f: f64, f0: f64, q: f64) -> f64 {
        let (w0, w) = (2.0 * PI * f0, 2.0 * PI * f);
        let real = w0 * w0 - w * w;
        let imag = w * w0 / q;
        10.0 * ((w * w * w * w) / real.mul_add(real, imag * imag)).log10()
    }

    /// Second-order Butterworth shelf,
    /// `H(s) = (1 + √2·g·s + g²s²)/(1 + √2·s/g + s²/g²)`, `g = G^(1/4)`.
    fn analog_highshelf2_db(f: f64, fc: f64, gain_db: f64) -> f64 {
        let g = 10f64.powf(gain_db / 80.0);
        let x = f / fc;
        let root2 = std::f64::consts::SQRT_2;
        let nr = 1.0 - g * g * x * x;
        let ni = root2 * g * x;
        let dr = 1.0 - x * x / (g * g);
        let di = root2 * x / g;
        10.0 * (nr.mul_add(nr, ni * ni) / dr.mul_add(dr, di * di)).log10()
    }

    fn analog_lowshelf2_db(f: f64, fc: f64, gain_db: f64) -> f64 {
        analog_highshelf2_db(f, fc, -gain_db) + gain_db
    }

    // -- rendered measurement ------------------------------------------------

    /// Gain in dB measured from audio actually pushed through `process`.
    ///
    /// A Hann-windowed single-bin DFT of input and output at the probe
    /// frequency, after enough warm-up for the filter to reach steady state.
    /// Chunk length is deliberately not a multiple of the control block, so
    /// the block-splitting path is exercised too.
    fn rendered_db(eq: &mut ParametricEq, freq: f64, fs: f64) -> f64 {
        rendered_db_with(eq, freq, fs, 30_000, 32_768)
    }

    /// [`rendered_db`] with explicit warm-up and analysis lengths, for bands
    /// whose poles sit close enough to the unit circle that the default
    /// warm-up is a fraction of one time constant.
    fn rendered_db_with(
        eq: &mut ParametricEq,
        freq: f64,
        fs: f64,
        warmup: usize,
        analysis: usize,
    ) -> f64 {
        const CHUNK: usize = 300;
        let w = 2.0 * PI * freq / fs;
        let mut n = 0usize;
        let mut l = [0f32; CHUNK];
        let mut r = [0f32; CHUNK];

        while n < warmup {
            for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                let s = (w * (n + i) as f64).sin() as f32;
                *a = s;
                *b = s;
            }
            eq.process(&mut l, &mut r);
            n += CHUNK;
        }

        let (mut in_re, mut in_im, mut out_re, mut out_im) = (0.0, 0.0, 0.0, 0.0);
        let start = n;
        let mut k = 0usize;
        while k < analysis {
            for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                let s = (w * (start + k + i) as f64).sin() as f32;
                *a = s;
                *b = s;
            }
            let dry = l;
            eq.process(&mut l, &mut r);
            for i in 0..CHUNK {
                if k + i >= analysis {
                    break;
                }
                let idx = k + i;
                let win = 0.5 - 0.5 * (2.0 * PI * idx as f64 / analysis as f64).cos();
                let phase = w * (start + idx) as f64;
                let (c, s) = (phase.cos() * win, -phase.sin() * win);
                in_re += f64::from(dry[i]) * c;
                in_im += f64::from(dry[i]) * s;
                out_re += f64::from(l[i]) * c;
                out_im += f64::from(l[i]) * s;
            }
            k += CHUNK;
        }
        10.0 * (out_re.mul_add(out_re, out_im * out_im)
            / in_re.mul_add(in_re, in_im * in_im))
        .log10()
    }

    /// Lowest frequency at which the response reaches `target` dB, by
    /// bisection on the log-frequency axis.
    fn crossing_up(eq: &ParametricEq, target: f64, mut lo: f64, mut hi: f64) -> f64 {
        for _ in 0..200 {
            let mid = (lo * hi).sqrt();
            if eq.response_db(mid) >= target {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        (lo * hi).sqrt()
    }

    // =======================================================================
    // L1 — coefficients against the reference implementation.
    // Brief §8.3 T5 plus the same generator run for the remaining types.
    // Relative tolerance 1e-9: measured libm spread propagates to ~1e-12 here.
    // =======================================================================

    const L1_TOL: f64 = 1e-9;

    #[test]
    fn golden_bell_16k_plus12_q2() {
        // Brief §8.3 T5, row 1. The headline case.
        let eq = bell(16000.0, 12.0, 2.0, FS);
        close_biquad(
            coeffs(&eq, 0),
            [
                1.7325980466812358,
                0.6255604272902443,
                0.16441331062068962,
                0.9577565649759407,
                0.5648152196162287,
            ],
            L1_TOL,
            "bell 16k +12 Q2",
        );
    }

    #[test]
    fn golden_highshelf_10k_plus12() {
        // Brief §8.3 T5, row 2.
        let eq = single(BandType::HighShelf, 10000.0, 12.0, BUTTERWORTH_Q, Slope::Db12, FS);
        close_biquad(
            coeffs(&eq, 0),
            [
                2.012691889935177,
                -1.5294857945024294,
                0.4756570175423308,
                -0.06314912750697602,
                0.02201224048205429,
            ],
            L1_TOL,
            "high shelf 10k +12",
        );
    }

    #[test]
    fn golden_bell_1k_plus6_q1() {
        // Brief §8.3 T5, row 3.
        let eq = bell(1000.0, 6.0, 1.0, FS);
        close_biquad(
            coeffs(&eq, 0),
            [
                1.0501438969026573,
                -1.889576020815245,
                0.8587086774343843,
                -1.884778353520253,
                0.9040549070420494,
            ],
            L1_TOL,
            "bell 1k +6 Q1",
        );
    }

    #[test]
    fn golden_bell_1k_minus6_q1_is_the_reciprocal() {
        // Generated by the reference implementation's reciprocal path.
        let eq = bell(1000.0, -6.0, 1.0, FS);
        close_biquad(
            coeffs(&eq, 0),
            [
                0.9522504515328712,
                -1.7947810381790426,
                0.8608866934412994,
                -1.799349619026909,
                0.8177057258220372,
            ],
            L1_TOL,
            "bell 1k -6 Q1",
        );
    }

    #[test]
    fn golden_lowshelf_100_minus9() {
        let eq = single(BandType::LowShelf, 100.0, -9.0, BUTTERWORTH_Q, Slope::Db12, FS);
        close_biquad(
            coeffs(&eq, 0),
            [
                0.9947358889801705,
                -1.9740030235137707,
                0.9793864820823374,
                -1.9738945147696496,
                0.9742308818638987,
            ],
            L1_TOL,
            "low shelf 100 -9",
        );
    }

    #[test]
    fn golden_highpass_80_butterworth() {
        let eq = single(BandType::HighPass, 80.0, 0.0, BUTTERWORTH_Q, Slope::Db12, FS);
        close_biquad(
            coeffs(&eq, 0),
            [
                0.9919727403329734,
                -1.983945480665947,
                0.9919727403329734,
                -1.9838810444445734,
                0.9840099175428237,
            ],
            L1_TOL,
            "highpass 80",
        );
    }

    #[test]
    fn golden_lowpass_15k_butterworth() {
        let eq = single(BandType::LowPass, 15000.0, 0.0, BUTTERWORTH_Q, Slope::Db12, FS);
        close_biquad(
            coeffs(&eq, 0),
            [
                0.7983793237987975,
                0.22401554657884715,
                0.0,
                -0.026290857900167,
                0.04868572827781175,
            ],
            L1_TOL,
            "lowpass 15k",
        );
    }

    #[test]
    fn golden_bandpass_1k_q2() {
        let eq = single(BandType::BandPass, 1000.0, 0.0, 2.0, Slope::Db12, FS);
        close_biquad(
            coeffs(&eq, 0),
            [
                0.06231865251152148,
                -0.055993769318436644,
                -0.006324883193084836,
                -1.9116802206638621,
                0.9312402970173943,
            ],
            L1_TOL,
            "bandpass 1k Q2",
        );
    }

    #[test]
    fn golden_notch_1k_q4_is_rbj() {
        let eq = single(BandType::Notch, 1000.0, 0.0, 4.0, Slope::Db12, FS);
        close_biquad(
            coeffs(&eq, 0),
            [
                0.9825602533712839,
                -1.9452088697173038,
                0.9825602533712839,
                -1.9452088697173038,
                0.9651205067425679,
            ],
            L1_TOL,
            "notch 1k Q4",
        );
    }

    #[test]
    fn golden_allpass_1k_butterworth_is_rbj() {
        let eq = single(BandType::AllPass, 1000.0, 0.0, BUTTERWORTH_Q, Slope::Db12, FS);
        close_biquad(
            coeffs(&eq, 0),
            [
                0.817512403384758,
                -1.799096409484668,
                1.0,
                -1.799096409484668,
                0.817512403384758,
            ],
            L1_TOL,
            "allpass 1k",
        );
    }

    #[test]
    fn golden_one_pole_shelves() {
        // Brief §7.4, the 2019 matched one-pole shelf. `b2` and `a2` are zero
        // by construction, which the mixed tolerance in `close` checks.
        let hi = single(BandType::HighShelf, 10000.0, 12.0, BUTTERWORTH_Q, Slope::Db6, FS);
        close_biquad(
            coeffs(&hi, 0),
            [
                1.956891358501084,
                -0.9493370661949639,
                0.0,
                0.007554292306120245,
                0.0,
            ],
            L1_TOL,
            "1-pole high shelf 10k +12",
        );
        let lo = single(BandType::LowShelf, 200.0, -6.0, BUTTERWORTH_Q, Slope::Db6, FS);
        close_biquad(
            coeffs(&lo, 0),
            [
                0.99008960463622,
                -0.970316411601222,
                0.0,
                -0.9605472930906633,
                0.0,
            ],
            L1_TOL,
            "1-pole low shelf 200 -6",
        );
    }

    #[test]
    fn golden_highpass_24db_is_a_butterworth_cascade() {
        // Brief §4.3: section Qs 0.5411961 and 1.3065630.
        let eq = single(BandType::HighPass, 100.0, 0.0, BUTTERWORTH_Q, Slope::Db24, FS);
        let s = eq.bands[0].cur;
        assert_eq!(s.n, 2, "24 dB/oct must be two sections");
        close_biquad(
            s.c[0],
            [
                0.9869350065559876,
                -1.9738700131119753,
                0.9869350065559876,
                -1.9738170636080645,
                0.9740174051958416,
            ],
            L1_TOL,
            "hp24 section 0",
        );
        close_biquad(
            s.c[1],
            [
                0.9945506253523624,
                -1.9891012507047248,
                0.9945506253523624,
                -1.9889527224214547,
                0.9891546099319116,
            ],
            L1_TOL,
            "hp24 section 1",
        );
    }

    // =======================================================================
    // L2 — closed form against the analog prototype the knob promises.
    // This is the layer a cookbook implementation fails. Brief §8.3.
    // =======================================================================

    #[test]
    fn t1_bell_16k_plus12_q2_is_not_cramped() {
        // Brief §8.3 T1. Columns: probe, analog, required, per-point tolerance.
        // What RBJ produces at each probe is in the comment, and every one of
        // them is outside the tolerance: this test fails a cookbook build.
        let eq = bell(16000.0, 12.0, 2.0, FS);
        let cases = [
            //  f          analog   tol     RBJ gives
            (1_000.0, 0.01591, 0.05),  // +0.004
            (8_000.0, 1.47113, 0.30),  // +0.400
            (11_314.0, 4.24404, 0.35), // +1.310
            (12_492.0, 5.99928, 0.35), // +2.112  <- the diagnostic one
            (14_000.0, 8.99304, 0.20), // +4.343
            (16_000.0, 12.00000, 0.01),// +12.000 (RBJ's peak is exact)
            (18_000.0, 9.47724, 0.25), // +3.139
            (20_000.0, 6.54640, 0.75), // +0.482
        ];
        for (f, analog, tol) in cases {
            let got = eq.response_db(f);
            assert!(
                (got - analog).abs() <= tol,
                "{f} Hz: got {got:+.5} dB, analog prototype {analog:+.5} dB, tol {tol}"
            );
            close(analog_bell_db(f, 16000.0, 12.0, 2.0), analog, 1e-4, "analog prototype itself");
        }
    }

    #[test]
    fn t1_measured_q_is_the_q_on_the_knob() {
        // The cramping killer. Ask for Q 2 at 16 kHz and measure the Q the
        // filter actually has, from its lower half-gain (+6 dB) point and the
        // geometric symmetry of the prototype: f_lo·f_hi = f₀², so
        // Q = f₀·f_lo/(f₀² − f_lo²). Measuring from the lower point is not a
        // convenience — the matched design's *upper* half-gain point is above
        // Nyquist at this sample rate, exactly as the analog filter's is at
        // 20 492 Hz.
        //
        // Analog:  f_lo = 12 492 Hz, Q = 2.000
        // Matched: f_lo = 12 391 Hz, Q = 1.935   (3.3% low)
        // RBJ:     f_lo = 14 579 Hz, Q = 5.367   (168% high; measured the
        //          other way, f₀/(f_hi − f_lo), RBJ reads Q 6.08 against a
        //          knob that says 2)
        let eq = bell(16000.0, 12.0, 2.0, FS);
        let f0 = 16000.0;
        let f_lo = crossing_up(&eq, 6.0, 1000.0, 16000.0);
        close(f_lo, 12391.29, 5e-4, "lower half-gain point");

        let q_measured = f0 * f_lo / f0.mul_add(f0, -(f_lo * f_lo));
        assert!(
            (q_measured - 2.0).abs() <= 0.2,
            "measured Q {q_measured:.4} is more than 10% from the Q 2 asked for"
        );

        // And the half-gain bandwidth in octaves. Analog is 0.714; RBJ manages
        // 0.240. The upper edge is past Nyquist, so this is a lower bound.
        let bandwidth_oct = (0.5 * FS / f_lo).log2();
        assert!(
            bandwidth_oct >= 0.65,
            "half-gain bandwidth {bandwidth_oct:.3} octaves, want at least 0.65"
        );
        assert!(
            eq.response_db(0.49 * FS) > 6.0,
            "the upper half-gain point should be above Nyquist, as it is in the analog filter"
        );
    }

    #[test]
    fn t1_the_design_is_not_the_cookbook() {
        // Brief §8.3 T6. Build RBJ's peaking coefficients for T1's parameters
        // and assert the shipped ones are nowhere near them. If someone
        // "simplifies" this module back to the cookbook, this is the test that
        // says so in one line.
        let w0 = 2.0 * PI * 16000.0 / FS;
        let a = 10f64.powf(12.0 / 40.0);
        let alpha = w0.sin() / (2.0 * 2.0);
        let rbj_b0 = (1.0 + alpha * a) / (1.0 + alpha / a);
        let ours = coeffs(&bell(16000.0, 12.0, 2.0, FS), 0).b0;
        assert!(
            (ours - rbj_b0).abs() > 0.1,
            "b0 {ours:.6} is within 0.1 of the RBJ cookbook's {rbj_b0:.6} — \
             this design is supposed to be a matched one"
        );
    }

    #[test]
    fn t2_high_shelf_10k_plus12() {
        // Brief §8.3 T2.
        let eq = single(BandType::HighShelf, 10000.0, 12.0, BUTTERWORTH_Q, Slope::Db12, FS);
        let cases = [
            //  f         analog    tol     RBJ S=1 gives
            (5_000.0, 0.89734, 0.10),   // +0.521
            (7_071.0, 2.73544, 0.10),   // +2.024
            (10_000.0, 6.00000, 0.15),  // +6.000
            (14_142.0, 9.26442, 0.15),  // +10.784 <- the diagnostic one
            (16_000.0, 10.10194, 0.20), // +11.617
            (20_000.0, 11.10266, 0.25), // +11.996
        ];
        for (f, analog, tol) in cases {
            let got = eq.response_db(f);
            assert!(
                (got - analog).abs() <= tol,
                "{f} Hz: got {got:+.5} dB, analog {analog:+.5} dB, tol {tol}"
            );
            close(analog_highshelf2_db(f, 10000.0, 12.0), analog, 1e-4, "analog prototype itself");
        }
    }

    #[test]
    fn t2_high_shelf_transition_starts_where_the_analog_one_does() {
        // A +12 dB high shelf at 16 kHz first reaches +1 dB at 8 252 Hz in the
        // analog prototype and 8 148 Hz here. RBJ does not get there until
        // 11 827 Hz — half an octave late, which is what a cramped shelf
        // sounds like.
        let eq = single(BandType::HighShelf, 16000.0, 12.0, BUTTERWORTH_Q, Slope::Db12, FS);
        let plus_one = crossing_up(&eq, 1.0, 1000.0, 16000.0);
        assert!(
            plus_one < 8600.0,
            "+1 dB point at {plus_one:.0} Hz; analog is 8252, RBJ is 11827"
        );
        close(plus_one, 8148.03, 1e-3, "+1 dB point");
    }

    #[test]
    fn t3_lowpass_15k_is_not_too_steep() {
        // Brief §8.3 T3. RBJ is 16.7 dB off at 20 kHz; this test alone is
        // unambiguous.
        let eq = single(BandType::LowPass, 15000.0, 0.0, BUTTERWORTH_Q, Slope::Db12, FS);
        let cases = [
            //  f          analog    tol     RBJ gives
            (10_000.0, -0.78287, 0.15), // -0.214
            (15_000.0, -3.01030, 0.01), // -3.010
            (17_500.0, -4.55244, 0.40), // -9.106
            (20_000.0, -6.19145, 1.20), // -22.909  <- the diagnostic one
        ];
        for (f, analog, tol) in cases {
            let got = eq.response_db(f);
            assert!(
                (got - analog).abs() <= tol,
                "{f} Hz: got {got:+.5} dB, analog {analog:+.5} dB, tol {tol}"
            );
            close(analog_lowpass_db(f, 15000.0, BUTTERWORTH_Q), analog, 1e-4, "analog itself");
        }
    }

    #[test]
    fn t4_ordinary_settings_do_not_move() {
        // Brief §8.3 T4: tight anchors on the common case, so that a refactor
        // cannot quietly change the sound of an everyday band.
        let b = bell(1000.0, 6.0, 1.0, FS);
        close(b.response_db(618.0), 2.999647, 5e-6, "bell 1k +6 Q1 @618");
        close(b.response_db(1000.0), 6.000000, 5e-6, "bell 1k +6 Q1 @1k");
        close(b.response_db(1618.0), 3.000145, 5e-6, "bell 1k +6 Q1 @1618");

        let s = single(BandType::LowShelf, 100.0, -9.0, BUTTERWORTH_Q, Slope::Db12, FS);
        close(s.response_db(20.0), -8.982925, 5e-6, "low shelf 100 -9 @20");
        close(s.response_db(100.0), -4.500000, 5e-6, "low shelf 100 -9 @100");
        close(s.response_db(200.0), -0.609367, 5e-6, "low shelf 100 -9 @200");

        // And the analog prototype agrees to four decimals, which is what
        // makes those numbers meaningful rather than self-referential.
        close(analog_bell_db(618.0, 1000.0, 6.0, 1.0), 2.999645, 1e-5, "analog @618");
        close(analog_lowshelf2_db(200.0, 100.0, -9.0), -0.609367, 1e-5, "analog @200");
    }

    #[test]
    fn highpass_at_80_hz_agrees_with_the_cookbook() {
        // The honest counterpoint: below about f₀/f_s = 0.005 the bilinear
        // transform is perfect and the matched design costs nothing. Both are
        // within 0.0002 dB of analog, so a rumble filter is not why this
        // module exists.
        let eq = single(BandType::HighPass, 80.0, 0.0, BUTTERWORTH_Q, Slope::Db12, FS);
        for f in [20.0, 40.0, 80.0, 160.0, 500.0, 2000.0] {
            let analog = analog_highpass_db(f, 80.0, BUTTERWORTH_Q);
            assert!(
                (eq.response_db(f) - analog).abs() < 0.001,
                "{f} Hz: {} vs analog {analog}",
                eq.response_db(f)
            );
        }
    }

    #[test]
    fn closed_form_matches_direct_complex_evaluation() {
        // Brief §8.2: the φ-form is worth 1e-6 dB of tolerance against complex
        // arithmetic; measured discrepancy is 2.3e-8 dB.
        for &ty in &BandType::ALL {
            for f0 in [30.0, 250.0, 3000.0, 12000.0] {
                for g in [-12.0, 0.5, 9.0] {
                    let eq = single(ty, f0, g, 1.7, Slope::Db12, FS);
                    let c = coeffs(&eq, 0);
                    for f in [25.0, 120.0, 900.0, 5000.0, 17000.0] {
                        let direct = complex_mag_db(c, f, FS);
                        let phi = eq.band_response_db(0, f);
                        assert!(
                            (direct - phi).abs() < 1e-6,
                            "{ty:?} f0={f0} g={g} probe={f}: φ-form {phi} vs complex {direct}"
                        );
                    }
                }
            }
        }
    }

    // =======================================================================
    // Reciprocity, degeneracy, stability
    // =======================================================================

    #[test]
    fn boost_and_cut_cancel_exactly() {
        // Brief §4.1: the reciprocal construction recovers the cookbook's
        // exact-reciprocity property, which users notice, without giving up
        // accuracy. Measured residual in the reference implementation is
        // 6.2e-15 dB; a directly designed matched cut leaves 1.126 dB.
        for &f0 in &[30.0, 120.0, 1000.0, 6000.0, 16000.0] {
            for &g in &[0.5, 3.0, 6.0, 12.0, 18.0] {
                for &q in &[0.2, 0.707, 2.0, 8.0] {
                    let up = coeffs(&bell(f0, g, q, FS), 0);
                    let down = coeffs(&bell(f0, -g, q, FS), 0);
                    for f in [20.0, 100.0, 440.0, 2500.0, 11000.0, 20000.0] {
                        let residual = complex_mag_db(up, f, FS) + complex_mag_db(down, f, FS);
                        assert!(
                            residual.abs() < 0.01,
                            "f0={f0} g={g} q={q} probe={f}: {residual:.3e} dB left over"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn boost_and_cut_cancel_to_machine_precision() {
        // The tight version of the above on the headline case, where the doc
        // measured 6.2e-15 dB.
        let up = coeffs(&bell(16000.0, 12.0, 2.0, FS), 0);
        let down = coeffs(&bell(16000.0, -12.0, 2.0, FS), 0);
        for f in [100.0, 1000.0, 8000.0, 12492.0, 16000.0, 20000.0] {
            let residual = complex_mag_db(up, f, FS) + complex_mag_db(down, f, FS);
            assert!(residual.abs() < 1e-9, "probe {f}: {residual:.3e} dB");
        }
    }

    #[test]
    fn zero_gain_is_exactly_a_wire() {
        // Brief §8.4 "Identity at 0 dB". At unity gain the matched peaking
        // design degenerates continuously to b = a, and the shelves are
        // forced to the same place because their linear system is singular
        // there. Exact, not approximate: the identity fast path in `process`
        // is a bit comparison.
        for &ty in &[BandType::Bell, BandType::LowShelf, BandType::HighShelf] {
            for &slope in &[Slope::Db6, Slope::Db12] {
                for &f0 in &[20.0, 120.0, 1000.0, 10000.0, 25000.0] {
                    let eq = single(ty, f0, 0.0, 1.3, slope, FS);
                    let c = coeffs(&eq, 0);
                    assert!(
                        c.is_wire(),
                        "{ty:?} {slope:?} at {f0} Hz, 0 dB: b = ({}, {}, {}), a = (1, {}, {})",
                        c.b0, c.b1, c.b2, c.a1, c.a2
                    );
                    for f in [20.0, 1000.0, 20000.0] {
                        assert_eq!(eq.response_db(f), 0.0, "{ty:?} at {f} Hz");
                    }
                }
            }
        }
    }

    #[test]
    fn crossing_zero_gain_is_continuous() {
        // The wire is a *wire with the design's poles*, not a bare identity,
        // so the coefficients do not jump as the gain crosses zero — which
        // matters because the block interpolator walks straight across it.
        for &ty in &[BandType::Bell, BandType::LowShelf, BandType::HighShelf] {
            let a = coeffs(&single(ty, 5000.0, -0.001, 1.0, Slope::Db12, FS), 0);
            let z = coeffs(&single(ty, 5000.0, 0.0, 1.0, Slope::Db12, FS), 0);
            let b = coeffs(&single(ty, 5000.0, 0.001, 1.0, Slope::Db12, FS), 0);
            for (name, x, y) in [("below", a, z), ("above", b, z)] {
                assert!(
                    (x.b0 - y.b0).abs() < 1e-3
                        && (x.a1 - y.a1).abs() < 1e-3
                        && (x.a2 - y.a2).abs() < 1e-3,
                    "{ty:?} {name} zero: coefficients jump"
                );
            }
        }
    }

    #[test]
    fn every_design_is_stable_and_minimum_phase() {
        // Brief §8.4 "Stability sweep". The reference implementation swept
        // 20 700 points and found zero failures, worst |pole| 0.9999993. This
        // is the same sweep at a coarser grid, run through the real `design`.
        let rates = [44100.0, 48000.0, 88200.0, 96000.0, 176_400.0, 192_000.0];
        let freqs = [10.0, 20.0, 60.0, 250.0, 1000.0, 5000.0, 16000.0, 24000.0, 30000.0];
        let gains = [-18.0, -12.0, -3.0, 0.0, 3.0, 12.0, 18.0];
        let qs = [0.1, 0.5, BUTTERWORTH_Q, 1.0, 4.0, 12.0, 40.0];
        let mut worst_pole: f64 = 0.0;
        let mut worst_zero: f64 = 0.0;
        let mut checked = 0usize;

        for &fs in &rates {
            for &ty in &BandType::ALL {
                for &f0 in &freqs {
                    for &g in &gains {
                        for &q in &qs {
                            for &slope in &[Slope::Db6, Slope::Db12, Slope::Db24] {
                                let s = design(ty, f0, g, q, slope.resolve(ty), fs);
                                for c in &s.c[..s.n] {
                                    assert!(
                                        c.b0.is_finite()
                                            && c.b1.is_finite()
                                            && c.b2.is_finite()
                                            && c.a1.is_finite()
                                            && c.a2.is_finite(),
                                        "{ty:?} {f0} {g} {q} @{fs}: non-finite coefficient"
                                    );
                                    worst_pole = worst_pole.max(root_radius(c.a1, c.a2));
                                    if ty != BandType::Notch && ty != BandType::AllPass {
                                        worst_zero =
                                            worst_zero.max(root_radius(c.b1 / c.b0, c.b2 / c.b0));
                                    }
                                    checked += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 20_000, "sweep covered only {checked} sections");
        assert!(worst_pole < 1.0, "worst |pole| {worst_pole}");
        assert!(
            worst_zero <= 1.0 + 1e-9,
            "worst |zero| {worst_zero}: the matched numerator is supposed to be \
             minimum-phase, which is what makes the reciprocal cut stable"
        );
    }

    /// Larger of the two root magnitudes of `z² + a1·z + a2`.
    fn root_radius(a1: f64, a2: f64) -> f64 {
        let disc = a1.mul_add(a1, -4.0 * a2);
        if disc >= 0.0 {
            let r = disc.sqrt();
            (0.5 * (-a1 + r)).abs().max((0.5 * (-a1 - r)).abs())
        } else {
            a2.abs().sqrt()
        }
    }

    #[test]
    fn f32_coefficients_would_ruin_a_low_band() {
        // Brief §3.2, and the reason `Biquad` is `f64`. Ask a 20 Hz Q 8 bell
        // for +12 dB at 192 kHz: the design is right, but round its
        // coefficients to f32 and the filter delivers about +5.75 dB. Not
        // noise — the wrong filter, off by half, before a sample is
        // processed. `1 + a1 + a2` is 1.7e-6 here, which is 19 bits of
        // cancellation against f32's 24 of mantissa.
        let eq = bell(20.0, 12.0, 8.0, 192_000.0);
        let c = coeffs(&eq, 0);
        let exact = complex_mag_db(c, 20.0, 192_000.0);
        assert!(
            (exact - 12.0).abs() < 0.001,
            "f64 path should deliver +12.000 dB, got {exact:+.4}"
        );

        let narrowed = Biquad {
            b0: f64::from(c.b0 as f32),
            b1: f64::from(c.b1 as f32),
            b2: f64::from(c.b2 as f32),
            a1: f64::from(c.a1 as f32),
            a2: f64::from(c.a2 as f32),
        };
        let damaged = complex_mag_db(narrowed, 20.0, 192_000.0);
        assert!(
            damaged < 8.0,
            "rounding the coefficients to f32 should cost most of the boost, \
             but it still delivers {damaged:+.4} dB — if this fires because the \
             damage got *smaller*, check that the design is still correct"
        );
    }

    // =======================================================================
    // L3 — audio actually pushed through `process`
    // =======================================================================

    #[test]
    fn rendered_response_matches_the_drawn_curve() {
        // Brief §8.4 "Rendered = closed form". If these two ever disagree the
        // terminal is drawing a lie. Tolerance 0.02 dB is the measurement
        // noise of a windowed DFT on a finite sine, not filter error.
        let cases = [
            (BandType::Bell, 1000.0, 6.0, 1.0, Slope::Db12),
            (BandType::Bell, 16000.0, 12.0, 2.0, Slope::Db12),
            (BandType::Bell, 120.0, -9.0, 4.0, Slope::Db12),
            (BandType::LowShelf, 200.0, 8.0, BUTTERWORTH_Q, Slope::Db12),
            (BandType::LowShelf, 200.0, -8.0, BUTTERWORTH_Q, Slope::Db6),
            (BandType::HighShelf, 8000.0, 10.0, BUTTERWORTH_Q, Slope::Db12),
            (BandType::HighShelf, 8000.0, 10.0, BUTTERWORTH_Q, Slope::Db6),
            (BandType::HighPass, 100.0, 0.0, BUTTERWORTH_Q, Slope::Db12),
            (BandType::HighPass, 100.0, 0.0, BUTTERWORTH_Q, Slope::Db24),
            (BandType::LowPass, 6000.0, 0.0, BUTTERWORTH_Q, Slope::Db24),
            (BandType::Notch, 2000.0, 0.0, 6.0, Slope::Db12),
            (BandType::BandPass, 1500.0, 0.0, 2.0, Slope::Db12),
            (BandType::AllPass, 1000.0, 0.0, 1.0, Slope::Db12),
        ];
        let probes = [80.0, 200.0, 450.0, 1000.0, 1900.0, 3000.0, 5500.0, 9000.0, 15000.0];
        for (ty, f0, g, q, slope) in cases {
            for f in probes {
                let mut eq = single(ty, f0, g, q, slope, FS);
                let predicted = eq.response_db(f);
                let measured = rendered_db(&mut eq, f, FS);
                assert!(
                    (measured - predicted).abs() < 0.02,
                    "{ty:?} f0={f0} g={g} q={q} {slope:?}, probe {f} Hz: \
                     rendered {measured:+.5} dB vs curve {predicted:+.5} dB"
                );
            }
        }
    }

    #[test]
    fn rendered_response_includes_the_output_trim() {
        let mut eq = bell(1000.0, 6.0, 1.0, FS);
        eq.set_output_trim_db(-6.0);
        eq.reset();
        let predicted = eq.response_db(1000.0);
        close(predicted, 0.0, 1e-9, "+6 dB band with -6 dB trim");
        let measured = rendered_db(&mut eq, 1000.0, FS);
        assert!((measured - predicted).abs() < 0.02, "rendered {measured}");
    }

    #[test]
    fn a_full_chain_of_bands_renders_what_the_curve_says() {
        // Eight bands at once, all different, so band ordering and the
        // series-product form of the curve are both exercised.
        let mut eq = ParametricEq::new(FS);
        let setup = [
            (BandType::HighPass, 40.0, 0.0, BUTTERWORTH_Q, Slope::Db24),
            (BandType::LowShelf, 150.0, 4.5, BUTTERWORTH_Q, Slope::Db12),
            (BandType::Bell, 300.0, -6.0, 2.0, Slope::Db12),
            (BandType::Bell, 800.0, 3.0, 0.5, Slope::Db12),
            (BandType::Bell, 2500.0, -2.5, 8.0, Slope::Db12),
            (BandType::Bell, 6000.0, 7.0, 1.2, Slope::Db12),
            (BandType::HighShelf, 10000.0, -5.0, BUTTERWORTH_Q, Slope::Db12),
            (BandType::LowPass, 15000.0, 0.0, BUTTERWORTH_Q, Slope::Db12),
        ];
        for (i, (ty, f0, g, q, slope)) in setup.into_iter().enumerate() {
            eq.set_band_type(i, ty);
            eq.set_band_slope(i, slope);
            eq.set_band_freq_hz(i, f0);
            eq.set_band_gain_db(i, g);
            eq.set_band_q(i, q);
            eq.set_band_enabled(i, true);
        }
        eq.set_output_trim_db(2.0);
        eq.reset();
        for f in [60.0, 150.0, 300.0, 800.0, 2500.0, 6000.0, 11000.0, 14000.0] {
            let predicted = eq.response_db(f);
            let mut probe = eq.clone();
            let measured = rendered_db(&mut probe, f, FS);
            assert!(
                (measured - predicted).abs() < 0.02,
                "{f} Hz: rendered {measured:+.5} vs curve {predicted:+.5}"
            );
        }
    }

    #[test]
    fn every_default_band_position_matches_the_closed_form() {
        // The factory layout, nudged off zero so each band is doing something,
        // measured at its own centre frequency. ±0.1 dB.
        let mut eq = ParametricEq::new(FS);
        for b in 0..BAND_COUNT {
            eq.set_band_enabled(b, true);
            if eq.band_type(b).uses_gain() {
                eq.set_band_gain_db(b, if b % 2 == 0 { 4.0 } else { -4.0 });
            }
        }
        eq.reset();
        for b in 0..BAND_COUNT {
            let f = eq.band_freq_hz(b);
            let predicted = eq.response_db(f);
            let mut probe = eq.clone();
            let measured = rendered_db(&mut probe, f, FS);
            assert!(
                (measured - predicted).abs() < 0.1,
                "band {b} ({:?} at {f} Hz): rendered {measured:+.4} vs curve {predicted:+.4}",
                eq.band_type(b)
            );
        }
    }

    #[test]
    fn the_response_does_not_depend_on_the_sample_rate() {
        // Where matched designs beat the cookbook and where it is worth the
        // most: high in the spectrum. The same 16 kHz +12 dB Q 2 bell at four
        // rates, compared against the one analog curve it is imitating.
        //
        // Matched spread across the four rates, measured: 0.166 dB at
        // 12 492 Hz, 0.077 at 14 kHz, 0.140 at 18 kHz, 0.668 at 20 kHz.
        // RBJ's, for the same four rates: 3.69, 4.48, 6.14, 5.76 dB.
        let rates = [44100.0, 48000.0, 96000.0, 192_000.0];
        for (probe, spread_tol, analog_tol) in [
            (12_492.0, 0.20, 0.20),
            (14_000.0, 0.10, 0.10),
            (18_000.0, 0.20, 0.15),
            (20_000.0, 0.70, 0.70),
        ] {
            let analog = analog_bell_db(probe, 16000.0, 12.0, 2.0);
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for fs in rates {
                let got = bell(16000.0, 12.0, 2.0, fs).response_db(probe);
                assert!(
                    (got - analog).abs() <= analog_tol,
                    "{probe} Hz at {fs}: {got:+.4} vs analog {analog:+.4}"
                );
                lo = lo.min(got);
                hi = hi.max(got);
            }
            assert!(
                hi - lo <= spread_tol,
                "{probe} Hz varies by {:.4} dB across sample rates",
                hi - lo
            );
        }
    }

    #[test]
    fn low_bands_survive_high_sample_rates() {
        // The other end of §3.2: a 20 Hz Q 8 band at 192 kHz is where the
        // coefficients are worst-conditioned — |pole| is 0.99998 and
        // `1 + a1 + a2` is 1.7e-6 — and f64 is what makes it a non-event.
        // Designed and rendered both, because the design being right is no
        // use if the state cannot carry it.
        for fs in [44100.0, 96000.0, 192_000.0] {
            let eq = bell(20.0, 12.0, 8.0, fs);
            close(eq.response_db(20.0), 12.0, 1e-4, "designed 20 Hz Q8 +12");
        }
        // Rendered at 44.1 kHz only: the pole's time constant is 12 000
        // samples here and 49 000 at 192 kHz, so a settled measurement up
        // there costs about two million samples for nothing the closed form
        // above has not already established.
        let mut eq = bell(20.0, 12.0, 8.0, FS);
        let measured = rendered_db_with(&mut eq, 20.0, FS, 200_000, 262_144);
        assert!(
            (measured - 12.0).abs() < 0.02,
            "20 Hz Q8 +12 dB rendered {measured:+.4} dB"
        );
    }

    // =======================================================================
    // Null tests
    // =======================================================================

    fn noise(n: usize) -> Vec<f32> {
        // A deterministic 32-bit xorshift, so a failure is reproducible.
        let mut state = 0x2545_F491u32;
        (0..n)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (f64::from(state) / f64::from(u32::MAX)).mul_add(2.0, -1.0) as f32 * 0.5
            })
            .collect()
    }

    #[track_caller]
    fn assert_bit_identical(eq: &mut ParametricEq, what: &str) {
        let dry = noise(8192);
        let mut l: Vec<f32> = dry.clone();
        let mut r: Vec<f32> = dry.iter().rev().copied().collect();
        let dry_r = r.clone();
        for (cl, cr) in l.chunks_mut(97).zip(r.chunks_mut(97)) {
            eq.process(cl, cr);
        }
        for (i, (got, want)) in l.iter().zip(dry.iter()).enumerate() {
            assert!(
                got.to_bits() == want.to_bits(),
                "{what}: sample {i} left is {got:?}, input was {want:?}"
            );
        }
        for (i, (got, want)) in r.iter().zip(dry_r.iter()).enumerate() {
            assert!(
                got.to_bits() == want.to_bits(),
                "{what}: sample {i} right is {got:?}, input was {want:?}"
            );
        }
    }

    #[test]
    fn a_fresh_instance_is_a_bit_exact_wire() {
        // Insert an EQ, touch nothing, and the track is unchanged — not
        // "unchanged to within a rounding error", unchanged. Two bands are off
        // and the other six sit at exactly 0 dB, where the matched design
        // degenerates to b = a and `process` copies instead of filtering.
        let mut eq = ParametricEq::new(FS);
        assert_bit_identical(&mut eq, "fresh instance");
    }

    #[test]
    fn all_bands_disabled_is_a_bit_exact_wire() {
        let mut eq = ParametricEq::new(FS);
        for b in 0..BAND_COUNT {
            eq.set_band_enabled(b, false);
        }
        eq.reset();
        assert_bit_identical(&mut eq, "all bands disabled");
    }

    #[test]
    fn eight_bands_at_zero_gain_are_a_bit_exact_wire() {
        let mut eq = ParametricEq::new(FS);
        for (b, ty) in [
            BandType::Bell,
            BandType::LowShelf,
            BandType::HighShelf,
            BandType::Bell,
            BandType::LowShelf,
            BandType::HighShelf,
            BandType::Bell,
            BandType::Bell,
        ]
        .into_iter()
        .enumerate()
        {
            eq.set_band_type(b, ty);
            eq.set_band_freq_hz(b, 100.0 * (b + 1) as f64);
            eq.set_band_gain_db(b, 0.0);
            eq.set_band_enabled(b, true);
        }
        eq.reset();
        assert_bit_identical(&mut eq, "eight bands at 0 dB");
    }

    #[test]
    fn returning_a_band_to_zero_returns_it_to_a_wire() {
        // The smoother snaps rather than approaching zero forever, which is
        // what lets the wire path re-engage after a move. Without the snap
        // this test hangs at "almost a wire" indefinitely.
        let mut eq = ParametricEq::new(FS);
        eq.set_band_gain_db(2, 6.0);
        let mut l = vec![0.0f32; 4096];
        let mut r = vec![0.0f32; 4096];
        eq.process(&mut l, &mut r);
        eq.set_band_gain_db(2, 0.0);
        for _ in 0..8 {
            eq.process(&mut l, &mut r);
        }
        assert!(
            eq.bands[2].cur.is_wire(),
            "band did not settle back to an exact wire"
        );
        assert_bit_identical(&mut eq, "band returned to 0 dB");
    }

    // =======================================================================
    // Denormals, divergence, and moving parameters
    // =======================================================================

    #[test]
    fn state_reaches_exactly_zero_after_silence() {
        // Brief §8.4 "Denormal". A 20 Hz Q 8 +12 dB bell has |pole| =
        // 0.999918 and would spend four seconds walking through the f32
        // subnormal range, or nine through the f64 one, after every gap in the
        // programme material. The block flush ends it: the output becomes
        // exactly 0.0 and stays there.
        let mut eq = bell(20.0, 12.0, 8.0, FS);
        let mut l = noise(FS as usize);
        let mut r = l.clone();
        eq.process(&mut l, &mut r);

        let mut silence_l = vec![0.0f32; 4096];
        let mut silence_r = vec![0.0f32; 4096];
        let mut blocks_to_silence = None;
        for block in 0..300 {
            silence_l.fill(0.0);
            silence_r.fill(0.0);
            eq.process(&mut silence_l, &mut silence_r);
            if silence_l.iter().all(|&x| x == 0.0) && silence_r.iter().all(|&x| x == 0.0) {
                blocks_to_silence = Some(block);
                break;
            }
        }
        let block = blocks_to_silence.expect("state never reached exactly zero");
        let seconds = f64::from(block + 1) * 4096.0 / FS;
        assert!(
            seconds < 30.0,
            "took {seconds:.1} s of silence to flush; the guard is meant to \
             catch it before the subnormal range costs anything"
        );
        // And the state itself, not just the output.
        for st in &eq.bands[0].state[0] {
            assert_eq!(st.y1, 0.0);
            assert_eq!(st.y2, 0.0);
        }
    }

    #[test]
    fn the_divergence_guard_catches_a_corrupted_state() {
        // Brief §3.4. One compare per band per block, and it converts a
        // theoretical blow-up into an inaudible 0.7 ms mute rather than a
        // full-scale scream in someone's headphones.
        for poison in [1e12, f64::NAN, f64::INFINITY] {
            let mut eq = bell(1000.0, 12.0, 2.0, FS);
            eq.bands[0].state[0][0].y1 = poison;
            eq.bands[0].state[1][0].y2 = poison;
            let mut l = vec![0.0f32; 256];
            let mut r = vec![0.0f32; 256];
            eq.process(&mut l, &mut r);
            assert!(
                l.iter().chain(r.iter()).all(|x| x.is_finite()),
                "state poisoned with {poison} produced a non-finite output"
            );
            assert!(
                l.iter().chain(r.iter()).all(|&x| x.abs() < 1e-6),
                "state poisoned with {poison} leaked into the output"
            );
        }
    }

    /// Peak magnitude of `samples` above 6 kHz, measured with a 24 dB/oct
    /// high-pass built from this same module.
    ///
    /// A *sideband* metric rather than a transient one, deliberately: the
    /// listening evidence is that perceived artifact severity correlates with
    /// sideband energy (r = −0.59) and is essentially uncorrelated with
    /// transient magnitude (r = 0.11). Optimising the smoothness of the
    /// waveform is optimising the wrong number.
    fn hf_peaks(samples: &[f32], fs: f64) -> Vec<f32> {
        let mut hp = single(BandType::HighPass, 6000.0, 0.0, BUTTERWORTH_Q, Slope::Db24, fs);
        let mut l = samples.to_vec();
        let mut r = vec![0.0f32; samples.len()];
        hp.process(&mut l, &mut r);
        l.iter().map(|x| x.abs()).collect()
    }

    fn peak(slice: &[f32]) -> f64 {
        f64::from(slice.iter().copied().fold(0.0f32, f32::max))
    }

    /// Render a 1 kHz probe through `eq` while `move_param` walks a parameter
    /// linearly over 100 ms, and return the click energy in dB above the
    /// steady-state floor.
    fn click_db(mut eq: ParametricEq, mut move_param: impl FnMut(&mut ParametricEq, f64)) -> f64 {
        const SETTLE: usize = 22_050;
        const MOVE: usize = 4_410; // 100 ms
        const TAIL: usize = 13_230;
        let w = 2.0 * PI * 1000.0 / FS;
        let total = SETTLE + MOVE + TAIL;
        let mut out = Vec::with_capacity(total);
        let mut l = [0.0f32; 32];
        let mut r = [0.0f32; 32];
        let mut n = 0;
        while n < total {
            for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                let s = (w * (n + i) as f64).sin() as f32 * 0.5;
                *a = s;
                *b = s;
            }
            if (SETTLE..SETTLE + MOVE).contains(&n) {
                move_param(&mut eq, (n - SETTLE) as f64 / MOVE as f64);
            } else if n >= SETTLE + MOVE {
                move_param(&mut eq, 1.0);
            }
            eq.process(&mut l, &mut r);
            out.extend_from_slice(&l);
            n += 32;
        }
        let hf = hf_peaks(&out, FS);
        let floor = peak(&hf[SETTLE / 2..SETTLE]);
        let during = peak(&hf[SETTLE..]);
        20.0 * (during / floor).log10()
    }

    #[test]
    fn a_frequency_sweep_does_not_click() {
        // Brief §8.4 "Zipper", the worst case in the whole matrix: f₀ 500 →
        // 5000 Hz on a 1 kHz probe with a +12 dB Q 2 bell. Measured 12.7 dB
        // above the floor with this scheme and 48.0 dB with the parameters
        // slammed straight into the coefficient formula.
        let eq = bell(500.0, 12.0, 2.0, FS);
        let click = click_db(eq, |eq, t| {
            eq.set_band_freq_hz(0, 500.0 * (5000.0f64 / 500.0).powf(t));
        });
        // Measured here: 10.9 dB. The brief's threshold is 20; without the
        // smoother the same move measures 48.
        assert!(
            click < 20.0,
            "frequency sweep injected {click:.1} dB of high-frequency energy \
             above the steady-state floor"
        );
    }

    #[test]
    fn a_gain_sweep_does_not_click() {
        // Gain +18 → −18 at 1 kHz Q 2, the move a user makes when they grab a
        // band and drag it through zero. Measured 0.7 dB smoothed against
        // 33.1 dB slammed.
        let mut eq = bell(1000.0, 18.0, 2.0, FS);
        eq.reset();
        let click = click_db(eq, |eq, t| {
            eq.set_band_gain_db(0, 18.0f64.mul_add(-2.0 * t, 18.0));
        });
        // Measured here: 0.0 dB. Slammed, the same move measures 33.1.
        assert!(click < 6.0, "gain sweep injected {click:.1} dB");
    }

    #[test]
    fn a_q_jump_does_not_click() {
        // Q 0.3 → 20 in one step, which is what a preset recall looks like.
        // The smoother is what turns it into a ramp; measured 3.1 dB.
        let eq = bell(1000.0, 12.0, 0.3, FS);
        let click = click_db(eq, |eq, t| {
            if t > 0.0 {
                eq.set_band_q(0, 20.0);
            }
        });
        // Measured here: 0.0 dB. Slammed, the same jump measures 15.0.
        assert!(click < 6.0, "Q jump injected {click:.1} dB");
    }

    #[test]
    fn toggling_a_band_does_not_click() {
        // Discrete parameters cannot be smoothed, so they crossfade over 64
        // samples instead. Without the crossfade this is a hard edge.
        let mut eq = ParametricEq::new(FS);
        eq.set_band_type(0, BandType::HighShelf);
        eq.set_band_gain_db(0, 12.0);
        eq.set_band_freq_hz(0, 4000.0);
        eq.set_band_enabled(0, false);
        eq.reset();
        let click = click_db(eq, |eq, t| {
            eq.set_band_enabled(0, t > 0.0);
        });
        // Measured here: 4.8 dB, against 17.8 with a linear crossfade.
        assert!(click < 10.0, "enabling a band injected {click:.1} dB");
    }

    #[test]
    fn changing_a_band_type_does_not_click() {
        let mut eq = bell(1200.0, 12.0, 1.0, FS);
        eq.reset();
        let click = click_db(eq, |eq, t| {
            if t > 0.0 {
                eq.set_band_type(0, BandType::HighShelf);
            }
        });
        // Measured here: 1.5 dB, against 12.3 with a linear crossfade.
        assert!(click < 10.0, "changing band type injected {click:.1} dB");
    }

    #[test]
    fn periodic_parameter_modulation_does_not_destabilise_the_filter() {
        // Brief §8.4 "Modulation stability", and the test that would catch
        // someone shortening or removing the smoother.
        //
        // Staying inside the biquad stability triangle proves nothing about a
        // time-varying filter: direct forms are known to diverge for about
        // 4.5% of modulation periods in the 2–400 sample range when
        // coefficients are interpolated between verified-stable endpoints.
        // That failure needs *periodic* modulation at a rate comparable to
        // the filter's own dynamics, and a one-pole parameter smoother makes
        // it unreachable — even an adversarial square wave becomes a slow
        // monotone approach.
        let static_peak = {
            let mut eq = single(BandType::Bell, 500.0, 12.0, 20.0, Slope::Db12, FS);
            impulse_peak(&mut eq, |_, _| {})
        };
        assert!(static_peak > 0.0);

        for period in [8usize, 16, 32, 64, 96, 128, 192, 256, 512, 1024, 2048, 4096] {
            let mut eq = single(BandType::Bell, 500.0, 12.0, 20.0, Slope::Db12, FS);
            let peak = impulse_peak(&mut eq, |eq, n| {
                let hi = (n / period) % 2 == 1;
                eq.set_band_freq_hz(0, if hi { 2000.0 } else { 500.0 });
            });
            assert!(
                peak <= 2.0 * static_peak,
                "square-waving f₀ at a {period}-sample period grew the response to \
                 {peak:.4} against a static peak of {static_peak:.4}"
            );
        }
    }

    /// Feed an impulse then silence, modulating parameters every chunk, and
    /// return the largest magnitude reached by the output or by any state
    /// word.
    fn impulse_peak(eq: &mut ParametricEq, mut modulate: impl FnMut(&mut ParametricEq, usize)) -> f64 {
        const CHUNK: usize = 8;
        let mut worst = 0.0f64;
        let mut l = [0.0f32; CHUNK];
        let mut r = [0.0f32; CHUNK];
        for block in 0..(65_536 / CHUNK) {
            l.fill(0.0);
            r.fill(0.0);
            if block == 0 {
                l[0] = 1.0;
                r[0] = 1.0;
            }
            modulate(eq, block * CHUNK);
            eq.process(&mut l, &mut r);
            for x in l.iter().chain(r.iter()) {
                worst = worst.max(f64::from(x.abs()));
            }
            for ch in &eq.bands[0].state {
                for st in ch {
                    worst = worst
                        .max(st.y1.abs())
                        .max(st.y2.abs())
                        .max(st.x1.abs())
                        .max(st.x2.abs());
                }
            }
            assert!(worst.is_finite(), "state went non-finite");
        }
        worst
    }

    #[test]
    fn a_hostile_gain_and_q_modulation_stays_bounded() {
        // The same argument applied to the two controls the listening
        // evidence says are the dangerous ones for a peaking filter.
        let mut eq = single(BandType::Bell, 1000.0, 0.0, 20.0, Slope::Db12, FS);
        let peak = impulse_peak(&mut eq, |eq, n| {
            let phase = (n / 24) % 2 == 1;
            eq.set_band_gain_db(0, if phase { 18.0 } else { -18.0 });
            eq.set_band_q(0, if phase { 40.0 } else { 0.1 });
        });
        assert!(
            peak < 100.0,
            "gain and Q square-waved together reached {peak:.3}"
        );
    }

    // =======================================================================
    // Real-time discipline
    // =======================================================================

    #[test]
    fn process_allocates_nothing() {
        // Rule one of the audio thread. Every buffer this module needs is
        // either a field or a fixed-size array on the stack, and the control
        // block is walked with slice indices rather than a scratch `Vec`.
        let mut eq = ParametricEq::new(FS);
        let mut l = vec![0.0f32; 1024];
        let mut r = vec![0.0f32; 1024];
        for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            let s = (i as f64 * 0.01).sin() as f32 * 0.25;
            *a = s;
            *b = -s;
        }
        eq.process(&mut l, &mut r);

        // Every path: a ramp, a discrete crossfade, an enable, the trim, a
        // wire band, a disabled band, a two-section band, the curve, and the
        // single-sample entry point.
        let allocations = crate::synth::tests::allocations_during(|| {
            eq.set_band_gain_db(3, 9.0);
            eq.set_band_type(4, BandType::Notch);
            eq.set_band_slope(7, Slope::Db24);
            eq.set_band_enabled(7, true);
            eq.set_output_trim_db(-3.0);
            eq.set_param(BandParam::Freq.index(5), 0.6);
            for _ in 0..64 {
                eq.process(&mut l, &mut r);
            }
            let _ = eq.process_sample(0.25, -0.25);
            let _ = eq.response_db(1000.0);
            let _ = eq.band_response_db(2, 1000.0);
        });

        assert_eq!(allocations, 0, "the audio path allocated {allocations} times");
    }

    #[test]
    fn cpu_cost_is_bounded() {
        // Brief §8.4 "CPU": a smoke test for accidental quadratic behaviour,
        // not a benchmark. Eight bands, stereo, 48 kHz, two seconds of audio,
        // with a parameter moving so the interpolated path runs rather than
        // the static one.
        let mut eq = ParametricEq::new(48_000.0);
        for b in 0..BAND_COUNT {
            eq.set_band_enabled(b, true);
            if eq.band_type(b).uses_gain() {
                eq.set_band_gain_db(b, 6.0);
            }
        }
        eq.set_band_slope(0, Slope::Db24);
        eq.set_band_slope(7, Slope::Db24);
        eq.reset();

        let dry: Vec<f32> = (0..512)
            .map(|i| (f64::from(i) * 0.05).sin() as f32 * 0.25)
            .collect();
        let mut l = dry.clone();
        let mut r = dry.clone();
        let blocks = 2 * 48_000 / 512;
        let start = std::time::Instant::now();
        for i in 0..blocks {
            // Refill rather than re-filtering the previous output, which would
            // measure an exponential rather than the EQ.
            l.copy_from_slice(&dry);
            r.copy_from_slice(&dry);
            eq.set_band_gain_db(4, f64::from(i % 12) - 6.0);
            eq.process(&mut l, &mut r);
        }
        let elapsed = start.elapsed().as_secs_f64();
        assert!(
            elapsed < 20.0,
            "two seconds of eight-band stereo audio took {elapsed:.2} s; \
             something is quadratic"
        );
        assert!(l.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn process_sample_agrees_with_the_buffer_path() {
        let mut a = bell(1500.0, -7.0, 3.0, FS);
        let mut b = a.clone();
        let dry = noise(2000);
        let mut l: Vec<f32> = dry.clone();
        let mut r: Vec<f32> = dry.clone();
        a.process(&mut l, &mut r);
        for (i, x) in dry.iter().enumerate() {
            let (yl, _) = b.process_sample(*x, *x);
            assert_eq!(
                yl.to_bits(),
                l[i].to_bits(),
                "sample {i}: per-sample entry point diverged from the buffer one"
            );
        }
    }

    // =======================================================================
    // Parameter surface
    // =======================================================================

    #[test]
    fn parameter_laws_hit_their_anchors() {
        // Brief §2.2.
        close(norm_from_freq_hz(20.0), 0.086_574_489, 1e-8, "20 Hz");
        close(norm_from_freq_hz(1000.0), 0.575_188_454, 1e-8, "1 kHz");
        close(norm_from_freq_hz(20000.0), 0.949_357_170, 1e-8, "20 kHz");
        assert_eq!(norm_from_freq_hz(30000.0), 1.0);
        assert_eq!(norm_from_freq_hz(10.0), 0.0);

        assert_eq!(gain_db_from_norm(0.5), 0.0, "p = 0.5 must be exactly 0 dB");
        assert_eq!(gain_db_from_norm(1.0), 18.0);
        assert_eq!(gain_db_from_norm(0.0), -18.0);
        close(gain_db_from_norm(0.5 + 1.0 / 360.0), 0.1, 1e-12, "one 1/360 step");

        close(norm_from_q(BUTTERWORTH_Q), 0.326_466_340, 1e-8, "Q 0.707");
        assert_eq!(norm_from_q(2.0), 0.5, "Q 2 must be exactly p = 0.5");
        assert_eq!(norm_from_q(40.0), 1.0);
        assert_eq!(trim_db_from_norm(0.5), 0.0);

        for p in [0.0, 0.137, 0.5, 0.8123, 1.0] {
            close(norm_from_freq_hz(freq_hz_from_norm(p)), p, 1e-12, "freq round trip");
            close(norm_from_q(q_from_norm(p)), p, 1e-12, "Q round trip");
            close(norm_from_gain_db(gain_db_from_norm(p)), p, 1e-12, "gain round trip");
            close(norm_from_trim_db(trim_db_from_norm(p)), p, 1e-12, "trim round trip");
        }
    }

    #[test]
    fn q_and_octaves_convert_both_ways() {
        // Brief §7.7. The octave number is the one that means something to a
        // person; the conversion is one `asinh`.
        for (q, octaves) in [
            (0.1, 6.672_287),
            (0.5, 2.543_107),
            (BUTTERWORTH_Q, 1.899_969),
            (1.0, 1.388_484),
            (std::f64::consts::SQRT_2, 1.000_000),
            (2.0, 0.714_037),
            (4.0, 0.359_741),
            (10.0, 0.144_209),
            (40.0, 0.036_066),
        ] {
            close(q_to_octaves(q), octaves, 1e-5, "Q to octaves");
            // The anchors carry six decimals, so the reverse direction can
            // only be checked to the precision the anchor has; the round trip
            // below is the exact statement.
            close(octaves_to_q(octaves), q, 1e-4, "octaves to Q");
            close(octaves_to_q(q_to_octaves(q)), q, 1e-12, "Q round trip");
        }
    }

    #[test]
    fn the_iso_grid_is_a_sixth_octave_apart() {
        let g = ISO_SIXTH_OCTAVE_HZ;
        assert_eq!(g[0], FREQ_MIN_HZ);
        assert_eq!(g[g.len() - 1], FREQ_MAX_HZ);
        for w in g.windows(2) {
            assert!(w[1] > w[0], "grid must ascend: {w:?}");
        }
        // Every step but the last, which is clamped to the top of the range,
        // is one sixth of an octave to within the rounding the R20 series
        // carries.
        for w in g[..g.len() - 1].windows(2) {
            let ratio = (w[1] / w[0]).log2() * 6.0;
            // R20 is rounded to two or three significant figures, so its
            // steps run from 0.912 of a sixth-octave (18 → 20) to 1.156
            // (14 → 16). These are the numbers people read; the exactness
            // lives in the parameter law, not in the grid.
            assert!(
                (ratio - 1.0).abs() < 0.16,
                "{:?} to {:?} is {ratio:.3} sixths of an octave",
                w[0],
                w[1]
            );
        }
        assert_eq!(iso_step_up(1000.0), 1120.0);
        assert_eq!(iso_step_down(1000.0), 900.0);
        // Starting off-grid steps onto the grid, never backwards.
        assert_eq!(iso_step_up(300.0), 315.0);
        assert_eq!(iso_step_down(300.0), 280.0);
        assert_eq!(iso_snap(300.0), 315.0);
        assert_eq!(iso_snap(2487.0), 2500.0);
        assert_eq!(iso_step_up(FREQ_MAX_HZ), FREQ_MAX_HZ);
        assert_eq!(iso_step_down(FREQ_MIN_HZ), FREQ_MIN_HZ);
    }

    #[test]
    fn the_factory_layout_is_the_one_in_the_brief() {
        let eq = ParametricEq::new(FS);
        let want = [
            (BandType::HighPass, 30.0, false),
            (BandType::LowShelf, 120.0, true),
            (BandType::Bell, 300.0, true),
            (BandType::Bell, 800.0, true),
            (BandType::Bell, 2500.0, true),
            (BandType::Bell, 6000.0, true),
            (BandType::HighShelf, 10000.0, true),
            (BandType::LowPass, 18000.0, false),
        ];
        for (b, (ty, hz, on)) in want.into_iter().enumerate() {
            assert_eq!(eq.band_type(b), ty, "band {b} type");
            close(eq.band_freq_hz(b), hz, 1e-9, &format!("band {b} freq"));
            assert_eq!(eq.band_enabled(b), on, "band {b} enable");
            assert_eq!(eq.band_gain_db(b), 0.0, "band {b} gain");
        }
        assert_eq!(eq.output_trim_db(), 0.0);
        assert_eq!(eq.latency_samples(), 0);
        assert_eq!(eq.sample_rate(), FS);
    }

    #[test]
    fn the_flat_parameter_space_round_trips() {
        // Brief §8.4 "Session round-trip". The flat space has to be a lossless
        // description of the instance, because it is what a preset and a
        // saved session will store.
        assert_eq!(PARAM_COUNT, BAND_COUNT * PARAMS_PER_BAND + 1);
        assert_eq!(param_address(PARAM_OUTPUT_TRIM), None);
        assert_eq!(param_address(0), Some((0, BandParam::Type)));
        assert_eq!(param_address(13), Some((2, BandParam::Freq)));
        assert_eq!(param_name(PARAM_OUTPUT_TRIM), "trim");

        let mut source = ParametricEq::new(FS);
        let mut v = 0.03f32;
        for i in 0..PARAM_COUNT {
            source.set_param(i, v);
            v = (v + 0.137).fract();
        }
        source.reset();

        let saved: Vec<f32> = (0..PARAM_COUNT).map(|i| source.param(i)).collect();
        let mut restored = ParametricEq::new(FS);
        for (i, &value) in saved.iter().enumerate() {
            restored.set_param(i, value);
        }
        restored.reset();

        for b in 0..BAND_COUNT {
            assert_eq!(restored.band_type(b), source.band_type(b), "band {b} type");
            assert_eq!(restored.band_slope(b), source.band_slope(b), "band {b} slope");
            assert_eq!(
                restored.band_enabled(b),
                source.band_enabled(b),
                "band {b} enable"
            );
            let (a, c) = (source.bands[b].cur, restored.bands[b].cur);
            assert_eq!(a.n, c.n, "band {b} section count");
            for s in 0..a.n {
                close(c.c[s].b0, a.c[s].b0, 1e-12, &format!("band {b} b0"));
                close(c.c[s].b1, a.c[s].b1, 1e-12, &format!("band {b} b1"));
                close(c.c[s].b2, a.c[s].b2, 1e-12, &format!("band {b} b2"));
                close(c.c[s].a1, a.c[s].a1, 1e-12, &format!("band {b} a1"));
                close(c.c[s].a2, a.c[s].a2, 1e-12, &format!("band {b} a2"));
            }
        }
        for f in [50.0, 500.0, 5000.0, 15000.0] {
            close(restored.response_db(f), source.response_db(f), 1e-12, "restored curve");
        }
    }

    #[test]
    fn out_of_range_indices_are_ignored() {
        let mut eq = ParametricEq::new(FS);
        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| eq.param(i)).collect();
        eq.set_param(PARAM_COUNT, 1.0);
        eq.set_param(usize::MAX, 1.0);
        assert_eq!(eq.param(PARAM_COUNT), 0.0);
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| eq.param(i)).collect();
        assert_eq!(before, after);
        assert_eq!(eq.band_type(BAND_COUNT), BandType::Bell);
        assert!(!eq.band_enabled(BAND_COUNT));
    }

    #[test]
    fn slope_choices_are_per_type() {
        assert_eq!(Slope::choices_for(BandType::Bell), &[]);
        assert_eq!(
            Slope::choices_for(BandType::HighShelf),
            &[Slope::Db6, Slope::Db12]
        );
        assert_eq!(
            Slope::choices_for(BandType::LowPass),
            &[Slope::Db12, Slope::Db24]
        );
        // A slope the type does not offer falls back rather than being taken
        // literally: a 24 dB/oct shelf is not a thing this module builds.
        let mut eq = ParametricEq::new(FS);
        eq.set_band_type(0, BandType::HighShelf);
        eq.set_band_slope(0, Slope::Db24);
        assert_eq!(eq.band_slope(0), Slope::Db12);
        // And changing type re-resolves the slope it is carrying.
        eq.set_band_type(0, BandType::LowPass);
        eq.set_band_slope(0, Slope::Db24);
        assert_eq!(eq.band_slope(0), Slope::Db24);
        eq.set_band_type(0, BandType::LowShelf);
        assert_eq!(eq.band_slope(0), Slope::Db12);
        assert_eq!(Slope::Db24.db_per_octave(), 24);
    }

    #[test]
    fn a_notch_is_a_true_null() {
        // The reason the notch is the one cookbook filter here: an exact zero
        // on the unit circle. Deriving one from `1 − matched bandpass` gives
        // −7.6 dB at 8 kHz instead of −∞.
        //
        // The depth reported here is bounded by the closed form rather than by
        // the filter: `|H|²` assembles the numerator from `(b0+b1+b2)²`, which
        // for a notch is `O(ω₀⁴)` built out of terms of order 1, so the drawn
        // curve bottoms out around −95 dB at 60 Hz where the filter itself
        // nulls to −224. That is a display limit on a value no one can hear,
        // and it costs nothing to leave: every band type that is *not* a notch
        // agrees with direct complex evaluation to 1e-6 dB.
        for f0 in [60.0, 1000.0, 8000.0, 15000.0] {
            let eq = single(BandType::Notch, f0, 0.0, 4.0, Slope::Db12, FS);
            assert!(
                eq.response_db(f0) < -60.0,
                "notch at {f0} Hz is only {:.1} dB deep",
                eq.response_db(f0)
            );
            // Two octaves below the null, a Q 4 notch is back to flat. Below
            // rather than above, because the top case is at 15 kHz and four
            // times that is past Nyquist.
            assert!(eq.response_db(f0 / 4.0).abs() < 1.0);
        }
    }

    #[test]
    fn an_all_pass_has_unity_magnitude_everywhere() {
        let eq = single(BandType::AllPass, 800.0, 0.0, 1.5, Slope::Db12, FS);
        for f in [20.0, 200.0, 800.0, 3000.0, 20000.0] {
            close(eq.response_db(f), 0.0, 1e-9, "all-pass magnitude");
        }
    }

    #[test]
    fn a_disabled_band_is_absent_from_the_curve() {
        let mut eq = single(BandType::Bell, 1000.0, 12.0, 1.0, Slope::Db12, FS);
        close(eq.response_db(1000.0), 12.0, 1e-9, "enabled");
        assert!(eq.band_response_db(0, 1000.0) > 11.9);
        eq.set_band_enabled(0, false);
        eq.reset();
        assert_eq!(eq.response_db(1000.0), 0.0);
        assert_eq!(eq.band_response_db(0, 1000.0), 0.0);
    }

    #[test]
    fn changing_the_sample_rate_redesigns_everything() {
        let mut eq = single(BandType::Bell, 5000.0, 9.0, 2.0, Slope::Db12, FS);
        let before = eq.response_db(5000.0);
        eq.set_sample_rate(96000.0);
        assert_eq!(eq.sample_rate(), 96000.0);
        close(eq.response_db(5000.0), before, 0.02, "peak gain survives a rate change");
        close(eq.response_db(5000.0), 9.0, 1e-6, "peak gain is still what was asked for");
        // Nonsense rates are refused rather than propagated into a division.
        eq.set_sample_rate(0.0);
        eq.set_sample_rate(f64::NAN);
        assert_eq!(eq.sample_rate(), 96000.0);
    }

    #[test]
    fn frequencies_above_nyquist_are_clamped_for_the_types_that_need_it() {
        // Shelves are designed to work with a corner above Nyquist; the
        // impulse-invariant pole map is not, so everything else clamps at
        // 0.49·f_s.
        for &ty in &[
            BandType::Bell,
            BandType::HighPass,
            BandType::LowPass,
            BandType::Notch,
            BandType::BandPass,
            BandType::AllPass,
        ] {
            let s = design(ty, 30000.0, 6.0, 1.0, Slope::Db12, FS);
            let clamped = design(ty, 0.49 * FS, 6.0, 1.0, Slope::Db12, FS);
            assert!(
                s.c[0] == clamped.c[0],
                "{ty:?} above Nyquist should design as if at 0.49·f_s"
            );
        }
        // A 24 kHz high shelf at 44.1 kHz is a real air move and is designed
        // as asked.
        let above = single(BandType::HighShelf, 24000.0, 12.0, BUTTERWORTH_Q, Slope::Db12, FS);
        assert!(above.response_db(20000.0) > 4.0);
        assert!(above.response_db(20000.0) < 12.0);
    }

    #[test]
    fn a_band_switched_off_stops_costing_anything() {
        // The crossfade has to actually finish: after it, the band is out of
        // the chain, its state is zero so it generates no denormals, and the
        // instance is a bit-exact wire again.
        let mut eq = single(BandType::Bell, 1000.0, 12.0, 1.0, Slope::Db12, FS);
        close(eq.response_db(1000.0), 12.0, 1e-9, "band on");

        eq.set_band_enabled(0, false);
        let mut l = vec![0.5f32; 256];
        let mut r = vec![0.5f32; 256];
        eq.process(&mut l, &mut r);

        assert!(!eq.bands[0].active, "crossfade never completed");
        assert_eq!(eq.response_db(1000.0), 0.0);
        for st in eq.bands[0].state.iter().flatten() {
            assert_eq!(st.y1, 0.0);
            assert_eq!(st.x1, 0.0);
        }
        assert_bit_identical(&mut eq, "band switched off");
    }

    #[test]
    fn a_band_switched_on_reaches_its_designed_response() {
        let mut eq = ParametricEq::new(FS);
        eq.set_band_type(0, BandType::Bell);
        eq.set_band_freq_hz(0, 1000.0);
        eq.set_band_gain_db(0, 12.0);
        eq.set_band_enabled(0, false);
        eq.reset();
        assert_eq!(eq.response_db(1000.0), 0.0);

        eq.set_band_enabled(0, true);
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        eq.process(&mut l, &mut r);
        assert_eq!(eq.bands[0].fade, Fade::None, "crossfade never completed");
        close(eq.response_db(1000.0), 12.0, 1e-9, "band switched on");
        let measured = rendered_db(&mut eq, 1000.0, FS);
        assert!((measured - 12.0).abs() < 0.02, "rendered {measured:+.4} dB");
    }

    #[test]
    fn a_crossfade_that_ends_mid_chunk_is_handled() {
        // The fade is 64 samples and buffers are whatever the host hands over,
        // so the completion lands in the middle of a chunk more often than
        // not. Feed prime-length chunks and check nothing is dropped.
        let mut eq = single(BandType::HighShelf, 5000.0, 9.0, BUTTERWORTH_Q, Slope::Db12, FS);
        let dry = noise(4096);
        let mut l = dry.clone();
        let mut r = dry.clone();
        eq.set_band_enabled(0, false);
        for (cl, cr) in l.chunks_mut(37).zip(r.chunks_mut(37)) {
            eq.process(cl, cr);
        }
        assert!(!eq.bands[0].active);
        // Everything after the 64-sample fade is the dry signal, bit for bit.
        for i in 200..dry.len() {
            assert_eq!(
                l[i].to_bits(),
                dry[i].to_bits(),
                "sample {i} after the fade is not the dry signal"
            );
        }
        assert!(l.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn switching_slope_grows_and_drops_a_section_cleanly() {
        let mut eq = single(BandType::LowPass, 4000.0, 0.0, BUTTERWORTH_Q, Slope::Db24, FS);
        assert_eq!(eq.bands[0].cur.n, 2);
        let mut l = noise(1024);
        let mut r = l.clone();
        eq.process(&mut l, &mut r);

        eq.set_band_slope(0, Slope::Db12);
        eq.process(&mut l.clone(), &mut r.clone());
        assert_eq!(eq.bands[0].cur.n, 1);

        // Back to 24, where the second section must start from zero rather
        // than from whatever it held two switches ago.
        eq.set_band_slope(0, Slope::Db24);
        assert_eq!(eq.bands[0].cur.n, 2);
        assert_eq!(eq.bands[0].state[0][1].y1, 0.0);
        assert_eq!(eq.bands[0].state[1][1].x1, 0.0);

        let mut probe = eq.clone();
        probe.reset();
        let measured = rendered_db(&mut probe, 4000.0, FS);
        close(measured, probe.response_db(4000.0), 0.02, "24 dB/oct corner");
        // −3.01 dB, not −6.02: the section Qs make this one fourth-order
        // Butterworth rather than two stacked second-order ones, so the corner
        // is still the half-power point.
        close(measured, -3.0103, 0.05, "fourth-order Butterworth at the corner");
        let octave_up = probe.response_db(8000.0);
        assert!(
            (-26.0..-22.0).contains(&octave_up),
            "an octave above the corner should be near -24 dB, got {octave_up:.2}"
        );
    }

    // =======================================================================
    // L8 — the natural-unit surface, which is what a session stores.
    // =======================================================================

    /// A vector with every control of every band moved somewhere different,
    /// in natural units. Types cycle so that all eight are exercised, and the
    /// slope asked for is legal for some types and not for others.
    fn moved_natural_params() -> [f32; PARAM_COUNT] {
        let mut params = default_natural_params();
        for band in 0..BAND_COUNT {
            params[BandParam::Type.index(band)] = band as f32;
            params[BandParam::Freq.index(band)] = 100.0 * (band + 1) as f32;
            params[BandParam::Gain.index(band)] = (band as f32 - 3.5) * 4.0;
            params[BandParam::Q.index(band)] = 0.5 + band as f32;
            params[BandParam::Slope.index(band)] = if band % 2 == 0 { 6.0 } else { 24.0 };
            params[BandParam::Enabled.index(band)] = f32::from(u8::from(band % 3 != 0));
        }
        params[PARAM_OUTPUT_TRIM] = -3.5;
        params
    }

    /// Every control written in its own unit comes back in its own unit.
    ///
    /// This is the property a session rests on: what is saved is what the
    /// knob said, so a law that changes later cannot re-point a stored file.
    #[test]
    fn the_natural_surface_round_trips_every_control() {
        let mut eq = ParametricEq::new(FS);
        let params = moved_natural_params();
        for (index, &value) in params.iter().enumerate() {
            eq.set_param_natural(index, value);
        }
        eq.reset();

        for band in 0..BAND_COUNT {
            let read = |p: BandParam| eq.param_natural(p.index(band));
            assert_eq!(read(BandParam::Type), band as f32, "band {band} type");
            close(
                f64::from(read(BandParam::Freq)),
                f64::from(params[BandParam::Freq.index(band)]),
                1e-6,
                &format!("band {band} freq"),
            );
            close(
                f64::from(read(BandParam::Gain)),
                f64::from(params[BandParam::Gain.index(band)]),
                1e-6,
                &format!("band {band} gain"),
            );
            close(
                f64::from(read(BandParam::Q)),
                f64::from(params[BandParam::Q.index(band)]),
                1e-6,
                &format!("band {band} q"),
            );
            assert_eq!(
                read(BandParam::Enabled),
                params[BandParam::Enabled.index(band)],
                "band {band} enable"
            );
            // The typed view and the flat one are the same numbers. Same to
            // within the last bits of an `f32`: the flat surface is `f32`
            // because that is what a session stores, and the typed one is the
            // `f64` the filter is designed in.
            close(
                f64::from(read(BandParam::Freq)),
                eq.band_freq_hz(band),
                1e-6,
                &format!("band {band} freq, both views"),
            );
            close(
                f64::from(read(BandParam::Gain)),
                eq.band_gain_db(band),
                1e-6,
                &format!("band {band} gain, both views"),
            );
            assert_eq!(read(BandParam::Type), eq.band_type(band).index() as f32);
        }
        close(
            f64::from(eq.param_natural(PARAM_OUTPUT_TRIM)),
            -3.5,
            1e-6,
            "output trim",
        );

        // ...and a second instance driven from what the first reported is the
        // same filter, which is the save-and-load path itself.
        let saved: Vec<f32> = (0..PARAM_COUNT).map(|i| eq.param_natural(i)).collect();
        let restored = eq_from_natural_params(&saved, FS);
        for f in [50.0, 500.0, 5000.0, 15000.0] {
            close(restored.response_db(f), eq.response_db(f), 1e-12, "restored curve");
        }
    }

    /// The factory table and a fresh instance are the same settings. A copy
    /// of the defaults that drifts from the ones the EQ actually builds is
    /// the whole reason this is read from one place.
    #[test]
    fn the_factory_defaults_are_what_a_fresh_instance_reads() {
        let eq = ParametricEq::new(FS);
        let defaults = default_natural_params();
        for (index, &default) in defaults.iter().enumerate() {
            let info = natural_param(index).expect("every index in range is a control");
            assert_eq!(default, info.default, "index {index}");
            close(
                f64::from(eq.param_natural(index)),
                f64::from(info.default),
                1e-6,
                &format!("index {index} ({})", info.name),
            );
            assert!(info.min <= info.default && info.default <= info.max, "index {index}");
        }
        assert_eq!(natural_param(PARAM_COUNT), None);
        assert_eq!(natural_param(usize::MAX), None);
        assert_eq!(natural_param(PARAM_OUTPUT_TRIM).unwrap().unit, "dB");
        assert_eq!(natural_param(BandParam::Freq.index(2)).unwrap().unit, "Hz");
        assert_eq!(natural_param(BandParam::Slope.index(0)).unwrap().unit, "dB/oct");
    }

    /// A slope is asked for in dB per octave and lands on one the band's type
    /// actually offers — the nearest, rather than a silent no-op or a
    /// fallback to the default.
    #[test]
    fn a_slope_lands_on_one_its_type_offers() {
        let mut eq = ParametricEq::new(FS);
        let slope = BandParam::Slope.index(0);

        eq.set_param_natural(BandParam::Type.index(0), BandType::HighPass.index() as f32);
        for (asked, want) in [(6.0, 12.0), (12.0, 12.0), (24.0, 24.0), (48.0, 24.0)] {
            eq.set_param_natural(slope, asked);
            assert_eq!(eq.param_natural(slope), want, "{asked} dB/oct on a high-pass");
        }

        eq.set_param_natural(BandParam::Type.index(0), BandType::LowShelf.index() as f32);
        // Changing type re-resolves what the band was carrying: 24 is not a
        // shelf slope, so the shelf comes up at 12.
        assert_eq!(eq.param_natural(slope), 12.0);
        for (asked, want) in [(6.0, 6.0), (24.0, 12.0)] {
            eq.set_param_natural(slope, asked);
            assert_eq!(eq.param_natural(slope), want, "{asked} dB/oct on a shelf");
        }

        // A type with no slope choice keeps the one it has rather than
        // pretending to accept a new one.
        eq.set_param_natural(BandParam::Type.index(0), BandType::Bell.index() as f32);
        eq.set_param_natural(slope, 24.0);
        assert_eq!(eq.param_natural(slope), 12.0, "a bell has no slope to set");
        assert_eq!(nearest_slope(BandType::Bell, 24.0), None);
    }

    /// The unit on the label is the unit at the filter: +12 dB asked for in
    /// natural units is +12 dB of measured level at the frequency asked for,
    /// and a decade below it nothing moved.
    #[test]
    fn natural_units_reach_the_filter_as_hertz_and_decibels() {
        let mut eq = ParametricEq::new(FS);
        eq.set_param_natural(BandParam::Gain.index(4), 12.0);
        eq.reset();
        close(eq.band_freq_hz(4), 2500.0, 1e-9, "band 5 is the 2.5 kHz bell");

        let boosted = rendered_db(&mut eq.clone(), 2500.0, FS);
        assert!(
            (boosted - 12.0).abs() < 0.05,
            "asked for +12 dB at 2.5 kHz, rendered {boosted:+.4} dB"
        );
        // A decade below, the control band. Not "zero": a Q 1 bell has a
        // skirt, and 3.3 octaves away it is still worth +0.162 dB. The
        // assertion is against the analog prototype rather than against a
        // tolerance someone picked, so a band that *did* leak — one whose
        // frequency landed somewhere else, say — could not pass by being
        // within a number chosen to let the skirt through.
        let control = rendered_db(&mut eq.clone(), 250.0, FS);
        close(
            control,
            analog_bell_db(250.0, 2500.0, 12.0, 1.0),
            0.02,
            "a decade below the band",
        );
        assert!(control.abs() < 0.25, "the boost is not local: {control:+.4} dB at 250 Hz");
        close(eq.response_db(2500.0), boosted, 0.01, "curve against render");
    }

    /// The mirror a UI draws from is the filter the audio thread is running:
    /// same parameters in, same curve out, and the curve is what the render
    /// measures.
    #[test]
    fn the_drawn_mirror_is_the_rendered_filter() {
        let params = moved_natural_params();
        let mirror = eq_from_natural_params(&params, FS);
        let mut running = eq_from_natural_params(&params, FS);
        for f in [80.0, 400.0, 1200.0, 6000.0] {
            let measured = rendered_db(&mut running, f, FS);
            close(
                measured,
                eq_response_db(&params, FS, f),
                0.05,
                &format!("mirror against render at {f} Hz"),
            );
            close(mirror.response_db(f), eq_response_db(&params, FS, f), 1e-12, "one-shot");
        }
        // A vector shorter than the space leaves the rest at the factory
        // settings rather than at zero, which is what makes the format
        // additive.
        let short = eq_from_natural_params(&params[..8], FS);
        close(short.band_freq_hz(7), 18000.0, 1e-9, "band 8 kept its factory corner");
        close(
            short.band_freq_hz(1),
            f64::from(params[BandParam::Freq.index(1)]),
            1e-9,
            "band 2 took the value the short vector carried",
        );
    }

    /// The audio thread's EQ takes whatever a UI sends it: an index past the
    /// end, a NaN, a frequency of a million. None of them move a control they
    /// were not addressed to.
    #[test]
    fn nonsense_natural_input_moves_nothing() {
        let mut eq = ParametricEq::new(FS);
        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| eq.param_natural(i)).collect();

        eq.set_param_natural(PARAM_COUNT, 1.0);
        eq.set_param_natural(usize::MAX, 1.0);
        for index in 0..PARAM_COUNT {
            eq.set_param_natural(index, f32::NAN);
        }
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| eq.param_natural(i)).collect();
        assert_eq!(before, after, "a NaN or a bad index moved a control");
        assert_eq!(eq.param_natural(PARAM_COUNT), 0.0);

        // Out of range in value, rather than in index: clamped to the travel.
        eq.set_param_natural(BandParam::Freq.index(0), 1.0e6);
        assert_eq!(f64::from(eq.param_natural(BandParam::Freq.index(0))), FREQ_MAX_HZ);
        eq.set_param_natural(BandParam::Gain.index(0), -500.0);
        assert_eq!(f64::from(eq.param_natural(BandParam::Gain.index(0))), -GAIN_MAX_DB);
        eq.set_param_natural(BandParam::Type.index(0), 99.0);
        assert_eq!(eq.band_type(0), BandType::AllPass);
        eq.set_param_natural(BandParam::Type.index(0), -99.0);
        assert_eq!(eq.band_type(0), BandType::Bell);
        eq.set_param_natural(PARAM_OUTPUT_TRIM, 900.0);
        assert_eq!(f64::from(eq.param_natural(PARAM_OUTPUT_TRIM)), TRIM_MAX_DB);
    }
}
