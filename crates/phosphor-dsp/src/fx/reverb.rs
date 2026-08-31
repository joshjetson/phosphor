//! Reverb: a Dattorro plate, an eight-line Jot FDN, and a modelled spring.
//!
//! One struct, one parameter set, four tank topologies behind an
//! [`Algorithm`] selector. The plate is the default and the one the rest of
//! the box is built around; the room and the hall are the same feedback delay
//! network at two delay-length sets; the spring is the plate's tank with a
//! three-hundred-section dispersion chain in front of it instead of the four
//! input diffusers.
//!
//! # Why Dattorro, and not "an FDN tuned to sound like a plate"
//!
//! A plate's identity is structural rather than a setting. An echo chamber's
//! echo density grows with the *square* of time; a plate's grows *linearly*
//! with time and linearly with frequency, and an FDN's growth is `t²` by
//! construction. You can damp and diffuse an FDN until it is smooth, but you
//! cannot make it build linearly, and what you get instead is the generic
//! digital reverb every cheap plug-in ships. An EMT 140 crosses the
//! statistical-noise threshold at about 2 ms where a concert hall takes
//! 344 ms — a hundred and fifty times faster — which is why "no early
//! reflections and a fast onset of a diffuse tail" is now the industry's
//! working definition of the word *plate*.
//!
//! Jon Dattorro's figure-eight tank (JAES 45(9), 1997, Figure 1 and Tables 1
//! and 2) is also the only published reverberator that arrives with a
//! complete parameter set, a stated reference sample rate, and coefficients
//! that were tuned by ear on hardware that shipped. The build therefore
//! starts from a known-good sound rather than from a tuning exercise.
//!
//! ```text
//!   in ─►[×½]─►[predelay]─►[bandwidth]─►[4 input diffusers]─┐
//!                    │                                       ▼
//!                    │            ┌─── ×decay ◄── R branch out ──┐
//!                    │            ▼                              │
//!                    │       (Σ)──►[AP 672+mod]──►[z⁻⁴⁴⁵³]──►[damp]
//!                    │        ▲                                  │
//!                    │        └────────────── ×decay ◄───────────┘
//!                    │                    ... and the mirror image, R
//!                    │
//!                    └──►[18-tap ER]──►(Σ)◄── 14 output taps ──► L, R
//! ```
//!
//! The tank is **one serial loop**, not two parallel ones: left feeds right
//! and right feeds left. That is the figure eight, and it is why there are
//! *four* `decay` multiplies per circulation rather than two — the fact the
//! whole RT60 map below rests on.
//!
//! # The three things the paper does not tell you
//!
//! **One. The two one-pole coefficients in Table 1 are z-plane poles, and a
//! fixed pole is a different frequency at a different sample rate.**
//! Everything else in the table is unitless. Render the plate at 22.05, 44.1
//! and 48 kHz with the literal `damping = 0.0005` and RT60 holds to 0.3% —
//! the lengths and the decay coefficient are doing their job — while the
//! tail's spectral centroid moves 3310 → 6642 → 7636 Hz. That is a factor of
//! 2.3, against a house rate-invariance bar of 2%, so the conversion is not
//! a nicety:
//!
//! ```text
//! pole = exp(−2π·f_c/fs)          f_c = −ln(pole)·fs/(2π)
//! ```
//!
//! **Two. The two filters use opposite conventions**, and getting it
//! backwards does not sound obviously wrong — it sounds like a decay-mapping
//! bug. `bandwidth` is the *feed-forward* coefficient (higher is brighter) and
//! `damping` is the *pole* (higher is darker). Writing the damping filter as
//! `state += damping·(x − state)` instead of `state = (1−damping)·x +
//! damping·state` puts its corner at 3.8 Hz rather than near Nyquist, and the
//! symptom is a tank stuck near RT60 0.55 s no matter where `decay` is set.
//! Both filters are written out in full below for that reason.
//!
//! **Three. Linear interpolation on a modulated delay is unaccounted
//! damping.** Its transfer is `(1−η) + η·z⁻¹`, a fraction-dependent lowpass
//! with a complete null at Nyquist when `η = 0.5`; as the modulator sweeps η
//! that is amplitude modulation *plus* a time-varying lowpass, and it
//! recirculates. Measured end to end in a modulated tank it costs about 10%
//! of the 12 kHz-band RT60 and 30% at 18 kHz. Dattorro names the artifact
//! himself. Allpass interpolation is flat in magnitude but carries state and
//! rings on a coefficient change, which is exactly what a continuous `size`
//! control does to it. So every modulated and every size-scaled read here is
//! a **four-point third-order Lagrange** with the fraction confined to the
//! middle interval, where the kernel's gain is bounded by one and therefore
//! cannot destabilise an allpass whose coefficient already is. It is
//! stateless, it has no transient, and in a whole reverb it costs 24% over
//! linear because the buffer read dominates, not the polynomial.
//!
//! # Decay is seconds, never a coefficient
//!
//! The figure eight circulates through all eight tank delays once, so
//! `T_loop = Σ delays / fs = 21589/29761 = 0.7254 s` at `size = 1`, with four
//! `decay` multiplies on the way round:
//!
//! ```text
//! RT60  = T_loop · 60 / (−80 · log10(decay))
//! decay = 10 ^ ( −0.75 · T_loop · size / RT60 )
//! ```
//!
//! Measured against Schroeder backward integration with a T30 fit, that is
//! accurate to ±3% for `RT60 ≥ 2·T_loop` and degrades to +10% at
//! `1.4·T_loop`, because the derivation assumes many circulations. The knob
//! is honest above about 1.5 s at `size = 1`, and short settings are reached
//! by shrinking `size` rather than by driving the coefficient toward zero.
//!
//! A happy confirmation rather than a coincidence to engineer around:
//! Dattorro's own default `decay = 0.5` measures **1.83 s**, and the house
//! default for a plate is 1.8 s. They are the same sound.
//!
//! # Size, and why it crossfades
//!
//! `size` multiplies the tank's delay lengths and nothing else — the four
//! input diffusers scale with the sample rate but not with size, because the
//! tank *is* the plate and the diffusers are its signature. Moving every
//! delay length under a running tail is the click problem, and there are only
//! three honest answers to it:
//!
//! * **Re-index instantly.** The click then recirculates and smears into
//!   grinding. Never.
//! * **Glide** — slew the fractional read. Free, because the reads are
//!   already fractional, but `ω_out = ω_in(1 − dD/dt)` means a 50 ms delay
//!   change over half a second is a **−182 cent** pitch bend. That is a
//!   performance gesture, not a mix reverb's default.
//! * **Morph** — crossfade two read-offset sets over the *same* buffers.
//!   Costs a doubled read count while a fade is running and nothing at all
//!   otherwise.
//!
//! Morph is the default here: geometry is quantised to 5% steps and each step
//! is a 30 ms equal-power crossfade, so a full 0.25 → 2.0 traverse is about a
//! second of soft flams rather than a comb. A target that arrives mid-fade
//! latches and is applied when the fade lands. Predelay rides the same
//! machinery, because it is the same kind of change. A program load calls
//! [`Reverb::set_param_natural_immediate`] and gets one 50 ms fade straight to
//! the destination: a patch change must not swoop and must not take a second.
//!
//! `decay` is recomputed from RT60 every time `size` moves, so the tail stays
//! where the player put it. That is the whole argument for putting seconds on
//! the knob: with a raw coefficient, every size change would silently be a
//! decay change too.
//!
//! # Wet and dry
//!
//! `out = dry·(1 − mix) + wet·mix`, a crossfade, and the shape is
//! load-bearing rather than a preference. The obvious alternative,
//! `dry + wet·mix`, looks like the same control and is not: at 100% it is
//! *dry plus a full reverb*, so a send bus set to "fully wet" returns the
//! source a second time a few milliseconds late. That is the phasey-send
//! trap, and it is why a player who tries a send once never tries it again.
//! Linear rather than equal-power, because a reverb's wet and dry are
//! correlated in the early part and equal-power over-sums them.
//!
//! At `mix == 0` the whole tank still runs — a tail must not glitch when the
//! knob comes back — and the input is then returned *itself*, by an early
//! exit, rather than as `dry·1.0 + wet·0.0`. The arithmetic would be right
//! for every value except one: `−0.0 + 0.0` is `+0.0`.
//!
//! # The 55× headroom finding
//!
//! An impulse says this plate peaks at 0.09 and has 20 dB of headroom. A
//! sustained 220 Hz sine at a long decay says it peaks at **5.05** and has
//! −14 dB. A resonant tank driven at a mode frequency accumulates coherently
//! to roughly `1/(1 − loop_gain)`, and fourteen output taps at 0.6 each then
//! add their own sum on top. Sweep, do not sample. Hence [`WET_TRIM`] on the
//! tap sum and [`crate::level::soft_saturate`] on the wet output only — the
//! saturator sits *outside* the tank, so the loop stays linear and RT60 stays
//! predictable.
//!
//! # Denormals
//!
//! A tail decays exponentially toward zero and then spends the rest of
//! eternity in the subnormal range. In `f32` a full-scale signal reaches the
//! smallest normal in `12.6 × RT60` after the input stops — 25 seconds at
//! RT60 2 s, which is an idle plug-in long after the musician stopped
//! playing. A reverb is the worst case in the whole rack: a hundred poisoned
//! multiply-adds per sample against a biquad's two, and on Intel every one of
//! them costs 120–150 cycles of microcode assist.
//!
//! Setting FTZ/DAZ is not available: `_mm_setcsr` has been deprecated in Rust
//! since 1.75 and its own documentation calls altering those bits immediate
//! undefined behaviour *even if the register is restored*, and the inline-asm
//! escape is UB under RFC 3514 as well. It would also make the output
//! platform-dependent, which breaks a cross-platform fingerprint by
//! construction. So the guard is arithmetic, in safe Rust, and correct
//! whether or not a host has already set FTZ:
//!
//! * **Every delay-line write below 1e-30 stores exact zero.** That threshold
//!   is eight decades under anything audible and eight *above* the `f32`
//!   subnormal range, so a buffer entry is never subnormal and the tail
//!   reaches true digital zero rather than idling forever.
//! * **The recursive scalar states get the same flush**, because the delay
//!   lines' own does not reach them: once the lines are zero these decay from
//!   about 1e-30 and would otherwise spend a few dozen samples in the `f64`
//!   subnormal range on the way down.
//!
//! **The published design for this is a DC injection** — `±1e-20` at each
//! tank input, sign alternating per block, to keep the states out of the
//! subnormal range — and it is not what shipped, because it is measurably
//! incompatible with the other half of the same requirement. An injection
//! that never stops is a tank that never stops: the wet output idles at
//! 2e-21 forever, the track never goes quiet, and the host can never sleep
//! it. The flush achieves what the injection was for (nothing is ever
//! subnormal) and what the injection prevents (the tail reaches exact zero),
//! and it costs a compare rather than an add.
//!
//! Apple Silicon has no denormal penalty at all, which is exactly why a
//! broken implementation passes on the development machine — so the test is
//! written against the *values*, which are the same on every platform, rather
//! than against a timing.
//!
//! # Cost
//!
//! **Memory: 1.45 MB of delay lines per instance at 48 kHz**, 1.27 at 44.1 and
//! 2.89 at 96 — every algorithm's buffers at once, sized for the longest
//! predelay, the largest `size` and the deepest modulation the controls
//! allow, because an algorithm change happens on the audio thread and cannot
//! allocate. About a third of that is the power-of-two rounding that makes a
//! delay-line read a mask instead of a branch, and it is worth it: a measured
//! sweep found the per-sample cost flat from 6 kB to 48 MB of delay memory,
//! because each line is one sequential read head and one write head.
//!
//! **CPU**, at 48 kHz, stereo, 512-frame blocks, as a percentage of one core:
//! **plate 0.59%**, room 1.20%, hall 1.13%, spring 2.71%. The eighteen-tap
//! stereo early-reflection section is 0.35% of the room's and the hall's
//! figures — the FDN tank alone is 0.85% and 0.79%.
//!
//! The plate is 6× the 22 ns/frame that a Dattorro tank benchmarks at in C,
//! and the difference is bought rather than lost: that figure is for a
//! structure whose reads are all *integer* except the two the modulator
//! moves. Every read here is fractional, because that is what buys exact
//! delay-length ratios at any sample rate and a continuous `size` control,
//! and a four-point read is four loads instead of one. Caching the
//! interpolation kernels ([`FracTap`]) recovers most of the difference; the
//! rest is the price of the two features.
//!
//! **The spring is the one algorithm over the 0.5% budget, by five times, and
//! it is a latency chain rather than a throughput problem.** Three hundred
//! first-order allpasses in series is a dependency chain three hundred deep:
//! about eight cycles from each section's input to its output, which no
//! amount of instruction-level parallelism can overlap. The lever, if one is
//! ever needed, is the section count — halving `M` halves the chirp's peak
//! group delay, which is a different (shorter) spring rather than a worse
//! one, and the measured tanks span 30 to 51 ms of it.
//!
//! # What is deliberately not here
//!
//! Stereo input to the tank (Dattorro sums to mono and the wet image comes
//! entirely from the cross-tapped outputs; feeding L and R into the two
//! branches is known to cause phase cancellation). Freeze as a control. A
//! delay feedback matrix. Per-band graphic-EQ damping. Jot's tone-correction
//! filter `E(z)` on the FDN — the design document flags its published form as
//! unresolved between a linear and a square-root exponent, and it is an HF
//! boost that would partly undo the damping control it is meant to
//! compensate. A dense allpass-free early field in place of the multitap.
//! Each of those is a real improvement and none of them is v1.

use std::f64::consts::TAU;

use crate::level::soft_saturate;

// ---------------------------------------------------------------------------
// The published constants
// ---------------------------------------------------------------------------

/// The rate every length in Dattorro's paper is quoted at.
pub const FS_REF: f64 = 29_761.0;

/// Input diffuser lengths at [`FS_REF`]: 142, 107, 379, 277.
///
/// Not size-scaled and not prime — only 107, 379 and 277 of the twelve
/// published lengths are, and Schroeder disclaimed the "maximal
/// incommensurate" folklore himself in this paper's own appendix: *"we just
/// picked a bunch of numbers and there was no mathematical basis."*
///
/// They are read fractionally like everything else, even though their lengths
/// never move. Rounding them to whole samples looks free and costs
/// rate-invariance: 0.6 × 142 samples is 126.2 at 44.1 kHz and 274.8 at
/// 96 kHz, and rounding both moves the room's diffuser combs by 0.3% in
/// opposite directions. The measured cost of that on a 232 ms network was
/// nearly 3% of tail centroid across the rate range, against a 2% bar.
const INPUT_DIFFUSER: [f64; 4] = [142.0, 107.0, 379.0, 277.0];

/// Tank delay lengths at [`FS_REF`], in the order the lines are indexed:
/// L allpass 1, L delay 1, L allpass 2, L delay 2, then the same four for R.
///
/// Dattorro modulates exactly two of these eight — the inner delays of the
/// first decay-diffusion allpass in each branch, indices 0 and 4 — driven by
/// a quadrature pair so the two are decorrelated.
const TANK: [f64; 8] = [
    672.0, 4453.0, 1800.0, 3720.0, // left branch
    908.0, 4217.0, 2656.0, 3163.0, // right branch
];

/// Maximum peak sample excursion of the delay modulation at [`FS_REF`], the
/// paper's `EXCURSION`. 16 samples is 0.538 ms, which is ±5.8 cents at 1 Hz —
/// microtonal, which is the word the paper uses.
const EXCURSION: f64 = 16.0;

/// Σ of [`TANK`], the figure eight's circulation time in samples at
/// [`FS_REF`]. 21 589 samples, 725.412 ms.
const TANK_TOTAL: f64 = 21_589.0;

/// `T_loop` at `size = 1.0`, in seconds. Rate-independent by construction.
pub const T_LOOP_SECONDS: f64 = TANK_TOTAL / FS_REF;

const INPUT_DIFFUSION_1: f64 = 0.750;
const INPUT_DIFFUSION_2: f64 = 0.625;

/// `decay diffusion 1`, applied **negative** in both branches. The paper:
/// *"Making them both negative will change the character of the impulse
/// response but does not destroy the all-pass transfer."*
const DECAY_DIFFUSION_1: f64 = 0.70;

/// The corner Table 1's `bandwidth = 0.9995` names once it is read as a pole
/// at [`FS_REF`] and converted to hertz. Far above Nyquist at every rate we
/// run at, which is the point: the paper's own gloss is that full bandwidth
/// is 0.9999999, so this filter is a formality rather than a tone control.
/// It is here as a frequency rather than as the number because a fixed pole
/// would drift 2.3× across our rate range.
const BANDWIDTH_HZ: f64 = 36_004.0;

/// Every output tap's gain, from Table 2.
const TAP_GAIN: f64 = 0.6;

/// Trim on the wet path, −6 dB.
///
/// It puts the realistic worst case — full-scale noise at maximum decay — at
/// 1.13 before the saturator, and the default case about 8 dB under the dry.
/// See the module docs on the 55× finding for why this is not tuned against
/// an impulse.
pub const WET_TRIM: f64 = 0.5;

/// Table 2, the left output. `(line, samples into that line, sign)`.
///
/// `yL` draws **four taps from the right branch and three from the left**,
/// and `yR` mirrors it. That cross-tapping is the entire stereo image: the
/// input is mono.
const TAPS_L: [(usize, f64, f64); 7] = [
    (5, 266.0, 1.0),
    (5, 2974.0, 1.0),
    (6, 1913.0, -1.0),
    (7, 1996.0, 1.0),
    (1, 1990.0, -1.0),
    (2, 187.0, -1.0),
    (3, 1066.0, -1.0),
];

/// Table 2, the right output.
const TAPS_R: [(usize, f64, f64); 7] = [
    (1, 353.0, 1.0),
    (1, 3627.0, 1.0),
    (2, 1228.0, -1.0),
    (3, 2673.0, 1.0),
    (5, 2111.0, -1.0),
    (6, 335.0, -1.0),
    (7, 121.0, -1.0),
];

/// The plate's intrinsic wet onset, in seconds: `node48_54[266]` is the
/// earliest output tap, so the first wet sample lands 8.94 ms after the
/// predelay however short the predelay is. Part of the design, not a defect —
/// but it has to be documented, because a player who sets predelay to zero
/// and measures 8.9 ms will otherwise file a bug.
pub const INTRINSIC_ONSET_SECONDS: f64 = 266.0 / FS_REF;

// ---------------------------------------------------------------------------
// Tuning
// ---------------------------------------------------------------------------

