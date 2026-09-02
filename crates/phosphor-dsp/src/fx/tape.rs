//! The tape machine: a magnetised medium, a transport that wobbles, and a
//! head that cannot read what it wrote.
//!
//! # Why there is a differential equation in here
//!
//! Everything else in this box distorts by *shape*: a curve, applied to a
//! sample, with no memory of the sample before it. Tape does not work like
//! that, and the difference is not subtle — it is the reason a mix glued to
//! tape sounds different from the same mix through a waveshaper trimmed to
//! the same 1 kHz distortion figure.
//!
//! The model is Jiles–Atherton, in Chowdhury's DAFx-19 formulation: the
//! magnetisation `M` of the medium follows the applied field `H` through an
//! ODE whose right-hand side depends on which way the field is *moving*.
//!
//! ```text
//! Q      = (H + α·M)/a                    L(Q) = coth Q − 1/Q   (Langevin)
//! M_diff = M_s·L(Q) − M                   δ    = sign(Ḣ)
//! δ_M    = 1 when sign(δ) == sign(M_diff), else 0
//!
//!           Ḣ · [ (1−c)·δ_M·M_diff / ((1−c)·δ·k − α·M_diff) + L′·c·M_s/a ]
//! dM/dt =   ───────────────────────────────────────────────────────────────
//!                            1 − L′·α·c·M_s/a
//! ```
//!
//! Four things fall out of that which no static curve has. The numbers are
//! this implementation's, measured by the tests below; the ones attributed to
//! a `tanh` are the design brief's, measured on an antiderivative-antialiased
//! waveshaper trimmed to the same distortion at −12 dBFS.
//!
//! * **It does not go clean when the music goes quiet.** At −36 dBFS the
//!   model still makes 0.072% THD where the `tanh` makes 0.005% — fourteen
//!   times less. That gap *is* the "tape glues a quiet mix" phenomenon, and
//!   a waveshaper cannot be trimmed into having it.
//! * **It saturates instead of squashing.** At 0 dBFS the fundamental is
//!   within a decibel of where it started and the energy has gone into
//!   harmonics; the `tanh` loses 3.35 dB of fundamental, which is a
//!   compressor wearing a saturator's name.
//! * **It distorts more as the frequency rises**, because the loop area grows
//!   with `dH/dt` and a memoryless shaper has no clock. Measured at −12 dBFS:
//!   1.11% at 50 Hz against 2.07% at 4 kHz.
//! * **It has a bias deadzone.** Underbias it and small signals barely
//!   magnetise the medium at all: with the bias knob at 2% the transfer slope
//!   near zero falls to 0.73 of the slope at −20 dBFS and the distortion of a
//!   −60 dBFS tone rises seven-hundredfold; at zero the slope ratio is 0.01
//!   and the same tone comes back at 16% distortion. A `tanh` has a slope
//!   ratio of exactly 1.000 at every drive and cannot be made to do this.
//!
//! There are **no even harmonics**. H2 measures 0.0000% at every drive, level
//! and bias setting, because the loop is odd-symmetric; its hysteresis shows
//! up as level-dependence and phase, not as second-order warmth. Any tape
//! plugin promising "even-order glue" from this model is describing something
//! else.
//!
//! # The trap in the loss equations
//!
//! The literature gives the playback head's losses as
//!
//! ```text
//! V = V₀ · e^(−k·d) · (1 − e^(−k·Δ))/(k·Δ) · sinc(k·g/2),   k = 2πf/v
//!       spacing        thickness             gap
//! ```
//!
//! and it is correct, and implementing it is a mistake. With plausible head
//! geometry — 2.5 µm spacing, 4 µm gap, 12 µm coating — it puts 15 kHz
//! **15.6 dB down** at 15 ips, with the −3 dB point at 2.5 kHz. Worn
//! geometry gives −46 dB. Both are catastrophically dark and both are right,
//! because *that curve is the head's response and not the machine's*: a real
//! recorder's reproduce EQ exists precisely to invert it, and published specs
//! are flat within a couple of decibels from 30 Hz to 18 kHz.
//!
//! So this file ships the **post-EQ** response — the net one, the one you
//! would measure at the machine's output — which is a gentle speed-scaled
//! rolloff and a head bump, and it does not offer spacing, gap and thickness
//! in microns for anyone to get lost in. `f_c = 1000·ips` puts −3 dB at
//! 15 kHz at 15 ips, exactly as specified, and it scales with speed because
//! every loss mechanism is a function of wavelength `v/f` alone.
//!
//! # The chain, and where the latency is not
//!
//! ```text
//! in ─┬───────────────────────── dry ────────────────────────────────┐
//!     │                                                              │
//!     └─ record EQ ─ ↑2 ─ hysteresis ─ ↓2 ─ makeup ─ reproduce EQ    │
//!        (+6 dB HS 3k,    RK2, f64      (min-phase   (exact inverse) │
//!         ×LP 8k)                        halfband)                   │
//!                                                                    │
//!        ─ DC block ─ wow/flutter line ─ head bump ─ HF loss ─ azimuth ─ hiss
//!          (12 Hz)     cubic tap         peaking     one-pole  (R only)
//!                                                                    │
//!                                                             mix ───┘
//! ```
//!
//! The **dry path touches nothing** — no filter, no gain stage — so `mix = 0`
//! is bit-identical to the input, as the layer requires.
//!
//! The oversampling filters are **minimum phase**, which costs the polyphase
//! halfband's free zero taps and buys back the latency: the pair's group
//! delay is 2.4 samples at the base rate, 50 µs at 48 kHz, where the
//! linear-phase version of the same filter would have carried 15. Nothing
//! here reports latency, and at 50 µs there is nothing worth reporting.
//!
//! The wow line **runs only when it has something to do**: with both
//! mechanical controls at zero the read is bypassed and the path is back to
//! the oversampler's 2.4 samples. Turning them up crossfades the line in over
//! 20 ms rather than stepping the read head, which would click.
//!
//! While it *is* running the read sits a wow excursion behind — 12.7 samples,
//! 265 µs, at the factory setting — and it is a *moving* offset rather than a
//! fixed one, which is why none of it is reported as latency. Two
//! consequences, and the second one is a feature: nothing needs delay
//! compensation, and at a partial `mix` the moving wet against the still dry
//! combs. That is not a defect to be designed out. It is flanging, and it was
//! discovered on exactly this apparatus.
//!
//! # What the makeup is, and what "level-matched" costs
//!
//! The small-signal gain of the hysteresis has a closed form,
//!
//! ```text
//! G₀ = (c·r/3) / (1 − α·c·r/3),      r = M_s/a
//! ```
//!
//! verified against the measured transfer slope to a part in a thousand, and
//! the makeup is its reciprocal. That is what makes the *drive knob* safe to
//! sweep: every position has the same small-signal gain, so turning drive up
//! changes the distortion and not the level.
//!
//! It is not, on its own, enough to make a bypass A/B honest, and the reason
//! is worth stating because it is the physics the whole effect is built on.
//! `G₀` is the **reversible** slope — the gain for a signal so small the
//! irreversible term never engages. Real programme material engages it
//! constantly: a quiet component riding under a loud, fast one comes back
//! **2.1 dB up**, because the loud one is dragging the material around its
//! loop and linearising it for the quiet one. That is not a bug; it is the
//! same physics an AC bias oscillator exploits, arriving for free out of the
//! programme's own high frequencies.
//!
//! Measured on band-limited pink at −12 dBFS peak — where this box's
//! instruments are gain-staged — `1/G₀` alone leaves the tape **about a
//! decibel louder than bypass** at every drive setting. Louder is not better,
//! so the makeup carries one more constant: a lineup gain of −1.05 dB,
//! measured rather than guessed, published as [`LINEUP_DB`], and tested. With
//! it, a programme A/B sits inside ±0.2 dB across the whole drive sweep.
//!
//! A *sine* is not programme material and does not come out level: at
//! −12 dBFS a 1 kHz tone comes back 0.8 dB down, because a tone has a third
//! of the crest factor and drives the medium far harder for the same peak.
//! Level-matched means matched on music, and the panel help says so.

use std::f64::consts::{PI, TAU};

// ---------------------------------------------------------------------------
// The shape of the thing
// ---------------------------------------------------------------------------

/// Anything smaller than this is zero, and zero is faster.
const DENORMAL_FLOOR: f64 = 1.0e-30;

/// The DC blocker's corner. Hysteresis is a *memory*: the medium keeps a
/// remanent magnetisation after the field goes away, and that remanence is a
/// DC offset at the output. It has to come off, and 12 Hz is the house's
/// number.
const DC_BLOCK_HZ: f64 = 12.0;

/// How long a smoothed gain takes to arrive, in seconds.
const SMOOTH_SECONDS: f64 = 0.015;
/// Below this a smoother chasing zero *is* zero, so that a control asked for
/// nothing gives nothing rather than a millionth of something.
const SMOOTH_SNAP: f64 = 1.0e-6;

/// How long the wow line takes to fade in or out when the mechanical
/// controls leave or reach zero, in seconds.
const ENGAGE_SECONDS: f64 = 0.020;

// ── The record chain ──

/// The record EQ's shelf: real record electronics drive the top harder,
/// which is why tape's HF headroom sits about 10 dB below its 1 kHz
/// headroom. The pair — this and its exact inverse after the medium — leaves
/// the *linear* response untouched and only moves where the distortion goes.
const EMPHASIS_HZ: f64 = 3_000.0;
const EMPHASIS_DB: f64 = 6.0;

/// ...and the ceiling on it. A record amplifier is not a shelf that rises
/// forever; the head is an inductor and the electronics run out. Cutting the
/// drive above 8 kHz is worth 15 dB of high-frequency artefact at full drive
/// (measured: a 10 kHz tone at −6 dBFS leaves −22 dBc of subharmonic debris
/// without this and −37 dBc with it) and it makes the model distort *more* in
/// the 2–6 kHz band, not less, because the inverse on the way out lifts the
/// harmonics it made. Exactly invertible, like the shelf.
const EMPHASIS_LP_HZ: f64 = 8_000.0;

// ── The medium ──

/// The mean-field coupling. Chowdhury's value, and the reference
/// implementation's.
const ALPHA: f64 = 1.6e-3;

/// The pinning parameter — how hard the domain walls stick. Fixed: it trades
/// against `drive` and `bias` and a third knob for the same axis is three
/// knobs nobody can aim.
const K_PINNING: f64 = 0.47875;

/// The damped derivative's coefficient. The DAFx paper uses the trapezoid
/// rule (`d = 1`) and says plainly that with it "the system will be unstable
/// for input signal at the Nyquist frequency"; the shipped reference quietly
/// uses 0.75 instead, and that is a large part of why 2× oversampling is
/// enough here where the paper needed 16×.
///
/// Measured, and worth knowing before anyone changes it: `d = 1` is *exactly*
/// consistent with the RK2 step — it makes `(Ḣ[n]+Ḣ[n−1])/2` equal the true
/// average slope — and it is 30 dB cleaner on a 10 kHz tone. It is also
/// marginally stable, so any transient leaves a Nyquist-frequency ring in the
/// derivative state that never decays, and a tape that never quite goes
/// silent is not shippable. 0.75.
const DERIVATIVE_DAMPING: f64 = 0.75;

/// Where the solver gives up. `|M|` past this, or not finite, and the state
/// is reset for that sample — the reference's own guard, kept because an ODE
/// with a switch in it deserves one.
const M_CEILING: f64 = 20.0;

/// The most the automatic makeup will ask for, in decibels. At the bottom of
/// the bias travel the medium's small-signal gain goes to nothing and the
/// exact reciprocal would go to infinity; the cap is where the knob stops
/// being a tone control and starts being a fault.
const MAKEUP_CEILING_DB: f64 = 18.0;

/// The lineup gain, in decibels, and the whole of what makes a bypass A/B
/// honest. See the module documentation: `1/G₀` matches the *reversible*
/// small-signal slope, programme material rides the irreversible one, and the
/// gap between them is this number.
///
/// Measured on band-limited pink at −12 dBFS peak — where this box's
/// instruments are gain-staged — across the whole drive travel: the device
/// with `1/G₀` alone runs between 1.01 and 1.07 dB hot depending on where in
/// the travel it is standing, and −1.05 dB puts every one of those settings
/// inside a fifth of a decibel of bypass.
pub const LINEUP_DB: f64 = -1.05;

// ── The transport ──

/// Wow's rate at 15 ips, in hertz. Below 4 Hz by definition — capstan and
/// reel, once per revolution.
const WOW_HZ: f64 = 0.6;
/// Flutter's rate at 15 ips. The 4–100 Hz band: idlers, bearings, guides.
const FLUTTER_HZ: f64 = 7.0;
/// Scrape flutter's rate. Above 100 Hz, from the tape sticking and slipping
/// against the heads.
const SCRAPE_HZ: f64 = 58.0;

/// The deepest wow, as a fraction of speed, at the top of the knob. The knob
/// is cubed on its way here because these are *deviation* controls and the
/// useful travel is all near the bottom: 50% is 0.10%, which is what a
/// tape effect should sound like, and 100% is a cassette that has been left
/// in a car.
const WOW_DEPTH_MAX: f64 = 0.008;
/// The same for flutter. 50% is 0.03%, which is a professional machine.
const FLUTTER_DEPTH_MAX: f64 = 0.0024;
/// Scrape rides the flutter knob at a fixed fraction of its depth. It is 0.02%
/// at the default — a quarter of a sample at 48 kHz — which sounds negligible
/// until the FM index is computed: on a 10 kHz partial it puts first-order
/// sidebands at −35 dBc, and that haze is part of the sound.
const SCRAPE_FRACTION: f64 = 0.2;

/// The read tap's floor, in samples. Four-point interpolation needs one
/// sample either side of the fraction, so the closest the head can get is one
/// whole sample behind the write.
const TAP_FLOOR: f64 = 1.0;

// ── The head ──

/// The head bump's centre at 15 ips, in hertz, and its shape. The literature
/// has the bumps of one machine moving "27 Hz to 54 Hz, 60 Hz to 120 Hz" when
/// the speed doubles, which is this proportionality.
const BUMP_HZ_AT_15: f64 = 70.0;
const BUMP_Q: f64 = 1.2;

/// The HF loss corner, in hertz per inch-per-second: 15 kHz at 15 ips, which
/// is the −3 dB point the specification asks for, and it scales with speed
/// because every loss mechanism is a function of wavelength alone.
const LOSS_HZ_PER_IPS: f64 = 1_000.0;

/// The width of one track on quarter-inch tape, in metres. Only the azimuth
/// loss uses it.
const TRACK_WIDTH_M: f64 = 2.3e-3;
/// Inches per second, in metres per second.
const IPS_TO_MS: f64 = 0.0254;
/// Where a `sinc` is 3 dB down, in radians. `sin(x)/x = 1/√2` at x = 1.3916.
const SINC_3DB: f64 = 1.3916;