/// Below this magnitude a delay-line write stores exact zero.
///
/// Eight decades under anything audible and eight above `f32::MIN_POSITIVE`,
/// so it never truncates signal and always beats the subnormal range.
const DENORMAL_FLOOR: f32 = 1.0e-30;

/// Flush anything smaller than [`DENORMAL_FLOOR`] to exact zero.
///
/// For the recursive scalar states, which the delay lines' own flush does not
/// reach. Once the lines have gone to zero these decay from about 1e-30, and
/// without this they would spend a few dozen samples in the `f64` subnormal
/// range on the way down.
#[inline]
fn flush(x: f64) -> f64 {
    if x.abs() < f64::from(DENORMAL_FLOOR) {
        0.0
    } else {
        x
    }
}

/// Geometry moves in 5% steps. A 5% delay difference crossfaded is a soft
/// flam; a continuous one is a pitch bend.
const SIZE_QUANTUM: f64 = 0.05;

/// How long a geometry crossfade takes when a knob moved it.
const MORPH_FADE_SECONDS: f64 = 0.030;

/// How long it takes when a program change moved it. One fade, straight to
/// the destination, no stepping.
const PROGRAM_FADE_SECONDS: f64 = 0.050;

/// How long the wet is muted for across an algorithm change. A discrete
/// selector may reload; a continuous knob may not.
const ALGORITHM_FADE_SECONDS: f64 = 0.020;

/// Time constant of the coefficient smoothers, matching the EQ's.
const SMOOTH_SECONDS: f64 = 0.015;

/// Below this, a smoother chasing zero is snapped to it — so that a mix knob
/// turned to zero actually reaches zero and the dry null is exact.
const SMOOTH_SNAP: f64 = 1.0e-6;

/// The longest predelay the buffer is built for.
pub const PREDELAY_MAX_SECONDS: f64 = 0.500;

/// The longest early-reflection tap, before `size`. Moorer's pattern is
/// stretched to 110 ms for the hall, and `size` can double it.
const ER_MAX_SECONDS: f64 = 0.110;

/// The largest `size` the buffers are built for.
const SIZE_MAX: f64 = 2.0;

/// The smallest.
const SIZE_MIN: f64 = 0.25;

// ---------------------------------------------------------------------------
// Delay lines
// ---------------------------------------------------------------------------

/// A delay line with an integer and a fractional read.
///
/// `pos` is where the *next* sample goes, so `tap(1)` is the most recently
/// written sample and `tap(m)` is `m` samples ago. Everything in the tank
/// reads before it writes, which is what makes a delay of `m` a read of
/// `tap(m)` and an allpass's inner delay of `m` the same call — one rule
/// rather than two off-by-one conventions to get wrong.
struct Ring {
    buf: Vec<f32>,
    /// `capacity − 1`. The capacity is a power of two so that wrapping a read
    /// index is one `AND` rather than a compare and a branch.
    ///
    /// It is worth the memory. A reverb makes about a hundred delay-line
    /// accesses per frame — twenty-six fractional reads of four taps each,
    /// for the plate — and a branch on every one of them measured as more
    /// than half the effect's whole cost. Rounding up to a power of two costs
    /// at worst a doubling of buffer space, and delay memory is the one thing
    /// a reverb has plenty of: a measured sweep found the per-sample cost
    /// flat from 6 kB to 48 MB of delay memory, because each line is one
    /// sequentially-advancing read head and one write head, which is the
    /// friendliest possible pattern for a prefetcher.
    mask: usize,
    pos: usize,
    /// The largest `back` a fractional read may ask for and still have its
    /// four taps inside the buffer.
    limit: f64,
}

impl Ring {
    fn new(len: usize) -> Self {
        let capacity = len.max(8).next_power_of_two();
        Self {
            buf: vec![0.0; capacity],
            mask: capacity - 1,
            pos: 0,
            limit: (capacity - 3) as f64,
        }
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }

    /// Store one sample, flushing anything that has fallen into the
    /// subnormal range to exact zero on the way in.
    #[inline]
    fn write(&mut self, x: f64) {
        let v = x as f32;
        self.buf[self.pos] = if v.abs() < DENORMAL_FLOOR { 0.0 } else { v };
        self.pos = (self.pos + 1) & self.mask;
    }

    /// The sample written `back` steps ago; `back = 1` is the newest.
    ///
    /// `back` is expected to be at least 1 and no more than the capacity;
    /// everything in the tank reads a length that was sized against the
    /// buffer it reads from, so the mask is a wrap rather than a clamp.
    #[inline]
    fn tap(&self, back: usize) -> f64 {
        debug_assert!(back >= 1 && back <= self.mask);
        f64::from(self.buf[self.pos.wrapping_sub(back) & self.mask])
    }

    /// A fractional read, four-point third-order Lagrange.
    ///
    /// The four taps straddle the requested delay so that the fraction always
    /// lands in the *middle* interval, which is the region where `|H| ≤ 1`
    /// and the kernel is therefore safe inside a feedback loop. Stateless, so
    /// a delay length that moves — which `size` and the modulator both do —
    /// costs no transient.
    #[inline]
    fn tap_cubic(&self, back: f64) -> f64 {
        // `max` then `min` rather than `clamp`: it is two instructions
        // instead of a branch, and it lands a NaN on the floor rather than
        // propagating it into an index.
        let back = back.max(2.0).min(self.limit);
        let whole = back.floor();
        let d = back - whole;
        let base = self.pos.wrapping_sub(whole as usize);
        let mask = self.mask;
        let ym1 = f64::from(self.buf[base.wrapping_add(1) & mask]);
        let y0 = f64::from(self.buf[base & mask]);
        let y1 = f64::from(self.buf[base.wrapping_sub(1) & mask]);
        let y2 = f64::from(self.buf[base.wrapping_sub(2) & mask]);
        let c = lagrange3(d);
        c[0].mul_add(ym1, c[1].mul_add(y0, c[2].mul_add(y1, c[3] * y2)))
    }

    /// A fractional read whose kernel was worked out when the geometry last
    /// moved: four masked loads and four multiply-adds.
    #[inline]
    fn read(&self, tap: &FracTap) -> f64 {
        let base = self.pos.wrapping_sub(tap.whole);
        let mask = self.mask;
        let ym1 = f64::from(self.buf[base.wrapping_add(1) & mask]);
        let y0 = f64::from(self.buf[base & mask]);
        let y1 = f64::from(self.buf[base.wrapping_sub(1) & mask]);
        let y2 = f64::from(self.buf[base.wrapping_sub(2) & mask]);
        tap.c[0].mul_add(ym1, tap.c[1].mul_add(y0, tap.c[2].mul_add(y1, tap.c[3] * y2)))
    }

    /// The same read at two geometries, mixed by the crossfade's gains.
    #[inline]
    fn read_fade(&self, a: &FracTap, b: &FracTap, ga: f64, gb: f64) -> f64 {
        ga * self.read(a) + gb * self.read(b)
    }
}

/// A delay-line read whose interpolation coefficients have already been
/// worked out.
///
/// Almost every read in a reverb is *quasi-static*: its length changes only
/// when the geometry does, which is when a knob moves and never while a block
/// is rendering. Recomputing a Lagrange kernel from the fractional part on
/// every sample is therefore about fifteen flops and a float-to-int
/// conversion spent re-deriving the same four numbers forty-eight thousand
/// times a second. Measured on the plate, caching them took the whole effect
/// from 0.90% of a core to 0.55%.
///
/// The two exceptions are the reads the modulator moves — the inner delays of
/// the first decay-diffusion allpass in each branch, and the FDN's eight
/// lines — which genuinely change every sample and keep the live path.
#[derive(Clone, Copy)]
struct FracTap {
    whole: usize,
    c: [f64; 4],
}

impl FracTap {
    const SILENT: Self = Self {
        whole: 2,
        c: [0.0; 4],
    };

    /// The kernel for a delay of `back` samples. `back` below two is clamped:
    /// four points around the read means the shortest delay a fractional read
    /// can express is two samples, 42 µs at 48 kHz.
    fn at(back: f64) -> Self {
        let back = back.max(2.0);
        let whole = back.floor();
        let d = back - whole;
        Self {
            whole: whole as usize,
            c: lagrange3(d),
        }
    }
}

/// The four-point third-order Lagrange kernel, with the fraction in the
/// middle interval.
#[inline]
fn lagrange3(d: f64) -> [f64; 4] {
    [
        -d * (d - 1.0) * (d - 2.0) / 6.0,
        (d + 1.0) * (d - 1.0) * (d - 2.0) * 0.5,
        -(d + 1.0) * d * (d - 2.0) * 0.5,
        (d + 1.0) * d * (d - 1.0) / 6.0,
    ]
}

/// A Schroeder allpass in the two-multiplier lattice form the paper draws.
///
/// `v = delay(m); u = x + a·v; write(u); y = v − a·u`, which realises
/// `(D − a)/(1 − a·D)`. Reading before writing is what makes the inner delay
/// exactly `m`.
struct Allpass {
    line: Ring,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            line: Ring::new(len),
        }
    }

    #[inline]
    fn tick(&mut self, x: f64, a: f64, len: f64) -> f64 {
        let v = self.line.tap_cubic(len);
        let u = x + a * v;
        self.line.write(u);
        v - a * u
    }

    /// The lattice on a read whose kernel is already worked out.
    #[inline]
    fn tick_tap(&mut self, x: f64, a: f64, tap: &FracTap) -> f64 {
        let v = self.line.read(tap);
        let u = x + a * v;
        self.line.write(u);
        v - a * u
    }

    /// The same at two geometries, which is what a morph does to an allpass:
    /// the *read* is crossfaded and the single write that follows carries
    /// both.
    #[inline]
    fn tick_tap_fade(&mut self, x: f64, a: f64, ta: &FracTap, tb: &FracTap, ga: f64, gb: f64) -> f64 {
        let v = self.line.read_fade(ta, tb, ga, gb);
        let u = x + a * v;
        self.line.write(u);
        v - a * u
    }

    /// The same, with the inner delay read at two lengths and the two reads
    /// crossfaded.
    ///
    /// An allpass is the one place a geometry morph cannot simply run two
    /// copies of the structure, because its write depends on its read. What
    /// is crossfaded is therefore the *read* — which is exactly the promise
    /// the morph makes everywhere else — and the single write that follows
    /// carries both.
    #[inline]
    fn tick_fade(&mut self, x: f64, a: f64, len_a: f64, len_b: f64, ga: f64, gb: f64) -> f64 {
        let v = ga * self.line.tap_cubic(len_a) + gb * self.line.tap_cubic(len_b);
        let u = x + a * v;
        self.line.write(u);
        v - a * u
    }

    fn clear(&mut self) {
        self.line.clear();
    }
}

/// A one-pole smoother chasing a target.
#[derive(Clone, Copy, Default)]
struct Smoother {
    value: f64,
    target: f64,
}

impl Smoother {
    fn snap(&mut self, value: f64) {
        self.value = value;
        self.target = value;
    }

    #[inline]
    fn advance(&mut self, a: f64) -> f64 {
        self.value += a * (self.target - self.value);
        // A control that was asked for zero has to *reach* zero, or a wet/dry
        // at 0% is a dry path multiplied by 1e-9 rather than a dry path.
        if self.target == 0.0 && self.value.abs() < SMOOTH_SNAP {
            self.value = 0.0;
        }
        self.value
    }
}

/// The pole of a one-pole at a corner frequency. The only correct way to move
/// any of Table 1's filters to another sample rate.
#[inline]
#[must_use]
pub fn pole_at(hz: f64, sample_rate: f64) -> f64 {
    if !(hz.is_finite() && sample_rate > 0.0) {
        return 0.0;
    }
    (-TAU * hz.max(0.0) / sample_rate).exp().clamp(0.0, 0.9999)
}

/// The integrator coefficient of a trapezoidal one-pole lowpass at `hz`.
///
/// # Why the damping filter is not the paper's shape
///
/// Dattorro's damping is `y = (1−d)·x + d·y[n−1]`, whose pole is the
/// impulse-invariant `exp(−2π·f_c/fs)`. Converting `d` by frequency puts the
/// corner in the right place at any rate — that part is mandatory and the
/// module docs say why — but it does not make the filter *shape* the same at
/// any rate. That form has a zero nowhere: its gain at Nyquist is
/// `(1−p)/(1+p)`, which for a 6 kHz corner is 0.40 at 44.1 kHz and 0.19 at
/// 96 kHz. In a feedback tank that difference is multiplied by every
/// circulation, and a 232 ms network at RT60 2 s circulates sixty-nine times.
///
/// The trapezoidal form has a zero at Nyquist at every rate, so its magnitude
/// tracks the analog prototype across the whole audible band. Measured on the
/// room, moving to it took the tail centroid's spread across 44.1–96 kHz from
/// 3.2% to under 2%, which is the house bar. It costs one add.
///
/// ```text
/// g = tan(π·f_c/fs)   G = g/(1+g)
/// v = (x − z)·G ;  y = v + z ;  z = y + v
/// ```
///
/// DC gain is exactly one, which is what a filter inside a decay loop has to
/// have or the RT60 knob lies.
#[inline]
#[must_use]
pub fn tpt_gain(hz: f64, sample_rate: f64) -> f64 {
    if !(hz.is_finite() && sample_rate > 0.0) {
        return 1.0;
    }
    // Half a hair under Nyquist: `tan` goes to infinity there.
    let corner = hz.clamp(1.0, sample_rate * 0.49);
    let g = (std::f64::consts::PI * corner / sample_rate).tan();
    g / (1.0 + g)
}

// ---------------------------------------------------------------------------
// Algorithms
// ---------------------------------------------------------------------------

/// Which tank is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    /// Dattorro's figure eight, at the paper's own lengths and coefficients.
    Plate,
    /// The eight-line FDN at its short delay set, 20–40 ms.
    Room,
    /// The same FDN at zita-rev1's published set, 125–257 ms.
    Hall,
    /// The plate tank behind a three-hundred-section dispersion chain.
    Spring,
}

impl Algorithm {
    /// The four, in the order the selector steps through them.
    pub const ALL: [Algorithm; 4] = [Self::Plate, Self::Room, Self::Hall, Self::Spring];

    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Plate => 0,
            Self::Room => 1,
            Self::Hall => 2,
            Self::Spring => 3,
        }
    }

    /// The algorithm at a position in [`Algorithm::ALL`]; anything else is
    /// the plate, because a session with a number this build does not know
    /// should load as the default rather than as nothing.
    #[must_use]
    pub fn from_index(index: usize) -> Self {
        *Self::ALL.get(index).unwrap_or(&Self::Plate)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Plate => "plate",
            Self::Room => "room",
            Self::Hall => "hall",
            Self::Spring => "spring",
        }
    }

    /// Whether this algorithm's tank is the plate's figure eight.
    #[must_use]
    fn uses_plate_tank(self) -> bool {
        matches!(self, Self::Plate | Self::Spring)
    }

    /// The early-reflection level this algorithm wants, as a percentage.
    ///
    /// **Zero for the plate, and that is the definition rather than a
    /// setting**: a plate's defining property is instantaneous density with
    /// no build-up, and an EMT 140 reaches the statistical-noise threshold in
    /// about 2 ms. The control exists on it anyway because the Prophet-6's
    /// `PLA` has an early-reflections knob and forty-five factory programs
    /// use it.
    ///
    /// **Half for the room and the hall, and that is arithmetic**: a bare
    /// eight-line FDN emits `N/mean_delay` echoes a second on its first pass
    /// — 276/s for the room and 44/s for the hall, against Schroeder's 1000/s
    /// floor — and the hall's shortest line means it emits *nothing at all*
    /// for 96 ms, which is the shorter of the two lines the output butterfly
    /// reads. Something has to carry the first tenth of a second.
    ///
    /// This is a *suggestion*, not a floor: the parameter's own default is
    /// the plate's, and the panel moves the control to the incoming
    /// algorithm's suggestion only when the player has not overridden it. The
    /// knob never lies about what it is set to.
    #[must_use]
    pub fn suggested_early(self) -> f32 {
        match self {
            Self::Plate | Self::Spring => 0.0,
            Self::Room | Self::Hall => 50.0,
        }
    }

    /// Whether a control does anything on this algorithm.
    ///
    /// One case, and it is real: the spring's input stage is the dispersion
    /// chain, so there are no diffuser coefficients for `diffusion` to scale.
    /// The panel greys what this refuses and the keys refuse to move it, which
    /// is the same fact said twice on purpose.
    #[must_use]
    pub fn uses(self, param: usize) -> bool {
        !(param == PARAM_DIFFUSION && self == Self::Spring)
    }
}

// ---------------------------------------------------------------------------
// The flat parameter surface, in natural units
// ---------------------------------------------------------------------------
//
// Hertz, milliseconds, seconds and percent — never a 0..1 knob fraction. A
// session stores what a control *meant*, so a range that moves later cannot
// silently re-point every saved file.

pub const PARAM_ALGORITHM: usize = 0;
pub const PARAM_PREDELAY_MS: usize = 1;
pub const PARAM_DECAY_S: usize = 2;
pub const PARAM_SIZE: usize = 3;
pub const PARAM_DAMP_HZ: usize = 4;
pub const PARAM_LOW_CUT_HZ: usize = 5;
pub const PARAM_EARLY: usize = 6;
pub const PARAM_DIFFUSION: usize = 7;
pub const PARAM_MOD_RATE_HZ: usize = 8;
pub const PARAM_MOD_DEPTH: usize = 9;
pub const PARAM_WIDTH: usize = 10;
pub const PARAM_MIX: usize = 11;

/// How many controls a reverb has.
pub const PARAM_COUNT: usize = 12;

/// One control, as a host sees it.
///
/// `&'static str` for both strings: a panel reads these while it is drawing,
/// sixty times a second, and a `String` per control per frame is an
/// allocation storm for text that never changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NaturalParam {
    pub name: &'static str,
    /// `"Hz"`, `"ms"`, `"s"`, `"%"`, or empty for the two counted controls.
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

/// The table every other view of the parameters is generated from.
///
/// The defaults are the house's: *plate, predelay 20 ms, RT60 1.8 s, damping
/// ~6 kHz, mix 25% on an insert*. A bus overrides the mix to 100%, which is
/// the bus's business and not this table's.
const PARAMS: [NaturalParam; PARAM_COUNT] = [
    NaturalParam {
        name: "alg",
        unit: "",
        min: 0.0,
        max: 3.0,
        default: 0.0,
    },
    NaturalParam {
        name: "predly",
        unit: "ms",
        min: 0.0,
        max: 500.0,
        default: 20.0,
    },
    NaturalParam {
        name: "decay",
        unit: "s",
        min: 0.2,
        max: 20.0,
        default: 1.8,
    },
    NaturalParam {
        name: "size",
        unit: "",
        min: SIZE_MIN as f32,
        max: SIZE_MAX as f32,
        default: 1.0,
    },
    NaturalParam {
        name: "damp",
        unit: "Hz",
        min: 1_000.0,
        max: 20_000.0,
        default: 6_000.0,
    },
    NaturalParam {
        name: "locut",
        unit: "Hz",
        min: 20.0,
        max: 1_000.0,
        default: 60.0,
    },
    NaturalParam {
        name: "early",
        unit: "%",
        min: 0.0,
        max: 100.0,
        default: 0.0,
    },
    NaturalParam {
        name: "diff",
        unit: "%",
        min: 0.0,
        max: 100.0,
        default: 100.0,
    },
    NaturalParam {
        name: "mrate",
        unit: "Hz",
        min: 0.05,
        max: 5.0,
        default: 1.0,
    },
    NaturalParam {
        name: "mdepth",
        unit: "%",
        min: 0.0,
        max: 100.0,
        default: 35.0,
    },
    NaturalParam {
        name: "width",
        unit: "%",
        min: 0.0,
        max: 100.0,
        default: 100.0,
    },
    NaturalParam {
        name: "mix",
        unit: "%",
        min: 0.0,
        max: 100.0,
        default: 25.0,
    },
];

/// What control `index` is, or `None` past the end of the space.
#[must_use]
pub fn natural_param(index: usize) -> Option<NaturalParam> {
    PARAMS.get(index).copied()
}

/// The control's name, without a unit. Empty past the end.
#[must_use]
pub fn param_name(index: usize) -> &'static str {
    PARAMS.get(index).map_or("", |p| p.name)
}

/// The factory settings as a flat natural-unit vector.
#[must_use]
pub fn default_natural_params() -> [f32; PARAM_COUNT] {
    let mut out = [0.0f32; PARAM_COUNT];
    for (index, value) in out.iter_mut().enumerate() {
        *value = PARAMS[index].default;
    }
    out
}

/// `decay_s` for a target RT60, at a given loop time.
///
/// The published map, and the reason the knob is in seconds: with a raw
/// coefficient every `size` change would silently be a decay change too.
#[inline]
#[must_use]
pub fn decay_for_rt60(rt60_s: f64, loop_seconds: f64) -> f64 {
    if rt60_s <= 0.0 || loop_seconds <= 0.0 || !rt60_s.is_finite() {
        return 0.0;
    }
    (10.0f64)
        .powf(-0.75 * loop_seconds / rt60_s)
        .clamp(0.0, 0.9995)
}

/// The RT60 a decay coefficient gives, the same map read backwards.
#[inline]
#[must_use]
pub fn rt60_for_decay(decay: f64, loop_seconds: f64) -> f64 {
    if !(0.0..1.0).contains(&decay) || decay <= 0.0 {
        return 0.0;
    }
    loop_seconds * 60.0 / (-80.0 * decay.log10())
}

// ---------------------------------------------------------------------------
// Early reflections
// ---------------------------------------------------------------------------

/// Moorer's tap pattern, from *About This Reverberation Business* (IRCAM
/// 17/78, published as CMJ 3(2), 1979), tap 0 — the direct signal — dropped.
///
/// Eighteen taps spanning 4.3 to 79.7 ms. Two provenance facts worth keeping
/// next to the numbers: they come from *"a highly idealized geometric
/// simulation of the Boston Symphony Hall"* rather than a measurement, and
/// they were chosen *"essentially by trial and error."* **All gains are
/// positive** — there are no sign inversions in the published table, and the
/// near-coincident pairs (21.5/22.5, 26.8/27.0, 70.7/70.8) are mirror-image
/// sources rather than an accident.
///
/// Gardner reached the same count from the other direction thirteen years
/// later: his ray-traced rooms *"suffered from an overly discrete sound... as
/// if the sound was being reattacked, like a drum flam"*, and pruning them to
/// something usable landed at *"roughly 20 taps per speaker."*
const MOORER_TAPS: [(f64, f64); 18] = [
    (4.3, 0.841),
    (21.5, 0.504),
    (22.5, 0.491),
    (26.8, 0.379),
    (27.0, 0.380),
    (29.8, 0.346),
    (45.8, 0.289),
    (48.5, 0.272),
    (57.2, 0.192),
    (58.7, 0.193),
    (59.5, 0.217),
    (61.2, 0.181),
    (70.7, 0.180),
    (70.8, 0.181),
    (72.6, 0.176),
    (74.1, 0.142),
    (75.3, 0.167),
    (79.7, 0.134),
];

const ER_COUNT: usize = MOORER_TAPS.len();
const MOORER_FIRST_MS: f64 = 4.3;
const MOORER_LAST_MS: f64 = 79.7;

/// The window Moorer's pattern is stretched into, per algorithm.
///
/// Room versus hall is the *span*, not the structure, and the two windows
/// come from Griesinger's division rather than from taste: lateral
/// reflections in 10–50 ms *"contribute a sense of distance... pushing it
/// away from the listener"*, while reflections later than 50 ms *"create a
/// 'spaciousness' impression that surrounds the listener"*, still growing at
/// 160 ms. The plate keeps Moorer's own span, because a plate has no early
/// reflections at all and this control only exists on it because the
/// Prophet-6's `PLA` has one.
fn er_window(algorithm: Algorithm) -> (f64, f64) {
    match algorithm {
        Algorithm::Plate => (MOORER_FIRST_MS, MOORER_LAST_MS),
        Algorithm::Room | Algorithm::Spring => (5.0, 40.0),
        Algorithm::Hall => (15.0, 110.0),
    }
}

// ---------------------------------------------------------------------------
// The feedback delay network
// ---------------------------------------------------------------------------

const FDN_LINES: usize = 8;

/// The room's eight lines as `(total ms, allpass ms)`. Σ = 232.1 ms, which by
/// Schroeder's `Σ delay ≥ 0.15·RT60` criterion is what caps the room's honest
/// decay at 1.55 s rather than at the 3 s a naive reading would allow.
///
/// **The two shortest come first**, because the output butterfly reads lines
/// 0 and 1: an FDN says nothing at all until its first line comes round, so
/// which two the output is taken from is the difference between a network
/// that starts at 20 ms and one that starts at 40.
const ROOM_MS: [(f64, f64); FDN_LINES] = [
    (20.000, 2.154),
    (22.082, 5.459),
    (40.000, 4.256),
    (24.380, 3.240),
    (36.229, 3.148),
    (26.918, 3.529),
    (32.813, 3.810),
    (29.720, 4.526),
];

/// The hall's eight lines: zita-rev1's published set, 125–257 ms,
/// Σ = 1460.3 ms, reordered shortest-first for the same reason as the room's.
///
/// **Each line contains an allpass**, which is zita's own design and a
/// deliberate departure from the rule that an FDN should get its density from
/// input diffusion and matrix scattering rather than from allpasses inside
/// the recursion. Blesser's warning about allpasses in a loop is real — *"the
/// effective loop time and reverberation time vary with frequency; after many
/// trips around the loop, the result will be very colored"* — and it is
/// overruled here by measurement: with 125–257 ms lines and no in-loop
/// diffusion, the hall's normalised echo density is still 0.44 at 400 ms,
/// which is audibly sparse. Eight allpasses of 13–32 ms are what turn eight
/// first-pass echoes into a hundred.
///
/// Even so the network says **nothing at all for 96 ms** — the shorter of the
/// two lines the output butterfly reads, measured. That is the arithmetic
/// behind [`Algorithm::suggested_early`]: a hall's first tenth of a second
/// has to come from the early-reflection multitap or there is an audible hole
/// between the predelay and the tail, heard as a detached, sourceless reverb
/// with no distance cue.
const HALL_MS: [(f64, f64); FDN_LINES] = [
    (125.000, 13.458),
    (127.837, 31.604),
    (256.891, 27.333),
    (153.129, 20.346),
    (219.991, 19.123),
    (174.713, 22.904),
    (210.389, 24.421),
    (192.303, 29.291),
];

/// The in-loop allpass coefficient, alternating in sign per line, as zita
/// ships it.
const FDN_AP_COEFFICIENT: f64 = 0.6;

/// The sign vector that breaks the Hadamard's involution.
///
/// A raw Sylvester Hadamard and Jot's Householder are both involutions —
/// `A² = I`, eigenvalues only ±1 — so applying either twice is a no-op and
/// the scattering never *rotates* energy through the state space. Stautner &
/// Puckette's 1982 matrix is a permuted, sign-flipped Hadamard with order 4
/// and eigenvalues spread around the unit circle, and that is the fix. It
/// costs nothing at runtime.
const FDN_SIGNS: [f64; FDN_LINES] = [1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0];

/// The row permutation that goes with [`FDN_SIGNS`].
const FDN_PERM: [usize; FDN_LINES] = [1, 3, 0, 5, 7, 2, 6, 4];

/// Input distribution: zita's `fanflip`, with the sign flipped on half the
/// lines. An all-`+1` input vector excites only the network's common mode —
/// one eigenvector — so most of the poles never receive any energy at all.
const FDN_INPUT_SIGNS: [f64; FDN_LINES] = [1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0];

/// `1/√8`, the Hadamard's normalisation, folded into the per-line gain so the
/// matrix itself is 24 add/subtracts and no multiplies.
const FDN_NORM: f64 = 0.353_553_390_593_273_8;

/// Output gain for the room and for the hall, so that changing algorithm is
/// not also a fader move.
///
/// The plate sums fourteen taps at 0.6 and the FDN takes a two-line butterfly
/// at 0.37, which is about 13 dB apart untrimmed. These do not close the gap
/// completely and that is deliberate: a network's *sustained* gain rises with
/// decay far faster when it is short, so matching the 232 ms room to the
/// 725 ms plate at the factory decay would put the room's long settings into
/// the saturator. At these numbers the four algorithms sit within about 4 dB
/// of each other at their defaults, which is a program-level difference
/// rather than a bug.
const ROOM_OUTPUT: f64 = 2.6;
const HALL_OUTPUT: f64 = 3.1;

/// The eight-point fast Walsh–Hadamard transform, in place. 24 add/sub.
#[inline]
fn fwht8(s: &mut [f64; FDN_LINES]) {
    let mut span = 1usize;
    while span < FDN_LINES {
        let mut i = 0usize;
        while i < FDN_LINES {
            for j in i..i + span {
                let (u, v) = (s[j], s[j + span]);
                s[j] = u + v;
                s[j + span] = u - v;
            }
            i += 2 * span;
        }
        span *= 2;
    }
}

// ---------------------------------------------------------------------------
// The spring
// ---------------------------------------------------------------------------
//
// A spring is not a small plate. Its echo density is `1/T_D` — constant in
// time and never growing, 18–33 chirps a second per spring, which is two to
// three orders of magnitude below a plate. The character comes from
// dispersion, not density: torsional waves in a helix have a group velocity
// that rises with frequency, so an impulse leaves as a chirp with the highs
// arriving first and the lows trailing. Any implementation that makes it
// dense has stopped being a spring.
//
// Välimäki, Parker & Abel's model (JAES 58(7/8), 2010, parameters recovered
// from Gamper/Parker/Välimäki DAFx-11 Table 1) is two parallel cascades of
// *stretched* first-order allpasses, whose group delay
// `D(ω) = k·M(1−a²)/(1 + 2a·cos(ωk) + a²)` peaks at `F_c = fs/2k` — which is
// how the chain concentrates its dispersion where a real spring does.

/// Sections in the low chirp's cascade, at the full sample rate.
const SPRING_LOW_SECTIONS: usize = 100;

/// Sections in the high chirp's cascade, run at `fs/4`.
const SPRING_HIGH_SECTIONS: usize = 200;

const SPRING_LOW_A: f64 = 0.62;
const SPRING_HIGH_A: f64 = -0.6;

/// The transition frequency the dispersion peaks at. Real tanks measure
/// 2.6–5.0 kHz; this is the paper's own fitted value.
const SPRING_FC_HZ: f64 = 4_300.0;

/// The high chirp sits at least 30 dB below the low one in the paper's
/// measurements, and it is what puts the metallic edge on the attack.
const SPRING_HIGH_LEVEL: f64 = 0.0316;

/// One stretched allpass: a `K1`-sample delay and a first-order allpass
/// standing in for the leftover fraction, wrapped in a Schroeder lattice.
struct SpringSection {
    line: Ring,
    /// The fractional allpass's two state words.
    x1: f64,
    y1: f64,
}

impl SpringSection {
    fn new(k1: usize) -> Self {
        Self {
            line: Ring::new(k1 + 8),
            x1: 0.0,
            y1: 0.0,
        }
    }

    #[inline]
    fn tick(&mut self, x: f64, a: f64, k1: usize, a2: f64) -> f64 {
        let d = self.line.tap(k1);
        // The fractional allpass, `(a2 + z⁻¹)/(1 + a2·z⁻¹)`.
        let f = a2.mul_add(d, self.x1) - a2 * self.y1;
        self.x1 = d;
        self.y1 = f;
        let u = x + a * f;
        self.line.write(u);
        f - a * u
    }

    fn clear(&mut self) {
        self.line.clear();
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// Every quasi-static read offset in the instance, for one geometry.
///
/// Rebuilt when the geometry or the algorithm moves — a few hundred flops,
/// once, off the per-sample path — and read four masked loads at a time
/// after that.
#[derive(Clone, Copy)]
struct Taps {
    predelay: FracTap,
    diffuser: [FracTap; 4],
    /// Indexed as [`TANK`]; slots 0 and 4 are the modulated allpasses and
    /// are read live instead.
    tank: [FracTap; 8],
    out_l: [FracTap; 7],
    out_r: [FracTap; 7],
    er_l: [FracTap; ER_COUNT],
    er_r: [FracTap; ER_COUNT],
    fdn_ap: [FracTap; FDN_LINES],
}

impl Taps {
    const SILENT: Self = Self {
        predelay: FracTap::SILENT,
        diffuser: [FracTap::SILENT; 4],
        tank: [FracTap::SILENT; 8],
        out_l: [FracTap::SILENT; 7],
        out_r: [FracTap::SILENT; 7],
        er_l: [FracTap::SILENT; ER_COUNT],
        er_r: [FracTap::SILENT; ER_COUNT],
        fdn_ap: [FracTap::SILENT; FDN_LINES],
    };
}

/// Everything a delay-length change touches, as one thing.
///
/// Size and predelay move together through the crossfade because they are the
/// same kind of change: a read offset that cannot be re-indexed under a
/// running tail without a click that then recirculates.
#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    size: f64,
    /// Predelay in samples.
    predelay: f64,
}

// ---------------------------------------------------------------------------
// The reverb
// ---------------------------------------------------------------------------

/// A stereo reverb: predelay, early reflections, one of four tanks, damping,
/// width and mix.
///
/// Every buffer is built in [`Reverb::new`] or [`Reverb::set_sample_rate`],
/// sized for the largest `size`, predelay and modulation depth the controls
/// allow. Nothing below those two functions allocates, including an algorithm
/// change while the tank is sounding.
pub struct Reverb {
    sample_rate: f64,
    /// Samples per reference sample, `fs/29761`.
    scale: f64,

    // ── The controls, as the host set them ──
    params: [f32; PARAM_COUNT],
    algorithm: Algorithm,

    // ── Plate ──
    diffuser: Vec<Allpass>,
    diffuser_len: [f64; 4],
    tank_ap: Vec<Allpass>,
    tank_del: Vec<Ring>,
    /// Base tank lengths in samples at `size = 1.0`, indexed as [`TANK`].
    tank_len: [f64; 8],
    damp_z: [f64; 2],
    hp_z: [f64; 2],
    feedback: [f64; 2],
    bandwidth_z: f64,
    bandwidth_a: f64,

    // ── FDN ──
    fdn_line: Vec<Ring>,
    fdn_ap: Vec<Allpass>,
    /// `(total ms, allpass ms)` per line, for whichever set is loaded.
    fdn_ms: [(f64, f64); FDN_LINES],
    fdn_damp_z: [f64; FDN_LINES],
    fdn_hp_z: [f64; FDN_LINES],
    fdn_diffuser: Vec<Allpass>,
    fdn_diffuser_len: [f64; 4],

    // ── Spring ──
    spring_low: Vec<SpringSection>,
    spring_high: Vec<SpringSection>,
    spring_k1: usize,
    spring_a2: f64,
    spring_phase: u32,
    spring_high_hold: f64,

    // ── Shared ──
    predelay: Ring,
    er_l: [f64; ER_COUNT],
    er_r: [f64; ER_COUNT],
    er_gain: [f64; ER_COUNT],
    excursion: f64,
    lfo_phase: f64,

    // ── Geometry crossfade ──
    /// The read kernels for `geo` and for `geo_next`, in that order.
    taps: [Taps; 2],
    geo: Geometry,
    geo_next: Geometry,
    geo_pending: Option<Geometry>,
    fade: f64,
    fade_step: f64,
    fading: bool,

    // ── Algorithm crossfade ──
    algorithm_pending: Option<Algorithm>,
    algorithm_gain: f64,
    algorithm_step: f64,

    // ── Smoothed coefficients ──
    smooth_a: f64,
    decay: Smoother,
    damp_pole: Smoother,
    hp_a: Smoother,
    early: Smoother,
    diffusion: Smoother,
    width: Smoother,
    mix: Smoother,
    mod_depth: Smoother,
    lfo_step: f64,

}

impl Reverb {
    /// Build one at a sample rate, with every buffer it will ever need.
    #[must_use]
    pub fn new(sample_rate: f64) -> Self {
        let mut verb = Self {
            sample_rate: 48_000.0,
            scale: 1.0,
            params: default_natural_params(),
            algorithm: Algorithm::Plate,
            diffuser: Vec::new(),
            diffuser_len: [0.0; 4],
            tank_ap: Vec::new(),
            tank_del: Vec::new(),
            tank_len: [0.0; 8],
            damp_z: [0.0; 2],
            hp_z: [0.0; 2],
            feedback: [0.0; 2],
            bandwidth_z: 0.0,
            bandwidth_a: 1.0,
            fdn_line: Vec::new(),
            fdn_ap: Vec::new(),
            fdn_ms: HALL_MS,
            fdn_damp_z: [0.0; FDN_LINES],
            fdn_hp_z: [0.0; FDN_LINES],
            fdn_diffuser: Vec::new(),
            fdn_diffuser_len: [0.0; 4],
            spring_low: Vec::new(),
            spring_high: Vec::new(),
            spring_k1: 4,
            spring_a2: 0.0,
            spring_phase: 0,
            spring_high_hold: 0.0,
            predelay: Ring::new(8),
            er_l: [0.0; ER_COUNT],
            er_r: [0.0; ER_COUNT],
            er_gain: [0.0; ER_COUNT],
            excursion: 0.0,
            lfo_phase: 0.0,
            taps: [Taps::SILENT; 2],
            geo: Geometry {
                size: 1.0,
                predelay: 0.0,
            },
            geo_next: Geometry {
                size: 1.0,
                predelay: 0.0,
            },
            geo_pending: None,
            fade: 0.0,
            fade_step: 1.0,
            fading: false,
            algorithm_pending: None,
            algorithm_gain: 1.0,
            algorithm_step: 1.0,
            smooth_a: 1.0,
            decay: Smoother::default(),
            damp_pole: Smoother::default(),
            hp_a: Smoother::default(),
            early: Smoother::default(),
            diffusion: Smoother::default(),
            width: Smoother::default(),
            mix: Smoother::default(),
            mod_depth: Smoother::default(),
            lfo_step: 0.0,
        };
        verb.build(sample_rate);
        verb.snap();
        verb
    }