/// The loudest the hiss gets, in dBFS RMS, at the top of the knob. Worse than
/// any real machine — a 15 ips professional recorder runs 65–70 dB below
/// 0 VU, which with 0 VU at −6 dBFS is about −72 dBFS — because the knob is
/// calibrated for effect and not for fidelity. The panel says dBFS rather
/// than pretending to be a signal-to-noise spec.
const HISS_MAX_DBFS: f64 = -48.0;

/// The tape speeds a machine has, in inches per second.
///
/// Three, and the three that matter. 3.75 ips is not here: the honest
/// response at that speed needs more correction than a first-order rolloff
/// can give (the physical curve is 15 dB out), and a speed whose loss model
/// is a lie is worse than a speed that is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    /// 7.5 ips — the thick, dark one. Bump at 35 Hz, top gone by 7.5 kHz.
    Slow,
    /// 15 ips — the studio standard, and the default. Bump at 70 Hz,
    /// −3 dB at 15 kHz.
    Studio,
    /// 30 ips — lean and open. The bump has moved out of the bass and into
    /// the low mids at 140 Hz, which is exactly why 30 ips has a reputation
    /// for sounding thin.
    Fast,
}

impl Speed {
    pub const ALL: [Speed; 3] = [Speed::Slow, Speed::Studio, Speed::Fast];

    #[must_use]
    pub fn from_index(index: usize) -> Self {
        Self::ALL[index.min(Self::ALL.len() - 1)]
    }

    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Slow => 0,
            Self::Studio => 1,
            Self::Fast => 2,
        }
    }

    /// Inches per second.
    #[must_use]
    pub fn ips(self) -> f64 {
        match self {
            Self::Slow => 7.5,
            Self::Studio => 15.0,
            Self::Fast => 30.0,
        }
    }

    /// How fast this is relative to the studio standard — the number every
    /// speed-dependent constant is scaled by.
    #[must_use]
    pub fn factor(self) -> f64 {
        self.ips() / 15.0
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Slow => "7.5 ips",
            Self::Studio => "15 ips",
            Self::Fast => "30 ips",
        }
    }
}

// ---------------------------------------------------------------------------
// The controls
// ---------------------------------------------------------------------------

// Percent, decibels, degrees — never a 0..1 knob fraction. A session stores
// what a control *meant*, so a range that moves later cannot silently
// re-point every saved file.

pub const PARAM_SPEED: usize = 0;
pub const PARAM_DRIVE: usize = 1;
pub const PARAM_SAT: usize = 2;
pub const PARAM_BIAS: usize = 3;
pub const PARAM_WOW: usize = 4;
pub const PARAM_FLUTTER: usize = 5;
pub const PARAM_BUMP_DB: usize = 6;
pub const PARAM_AZIMUTH_DEG: usize = 7;
pub const PARAM_HISS: usize = 8;
pub const PARAM_TRIM_DB: usize = 9;
pub const PARAM_AUTO_MAKEUP: usize = 10;
pub const PARAM_MIX: usize = 11;

/// How many controls a tape has.
pub const PARAM_COUNT: usize = 12;

/// One control, as a host sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NaturalParam {
    pub name: &'static str,
    /// `"dB"`, `"%"`, `"\u{b0}"`, or empty for the counted controls and
    /// switches.
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

/// The table every other view of the parameters is generated from.
///
/// The defaults are a lined-up 15 ips machine: **drive and saturation and
/// bias at the middle of their travel, wow 0.10% at 0.6 Hz, flutter 0.03% at
/// 7 Hz, a +1.5 dB head bump at 70 Hz, azimuth true, hiss off, automatic
/// makeup on, fully wet.** Drive at the middle is not an arbitrary
/// mid-position: it puts 3% third harmonic — the industry's definition of
/// maximum output level for magnetic tape — at −6 dBFS, which is where this
/// box's instruments peak once they are gain-staged. 0 VU lands where the
/// music is.
const PARAMS: [NaturalParam; PARAM_COUNT] = [
    NaturalParam { name: "speed", unit: "", min: 0.0, max: 2.0, default: 1.0 },
    NaturalParam { name: "drive", unit: "%", min: 0.0, max: 100.0, default: 50.0 },
    NaturalParam { name: "sat", unit: "%", min: 0.0, max: 100.0, default: 50.0 },
    NaturalParam { name: "bias", unit: "%", min: 0.0, max: 100.0, default: 50.0 },
    NaturalParam { name: "wow", unit: "%", min: 0.0, max: 100.0, default: 50.0 },
    NaturalParam { name: "flutr", unit: "%", min: 0.0, max: 100.0, default: 50.0 },
    NaturalParam { name: "bump", unit: "dB", min: 0.0, max: 3.0, default: 1.5 },
    NaturalParam { name: "azimth", unit: "\u{b0}", min: 0.0, max: 1.0, default: 0.0 },
    NaturalParam { name: "hiss", unit: "%", min: 0.0, max: 100.0, default: 0.0 },
    NaturalParam { name: "trim", unit: "dB", min: -24.0, max: 24.0, default: 0.0 },
    NaturalParam { name: "mkauto", unit: "", min: 0.0, max: 1.0, default: 1.0 },
    NaturalParam { name: "mix", unit: "%", min: 0.0, max: 100.0, default: 100.0 },
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
    let mut index = 0;
    while index < PARAM_COUNT {
        out[index] = PARAMS[index].default;
        index += 1;
    }
    out
}

fn at(params: &[f32], index: usize) -> f32 {
    params.get(index).copied().unwrap_or(0.0)
}

/// The speed a parameter vector names.
#[must_use]
pub fn speed_of(params: &[f32]) -> Speed {
    Speed::from_index(at(params, PARAM_SPEED).round().max(0.0) as usize)
}

/// Whether the makeup follows the medium rather than the trim knob.
#[must_use]
pub fn auto_makeup_on(params: &[f32]) -> bool {
    at(params, PARAM_AUTO_MAKEUP) >= 0.5
}

/// Whether a control does anything at these settings.
///
/// One control is conditional, and it is conditional on a switch sitting two
/// rows above it: the output trim is the automatic makeup's manual
/// alternative, so it is inert while the automatic is on. Everything else is
/// always live — including `bump` and `hiss` at the bottom of their travel,
/// because a control you cannot turn back on is worse than a control that is
/// doing nothing.
///
/// The panel greys what this refuses and the keys refuse to move it, so a
/// control that reads as inert is inert.
#[must_use]
pub fn uses(params: &[f32], index: usize) -> bool {
    match index {
        PARAM_TRIM_DB => !auto_makeup_on(params),
        _ => index < PARAM_COUNT,
    }
}

/// The medium's small-signal gain, in closed form.
///
/// `G₀ = (c·r/3)/(1 − α·c·r/3)` with `r = M_s/a`, which falls out of the
/// model when `L → Q/3` and the irreversible term is second order. Published
/// because it is what the makeup is built from and a test should be able to
/// ask for the number rather than re-derive it.
#[must_use]
pub fn g0_for(drive_percent: f32, sat_percent: f32, bias_percent: f32) -> f64 {
    let m = Medium::cook(drive_percent, sat_percent, bias_percent);
    m.g0
}

/// What the automatic makeup produces at these settings, in decibels,
/// whether or not it is switched on.
///
/// The lineup gain is in it: this is the number the output is actually
/// multiplied by, so the panel can show it and the manual trim can be seeded
/// from it without the level moving.
#[must_use]
pub fn auto_makeup_db(params: &[f32]) -> f64 {
    Medium::cook(at(params, PARAM_DRIVE), at(params, PARAM_SAT), at(params, PARAM_BIAS)).makeup_db
}

/// Wow's depth at a knob position, as a percentage of tape speed.
#[must_use]
pub fn wow_percent(knob_percent: f32) -> f64 {
    let k = f64::from(knob_percent.clamp(0.0, 100.0)) / 100.0;
    WOW_DEPTH_MAX * k * k * k * 100.0
}

/// Flutter's depth at a knob position, as a percentage of tape speed. Scrape
/// rides on top of it at [`SCRAPE_FRACTION`] of the same number.
#[must_use]
pub fn flutter_percent(knob_percent: f32) -> f64 {
    let k = f64::from(knob_percent.clamp(0.0, 100.0)) / 100.0;
    FLUTTER_DEPTH_MAX * k * k * k * 100.0
}

/// The peak excursion a sinusoidal speed deviation produces, in seconds.
///
/// For a deviation `D·sin(2πft)` the instantaneous frequency ratio of a
/// modulated delay is `1 − dτ/dt`, so `dτ/dt = −D·sin(2πft)` and the
/// excursion is `D/(2πf)`. **This is a tape recorder, so it is additive** —
/// the record and play heads are a fixed distance apart and a speed error
/// moves the read by `D/(2πf)` regardless of any delay. A tape *echo* is the
/// other case entirely, where the delay time *is* `d_head/v` and the error
/// scales it; that one lives in the delay, and copying this into it (or the
/// reverse) is the classic mistake.
#[must_use]
pub fn excursion_seconds(depth: f64, hz: f64) -> f64 {
    if hz <= 0.0 {
        return 0.0;
    }
    depth / (TAU * hz)
}

/// Where the head bump sits at a speed, in hertz.
#[must_use]
pub fn bump_hz(speed: Speed) -> f64 {
    BUMP_HZ_AT_15 * speed.factor()
}

/// Where the high-frequency loss reaches −3 dB at a speed, in hertz, before
/// the sample rate has its say.
#[must_use]
pub fn loss_hz(speed: Speed) -> f64 {
    LOSS_HZ_PER_IPS * speed.ips()
}

/// The corner of the extra high-frequency loss an azimuth error puts on one
/// channel, in hertz. Infinite — no filter at all — at zero.
///
/// A tilted head scans each track across the gap rather than along it, and
/// the loss is the gap-scanning `sinc`: `sin(x)/x` with
/// `x = π·W·tanθ/λ`. This answers where that is 3 dB down.
///
/// **What this is not.** The reference implementation renders azimuth as a
/// pure inter-channel *delay*, `tape_width·sinθ/v`, which at quarter-inch and
/// 15 ips is 0.29 ms per degree. That is a Haas widener whose entire
/// character is invisible until someone folds the mix to mono, at which point
/// it comb-filters. This is the mono-safe half of the same misalignment: one
/// channel loses top, the sum loses a little top, and nothing cancels.
#[must_use]
pub fn azimuth_hz(degrees: f32, speed: Speed) -> f64 {
    let degrees = f64::from(degrees.max(0.0));
    if degrees <= 0.0 {
        return f64::INFINITY;
    }
    let velocity = speed.ips() * IPS_TO_MS;
    let offset = TRACK_WIDTH_M * degrees.to_radians().tan();
    SINC_3DB * velocity / (PI * offset)
}

/// The hiss level a knob position asks for, in dBFS RMS. `None` at zero,
/// where the answer is silence rather than a number.
#[must_use]
pub fn hiss_dbfs(knob_percent: f32) -> Option<f64> {
    let level = hiss_amplitude(knob_percent);
    (level > 0.0).then(|| 20.0 * level.log10())
}