    /// Rebuild every buffer for a new rate. Allocates; never called from the
    /// audio thread.
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        if !sample_rate.is_finite()
            || sample_rate <= 0.0
            || (sample_rate - self.sample_rate).abs() < 1.0
        {
            return;
        }
        self.build(sample_rate);
        self.snap();
    }

    #[must_use]
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    fn build(&mut self, sample_rate: f64) {
        let fs = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.sample_rate = fs;
        self.scale = fs / FS_REF;
        self.excursion = EXCURSION * self.scale;

        // Input diffusers: scaled by rate, never by size — the tank is the
        // plate's size and the diffusers are its signature.
        self.diffuser.clear();
        for (slot, base) in INPUT_DIFFUSER.iter().enumerate() {
            let len = (base * self.scale).max(4.0);
            self.diffuser_len[slot] = len;
            self.diffuser.push(Allpass::new(len.ceil() as usize + 8));
        }

        // The room's own input diffusion is Dattorro's at 60% of the length;
        // the hall uses only the first two, at a gentler coefficient, because
        // its ER section carries the density and more allpasses would smear
        // the attack.
        self.fdn_diffuser.clear();
        for (slot, base) in INPUT_DIFFUSER.iter().enumerate() {
            let len = (base * 0.6 * self.scale).max(4.0);
            self.fdn_diffuser_len[slot] = len;
            self.fdn_diffuser.push(Allpass::new(len.ceil() as usize + 8));
        }

        // The tank, sized for the largest size and the deepest modulation.
        self.tank_ap.clear();
        self.tank_del.clear();
        let headroom = 2.0 * self.excursion + 8.0;
        for (index, base) in TANK.iter().enumerate() {
            let len = base * self.scale;
            self.tank_len[index] = len;
            let capacity = (len * SIZE_MAX + headroom).ceil() as usize;
            if index % 4 == 0 || index % 4 == 2 {
                self.tank_ap.push(Allpass::new(capacity));
            } else {
                self.tank_del.push(Ring::new(capacity));
            }
        }

        // The FDN's eight lines and the allpass inside each of them, sized
        // for the hall — the room's are shorter and share them.
        self.fdn_line.clear();
        self.fdn_ap.clear();
        for (total_ms, ap_ms) in HALL_MS {
            let delay = (total_ms - ap_ms) * 0.001 * fs * SIZE_MAX + headroom;
            self.fdn_line.push(Ring::new(delay.ceil() as usize));
            let allpass = ap_ms * 0.001 * fs * SIZE_MAX + 8.0;
            self.fdn_ap.push(Allpass::new(allpass.ceil() as usize));
        }

        // The spring's dispersion chain. `K = fs/(2·F_c)` is the stretch that
        // puts the group-delay peak at `F_c`; the integer part is a delay
        // line and the leftover is a first-order allpass.
        let k = fs / (2.0 * SPRING_FC_HZ);
        let k1 = (k.round() as usize).max(2) - 1;
        let frac = (k - k1 as f64).clamp(0.4, 1.6);
        self.spring_k1 = k1;
        self.spring_a2 = (1.0 - frac) / (1.0 + frac);
        self.spring_low.clear();
        self.spring_high.clear();
        for _ in 0..SPRING_LOW_SECTIONS {
            self.spring_low.push(SpringSection::new(k1));
        }
        for _ in 0..SPRING_HIGH_SECTIONS {
            self.spring_high.push(SpringSection::new(k1));
        }

        // One buffer for the predelay and the early reflections, because the
        // taps read from it: the longest predelay plus the longest tap.
        let predelay_capacity =
            ((PREDELAY_MAX_SECONDS + ER_MAX_SECONDS * SIZE_MAX) * fs).ceil() as usize + 8;
        self.predelay = Ring::new(predelay_capacity);

        self.bandwidth_a = 1.0 - pole_at(BANDWIDTH_HZ, fs);
        self.smooth_a = 1.0 - (-1.0 / (SMOOTH_SECONDS * fs)).exp();
        self.reset();
        self.refresh_algorithm_tables();
        self.apply_params(true);
        self.refresh_taps();
    }

    /// Drop every tail. Real-time: fills the buffers it already owns.
    pub fn reset(&mut self) {
        for ap in &mut self.diffuser {
            ap.clear();
        }
        for ap in &mut self.fdn_diffuser {
            ap.clear();
        }
        for ap in &mut self.tank_ap {
            ap.clear();
        }
        for line in &mut self.tank_del {
            line.clear();
        }
        for line in &mut self.fdn_line {
            line.clear();
        }
        for ap in &mut self.fdn_ap {
            ap.clear();
        }
        for section in &mut self.spring_low {
            section.clear();
        }
        for section in &mut self.spring_high {
            section.clear();
        }
        self.predelay.clear();
        self.damp_z = [0.0; 2];
        self.hp_z = [0.0; 2];
        self.feedback = [0.0; 2];
        self.bandwidth_z = 0.0;
        self.fdn_damp_z = [0.0; FDN_LINES];
        self.fdn_hp_z = [0.0; FDN_LINES];
        self.spring_phase = 0;
        self.spring_high_hold = 0.0;
        self.lfo_phase = 0.0;
    }

    /// Snap every smoother and every crossfade to its destination.
    ///
    /// What [`crate::fx::reverb::Reverb::new`] and a rate change do, and what
    /// an effect being installed into a slot does: a session load sets the
    /// controls before the effect is in the signal path, and there is nothing
    /// audible to protect from the jump. Deliberately *not* what a program
    /// change does — that is [`Reverb::set_param_natural_immediate`], which
    /// crossfades in 50 ms so a patch change neither swoops nor clicks.
    pub fn snap(&mut self) {
        self.fading = false;
        self.geo_pending = None;
        self.geo = self.geo_next;
        if let Some(wanted) = self.algorithm_pending.take() {
            self.algorithm = wanted;
            self.refresh_algorithm_tables();
            self.refresh_coefficients();
            self.reset_tanks();
        }
        self.algorithm_gain = 1.0;
        self.refresh_taps();
        self.snap_smoothers();
    }

    /// The coefficient smoothers only, leaving the geometry crossfade alone.
    fn snap_smoothers(&mut self) {
        self.decay.snap(self.decay.target);
        self.damp_pole.snap(self.damp_pole.target);
        self.hp_a.snap(self.hp_a.target);
        self.early.snap(self.early.target);
        self.diffusion.snap(self.diffusion.target);
        self.width.snap(self.width.target);
        self.mix.snap(self.mix.target);
        self.mod_depth.snap(self.mod_depth.target);
    }

    // ── Parameters ──

    /// One control, in its own unit. Real-time safe: an algorithm change
    /// starts a crossfade and a geometry change starts a morph, neither of
    /// which allocates.
    pub fn set_param_natural(&mut self, index: usize, value: f32) {
        self.write_param(index, value);
        self.apply_params(false);
    }

    /// The same, applied the way a program change should be: one 50 ms fade
    /// straight to the destination rather than a walk through 5% steps.
    pub fn set_param_natural_immediate(&mut self, index: usize, value: f32) {
        self.write_param(index, value);
        self.apply_params(true);
    }

    fn write_param(&mut self, index: usize, value: f32) {
        let Some(info) = natural_param(index) else {
            return;
        };
        if !value.is_finite() {
            return;
        }
        self.params[index] = value.clamp(info.min, info.max);
    }

    /// A control's current value, in its own unit.
    #[must_use]
    pub fn param_natural(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    #[must_use]
    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// The circulation time the RT60 map is written against, at the current
    /// size and algorithm. Public because a test that checks the map has to
    /// know which loop it is checking.
    #[must_use]
    pub fn loop_seconds(&self) -> f64 {
        self.loop_seconds_for(self.geo.size, self.algorithm)
    }

    fn loop_seconds_for(&self, size: f64, algorithm: Algorithm) -> f64 {
        if algorithm.uses_plate_tank() {
            T_LOOP_SECONDS * size
        } else {
            // An FDN's mean circulation is one line, not the sum of them.
            let mean: f64 =
                self.fdn_ms.iter().map(|(total, _)| total).sum::<f64>() / FDN_LINES as f64;
            mean * 0.001 * size
        }
    }

    /// Turn the stored controls into working coefficients.
    fn apply_params(&mut self, immediate: bool) {
        let wanted = Algorithm::from_index(self.params[PARAM_ALGORITHM].round().max(0.0) as usize);
        if wanted != self.algorithm {
            if immediate {
                self.algorithm = wanted;
                self.algorithm_pending = None;
                self.algorithm_gain = 1.0;
                self.refresh_algorithm_tables();
                self.reset();
            } else {
                self.algorithm_pending = Some(wanted);
                self.algorithm_step =
                    1.0 / (ALGORITHM_FADE_SECONDS * self.sample_rate).max(1.0);
            }
        }

        // Geometry. Quantised to 5% so that a knob dragged across its travel
        // is a sequence of soft flams rather than one long comb.
        let size_raw = f64::from(self.params[PARAM_SIZE]).clamp(SIZE_MIN, SIZE_MAX);
        let size = if immediate {
            size_raw
        } else {
            (size_raw / SIZE_QUANTUM).round() * SIZE_QUANTUM
        };
        let predelay = f64::from(self.params[PARAM_PREDELAY_MS]).max(0.0) * 0.001 * self.sample_rate;
        let target = Geometry { size, predelay };
        if target != self.geo_next {
            let seconds = if immediate {
                PROGRAM_FADE_SECONDS
            } else {
                MORPH_FADE_SECONDS
            };
            if self.fading {
                // A target arriving mid-fade latches and is applied when the
                // fade completes; re-aiming a running crossfade is how a
                // knob-drag turns into a stutter.
                self.geo_pending = Some(target);
            } else {
                self.geo_next = target;
                self.fade = 0.0;
                self.fade_step = 1.0 / (seconds * self.sample_rate).max(1.0);
                self.fading = true;
            }
        }

        self.refresh_taps();
        self.refresh_coefficients();
        if immediate {
            // A program change lands on its coefficients at once — nobody
            // wants a patch that glides — but its *geometry* still crosses
            // over, because re-indexing every delay under a running tail is
            // a click that then recirculates.
            self.snap_smoothers();
        }
    }

    /// The per-algorithm delay tables and ER taps. Cheap enough to redo on
    /// any change, and it never allocates: the vectors are already the right
    /// length.
    fn refresh_algorithm_tables(&mut self) {
        self.fdn_ms = match self.algorithm {
            Algorithm::Room => ROOM_MS,
            _ => HALL_MS,
        };
        let (lo, hi) = er_window(self.algorithm);
        let span = (hi - lo) / (MOORER_LAST_MS - MOORER_FIRST_MS);
        let mut energy = 0.0f64;
        for (index, (ms, gain)) in MOORER_TAPS.iter().enumerate() {
            let mapped = lo + (ms - MOORER_FIRST_MS) * span;
            let left = mapped * 0.001 * self.sample_rate;
            // The right channel takes the same gains with the tap times
            // jittered by ±5%: that decorrelates the two while keeping one
            // pattern's character, rather than running two different rooms.
            let jitter = 1.0 + 0.05 * (((index as f64) * 0.618_033_988_749_9).fract() * 2.0 - 1.0);
            // Fractional, like every other read here: rounding an early
            // tap to a whole sample is an 11 µs error at 44.1 kHz and 5 µs at
            // 96, which is 14° of phase at 3.5 kHz — enough for eighteen taps
            // summing to interfere differently at the two rates and for the
            // rate fingerprint to read a 2.4% level difference that is not a
            // property of the reverb.
            self.er_l[index] = left.max(2.0);
            self.er_r[index] = (left * jitter).max(2.0);
            self.er_gain[index] = *gain;
            energy += gain * gain;
        }
        // Normalised so `Σg² = 1`, which is what makes `early` mean the same
        // thing at every size and in every algorithm.
        let norm = 1.0 / energy.sqrt();
        for gain in &mut self.er_gain {
            *gain *= norm;
        }
    }

    /// Work out every quasi-static read kernel for both geometries.
    ///
    /// Called when the geometry or the algorithm moves and never per sample.
    /// Bounded work, fixed-size arrays, no allocation — a crossfade that
    /// completes inside a callback runs this and it is still real-time.
    fn refresh_taps(&mut self) {
        let fs = self.sample_rate;
        let geometries = [self.geo, self.geo_next];
        for (slot, geo) in geometries.into_iter().enumerate() {
            let taps = &mut self.taps[slot];
            taps.predelay = FracTap::at(geo.predelay);
            for index in 0..4 {
                taps.diffuser[index] = FracTap::at(self.diffuser_len[index]);
            }
            for index in 0..8 {
                taps.tank[index] = FracTap::at(self.tank_len[index] * geo.size);
            }
            for (slot_index, &(line, position, _)) in TAPS_L.iter().enumerate() {
                let _ = line;
                taps.out_l[slot_index] = FracTap::at(position * self.scale * geo.size + 1.0);
            }
            for (slot_index, &(line, position, _)) in TAPS_R.iter().enumerate() {
                let _ = line;
                taps.out_r[slot_index] = FracTap::at(position * self.scale * geo.size + 1.0);
            }
            for index in 0..ER_COUNT {
                taps.er_l[index] = FracTap::at(geo.predelay + self.er_l[index] * geo.size);
                taps.er_r[index] = FracTap::at(geo.predelay + self.er_r[index] * geo.size);
            }
            for index in 0..FDN_LINES {
                let (_, ap_ms) = self.fdn_ms[index];
                taps.fdn_ap[index] = FracTap::at(ap_ms * 0.001 * fs * geo.size);
            }
        }
    }

    fn refresh_coefficients(&mut self) {
        let rt60 = f64::from(self.params[PARAM_DECAY_S]).max(0.05);
        let loops = self.loop_seconds_for(self.geo_next.size, self.pending_algorithm());
        self.decay.target = decay_for_rt60(rt60, loops);
        self.damp_pole.target = tpt_gain(f64::from(self.params[PARAM_DAMP_HZ]), self.sample_rate);
        self.hp_a.target = 1.0 - pole_at(f64::from(self.params[PARAM_LOW_CUT_HZ]), self.sample_rate);
        self.early.target = f64::from(self.params[PARAM_EARLY]) * 0.01;
        self.diffusion.target = f64::from(self.params[PARAM_DIFFUSION]) * 0.01;
        self.width.target = f64::from(self.params[PARAM_WIDTH]) * 0.01;
        self.mix.target = f64::from(self.params[PARAM_MIX]) * 0.01;
        // Detuning accumulates as the square root of the number of passes,
        // and the number of passes is `RT60/T_loop`. Costello: *"For long
        // decays, you may wish to back off on the modulation depth, as the
        // sound will travel through the modulators many more times."*
        let depth = f64::from(self.params[PARAM_MOD_DEPTH]) * 0.01;
        self.mod_depth.target = depth * (1.8 / rt60).sqrt().min(1.0);
        self.lfo_step = TAU * f64::from(self.params[PARAM_MOD_RATE_HZ]) / self.sample_rate;
    }

    fn pending_algorithm(&self) -> Algorithm {
        self.algorithm_pending.unwrap_or(self.algorithm)
    }

    // ── Rendering ──

    /// One block, rewritten in place. Real-time: no allocation, no locks.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len());
        for i in 0..frames {
            let (l, r) = self.process_sample(left[i], right[i]);
            left[i] = l;
            right[i] = r;
        }
    }

    /// One frame.
    ///
    /// At `mix == 0` this still runs the whole tank — a tail must not glitch
    /// when the knob comes back — and then returns the input *itself*, not
    /// the input plus zero times the wet. Bit-identical dry is a property of
    /// the control flow rather than of floating-point luck, and `−0.0 + 0.0`
    /// is `+0.0`, which is why the difference matters.
    #[inline]
    pub fn process_sample(&mut self, left: f32, right: f32) -> (f32, f32) {
        let a = self.smooth_a;
        let decay = self.decay.advance(a);
        let damp = self.damp_pole.advance(a);
        let hp_a = self.hp_a.advance(a);
        let early = self.early.advance(a);
        let diffusion = self.diffusion.advance(a);
        let width = self.width.advance(a);
        let mix = self.mix.advance(a);
        let depth = self.mod_depth.advance(a);

        self.advance_fades();

        let (ga, gb) = self.fade_gains();
        let x = (f64::from(left) + f64::from(right)) * 0.5;

        // Predelay, and the early-reflection taps that read from it.
        self.predelay.write(x);
        let delayed = if self.fading {
            self.predelay
                .read_fade(&self.taps[0].predelay, &self.taps[1].predelay, ga, gb)
        } else {
            self.predelay.read(&self.taps[0].predelay)
        };
        let (er_l, er_r) = if early > 0.0 {
            self.early_reflections(ga, gb)
        } else {
            (0.0, 0.0)
        };

        let (mut wet_l, mut wet_r) = if self.algorithm.uses_plate_tank() {
            self.plate_sample(delayed, decay, damp, hp_a, diffusion, depth, ga, gb)
        } else {
            self.fdn_sample(delayed, damp, hp_a, diffusion, depth, ga, gb)
        };

        wet_l += er_l * early;
        wet_r += er_r * early;

        // Mid/side on the wet only, never above unity: widening past the
        // source is a different effect and it wrecks mono compatibility. The
        // early reflections are inside it, not beside it, so `width = 0` is
        // an exactly mono wet — which is a useful thing to be able to assert.
        let mid = (wet_l + wet_r) * 0.5;
        let side = (wet_l - wet_r) * 0.5 * width;
        let trim = WET_TRIM * self.algorithm_gain;
        wet_l = (mid + side) * trim;
        wet_r = (mid - side) * trim;

        if mix == 0.0 {
            return (left, right);
        }
        // A crossfade, not an addition. `dry + wet·mix` looks like the same
        // control and is not: at 100% it is *dry plus a full reverb*, so a
        // send bus set to "fully wet" returns the source a second time a few
        // milliseconds late — which is the phasey-send trap, and the reason a
        // player who tries a send once never tries it again. Linear rather
        // than equal-power, because a reverb's wet and dry are correlated in
        // the early part and equal-power over-sums them.
        //
        // The `mix == 0` case above is what keeps the null exact: the
        // arithmetic here would give `dry·1.0 + wet·0.0`, and `−0.0 + 0.0` is
        // `+0.0`.
        let dry = 1.0 - mix as f32;
        // The saturator is outside the tank, so the loop stays linear and
        // RT60 stays predictable; it exists for the sustained-resonance case
        // an impulse test cannot see.
        let out_l = soft_saturate(wet_l as f32).mul_add(mix as f32, left * dry);
        let out_r = soft_saturate(wet_r as f32).mul_add(mix as f32, right * dry);
        (out_l, out_r)
    }

    /// Walk the geometry and algorithm crossfades one sample.
    #[inline]
    fn advance_fades(&mut self) {
        if self.fading {
            self.fade += self.fade_step;
            if self.fade >= 1.0 {
                self.fade = 0.0;
                self.fading = false;
                self.geo = self.geo_next;
                if let Some(pending) = self.geo_pending.take() {
                    if pending != self.geo_next {
                        self.geo_next = pending;
                        self.fading = true;
                    }
                }
                self.refresh_taps();
                self.refresh_coefficients();
            }
        }
        if let Some(wanted) = self.algorithm_pending {
            self.algorithm_gain -= self.algorithm_step;
            if self.algorithm_gain <= 0.0 {
                self.algorithm_gain = 0.0;
                self.algorithm = wanted;
                self.algorithm_pending = None;
                self.refresh_algorithm_tables();
                self.refresh_taps();
                self.refresh_coefficients();
                self.reset_tanks();
            }
        } else if self.algorithm_gain < 1.0 {
            self.algorithm_gain = (self.algorithm_gain + self.algorithm_step).min(1.0);
        }
    }

    /// Silence the tanks without touching the predelay, so that an algorithm
    /// change does not also swallow the signal that is already on its way in.
    fn reset_tanks(&mut self) {
        for ap in &mut self.tank_ap {
            ap.clear();
        }
        for line in &mut self.tank_del {
            line.clear();
        }
        for line in &mut self.fdn_line {
            line.clear();
        }
        for ap in &mut self.fdn_ap {
            ap.clear();
        }
        for section in &mut self.spring_low {
            section.clear();
        }
        for section in &mut self.spring_high {
            section.clear();
        }
        self.damp_z = [0.0; 2];
        self.hp_z = [0.0; 2];
        self.feedback = [0.0; 2];
        self.fdn_damp_z = [0.0; FDN_LINES];
        self.fdn_hp_z = [0.0; FDN_LINES];
        self.spring_high_hold = 0.0;
    }

    /// Equal-power gains for the geometry crossfade.
    #[inline]
    fn fade_gains(&self) -> (f64, f64) {
        if self.fading {
            let t = self.fade * std::f64::consts::FRAC_PI_2;
            (t.cos(), t.sin())
        } else {
            (1.0, 0.0)
        }
    }

    #[inline]
    fn early_reflections(&self, ga: f64, gb: f64) -> (f64, f64) {
        let mut l = 0.0f64;
        let mut r = 0.0f64;
        let (a, b) = (&self.taps[0], &self.taps[1]);
        if self.fading {
            for index in 0..ER_COUNT {
                let gain = self.er_gain[index];
                l += gain * self.predelay.read_fade(&a.er_l[index], &b.er_l[index], ga, gb);
                r += gain * self.predelay.read_fade(&a.er_r[index], &b.er_r[index], ga, gb);
            }
        } else {
            for index in 0..ER_COUNT {
                let gain = self.er_gain[index];
                l += gain * self.predelay.read(&a.er_l[index]);
                r += gain * self.predelay.read(&a.er_r[index]);
            }
        }
        (l, r)
    }

    // ── The plate ──

    #[allow(clippy::too_many_arguments)]
    #[inline]
    fn plate_sample(
        &mut self,
        input: f64,
        decay: f64,
        damp: f64,
        hp_a: f64,
        diffusion: f64,
        depth: f64,
        ga: f64,
        gb: f64,
    ) -> (f64, f64) {
        // The input bandwidth filter, in its own convention: `bandwidth` is
        // the feed-forward coefficient, so higher is brighter.
        self.bandwidth_z = flush(self.bandwidth_z + self.bandwidth_a * (input - self.bandwidth_z));
        let mut x = self.bandwidth_z;

        if self.algorithm == Algorithm::Spring {
            x = self.spring_input(x);
        } else {
            // Dattorro's guidance is that optimum allpass diffusion lies
            // *"closer to |0.5| than to the extreme values"* and that
            // coefficients near unity *"produce buzzing that is local to the
            // afflicted all-pass filter"*, so the knob scales toward the
            // published values and stops at 0.85.
            let id1 = (INPUT_DIFFUSION_1 * diffusion).min(0.85);
            let id2 = (INPUT_DIFFUSION_2 * diffusion).min(0.85);
            // The diffuser lengths never move, so neither do their kernels.
            let taps = self.taps[0].diffuser;
            x = self.diffuser[0].tick_tap(x, id1, &taps[0]);
            x = self.diffuser[1].tick_tap(x, id1, &taps[1]);
            x = self.diffuser[2].tick_tap(x, id2, &taps[2]);
            x = self.diffuser[3].tick_tap(x, id2, &taps[3]);
        }

        let dd1 = (DECAY_DIFFUSION_1 * diffusion).min(0.85);
        // Not independent: the paper defines it as `decay + 0.15`, floored at
        // 0.25 and ceilinged at 0.50.
        let dd2 = ((decay + 0.15).clamp(0.25, 0.50) * diffusion).min(0.85);

        let excursion = self.excursion * depth;
        let (sin, cos) = self.lfo_phase.sin_cos();
        self.lfo_phase += self.lfo_step;
        if self.lfo_phase >= TAU {
            self.lfo_phase -= TAU;
        }

        // Left branch, then right, in that order: the figure eight is one
        // serial loop and the right branch consumes what the left branch just
        // made this very sample.
        self.plate_branch(0, x, sin, decay, damp, hp_a, dd1, dd2, excursion, ga, gb);
        self.plate_branch(1, x, cos, decay, damp, hp_a, dd1, dd2, excursion, ga, gb);

        (
            self.plate_taps(&TAPS_L, false, ga, gb),
            self.plate_taps(&TAPS_R, true, ga, gb),
        )
    }

    /// One side of the figure eight: modulated allpass, delay, damping, LF
    /// shelf, `×decay`, allpass, delay, `×decay` into the other side.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    fn plate_branch(
        &mut self,
        branch: usize,
        x: f64,
        modulator: f64,
        decay: f64,
        damp: f64,
        hp_a: f64,
        dd1: f64,
        dd2: f64,
        excursion: f64,
        ga: f64,
        gb: f64,
    ) {
        let base = branch * 4;
        let (size_a, size_b) = (self.geo.size, self.geo_next.size);
        let fading = self.fading;
        let sway = excursion * (1.0 + modulator);
        let input = x + self.feedback[1 - branch];

        let ap1 = &mut self.tank_ap[branch * 2];
        let mut v = if fading {
            ap1.tick_fade(
                input,
                -dd1,
                self.tank_len[base] * size_a + sway,
                self.tank_len[base] * size_b + sway,
                ga,
                gb,
            )
        } else {
            ap1.tick(input, -dd1, self.tank_len[base] * size_a + sway)
        };

        let del1 = &mut self.tank_del[branch * 2];
        let read = if fading {
            del1.read_fade(
                &self.taps[0].tank[base + 1],
                &self.taps[1].tank[base + 1],
                ga,
                gb,
            )
        } else {
            del1.read(&self.taps[0].tank[base + 1])
        };
        del1.write(v);

        // The damping filter. Dattorro's own is a plain one-pole whose
        // *pole* is the coefficient, so higher is darker — writing it as
        // `state += damp·(x − state)` instead puts the corner at 3.8 Hz and
        // pins RT60 near 0.55 s whatever `decay` says, a bug that reads as a
        // decay-mapping bug for days. This is the trapezoidal form of the
        // same filter, for the rate-invariance reason in [`tpt_gain`].
        let step = (read - self.damp_z[branch]) * damp;
        v = step + self.damp_z[branch];
        self.damp_z[branch] = flush(v + step);

        // The low-frequency shelf the paper does not have. Its tank has unity
        // DC loop gain and no bass control at all, so long settings turn
        // boomy. A one-pole high-pass per branch fixes it — and the denormal
        // guard is deliberately built not to depend on it, which is why the
        // injection alternates per block rather than sitting at DC.
        self.hp_z[branch] = flush(self.hp_z[branch] + hp_a * (v - self.hp_z[branch]));
        v -= self.hp_z[branch];
        v *= decay;

        let ap2 = &mut self.tank_ap[branch * 2 + 1];
        v = if fading {
            ap2.tick_tap_fade(
                v,
                dd2,
                &self.taps[0].tank[base + 2],
                &self.taps[1].tank[base + 2],
                ga,
                gb,
            )
        } else {
            ap2.tick_tap(v, dd2, &self.taps[0].tank[base + 2])
        };

        let del2 = &mut self.tank_del[branch * 2 + 1];
        let tail = if fading {
            del2.read_fade(
                &self.taps[0].tank[base + 3],
                &self.taps[1].tank[base + 3],
                ga,
                gb,
            )
        } else {
            del2.read(&self.taps[0].tank[base + 3])
        };
        del2.write(v);
        self.feedback[branch] = flush(tail * decay);
    }

    /// One channel's seven output taps, gathered after every write, so a tap
    /// `p` samples into a line is `tap(p + 1)`.
    #[inline]
    fn plate_taps(&self, table: &[(usize, f64, f64); 7], right: bool, ga: f64, gb: f64) -> f64 {
        let mut sum = 0.0f64;
        for (slot, &(line, _, sign)) in table.iter().enumerate() {
            let tap_a = if right { &self.taps[0].out_r[slot] } else { &self.taps[0].out_l[slot] };
            let value = if self.fading {
                let tap_b =
                    if right { &self.taps[1].out_r[slot] } else { &self.taps[1].out_l[slot] };
                ga * self.tank_read(line, tap_a) + gb * self.tank_read(line, tap_b)
            } else {
                self.tank_read(line, tap_a)
            };
            sum += sign * TAP_GAIN * value;
        }
        sum
    }

    /// A read on tank line `index`, whichever of the two vectors it lives in.
    #[inline]
    fn tank_read(&self, index: usize, tap: &FracTap) -> f64 {
        let branch = index / 4;
        match index % 4 {
            0 => self.tank_ap[branch * 2].line.read(tap),
            1 => self.tank_del[branch * 2].read(tap),
            2 => self.tank_ap[branch * 2 + 1].line.read(tap),
            _ => self.tank_del[branch * 2 + 1].read(tap),
        }
    }

    // ── The spring's input stage ──

    /// Two parallel dispersion cascades in place of the four input diffusers.
    ///
    /// The low chirp sweeps 2.7 → 49.6 ms *upward* across the spectrum and
    /// the high chirp 18.1 → 1.1 ms *downward*, an 18:1 ratio; the 49.6 ms
    /// peak nearly fills the 56 ms echo period, which is why spring chirps in
    /// a spectrogram very nearly touch the next one. That is the single most
    /// recognisable thing about the sound.
    ///
    /// The high cascade runs at `fs/4`, as the paper specifies, which is what
    /// keeps three hundred first-order sections affordable.
    #[inline]
    fn spring_input(&mut self, x: f64) -> f64 {
        let (k1, a2) = (self.spring_k1, self.spring_a2);
        let mut low = x;
        for section in &mut self.spring_low {
            low = section.tick(low, SPRING_LOW_A, k1, a2);
        }
        self.spring_high_hold = flush(self.spring_high_hold);
        self.spring_phase += 1;
        if self.spring_phase >= 4 {
            self.spring_phase = 0;
            let mut high = x;
            for section in &mut self.spring_high {
                high = section.tick(high, SPRING_HIGH_A, k1, a2);
            }
            self.spring_high_hold = high;
        }
        low + self.spring_high_hold * SPRING_HIGH_LEVEL
    }

    // ── The FDN ──

    #[allow(clippy::too_many_arguments)]
    #[inline]
    fn fdn_sample(
        &mut self,
        input: f64,
        damp: f64,
        hp_a: f64,
        diffusion: f64,
        depth: f64,
        ga: f64,
        gb: f64,
    ) -> (f64, f64) {
        self.bandwidth_z = flush(self.bandwidth_z + self.bandwidth_a * (input - self.bandwidth_z));
        let mut x = self.bandwidth_z;

        // The room's density needs the help; the hall's comes from its ER
        // section, and more allpasses there would smear the attack. Costello:
        // *"Use fewer series allpasses at the input... Many 'hall' algorithms
        // use this trick."*
        let taps = if self.algorithm == Algorithm::Room { 4 } else { 2 };
        for slot in 0..taps {
            let coefficient = if self.algorithm == Algorithm::Room {
                (if slot < 2 { INPUT_DIFFUSION_1 } else { INPUT_DIFFUSION_2 } * diffusion).min(0.85)
            } else {
                (0.6 * diffusion).min(0.6)
            };
            x = self.fdn_diffuser[slot].tick(x, coefficient, self.fdn_diffuser_len[slot]);
        }

        let excursion = self.excursion * depth;
        let (sin, cos) = self.lfo_phase.sin_cos();
        self.lfo_phase += self.lfo_step;
        if self.lfo_phase >= TAU {
            self.lfo_phase -= TAU;
        }
        // Eight decorrelated modulators from one quadrature pair, so the LFO
        // costs one `sin_cos` a sample rather than eight.
        const OCT: f64 = std::f64::consts::FRAC_1_SQRT_2;
        let modulator = [
            sin,
            cos,
            -sin,
            -cos,
            (sin + cos) * OCT,
            (sin - cos) * OCT,
            (cos - sin) * OCT,
            -(sin + cos) * OCT,
        ];

        let rt60 = f64::from(self.params[PARAM_DECAY_S]).max(0.05);
        let fs = self.sample_rate;
        let (size_a, size_b) = (self.geo.size, self.geo_next.size);
        let mut state = [0.0f64; FDN_LINES];
        let mut out = [0.0f64; FDN_LINES];
        for index in 0..FDN_LINES {
            let (total_ms, ap_ms) = self.fdn_ms[index];
            let delay = (total_ms - ap_ms) * 0.001 * fs;
            let sway = excursion * (1.0 + modulator[index]);
            let length = if self.fading {
                ga * self.fdn_line[index].tap_cubic(delay * size_a + sway)
                    + gb * self.fdn_line[index].tap_cubic(delay * size_b + sway)
            } else {
                self.fdn_line[index].tap_cubic(delay * size_a + sway)
            };
            out[index] = length;

            // Jot's per-line gain, orthogonalized: with
            // `H(z) = g·(1−p)/(1 − p·z⁻¹)` the DC gain is exactly `g` for any
            // stable pole, so the decay knob sets `g` and the damping knob
            // sets `p` and moving one does not shift the other. With a plain
            // one-pole they interact, and the player experiences it as a
            // decay knob that lies.
            //
            // The gain is written against the *total* line — the allpass is
            // inside the loop, so its delay circulates too.
            let samples = total_ms * 0.001 * fs * size_a;
            let gain = (10.0f64).powf(-3.0 * samples / (rt60 * fs)) * FDN_NORM;
            let step = (length - self.fdn_damp_z[index]) * damp;
            let damped = step + self.fdn_damp_z[index];
            self.fdn_damp_z[index] = flush(damped + step);
            let mut v = damped * gain;
            self.fdn_hp_z[index] =
                flush(self.fdn_hp_z[index] + hp_a * (v - self.fdn_hp_z[index]));
            v -= self.fdn_hp_z[index];

            // zita's in-loop allpass, sign alternating per line.
            let coefficient = if index % 2 == 0 {
                FDN_AP_COEFFICIENT * diffusion
            } else {
                -FDN_AP_COEFFICIENT * diffusion
            };
            v = if self.fading {
                self.fdn_ap[index].tick_tap_fade(
                    v,
                    coefficient,
                    &self.taps[0].fdn_ap[index],
                    &self.taps[1].fdn_ap[index],
                    ga,
                    gb,
                )
            } else {
                self.fdn_ap[index].tick_tap(v, coefficient, &self.taps[0].fdn_ap[index])
            };
            state[index] = v;
        }

        fwht8(&mut state);

        for index in 0..FDN_LINES {
            let scattered = FDN_SIGNS[index] * state[FDN_PERM[index]];
            self.fdn_line[index].write(scattered + FDN_INPUT_SIGNS[index] * x * FDN_NORM);
        }

        // zita's output butterfly: sum and difference of two lines. Two
        // uncorrelated equal-power signals give an uncorrelated equal-power
        // pair, which is exact decorrelation for two adds and no multiplies.
        // Freeverb's answer to the same problem is to make every one of its
        // twelve lines 23 samples longer on the right.
        let gain = 0.37
            * if self.algorithm == Algorithm::Room {
                ROOM_OUTPUT
            } else {
                HALL_OUTPUT
            };
        (gain * (out[0] + out[1]), gain * (out[0] - out[1]))
    }
}