/// The RMS amplitude hiss is generated at. Squared in the knob, so the
/// useful part of the travel is not all crammed against the top.
fn hiss_amplitude(knob_percent: f32) -> f64 {
    let k = f64::from(knob_percent.clamp(0.0, 100.0)) / 100.0;
    if k <= 0.0 {
        return 0.0;
    }
    k * k * 10f64.powf(HISS_MAX_DBFS / 20.0)
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

#[inline]
fn flush(x: f64) -> f64 {
    if x.abs() < DENORMAL_FLOOR {
        0.0
    } else {
        x
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
        if self.target == 0.0 && self.value.abs() < SMOOTH_SNAP {
            self.value = 0.0;
        }
        self.value
    }

    /// A linear ramp rather than a one-pole, for the one control that has to
    /// *arrive* rather than approach: the wobbling line is either in the path
    /// or bypassed, and "asymptotically bypassed" is not bypassed.
    #[inline]
    fn advance_linear(&mut self, step: f64) -> f64 {
        if self.value < self.target {
            self.value = (self.value + step).min(self.target);
        } else if self.value > self.target {
            self.value = (self.value - step).max(self.target);
        }
        self.value
    }
}

/// A first-order high shelf, and the exact inverse of one.
///
/// `H(z) = (b0 + b1·z⁻¹)/(1 + a1·z⁻¹)`, unity at DC and `gain` at Nyquist.
/// [`Shelf::inverse`] swaps the numerator and denominator, which is a filter
/// rather than an approximation of one: the pair is transparent to the
/// arithmetic's own precision (measured: 0.0002 dB, 20 Hz–16 kHz), which is
/// what lets the record EQ redistribute the distortion without touching the
/// response.
#[derive(Clone, Copy, Default)]
struct Shelf {
    b0: f64,
    b1: f64,
    a1: f64,
    x1: f64,
    y1: f64,
}

impl Shelf {
    fn design(hz: f64, gain_db: f64, sample_rate: f64) -> Self {
        let gain = 10f64.powf(gain_db / 20.0);
        let w = (PI * hz.clamp(1.0, sample_rate * 0.49) / sample_rate).tan();
        let pole = w * gain.sqrt();
        let zero = pole / gain;
        let b0 = (1.0 + zero) / (1.0 + pole);
        let b1 = (zero - 1.0) / (1.0 + pole);
        let a1 = (pole - 1.0) / (1.0 + pole);
        // Normalised so DC is exactly unity, whatever the bilinear warping
        // did to the corner.
        let dc = (b0 + b1) / (1.0 + a1);
        Self { b0: b0 / dc, b1: b1 / dc, a1, x1: 0.0, y1: 0.0 }
    }

    fn inverse(&self) -> Self {
        Self { b0: 1.0 / self.b0, b1: self.a1 / self.b0, a1: self.b1 / self.b0, x1: 0.0, y1: 0.0 }
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 - self.a1 * self.y1;
        self.x1 = flush(x);
        self.y1 = flush(y);
        y
    }

    fn clear(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

/// A one-pole low-pass, and the exact inverse of one.
///
/// The inverse is an FIR — `x = (y − (1−a)·y₋₁)/a` — so it is unconditionally
/// stable however the corner is placed, which is the property that lets the
/// record chain's ceiling be undone exactly rather than approximately.
#[derive(Clone, Copy, Default)]
struct Emphasis {
    a: f64,
    z: f64,
}

impl Emphasis {
    fn design(hz: f64, sample_rate: f64) -> Self {
        let corner = hz.clamp(1.0, sample_rate * 0.45);
        Self { a: 1.0 - (-TAU * corner / sample_rate).exp(), z: 0.0 }
    }

    #[inline]
    fn low_pass(&mut self, x: f64) -> f64 {
        self.z += self.a * (x - self.z);
        self.z = flush(self.z);
        self.z
    }

    #[inline]
    fn undo(&mut self, y: f64) -> f64 {
        let x = (y - (1.0 - self.a) * self.z) / self.a;
        self.z = flush(y);
        x
    }

    fn clear(&mut self) {
        self.z = 0.0;
    }
}

/// A trapezoidal one-pole low-pass — the head's high-frequency loss, and the
/// azimuth's.
///
/// The trapezoidal form rather than the impulse-invariant one because its
/// −3 dB point lands exactly on the designed corner at every sample rate, and
/// the corner *is* the specification here.
#[derive(Clone, Copy, Default)]
struct Tpt {
    g: f64,
    z: f64,
    /// Set when the corner is at or above the band this rate can carry, in
    /// which case the filter is a wire and says so.
    open: bool,
}

impl Tpt {
    fn design(hz: f64, sample_rate: f64) -> Self {
        if !hz.is_finite() || hz >= sample_rate * 0.45 {
            return Self { g: 1.0, z: 0.0, open: true };
        }
        let corner = hz.clamp(1.0, sample_rate * 0.45);
        let g = (PI * corner / sample_rate).tan();
        Self { g: g / (1.0 + g), z: 0.0, open: false }
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        if self.open {
            return x;
        }
        let v = (x - self.z) * self.g;
        let low = v + self.z;
        self.z = flush(low + v);
        low
    }

    fn clear(&mut self) {
        self.z = 0.0;
    }
}

/// An RBJ peaking biquad in transposed direct form II — the head bump.
#[derive(Clone, Copy)]
struct Peaking {
    b: [f64; 3],
    a: [f64; 2],
    z1: f64,
    z2: f64,
    /// True when the gain is zero and the filter is a wire.
    open: bool,
}

impl Peaking {
    fn new() -> Self {
        Self { b: [1.0, 0.0, 0.0], a: [0.0, 0.0], z1: 0.0, z2: 0.0, open: true }
    }

    fn design(&mut self, hz: f64, q: f64, gain_db: f64, sample_rate: f64) {
        if !(hz.is_finite() && sample_rate > 0.0) {
            return;
        }
        if gain_db.abs() < 1.0e-6 {
            self.open = true;
            return;
        }
        self.open = false;
        let a = 10f64.powf(gain_db / 40.0);
        let w = TAU * hz.clamp(1.0, sample_rate * 0.49) / sample_rate;
        let (sin_w, cos_w) = w.sin_cos();
        let alpha = sin_w / (2.0 * q.max(0.05));
        let a0 = 1.0 + alpha / a;
        self.b = [(1.0 + alpha * a) / a0, (-2.0 * cos_w) / a0, (1.0 - alpha * a) / a0];
        self.a = [(-2.0 * cos_w) / a0, (1.0 - alpha / a) / a0];
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        if self.open {
            return x;
        }
        let y = self.b[0] * x + self.z1;
        self.z1 = flush(self.b[1] * x - self.a[0] * y + self.z2);
        self.z2 = flush(self.b[2] * x - self.a[1] * y);
        y
    }

    fn clear(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// One-pole DC blocker.
#[derive(Clone, Copy, Default)]
struct DcBlock {
    x1: f64,
    y1: f64,
    a: f64,
}

impl DcBlock {
    fn design(hz: f64, sample_rate: f64) -> Self {
        Self { x1: 0.0, y1: 0.0, a: (-TAU * hz / sample_rate).exp().clamp(0.0, 0.9999) }
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let y = x - self.x1 + self.a * self.y1;
        self.x1 = flush(x);
        self.y1 = flush(y);
        y
    }

    fn clear(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

// ── Noise ──

#[inline]
fn mix32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// What the Kellet sum has to be multiplied by to come out at unit RMS.
/// Measured over ten seconds of it, because the filter's own gain is not a
/// number anybody should be deriving by hand.
const PINK_SCALE: f64 = 0.583_9;

/// Pink noise by the Kellet filter, unit RMS and independent per channel.
///
/// Pink and not white because tape hiss is pink to within a couple of
/// decibels across the band, and because white noise on a mix bus sounds
/// like a fault rather than like tape.
#[derive(Clone, Copy)]
struct Pink {
    state: u32,
    stride: u32,
    b: [f64; 3],
}

impl Pink {
    fn new(seed: u32) -> Self {
        Self { state: mix32(seed), stride: seed | 1, b: [0.0; 3] }
    }

    #[inline]
    fn next(&mut self) -> f64 {
        self.state = self.state.wrapping_add(self.stride);
        let white = f64::from(mix32(self.state) >> 8) / f64::from(1u32 << 23) - 1.0;
        self.b[0] = 0.997_65 * self.b[0] + white * 0.099_046_0;
        self.b[1] = 0.963_00 * self.b[1] + white * 0.296_516_4;
        self.b[2] = 0.570_00 * self.b[2] + white * 1.052_691_3;
        // The scale brings the sum to unit RMS, measured over ten seconds of
        // it, so that the hiss knob's dBFS reading is the level it says.
        (self.b[0] + self.b[1] + self.b[2] + white * 0.184_8) * PINK_SCALE
    }

    fn clear(&mut self) {
        self.b = [0.0; 3];
    }
}

// ── The delay line the transport wobbles ──

/// A short delay line with a fractional read.
///
/// Written before it is read, so a read of one sample back is the sample just
/// written and the interpolator's four taps always exist. The capacity is a
/// power of two, so wrapping is one `AND`.
struct Line {
    buf: Vec<f32>,
    mask: usize,
    pos: usize,
    limit: f64,
}

impl Line {
    fn new(len: usize) -> Self {
        let capacity = len.max(16).next_power_of_two();
        Self { buf: vec![0.0; capacity], mask: capacity - 1, pos: 0, limit: (capacity - 4) as f64 }
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }

    #[inline]
    fn write(&mut self, x: f64) {
        let v = x as f32;
        self.buf[self.pos] = if f64::from(v).abs() < DENORMAL_FLOOR { 0.0 } else { v };
        self.pos = (self.pos + 1) & self.mask;
    }

    /// A fractional read, four-point third-order Lagrange.
    ///
    /// Cubic and not linear because the tap moves: swept across one whole
    /// sample on a 10 kHz tone, linear interpolation ripples 2.03 dB
    /// peak-to-peak and cubic 0.56 dB, and two decibels of amplitude
    /// modulation on the top octave once per wow cycle is a chorus that has
    /// nothing to do with tape.
    ///
    /// **The `+ 1` in the base is the whole reason [`TAP_FLOOR`] is one and
    /// not two.** This line is written *before* it is read, so the sample at
    /// `pos − 1` is the one that just arrived and its delay is zero; the four
    /// taps straddle the requested delay, so the newest of them is one sample
    /// *ahead* of it and a read of one sample back is the closest the head
    /// can get. Getting this off by one does not sound broken — it quietly
    /// mixes 6% of the buffer's oldest sample into every read, which on a
    /// steady tone is a phase-shifted copy whose weight follows the wow, and
    /// the measured flutter depth then depends on the carrier frequency and
    /// nulls entirely at some of them.
    #[inline]
    fn tap_cubic(&self, back: f64) -> f64 {
        let back = back.max(TAP_FLOOR).min(self.limit);
        let whole = back.floor();
        let d = back - whole;
        let base = self.pos.wrapping_sub(whole as usize + 1);
        let mask = self.mask;
        let buf = &self.buf[..=mask];
        let ym1 = f64::from(buf[base.wrapping_add(1) & mask]);
        let y0 = f64::from(buf[base & mask]);
        let y1 = f64::from(buf[base.wrapping_sub(1) & mask]);
        let y2 = f64::from(buf[base.wrapping_sub(2) & mask]);
        let c0 = -d * (d - 1.0) * (d - 2.0) / 6.0;
        let c1 = (d + 1.0) * (d - 1.0) * (d - 2.0) * 0.5;
        let c2 = -(d + 1.0) * d * (d - 2.0) * 0.5;
        let c3 = (d + 1.0) * d * (d - 1.0) / 6.0;
        c0.mul_add(ym1, c1.mul_add(y0, c2.mul_add(y1, c3 * y2)))
    }
}

// ---------------------------------------------------------------------------
// Two times, and the filter that gets us there
// ---------------------------------------------------------------------------

/// The oversampling filter, minimum phase.
///
/// A 31-tap Kaiser halfband — passband to 0.38 of the base rate flat within
/// 0.006 dB, stopband from 0.62 of it down 67.7 dB — factored into its
/// minimum-phase form. The linear-phase original carries 15 samples of delay
/// at the doubled rate; this carries 2.4 at the base rate for the
/// interpolator and decimator *together*, and this box does not report
/// latency because nothing in it has any worth reporting.
///
/// Two times and not sixteen. The paper's 16× exists to host a real 55 kHz
/// bias tone and we do not have one, so the requirement collapses to alias
/// rejection: measured on two tones at 5.5 and 7.1 kHz at −6 dBFS each with
/// the drive at maximum, the worst non-harmonic product is −63.8 dBc at 2×
/// against −45.6 dBc at 1×.
const HALFBAND: [f64; 31] = [
    1.722_128_518_035_795e-2,
    1.031_713_265_776_887e-1,
    2.802_271_107_654_785e-1,
    4.254_891_154_434_087e-1,
    3.355_661_815_319_846e-1,
    2.138_983_797_885_064e-2,
    -2.015_341_864_008_128e-1,
    -1.074_905_527_224_605e-1,
    1.019_466_484_434_758e-1,
    1.057_979_381_183_314e-1,
    -4.899_980_165_754_101e-2,
    -8.518_021_939_915_828e-2,
    2.247_233_790_419_471e-2,
    6.308_862_430_681_114e-2,
    -9.808_305_096_766_16e-3,
    -4.404_752_713_683_208e-2,
    4.253_780_722_500_224e-3,
    2.903_634_892_482_064e-2,
    -2.158_439_817_612_334e-3,
    -1.794_315_313_340_966e-2,
    1.544_559_997_153_264e-3,
    1.026_676_736_013_755e-2,
    -1.369_931_879_800_565e-3,
    -5.291_189_639_242_592e-3,
    1.274_511_352_487_694e-3,
    2.443_846_156_775_288e-3,
    -9.863_837_090_284_282e-4,
    -9.395_974_451_021_351e-4,
    6.208_499_520_409_597e-4,
    2.424_183_024_190_434e-4,
    -3.042_009_811_501_558e-4,
];

/// How many taps the halfband has, and how long each polyphase branch is.
const TAPS: usize = HALFBAND.len();
const BRANCH: usize = TAPS.div_ceil(2);

/// The two polyphase branches of the interpolator, already doubled for the
/// gain a zero-stuffed upsampler loses.
const UP_EVEN: [f64; BRANCH] = up_branch(0);
const UP_ODD: [f64; BRANCH] = up_branch(1);

const fn up_branch(phase: usize) -> [f64; BRANCH] {
    let mut out = [0.0; BRANCH];
    let mut i = 0;
    while i < BRANCH {
        let tap = 2 * i + phase;
        out[i] = if tap < TAPS { 2.0 * HALFBAND[tap] } else { 0.0 };
        i += 1;
    }
    out
}

/// The interpolator and decimator for one channel.
///
/// Both histories are written twice — once at the head and once a window
/// later — so the dot product is over a contiguous slice and the compiler can
/// vectorise it, rather than a wrapping index it cannot.
struct Oversampler {
    up: [f64; BRANCH * 2],
    up_pos: usize,
    down: [f64; TAPS * 2],
    down_pos: usize,
}

impl Oversampler {
    fn new() -> Self {
        Self { up: [0.0; BRANCH * 2], up_pos: 0, down: [0.0; TAPS * 2], down_pos: 0 }
    }

    fn clear(&mut self) {
        self.up = [0.0; BRANCH * 2];
        self.down = [0.0; TAPS * 2];
        self.up_pos = 0;
        self.down_pos = 0;
    }

    /// One input sample in, two oversampled samples out, in time order.
    #[inline]
    fn upsample(&mut self, x: f64) -> (f64, f64) {
        self.up_pos = if self.up_pos == 0 { BRANCH - 1 } else { self.up_pos - 1 };
        self.up[self.up_pos] = x;
        self.up[self.up_pos + BRANCH] = x;
        let window = &self.up[self.up_pos..self.up_pos + BRANCH];
        (dot(&UP_EVEN, window), dot(&UP_ODD, window))
    }

    /// Two oversampled samples in, one output sample out.
    #[inline]
    fn downsample(&mut self, first: f64, second: f64) -> f64 {
        for value in [first, second] {
            self.down_pos = if self.down_pos == 0 { TAPS - 1 } else { self.down_pos - 1 };
            self.down[self.down_pos] = value;
            self.down[self.down_pos + TAPS] = value;
        }
        dot(&HALFBAND, &self.down[self.down_pos..self.down_pos + TAPS])
    }
}

/// The dot product at the heart of both filters, on four accumulators.
///
/// The obvious single-accumulator loop is *latency* bound rather than
/// throughput bound: every multiply-add waits for the one before it, and at
/// four cycles apiece a 31-tap filter is 124 cycles of a processor that could
/// have issued four of them per cycle. Four partial sums break the chain and
/// measured 40% off the whole effect's cost. The summation order changes,
/// which is why this is one function and not four hand-unrolled copies.
#[inline]
fn dot(h: &[f64], window: &[f64]) -> f64 {
    let n = h.len().min(window.len());
    let mut acc = [0.0f64; 4];
    let mut i = 0;
    while i + 4 <= n {
        acc[0] = h[i].mul_add(window[i], acc[0]);
        acc[1] = h[i + 1].mul_add(window[i + 1], acc[1]);
        acc[2] = h[i + 2].mul_add(window[i + 2], acc[2]);
        acc[3] = h[i + 3].mul_add(window[i + 3], acc[3]);
        i += 4;
    }
    let mut tail = 0.0;
    while i < n {
        tail = h[i].mul_add(window[i], tail);
        i += 1;
    }
    (acc[0] + acc[1]) + (acc[2] + acc[3]) + tail
}

// ---------------------------------------------------------------------------
// The medium
// ---------------------------------------------------------------------------

/// Everything the hysteresis needs that only changes when a knob does.
#[derive(Clone, Copy)]
struct Medium {
    m_s: f64,
    /// `1/a`, because the inner loop divides by `a` twice per evaluation and
    /// a reciprocal computed once a block is a reciprocal not computed
    /// 384 000 times a second.
    inv_a: f64,
    /// `1 − c`, which appears three times per evaluation.
    nc: f64,
    /// `c·M_s/a`, the reversible term's coefficient.
    reversible: f64,
    /// `α·c·M_s/a`, which is the same thing scaled — it is the only term in
    /// the equation's denominator, and it is small.
    reversible_alpha: f64,
    /// The closed-form small-signal gain.
    g0: f64,
    /// The gain the output is multiplied by when the makeup is automatic,
    /// in decibels — `1/G₀` with the lineup gain folded in, capped.
    makeup_db: f64,
}

impl Medium {
    /// The three knobs, mapped.
    ///
    /// **Drive is the ratio `r = M_s/a`, on `[1, 7]`.** The reference maps
    /// `a = M_s/(0.01 + 6·drive)`, which lets `r` fall to 0.01 at the bottom
    /// of the knob and drives the makeup to +53 dB; bounding `r` below at
    /// unity caps it at +12.7 dB and costs nothing audible.
    ///
    /// **Bias is the knob and is not inverted anywhere.** The reference
    /// carries an internal *width* and inverts it at the call site, one file
    /// away from the mapping — so a reader of either file alone gets the
    /// deadzone backwards. Here low bias is a wide loop is a deadzone, which
    /// is also what the physics says.
    fn cook(drive_percent: f32, sat_percent: f32, bias_percent: f32) -> Self {
        let drive = f64::from(drive_percent.clamp(0.0, 100.0)) / 100.0;
        let sat = f64::from(sat_percent.clamp(0.0, 100.0)) / 100.0;
        let bias = f64::from(bias_percent.clamp(0.0, 100.0)) / 100.0;

        let m_s = 0.5 + 1.5 * (1.0 - sat);
        let r = 1.0 + 6.0 * drive;
        let a = m_s / r;
        let c = (bias.sqrt() - 0.01).clamp(0.0, 0.99);

        let cr3 = c * r / 3.0;
        let g0 = cr3 / (1.0 - ALPHA * cr3);
        let makeup_db = if g0 > 1.0e-9 {
            (-20.0 * g0.log10()).min(MAKEUP_CEILING_DB) + LINEUP_DB
        } else {
            MAKEUP_CEILING_DB + LINEUP_DB
        };

        let reversible = c * m_s / a;
        Self {
            m_s,
            inv_a: 1.0 / a,
            nc: 1.0 - c,
            reversible,
            reversible_alpha: ALPHA * reversible,
            g0,
            makeup_db,
        }
    }
}

/// The state the ODE carries between samples: the magnetisation, the field
/// and the field's damped derivative.
#[derive(Clone, Copy, Default)]
struct Hysteresis {
    m: f64,
    h: f64,
    h_dot: f64,
}

impl Hysteresis {
    /// The right-hand side of the differential equation.
    ///
    /// Everything stays in registers. The reference writes the intermediates
    /// into struct fields so that a Newton–Raphson solver can share them with
    /// the derivative of this function; we ship RK2 only, never need that
    /// derivative, and keeping the scratch in locals measures twice as fast.
    /// Three reciprocals have been taken out of the obvious form of this and
    /// none of them changed the arithmetic by more than a part in ten
    /// million, which is worth saying because divides do not pipeline and
    /// this runs four times per sample per channel:
    ///
    /// * `1/a` is a per-block constant.
    /// * `coth Q` and `1/Q` come out of the *same* reciprocal:
    ///   `1/(Q·tanh Q)` scaled by `Q` and by `tanh Q` respectively.
    /// * the equation's own denominator is `1 − ε` with
    ///   `ε = α·c·M_s/a ≤ 0.005`, so two terms of its series are exact to
    ///   nine decimal places and a divide becomes two multiply-adds.
    #[inline]
    fn slope(m: f64, h: f64, h_dot: f64, k: &Medium) -> f64 {
        let q = (h + m * ALPHA) * k.inv_a;
        // The Langevin function and its derivative are singular at zero and
        // the series is not optional: `coth` overflows there.
        let (l, l_prime) = if q.abs() < 1.0e-3 {
            (q * (1.0 / 3.0), 1.0 / 3.0)
        } else {
            let t = q.tanh();
            let recip = 1.0 / (q * t);
            let coth = q * recip;
            let inv = t * recip;
            (coth - inv, inv * inv - coth * coth + 1.0)
        };
        let m_diff = k.m_s * l - m;
        let delta = if h_dot >= 0.0 { 1.0 } else { -1.0 };
        let delta_m = f64::from((delta > 0.0) == (m_diff > 0.0));
        let denominator = k.nc * delta * K_PINNING - ALPHA * m_diff;
        let irreversible = k.nc * delta_m * m_diff / denominator;
        let reversible = l_prime * k.reversible;
        let epsilon = l_prime * k.reversible_alpha;
        h_dot * (irreversible + reversible) * (1.0 + epsilon * (1.0 + epsilon))
    }

    /// One step of RK2 over one oversampled sample.
    ///
    /// RK2 and not RK4, and no selector for it. RK4 costs 2.2× for a
    /// difference nobody has been able to name, and the reference's own manual
    /// frames its highest solver as "for mix busses and key tracks", which is
    /// a CPU confession dressed as a feature. One sound.
    #[inline]
    fn step(&mut self, h: f64, t: f64, k: &Medium) -> f64 {
        let mut h_dot = ((1.0 + DERIVATIVE_DAMPING) / t) * (h - self.h)
            - DERIVATIVE_DAMPING * self.h_dot;
        let k1 = t * Self::slope(self.m, self.h, self.h_dot, k);
        let k2 = t
            * Self::slope(
                self.m + k1 * 0.5,
                (h + self.h) * 0.5,
                (h_dot + self.h_dot) * 0.5,
                k,
            );
        let mut m = self.m + k2;
        // The reference's own guard, and an ODE with a switch in it deserves
        // one: an excursion this large is a solver that has left the physics
        // behind, and the honest recovery is to drop the state rather than to
        // let it come back as a bang.
        if !m.is_finite() || m.abs() > M_CEILING {
            m = 0.0;
            h_dot = 0.0;
        }
        self.m = flush(m);
        self.h = flush(h);
        // The derivative is flushed too, and at a threshold far above the
        // denormal range: it decays by a quarter per sample once the field
        // stops moving, and without this it would spend a couple of thousand
        // samples in the region where the arithmetic goes slow.
        self.h_dot = flush(h_dot);
        self.m
    }

    fn clear(&mut self) {
        self.m = 0.0;
        self.h = 0.0;
        self.h_dot = 0.0;
    }
}

// ---------------------------------------------------------------------------
// One channel of tape
// ---------------------------------------------------------------------------

/// Everything that is per-channel: the medium, the filters around it and the
/// line the transport wobbles.
struct Channel {
    pre_shelf: Shelf,
    post_shelf: Shelf,
    pre_lp: Emphasis,
    post_lp: Emphasis,
    os: Oversampler,
    hysteresis: Hysteresis,
    dc: DcBlock,
    line: Line,
    bump: Peaking,
    loss: Tpt,
    azimuth: Tpt,
    hiss: Pink,
}

impl Channel {
    fn new(seed: u32) -> Self {
        Self {
            pre_shelf: Shelf::default(),
            post_shelf: Shelf::default(),
            pre_lp: Emphasis::default(),
            post_lp: Emphasis::default(),
            os: Oversampler::new(),
            hysteresis: Hysteresis::default(),
            dc: DcBlock::default(),
            line: Line::new(16),
            bump: Peaking::new(),
            loss: Tpt::default(),
            azimuth: Tpt::default(),
            hiss: Pink::new(seed),
        }
    }

    fn clear(&mut self) {
        self.pre_shelf.clear();
        self.post_shelf.clear();
        self.pre_lp.clear();
        self.post_lp.clear();
        self.os.clear();
        self.hysteresis.clear();
        self.dc.clear();
        self.line.clear();
        self.bump.clear();
        self.loss.clear();
        self.azimuth.clear();
        self.hiss.clear();
    }
}

// ---------------------------------------------------------------------------
// The tape
// ---------------------------------------------------------------------------

/// A stereo tape machine: a medium, a transport and a head.
pub struct Tape {
    sample_rate: f64,
    params: [f32; PARAM_COUNT],

    channel: [Channel; 2],

    // ── Per-block state, resolved from the controls ──
    medium: Medium,
    /// The oversampled step, in seconds.
    step: f64,
    /// The three excursions, in samples at this rate.
    wow_samples: f64,
    flutter_samples: f64,
    scrape_samples: f64,
    /// Phase increments per sample, in cycles.
    wow_step: f64,
    flutter_step: f64,
    scrape_step: f64,
    /// Whether the transport wobbles at all.
    moving: bool,

    // ── Running state ──
    wow_phase: f64,
    flutter_phase: f64,
    scrape_phase: f64,

    // ── Smoothed gains ──
    smooth_a: f64,
    makeup: Smoother,
    hiss: Smoother,
    mix: Smoother,
    /// How much of the wobbling line is in the path: 0 is the line bypassed
    /// and the output exactly what went in, 1 is the line alone.
    engage: Smoother,
    engage_step: f64,
}

impl Tape {
    /// Build one at a sample rate, with every buffer it will ever need.
    #[must_use]
    pub fn new(sample_rate: f64) -> Self {
        let mut tape = Self {
            sample_rate: 48_000.0,
            params: default_natural_params(),
            channel: [Channel::new(0x9E37_79B9), Channel::new(0x85EB_CA6B)],
            medium: Medium::cook(50.0, 50.0, 50.0),
            step: 1.0 / 96_000.0,
            wow_samples: 0.0,
            flutter_samples: 0.0,
            scrape_samples: 0.0,
            wow_step: 0.0,
            flutter_step: 0.0,
            scrape_step: 0.0,
            moving: true,
            wow_phase: 0.0,
            flutter_phase: 0.0,
            scrape_phase: 0.0,
            smooth_a: 1.0,
            makeup: Smoother::default(),
            hiss: Smoother::default(),
            mix: Smoother::default(),
            engage: Smoother::default(),
            engage_step: 1.0,
        };
        tape.build(sample_rate);
        tape.snap();
        tape
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
        let fs = if sample_rate.is_finite() && sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        self.sample_rate = fs;
        self.step = 1.0 / (2.0 * fs);
        self.smooth_a = 1.0 - (-1.0 / (SMOOTH_SECONDS * fs)).exp();
        self.engage_step = 1.0 / (ENGAGE_SECONDS * fs);

        // The line only ever holds the wobble, so it is tiny: twice the
        // deepest excursion the knobs allow, at the slowest speed, plus the
        // interpolator's own margin. Absolute, not proportional to a delay
        // time — that is the difference between a tape recorder and a tape
        // echo, and it is why this buffer is a kilobyte rather than a
        // megabyte.
        let slowest = Speed::Slow.factor();
        let deepest = excursion_seconds(WOW_DEPTH_MAX, WOW_HZ * slowest)
            + excursion_seconds(FLUTTER_DEPTH_MAX, FLUTTER_HZ * slowest)
            + excursion_seconds(FLUTTER_DEPTH_MAX * SCRAPE_FRACTION, SCRAPE_HZ * slowest);
        let frames = (2.0 * deepest * fs).ceil() as usize + 8;
        for channel in &mut self.channel {
            channel.line = Line::new(frames);
            channel.pre_shelf = Shelf::design(EMPHASIS_HZ, EMPHASIS_DB, fs);
            channel.post_shelf = channel.pre_shelf.inverse();
            channel.pre_lp = Emphasis::design(EMPHASIS_LP_HZ, fs);
            channel.post_lp = Emphasis::design(EMPHASIS_LP_HZ, fs);
            channel.dc = DcBlock::design(DC_BLOCK_HZ, fs);
        }

        self.resolve();
        self.reset();
    }

    /// Drop every tail: the medium to zero, the line to silence, the filters
    /// to rest.
    pub fn reset(&mut self) {
        for channel in &mut self.channel {
            channel.clear();
        }
        self.wow_phase = 0.0;
        self.flutter_phase = 0.0;
        self.scrape_phase = 0.0;
    }

    /// Take every smoothed gain straight to its target.
    ///
    /// A session load sets the controls before the effect is in a slot, and
    /// those controls are glide targets. Snapping means the first block a
    /// loaded session renders is the tape that was saved rather than the
    /// factory one gliding towards it.
    pub fn snap(&mut self) {
        self.resolve();
        self.makeup.snap(self.makeup.target);
        self.hiss.snap(self.hiss.target);
        self.mix.snap(self.mix.target);
        self.engage.snap(self.engage.target);
    }

    // ── Parameters ──

    /// One control, in its own unit. Real-time safe.
    pub fn set_param_natural(&mut self, index: usize, value: f32) {
        let Some(info) = natural_param(index) else { return };
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

    /// The speed the transport is running.
    #[must_use]
    pub fn speed(&self) -> Speed {
        speed_of(&self.params)
    }

    /// The gain the output is being multiplied by, in decibels.
    #[must_use]
    pub fn makeup_db(&self) -> f64 {
        if auto_makeup_on(&self.params) {
            self.medium.makeup_db
        } else {
            f64::from(self.params[PARAM_TRIM_DB])
        }
    }

    /// Everything that is settled once per block.
    fn resolve(&mut self) {
        let params = self.params;
        self.medium = Medium::cook(
            params[PARAM_DRIVE],
            params[PARAM_SAT],
            params[PARAM_BIAS],
        );
        let speed = speed_of(&params);
        let factor = speed.factor();
        let fs = self.sample_rate;

        let wow_depth = wow_percent(params[PARAM_WOW]) / 100.0;
        let flutter_depth = flutter_percent(params[PARAM_FLUTTER]) / 100.0;
        let wow_hz = WOW_HZ * factor;
        let flutter_hz = FLUTTER_HZ * factor;
        let scrape_hz = SCRAPE_HZ * factor;
        self.wow_samples = excursion_seconds(wow_depth, wow_hz) * fs;
        self.flutter_samples = excursion_seconds(flutter_depth, flutter_hz) * fs;
        self.scrape_samples =
            excursion_seconds(flutter_depth * SCRAPE_FRACTION, scrape_hz) * fs;
        self.wow_step = wow_hz / fs;
        self.flutter_step = flutter_hz / fs;
        self.scrape_step = scrape_hz / fs;
        self.moving = wow_depth > 0.0 || flutter_depth > 0.0;

        let bump_db = f64::from(params[PARAM_BUMP_DB]);
        let bump_hz = bump_hz(speed);
        let loss_hz = loss_hz(speed);
        let azimuth_hz = azimuth_hz(params[PARAM_AZIMUTH_DEG], speed);
        for (index, channel) in self.channel.iter_mut().enumerate() {
            channel.bump.design(bump_hz, BUMP_Q, bump_db, fs);
            channel.loss = keep_state(channel.loss, Tpt::design(loss_hz, fs));
            // The tilt lands on the right channel only. Both channels of a
            // stereo pair lose the same top to a tilted gap; what a
            // misalignment actually does is put them at different heights on
            // the tape, and rendering that as a delay is the mono-compatible
            // trap this refuses to ship.
            let corner = if index == 1 { azimuth_hz } else { f64::INFINITY };
            channel.azimuth = keep_state(channel.azimuth, Tpt::design(corner, fs));
        }

        self.makeup.target = 10f64.powf(self.makeup_db() / 20.0);
        self.hiss.target = hiss_amplitude(params[PARAM_HISS]);
        self.mix.target = f64::from(params[PARAM_MIX]) / 100.0;
        self.engage.target = f64::from(u8::from(self.moving));
    }

    // ── Rendering ──

    /// Rewrite one block in place.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.resolve();
        let frames = left.len().min(right.len());
        for i in 0..frames {
            let (l, r) = self.process_sample(left[i], right[i]);
            left[i] = l;
            right[i] = r;
        }
    }

    /// One frame.
    ///
    /// At `mix == 0` the medium still runs — a tape that has been magnetised
    /// must not forget it because the blend knob is down — and the input is
    /// returned *itself* rather than the input plus zero times the wet.
    /// Bit-identical dry is a property of the control flow rather than of
    /// floating-point luck, and `−0.0 + 0.0` is `+0.0`, which is why the
    /// difference matters.
    #[inline]
    pub fn process_sample(&mut self, left: f32, right: f32) -> (f32, f32) {
        let a = self.smooth_a;
        let makeup = self.makeup.advance(a);
        let hiss = self.hiss.advance(a);
        let mix = self.mix.advance(a);
        let engage = self.engage.advance_linear(self.engage_step);

        let offset = self.wobble(engage);
        let wet_l = self.run(0, f64::from(left), makeup, hiss, offset, engage);
        let wet_r = self.run(1, f64::from(right), makeup, hiss, offset, engage);

        if mix == 0.0 {
            return (left, right);
        }
        // A crossfade, not an addition: at 100% this is the tape and not the
        // tape *plus* the source, which would be a parallel path nobody asked
        // for and a comb filter nobody wants.
        let dry = 1.0 - mix as f32;
        (
            (wet_l as f32).mul_add(mix as f32, left * dry),
            (wet_r as f32).mul_add(mix as f32, right * dry),
        )
    }

    /// Where the read head is this sample, in samples behind the write.
    ///
    /// **The `(1 − cos)` form is what keeps this effect zero-latency.** A
    /// centred sine needs a positive offset at least as large as its deepest
    /// negative excursion, and that offset is real latency — 4 ms of it at
    /// the bottom of the speed range with the wow knob up. `Δτ·(1 − cos)`
    /// puts the excursion on `[0, 2Δτ]` instead, so the minimum delay is
    /// zero, while leaving the *pitch* modulation exactly symmetric: pitch is
    /// `−dτ/dt`, and the derivative of `Δτ(1 − cos 2πft)` is the `D·sin(2πft)`
    /// that was asked for.
    #[inline]
    fn wobble(&mut self, engage: f64) -> f64 {
        if engage <= 0.0 {
            // Held at the top of the cycle, so that turning the transport
            // back on starts from zero excursion rather than from wherever
            // the phase happened to have got to.
            self.wow_phase = 0.0;
            self.flutter_phase = 0.0;
            self.scrape_phase = 0.0;
            return TAP_FLOOR;
        }
        self.wow_phase = wrap_phase(self.wow_phase + self.wow_step);
        self.flutter_phase = wrap_phase(self.flutter_phase + self.flutter_step);
        self.scrape_phase = wrap_phase(self.scrape_phase + self.scrape_step);
        let wow = self.wow_samples * (1.0 - (TAU * self.wow_phase).cos());
        let flutter = self.flutter_samples * (1.0 - (TAU * self.flutter_phase).cos());
        let scrape = self.scrape_samples * (1.0 - (TAU * self.scrape_phase).cos());
        TAP_FLOOR + wow + flutter + scrape
    }

    /// One channel, end to end.
    #[inline]
    fn run(
        &mut self,
        index: usize,
        x: f64,
        makeup: f64,
        hiss: f64,
        offset: f64,
        engage: f64,
    ) -> f64 {
        let step = self.step;
        let medium = self.medium;
        let channel = &mut self.channel[index];

        // ── The record chain ──
        let recorded = channel.pre_lp.low_pass(channel.pre_shelf.tick(x));

        // ── The medium, at twice the rate ──
        let (first, second) = channel.os.upsample(recorded);
        let m1 = channel.hysteresis.step(first, step, &medium);
        let m2 = channel.hysteresis.step(second, step, &medium);
        let played = channel.os.downsample(m1, m2) * makeup;

        // ── The reproduce chain ──
        let mut y = channel.post_shelf.tick(channel.post_lp.undo(played));
        y = channel.dc.tick(y);

        // ── The transport ──
        channel.line.write(y);
        if engage > 0.0 {
            let tapped = channel.line.tap_cubic(offset);
            y += (tapped - y) * engage;
        }

        // ── The head ──
        y = channel.azimuth.tick(channel.loss.tick(channel.bump.tick(y)));
        if hiss > 0.0 {
            y += channel.hiss.next() * hiss;
        }
        y
    }
}

/// A filter redesigned without dropping what it was in the middle of.
///
/// Coefficients are resolved every block; carrying the state across means a
/// knob that moves does not restart the filter and click.
fn keep_state(old: Tpt, new: Tpt) -> Tpt {
    Tpt { z: old.z, ..new }
}

#[inline]
fn wrap_phase(phase: f64) -> f64 {
    if phase >= 1.0 {
        phase - phase.floor()
    } else {
        phase
    }
}

impl Default for Tape {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f64 = 48_000.0;
    /// The block the tests render in, and the one the engine defaults to.
    const BLOCK: usize = 64;

    // ── Rigs ──

    fn tape() -> Tape {
        Tape::new(FS)
    }

    /// A tape with the transport locked still and the noise off — what every
    /// measurement of the *medium* or the *head* wants, because a wobbling
    /// read head smears a tone across two bins.
    fn still(fs: f64) -> Tape {
        let mut tape = Tape::new(fs);
        tape.set_param_natural(PARAM_WOW, 0.0);
        tape.set_param_natural(PARAM_FLUTTER, 0.0);
        tape.set_param_natural(PARAM_HISS, 0.0);
        tape.snap();
        tape
    }

    /// The same, with the head out of the way as well: no bump, the top open,
    /// azimuth true. What a measurement of the medium alone wants.
    fn bare(fs: f64) -> Tape {
        let mut tape = still(fs);
        tape.set_param_natural(PARAM_BUMP_DB, 0.0);
        tape.set_param_natural(PARAM_SPEED, 2.0);
        tape.snap();
        tape
    }

    fn set(tape: &mut Tape, index: usize, value: f32) {
        tape.set_param_natural(index, value);
    }

    /// Push a mono signal through, in blocks, and answer both channels.
    fn render(tape: &mut Tape, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
        render_stereo(tape, input, input)
    }

    fn render_stereo(tape: &mut Tape, left: &[f32], right: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let frames = left.len().min(right.len());
        let mut out_l = Vec::with_capacity(frames);
        let mut out_r = Vec::with_capacity(frames);
        let mut at = 0;
        while at < frames {
            let end = (at + BLOCK).min(frames);
            let mut block_l = left[at..end].to_vec();
            let mut block_r = right[at..end].to_vec();
            tape.process(&mut block_l, &mut block_r);
            out_l.extend_from_slice(&block_l);
            out_r.extend_from_slice(&block_r);
            at = end;
        }
        (out_l, out_r)
    }

    fn sine(hz: f64, amplitude: f64, frames: usize, fs: f64) -> Vec<f32> {
        (0..frames).map(|n| (amplitude * (TAU * hz * n as f64 / fs).sin()) as f32).collect()
    }

    fn db_to_amp(db: f64) -> f64 {
        10f64.powf(db / 20.0)
    }

    fn peak(x: &[f32]) -> f64 {
        x.iter().map(|v| f64::from(v.abs())).fold(0.0, f64::max)
    }

    fn rms(x: &[f32]) -> f64 {
        if x.is_empty() {
            return 0.0;
        }
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64).sqrt()
    }

    /// The largest step between neighbouring samples — what a click is.
    fn biggest_step(x: &[f32]) -> f64 {
        x.windows(2).map(|p| f64::from((p[1] - p[0]).abs())).fold(0.0, f64::max)
    }

    /// The amplitude of one frequency in a window, by a single Hann-windowed
    /// DFT bin. No FFT crate: one bin is all any of these measurements ask
    /// for.
    fn tone_amplitude(x: &[f32], hz: f64, fs: f64) -> f64 {
        let n = x.len();
        if n == 0 {
            return 0.0;
        }
        let (mut re, mut im, mut norm) = (0.0f64, 0.0f64, 0.0f64);
        for (index, sample) in x.iter().enumerate() {
            let w = 0.5 - 0.5 * (TAU * index as f64 / n as f64).cos();
            let phase = TAU * hz * index as f64 / fs;
            let v = f64::from(*sample) * w;
            re += v * phase.cos();
            im -= v * phase.sin();
            norm += w;
        }
        2.0 * (re * re + im * im).sqrt() / norm.max(1.0)
    }

    /// Render a steady tone and answer the second half of it, which is the
    /// part after every filter and the medium itself have settled.
    fn steady(tape: &mut Tape, hz: f64, db: f64, fs: f64) -> Vec<f32> {
        let frames = 1 << 15;
        let input = sine(hz, db_to_amp(db), frames + frames / 4, fs);
        let (out, _) = render(tape, &input);
        out[frames / 4..].to_vec()
    }

    /// Total harmonic distortion of a rendered tone, as a percentage of the
    /// fundamental, over every harmonic this rate can carry up to the
    /// seventh.
    fn thd_percent(out: &[f32], hz: f64, fs: f64) -> f64 {
        let fundamental = tone_amplitude(out, hz, fs);
        let mut sum = 0.0;
        for k in 2..=7 {
            let f = hz * f64::from(k);
            if f < fs * 0.45 {
                let m = tone_amplitude(out, f, fs);
                sum += m * m;
            }
        }
        sum.sqrt() / fundamental * 100.0
    }

    /// One harmonic as a percentage of the fundamental.
    fn harmonic_percent(out: &[f32], hz: f64, order: u32, fs: f64) -> f64 {
        tone_amplitude(out, hz * f64::from(order), fs) / tone_amplitude(out, hz, fs) * 100.0
    }

    /// Band-limited pink, peak-normalised: a stand-in for programme material
    /// that is not a sine and not a step.
    fn programme(frames: usize, peak_db: f64, fs: f64) -> Vec<f32> {
        let mut state = 0x1234_5678u32;
        let (mut b0, mut b1, mut b2) = (0.0f64, 0.0, 0.0);
        let mut out: Vec<f64> = (0..frames)
            .map(|_| {
                state = mix32(state.wrapping_add(0x9E37_79B9));
                let white = f64::from(state >> 8) / f64::from(1u32 << 23) - 1.0;
                b0 = 0.997_65 * b0 + white * 0.099_046_0;
                b1 = 0.963_00 * b1 + white * 0.296_516_4;
                b2 = 0.570_00 * b2 + white * 1.052_691_3;
                b0 + b1 + b2 + white * 0.184_8
            })
            .collect();
        // Two one-pole high-passes at 30 Hz, because real programme material
        // does not have infinite energy at DC and a peak normalisation
        // against a sub-audio wander measures the wander.
        for _ in 0..2 {
            let a = (-TAU * 30.0 / fs).exp();
            let (mut x1, mut y1) = (0.0f64, 0.0f64);
            for v in out.iter_mut() {
                let y = *v - x1 + a * y1;
                x1 = *v;
                y1 = y;
                *v = y;
            }
        }
        let top = out.iter().fold(0.0f64, |a, v| a.max(v.abs())).max(1.0e-12);
        let scale = db_to_amp(peak_db) / top;
        out.iter().map(|v| (v * scale) as f32).collect()
    }

    /// The gain of the whole device at one frequency, in decibels, measured
    /// small enough that the medium is in its linear region.
    ///
    /// Long windows at low frequencies: a Hann-windowed bin needs several
    /// cycles of the tone it is looking for, and 20 Hz at 48 kHz is 2 400
    /// samples a cycle.
    fn response_db(tape: &mut Tape, hz: f64, fs: f64) -> f64 {
        let frames = if hz < 200.0 { 1 << 16 } else { 1 << 14 };
        let db = -60.0;
        let input = sine(hz, db_to_amp(db), frames + frames / 4, fs);
        let (out, _) = render(tape, &input);
        20.0 * (tone_amplitude(&out[frames / 4..], hz, fs) / db_to_amp(db)).log10()
    }

    /// Where the response falls 3 dB below its 1 kHz value, by bisection.
    fn minus3_hz(speed: Speed, fs: f64) -> f64 {
        let probe = |hz: f64| {
            let mut tape = still(fs);
            set(&mut tape, PARAM_SPEED, speed.index() as f32);
            tape.snap();
            response_db(&mut tape, hz, fs)
        };
        let reference = probe(1000.0);
        let (mut low, mut high) = (2_000.0f64, fs * 0.499);
        for _ in 0..12 {
            let mid = (low * high).sqrt();
            if probe(mid) - reference > -3.0 {
                low = mid;
            } else {
                high = mid;
            }
        }
        (low * high).sqrt()
    }

    /// The instantaneous frequency deviation of a rendered tone, as a
    /// fraction of the carrier, and how much of it sits at one rate.
    ///
    /// Both borrowed from the delay's tests rather than written twice: the
    /// quadrature demodulator is the same instrument either way, and a second
    /// copy is a second thing to get wrong.
    fn deviation_at(out: &[f32], carrier: f64, rate: f64, fs: f64) -> f64 {
        let track = crate::fx::delay::tests::fm_deviation(out, carrier, fs);
        crate::fx::delay::tests::track_component(&track, rate, fs) * 100.0
    }

    // ── The null tests ──

    /// **A tape nobody has turned up is inaudible, sample for sample.**
    ///
    /// Every other control is at an extreme, because the promise is about the
    /// blend knob and not about the settings behind it.
    #[test]
    fn wet_at_zero_is_bit_identical_to_the_input() {
        let mut tape = tape();
        set(&mut tape, PARAM_MIX, 0.0);
        set(&mut tape, PARAM_DRIVE, 100.0);
        set(&mut tape, PARAM_BIAS, 0.0);
        set(&mut tape, PARAM_WOW, 100.0);
        set(&mut tape, PARAM_FLUTTER, 100.0);
        set(&mut tape, PARAM_HISS, 100.0);
        set(&mut tape, PARAM_AZIMUTH_DEG, 1.0);
        tape.snap();
        let source: Vec<f32> = (0..4096)
            .map(|i| (i as f32 * 0.021).sin() * 0.6 + (i as f32 * 0.37).cos() * 0.2)
            .chain([0.0, -0.0, f32::MIN_POSITIVE, -f32::MIN_POSITIVE])
            .collect();
        let (left, right) = render(&mut tape, &source);
        for (index, (before, after)) in source.iter().zip(&left).enumerate() {
            assert_eq!(before.to_bits(), after.to_bits(), "sample {index}: {before} -> {after}");
        }
        assert_eq!(right, source);
    }

    /// **Silence in, silence out** — exactly, with every control at its
    /// extreme and the noise off, which is where the noise ships.
    #[test]
    fn silence_in_is_silence_out() {
        let mut tape = tape();
        for index in 0..PARAM_COUNT {
            let info = natural_param(index).unwrap();
            set(&mut tape, index, info.max);
        }
        set(&mut tape, PARAM_HISS, 0.0);
        tape.snap();
        let (left, right) = render(&mut tape, &vec![0.0f32; (4.0 * FS) as usize]);
        assert_eq!(peak(&left), 0.0, "the left channel was not silent");
        assert_eq!(peak(&right), 0.0, "the right channel was not silent");
    }

    /// **A loud burst leaves nothing behind.**
    ///
    /// Hysteresis is a memory, so the medium keeps a remanent magnetisation
    /// after the field goes away — this is the test that says the DC blocker
    /// takes it off and that the tail reaches *exactly* zero rather than
    /// approaching it through the denormal range, which is where a processor
    /// goes slow and stays slow.
    #[test]
    fn a_burst_leaves_no_tail_and_no_denormals() {
        let mut tape = tape();
        set(&mut tape, PARAM_DRIVE, 100.0);
        tape.snap();
        let burst = sine(220.0, 0.9, (0.5 * FS) as usize, FS);
        let _ = render(&mut tape, &burst);

        let silence = vec![0.0f32; (4.0 * FS) as usize];
        let (settling, _) = render(&mut tape, &silence);
        assert!(peak(&settling) > 0.0, "the remanence was never there to begin with");

        // **Four seconds, and the shape of what happens in them is worth
        // knowing.** The DC blocker takes the remanence off within a tenth of
        // a second; what is left is the magnetisation drifting on the last
        // denormal-scale crumbs coming out of the record chain, and it sits
        // at about 1e-28 — 560 dB down, a normal number rather than a
        // denormal, and therefore free — until those crumbs flush to exactly
        // zero and the state freezes. Then the output is exactly zero and
        // stays there.
        let (tail, tail_r) = render(&mut tape, &vec![0.0f32; (4.0 * FS) as usize]);
        assert_eq!(peak(&tail), 0.0, "the tail never reached zero");
        assert_eq!(peak(&tail_r), 0.0);

        // ...and the arithmetic did not slow down on the way there, which is
        // what a denormal tail costs.
        let mut left = vec![0.0f32; 512];
        let mut right = vec![0.0f32; 512];
        let started = std::time::Instant::now();
        for _ in 0..200 {
            tape.process(&mut left, &mut right);
        }
        let quiet = started.elapsed();
        let mut left = sine(220.0, 0.3, 512, FS);
        let mut right = left.clone();
        let started = std::time::Instant::now();
        for _ in 0..200 {
            tape.process(&mut left, &mut right);
        }
        let loud = started.elapsed();
        assert!(
            quiet.as_secs_f64() < loud.as_secs_f64() * 3.0,
            "silence cost {quiet:?} against {loud:?} for signal, which is a denormal tail"
        );
    }

    // ── The medium ──

    /// **There are no even harmonics, at any drive or level.**
    ///
    /// The loop is odd-symmetric; its hysteresis shows up as level-dependence
    /// and phase, not as second-order warmth. A brief that promises
    /// "even-order tape glue" from this model is describing something else.
    #[test]
    fn the_harmonic_signature_is_odd_only() {
        for db in [-24.0f64, -12.0, -6.0] {
            for drive in [10.0f32, 50.0, 100.0] {
                let mut tape = still(FS);
                set(&mut tape, PARAM_DRIVE, drive);
                tape.snap();
                let out = steady(&mut tape, 1000.0, db, FS);
                let h2 = harmonic_percent(&out, 1000.0, 2, FS);
                let h4 = harmonic_percent(&out, 1000.0, 4, FS);
                let h3 = harmonic_percent(&out, 1000.0, 3, FS);
                assert!(h2 < 0.01, "{db} dBFS drive {drive}: H2 was {h2:.4}%");
                assert!(h4 < 0.01, "{db} dBFS drive {drive}: H4 was {h4:.4}%");
                assert!(h3 > h2 * 10.0, "{db} dBFS drive {drive}: H3 {h3:.4}% is not dominant");
            }
        }
    }

    /// **The factory drive puts maximum output level at −6 dBFS**, which is
    /// where this box's instruments peak.
    ///
    /// Two numbers, and they are the calibration: 3% third harmonic at
    /// −6 dBFS is the industry's definition of MOL for magnetic tape, and
    /// 1–2% THD at −12 dBFS is what the effects brief asks for. Measured
    /// 3.19% and 1.03%.
    #[test]
    fn the_factory_drive_is_lined_up_for_this_boxs_instruments() {
        let mut tape = still(FS);
        let out = steady(&mut tape, 1000.0, -12.0, FS);
        let thd = thd_percent(&out, 1000.0, FS);
        assert!((0.95..=2.0).contains(&thd), "THD at -12 dBFS was {thd:.3}%");

        let mut tape = still(FS);
        let out = steady(&mut tape, 1000.0, -6.0, FS);
        let h3 = harmonic_percent(&out, 1000.0, 3, FS);
        assert!((2.5..=3.5).contains(&h3), "the third harmonic at -6 dBFS was {h3:.3}%");
    }

    /// **The distortion follows the level all the way down**, which is the
    /// whole difference between this and a waveshaper.
    ///
    /// A `tanh` trimmed to the same distortion at −12 dBFS makes 0.005% at
    /// −36 dBFS; this makes 0.072%, fourteen times more, and that gap is the
    /// "tape glues a quiet mix" phenomenon.
    #[test]
    fn distortion_is_level_dependent_the_way_tape_is() {
        let mut last = 0.0;
        for db in [-36.0f64, -24.0, -12.0, -6.0, 0.0] {
            let mut tape = still(FS);
            let out = steady(&mut tape, 1000.0, db, FS);
            let thd = thd_percent(&out, 1000.0, FS);
            assert!(thd > last * 2.0, "{db} dBFS: THD {thd:.4}% did not rise from {last:.4}%");
            last = thd;
        }
        let mut tape = still(FS);
        let out = steady(&mut tape, 1000.0, -36.0, FS);
        let quiet = thd_percent(&out, 1000.0, FS);
        assert!(quiet > 0.05, "at -36 dBFS the tape went clean: {quiet:.4}%");
    }

    /// **And it rises with frequency**, because the loop's area grows with
    /// `dH/dt` and a memoryless shaper has no clock. Real tape has *less*
    /// high-frequency headroom, not more.
    #[test]
    fn distortion_rises_with_frequency() {
        let mut low = still(FS);
        let low = thd_percent(&steady(&mut low, 50.0, -12.0, FS), 50.0, FS);
        let mut high = still(FS);
        let high = thd_percent(&steady(&mut high, 4000.0, -12.0, FS), 4000.0, FS);
        assert!(high > low * 1.5, "50 Hz {low:.3}% against 4 kHz {high:.3}%");
    }

    /// **Every knob position has the same small-signal gain**, so the drive
    /// control changes the distortion and never the level.
    ///
    /// A 5×5×5 grid, minus the twenty-five positions where the bias is at the
    /// bottom of its travel and the medium's gain has gone to nothing — there
    /// the makeup is capped, deliberately, because the alternative is a
    /// control that turns into a fault.
    #[test]
    fn the_makeup_holds_the_level_at_every_knob_position() {
        let mut capped = 0;
        for d in 0..5 {
            for s in 0..5 {
                for b in 0..5 {
                    let (drive, sat, bias) = (d as f32 * 25.0, s as f32 * 25.0, b as f32 * 25.0);
                    if -20.0 * g0_for(drive, sat, bias).log10() > MAKEUP_CEILING_DB {
                        capped += 1;
                        continue;
                    }
                    let mut tape = bare(FS);
                    set(&mut tape, PARAM_DRIVE, drive);
                    set(&mut tape, PARAM_SAT, sat);
                    set(&mut tape, PARAM_BIAS, bias);
                    tape.snap();
                    let gain = response_db(&mut tape, 1000.0, FS);
                    assert!(
                        (gain - LINEUP_DB).abs() < 0.15,
                        "drive {drive} sat {sat} bias {bias}: {gain:+.4} dB, not {LINEUP_DB}"
                    );
                }
            }
        }
        assert_eq!(capped, 25, "the capped corner of the grid moved");
    }

    /// **The closed form is the measured slope**, to a part in a thousand.
    ///
    /// This is the test that lets the makeup be arithmetic instead of a
    /// curve fit: if `G₀` stops predicting what the medium does, the makeup
    /// is wrong at every knob position at once and nothing else would say so.
    #[test]
    fn the_closed_form_is_the_measured_small_signal_gain() {
        for (drive, bias) in [(50.0f32, 50.0f32), (33.5, 99.0), (100.0, 100.0), (0.0, 25.0)] {
            let mut tape = bare(FS);
            set(&mut tape, PARAM_DRIVE, drive);
            set(&mut tape, PARAM_BIAS, bias);
            set(&mut tape, PARAM_AUTO_MAKEUP, 0.0);
            tape.snap();
            let db = -60.0;
            let frames = 1 << 14;
            let input = sine(1000.0, db_to_amp(db), frames + frames / 4, FS);
            let (out, _) = render(&mut tape, &input);
            let measured = tone_amplitude(&out[frames / 4..], 1000.0, FS) / db_to_amp(db);
            let predicted = g0_for(drive, 50.0, bias);
            assert!(
                (measured - predicted).abs() / predicted < 0.005,
                "drive {drive} bias {bias}: closed form {predicted:.4}, measured {measured:.4}"
            );
        }
    }

    /// **Underbiased tape has a deadzone**, and this is the property no
    /// waveshaper can be made to have at any drive.
    ///
    /// Measured as the ratio of the transfer slope near zero to the slope at
    /// −20 dBFS: 1.005 with the bias up, 0.735 with it nearly off, and 0.010
    /// with it off altogether. The distortion of a −60 dBFS tone goes from a
    /// ten-thousandth of a percent to sixteen percent over the same travel.
    #[test]
    fn low_bias_makes_a_deadzone() {
        let slope_and_thd = |bias: f32| {
            let mut quiet = bare(FS);
            set(&mut quiet, PARAM_BIAS, bias);
            quiet.snap();
            let small = steady(&mut quiet, 1000.0, -60.0, FS);
            let mut loud = bare(FS);
            set(&mut loud, PARAM_BIAS, bias);
            loud.snap();
            let large = steady(&mut loud, 1000.0, -20.0, FS);
            (
                (tone_amplitude(&small, 1000.0, FS) / db_to_amp(-60.0))
                    / (tone_amplitude(&large, 1000.0, FS) / db_to_amp(-20.0)),
                thd_percent(&small, 1000.0, FS),
            )
        };
        let (open_slope, open_thd) = slope_and_thd(100.0);
        let (narrow_slope, narrow_thd) = slope_and_thd(2.0);
        assert!(open_slope > 0.99, "a well biased medium was not linear: {open_slope:.4}");
        assert!(narrow_slope < 0.8, "an underbiased medium had no deadzone: {narrow_slope:.4}");
        assert!(
            narrow_thd > open_thd * 100.0,
            "the deadzone did not distort: {open_thd:.5}% -> {narrow_thd:.5}%"
        );
    }

    // ── The transport ──

    /// **Wow is the depth and the rate the panel says**, at three positions
    /// of the knob and to within a percent.
    ///
    /// Measured the way the standards measure it: the instantaneous frequency
    /// of a rendered tone, tracked by quadrature demodulation, and the
    /// component of that track at the rate the transport is supposed to
    /// wobble at.
    #[test]
    fn wow_is_the_depth_and_the_rate_the_panel_says() {
        for knob in [25.0f32, 50.0, 100.0] {
            let mut tape = tape();
            set(&mut tape, PARAM_FLUTTER, 0.0);
            set(&mut tape, PARAM_WOW, knob);
            tape.snap();
            let input = sine(5000.0, db_to_amp(-12.0), (12.0 * FS) as usize, FS);
            let (out, _) = render(&mut tape, &input);
            let out = &out[FS as usize..];
            let asked = wow_percent(knob);
            let got = deviation_at(out, 5000.0, WOW_HZ, FS);
            assert!(
                (got - asked).abs() < asked * 0.05,
                "wow {knob}%: asked {asked:.4}%, measured {got:.4}%"
            );
            // ...and it is *at* 0.6 Hz rather than merely near it.
            for elsewhere in [0.3, 1.2, 2.4] {
                let stray = deviation_at(out, 5000.0, elsewhere, FS);
                assert!(stray < got * 0.1, "wow {knob}%: {stray:.4}% at {elsewhere} Hz");
            }
        }
    }

    /// **Flutter likewise, and scrape rides on it.**
    ///
    /// The scrape component reads 90% of what it asks for because the
    /// demodulator's own 120 Hz smoothing is 0.9 at 58 Hz — the number is
    /// corrected here rather than the tolerance being widened to hide it.
    #[test]
    fn flutter_is_the_depth_and_the_rate_the_panel_says() {
        for knob in [50.0f32, 100.0] {
            let mut tape = tape();
            set(&mut tape, PARAM_WOW, 0.0);
            set(&mut tape, PARAM_FLUTTER, knob);
            tape.snap();
            let input = sine(5000.0, db_to_amp(-12.0), (12.0 * FS) as usize, FS);
            let (out, _) = render(&mut tape, &input);
            let out = &out[FS as usize..];
            let asked = flutter_percent(knob);
            let got = deviation_at(out, 5000.0, FLUTTER_HZ, FS);
            assert!(
                (got - asked).abs() < asked * 0.05,
                "flutter {knob}%: asked {asked:.4}%, measured {got:.4}%"
            );
            // Scrape: a fifth of the flutter depth, at 58 Hz, and the
            // demodulator's smoothing takes a tenth off it there.
            let asked_scrape = asked * SCRAPE_FRACTION * 0.9;
            let scrape = deviation_at(out, 5000.0, SCRAPE_HZ, FS);
            assert!(
                (scrape - asked_scrape).abs() < asked_scrape * 0.15,
                "flutter {knob}%: scrape asked {asked_scrape:.5}%, measured {scrape:.5}%"
            );
        }
    }

    /// **A still transport does not modulate at all**, and the line it would
    /// have read through is out of the path entirely — which is what returns
    /// the effect to genuinely zero latency when nobody is using the wobble.
    #[test]
    fn a_still_transport_is_an_exact_null() {
        let mut tape = tape();
        set(&mut tape, PARAM_WOW, 0.0);
        set(&mut tape, PARAM_FLUTTER, 0.0);
        tape.snap();
        let input = sine(5000.0, db_to_amp(-12.0), (4.0 * FS) as usize, FS);
        let (out, _) = render(&mut tape, &input);
        let out = &out[FS as usize..];
        for rate in [WOW_HZ, FLUTTER_HZ, SCRAPE_HZ] {
            let stray = deviation_at(out, 5000.0, rate, FS);
            assert!(stray < 1.0e-4, "a still transport wobbled {stray:.6}% at {rate} Hz");
        }
    }

    /// **Turning the transport on fades the line in rather than stepping the
    /// read head**, which would be a sample-and-a-bit discontinuity in the
    /// middle of whatever is playing.
    #[test]
    fn engaging_the_transport_does_not_click() {
        let mut tape = tape();
        set(&mut tape, PARAM_WOW, 0.0);
        set(&mut tape, PARAM_FLUTTER, 0.0);
        tape.snap();
        let input = sine(1000.0, db_to_amp(-6.0), (0.5 * FS) as usize, FS);
        let (settled, _) = render(&mut tape, &input);
        let reference = biggest_step(&settled[BLOCK * 4..]);

        set(&mut tape, PARAM_WOW, 100.0);
        let (moving, _) = render(&mut tape, &input);
        let step = biggest_step(&moving);
        assert!(
            step < reference * 1.5,
            "engaging the transport stepped by {step:.5} against {reference:.5} for the tone itself"
        );
    }

    // ── The head ──

    /// **The head bump moves with the tape speed**, which is where 15 ips
    /// gets its reputation for a thick low end and 30 ips for sounding lean.
    ///
    /// The peak is measured against the 1 kHz response. It reads a little
    /// under the +1.5 dB the knob asks for at the slow speed and that is the
    /// DC blocker, not the bump: a 12 Hz first-order high-pass is 0.4 dB down
    /// at 35 Hz, and the honest number is what the machine does rather than
    /// what one filter in it would do alone.
    #[test]
    fn the_head_bump_moves_with_the_speed() {
        for (speed, wanted) in [(Speed::Slow, 35.0f64), (Speed::Studio, 70.0), (Speed::Fast, 140.0)]
        {
            assert!((bump_hz(speed) - wanted).abs() < 0.01, "{}", speed.label());
            let mut tape = still(FS);
            set(&mut tape, PARAM_SPEED, speed.index() as f32);
            tape.snap();
            let reference = response_db(&mut tape, 1000.0, FS);
            let mut best = (f64::MIN, 0.0f64);
            let mut hz = 20.0f64;
            while hz < 400.0 {
                let mut tape = still(FS);
                set(&mut tape, PARAM_SPEED, speed.index() as f32);
                tape.snap();
                let db = response_db(&mut tape, hz, FS);
                if db > best.0 {
                    best = (db, hz);
                }
                hz *= 1.06;
            }
            let (gain, centre) = (best.0 - reference, best.1);
            assert!(
                (centre - wanted).abs() < wanted * 0.08,
                "{}: the bump peaked at {centre:.0} Hz, not {wanted:.0}",
                speed.label()
            );
            assert!(
                (1.0..=1.55).contains(&gain),
                "{}: the bump was {gain:+.2} dB at {centre:.0} Hz",
                speed.label()
            );
        }
        // Turned all the way down it is a wire, and it can be turned back up.
        let mut tape = still(FS);
        set(&mut tape, PARAM_BUMP_DB, 0.0);
        tape.snap();
        let flat = response_db(&mut tape, 70.0, FS) - response_db(&mut tape, 1000.0, FS);
        assert!(flat.abs() < 0.2, "the bump at 0 dB was {flat:+.3} dB");
    }

    /// **The high-frequency loss moves with the tape speed too**, because
    /// every loss mechanism is a function of wavelength alone: −3 dB at
    /// 7.5 kHz, 15 kHz and 30 kHz for the three speeds, with the last one
    /// stopped by the sample rate rather than by the tape.
    #[test]
    fn the_hf_loss_moves_with_the_speed() {
        for speed in Speed::ALL {
            let wanted = loss_hz(speed).min(FS * 0.45);
            let measured = minus3_hz(speed, FS);
            assert!(
                (measured - wanted).abs() < wanted * 0.05,
                "{}: -3 dB at {measured:.0} Hz, not {wanted:.0}",
                speed.label()
            );
        }
        // 15 kHz at the studio speed is the number the brief asks for.
        assert!((loss_hz(Speed::Studio) - 15_000.0).abs() < 1.0);
    }

    /// **Azimuth is a high-frequency difference between the channels and
    /// nothing else** — no delay, so no comb filter when the mix is folded
    /// down.
    ///
    /// The reference implementation renders it as an inter-channel delay,
    /// which is a Haas widener whose entire character is invisible until
    /// somebody sums to mono. This is the mono-safe half of the same
    /// misalignment.
    #[test]
    fn azimuth_is_a_stereo_top_end_difference_and_survives_mono() {
        let difference_at = |degrees: f32, hz: f64| {
            let mut tape = still(FS);
            set(&mut tape, PARAM_AZIMUTH_DEG, degrees);
            tape.snap();
            let frames = 1 << 14;
            let input = sine(hz, db_to_amp(-40.0), frames + frames / 4, FS);
            let (left, right) = render(&mut tape, &input);
            let (left, right) = (&left[frames / 4..], &right[frames / 4..]);
            20.0 * (tone_amplitude(left, hz, FS) / tone_amplitude(right, hz, FS)).log10()
        };
        // True: the two channels are the same signal, bit for bit.
        let mut tape = still(FS);
        let input = sine(9_000.0, db_to_amp(-20.0), 4096, FS);
        let (left, right) = render(&mut tape, &input);
        assert_eq!(left, right, "the channels differ with the azimuth true");

        assert!(difference_at(0.25, 10_000.0) > 0.4, "a quarter degree did nothing at 10 kHz");
        let tilted = difference_at(1.0, 10_000.0);
        assert!(tilted > 5.0, "a whole degree lost only {tilted:.2} dB of top");
        let midband = difference_at(1.0, 1_000.0);
        assert!(midband < 0.5, "azimuth took {midband:.2} dB out of the midrange");

        // And the mono sum keeps its level rather than cancelling, which is
        // the entire point of not shipping it as a delay.
        let mut tape = still(FS);
        set(&mut tape, PARAM_AZIMUTH_DEG, 1.0);
        tape.snap();
        let frames = 1 << 14;
        let input = sine(10_000.0, db_to_amp(-20.0), frames + frames / 4, FS);
        let (left, right) = render(&mut tape, &input);
        let sum: Vec<f32> = left[frames / 4..]
            .iter()
            .zip(&right[frames / 4..])
            .map(|(l, r)| (l + r) * 0.5)
            .collect();
        let mono = tone_amplitude(&sum, 10_000.0, FS);
        let single = tone_amplitude(&left[frames / 4..], 10_000.0, FS);
        assert!(
            mono > single * 0.5,
            "the mono sum lost the tone: {mono:.5} against {single:.5} on one channel"
        );
    }

    /// **The record EQ and the reproduce EQ are exactly inverse**, so the
    /// pair moves where the distortion goes and leaves the response alone.
    ///
    /// Measured at the drive floor and a −60 dBFS tone, where the medium is
    /// linear: flat to 0.02 dB from 200 Hz to 16 kHz. Below 200 Hz the DC
    /// blocker takes over and that is not the emphasis pair's doing.
    #[test]
    fn the_record_eq_pair_is_exactly_inverse() {
        let mut worst = 0.0f64;
        for hz in [200.0f64, 1000.0, 3000.0, 8000.0, 12000.0, 16000.0] {
            let mut tape = bare(FS);
            set(&mut tape, PARAM_DRIVE, 0.0);
            set(&mut tape, PARAM_BIAS, 100.0);
            tape.snap();
            let gain = response_db(&mut tape, hz, FS);
            worst = worst.max((gain - LINEUP_DB).abs());
        }
        assert!(worst < 0.05, "the emphasis pair left {worst:.4} dB of tilt behind it");
    }

    // ── Oversampling ──

    /// **Two times is enough**, measured the way the brief measures it: two
    /// tones at 5.5 and 7.1 kHz at −6 dBFS each, and the worst product that
    /// is not a harmonic or an intermodulation of them.
    ///
    /// −66.9 dBc at the factory drive and −63.8 dBc with the drive at
    /// maximum, against −45.6 dBc at one times.
    #[test]
    fn two_times_oversampling_keeps_the_folded_products_down() {
        for (drive, floor) in [(50.0f32, -65.0f64), (100.0, -60.0)] {
            let mut tape = bare(FS);
            set(&mut tape, PARAM_DRIVE, drive);
            tape.snap();
            let frames = 1 << 15;
            let amplitude = db_to_amp(-6.0);
            let input: Vec<f32> = (0..frames + frames / 4)
                .map(|i| {
                    let t = i as f64 / FS;
                    (amplitude * ((TAU * 5500.0 * t).sin() + (TAU * 7100.0 * t).sin()) * 0.5) as f32
                })
                .collect();
            let (out, _) = render(&mut tape, &input);
            let out = &out[frames / 4..];

            // Everything two tones are allowed to make on their own: the
            // harmonics and the intermodulation products, up to eighth order.
            let mut legitimate = Vec::new();
            for a in 0..8i32 {
                for b in 0..8i32 {
                    for sign in [-1i32, 1] {
                        let f = (5500.0 * f64::from(a) + f64::from(sign) * 7100.0 * f64::from(b))
                            .abs();
                        if f > 40.0 && f < FS * 0.5 {
                            legitimate.push(f);
                        }
                    }
                }
            }
            let fundamental = tone_amplitude(out, 5500.0, FS);
            let (mut worst, mut worst_hz) = (0.0f64, 0.0f64);
            let mut hz = 100.0f64;
            while hz < 20_000.0 {
                if legitimate.iter().all(|f| (f - hz).abs() > 90.0) {
                    let m = tone_amplitude(out, hz, FS);
                    if m > worst {
                        worst = m;
                        worst_hz = hz;
                    }
                }
                hz += 50.0;
            }
            let dbc = 20.0 * (worst / fundamental).log10();
            assert!(
                dbc < floor,
                "drive {drive}: the worst folded product was {dbc:.1} dBc at {worst_hz:.0} Hz"
            );
        }
    }

    // ── The whole device ──

    /// **A tape you can A/B against bypass without being flattered by the
    /// level.**
    ///
    /// Band-limited pink at −12 dBFS peak, which is where this box's
    /// instruments are gain-staged, through the whole device at five drive
    /// settings: every one of them sits inside a quarter of a decibel of the
    /// input. This is the test the lineup constant exists for — the medium's
    /// small-signal gain is not the gain a programme sees, and the difference
    /// is a decibel.
    #[test]
    fn an_a_b_against_bypass_is_level_matched() {
        let input = programme((4.0 * FS) as usize, -12.0, FS);
        let from = FS as usize / 4;
        for drive in [0.0f32, 25.0, 50.0, 75.0, 100.0] {
            let mut tape = tape();
            set(&mut tape, PARAM_DRIVE, drive);
            tape.snap();
            let (out, _) = render(&mut tape, &input);
            let db = 20.0 * (rms(&out[from..]) / rms(&input[from..])).log10();
            assert!(
                db.abs() < 0.25,
                "drive {drive}: the tape was {db:+.3} dB against bypass"
            );
        }
    }

    /// **The manual trim takes over when the automatic is switched off**, and
    /// the panel greys the one that is not in charge.
    #[test]
    fn the_trim_takes_over_when_the_automatic_is_off() {
        let params = default_natural_params();
        assert!(auto_makeup_on(&params), "the automatic makeup ships off");
        assert!(!uses(&params, PARAM_TRIM_DB), "the trim is live under the automatic");

        let mut manual = params;
        manual[PARAM_AUTO_MAKEUP] = 0.0;
        assert!(uses(&manual, PARAM_TRIM_DB), "the trim stayed greyed with the automatic off");

        let mut tape = bare(FS);
        set(&mut tape, PARAM_AUTO_MAKEUP, 0.0);
        set(&mut tape, PARAM_TRIM_DB, 6.0);
        tape.snap();
        assert!((tape.makeup_db() - 6.0).abs() < 1.0e-6);
        let with_trim = response_db(&mut tape, 1000.0, FS);

        let mut raw = bare(FS);
        set(&mut raw, PARAM_AUTO_MAKEUP, 0.0);
        set(&mut raw, PARAM_TRIM_DB, 0.0);
        raw.snap();
        let without = response_db(&mut raw, 1000.0, FS);
        assert!(
            (with_trim - without - 6.0).abs() < 0.02,
            "six decibels of trim moved the level by {:.3}",
            with_trim - without
        );

        // ...and with the automatic on, the published number is the one the
        // output is actually multiplied by, so the panel can seed the manual
        // knob from it without the level jumping.
        let mut automatic = bare(FS);
        automatic.snap();
        assert!((automatic.makeup_db() - auto_makeup_db(&automatic.params)).abs() < 1.0e-9);
    }

    /// **Hiss is off at the factory, and pink when it is turned up.**
    ///
    /// Off is the default so that "silence in, silence out" is the shipped
    /// configuration rather than a special case, and the level is calibrated
    /// for effect rather than for fidelity: −48 dBFS at the top of the travel
    /// is worse than any real machine, and the panel says dBFS rather than
    /// pretending to be a signal-to-noise figure.
    #[test]
    fn hiss_is_off_by_default_and_pink_when_it_is_on() {
        assert_eq!(default_natural_params()[PARAM_HISS], 0.0);
        assert_eq!(hiss_dbfs(0.0), None);
        assert!((hiss_dbfs(100.0).unwrap() - HISS_MAX_DBFS).abs() < 1.0e-9);

        let mut tape = still(FS);
        set(&mut tape, PARAM_HISS, 100.0);
        tape.snap();
        let (left, right) = render(&mut tape, &vec![0.0f32; (4.0 * FS) as usize]);
        let level = 20.0 * rms(&left).log10();
        assert!(
            (level - HISS_MAX_DBFS).abs() < 1.5,
            "the hiss measured {level:.1} dBFS, not {HISS_MAX_DBFS}"
        );

        // Pink: an octave up is about 3 dB down.
        let low = tone_amplitude(&left, 250.0, FS);
        let high = tone_amplitude(&left, 4000.0, FS);
        assert!(high < low, "the hiss was not pink: {low:.2e} at 250 Hz, {high:.2e} at 4 kHz");

        // The two channels are independent, so the noise is not a mono
        // signal pinned in the middle of the image.
        let correlation: f64 = left
            .iter()
            .zip(&right)
            .map(|(l, r)| f64::from(*l) * f64::from(*r))
            .sum::<f64>()
            / (rms(&left) * rms(&right) * left.len() as f64);
        assert!(correlation.abs() < 0.1, "the two channels' hiss correlates at {correlation:.3}");
    }

    /// **The solver never runs away**, at three sample rates, with every
    /// control at its worst and a full-scale noise floor going in.
    #[test]
    fn the_solver_never_diverges() {
        for fs in [44_100.0f64, 48_000.0, 96_000.0] {
            let mut tape = Tape::new(fs);
            for index in 0..PARAM_COUNT {
                set(&mut tape, index, natural_param(index).unwrap().max);
            }
            set(&mut tape, PARAM_BIAS, 0.0);
            tape.snap();
            // The bound is the makeup and not a fixed number: with the bias
            // at the bottom of its travel the medium's small-signal gain has
            // gone to nothing and the makeup is at its +18 dB ceiling, so a
            // full-scale input is *supposed* to come back big. What would be
            // a bug is it coming back infinite.
            let ceiling = 3.0 * 10f64.powf(tape.makeup_db() / 20.0);
            let mut state = 0x1234_5678u32;
            let noise: Vec<f32> = (0..(4.0 * fs) as usize)
                .map(|_| {
                    state = mix32(state.wrapping_add(0x9E37_79B9));
                    (f64::from(state >> 8) / f64::from(1u32 << 23) - 1.0) as f32
                })
                .collect();
            let (left, right) = render_stereo(&mut tape, &noise, &noise);
            for (index, sample) in left.iter().chain(&right).enumerate() {
                assert!(
                    sample.is_finite() && f64::from(sample.abs()) <= ceiling,
                    "{fs} Hz: sample {index} was {sample}, past a ceiling of {ceiling:.2}"
                );
            }
        }
    }

    /// **The rate does not change the tape.** The medium is an ODE with a
    /// step in seconds and the filters are designed from the rate, so the
    /// same settings have to sound the same at 44.1, 48 and 96 kHz.
    #[test]
    fn the_rate_does_not_change_the_sound() {
        let mut reference = (0.0f64, 0.0f64, 0.0f64);
        for (index, fs) in [44_100.0f64, 48_000.0, 96_000.0].iter().enumerate() {
            let fs = *fs;
            let mut tape = still(fs);
            let out = steady(&mut tape, 1000.0, -12.0, fs);
            let thd = thd_percent(&out, 1000.0, fs);
            let gain = 20.0 * (tone_amplitude(&out, 1000.0, fs) / db_to_amp(-12.0)).log10();

            let mut wobbling = Tape::new(fs);
            let input = sine(5000.0, db_to_amp(-12.0), (12.0 * fs) as usize, fs);
            let (rendered, _) = render(&mut wobbling, &input);
            let wow = deviation_at(&rendered[fs as usize..], 5000.0, WOW_HZ, fs);

            if index == 0 {
                reference = (thd, gain, wow);
            } else {
                assert!(
                    (thd - reference.0).abs() < reference.0 * 0.05,
                    "{fs} Hz: THD {thd:.4}% against {:.4}%",
                    reference.0
                );
                assert!(
                    (gain - reference.1).abs() < 0.05,
                    "{fs} Hz: gain {gain:+.3} dB against {:+.3}",
                    reference.1
                );
                assert!(
                    (wow - reference.2).abs() < reference.2 * 0.02,
                    "{fs} Hz: wow {wow:.4}% against {:.4}%",
                    reference.2
                );
            }
        }
    }

    /// **Nothing is allocated while it runs** — including while the speed is
    /// changed, which is the one control that rewrites filter coefficients
    /// rather than a gain.
    #[test]
    fn nothing_is_allocated_while_it_runs() {
        let mut tape = tape();
        let mut left = sine(220.0, 0.3, 512, FS);
        let mut right = left.clone();
        for _ in 0..8 {
            tape.process(&mut left, &mut right);
        }
        let allocations = crate::synth::tests::allocations_during(|| {
            for block in 0..200 {
                if block % 20 == 0 {
                    tape.set_param_natural(PARAM_SPEED, f32::from(block as u8 % 3));
                    tape.set_param_natural(PARAM_DRIVE, (block % 100) as f32);
                    tape.set_param_natural(PARAM_WOW, ((block * 7) % 100) as f32);
                    tape.set_param_natural(PARAM_AZIMUTH_DEG, (block % 2) as f32);
                }
                tape.process(&mut left, &mut right);
            }
            tape.reset();
            tape.snap();
        });
        assert_eq!(allocations, 0, "the audio path allocated");
    }

    // ── The controls ──

    /// The twelve controls are the ones the table declares, and a whole
    /// vector written back in index order restores the instance — which is
    /// the session load path.
    #[test]
    fn every_control_round_trips() {
        let defaults = default_natural_params();
        assert_eq!(defaults.len(), PARAM_COUNT);
        assert_eq!(defaults[PARAM_SPEED], Speed::Studio.index() as f32);
        assert_eq!(defaults[PARAM_MIX], 100.0);
        assert_eq!(defaults[PARAM_HISS], 0.0);
        assert_eq!(defaults[PARAM_AZIMUTH_DEG], 0.0);
        assert_eq!(defaults[PARAM_AUTO_MAKEUP], 1.0);
        assert!(natural_param(PARAM_COUNT).is_none());
        assert_eq!(param_name(PARAM_COUNT), "");

        let written = [
            (PARAM_SPEED, 2.0f32),
            (PARAM_DRIVE, 73.0),
            (PARAM_SAT, 12.0),
            (PARAM_BIAS, 91.0),
            (PARAM_WOW, 33.0),
            (PARAM_FLUTTER, 88.0),
            (PARAM_BUMP_DB, 2.25),
            (PARAM_AZIMUTH_DEG, 0.4),
            (PARAM_HISS, 55.0),
            (PARAM_TRIM_DB, -7.5),
            (PARAM_AUTO_MAKEUP, 0.0),
            (PARAM_MIX, 64.0),
        ];
        assert_eq!(written.len(), PARAM_COUNT, "a control was left out of the round trip");
        let mut source = tape();
        for (index, value) in written {
            source.set_param_natural(index, value);
        }
        let saved: Vec<f32> = (0..PARAM_COUNT).map(|i| source.param_natural(i)).collect();
        let mut restored = Tape::new(44_100.0);
        for (index, value) in saved.iter().enumerate() {
            restored.set_param_natural(index, *value);
        }
        let read_back: Vec<f32> = (0..PARAM_COUNT).map(|i| restored.param_natural(i)).collect();
        assert_eq!(saved, read_back);
        for (index, value) in written {
            assert_eq!(read_back[index], value, "index {index}");
        }
        assert_eq!(restored.speed(), Speed::Fast);
    }

    /// Nonsense from a UI or a hand-edited session file is refused, not
    /// propagated into a differential equation.
    #[test]
    fn it_survives_nonsense() {
        let mut tape = tape();
        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| tape.param_natural(i)).collect();
        tape.set_param_natural(PARAM_COUNT, 1.0);
        tape.set_param_natural(usize::MAX, 1.0);
        for index in 0..PARAM_COUNT {
            tape.set_param_natural(index, f32::NAN);
            tape.set_param_natural(index, f32::INFINITY);
        }
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| tape.param_natural(i)).collect();
        assert_eq!(before, after);
        assert_eq!(tape.param_natural(PARAM_COUNT), 0.0);

        // A rate the device could not have asked for leaves the tape built at
        // the last one it was given, and still sounding.
        tape.set_sample_rate(0.0);
        tape.set_sample_rate(f64::NAN);
        tape.set_sample_rate(-1.0);
        assert_eq!(tape.sample_rate(), FS);
        let (out, _) = render(&mut tape, &sine(440.0, 0.3, 4096, FS));
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(peak(&out) > 0.1);

        // Out-of-range values clamp to the table rather than reaching the
        // model.
        tape.set_param_natural(PARAM_DRIVE, 1_000.0);
        tape.set_param_natural(PARAM_TRIM_DB, -1_000.0);
        assert_eq!(tape.param_natural(PARAM_DRIVE), 100.0);
        assert_eq!(tape.param_natural(PARAM_TRIM_DB), -24.0);

        // Speeds past the end of the list answer the last one rather than
        // panicking on an index.
        assert_eq!(Speed::from_index(99), Speed::Fast);
        assert_eq!(speed_of(&[7.0]), Speed::Fast);
        assert_eq!(speed_of(&[]), Speed::Slow);
    }

    /// The derived numbers the panel prints are the ones the transport runs.
    #[test]
    fn the_panel_numbers_are_the_transports_own() {
        assert!((wow_percent(50.0) - 0.1).abs() < 1.0e-9, "the default wow is not 0.1%");
        assert!((flutter_percent(50.0) - 0.03).abs() < 1.0e-9, "the default flutter is not 0.03%");
        assert_eq!(wow_percent(0.0), 0.0);
        assert_eq!(flutter_percent(0.0), 0.0);
        // Cubed, so the useful travel is not all crammed against the bottom.
        assert!(wow_percent(25.0) < wow_percent(50.0) / 4.0);

        assert!((bump_hz(Speed::Studio) - 70.0).abs() < 1.0e-9);
        assert!((loss_hz(Speed::Slow) - 7_500.0).abs() < 1.0e-9);
        assert_eq!(azimuth_hz(0.0, Speed::Studio), f64::INFINITY);
        assert!(azimuth_hz(0.5, Speed::Fast) > azimuth_hz(0.5, Speed::Studio));

        for speed in Speed::ALL {
            assert_eq!(Speed::from_index(speed.index()), speed);
            assert!(!speed.label().is_empty());
        }
        assert_eq!(Speed::Studio.factor(), 1.0);

        // The excursion closed form, which is what the depth means.
        assert!(
            (excursion_seconds(0.001, 0.6) - 0.001 / (TAU * 0.6)).abs() < 1.0e-12,
            "the excursion is not D/(2πf)"
        );
        assert_eq!(excursion_seconds(0.001, 0.0), 0.0);
    }

    /// Not an assertion: the table this file's tolerances came from.
    ///
    /// `cargo test -p phosphor-dsp --release --lib fx::tape::tests::the_cost -- --nocapture`
    ///
    /// Measured, release, Apple silicon, stereo, per instance:
    ///
    /// | stage | ns / stereo frame | share of one core at 48 kHz |
    /// |---|---|---|
    /// | the whole device | 466 | **2.24%** |
    /// | *of which the transport* | 36 | 0.17% |
    /// | *of which the libm `tanh`* | ~50 | 0.24% |
    ///
    /// The brief's budget was 1.78% for the hysteresis and the oversampling
    /// filters alone; the rest is the record and reproduce EQ pairs, the
    /// wobbling line, the head and the smoothers. The first optimisation, if
    /// one is ever needed, is a rational `tanh` — it is worth 13% and it
    /// costs the exactness of the harmonic table, which is why it is not
    /// here. Splitting the halfband's dot products across four accumulators
    /// was worth 5% and cost nothing, which is why that one *is*.
    #[test]
    fn the_cost() {
        let mut tape = tape();
        let source = sine(220.0, 0.3, 512, FS);
        let mut left = source.clone();
        let mut right = source.clone();
        for _ in 0..64 {
            tape.process(&mut left, &mut right);
        }
        // The source is written back every block. An effect benchmarked on
        // its own output is benchmarked on whatever that output has become,
        // and this one is lined up a decibel below unity: a few hundred
        // passes and the buffer is silence, where the medium takes the
        // Langevin series' cheap branch instead of its `tanh`.
        let blocks = 200;
        let started = std::time::Instant::now();
        for _ in 0..blocks {
            left.copy_from_slice(&source);
            right.copy_from_slice(&source);
            tape.process(&mut left, &mut right);
        }
        let per_frame = started.elapsed().as_secs_f64() / (blocks * 512) as f64;
        println!(
            "  tape: {:.1} ns per stereo frame = {:.3}% of one core at 48 kHz",
            per_frame * 1.0e9,
            per_frame * FS * 100.0
        );
        assert!(left.iter().all(|s| s.is_finite()));
    }



    /// The knob-torture standard: speed, wow, flutter and the head bump
    /// flicked fast under a tone. The wow line is a modulated delay — the
    /// class of machinery that broke the reverb — so the proof matters:
    /// bounded during, silent after.
    #[test]
    fn knob_torture_stays_bounded() {
        let fs = 48_000.0;
        let mut tape = Tape::new(fs);
        let mut toggle = false;
        let mut peak = 0.0f32;
        let frames = (2.0 * fs) as usize;
        for n in 0..frames {
            if n % ((fs * 0.02) as usize) == 0 {
                toggle = !toggle;
                tape.set_param_natural(PARAM_SPEED, if toggle { 0.0 } else { 2.0 });
                tape.set_param_natural(PARAM_WOW, if toggle { 100.0 } else { 0.0 });
                tape.set_param_natural(PARAM_FLUTTER, if toggle { 100.0 } else { 0.0 });
                tape.set_param_natural(PARAM_BUMP_DB, if toggle { 6.0 } else { 0.0 });
                tape.set_param_natural(PARAM_DRIVE, if toggle { 12.0 } else { -6.0 });
            }
            let x = 0.25 * (2.0 * std::f64::consts::PI * 220.0 * n as f64 / fs).sin() as f32;
            let mut l = [x];
            let mut r = [x];
            tape.process(&mut l, &mut r);
            assert!(l[0].is_finite() && r[0].is_finite(), "the tape went non-finite");
            peak = peak.max(l[0].abs()).max(r[0].abs());
        }
        assert!(peak < 4.0, "the torture blew up: peak {peak}");

        // Silence in: the machine must go quiet (hiss is off by default).
        let mut late = 0.0f32;
        for n in 0..(fs as usize) {
            let mut l = [0.0f32];
            let mut r = [0.0f32];
            tape.process(&mut l, &mut r);
            if n > (fs * 0.5) as usize {
                late = late.max(l[0].abs());
            }
        }
        assert!(late < 1.0e-3, "the tape kept sounding after silence: {late}");
    }
}