impl Default for Reverb {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

// ---------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------

/// Reverberation time from a rendered decay, by Schroeder backward
/// integration and a least-squares T30 fit.
///
/// The 1965 method: integrate the squared impulse response from the end
/// backwards, which removes the fluctuation that makes a raw decay curve
/// unfittable, then fit a line to the part of it between −5 and −35 dB. ISO
/// 3382 calls that T30, and it is what a "reverb time" number means anywhere
/// it is quoted.
///
/// Returns `None` when the record never falls far enough to fit — a tail that
/// has not finished inside the render is a measurement that does not exist,
/// not a number to report anyway.
#[must_use]
pub fn rt60_t30(tail: &[f32], sample_rate: f64) -> Option<f64> {
    schroeder_fit(tail, sample_rate, -5.0, -35.0)
}

/// The same fit over −5 to −25 dB. Comparing the two is how ISO 3382-2's
/// curvature test decides whether a decay is actually exponential.
#[must_use]
pub fn rt60_t20(tail: &[f32], sample_rate: f64) -> Option<f64> {
    schroeder_fit(tail, sample_rate, -5.0, -25.0)
}

fn schroeder_fit(tail: &[f32], sample_rate: f64, from_db: f64, to_db: f64) -> Option<f64> {
    if tail.len() < 16 || sample_rate <= 0.0 {
        return None;
    }
    let mut curve = vec![0.0f64; tail.len()];
    let mut running = 0.0f64;
    for (index, sample) in tail.iter().enumerate().rev() {
        running += f64::from(*sample) * f64::from(*sample);
        curve[index] = running;
    }
    let total = curve[0];
    if total <= 0.0 {
        return None;
    }
    for value in &mut curve {
        *value = 10.0 * (*value / total).max(1.0e-300).log10();
    }
    let start = curve.iter().position(|v| *v <= from_db)?;
    let end = curve.iter().position(|v| *v <= to_db)?;
    if end <= start + 8 {
        return None;
    }
    // Least squares on (time, dB).
    let n = (end - start) as f64;
    let (mut sx, mut sy, mut sxx, mut sxy) = (0.0f64, 0.0, 0.0, 0.0);
    for (index, &y) in curve.iter().enumerate().take(end).skip(start) {
        let t = index as f64 / sample_rate;
        sx += t;
        sy += y;
        sxx += t * t;
        sxy += t * y;
    }
    let denominator = n * sxx - sx * sx;
    if denominator.abs() < 1.0e-18 {
        return None;
    }
    let slope = (n * sxy - sx * sy) / denominator;
    if slope >= 0.0 {
        return None;
    }
    Some(-60.0 / slope)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const FS: f64 = 48_000.0;

    /// A reverb at its factory settings, at `fs`, wet only, settled.
    pub(crate) fn wet_only(fs: f64) -> Reverb {
        let mut verb = Reverb::new(fs);
        verb.set_param_natural_immediate(PARAM_MIX, 100.0);
        verb
    }

    /// Drive an impulse in and keep `seconds` of output.
    ///
    /// Every caller here runs at 100% wet, where the mix crossfade has
    /// already taken the dry out, so what comes back is the wet and nothing
    /// else.
    pub(crate) fn impulse(verb: &mut Reverb, seconds: f64) -> (Vec<f32>, Vec<f32>) {
        verb.snap();
        let frames = (seconds * verb.sample_rate()) as usize;
        let mut left = Vec::with_capacity(frames);
        let mut right = Vec::with_capacity(frames);
        for _ in 0..frames {
            let x = if left.is_empty() { 1.0f32 } else { 0.0 };
            let (l, r) = verb.process_sample(x, x);
            left.push(l);
            right.push(r);
        }
        (left, right)
    }

    /// A quarter-second of a 220 Hz sine at −12 dBFS, then silence: `seconds`
    /// of wet, dry removed.
    ///
    /// The excitation for anything compared *across sample rates*. An impulse
    /// carries the same energy at every rate but spreads it over a different
    /// number of samples, so its RMS is `1/√fs` by construction and a
    /// fingerprint built on it would report a 47% difference between 22.05
    /// and 48 kHz that says nothing about the reverb. A tone burst carries
    /// the same energy *per second* at every rate.
    pub(crate) fn burst(verb: &mut Reverb, seconds: f64) -> (Vec<f32>, Vec<f32>) {
        verb.snap();
        let fs = verb.sample_rate();
        let frames = (seconds * fs) as usize;
        let driven = (0.25 * fs) as usize;
        let mut left = Vec::with_capacity(frames);
        let mut right = Vec::with_capacity(frames);
        for index in 0..frames {
            let x = if index < driven {
                // Six tones rather than one. A single 220 Hz sine excites
                // very few of a 232 ms network's modes, so which of them it
                // happens to land between moves with the interpolator's
                // fractions and the fingerprint reads a rate dependence that
                // is not there. A chord averages over the modal structure.
                let t = index as f64 / fs;
                let sum: f64 = [110.0, 220.0, 440.0, 880.0, 1760.0, 3520.0]
                    .iter()
                    .map(|hz| (TAU * hz * t).sin())
                    .sum();
                (0.25 * sum / 6.0) as f32
            } else {
                0.0
            };
            let (l, r) = verb.process_sample(x, x);
            left.push(l);
            right.push(r);
        }
        (left, right)
    }

    pub(crate) fn peak(x: &[f32]) -> f32 {
        x.iter().map(|v| v.abs()).fold(0.0f32, f32::max)
    }

    pub(crate) fn rms(x: &[f32]) -> f64 {
        if x.is_empty() {
            return 0.0;
        }
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64).sqrt()
    }

    pub(crate) fn correlation(a: &[f32], b: &[f32], lag: isize) -> f64 {
        let n = a.len().min(b.len());
        let (mut num, mut da, mut db) = (0.0f64, 0.0f64, 0.0f64);
        for (i, sample) in a.iter().enumerate().take(n) {
            let j = i as isize + lag;
            if j < 0 || j as usize >= n {
                continue;
            }
            let x = f64::from(*sample);
            let y = f64::from(b[j as usize]);
            num += x * y;
            da += x * x;
            db += y * y;
        }
        num / (da.sqrt() * db.sqrt()).max(1.0e-30)
    }

    pub(crate) fn max_correlation(a: &[f32], b: &[f32], span: isize) -> f64 {
        let mut best = 0.0f64;
        let step = (span / 32).max(1);
        let mut lag = -span;
        while lag <= span {
            best = best.max(correlation(a, b, lag).abs());
            lag += step;
        }
        best
    }

    /// Abel–Huang normalised echo density in a Hann window centred on `at`.
    ///
    /// `η` is the fraction of samples in the window exceeding one standard
    /// deviation, normalised so Gaussian noise scores exactly 1.0. Three
    /// traps live in the definition: `σ` is `sqrt(Σ w·h²)` — a windowed RMS
    /// assuming zero mean, not a mean-subtracted `std`; the same weights
    /// appear in `σ` and in the count; and `η` must not be clamped at 1,
    /// because legitimate overshoot is what the late field reads as.
    pub(crate) fn echo_density(x: &[f32], sample_rate: f64, at: f64) -> f64 {
        const WINDOW_SECONDS: f64 = 0.020;
        let half = (WINDOW_SECONDS * 0.5 * sample_rate) as usize;
        let centre = (at * sample_rate) as usize;
        let start = centre.saturating_sub(half);
        let end = (centre + half).min(x.len());
        if end <= start + 8 {
            return 0.0;
        }
        let n = end - start;
        let weights: Vec<f64> = (0..n)
            .map(|i| 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos())
            .collect();
        let total: f64 = weights.iter().sum();
        let sigma = (0..n)
            .map(|i| weights[i] * f64::from(x[start + i]) * f64::from(x[start + i]))
            .sum::<f64>()
            .sqrt()
            / total.sqrt();
        if sigma <= 0.0 {
            return 0.0;
        }
        let counted: f64 = (0..n)
            .filter(|i| f64::from(x[start + i]).abs() > sigma)
            .map(|i| weights[i])
            .sum();
        counted / total / 0.317_310_507_863_206_4
    }

    /// The shape half of the rate fingerprint: RMS over 0.0–0.5 s and over
    /// 0.5–1.0 s of a burst render.
    ///
    /// Two fixed *time* windows rather than "the two halves of the record",
    /// because a tail that has fallen 50 dB by the second half is measuring
    /// its own last few samples and the number stops being about the reverb.
    pub(crate) fn shape_windows(x: &[f32], sample_rate: f64) -> (f64, f64) {
        let half = (sample_rate * 0.5) as usize;
        let one = (sample_rate) as usize;
        if x.len() < one {
            return (rms(x), 0.0);
        }
        (rms(&x[..half]), rms(&x[half..one]))
    }

    /// A deterministic pseudo-random stereo signal, for the null tests.
    ///
    /// Includes the two zeros and the two smallest subnormals on purpose: a
    /// dry path that is "multiplied by one" gets `−0.0 + 0.0 = +0.0` wrong,
    /// and a dry path that is genuinely untouched does not.
    pub(crate) fn awkward_signal(len: usize) -> Vec<f32> {
        let mut state = 0x2545_f491u32;
        let mut out: Vec<f32> = (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((state >> 8) as f32 / 8_388_608.0 - 1.0) * 0.4
            })
            .collect();
        out.extend([0.0, -0.0, f32::MIN_POSITIVE, -f32::MIN_POSITIVE, 1.0, -1.0]);
        out
    }

    /// The three-number aggregate fingerprint the rate comparison uses:
    /// level, spectrum and shape. Never a hash — CI runs three libms.
    fn fingerprint(verb: &mut Reverb) -> [f64; 3] {
        let fs = verb.sample_rate();
        let (left, _) = burst(verb, 3.0);
        let (early, late) = shape_windows(&left, fs);
        [
            rms(&left),
            crate::teo5::tests::brightness_below(&left, 3_000.0, fs),
            late / early.max(1.0e-12),
        ]
    }

    /// **The number on the decay knob is the number in the render.**
    ///
    /// Schroeder backward integration and a T30 fit, at four settings that
    /// span the useful travel. The tolerance is the published one and the
    /// short settings are reached by shrinking `size`, not by driving the
    /// coefficient toward zero: the map assumes many circulations and it is
    /// only honest for `RT60 ≥ 2·T_loop`, which at `size = 1` is 1.45 s.
    #[test]
    fn the_decay_knob_is_rt60_in_seconds() {
        // (rt60, size, tolerance)
        let cases = [
            (0.5f32, 0.35f32, 0.20f64),
            (1.8, 1.0, 0.10),
            (4.0, 1.0, 0.10),
            (8.0, 1.0, 0.10),
        ];
        for (rt60, size, tolerance) in cases {
            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_SIZE, size);
            verb.set_param_natural_immediate(PARAM_DECAY_S, rt60);
            // Damping shortens the tail's high end; the RT60 map is about the
            // broadband decay, so this measurement opens the filter up.
            verb.set_param_natural_immediate(PARAM_DAMP_HZ, 20_000.0);
            verb.set_param_natural_immediate(PARAM_LOW_CUT_HZ, 20.0);
            let (left, right) = impulse(&mut verb, f64::from(rt60) * 2.5 + 1.0);
            for (name, channel) in [("L", &left), ("R", &right)] {
                let measured = rt60_t30(channel, FS).expect("a tail long enough to fit");
                let error = (measured - f64::from(rt60)) / f64::from(rt60);
                assert!(
                    error.abs() <= tolerance,
                    "{name}: decay {rt60} s at size {size} measured {measured:.3} s \
                     ({:+.1}%, tolerance {:.0}%)",
                    error * 100.0,
                    tolerance * 100.0
                );
            }
        }
    }


    /// **Every algorithm decays at the time it was asked for.**
    ///
    /// The plate's map is the published one; the FDN's is Jot's per-line gain
    /// `10^(−3·M/(RT60·fs))`, which is exact by construction. The room is
    /// tested only inside its mode-density budget — a 232 ms network asked
    /// for eight seconds is a resonator, and saying so is what
    /// [`the_delay_tables_carry_the_decay_they_advertise`] is for.
    #[test]
    fn every_algorithm_decays_at_the_time_it_was_asked_for() {
        for algorithm in Algorithm::ALL {
            let ceiling = match algorithm {
                Algorithm::Room => 1.5f32,
                _ => 8.0,
            };
            for rt60 in [1.0f32, 1.8, 4.0, 8.0] {
                if rt60 > ceiling {
                    continue;
                }
                let mut verb = wet_only(FS);
                verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
                verb.set_param_natural_immediate(PARAM_DECAY_S, rt60);
                verb.set_param_natural_immediate(PARAM_DAMP_HZ, 20_000.0);
                verb.set_param_natural_immediate(PARAM_LOW_CUT_HZ, 20.0);
                let (left, right) = impulse(&mut verb, f64::from(rt60) * 2.5 + 1.0);
                for (side, channel) in [("L", &left), ("R", &right)] {
                    let measured = rt60_t30(channel, FS).expect("a tail long enough to fit");
                    let error = (measured - f64::from(rt60)) / f64::from(rt60);
                    assert!(
                        error.abs() <= 0.15,
                        "{} {side}: decay {rt60} s measured {measured:.3} s ({:+.1}%)",
                        algorithm.label(),
                        error * 100.0
                    );
                }
            }
        }
    }

    /// **The decay is actually exponential**, by ISO 3382-2 Annex B.
    ///
    /// Curvature `C = 100·(T30/T20 − 1)` in percent, which the standard puts
    /// in a 0–5% band for a well-behaved decay. A positive C means the late
    /// tail decays *slower* than the early one, which is what a per-line
    /// damping mismatch looks like — the cheapest single test that a
    /// feedback network decays the way it claims to.
    #[test]
    fn the_decay_curve_is_a_straight_line() {
        for algorithm in Algorithm::ALL {
            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
            verb.set_param_natural_immediate(PARAM_DECAY_S, 2.0);
            verb.set_param_natural_immediate(PARAM_DAMP_HZ, 20_000.0);
            verb.set_param_natural_immediate(PARAM_LOW_CUT_HZ, 20.0);
            let (left, _) = impulse(&mut verb, 6.0);
            let t30 = rt60_t30(&left, FS).expect("a fittable tail");
            let t20 = rt60_t20(&left, FS).expect("a fittable tail");
            let curvature = 100.0 * (t30 / t20 - 1.0);
            assert!(
                curvature.abs() <= 8.0,
                "{}: T20 {t20:.3} s, T30 {t30:.3} s, curvature {curvature:+.1}%",
                algorithm.label()
            );
        }
    }

    /// **Echo density grows, and reaches noise.**
    ///
    /// The Abel–Huang measure: the fraction of samples in a 20 ms window
    /// above one standard deviation, normalised so that Gaussian noise scores
    /// exactly 1. Below 1 the tail is sparser than noise and individual
    /// echoes are audible; 0.2 reads as "sputtery" and 0.8 as "smooth".
    ///
    /// The plate is the one with a real target, because instantaneous density
    /// is what a plate *is*. The hall's is deliberately much later: with
    /// 125 ms as its shortest line it cannot be dense early, which is exactly
    /// why [`Algorithm::suggested_early`] gives it an ER section. A hall's
    /// first two hundred milliseconds are *supposed* to be a designed
    /// reflection pattern rather than noise.
    #[test]
    fn echo_density_grows_towards_noise() {
        let targets = [
            (Algorithm::Plate, [(0.050f64, 0.35f64), (0.200, 0.70), (0.400, 0.85)]),
            (Algorithm::Room, [(0.050, 0.35), (0.200, 0.70), (0.400, 0.85)]),
            (Algorithm::Hall, [(0.200, 0.10), (0.400, 0.60), (0.800, 0.80)]),
            (Algorithm::Spring, [(0.050, 0.35), (0.200, 0.70), (0.400, 0.60)]),
        ];
        for (algorithm, points) in targets {
            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
            verb.set_param_natural_immediate(PARAM_PREDELAY_MS, 0.0);
            verb.set_param_natural_immediate(PARAM_DECAY_S, 3.0);
            let (left, _) = impulse(&mut verb, 2.0);
            let mut previous = 0.0f64;
            for (at, floor) in points {
                let density = echo_density(&left, FS, at);
                assert!(
                    density >= floor,
                    "{}: density at {:.0} ms is {density:.3}, wanted {floor}",
                    algorithm.label(),
                    at * 1000.0
                );
                assert!(
                    density >= previous - 0.15,
                    "{}: density fell from {previous:.3} to {density:.3} by {:.0} ms",
                    algorithm.label(),
                    at * 1000.0
                );
                previous = density;
            }
        }
    }

    /// **The two channels are one space, not two rooms.**
    ///
    /// Normalised cross-correlation, **max over ±1 ms of lag** rather than at
    /// zero lag alone: a pure inter-channel *delay* scores ~0 at zero lag and
    /// ~1 at its own lag, and sounds like slapback rather than width. And at
    /// `width = 0` the two channels are the same numbers, which is the other
    /// end of the same control.
    #[test]
    fn the_wet_is_decorrelated_and_the_width_control_closes_it() {
        for algorithm in Algorithm::ALL {
            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
            verb.set_param_natural_immediate(PARAM_DECAY_S, 3.0);
            verb.set_param_natural_immediate(PARAM_EARLY, algorithm.suggested_early());
            let (left, right) = impulse(&mut verb, 3.0);
            let from = (FS * 0.15) as usize;
            let worst = max_correlation(&left[from..], &right[from..], (FS * 0.001) as isize);
            assert!(
                worst <= 0.3,
                "{}: |r| over ±1 ms of lag is {worst:.4}",
                algorithm.label()
            );

            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
            verb.set_param_natural_immediate(PARAM_EARLY, 50.0);
            verb.set_param_natural_immediate(PARAM_WIDTH, 0.0);
            let (left, right) = impulse(&mut verb, 1.0);
            assert_eq!(left, right, "{}: width 0 is not mono", algorithm.label());
        }
    }

    /// **The damping knob moves the tail's centre of gravity, monotonically.**
    ///
    /// This is the test that catches the coefficient-convention trap: with
    /// the damping filter written the other way round the corner lands at
    /// 3.8 Hz, the centroid stops responding, and RT60 pins near 0.55 s.
    #[test]
    fn damping_darkens_the_tail_monotonically() {
        for algorithm in Algorithm::ALL {
            let mut previous = 0.0f64;
            for damp in [1_000.0f32, 2_000.0, 4_000.0, 8_000.0, 16_000.0] {
                let mut verb = wet_only(FS);
                verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
                verb.set_param_natural_immediate(PARAM_DECAY_S, 3.0);
                verb.set_param_natural_immediate(PARAM_DAMP_HZ, damp);
                let (left, _) = impulse(&mut verb, 2.0);
                let window = &left[(FS * 0.3) as usize..(FS * 1.5) as usize];
                let centroid = crate::teo5::tests::brightness_below(window, 12_000.0, FS);
                assert!(
                    centroid > previous,
                    "{}: damping {damp} Hz gave centroid {centroid:.1} Hz, \
                     no brighter than the setting below it ({previous:.1} Hz)",
                    algorithm.label()
                );
                previous = centroid;
            }
        }
    }

    /// **Predelay adds exactly what it says, on top of the plate's own
    /// 8.94 ms floor.**
    ///
    /// The floor is `node48_54[266]`, the earliest of Table 2's output taps,
    /// and it is part of the plate's sound rather than something to
    /// compensate away — but it has to be documented, because a player who
    /// sets predelay to zero and measures 8.9 ms will otherwise file a bug.
    ///
    /// Measured at full diffusion, which is the only setting where the
    /// measurement means what it looks like: an allpass at coefficient zero
    /// is not a wire, it is a plain delay of its own length, so turning
    /// diffusion off moves the onset out by the whole diffuser chain.
    #[test]
    fn predelay_is_exact_on_top_of_the_plates_own_onset() {
        let onset = |verb: &mut Reverb| -> f64 {
            let (left, _) = impulse(verb, 1.0);
            left.iter()
                .position(|v| v.abs() > 1.0e-4)
                .map_or(f64::NAN, |i| i as f64 / FS)
        };
        let mut verb = wet_only(FS);
        verb.set_param_natural_immediate(PARAM_PREDELAY_MS, 0.0);
        verb.set_param_natural_immediate(PARAM_DECAY_S, 1.0);
        let floor = onset(&mut verb);
        assert!(
            (floor - INTRINSIC_ONSET_SECONDS).abs() < 2.0 / FS,
            "the plate's own onset is {:.4} ms, not the {:.4} ms tap 266 predicts",
            floor * 1000.0,
            INTRINSIC_ONSET_SECONDS * 1000.0
        );
        for ms in [20.0f32, 120.0, 500.0] {
            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_PREDELAY_MS, ms);
            verb.set_param_natural_immediate(PARAM_DECAY_S, 1.0);
            // Three samples of slop: a threshold crossing on an
            // interpolated edge is worth one, and the predelay line's own
            // floor is two — `tap_cubic` needs four points around the read,
            // so the shortest delay it can express is two samples, 42 µs.
            let measured = onset(&mut verb) - floor;
            assert!(
                (measured * 1000.0 - f64::from(ms)).abs() < 3000.0 / FS,
                "predelay {ms} ms added {:.4} ms",
                measured * 1000.0
            );
        }
    }

    /// **The same reverb at 44.1, 48 and 96 kHz.**
    ///
    /// Three aggregate numbers — level, spectrum and shape — never a hash,
    /// because CI runs three different libms and a bit-exact comparison would
    /// be testing `sin`.
    ///
    /// The shape number carries a wider band than the other two on purpose.
    /// It is `10^(−3Δt/RT60)`, an *exponential* of the decay time, so its
    /// sensitivity is `3Δt·ln10/RT60 ≈ 1.9` — asserting 3% on the ratio is
    /// asserting 1.5% on the reverberation time itself, which is the quantity
    /// anyone would actually listen for.
    #[test]
    fn the_reverb_is_the_same_reverb_at_every_rate() {
        for algorithm in Algorithm::ALL {
            let reference = {
                let mut verb = wet_only(44_100.0);
                verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
                verb.set_param_natural_immediate(PARAM_EARLY, algorithm.suggested_early());
                fingerprint(&mut verb)
            };
            for fs in [48_000.0f64, 96_000.0] {
                let mut verb = wet_only(fs);
                verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
                verb.set_param_natural_immediate(PARAM_EARLY, algorithm.suggested_early());
                let measured = fingerprint(&mut verb);
                for (index, (name, tolerance)) in
                    [("rms", 0.02), ("centroid", 0.02), ("shape", 0.03)].iter().enumerate()
                {
                    let drift = (measured[index] - reference[index]) / reference[index].abs();
                    assert!(
                        drift.abs() <= *tolerance,
                        "{} at {fs} Hz: {name} {:.5} against {:.5} at 44.1 kHz ({:+.2}%)",
                        algorithm.label(),
                        measured[index],
                        reference[index],
                        drift * 100.0
                    );
                }
            }
        }
    }

    /// **Wet at zero is the dry signal, bit for bit.**
    ///
    /// Not "the dry signal times one": the tank still runs, so that a tail is
    /// there when the knob comes back, and the wet is then not added at all.
    /// `−0.0 + 0.0` is `+0.0`, which is the difference between a control-flow
    /// guarantee and a floating-point coincidence.
    #[test]
    fn wet_at_zero_is_bit_identical_dry() {
        for fs in [44_100.0f64, 48_000.0, 96_000.0] {
            for algorithm in Algorithm::ALL {
                let mut verb = Reverb::new(fs);
                verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
                verb.set_param_natural_immediate(PARAM_MIX, 0.0);
                let source = awkward_signal(4096);
                let mut left = source.clone();
                let mut right = source.clone();
                verb.process(&mut left, &mut right);
                for (index, (before, after)) in source.iter().zip(&left).enumerate() {
                    assert_eq!(
                        before.to_bits(),
                        after.to_bits(),
                        "{} at {fs} Hz: sample {index} became {after} from {before}",
                        algorithm.label()
                    );
                }
                assert_eq!(right, source);
            }
        }
    }

    /// **A knob turned to zero reaches zero.**
    ///
    /// The mix is smoothed, and a one-pole never arrives; the smoother snaps
    /// the last millionth so that the null above is reachable from a running
    /// wet signal rather than only from a fresh instance.
    #[test]
    fn a_mix_turned_down_settles_to_an_exact_null() {
        let mut verb = Reverb::new(FS);
        let source = awkward_signal(2048);
        let mut left = source.clone();
        let mut right = source.clone();
        verb.process(&mut left, &mut right);
        assert!(left != source, "the reverb was not audible to begin with");

        verb.set_param_natural(PARAM_MIX, 0.0);
        // Fifteen time constants of the 15 ms smoother.
        for _ in 0..16 {
            let mut l = source.clone();
            let mut r = source.clone();
            verb.process(&mut l, &mut r);
        }
        let mut left = source.clone();
        let mut right = source.clone();
        verb.process(&mut left, &mut right);
        assert_eq!(left, source, "the mix never reached zero");
        assert_eq!(right, source);
    }

    /// **At the levels this box is gain-staged to, the wet never reaches the
    /// saturator.**
    ///
    /// Swept, not sampled: an impulse says this plate peaks at 0.09 and a
    /// sustained tone at a mode frequency says 5.05, a factor of 55. The
    /// input here is −12 dBFS, which is what the instruments are trimmed to.
    #[test]
    fn a_sustained_tone_stays_under_the_saturator_knee() {
        const KNEE: f32 = 0.75;
        for algorithm in Algorithm::ALL {
            for rt60 in [0.5f32, 1.8, 4.0] {
                for size in [0.25f32, 1.0, 2.0] {
                    for hz in [110.0f64, 440.0] {
                        let mut verb = wet_only(FS);
                        verb.set_param_natural_immediate(
                            PARAM_ALGORITHM,
                            algorithm.index() as f32,
                        );
                        verb.set_param_natural_immediate(PARAM_DECAY_S, rt60);
                        verb.set_param_natural_immediate(PARAM_SIZE, size);
                        verb.set_param_natural_immediate(
                            PARAM_EARLY,
                            algorithm.suggested_early(),
                        );
                        verb.snap();
                        let frames = (FS * (f64::from(rt60) + 0.75)) as usize;
                        let mut top = 0.0f32;
                        for n in 0..frames {
                            let x = 0.25 * (TAU * hz * n as f64 / FS).sin() as f32;
                            let (l, r) = verb.process_sample(x, x);
                            top = top.max(l.abs()).max(r.abs());
                        }
                        assert!(
                            top < KNEE,
                            "{} at rt60 {rt60} s, size {size}, {hz} Hz: wet peaked at {top:.4}",
                            algorithm.label()
                        );
                    }
                }
            }
        }
    }

    /// **And at the levels it is not gain-staged to, it is still bounded.**
    ///
    /// Full scale, the longest decay, every algorithm, both ends of `size`
    /// and `diffusion`. An effect cannot bound its output below its input, so
    /// this is not a claim about `TARGET_PEAK` — it is the claim that the
    /// saturator holds and that nothing in the tank ever produces a value the
    /// device cannot play.
    #[test]
    fn the_wet_is_bounded_even_when_it_is_driven_absurdly() {
        for algorithm in Algorithm::ALL {
            for size in [0.25f32, 2.0] {
                for diffusion in [0.0f32, 100.0] {
                    let mut verb = wet_only(FS);
                    verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
                    verb.set_param_natural_immediate(PARAM_DECAY_S, 20.0);
                    verb.set_param_natural_immediate(PARAM_SIZE, size);
                    verb.set_param_natural_immediate(PARAM_DIFFUSION, diffusion);
                    verb.set_param_natural_immediate(PARAM_EARLY, 100.0);
                    verb.snap();
                    let mut top = 0.0f32;
                    for n in 0..(FS as usize * 4) {
                        let x = (TAU * 110.0 * n as f64 / FS).sin() as f32;
                        let (l, r) = verb.process_sample(x, x);
                        assert!(
                            l.is_finite() && r.is_finite(),
                            "{}: not finite at sample {n}",
                            algorithm.label()
                        );
                        top = top.max(l.abs()).max(r.abs());
                    }
                    assert!(
                        top < 2.0,
                        "{} at size {size}, diffusion {diffusion}: peaked at {top:.4}",
                        algorithm.label()
                    );
                }
            }
        }
    }

    /// **Thirty seconds of silence after the last note, and nothing is
    /// subnormal.**
    ///
    /// An `f32` signal falls from full scale to the smallest normal in
    /// `12.6 × RT60` after the input stops — 25 seconds at RT60 2 s, which is
    /// an idle plug-in long after the musician stopped playing. On x86 every
    /// operation that *produces* a subnormal costs 120–150 cycles of
    /// microcode assist, and a reverb runs a hundred of them per sample.
    ///
    /// The output must also reach exactly zero: an implementation that idles
    /// forever at 2e-20 never lets the host sleep the track.
    ///
    /// **This test cannot fail on Apple Silicon**, which has no denormal
    /// penalty at all; what it checks is the *values*, which are the same
    /// everywhere.
    #[test]
    fn a_thirty_second_tail_never_goes_subnormal() {
        let mut verb = wet_only(FS);
        verb.set_param_natural_immediate(PARAM_DECAY_S, 2.0);
        let mut left = [0.0f32; 256];
        let mut right = [0.0f32; 256];
        left[0] = 1.0;
        right[0] = 1.0;
        verb.process(&mut left, &mut right);

        let blocks = (FS * 30.0 / 256.0) as usize;
        let mut silent_from = None;
        for block in 0..blocks {
            left.fill(0.0);
            right.fill(0.0);
            verb.process(&mut left, &mut right);
            for (index, sample) in left.iter().chain(right.iter()).enumerate() {
                assert!(sample.is_finite(), "block {block}, sample {index}: {sample}");
                assert!(
                    !sample.is_subnormal(),
                    "block {block}, sample {index} is subnormal: {sample:e}"
                );
            }
            let quiet = left.iter().chain(right.iter()).all(|s| *s == 0.0);
            match (quiet, silent_from) {
                (true, None) => silent_from = Some(block),
                (false, Some(_)) => silent_from = None,
                _ => {}
            }
        }
        let landed = silent_from.expect("the tail never reached exact zero in 30 s");
        assert!(
            landed as f64 * 256.0 / FS < 25.0,
            "the tail took {:.1} s to reach exact zero",
            landed as f64 * 256.0 / FS
        );
    }

    /// **The scattering matrix is lossless, and it is not an involution.**
    ///
    /// `A·Aᵀ = I` is the correct losslessness check and eigenvalue inspection
    /// is not: Schlecht & Habets' counterexample `[[3,2],[−4,−3]]` has
    /// eigenvalues `{+1,−1}` and independent eigenvectors and is not
    /// lossless. And `A² ≠ I` is the point of the sign vector: a raw Hadamard
    /// is an involution, so applying it twice is a no-op and the scattering
    /// never rotates energy through the state space.
    #[test]
    fn the_scattering_matrix_is_lossless_and_not_an_involution() {
        let mut a = [[0.0f64; FDN_LINES]; FDN_LINES];
        for (row, line) in a.iter_mut().enumerate() {
            for (column, cell) in line.iter_mut().enumerate() {
                let hadamard = if (FDN_PERM[row] & column).count_ones() % 2 == 0 {
                    1.0
                } else {
                    -1.0
                };
                *cell = FDN_SIGNS[row] * hadamard * FDN_NORM;
            }
        }
        let product = |x: &[[f64; FDN_LINES]; FDN_LINES], y: &[[f64; FDN_LINES]; FDN_LINES]| {
            let mut out = [[0.0f64; FDN_LINES]; FDN_LINES];
            for i in 0..FDN_LINES {
                for j in 0..FDN_LINES {
                    out[i][j] = (0..FDN_LINES).map(|k| x[i][k] * y[k][j]).sum();
                }
            }
            out
        };
        let mut transpose = [[0.0f64; FDN_LINES]; FDN_LINES];
        for (i, row) in transpose.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = a[j][i];
            }
        }
        let identity = product(&a, &transpose);
        for (i, row) in identity.iter().enumerate() {
            for (j, cell) in row.iter().enumerate() {
                let wanted = f64::from(u8::from(i == j));
                assert!((cell - wanted).abs() < 1.0e-12, "A·Aᵀ[{i}][{j}] = {cell}");
            }
        }
        let squared = product(&a, &a);
        let is_identity = (0..FDN_LINES).all(|i| {
            (0..FDN_LINES).all(|j| (squared[i][j] - f64::from(u8::from(i == j))).abs() < 1.0e-12)
        });
        assert!(!is_identity, "A² = I: the involution was not broken");

        // And the transform the audio path actually runs is that matrix.
        for column in 0..FDN_LINES {
            let mut vector = [0.0f64; FDN_LINES];
            vector[column] = 1.0;
            fwht8(&mut vector);
            for row in 0..FDN_LINES {
                let applied = FDN_SIGNS[row] * vector[FDN_PERM[row]] * FDN_NORM;
                assert!(
                    (applied - a[row][column]).abs() < 1.0e-12,
                    "the butterfly and the matrix disagree at [{row}][{column}]"
                );
            }
        }
    }

    /// **Every delay set carries the decay range it is offered for.**
    ///
    /// Schroeder's criterion, in its sample-rate-free form: total delay in
    /// *seconds* must be at least `0.15 × RT60`, or the modes are spaced
    /// further apart than their own width and the tail beats rather than
    /// decays. Three lines of arithmetic that catch the whole "sounds
    /// metallic at long settings" class before a listening session does.
    #[test]
    fn the_delay_tables_carry_the_decay_they_advertise() {
        let budget = |total_seconds: f64| total_seconds / 0.15;
        assert!(
            (budget(T_LOOP_SECONDS) - 4.836).abs() < 0.01,
            "the plate's honest ceiling moved: {:.3} s",
            budget(T_LOOP_SECONDS)
        );
        let room: f64 = ROOM_MS.iter().map(|(total, _)| total).sum::<f64>() * 0.001;
        let hall: f64 = HALL_MS.iter().map(|(total, _)| total).sum::<f64>() * 0.001;
        assert!(
            (room - 0.2321).abs() < 0.001 && (budget(room) - 1.547).abs() < 0.01,
            "the room's 232 ms carries {:.3} s of decay",
            budget(room)
        );
        assert!(
            (hall - 1.4603).abs() < 0.001 && budget(hall) > 9.0,
            "the hall's {hall:.4} s carries {:.3} s of decay",
            budget(hall)
        );
        // At `size = 2` every budget doubles, which is why long settings want
        // a bigger room rather than a longer coefficient.
        assert!(budget(room * SIZE_MAX) > 3.0);
    }

    /// **Turning `size` under a running tail does not click, on any of the
    /// three paths.**
    ///
    /// Morph is what a knob does, the program path is what a patch change
    /// does, and an algorithm change is the discrete case that is allowed to
    /// reload. The test is a step limit rather than an ear: a click is a
    /// discontinuity, and one large enough to hear while the signal is small
    /// is what "click" means.
    #[test]
    fn size_and_algorithm_changes_do_not_click() {
        for immediate in [false, true] {
            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_DECAY_S, 4.0);
            let mut previous = 0.0f32;
            let mut worst = 0.0f32;
            let mut n = 0usize;
            // A tail, then `size` automated across its whole travel at
            // host-automation rate — every 64 samples, not every mouse move.
            let mut sizes = [0.5f32, 0.8, 1.1, 1.4, 1.7, 2.0].iter().cycle();
            for block in 0..((FS * 3.0) as usize / 64) {
                if block == 8 {
                    // Nothing more goes in: everything from here is tail.
                }
                if block > 16 && block % 4 == 0 {
                    let size = *sizes.next().unwrap();
                    if immediate {
                        verb.set_param_natural_immediate(PARAM_SIZE, size);
                    } else {
                        verb.set_param_natural(PARAM_SIZE, size);
                    }
                }
                for _ in 0..64 {
                    let x = if n < 4096 {
                        0.25 * (TAU * 220.0 * n as f64 / FS).sin() as f32
                    } else {
                        0.0
                    };
                    let (l, _) = verb.process_sample(x, x);
                    let wet = l;
                    if n > 4096 {
                        worst = worst.max((wet - previous).abs());
                    }
                    previous = wet;
                    n += 1;
                }
            }
            assert!(
                worst < 0.05,
                "size automation ({}) produced a {worst:.4} step between samples",
                if immediate { "program path" } else { "morph" }
            );
        }

        // The algorithm selector is the discrete case, and it crossfades to
        // silence rather than re-indexing a running tank.
        let mut verb = wet_only(FS);
        verb.set_param_natural_immediate(PARAM_DECAY_S, 4.0);
        let mut previous = 0.0f32;
        let mut worst = 0.0f32;
        for n in 0..(FS as usize * 2) {
            if n == 24_000 {
                verb.set_param_natural(PARAM_ALGORITHM, Algorithm::Hall.index() as f32);
            }
            if n == 60_000 {
                verb.set_param_natural(PARAM_ALGORITHM, Algorithm::Plate.index() as f32);
            }
            let x = if n < 4096 {
                0.25 * (TAU * 220.0 * n as f64 / FS).sin() as f32
            } else {
                0.0
            };
            let (l, _) = verb.process_sample(x, x);
            let wet = l;
            if n > 4096 {
                worst = worst.max((wet - previous).abs());
            }
            previous = wet;
        }
        assert!(worst < 0.05, "an algorithm change stepped by {worst:.4}");
    }

    /// **Moving `size` does not move the decay time.**
    ///
    /// The whole argument for putting seconds on the knob: `decay` is
    /// recomputed from RT60 every time the geometry moves, so a player who
    /// makes the room bigger gets a bigger room rather than a longer one.
    #[test]
    fn size_does_not_change_the_decay_time() {
        for size in [0.5f32, 1.0, 1.5] {
            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_SIZE, size);
            verb.set_param_natural_immediate(PARAM_DECAY_S, 1.8);
            verb.set_param_natural_immediate(PARAM_DAMP_HZ, 20_000.0);
            verb.set_param_natural_immediate(PARAM_LOW_CUT_HZ, 20.0);
            let (left, _) = impulse(&mut verb, 6.0);
            let measured = rt60_t30(&left, FS).expect("a fittable tail");
            assert!(
                (measured - 1.8).abs() / 1.8 <= 0.12,
                "at size {size} the 1.8 s setting measured {measured:.3} s"
            );
        }
    }

    /// **Nothing in `process` reaches the allocator** — including a geometry
    /// morph, an algorithm change and a reset while the tank is sounding.
    #[test]
    fn nothing_in_the_audio_path_allocates() {
        let mut verb = wet_only(FS);
        let mut left = vec![0.0f32; 256];
        let mut right = vec![0.0f32; 256];
        left[0] = 1.0;
        right[0] = 1.0;
        verb.process(&mut left, &mut right);

        let allocations = crate::synth::tests::allocations_during(|| {
            for block in 0..400 {
                match block % 40 {
                    5 => verb.set_param_natural(PARAM_SIZE, 1.7),
                    10 => verb.set_param_natural(PARAM_DECAY_S, 6.0),
                    15 => verb.set_param_natural(PARAM_PREDELAY_MS, 180.0),
                    20 => verb.set_param_natural(
                        PARAM_ALGORITHM,
                        Algorithm::Hall.index() as f32,
                    ),
                    25 => verb.set_param_natural_immediate(PARAM_SIZE, 0.4),
                    30 => verb.set_param_natural(
                        PARAM_ALGORITHM,
                        Algorithm::Spring.index() as f32,
                    ),
                    35 => verb.reset(),
                    _ => {}
                }
                verb.process(&mut left, &mut right);
            }
        });
        assert_eq!(allocations, 0, "the audio path allocated {allocations} times");
    }

    /// The RT60 map inverts, which is what makes it a map rather than a
    /// fitted curve.
    #[test]
    fn the_rt60_map_is_its_own_inverse() {
        for rt60 in [0.5f64, 1.0, 1.8, 4.0, 8.0, 20.0] {
            let decay = decay_for_rt60(rt60, T_LOOP_SECONDS);
            let back = rt60_for_decay(decay, T_LOOP_SECONDS);
            assert!(
                (back - rt60).abs() / rt60 < 1.0e-6,
                "{rt60} s -> {decay:.6} -> {back} s"
            );
        }
        // The paper's own default, and the house's, are the same sound.
        let published = rt60_for_decay(0.5, T_LOOP_SECONDS);
        assert!(
            (published - 1.807).abs() < 0.005,
            "Dattorro's decay = 0.5 maps to {published:.4} s, not 1.81"
        );
        assert!(decay_for_rt60(0.0, T_LOOP_SECONDS) == 0.0);
        assert!(rt60_for_decay(1.0, T_LOOP_SECONDS) == 0.0);
    }

    /// Modulation depth backs off as the decay grows, because detuning
    /// accumulates with the number of passes and the number of passes is
    /// `RT60/T_loop`.
    #[test]
    fn modulation_backs_off_for_long_decays() {
        let depth_at = |rt60: f32| {
            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_DECAY_S, rt60);
            verb.mod_depth.target
        };
        let short = depth_at(0.5);
        let house = depth_at(1.8);
        let long = depth_at(16.0);
        assert!((house - 0.35).abs() < 1.0e-9, "the house setting is {house}");
        assert!(short == house, "below 1.8 s the depth is not scaled up: {short}");
        assert!(long < house * 0.4, "at 16 s the depth is {long}");
    }

    /// Table 2's tap positions all land inside the lines they name — a
    /// self-check on the transcription that costs nothing and catches a
    /// transposed digit.
    #[test]
    fn every_output_tap_is_inside_its_line() {
        for (name, taps) in [("L", &TAPS_L), ("R", &TAPS_R)] {
            for &(line, position, sign) in taps.iter() {
                assert!(line < TANK.len(), "{name}: line {line}");
                assert!(
                    position < TANK[line],
                    "{name}: tap {position} is past line {line}'s {} samples",
                    TANK[line]
                );
                assert!(sign == 1.0 || sign == -1.0, "{name}: sign {sign}");
            }
        }
        // Four of the left channel's seven taps come from the right branch
        // and three from the left, and the right mirrors it. That
        // cross-tapping is the entire stereo image; the input is mono.
        assert_eq!(TAPS_L.iter().filter(|(line, _, _)| *line >= 4).count(), 4);
        assert_eq!(TAPS_R.iter().filter(|(line, _, _)| *line < 4).count(), 4);
        assert!((TANK.iter().sum::<f64>() - TANK_TOTAL).abs() < 1.0e-9);
    }

    /// Nonsense from a host or a hand-edited session file is refused, not
    /// propagated into a delay length.
    #[test]
    fn it_survives_nonsense() {
        let mut verb = Reverb::new(FS);
        let before = default_natural_params();
        verb.set_param_natural(PARAM_COUNT, 1.0);
        verb.set_param_natural(usize::MAX, 1.0);
        for index in 0..PARAM_COUNT {
            verb.set_param_natural(index, f32::NAN);
            verb.set_param_natural(index, f32::INFINITY);
        }
        for (index, &value) in before.iter().enumerate() {
            assert_eq!(verb.param_natural(index), value, "index {index}");
        }
        assert_eq!(verb.param_natural(PARAM_COUNT), 0.0);

        // Out of range clamps to the published travel rather than landing in
        // a buffer that is not there.
        verb.set_param_natural_immediate(PARAM_SIZE, 40.0);
        verb.set_param_natural_immediate(PARAM_PREDELAY_MS, 9_000.0);
        verb.set_param_natural_immediate(PARAM_ALGORITHM, 99.0);
        assert_eq!(verb.param_natural(PARAM_SIZE), SIZE_MAX as f32);
        assert_eq!(verb.param_natural(PARAM_PREDELAY_MS), 500.0);
        assert_eq!(verb.algorithm(), Algorithm::Spring);

        // And a rate the device could not have asked for leaves it built at
        // the last one it was given.
        verb.set_sample_rate(0.0);
        verb.set_sample_rate(f64::NAN);
        assert_eq!(verb.sample_rate(), FS);

        let mut left = awkward_signal(512);
        let mut right = left.clone();
        verb.process(&mut left, &mut right);
        assert!(left.iter().all(|s| s.is_finite()));
    }

    /// The parameter table is the one thing every other view is generated
    /// from, so its shape is worth pinning.
    #[test]
    fn the_control_surface_is_the_published_one() {
        assert_eq!(PARAM_COUNT, 12);
        let defaults = default_natural_params();
        for (index, &default) in defaults.iter().enumerate() {
            let info = natural_param(index).expect("a control at every index");
            assert!(!info.name.is_empty(), "index {index}");
            assert!(info.min <= info.default && info.default <= info.max, "index {index}");
            assert_eq!(default, info.default);
            assert_eq!(param_name(index), info.name);
        }
        assert!(natural_param(PARAM_COUNT).is_none());
        assert_eq!(param_name(PARAM_COUNT), "");

        // The house defaults, which `FX.md` fixes: plate, 20 ms, 1.8 s,
        // ~6 kHz, 25% on an insert.
        assert_eq!(defaults[PARAM_ALGORITHM], 0.0);
        assert_eq!(Algorithm::from_index(0), Algorithm::Plate);
        assert_eq!(defaults[PARAM_PREDELAY_MS], 20.0);
        assert_eq!(defaults[PARAM_DECAY_S], 1.8);
        assert_eq!(defaults[PARAM_DAMP_HZ], 6_000.0);
        assert_eq!(defaults[PARAM_MIX], 25.0);

        for algorithm in Algorithm::ALL {
            assert_eq!(Algorithm::from_index(algorithm.index()), algorithm);
            assert!(!algorithm.label().is_empty());
        }
        assert_eq!(Algorithm::from_index(99), Algorithm::Plate);

        // One control is inert on one algorithm, and the panel greys exactly
        // what this refuses.
        for algorithm in Algorithm::ALL {
            for index in 0..PARAM_COUNT {
                let expected =
                    !(index == PARAM_DIFFUSION && algorithm == Algorithm::Spring);
                assert_eq!(algorithm.uses(index), expected, "{} {index}", algorithm.label());
            }
        }
    }

    /// A `reset` drops the tail without dropping the settings.
    #[test]
    fn reset_silences_the_tank_and_keeps_the_controls() {
        let mut verb = wet_only(FS);
        verb.set_param_natural_immediate(PARAM_DECAY_S, 8.0);
        // Long enough to contain the wet: the default predelay is 20 ms and
        // the plate's own onset is another 8.94 ms on top of it, so a block
        // shorter than 1390 samples is still inside the silence before the
        // first tap.
        let mut left = vec![0.0f32; 4096];
        let mut right = vec![0.0f32; 4096];
        left[0] = 1.0;
        right[0] = 1.0;
        verb.process(&mut left, &mut right);
        assert!(peak(&left) > 0.0, "the reverb made no sound at all");

        verb.reset();
        let mut left = vec![0.0f32; 4096];
        let mut right = vec![0.0f32; 4096];
        verb.process(&mut left, &mut right);
        assert_eq!(peak(&left), 0.0, "the tail survived a reset");
        assert_eq!(verb.param_natural(PARAM_DECAY_S), 8.0);
        assert_eq!(verb.param_natural(PARAM_MIX), 100.0);
    }
}

#[cfg(test)]
mod measure {
    use super::tests::*;
    use super::*;

    /// `cargo test -p phosphor-dsp --lib -- --ignored report_rt60 --nocapture`
    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_rt60() {
        println!("T_loop = {T_LOOP_SECONDS:.6} s");
        println!("{:>8} {:>6} {:>10} {:>10} {:>8}", "rt60", "size", "meas L", "meas R", "err %");
        for size in [0.35f32, 0.5, 1.0, 1.5, 2.0] {
            for rt60 in [0.5f32, 1.0, 1.8, 4.0, 8.0] {
                let mut verb = wet_only(FS);
                verb.set_param_natural_immediate(PARAM_SIZE, size);
                verb.set_param_natural_immediate(PARAM_DECAY_S, rt60);
                verb.set_param_natural_immediate(PARAM_DAMP_HZ, 20_000.0);
                verb.set_param_natural_immediate(PARAM_LOW_CUT_HZ, 20.0);
                let (l, r) = impulse(&mut verb, f64::from(rt60) * 2.5 + 1.0);
                let a = rt60_t30(&l, FS).unwrap_or(f64::NAN);
                let b = rt60_t30(&r, FS).unwrap_or(f64::NAN);
                println!(
                    "{rt60:>8} {size:>6} {a:>10.3} {b:>10.3} {:>8.1}",
                    100.0 * (a - f64::from(rt60)) / f64::from(rt60)
                );
            }
        }
    }

    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_levels() {
        println!("{:>8} {:>10} {:>10} {:>10}", "alg", "impulse", "noise pk", "sine pk");
        for algorithm in Algorithm::ALL {
            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
            verb.set_param_natural_immediate(PARAM_DECAY_S, 8.0);
            let (l, _) = impulse(&mut verb, 4.0);
            let impulse_peak = peak(&l);

            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
            verb.set_param_natural_immediate(PARAM_DECAY_S, 8.0);
            verb.snap();
            let mut state = 0x1234_5678u32;
            let mut noise_peak = 0.0f32;
            for _ in 0..(FS as usize * 6) {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let x = (state >> 8) as f32 / 8_388_608.0 - 1.0;
                let (a, _) = verb.process_sample(x, x);
                noise_peak = noise_peak.max(a.abs());
            }

            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
            verb.set_param_natural_immediate(PARAM_DECAY_S, 20.0);
            verb.snap();
            let mut sine_peak = 0.0f32;
            for n in 0..(FS as usize * 8) {
                let x = (TAU * 220.0 * n as f64 / FS).sin() as f32;
                let (a, _) = verb.process_sample(x, x);
                sine_peak = sine_peak.max(a.abs());
            }
            println!(
                "{:>8} {impulse_peak:>10.4} {noise_peak:>10.4} {sine_peak:>10.4}",
                algorithm.label()
            );
        }
    }

    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_damping() {
        println!("{:>8} {:>10} {:>12} {:>10}", "alg", "damp Hz", "centroid", "rms");
        for algorithm in Algorithm::ALL {
            for damp in [1_000.0f32, 2_000.0, 4_000.0, 8_000.0, 16_000.0] {
                let mut verb = wet_only(FS);
                verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
                verb.set_param_natural_immediate(PARAM_DECAY_S, 3.0);
                verb.set_param_natural_immediate(PARAM_DAMP_HZ, damp);
                let (l, _) = impulse(&mut verb, 2.0);
                let window = &l[(FS * 0.3) as usize..(FS * 1.5) as usize];
                println!(
                    "{:>8} {damp:>10} {:>12.1} {:>10.5}",
                    algorithm.label(),
                    crate::teo5::tests::brightness_below(window, 12_000.0, FS),
                    rms(window)
                );
            }
        }
    }

    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_stereo_and_density() {
        for algorithm in Algorithm::ALL {
            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
            verb.set_param_natural_immediate(PARAM_DECAY_S, 3.0);
            verb.set_param_natural_immediate(PARAM_EARLY, algorithm.suggested_early());
            let (l, r) = impulse(&mut verb, 3.0);
            let from = (FS * 0.1) as usize;
            println!(
                "{:>8}  |r| zero-lag {:.4}  max over +/-1 ms {:.4}",
                algorithm.label(),
                correlation(&l[from..], &r[from..], 0),
                max_correlation(&l[from..], &r[from..], (FS * 0.001) as isize),
            );

            // Density is measured from the impulse, so the predelay comes
            // off: a window sitting in the predelay's silence measures the
            // silence.
            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
            verb.set_param_natural_immediate(PARAM_DECAY_S, 3.0);
            verb.set_param_natural_immediate(PARAM_PREDELAY_MS, 0.0);
            verb.set_param_natural_immediate(PARAM_EARLY, algorithm.suggested_early());
            let (l, _) = impulse(&mut verb, 3.0);
            print!("{:>8}  density:", algorithm.label());
            for ms in [10.0f64, 20.0, 30.0, 50.0, 80.0, 120.0, 200.0, 400.0] {
                print!(" {ms:.0}ms {:.3}", echo_density(&l, FS, ms * 0.001));
            }
            println!();
        }
    }

    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_rate_independence() {
        println!("{:>8} {:>8} {:>10} {:>12} {:>10}", "alg", "fs", "rms", "centroid", "late/early");
        for algorithm in Algorithm::ALL {
            for fs in [22_050.0f64, 44_100.0, 48_000.0, 96_000.0] {
                let mut verb = wet_only(fs);
                verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
                verb.set_param_natural_immediate(PARAM_DECAY_S, 2.0);
                let (l, _) = burst(&mut verb, 3.0);
                let (early, late) = shape_windows(&l, fs);
                println!(
                    "{:>8} {fs:>8} {:>10.5} {:>12.1} {:>10.4}",
                    algorithm.label(),
                    rms(&l),
                    crate::teo5::tests::brightness_below(&l, 3_000.0, fs),
                    late / early.max(1.0e-12),
                );
            }
        }
    }

    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_predelay() {
        for algorithm in Algorithm::ALL {
            for ms in [0.0f32, 20.0, 120.0] {
                let mut verb = wet_only(FS);
                verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
                verb.set_param_natural_immediate(PARAM_PREDELAY_MS, ms);
                verb.set_param_natural_immediate(PARAM_DECAY_S, 1.0);
                let (l, r) = impulse(&mut verb, 1.0);
                let onset = |x: &[f32]| {
                    x.iter()
                        .position(|v| v.abs() > 1.0e-4)
                        .map_or(f64::NAN, |i| i as f64 * 1000.0 / FS)
                };
                println!(
                    "{:>8} predelay {ms:>6} ms -> L {:.3} ms  R {:.3} ms",
                    algorithm.label(),
                    onset(&l),
                    onset(&r)
                );
            }
        }
    }

    // ── Helpers the reports share ──

}

#[cfg(test)]
mod measure_levels {
    use super::tests::*;
    use super::*;

    /// The worst case, swept rather than sampled.
    ///
    /// A sustained tone at a mode frequency accumulates coherently to roughly
    /// `1/(1 − loop_gain)`; an impulse never finds that, which is the whole
    /// 55× finding. So this sweeps frequency *and* decay *and* input level.
    fn sustained_peak(algorithm: Algorithm, rt60: f32, amplitude: f32, hz: f64) -> f32 {
        let mut verb = wet_only(FS);
        verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
        verb.set_param_natural_immediate(PARAM_DECAY_S, rt60);
        verb.snap();
        let mut top = 0.0f32;
        let frames = (FS * (f64::from(rt60) * 1.5 + 1.0)) as usize;
        for n in 0..frames {
            let x = amplitude * (TAU * hz * n as f64 / FS).sin() as f32;
            let (l, r) = verb.process_sample(x, x);
            top = top.max(l.abs()).max(r.abs());
        }
        top
    }

    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_headroom_sweep() {
        println!("{:>8} {:>6} {:>8} {:>8} {:>8}", "alg", "rt60", "amp", "hz", "peak");
        for algorithm in Algorithm::ALL {
            for rt60 in [1.8f32, 8.0, 20.0] {
                for amplitude in [1.0f32, 0.25] {
                    let mut worst = 0.0f32;
                    let mut worst_hz = 0.0f64;
                    for hz in [55.0f64, 110.0, 220.0, 440.0, 880.0, 1760.0] {
                        let top = sustained_peak(algorithm, rt60, amplitude, hz);
                        if top > worst {
                            worst = top;
                            worst_hz = hz;
                        }
                    }
                    println!(
                        "{:>8} {rt60:>6} {amplitude:>8} {worst_hz:>8} {worst:>8.4}",
                        algorithm.label()
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_algorithm_levels() {
        println!("{:>8} {:>12} {:>12}", "alg", "wet rms", "impulse pk");
        for algorithm in Algorithm::ALL {
            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
            verb.snap();
            let mut state = 0x1234_5678u32;
            let mut wet = Vec::new();
            for _ in 0..(FS as usize * 4) {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let x = ((state >> 8) as f32 / 8_388_608.0 - 1.0) * 0.25;
                let (l, _) = verb.process_sample(x, x);
                wet.push(l);
            }
            let mut verb = wet_only(FS);
            verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
            let (l, _) = impulse(&mut verb, 3.0);
            println!(
                "{:>8} {:>12.5} {:>12.5}",
                algorithm.label(),
                rms(&wet[FS as usize..]),
                peak(&l)
            );
        }
    }
}
