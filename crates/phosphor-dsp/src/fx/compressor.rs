//! The compressor: a gain computer, a detector *behind* it, and a release
//! knob that tells the truth.
//!
//! # The one decision everything else follows from
//!
//! Giannoulis, Massberg and Reiss spent a paper measuring the seven or eight
//! ways a digital compressor can be wired and then wrote down which one to
//! build. This is that one, verbatim: feed-forward, and with the level
//! detector placed **after** the gain computer, in the log domain — their
//! Fig. 7(c), their Eq. (23).
//!
//! ```text
//!         ┌──────────────────────── dry ─────────────────────────┐
//!  in L/R ┤                                                      ▼
//!         └──────────────────────── wet ───────────────► × c ──► mix ──► out
//!                                                          ▲
//!   key (another track's signal, or this one)               │
//!         │                                                 │
//!         ▼                                                 │
//!    ┌─────────┐  ┌────────┐  ┌────┐  ┌──────────┐  ┌───────┴──────┐
//!    │ S/C HPF │─►│ sense  │─►│ dB │─►│   GAIN   │─►│   DETECTOR   │
//!    │ 2-pole  │  │ peak / │  │    │  │ COMPUTER │  │  attack and  │
//!    │ 20..300 │  │ rms    │  │    │  │  T, R, W │  │   release    │
//!    └─────────┘  └────────┘  └────┘  └──────────┘  └──────────────┘
//! ```
//!
//! Everything on that bottom row is in decibels, and the consequence is the
//! reason the topology was chosen: **threshold, ratio and knee cannot
//! zipper.** They move the gain computer's output, which the attack/release
//! filter then smooths before it becomes a gain. Slam the threshold knob
//! across its whole travel at a block boundary and what comes out is a glide
//! at the attack rate, not a click. Only the makeup and the parallel mix sit
//! downstream of the detector, and those two are the only two that are
//! ramped by hand.
//!
//! The feedback topology was rejected for a reason worth writing down: a
//! feedback compressor cannot limit. A ratio of ∞:1 needs infinite negative
//! amplification round the loop, so the top of the ratio knob would have to
//! be a lie. Here the ratio control is [linear in *slope*](ratio_to_percent),
//! `S = 1/R − 1`, so 1:1 is `S = 0` exactly and ∞:1 is `S = −1` exactly —
//! both ends of the travel are ordinary numbers and neither needs a clamp.
//!
//! # The release knob lied, and here is the fix
//!
//! The detector is GMR's Eq. (17), the *smooth, decoupled* peak detector,
//! which is the one they recommend for the widest range of material and the
//! one with the lowest measured distortion of the four they publish. It has
//! one known defect, stated in both of their papers: the attack filter also
//! shapes the release trajectory, so the *measured* release time is about
//!
//! ```text
//! τ_measured ≈ τ_A + τ_R
//! ```
//!
//! A release knob wired straight through therefore reads low by exactly the
//! attack time. At the settings a drum bus lives at — 30 ms attack, 50 ms
//! release — the knob is off by more than half. Their own follow-up paper
//! fixes it inside their automatic mode by subtracting the attack time; this
//! compressor applies the same correction to the *manual* knob:
//!
//! ```text
//! τ_R_effective = max(τ_R_dialled − τ_A, 1/fs)
//! ```
//!
//! Measured on the isolated detector at 44.1 kHz with a 1 ms attack, at the
//! four release settings the tests sweep:
//!
//! | dialled | compensated | uncompensated | τ_A + τ_R |
//! |---|---|---|---|
//! | 5 ms | 5.102 ms | 6.077 ms | 6 ms |
//! | 50 ms | 49.977 ms | 50.998 ms | 51 ms |
//! | 500 ms | 499.977 ms | 500.975 ms | 501 ms |
//! | 3000 ms | 2999.977 ms | 3000.975 ms | 3001 ms |
//!
//! Both columns are asserted, because a fix nobody can see fail is a fix
//! nobody can see removed.
//!
//! The `max(…, 1/fs)` end of it has a second job: the release can never be
//! made faster than the attack. GMR call that desirable and so does this
//! house — a compressor whose gain comes back faster than it went down is a
//! compressor that is modulating the waveform rather than riding it.
//!
//! # Peak and RMS are two different questions
//!
//! The detector above sits after the gain computer, so it is not the place
//! for an RMS window. RMS goes where the dbx 160A put it: at the **head** of
//! the sidechain, before the decibel conversion. The two stages compose —
//! mean-square front end, then gain computer, then log-domain ballistics —
//! and the switch between them is calibrated so that a steady sine reads the
//! same in both. That `+3.01 dB` is not cosmetic: without it, flipping the
//! switch moves the effective threshold by three decibels and every preset
//! in the bank is wrong.
//!
//! What the RMS position is *for*, as a number rather than an adjective: the
//! mean-square front end reads a signal lower by its crest factor, so a snare
//! with 10 dB of crest gets about 10 dB less gain reduction than a sine at
//! the same peak level and the same threshold. That is the whole of "glue" —
//! the compressor stops chasing transients and starts riding the body — and
//! it is measured rather than described. The corollary, also measured: in the
//! RMS position the attack can never be faster than the 10 ms window,
//! whatever the attack knob says. Which is why an RMS drum smasher does not
//! exist.
//!
//! # Zero latency, and what it costs
//!
//! No lookahead, and this is not a scheduling deferral. The mixer has no
//! plugin delay compensation, so one insert that delayed its track by five
//! milliseconds would smear that track against every other track, against
//! both sends, and against the dry half of its own parallel mix.
//!
//! What is given up is precise and is measured rather than hidden: at ∞:1
//! this is a limiter in *shape* and not a brickwall, because the first τ_A of
//! every transient goes past with the gain still on its way down. The
//! overshoot is a documented number — see
//! `transient_overshoot_is_measured_not_assumed` — and the master safety
//! limiter remains the only thing that guarantees a ceiling.
//!
//! What is bought is that every null in the test file is exact: bypass is
//! bit-identical, 1:1 is bit-identical, `mix 0` is bit-identical, and a
//! parallel path stays sample-aligned with the dry one it is summed against.

use std::f64::consts::PI;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// `20 / ln 10`. Natural logs and one multiply, rather than `log10` and a
/// second one — the two transcendentals are most of the per-sample cost, and
/// `ln` is the one the library actually implements.
const LN_TO_DB: f64 = 8.685_889_638_065_035;

/// `ln 10 / 20`, the other way.
const DB_TO_LN: f64 = 0.115_129_254_649_702_3;

/// The quietest level the peak detector will name, −140 dB.
///
/// The lowest knee edge the controls can reach is `−60 − 12 = −72 dB`, so the
/// floor never intrudes on anything audible; what it does is keep `ln(0)`
/// from producing negative infinity and poisoning every state downstream of
/// it.
const PEAK_FLOOR: f64 = 1.0e-7;

/// The same floor for the mean-square state, which is a squared level.
const MS_FLOOR: f64 = 1.0e-14;

/// The window the RMS front end averages over.
///
/// Not exposed. Two controls that both mean "how long is a transient" is a
/// trap: the attack knob is the one a player reaches for, and a second knob
/// that silently overrides it at the bottom of its travel would be a control
/// whose effect depends on another control's position.
const RMS_WINDOW_SECONDS: f64 = 0.010;

/// What is added to the mean-square reading so a steady sine measures the
/// same in both sense positions.
///
/// A sine of amplitude `A` has a mean square of `A²/2`, which is 3.0103 dB
/// under its own peak. Adding it back means the threshold knob means the same
/// thing on either side of the switch — and every character preset survives
/// being flipped.
const RMS_CALIBRATION_DB: f64 = 3.010_299_956_639_812;

/// Below this much reduction, in decibels, a detector state is flushed to
/// zero.
///
/// A hundred-billionth of a decibel is a gain of `1 − 1.2e−11`; what it is
/// really worth is that the states reach *exactly* zero after a signal
/// stops, so silence in gives silence out and the 1:1 null is a property of
/// the arithmetic rather than of how long the test ran.
const DETECTOR_FLOOR_DB: f64 = 1.0e-10;

/// Below this, a filter or mean-square state is flushed to zero. These are
/// squared levels and filter states rather than decibels, so the floor is
/// much further down.
const STATE_FLOOR: f64 = 1.0e-20;

/// The sidechain high-pass is a second-order Butterworth: `Q = 1/√2`, and the
/// state-variable form wants `1/Q`.
const HPF_K: f64 = std::f64::consts::SQRT_2;

/// The two release networks of the automatic mode, in seconds.
///
/// Solid State Logic publish both pairs for the bus compressor: AUTO is "a
/// two-stage release (100 ms short, 12 seconds long)" and AUTO 2 is "50 ms
/// short, 6 seconds long". Louder material lets go quickly and quieter
/// material slowly, which is the sentence these four numbers are.
const AUTO_RELEASE_FAST_S: f64 = 0.100;
const AUTO_RELEASE_SLOW_S: f64 = 12.0;
const AUTO2_RELEASE_FAST_S: f64 = 0.050;
const AUTO2_RELEASE_SLOW_S: f64 = 6.0;

/// How slowly the slow network *charges*, as a multiple of the attack time,
/// and the two ends it is held between.
///
/// This is the part that makes the automatic mode program-dependent rather
/// than merely slow. A 20 ms transient charges a 100 ms network by
/// `1 − e^(−0.2) ≈ 18%`, so a ninth of the reduction lingers — a tail, not a
/// stuck gain. A two-second passage charges it fully, so half the reduction
/// then takes the full twelve seconds to leave. That is the manual's sentence
/// rendered as arithmetic, and both halves of it are asserted.
///
/// **The floor is 100 ms and not 50.** A network that charges in fifty
/// milliseconds is not a slow network, it is a second fast one: at a 1 ms
/// attack it would take a third of its charge from a 20 ms transient and then
/// spend twelve seconds giving it back, which is a stuck gain and not a tail.
/// 100 ms is SSL's own published *fast* release, which is the natural
/// boundary between a transient and a passage.
const AUTO_SLOW_ATTACK_FACTOR: f64 = 10.0;
const AUTO_SLOW_ATTACK_MIN_S: f64 = 0.100;
const AUTO_SLOW_ATTACK_MAX_S: f64 = 0.500;

/// How the two automatic networks are weighted against each other.
///
/// The Teletronix LA-2A's specification is "approximately 0.06 seconds for
/// 50% release, 0.5 to 5 seconds for complete release depending upon the
/// amount of previous reduction". Half fast and half slow is that figure
/// taken literally.
const AUTO_MIX: f64 = 0.5;

// ---------------------------------------------------------------------------
// The two switches
// ---------------------------------------------------------------------------

/// What the sidechain measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sense {
    /// `max(|L|, |R|)`, instantaneous. Reads the transient. What the SSL bus
    /// compressor does — "the left and right channels are independently
    /// rectified using a true peak full wave detector circuit".
    Peak,
    /// A 10 ms mean square, calibrated so a sine reads the same as it does in
    /// [`Sense::Peak`]. Reads the body.
    Rms,
}

impl Sense {
    pub const ALL: [Sense; 2] = [Sense::Peak, Sense::Rms];

    #[must_use]
    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Sense::Peak)
    }

    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Peak => 0,
            Self::Rms => 1,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Peak => "peak",
            Self::Rms => "rms",
        }
    }
}

/// Whether the release time is the knob's or the programme's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoRelease {
    /// The `releas` knob, through the compensated smooth-decoupled detector.
    Off,
    /// Two networks, 100 ms and 12 s, summed half and half.
    Auto,
    /// The same shape, faster: 50 ms and 6 s.
    Auto2,
}

impl AutoRelease {
    pub const ALL: [AutoRelease; 3] = [AutoRelease::Off, AutoRelease::Auto, AutoRelease::Auto2];

    #[must_use]
    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(AutoRelease::Off)
    }

    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Off => 0,
            Self::Auto => 1,
            Self::Auto2 => 2,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::Auto2 => "auto 2",
        }
    }

    /// `(fast, slow)` release time constants in seconds, or `None` when the
    /// knob is in charge.
    #[must_use]
    pub fn networks(self) -> Option<(f64, f64)> {
        match self {
            Self::Off => None,
            Self::Auto => Some((AUTO_RELEASE_FAST_S, AUTO_RELEASE_SLOW_S)),
            Self::Auto2 => Some((AUTO2_RELEASE_FAST_S, AUTO2_RELEASE_SLOW_S)),
        }
    }
}

// ---------------------------------------------------------------------------
// The flat parameter surface, in natural units
// ---------------------------------------------------------------------------
//
// Decibels, milliseconds, hertz and percent — never a 0..1 knob fraction. A
// session stores what a control *meant*, so a range that moves later cannot
// silently re-point every saved file.

/// Which character was last recalled. A macro, not a mode — see [`CHARACTERS`].
pub const PARAM_CHARACTER: usize = 0;
/// Where compression starts, in dBFS.
pub const PARAM_THRESHOLD_DB: usize = 1;
/// The slope, as a percentage of full limiting. See [`ratio_to_percent`].
pub const PARAM_RATIO: usize = 2;
/// How wide the knee is, in decibels, centred on the threshold.
pub const PARAM_KNEE_DB: usize = 3;
pub const PARAM_ATTACK_MS: usize = 4;
pub const PARAM_RELEASE_MS: usize = 5;
/// [`AutoRelease`], as an index.
pub const PARAM_AUTO_RELEASE: usize = 6;
pub const PARAM_MAKEUP_DB: usize = 7;
/// Whether the makeup follows the threshold and ratio. On by default.
pub const PARAM_AUTO_MAKEUP: usize = 8;
/// The parallel blend, 0 dry to 100 fully compressed.
pub const PARAM_MIX: usize = 9;
/// [`Sense`], as an index.
pub const PARAM_SENSE: usize = 10;
/// The detector high-pass corner in hertz; zero is off.
pub const PARAM_SC_HPF_HZ: usize = 11;

/// How many controls a compressor has.
pub const PARAM_COUNT: usize = 12;

/// The lowest corner the sidechain high-pass can be set to. Below this the
/// control reads `off` and the filter is out of the path entirely.
pub const SC_HPF_MIN_HZ: f32 = 20.0;
/// The highest, matching SSL's own "ranging from OUT to 300 Hz".
pub const SC_HPF_MAX_HZ: f32 = 300.0;

/// One control, as a host sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NaturalParam {
    pub name: &'static str,
    /// `"dB"`, `"ms"`, `"Hz"`, `"%"`, or empty for the counted controls and
    /// switches.
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

/// The table every other view of the parameters is generated from.
///
/// The defaults are the house's "hear it in two seconds" settings: threshold
/// −18 dB, 3:1, a 6 dB soft knee, 10 ms attack, 120 ms release, **automatic
/// makeup on**, fully wet. Dropped on a track whose instrument is gain-staged
/// to −12 dBFS peaks, that is two or three decibels of reduction on the loud
/// notes and nothing on the quiet ones, which is what a compressor is
/// supposed to sound like the first time.
///
/// `ratio` is stored as the slope in percent rather than as `3.0`, because
/// the top of the travel is infinity and a session file cannot hold one. The
/// panel never shows the percentage — it shows `3.0:1` and `∞:1` — and the
/// two are the same number through [`ratio_to_percent`].
const PARAMS: [NaturalParam; PARAM_COUNT] = [
    NaturalParam { name: "char", unit: "", min: 0.0, max: 8.0, default: 0.0 },
    NaturalParam { name: "thresh", unit: "dB", min: -60.0, max: 0.0, default: -18.0 },
    // 66.666_664 is `ratio_to_percent(3.0)` exactly, in f32. The test
    // `the_ratio_law_is_linear_in_slope` is what keeps it that way.
    NaturalParam { name: "ratio", unit: "%", min: 0.0, max: 100.0, default: 66.666_664 },
    NaturalParam { name: "knee", unit: "dB", min: 0.0, max: 24.0, default: 6.0 },
    NaturalParam { name: "attack", unit: "ms", min: 0.05, max: 100.0, default: 10.0 },
    NaturalParam { name: "releas", unit: "ms", min: 5.0, max: 3_000.0, default: 120.0 },
    NaturalParam { name: "arel", unit: "", min: 0.0, max: 2.0, default: 0.0 },
    NaturalParam { name: "makeup", unit: "dB", min: -30.0, max: 30.0, default: 0.0 },
    NaturalParam { name: "mkauto", unit: "", min: 0.0, max: 1.0, default: 1.0 },
    NaturalParam { name: "mix", unit: "%", min: 0.0, max: 100.0, default: 100.0 },
    NaturalParam { name: "sense", unit: "", min: 0.0, max: 1.0, default: 0.0 },
    NaturalParam { name: "schpf", unit: "Hz", min: 0.0, max: SC_HPF_MAX_HZ, default: 0.0 },
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

/// Whether a control does anything at these settings.
///
/// Two controls on this panel can be taken over by an automatic, and both of
/// them grey out rather than disappearing:
///
/// * `makeup` while `mkauto` is on — the threshold and the ratio are setting
///   it.
/// * `releas` while `arel` is anything but `off` — the two automatic networks
///   have their own published time constants and the knob is not one of them.
///
/// The house rule the panel enforces on top of this: **turning a greyed
/// control takes it back.** Reaching for the makeup knob switches the
/// automatic off and seeds the knob with the value it was already producing,
/// so the control never jumps; reaching for the release knob switches `arel`
/// to `off`. A control that simply refuses to move is a control a player
/// assumes is broken.
///
/// The sidechain high-pass is *not* in this list even though it reads `off`
/// at the bottom of its travel: a control you cannot turn back on is worse
/// than a control that is doing nothing.
#[must_use]
pub fn uses(params: &[f32], index: usize) -> bool {
    match index {
        PARAM_MAKEUP_DB => at(params, PARAM_AUTO_MAKEUP) < 0.5,
        PARAM_RELEASE_MS => auto_release_of(params) == AutoRelease::Off,
        _ => index < PARAM_COUNT,
    }
}

/// The sense position a parameter vector names.
#[must_use]
pub fn sense_of(params: &[f32]) -> Sense {
    Sense::from_index(at(params, PARAM_SENSE).round().max(0.0) as usize)
}

/// The automatic-release position a parameter vector names.
#[must_use]
pub fn auto_release_of(params: &[f32]) -> AutoRelease {
    AutoRelease::from_index(at(params, PARAM_AUTO_RELEASE).round().max(0.0) as usize)
}

/// Whether the makeup follows the threshold and the ratio.
#[must_use]
pub fn auto_makeup_on(params: &[f32]) -> bool {
    at(params, PARAM_AUTO_MAKEUP) >= 0.5
}

// ── The ratio law ──

/// The slope percentage a ratio names — the number the `ratio` control
/// stores. Infinity answers 100.
///
/// The law is `S = 1/R − 1`, and the control is linear in `S` rather than in
/// `R`. Three things follow, and none of them are conveniences:
///
/// * `0%` is `S = 0` exactly, so the 1:1 null is a property of the law rather
///   than a special case bolted onto it.
/// * `100%` is `S = −1` exactly — a true limiter, with no ratio variable
///   anywhere in the code ever holding infinity.
/// * `S` is the compression in decibels per decibel of overshoot, which is
///   the quantity the ear actually tracks. Linear in `S` is linear in effect,
///   and the travel lands where mixing lives: the bottom half of the knob
///   covers 1:1 to 2:1 and the top tenth covers 10:1 to ∞, where everything
///   sounds the same anyway.
#[must_use]
pub fn ratio_to_percent(ratio: f64) -> f32 {
    if !ratio.is_finite() {
        return 100.0;
    }
    (100.0 * (1.0 - 1.0 / ratio.max(1.0))) as f32
}

/// The ratio a slope percentage names. `100` answers [`f64::INFINITY`].
#[must_use]
pub fn percent_to_ratio(percent: f32) -> f64 {
    let slope = f64::from(percent.clamp(0.0, 100.0)) / 100.0;
    if slope >= 1.0 {
        f64::INFINITY
    } else {
        1.0 / (1.0 - slope)
    }
}

/// The ratio, as a compressor says it: `3.0:1`, `20:1`, `∞:1`.
#[must_use]
pub fn ratio_label(percent: f32) -> String {
    let ratio = percent_to_ratio(percent);
    if !ratio.is_finite() {
        return "\u{221e}:1".to_string();
    }
    if ratio < 9.95 {
        format!("{ratio:.1}:1")
    } else {
        format!("{ratio:.0}:1")
    }
}

/// The ratios a coarse press steps between.
///
/// Continuous travel with clean stops: `h`/`l` moves the slope by one point
/// and `H`/`L` jumps to the next number a manual would print.
pub const RATIO_STOPS: [f64; 11] =
    [1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 20.0, f64::INFINITY];

// ---------------------------------------------------------------------------
// Character
// ---------------------------------------------------------------------------

/// One character: a name and the twelve numbers behind it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Character {
    pub name: &'static str,
    /// What it is, in the words a player would use.
    pub note: &'static str,
    pub params: [f32; PARAM_COUNT],
}

/// A shorthand for the table below, so a row reads as its settings rather
/// than as twelve positional floats.
///
/// Eleven arguments, which is more than clippy likes and fewer than the twelve
/// bare floats the alternative would be. A builder cannot help — this runs in
/// a `const` — and naming the arguments is the entire point of the function.
#[allow(clippy::too_many_arguments)]
const fn character(
    name: &'static str,
    note: &'static str,
    sense: Sense,
    thresh: f32,
    ratio: f32,
    knee: f32,
    attack: f32,
    release: f32,
    arel: AutoRelease,
    schpf: f32,
    mix: f32,
    makeup: Option<f32>,
) -> Character {
    Character {
        name,
        note,
        params: [
            0.0, // filled in by `character_params`
            thresh,
            ratio,
            knee,
            attack,
            release,
            match arel {
                AutoRelease::Off => 0.0,
                AutoRelease::Auto => 1.0,
                AutoRelease::Auto2 => 2.0,
            },
            match makeup {
                Some(db) => db,
                None => 0.0,
            },
            match makeup {
                Some(_) => 0.0,
                None => 1.0,
            },
            mix,
            match sense {
                Sense::Peak => 0.0,
                Sense::Rms => 1.0,
            },
            schpf,
        ],
    }
}

/// The characters, as parameter sets.
///
/// **These are a macro, not a mode.** There is one compressor in this file
/// and one topology; what a classic sounds like is mostly its ballistics and
/// its ratio, and those are numbers this design already has. What a second
/// engine would buy — the FET's input-stage distortion and its bias shift in
/// all-buttons mode, the opto cell's frequency-dependent sensitivity — is
/// saturation and detector shaping, both of which are additive later without
/// touching the topology, and neither of which is worth doubling the
/// verification surface for now.
///
/// Index 0 is the factory setting itself, so the control has a home to come
/// back to. The panel marks the reading `edited` the moment the parameters
/// stop matching, because a selector that keeps naming a preset after the
/// preset has been dialled away from is a selector that lies.
pub const CHARACTERS: [Character; 9] = [
    character(
        "basic",
        "the factory setting",
        Sense::Peak,
        -18.0,
        66.666_664,
        6.0,
        10.0,
        120.0,
        AutoRelease::Off,
        0.0,
        100.0,
        None,
    ),
    character(
        "bus glue",
        "the SSL. aim for 2-4 dB",
        Sense::Peak,
        -18.0,
        50.0,
        10.0,
        30.0,
        120.0,
        AutoRelease::Auto,
        60.0,
        100.0,
        None,
    ),
    character(
        "mix glue",
        "tighter, more active",
        Sense::Peak,
        -16.0,
        75.0,
        8.0,
        10.0,
        120.0,
        AutoRelease::Auto2,
        100.0,
        100.0,
        None,
    ),
    character(
        "drum smash",
        "hear the room come up",
        Sense::Peak,
        -24.0,
        90.0,
        0.0,
        0.1,
        100.0,
        AutoRelease::Off,
        0.0,
        100.0,
        None,
    ),
    character(
        "all buttons",
        "the 1176, every button in",
        Sense::Peak,
        -30.0,
        95.0,
        0.0,
        0.05,
        60.0,
        AutoRelease::Off,
        0.0,
        100.0,
        None,
    ),
    character(
        "vocal level",
        "opto. two-stage release",
        Sense::Rms,
        -22.0,
        66.666_664,
        12.0,
        10.0,
        120.0,
        AutoRelease::Auto,
        0.0,
        100.0,
        None,
    ),
    character(
        "parallel",
        "new york. crushed, blended",
        Sense::Peak,
        -40.0,
        100.0,
        0.0,
        0.05,
        60.0,
        AutoRelease::Off,
        0.0,
        35.0,
        Some(6.0),
    ),
    character(
        "pumping",
        "needs a key. release is the effect",
        Sense::Peak,
        -30.0,
        87.5,
        0.0,
        0.05,
        300.0,
        AutoRelease::Off,
        0.0,
        100.0,
        None,
    ),
    character(
        "auto ratio",
        "infinite ratio, 24 dB knee",
        Sense::Peak,
        -20.0,
        100.0,
        24.0,
        10.0,
        200.0,
        AutoRelease::Off,
        0.0,
        100.0,
        None,
    ),
];

/// How many characters there are.
pub const CHARACTER_COUNT: usize = CHARACTERS.len();

/// The whole parameter vector a character names, with the selector itself set
/// to it.
#[must_use]
pub fn character_params(index: usize) -> [f32; PARAM_COUNT] {
    let index = index.min(CHARACTER_COUNT - 1);
    let mut params = CHARACTERS[index].params;
    params[PARAM_CHARACTER] = index as f32;
    params
}

/// The name of the character a parameter vector's selector is pointing at.
#[must_use]
pub fn character_name(params: &[f32]) -> &'static str {
    let index = (at(params, PARAM_CHARACTER).round().max(0.0) as usize).min(CHARACTER_COUNT - 1);
    CHARACTERS[index].name
}

/// What that character is, in the words a player would use.
#[must_use]
pub fn character_note(params: &[f32]) -> &'static str {
    let index = (at(params, PARAM_CHARACTER).round().max(0.0) as usize).min(CHARACTER_COUNT - 1);
    CHARACTERS[index].note
}

/// Whether the parameters still *are* the character the selector names.
///
/// The whole of the honesty measure: touching any control the character set
/// makes this false, and the panel says `edited` from then on.
#[must_use]
pub fn matches_character(params: &[f32]) -> bool {
    let index = (at(params, PARAM_CHARACTER).round().max(0.0) as usize).min(CHARACTER_COUNT - 1);
    let wanted = character_params(index);
    (0..PARAM_COUNT).all(|i| {
        // The release knob is not part of a character that has the automatic
        // release switched on — the automatic owns the release, so the knob's
        // position underneath it is not a difference anybody can hear.
        i == PARAM_CHARACTER
            || (i == PARAM_RELEASE_MS && auto_release_of(params) != AutoRelease::Off)
            || at(params, i) == wanted[i]
    })
}

// ---------------------------------------------------------------------------
// The pieces
// ---------------------------------------------------------------------------

/// The one-pole coefficient a time constant names, GMR Eq. (7).
///
/// `α = e^(−1/(τ·fs))`, where `τ` is defined as the time to reach `1 − 1/e` of
/// a step — which is the definition every measurement in the test file uses,
/// and the reason those measurements agree with the knob.
#[must_use]
pub fn one_pole(tau_seconds: f64, sample_rate: f64) -> f64 {
    if !(tau_seconds.is_finite() && sample_rate.is_finite()) || sample_rate <= 0.0 {
        return 0.0;
    }
    let tau = tau_seconds.max(1.0 / sample_rate);
    (-1.0 / (tau * sample_rate)).exp()
}

/// The release coefficient the knob's dialled value actually needs.
///
/// `τ_R_eff = max(τ_R − τ_A, 1/fs)`. See the module docs: without this the
/// measured release is `τ_A + τ_R` and the knob reads low by the attack time.
#[must_use]
pub fn compensated_release(attack_seconds: f64, release_seconds: f64, sample_rate: f64) -> f64 {
    one_pole((release_seconds - attack_seconds).max(1.0 / sample_rate), sample_rate)
}

/// GMR Eq. (17), the smooth decoupled peak detector, on its own.
///
/// Exposed so the ballistics can be measured the way the paper defines them —
/// a step in the *demanded* reduction, straight into the detector, with no
/// gain computer and no audio anywhere near it. Both `x_l` and the value it
/// answers with are amounts of reduction in decibels, and both are `≥ 0`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PeakDetector {
    /// The decoupled stage.
    y1: f64,
    /// The smoothed output.
    y_l: f64,
}

impl PeakDetector {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One sample. `a_att` and `a_rel` are [`one_pole`] coefficients.
    #[inline]
    #[must_use]
    pub fn tick(&mut self, x_l: f64, a_att: f64, a_rel: f64) -> f64 {
        self.y1 = x_l.max(a_rel * self.y1 + (1.0 - a_rel) * x_l);
        self.y_l = a_att * self.y_l + (1.0 - a_att) * self.y1;
        if self.y1 < DETECTOR_FLOOR_DB {
            self.y1 = 0.0;
        }
        if self.y_l < DETECTOR_FLOOR_DB {
            self.y_l = 0.0;
        }
        self.y_l
    }

    /// What it is holding now.
    #[must_use]
    pub fn current(&self) -> f64 {
        self.y_l
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// A topology-preserving state-variable filter, used for its high-pass.
///
/// Second order and not first, and the difference is the whole point: a
/// 6 dB/octave filter at 100 Hz takes only 6 dB off a 50 Hz kick fundamental,
/// which does not stop a mix pumping in time with the kick. Twelve gives
/// twelve, and at 200 Hz it gives twenty-four. SSL's own sidechain filter is
/// "a 12 dB/oct (2nd order) High-Pass Filter... ranging from OUT to 300 Hz",
/// and this is that filter.
#[derive(Debug, Clone, Copy, Default)]
struct Svf {
    ic1: f64,
    ic2: f64,
}

impl Svf {
    /// The three coefficients a corner frequency names, computed once per
    /// block when the corner is not moving.
    #[inline]
    fn coefficients(g: f64) -> (f64, f64, f64) {
        let a1 = 1.0 / (1.0 + g * (g + HPF_K));
        let a2 = g * a1;
        (a1, a2, g * a2)
    }

    /// `g = tan(π f / fs)`, the bilinear pre-warp.
    #[inline]
    fn g_for(hz: f64, sample_rate: f64) -> f64 {
        (PI * hz.clamp(1.0, sample_rate * 0.49) / sample_rate).tan()
    }

    #[inline]
    fn highpass(&mut self, x: f64, a1: f64, a2: f64, a3: f64) -> f64 {
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0f64.mul_add(v1, -self.ic1);
        self.ic2 = 2.0f64.mul_add(v2, -self.ic2);
        if self.ic1.abs() < STATE_FLOOR {
            self.ic1 = 0.0;
        }
        if self.ic2.abs() < STATE_FLOOR {
            self.ic2 = 0.0;
        }
        x - HPF_K * v1 - v2
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

/// A control that walks from where it was to where it is, in one block.
///
/// Linear and per block, not a one-pole: the makeup and the parallel mix are
/// the only two controls downstream of the detector and therefore the only
/// two that can zipper, and a ramp that *arrives* by the end of the block is
/// what makes `mix 0` exactly dry on the next one rather than asymptotically
/// dry forever.
#[derive(Debug, Clone, Copy, Default)]
struct Ramp {
    current: f64,
    target: f64,
    step: f64,
}

impl Ramp {
    fn set(&mut self, value: f64) {
        self.target = value;
    }

    fn snap(&mut self) {
        self.current = self.target;
        self.step = 0.0;
    }

    /// Prepare a block of `frames` samples, and answer whether the value is
    /// moving at all.
    fn begin(&mut self, frames: usize) -> bool {
        if self.current == self.target || frames == 0 {
            self.current = self.target;
            self.step = 0.0;
            return false;
        }
        self.step = (self.target - self.current) / frames as f64;
        true
    }

    #[inline]
    fn advance(&mut self) -> f64 {
        self.current += self.step;
        self.current
    }

    fn end(&mut self) {
        self.current = self.target;
        self.step = 0.0;
    }
}

// ---------------------------------------------------------------------------
// The compressor
// ---------------------------------------------------------------------------

/// A stereo compressor with an external sidechain.
pub struct Compressor {
    sample_rate: f64,
    params: [f32; PARAM_COUNT],

    // ── The sidechain ──
    hpf: [Svf; 2],
    /// Mean-square states, one per channel, for [`Sense::Rms`].
    ms: [f64; 2],
    hpf_g: Ramp,

    // ── The ballistics, all in decibels of reduction and all ≥ 0 ──
    detector: PeakDetector,
    /// The fast automatic-release network.
    y_f: f64,
    /// The slow one.
    y_s: f64,

    // ── Downstream of the detector, and therefore ramped ──
    makeup: Ramp,
    mix: Ramp,

    // ── Resolved once per block ──
    threshold_db: f64,
    /// `S = 1/R − 1`, in `[−1, 0]`.
    slope: f64,
    knee_db: f64,
    a_att: f64,
    a_rel: f64,
    a_att_slow: f64,
    a_rel_fast: f64,
    a_rel_slow: f64,
    a_rms: f64,
    sense: Sense,
    auto_release: AutoRelease,

    /// The smallest linear gain the reduction stage applied anywhere in the
    /// last block — what a gain-reduction meter draws, before the makeup.
    block_min_gain: f32,
}

impl Compressor {
    /// Build one at a sample rate.
    #[must_use]
    pub fn new(sample_rate: f64) -> Self {
        let mut comp = Self {
            sample_rate: 48_000.0,
            params: default_natural_params(),
            hpf: [Svf::default(); 2],
            ms: [0.0; 2],
            hpf_g: Ramp::default(),
            detector: PeakDetector::new(),
            y_f: 0.0,
            y_s: 0.0,
            makeup: Ramp::default(),
            mix: Ramp::default(),
            threshold_db: -18.0,
            slope: -2.0 / 3.0,
            knee_db: 6.0,
            a_att: 0.0,
            a_rel: 0.0,
            a_att_slow: 0.0,
            a_rel_fast: 0.0,
            a_rel_slow: 0.0,
            a_rms: 0.0,
            sense: Sense::Peak,
            auto_release: AutoRelease::Off,
            block_min_gain: 1.0,
        };
        comp.set_sample_rate(sample_rate);
        comp.snap();
        comp
    }

    /// Point it at a new rate and rebuild every coefficient. Allocates
    /// nothing; safe to call from anywhere, and called from `init` before the
    /// effect reaches a slot.
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return;
        }
        self.sample_rate = sample_rate;
        self.resolve();
        self.reset();
    }

    #[must_use]
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Drop every tail: detectors to rest, filters and mean-square states to
    /// zero. The controls are left alone.
    ///
    /// A re-enabled slot therefore starts from unity gain and attacks in over
    /// `τ_A`, which is click-free by itself and inaudible once the chain's own
    /// bypass crossfade is in front of it.
    pub fn reset(&mut self) {
        self.detector.clear();
        self.y_f = 0.0;
        self.y_s = 0.0;
        self.ms = [0.0; 2];
        for filter in &mut self.hpf {
            filter.clear();
        }
        self.block_min_gain = 1.0;
    }

    /// Take the two ramped controls straight to their targets.
    ///
    /// A session load sets the controls before the effect is in a slot, and
    /// those two are ramp targets. Snapping them means the first block a
    /// loaded session renders is the compressor that was saved rather than the
    /// factory one walking towards it.
    pub fn snap(&mut self) {
        self.push_targets();
        self.makeup.snap();
        self.mix.snap();
        self.hpf_g.snap();
    }

    // ── Parameters ──

    /// One control, in its own unit. Real-time safe.
    ///
    /// Setting the character selector stores *which* character was recalled
    /// and nothing else: recalling the parameter set is the front end's job,
    /// through [`character_params`]. One write here is one control, always —
    /// a `set_parameter` that quietly rewrote eleven others would make a
    /// session load depend on the order the controls happen to be written in.
    pub fn set_param_natural(&mut self, index: usize, value: f32) {
        let Some(info) = natural_param(index) else { return };
        if !value.is_finite() {
            return;
        }
        self.params[index] = value.clamp(info.min, info.max);
        self.push_targets();
    }

    /// A control's current value, in its own unit.
    #[must_use]
    pub fn param_natural(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    /// Every control at once, for a test or a preset recall.
    pub fn set_params(&mut self, params: &[f32]) {
        for (index, &value) in params.iter().enumerate().take(PARAM_COUNT) {
            self.set_param_natural(index, value);
        }
    }

    /// The makeup gain in force, in decibels — the automatic one when the
    /// automatic is on, and the knob otherwise.
    ///
    /// GMR's own estimate of the average gain reduction, negated:
    /// `M = −T·(1 − 1/R)/2`. −18 dB at 3:1 gives +6.0; −24 at 10:1 gives
    /// +10.8. It is recomputed when the threshold or the ratio moves and
    /// never from the *measured* reduction, because a makeup that followed the
    /// programme would make the output level depend on the material — which
    /// breaks the parallel-sum test, breaks an honest A/B, and makes a session
    /// non-reproducible.
    #[must_use]
    pub fn makeup_db(&self) -> f64 {
        if auto_makeup_on(&self.params) {
            self.auto_makeup_db()
        } else {
            f64::from(self.params[PARAM_MAKEUP_DB])
        }
    }

    /// What the automatic would produce at these settings, whether or not it
    /// is switched on. The panel seeds the manual knob from this so that
    /// taking the control back never moves the level.
    #[must_use]
    pub fn auto_makeup_db(&self) -> f64 {
        auto_makeup_for(
            f64::from(self.params[PARAM_THRESHOLD_DB]),
            f64::from(self.params[PARAM_RATIO]) / 100.0,
        )
    }

    /// The smallest linear gain the reduction stage applied anywhere in the
    /// last block.
    ///
    /// The *worst moment*, not the average, and before the makeup: a meter
    /// that averaged would never show a transient at all, and one that
    /// included the makeup would read nothing while a compressor was working
    /// hard and being made up for.
    #[must_use]
    pub fn block_min_gain(&self) -> f32 {
        self.block_min_gain
    }

    fn push_targets(&mut self) {
        self.makeup.set(self.makeup_db());
        self.mix.set(f64::from(self.params[PARAM_MIX]) / 100.0);
        let hz = f64::from(self.params[PARAM_SC_HPF_HZ]);
        if hz >= f64::from(SC_HPF_MIN_HZ) {
            self.hpf_g.set(Svf::g_for(hz, self.sample_rate));
        }
    }

    /// Whether the sidechain high-pass is in the path at all.
    #[must_use]
    pub fn hpf_on(&self) -> bool {
        self.params[PARAM_SC_HPF_HZ] >= SC_HPF_MIN_HZ
    }

    /// Everything settled once per block: the static curve, the ballistics
    /// coefficients, and the two switches.
    ///
    /// The coefficients are retuned without smoothing, and GMR justify it:
    /// the step-invariant one-pole preserves the analogue topology "with the
    /// capacitor's voltage as the state variable. Therefore, we will not
    /// experience any clicks and pops once we start varying the filter
    /// coefficients over time." The state-variable filter is not covered by
    /// that argument, which is why its `g` is the one thing here that ramps.
    fn resolve(&mut self) {
        let fs = self.sample_rate;
        self.threshold_db = f64::from(self.params[PARAM_THRESHOLD_DB]);
        self.slope = -f64::from(self.params[PARAM_RATIO]) / 100.0;
        self.knee_db = f64::from(self.params[PARAM_KNEE_DB]).max(0.0);

        let attack = f64::from(self.params[PARAM_ATTACK_MS]) / 1000.0;
        let release = f64::from(self.params[PARAM_RELEASE_MS]) / 1000.0;
        self.a_att = one_pole(attack, fs);
        self.a_rel = compensated_release(attack, release, fs);

        self.auto_release = auto_release_of(&self.params);
        let slow_attack =
            (attack * AUTO_SLOW_ATTACK_FACTOR).clamp(AUTO_SLOW_ATTACK_MIN_S, AUTO_SLOW_ATTACK_MAX_S);
        self.a_att_slow = one_pole(slow_attack, fs);
        let (fast, slow) = self
            .auto_release
            .networks()
            .unwrap_or((AUTO_RELEASE_FAST_S, AUTO_RELEASE_SLOW_S));
        self.a_rel_fast = one_pole(fast, fs);
        self.a_rel_slow = one_pole(slow, fs);

        self.a_rms = one_pole(RMS_WINDOW_SECONDS, fs);
        self.sense = sense_of(&self.params);
    }

    // ── Rendering ──

    /// Rewrite one block in place, keyed off `key` when there is one and off
    /// the input itself when there is not.
    ///
    /// The key is another track's signal as its instrument produced it —
    /// post-instrument, pre-insert — and it is always the same block this call
    /// is rendering. Which is why compressing the kick does not change how the
    /// kick ducks the bass: the trigger is the raw instrument, and a trigger
    /// that changed when you EQ'd the source would be a trigger nobody could
    /// rely on.
    pub fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<(&[f32], &[f32])>,
    ) {
        let frames = left.len().min(right.len());
        if frames == 0 {
            self.block_min_gain = 1.0;
            return;
        }
        self.resolve();

        // ── The three hoisted decisions ──
        //
        // Each of them is a null somebody depends on, and each is decided once
        // for the block rather than left to floating-point luck inside the
        // loop. A ratio of 1:1 demands no reduction at all; a mix of zero is
        // the dry slice *itself* rather than `dry + 0·(wet − dry)`, which
        // would turn −0.0 into +0.0; a mix of one is the wet slice with no
        // blend arithmetic on top of it.
        let compressing = self.slope != 0.0;
        let soft_knee = self.knee_db > 0.0;
        let mix_moving = self.mix.begin(frames);
        let dry_only = !mix_moving && self.mix.current == 0.0;
        let wet_only = !mix_moving && self.mix.current == 1.0;
        let makeup_moving = self.makeup.begin(frames);

        let hpf_on = self.hpf_on();
        let hpf_moving = hpf_on && self.hpf_g.begin(frames);
        if !hpf_on {
            for filter in &mut self.hpf {
                filter.clear();
            }
            self.hpf_g.snap();
        }
        let (mut a1, mut a2, mut a3) = Svf::coefficients(self.hpf_g.current);

        let external = key.filter(|(l, r)| l.len() >= frames && r.len() >= frames);
        let rms = self.sense == Sense::Rms;
        let half_ratio = self.knee_db * 0.5;
        let knee_scale = if soft_knee { 1.0 / (2.0 * self.knee_db) } else { 0.0 };
        let auto = self.auto_release != AutoRelease::Off;

        let mut worst_reduction_db = 0.0f64;

        for i in 0..frames {
            let dry_l = left[i];
            let dry_r = right[i];
            let (key_l, key_r) = match external {
                Some((l, r)) => (f64::from(l[i]), f64::from(r[i])),
                None => (f64::from(dry_l), f64::from(dry_r)),
            };

            // ── Sidechain: filter, rectify, take the dominant channel ──
            //
            // `max` of the two, not their average. SSL again: "the dominant,
            // ie. louder channel, controls the gain reduction of the overall
            // stereo level." An average under-reads a hard-panned transient by
            // up to 6 dB, so a snare panned left would escape the compressor;
            // two independent gains would move the image, which is the thing
            // stereo linking exists to prevent.
            let (key_l, key_r) = if hpf_on {
                if hpf_moving {
                    let g = self.hpf_g.advance();
                    let c = Svf::coefficients(g);
                    a1 = c.0;
                    a2 = c.1;
                    a3 = c.2;
                }
                (
                    self.hpf[0].highpass(key_l, a1, a2, a3),
                    self.hpf[1].highpass(key_r, a1, a2, a3),
                )
            } else {
                (key_l, key_r)
            };

            let x_g = if rms {
                let a = self.a_rms;
                self.ms[0] = a * self.ms[0] + (1.0 - a) * key_l * key_l;
                self.ms[1] = a * self.ms[1] + (1.0 - a) * key_r * key_r;
                if self.ms[0] < STATE_FLOOR {
                    self.ms[0] = 0.0;
                }
                if self.ms[1] < STATE_FLOOR {
                    self.ms[1] = 0.0;
                }
                self.ms[0].max(self.ms[1]).max(MS_FLOOR).ln() * (LN_TO_DB * 0.5)
                    + RMS_CALIBRATION_DB
            } else {
                key_l.abs().max(key_r.abs()).max(PEAK_FLOOR).ln() * LN_TO_DB
            };

            // ── Gain computer, GMR Eq. (4) rearranged for the reduction ──
            let x_l = if !compressing {
                0.0
            } else {
                let over = x_g - self.threshold_db;
                if soft_knee {
                    if 2.0 * over < -self.knee_db {
                        0.0
                    } else if 2.0 * over.abs() <= self.knee_db {
                        let inside = over + half_ratio;
                        -self.slope * inside * inside * knee_scale
                    } else {
                        -self.slope * over
                    }
                } else if over > 0.0 {
                    -self.slope * over
                } else {
                    0.0
                }
            };

            // ── Ballistics ──
            let y_l = if auto {
                // Two branching networks on the same demand, summed half and
                // half. Branching rather than decoupled *here* on purpose:
                // two decoupled networks would need four states and two
                // separate attack compensations for something nobody can
                // hear, and branching produces the intended release time
                // constant directly — so SSL's published 100 ms and 12 s land
                // exactly where the manual says they do.
                self.y_f = if x_l > self.y_f {
                    self.a_att * self.y_f + (1.0 - self.a_att) * x_l
                } else {
                    self.a_rel_fast * self.y_f + (1.0 - self.a_rel_fast) * x_l
                };
                self.y_s = if x_l > self.y_s {
                    self.a_att_slow * self.y_s + (1.0 - self.a_att_slow) * x_l
                } else {
                    self.a_rel_slow * self.y_s + (1.0 - self.a_rel_slow) * x_l
                };
                if self.y_f < DETECTOR_FLOOR_DB {
                    self.y_f = 0.0;
                }
                if self.y_s < DETECTOR_FLOOR_DB {
                    self.y_s = 0.0;
                }
                AUTO_MIX * self.y_f + (1.0 - AUTO_MIX) * self.y_s
            } else {
                self.detector.tick(x_l, self.a_att, self.a_rel)
            };

            if y_l > worst_reduction_db {
                worst_reduction_db = y_l;
            }

            // ── Out ──
            let makeup = if makeup_moving { self.makeup.advance() } else { self.makeup.current };
            let gain = ((makeup - y_l) * DB_TO_LN).exp() as f32;

            if dry_only {
                // Not `dry + 0·(wet − dry)`: the samples are not touched at
                // all, so the null is a property of the control flow.
                continue;
            }
            let wet_l = dry_l * gain;
            let wet_r = dry_r * gain;
            if wet_only {
                left[i] = wet_l;
                right[i] = wet_r;
            } else {
                let mix = if mix_moving { self.mix.advance() } else { self.mix.current } as f32;
                left[i] = dry_l + mix * (wet_l - dry_l);
                right[i] = dry_r + mix * (wet_r - dry_r);
            }
        }

        self.mix.end();
        self.makeup.end();
        self.hpf_g.end();
        self.block_min_gain = (-worst_reduction_db * DB_TO_LN).exp() as f32;
    }

    /// The reduction the static curve asks for at an input level, in decibels
    /// — the closed form of GMR Eq. (4), with no ballistics in front of it.
    ///
    /// Here so the panel can draw the curve and the tests can check the
    /// running compressor against the same arithmetic the paper publishes
    /// rather than against a second copy of it.
    #[must_use]
    pub fn static_reduction_db(&self, input_db: f64) -> f64 {
        static_reduction(input_db, self.threshold_db, self.slope, self.knee_db)
    }

    /// What the static curve puts out for an input level, makeup included.
    #[must_use]
    pub fn static_output_db(&self, input_db: f64) -> f64 {
        input_db - self.static_reduction_db(input_db) + self.makeup_db()
    }
}

impl Default for Compressor {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

/// The reduction the static curve asks for, as a free function.
///
/// ```text
///         ⎧ 0                                    2(x − T) < −W
///   x_L = ⎨ −S · (x − T + W/2)² / (2W)           2|x − T| ≤ W
///         ⎩ −S · (x − T)                         2(x − T) > W
/// ```
///
/// `W = 0` reduces exactly to the hard knee. The middle branch is what makes
/// the curve C¹: at `x = T − W/2` the knee term is zero and the slope is 1; at
/// `x = T + W/2` both branches give `T + W/(2R)` and a slope of `1/R`; and at
/// `x = T` exactly the slope is `1 + S/2`, which for a limiter is **one half**
/// — a 2:1 ratio at the threshold of an infinite-ratio compressor. That is the
/// `auto ratio` character, and it is a closed-form anchor that catches any
/// sign error in the knee.
#[must_use]
pub fn static_reduction(input_db: f64, threshold_db: f64, slope: f64, knee_db: f64) -> f64 {
    if slope == 0.0 {
        return 0.0;
    }
    let over = input_db - threshold_db;
    if knee_db > 0.0 {
        if 2.0 * over < -knee_db {
            0.0
        } else if 2.0 * over.abs() <= knee_db {
            let inside = over + knee_db * 0.5;
            -slope * inside * inside / (2.0 * knee_db)
        } else {
            -slope * over
        }
    } else if over > 0.0 {
        -slope * over
    } else {
        0.0
    }
}

/// The automatic makeup for a threshold and a slope, in decibels.
#[must_use]
pub fn auto_makeup_for(threshold_db: f64, slope_magnitude: f64) -> f64 {
    -threshold_db * slope_magnitude.clamp(0.0, 1.0) * 0.5
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const FS: f64 = 44_100.0;

    // ── Rigs ──

    fn comp() -> Compressor {
        Compressor::new(FS)
    }

    /// A compressor with the controls a measurement wants.
    ///
    /// A 5 ms attack and a 200 ms release: fast enough that a tenth of a
    /// second settles it to twenty time constants, slow enough that the
    /// detector does not ripple at the tone's own period. The static curve is
    /// a *static* thing, so the ballistics here exist only to get out of the
    /// way quickly.
    fn curve_comp(threshold: f64, ratio: f64, knee: f64, sense: Sense) -> Compressor {
        let mut c = comp();
        c.set_param_natural(PARAM_THRESHOLD_DB, threshold as f32);
        c.set_param_natural(PARAM_RATIO, ratio_to_percent(ratio));
        c.set_param_natural(PARAM_KNEE_DB, knee as f32);
        c.set_param_natural(PARAM_ATTACK_MS, 5.0);
        c.set_param_natural(PARAM_RELEASE_MS, 200.0);
        c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
        c.set_param_natural(PARAM_SENSE, sense.index() as f32);
        c.snap();
        c
    }

    /// Render a steady sine at `db` dBFS and answer the output level in dB,
    /// measured over the last 30 ms after 100 of settling.
    fn settled_output_db(c: &mut Compressor, db: f64, hz: f64) -> f64 {
        let amplitude = 10f64.powf(db / 20.0);
        let block = 256usize;
        let settle = (0.100 * FS / block as f64) as usize;
        let measure = (0.030 * FS / block as f64) as usize;
        let mut n = 0usize;
        let mut l = vec![0.0f32; block];
        let mut r = vec![0.0f32; block];
        let mut peak = 0.0f64;
        for pass in 0..(settle + measure) {
            for i in 0..block {
                let s = (amplitude * (TAU * hz * n as f64 / FS).sin()) as f32;
                l[i] = s;
                r[i] = s;
                n += 1;
            }
            c.process(&mut l, &mut r, None);
            if pass >= settle {
                for &s in &l {
                    peak = peak.max(f64::from(s.abs()));
                }
            }
        }
        20.0 * peak.max(1.0e-12).log10()
    }

    fn db_to_gain(db: f64) -> f64 {
        10f64.powf(db / 20.0)
    }

    /// Feed a level in dBFS as a sine for `frames` and answer the gain
    /// reduction envelope, one entry per block.
    fn gr_envelope(c: &mut Compressor, level_db: f64, frames: usize, block: usize) -> Vec<f64> {
        let amplitude = 10f64.powf(level_db / 20.0);
        let mut l = vec![0.0f32; block];
        let mut r = vec![0.0f32; block];
        let mut out = Vec::with_capacity(frames / block + 1);
        let mut n = 0usize;
        while n < frames {
            for i in 0..block {
                let s = (amplitude * (TAU * 1_000.0 * n as f64 / FS).sin()) as f32;
                l[i] = s;
                r[i] = s;
                n += 1;
            }
            c.process(&mut l, &mut r, None);
            out.push(-20.0 * f64::from(c.block_min_gain()).max(1.0e-12).log10());
        }
        out
    }

    // ── V1: the static curve ──

    /// **The static curve is the closed form, below the knee, inside it, and
    /// above it.**
    ///
    /// A settled sine is a static-curve reading, so the output peak is
    /// `x − x_L(x)`. Two sweeps: a coarse one across the whole grid — three
    /// knees, four ratios including infinity, three thresholds, both sense
    /// positions, which is what the +3.01 dB calibration is for — and a fine
    /// one in quarter-decibel steps through the two knees whose shape matters
    /// most, where a formula error would hide between the coarse points.
    #[test]
    fn the_static_curve_is_the_closed_form_below_inside_and_above_the_knee() {
        let mut worst = 0.0f64;
        let mut check = |sense: Sense, knee: f64, ratio: f64, threshold: f64, step: f64| {
            let mut c = curve_comp(threshold, ratio, knee, sense);
            let slope = -f64::from(ratio_to_percent(ratio)) / 100.0;
            let mut level = -80.0f64;
            while level <= 0.0001 {
                let measured = settled_output_db(&mut c, level, 1_000.0);
                let wanted = level - static_reduction(level, threshold, slope, knee);
                let error = (measured - wanted).abs();
                assert!(
                    error < 0.15,
                    "{sense:?} knee {knee} ratio {ratio} T {threshold}: \
                     {level} dB in read {measured:.3} dB, the curve says {wanted:.3}"
                );
                worst = worst.max(error);
                level += step;
            }
        };

        for sense in Sense::ALL {
            for knee in [0.0f64, 6.0, 24.0] {
                for ratio in [1.0f64, 2.0, 4.0, f64::INFINITY] {
                    for threshold in [-30.0f64, -18.0, -6.0] {
                        check(sense, knee, ratio, threshold, 2.0);
                    }
                }
            }
        }
        // Through the knees, a quarter of a decibel at a time. The second one
        // is the `auto ratio` shape, where the curve's slope runs the whole
        // way from 1 to 0 inside 24 dB.
        check(Sense::Peak, 6.0, 4.0, -18.0, 0.25);
        check(Sense::Peak, 24.0, f64::INFINITY, -18.0, 0.25);

        // A fingerprint rather than a bit hash: if this drifts, something in
        // the gain computer or the calibration moved.
        assert!(worst < 0.15, "worst curve error {worst:.4} dB");
    }

    // ── V2/V3: the ballistics ──

    /// Feed a step into the isolated detector and answer the time to reach a
    /// fraction of the final value, in seconds.
    fn time_to_fraction(a_att: f64, a_rel: f64, rising: bool, fraction: f64) -> f64 {
        let mut d = PeakDetector::new();
        if rising {
            let mut n = 0usize;
            while d.tick(1.0, a_att, a_rel) < fraction && n < (60.0 * FS) as usize {
                n += 1;
            }
            n as f64 / FS
        } else {
            // Settle at 1.0 first, then let go.
            for _ in 0..(60.0 * FS) as usize {
                let _ = d.tick(1.0, a_att, a_rel);
                if d.current() > 0.999_999 {
                    break;
                }
            }
            let start = d.current();
            let target = start * fraction;
            let mut n = 0usize;
            while d.tick(0.0, a_att, a_rel) > target && n < (60.0 * FS) as usize {
                n += 1;
            }
            n as f64 / FS
        }
    }

    /// **The attack time is the time constant it says it is.**
    ///
    /// GMR's own definition, on GMR's own rig: a unit step in the demanded
    /// reduction fed straight into the detector, with no audio anywhere near
    /// it. The independent check is the 10→90% rise time, which for a true
    /// one-pole is `τ·ln 9` and cannot be right by accident if the 63.2%
    /// figure was fudged.
    #[test]
    fn the_attack_time_is_the_time_constant_it_says() {
        for attack_ms in [0.1f64, 1.0, 10.0, 100.0] {
            let attack = attack_ms / 1000.0;
            let a_att = one_pole(attack, FS);
            let a_rel = one_pole(1.0, FS);

            // Two percent, or a sample and a half, whichever is larger. The
            // shortest attack the control offers is 4.4 samples at 44.1 kHz,
            // so at that end of the travel the measurement's own resolution
            // is worth more than two percent of it.
            let grain = 1.5 / FS;
            let t63 = time_to_fraction(a_att, a_rel, true, 1.0 - std::f64::consts::E.recip());
            assert!(
                (t63 - attack).abs() <= (attack * 0.02).max(grain),
                "{attack_ms} ms attack reached 63.2% at {:.4} ms",
                t63 * 1000.0
            );

            let t10 = time_to_fraction(a_att, a_rel, true, 0.1);
            let t90 = time_to_fraction(a_att, a_rel, true, 0.9);
            let rise = t90 - t10;
            let wanted = attack * 9f64.ln();
            assert!(
                (rise - wanted).abs() <= (wanted * 0.03).max(2.0 * grain),
                "{attack_ms} ms attack rose 10-90% in {:.4} ms, not {:.4}",
                rise * 1000.0,
                wanted * 1000.0
            );
        }
    }

    /// **The release knob means what it says, because the attack time is
    /// subtracted from it.**
    ///
    /// Both halves, and the second half is the point. The compensated
    /// detector measures the dialled value; the *uncompensated* one — the same
    /// detector with `one_pole(τ_R)` instead of
    /// [`compensated_release`] — measures `τ_A + τ_R`, which is the defect
    /// GMR document and the reason the compensation exists. If somebody ever
    /// deletes the subtraction, the first assertion fails; if somebody
    /// "simplifies" the second assertion away, the evidence for why goes with
    /// it.
    #[test]
    fn the_release_knob_means_what_it_says_because_we_subtract_the_attack() {
        let attack = 0.001f64;
        let a_att = one_pole(attack, FS);
        for release_ms in [5.0f64, 50.0, 500.0, 3_000.0] {
            let release = release_ms / 1000.0;
            let fraction = std::f64::consts::E.recip();

            let compensated = time_to_fraction(
                a_att,
                compensated_release(attack, release, FS),
                false,
                fraction,
            );
            assert!(
                (compensated - release).abs() / release < 0.10,
                "{release_ms} ms release measured {:.3} ms",
                compensated * 1000.0
            );

            // The companion. Without the subtraction the knob reads low by
            // the attack time — at 5 ms release and 1 ms attack that is 22%
            // wrong, and at a drum-bus 30 ms attack it would be worse than
            // double.
            let raw = time_to_fraction(a_att, one_pole(release, FS), false, fraction);
            let uncompensated_target = attack + release;
            assert!(
                (raw - uncompensated_target).abs() / uncompensated_target < 0.10,
                "the uncompensated detector measured {:.3} ms, not the {:.3} ms \
                 that would be tau_A + tau_R",
                raw * 1000.0,
                uncompensated_target * 1000.0
            );
            assert!(
                raw > compensated,
                "the uncompensated release was not slower than the compensated one"
            );
        }
    }

    /// **The release can never be faster than the attack.**
    ///
    /// The `max(…, 1/fs)` end of the compensation. GMR call this desirable and
    /// so does this house: a gain that comes back faster than it went down is
    /// modulating the waveform rather than riding it.
    #[test]
    fn the_release_is_clamped_at_the_attack_time() {
        let attack = 0.050f64;
        let a_att = one_pole(attack, FS);
        let fast = compensated_release(attack, 0.005, FS);
        let slower = compensated_release(attack, 0.030, FS);
        assert_eq!(fast, slower, "two releases under the attack time differ");
        let measured = time_to_fraction(a_att, fast, false, std::f64::consts::E.recip());
        assert!(
            measured >= attack * 0.9,
            "a 5 ms release under a 50 ms attack came back in {:.2} ms",
            measured * 1000.0
        );
    }

    /// **A real tone burst agrees with the isolated detector.**
    ///
    /// Guards against the detector being correct and wired to the wrong
    /// signal. The measured attack has to be monotone in the knob and within
    /// a factor of two of what the isolated rig says.
    #[test]
    fn a_real_tone_burst_agrees_with_the_isolated_detector() {
        let mut previous = 0.0f64;
        for attack_ms in [1.0f64, 10.0, 100.0] {
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, -40.0);
            c.set_param_natural(PARAM_RATIO, ratio_to_percent(4.0));
            c.set_param_natural(PARAM_KNEE_DB, 0.0);
            c.set_param_natural(PARAM_ATTACK_MS, attack_ms as f32);
            c.set_param_natural(PARAM_RELEASE_MS, 1_000.0);
            c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
            c.snap();

            // −40 dBFS for a moment, then −6: the demanded reduction steps.
            let block = 32usize;
            let quiet = gr_envelope(&mut c, -40.0, (0.3 * FS) as usize, block);
            let _ = quiet;
            let loud = gr_envelope(&mut c, -6.0, (1.5 * FS) as usize, block);
            let final_gr = loud.last().copied().unwrap_or(0.0);
            assert!(final_gr > 20.0, "{attack_ms} ms: only {final_gr:.2} dB of reduction");
            let target = final_gr * (1.0 - std::f64::consts::E.recip());
            let blocks = loud.iter().position(|g| *g >= target).unwrap_or(loud.len());
            let measured = blocks as f64 * block as f64 / FS;

            let isolated = attack_ms / 1000.0;
            assert!(
                measured < isolated * 2.0 + 4.0 * block as f64 / FS,
                "{attack_ms} ms attack measured {:.3} ms on a burst",
                measured * 1000.0
            );
            assert!(
                measured >= previous,
                "{attack_ms} ms was not slower than the setting below it"
            );
            previous = measured;
        }
    }

    // ── V5: the knee ──

    /// **The knee is C¹, and an infinite ratio with a wide knee is 2:1 at the
    /// threshold.**
    ///
    /// The second half is GMR's own trick and the reason the `auto ratio`
    /// character ships: with `R = ∞` the slope of the static curve runs from 1
    /// at `T − W/2` to 0 at `T + W/2`, passing through exactly one half — a
    /// 2:1 ratio — at the threshold itself. It is a closed-form anchor that
    /// no sign error in the knee formula can survive.
    #[test]
    fn the_knee_is_c1_and_infinite_ratio_with_a_wide_knee_is_two_to_one_at_threshold() {
        let threshold = -20.0f64;
        let knee = 24.0f64;
        let slope = -1.0f64;
        let step = 1.0e-4;
        let slope_at = |x: f64| {
            let a = x - static_reduction(x, threshold, slope, knee);
            let b = (x + step) - static_reduction(x + step, threshold, slope, knee);
            (b - a) / step
        };

        assert!(
            (slope_at(threshold) - 0.5).abs() < 0.01,
            "the slope at the threshold is {:.4}, not one half",
            slope_at(threshold)
        );

        for edge in [threshold - knee * 0.5, threshold + knee * 0.5] {
            let below = slope_at(edge - 0.05);
            let above = slope_at(edge + 0.05);
            assert!(
                (below - above).abs() < 0.01,
                "the slope jumps from {below:.4} to {above:.4} at the knee edge {edge}"
            );
        }
        assert!((slope_at(threshold - knee) - 1.0).abs() < 1.0e-6, "below the knee is a wire");
        assert!(slope_at(threshold + knee).abs() < 1.0e-6, "above the knee is a limiter");

        // The published reduction at the threshold, `x_L(T) = −S·W/8`.
        for (ratio, width, wanted) in [(3.0f64, 6.0f64, 0.5f64), (f64::INFINITY, 24.0, 3.0)] {
            let slope = -f64::from(ratio_to_percent(ratio)) / 100.0;
            let reduction = static_reduction(threshold, threshold, slope, width);
            assert!(
                (reduction - wanted).abs() < 0.01,
                "{ratio}:1 with a {width} dB knee reduces {reduction:.3} dB at the threshold, \
                 not {wanted}"
            );
        }

        // Zero knee is the hard knee exactly.
        for level in [-40.0f64, -20.0, -19.999, 0.0] {
            let hard = static_reduction(level, threshold, -0.5, 0.0);
            let wanted = if level > threshold { 0.5 * (level - threshold) } else { 0.0 };
            assert!((hard - wanted).abs() < 1.0e-12, "hard knee at {level}");
        }
    }

    // ── V6: the nulls ──

    /// **1:1 and mix 0 are bit-identical, sample for sample.**
    ///
    /// Both are hoisted decisions rather than arithmetic that happens to round
    /// back: a ratio of 1:1 demands no reduction at all, and a mix of zero
    /// does not touch the buffer — which matters, because `−0.0 + 0.0` is
    /// `+0.0` and a signal containing negative zero would not survive a
    /// blend that "should" be a no-op.
    #[test]
    fn ratio_one_to_one_and_mix_zero_are_bit_identical() {
        let source: Vec<f32> = (0..2_048)
            .map(|i| (i as f32 * 0.021).sin() * 0.6 + (i as f32 * 0.37).cos() * 0.3)
            .chain([0.0, -0.0, 1.0, -1.0, f32::MIN_POSITIVE, -f32::MIN_POSITIVE])
            .collect();

        // Ratio 1:1, automatic makeup on — which at 1:1 is zero by the
        // formula, so the whole device is a wire.
        let mut unity = comp();
        unity.set_param_natural(PARAM_RATIO, 0.0);
        unity.snap();
        let mut l = source.clone();
        let mut r = source.clone();
        unity.process(&mut l, &mut r, None);
        for (i, (a, b)) in source.iter().zip(&l).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "1:1 changed sample {i}: {a} -> {b}");
        }
        assert_eq!(r, source);

        // Mix zero, at settings that would otherwise flatten the signal.
        let mut dry = comp();
        dry.set_param_natural(PARAM_MIX, 0.0);
        dry.set_param_natural(PARAM_THRESHOLD_DB, -60.0);
        dry.set_param_natural(PARAM_RATIO, 100.0);
        dry.set_param_natural(PARAM_ATTACK_MS, 0.05);
        dry.snap();
        let mut l = source.clone();
        let mut r = source.clone();
        dry.process(&mut l, &mut r, None);
        for (i, (a, b)) in source.iter().zip(&l).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "mix 0 changed sample {i}: {a} -> {b}");
        }
        assert_eq!(r, source);
        // ...and the detector kept running underneath it, so the knob coming
        // back up does not start from a cold envelope.
        assert!(dry.block_min_gain() < 0.5, "the detector was asleep behind a dry mix");
    }

    // ── V8: the parallel sum ──

    /// **The mix control sums the two paths, and the detector never hears the
    /// sum.**
    ///
    /// `out = dry + m·(wet − dry)` to within a millionth. A compressor that
    /// fed its own detector from the mixed output would fail this at every
    /// setting between the ends, which is exactly the bug this catches.
    #[test]
    fn the_mix_control_sums_the_parallel_paths() {
        let source: Vec<f32> = (0..4_096)
            .map(|i| {
                let t = i as f32 / FS as f32;
                0.5 * (TAU as f32 * 220.0 * t).sin() + 0.25 * (TAU as f32 * 1_700.0 * t).sin()
            })
            .collect();

        let render = |mix: f32| -> Vec<f32> {
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, -24.0);
            c.set_param_natural(PARAM_RATIO, ratio_to_percent(4.0));
            c.set_param_natural(PARAM_MIX, mix);
            c.snap();
            let mut l = source.clone();
            let mut r = source.clone();
            c.process(&mut l, &mut r, None);
            l
        };

        let dry = render(0.0);
        let wet = render(100.0);
        for m in [0.25f32, 0.5, 0.75] {
            let blended = render(m * 100.0);
            for i in 0..source.len() {
                let wanted = dry[i] + m * (wet[i] - dry[i]);
                assert!(
                    (blended[i] - wanted).abs() <= 1.0e-6 * wanted.abs().max(1.0e-3),
                    "mix {m}: sample {i} is {} and the parallel sum is {wanted}",
                    blended[i]
                );
            }
        }
    }

    // ── V9: the automatic release ──

    /// **The automatic release lets go of a transient quickly** — and the
    /// residue it does keep is the whole point of having two networks.
    ///
    /// A 20 ms burst charges the fast network completely and the slow one by
    /// `1 − e^(−0.02/0.1) ≈ 18%`, so 300 ms later about a fifth of the peak
    /// reduction is still there, decaying over twelve seconds. That is a
    /// gentle tail rather than a stuck gain, and it is what "louder elements
    /// are released quickly and quieter elements more slowly" costs.
    ///
    /// The contrast is what makes the number mean something: the same burst
    /// with the automatic switched off and a three-second release still has
    /// nine tenths of its reduction 300 ms later.
    #[test]
    fn auto_release_lets_go_of_a_transient_quickly() {
        let block = 32usize;
        let burst_then_silence = |c: &mut Compressor| -> (f64, f64) {
            let burst = gr_envelope(c, -6.0, (0.020 * FS) as usize, block);
            let peak = burst.iter().copied().fold(0.0f64, f64::max);
            let mut left = 0.0;
            let mut l = vec![0.0f32; block];
            let mut r = vec![0.0f32; block];
            for _ in 0..(0.300 * FS / block as f64) as usize {
                l.fill(0.0);
                r.fill(0.0);
                c.process(&mut l, &mut r, None);
                left = -20.0 * f64::from(c.block_min_gain()).max(1.0e-12).log10();
            }
            (peak, left)
        };
        let rig = |arel: AutoRelease, release_ms: f32| -> Compressor {
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, -30.0);
            c.set_param_natural(PARAM_RATIO, ratio_to_percent(4.0));
            c.set_param_natural(PARAM_ATTACK_MS, 1.0);
            c.set_param_natural(PARAM_RELEASE_MS, release_ms);
            c.set_param_natural(PARAM_AUTO_RELEASE, arel.index() as f32);
            c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
            c.snap();
            c
        };

        let (slow_peak, slow_left) = burst_then_silence(&mut rig(AutoRelease::Off, 3_000.0));
        assert!(
            slow_left > slow_peak * 0.8,
            "a three-second release let go of a transient in 300 ms: {slow_left:.3} of \
             {slow_peak:.3} dB"
        );

        for mode in [AutoRelease::Auto, AutoRelease::Auto2] {
            let (peak, left) = burst_then_silence(&mut rig(mode, 3_000.0));
            assert!(peak > 4.0, "{mode:?}: the burst only asked for {peak:.2} dB");
            assert!(
                left < peak * 0.25,
                "{mode:?}: 300 ms after a 20 ms burst there was still {left:.3} dB of the \
                 {peak:.3} dB left"
            );
            // In absolute terms: about two decibels out of eleven, which is
            // the slow network's eighteen percent share arriving as a tail.
            assert!(
                left < 2.5,
                "{mode:?}: {left:.3} dB of a transient's reduction was still audible"
            );
        }
    }

    /// **...and holds on after sustained compression.**
    ///
    /// The other half of the SSL sentence: "louder elements of the signal are
    /// released quickly and quieter elements more slowly". A two-second
    /// passage charges the slow network fully, so half the reduction is still
    /// there a second after the tone stops — and `auto 2`, whose slow network
    /// is six seconds rather than twelve, has measurably less of it.
    #[test]
    fn auto_release_holds_on_after_sustained_compression() {
        let mut remaining = Vec::new();
        for mode in [AutoRelease::Auto, AutoRelease::Auto2] {
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, -30.0);
            c.set_param_natural(PARAM_RATIO, ratio_to_percent(4.0));
            c.set_param_natural(PARAM_ATTACK_MS, 1.0);
            c.set_param_natural(PARAM_AUTO_RELEASE, mode.index() as f32);
            c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
            c.snap();

            let block = 64usize;
            let held = gr_envelope(&mut c, -6.0, (2.0 * FS) as usize, block);
            let peak = held.last().copied().unwrap_or(0.0);
            assert!(peak > 4.0, "{mode:?}: only {peak:.2} dB after two seconds");

            let mut after = 0.0;
            let mut l = vec![0.0f32; block];
            let mut r = vec![0.0f32; block];
            for _ in 0..(1.0 * FS / block as f64) as usize {
                l.fill(0.0);
                r.fill(0.0);
                c.process(&mut l, &mut r, None);
                after = -20.0 * f64::from(c.block_min_gain()).max(1.0e-12).log10();
            }
            assert!(
                after > peak * 0.30,
                "{mode:?}: a second after two seconds of compression only {after:.3} dB of \
                 {peak:.3} dB was left"
            );
            remaining.push(after / peak);
        }
        assert!(
            remaining[1] < remaining[0],
            "auto 2 did not recover faster than auto: {:.3} against {:.3}",
            remaining[1],
            remaining[0]
        );
    }

    // ── V10: the sidechain high-pass ──

    /// **The sidechain high-pass removes the bass it promises to.**
    ///
    /// Two poles at 200 Hz put a 50 Hz fundamental 24 dB down, which is the
    /// difference between a mix that pumps in time with the kick and one that
    /// does not. Measured against the reference the filter is supposed to
    /// produce: the same compressor keyed on the 1 kHz component alone.
    #[test]
    fn the_sidechain_highpass_removes_the_bass_it_promises_to() {
        let frames = (3.0 * FS) as usize;
        let block = 256usize;

        let steady_gr = |hpf: f32, with_bass: bool| -> f64 {
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, -30.0);
            c.set_param_natural(PARAM_RATIO, ratio_to_percent(4.0));
            c.set_param_natural(PARAM_KNEE_DB, 0.0);
            c.set_param_natural(PARAM_ATTACK_MS, 20.0);
            c.set_param_natural(PARAM_RELEASE_MS, 500.0);
            c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
            c.set_param_natural(PARAM_SC_HPF_HZ, hpf);
            c.snap();

            let mut l = vec![0.0f32; block];
            let mut r = vec![0.0f32; block];
            let mut n = 0usize;
            let mut last = 0.0;
            while n < frames {
                for i in 0..block {
                    let t = n as f64 / FS;
                    let bass = if with_bass { 0.2 * (TAU * 50.0 * t).sin() } else { 0.0 };
                    let top = 0.2 * (TAU * 1_000.0 * t).sin();
                    l[i] = (bass + top) as f32;
                    r[i] = l[i];
                    n += 1;
                }
                c.process(&mut l, &mut r, None);
                last = -20.0 * f64::from(c.block_min_gain()).max(1.0e-12).log10();
            }
            last
        };

        let reference = steady_gr(0.0, false);
        let filtered = steady_gr(200.0, true);
        let unfiltered = steady_gr(0.0, true);

        assert!(
            (filtered - reference).abs() < 0.5,
            "with the filter at 200 Hz the bass still moved the detector: {filtered:.2} dB \
             against the {reference:.2} dB the top alone asks for"
        );
        assert!(
            (unfiltered - reference) > 3.0,
            "the bass was not making a difference to begin with: {unfiltered:.2} against \
             {reference:.2}"
        );
    }

    /// The high-pass is second order: 12 dB per octave, measured on its own
    /// magnitude response.
    #[test]
    fn the_sidechain_highpass_is_twelve_db_per_octave() {
        let corner = 200.0f64;
        let g = Svf::g_for(corner, FS);
        let (a1, a2, a3) = Svf::coefficients(g);
        let amplitude_at = |hz: f64| -> f64 {
            let mut filter = Svf::default();
            let cycles = 200.0;
            let n = ((cycles * FS / hz) as usize).max(4_096);
            let mut peak = 0.0f64;
            for i in 0..n {
                let x = (TAU * hz * i as f64 / FS).sin();
                let y = filter.highpass(x, a1, a2, a3);
                if i > n / 2 {
                    peak = peak.max(y.abs());
                }
            }
            20.0 * peak.max(1.0e-12).log10()
        };
        // A pole pair at the corner is 3 dB down there.
        assert!(
            (amplitude_at(corner) + 3.01).abs() < 0.6,
            "the corner reads {:.2} dB",
            amplitude_at(corner)
        );
        // Two octaves below, the slope is 12 dB per octave: 50 Hz is 24 dB
        // under 200, and 100 Hz is 12 under it.
        let one = amplitude_at(100.0);
        let two = amplitude_at(50.0);
        assert!((one - two - 12.0).abs() < 1.0, "the slope is {:.2} dB/oct", one - two);
        assert!(two < -20.0, "50 Hz is only {two:.2} dB down at a 200 Hz corner");
    }

    // ── V11: the meter ──

    /// **The block minimum is the worst moment in the block, whatever the
    /// block size.**
    ///
    /// One transient inside one callback has to register: a compressor that
    /// published the *last* sample's gain, or an average, would show nothing
    /// at all on the only events anybody watches a gain-reduction meter for.
    #[test]
    fn the_block_minimum_is_the_worst_moment_in_the_block() {
        for block in [32usize, 64, 256] {
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, -40.0);
            c.set_param_natural(PARAM_RATIO, 100.0);
            c.set_param_natural(PARAM_ATTACK_MS, 0.05);
            c.set_param_natural(PARAM_RELEASE_MS, 3_000.0);
            c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
            c.snap();

            let mut l = vec![0.0f32; block];
            let mut r = vec![0.0f32; block];
            // One full-scale sample at the very start of the block, silence
            // after it. The gain is back near unity long before the block
            // ends, so only a block *minimum* can see it.
            l[0] = 1.0;
            r[0] = 1.0;
            c.process(&mut l, &mut r, None);
            let gr = -20.0 * f64::from(c.block_min_gain()).log10();
            assert!(
                gr > 20.0,
                "block {block}: a full-scale transient registered only {gr:.2} dB"
            );
        }
    }

    /// A steady reduction reads the steady value.
    #[test]
    fn the_meter_reads_the_steady_reduction() {
        let mut c = comp();
        c.set_param_natural(PARAM_THRESHOLD_DB, -30.0);
        c.set_param_natural(PARAM_RATIO, ratio_to_percent(2.0));
        c.set_param_natural(PARAM_KNEE_DB, 0.0);
        c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
        c.snap();
        // −18 dBFS against a −30 dB threshold at 2:1 is 6 dB of reduction.
        let envelope = gr_envelope(&mut c, -18.0, (2.0 * FS) as usize, 64);
        let settled = envelope.last().copied().unwrap_or(0.0);
        assert!(
            (settled - 6.0).abs() < 0.1,
            "a 6 dB reduction read {settled:.3} dB"
        );
    }

    // ── V12: denormals ──

    /// **Nothing denormals in a long silence.**
    ///
    /// Deterministic rather than timed: after enough silence every state in
    /// the device is *exactly* zero, so there is nothing left to decay into
    /// the subnormal range and cost a microcoded fault per sample forever, and
    /// silence in gives silence out rather than an asymptote.
    ///
    /// How long "enough" is depends on the slowest thing in the path, which is
    /// the automatic mode's twelve-second network: from 40 dB of reduction
    /// down to the `1e−10 dB` flush is `ln(4e11) ≈ 27` time constants. The cap
    /// below is that number, and the assertion is that the states got there
    /// inside it — which is what pins the flush.
    #[test]
    fn nothing_denormals_in_a_long_silence() {
        // A lower rate than the device runs at, for the test's own sake: what
        // is under test is a count of time constants, not of samples.
        const RATE: f64 = 22_050.0;
        let block = 512usize;

        // Three release modes on the peak front end, and the RMS front end
        // once — the mean-square state is the only thing the sense switch
        // adds, and it decays in ten milliseconds whatever the mode.
        let cases = [
            (Sense::Peak, AutoRelease::Off),
            (Sense::Peak, AutoRelease::Auto),
            (Sense::Peak, AutoRelease::Auto2),
            (Sense::Rms, AutoRelease::Off),
        ];
        for (sense, arel) in cases {
            {
                let mut c = Compressor::new(RATE);
                c.set_param_natural(PARAM_THRESHOLD_DB, -40.0);
                c.set_param_natural(PARAM_RATIO, 100.0);
                c.set_param_natural(PARAM_RELEASE_MS, 3_000.0);
                c.set_param_natural(PARAM_SENSE, sense.index() as f32);
                c.set_param_natural(PARAM_AUTO_RELEASE, arel.index() as f32);
                c.set_param_natural(PARAM_SC_HPF_HZ, 100.0);
                c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
                c.snap();

                let mut l = vec![0.0f32; block];
                let mut r = vec![0.0f32; block];
                let mut n = 0usize;
                for _ in 0..(2.0 * RATE / block as f64) as usize {
                    for i in 0..block {
                        l[i] = 0.8 * (TAU * 220.0 * n as f64 / RATE).sin() as f32;
                        r[i] = l[i];
                        n += 1;
                    }
                    c.process(&mut l, &mut r, None);
                }

                let slowest = match arel {
                    AutoRelease::Off => 3.0f64,
                    AutoRelease::Auto => AUTO_RELEASE_SLOW_S,
                    AutoRelease::Auto2 => AUTO2_RELEASE_SLOW_S,
                };
                let cap = 27.0 * slowest;
                let mut l = vec![0.0f32; block];
                let mut r = vec![0.0f32; block];
                for _ in 0..(cap * RATE / block as f64) as usize {
                    l.fill(0.0);
                    r.fill(0.0);
                    c.process(&mut l, &mut r, None);
                }

                let where_am_i = format!("{sense:?}/{arel:?}");
                let mut l = vec![0.0f32; block];
                let mut r = vec![0.0f32; block];
                c.process(&mut l, &mut r, None);
                assert!(l.iter().all(|s| *s == 0.0), "{where_am_i}: silence in, signal out");
                assert_eq!(c.detector.y1, 0.0, "{where_am_i}: the decoupled stage");
                assert_eq!(c.detector.y_l, 0.0, "{where_am_i}: the detector");
                assert_eq!(c.y_f, 0.0, "{where_am_i}: the fast network");
                assert_eq!(c.y_s, 0.0, "{where_am_i}: the slow network");
                assert_eq!(c.ms, [0.0, 0.0], "{where_am_i}: the mean square");
                assert_eq!(c.hpf[0].ic1, 0.0, "{where_am_i}: the filter");
                assert_eq!(c.hpf[0].ic2, 0.0);
                assert_eq!(c.hpf[1].ic1, 0.0);
                assert_eq!(c.hpf[1].ic2, 0.0);
                assert_eq!(c.block_min_gain(), 1.0, "{where_am_i}: the meter never came home");
            }
        }
    }

    // ── V13: peak against RMS ──

    /// **RMS sensing reads lower by the crest factor.**
    ///
    /// The whole of what the switch is for, stated as a number rather than an
    /// adjective. Two signals matched at the *peak*: a sine, whose crest is
    /// 3.01 dB, and a one-in-twenty pulse train, whose crest is
    /// `10·log₁₀(20) = 13.01 dB`. Exactly ten decibels apart.
    ///
    /// * In the peak position they get the same reduction, because the
    ///   rectifier reads the transient and the two transients are identical.
    /// * In the RMS position the pulse train reads ten decibels lower, so at
    ///   2:1 it gets five decibels less reduction.
    ///
    /// The third assertion is the calibration: a sine reads the same in both
    /// positions, which is what the `+3.01 dB` is for and what keeps every
    /// character preset working on either side of the switch.
    #[test]
    fn rms_sensing_reads_lower_by_the_crest_factor() {
        let block = 256usize;
        let frames = (4.0 * FS) as usize;
        /// One sample in twenty: `10·log₁₀(20) = 13.01 dB` of crest, which is
        /// ten decibels more than a sine's.
        const DUTY: usize = 20;
        const CREST_OVER_A_SINE_DB: f64 = 10.0;

        let measure = |sense: Sense, pulsed: bool| -> f64 {
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, -30.0);
            c.set_param_natural(PARAM_RATIO, ratio_to_percent(2.0));
            c.set_param_natural(PARAM_KNEE_DB, 0.0);
            c.set_param_natural(PARAM_ATTACK_MS, 5.0);
            c.set_param_natural(PARAM_RELEASE_MS, 2_000.0);
            c.set_param_natural(PARAM_SENSE, sense.index() as f32);
            c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
            c.snap();
            let mut l = vec![0.0f32; block];
            let mut r = vec![0.0f32; block];
            let mut n = 0usize;
            let mut worst = 0.0f64;
            while n < frames {
                for i in 0..block {
                    l[i] = if pulsed {
                        if n % DUTY == 0 { 0.5 } else { 0.0 }
                    } else {
                        0.5 * (TAU * 1_000.0 * n as f64 / FS).sin() as f32
                    };
                    r[i] = l[i];
                    n += 1;
                }
                c.process(&mut l, &mut r, None);
                worst = -20.0 * f64::from(c.block_min_gain()).max(1.0e-12).log10();
            }
            worst
        };

        let peak_sine = measure(Sense::Peak, false);
        let peak_pulsed = measure(Sense::Peak, true);
        assert!(peak_sine > 6.0, "the rig was not compressing: {peak_sine:.2} dB");
        assert!(
            (peak_sine - peak_pulsed).abs() < 0.3,
            "the peak position told the two apart: {peak_sine:.2} against {peak_pulsed:.2}"
        );

        let rms_sine = measure(Sense::Rms, false);
        let rms_pulsed = measure(Sense::Rms, true);
        // At 2:1 a ten-decibel drop in what the detector reads is five
        // decibels less reduction.
        let lost = rms_sine - rms_pulsed;
        assert!(
            (lost - CREST_OVER_A_SINE_DB * 0.5).abs() < 1.0,
            "RMS lost {lost:.2} dB of reduction to a crest ten decibels deeper, not \
             {:.2} dB",
            CREST_OVER_A_SINE_DB * 0.5
        );
        assert!(
            (rms_sine - peak_sine).abs() < 0.3,
            "the two positions disagree on a sine: {rms_sine:.2} against {peak_sine:.2} \
             \u{2014} the +3.01 dB calibration is wrong"
        );
    }

    /// **In the RMS position the attack cannot outrun the window.**
    ///
    /// A feature, not a limitation, and the reason an RMS drum smasher does
    /// not exist. What is measured is the time to reach nine tenths of the
    /// settled reduction after a step from −30 to −6 dBFS — not the 63% mark,
    /// because the logarithm of a rising exponential crosses that very early
    /// and would flatter the front end.
    ///
    /// Measured at 44.1 kHz: in the RMS position every attack setting from
    /// 0.05 ms to 10 ms takes more than 5 ms to get there, because the 10 ms
    /// mean-square window is in front of all of them. In the peak position the
    /// same measurement at 0.05 ms is under a millisecond.
    #[test]
    fn rms_sensing_cannot_attack_faster_than_its_window() {
        let block = 8usize;
        // 5 kHz, so the rectifier sees a peak every tenth of a millisecond and
        // the peak position is not measuring the tone's own period.
        let hz = 5_000.0f64;

        let time_to_ninety = |sense: Sense, attack_ms: f32| -> f64 {
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, -40.0);
            c.set_param_natural(PARAM_RATIO, ratio_to_percent(4.0));
            c.set_param_natural(PARAM_KNEE_DB, 0.0);
            c.set_param_natural(PARAM_ATTACK_MS, attack_ms);
            c.set_param_natural(PARAM_RELEASE_MS, 2_000.0);
            c.set_param_natural(PARAM_SENSE, sense.index() as f32);
            c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
            c.snap();

            let mut l = vec![0.0f32; block];
            let mut r = vec![0.0f32; block];
            let mut n = 0usize;
            let mut render = |c: &mut Compressor, level_db: f64, seconds: f64| -> Vec<f64> {
                let amplitude = db_to_gain(level_db);
                let mut out = Vec::new();
                for _ in 0..(seconds * FS / block as f64) as usize {
                    for i in 0..block {
                        l[i] = (amplitude * (TAU * hz * n as f64 / FS).sin()) as f32;
                        r[i] = l[i];
                        n += 1;
                    }
                    c.process(&mut l, &mut r, None);
                    out.push(-20.0 * f64::from(c.block_min_gain()).max(1.0e-12).log10());
                }
                out
            };

            // Settle at the quiet level, then step. An attack time is the
            // response to a step, not to being switched on.
            let quiet = render(&mut c, -30.0, 1.0);
            let from = quiet.last().copied().unwrap_or(0.0);
            let loud = render(&mut c, -6.0, 0.5);
            let to = loud.last().copied().unwrap_or(0.0);
            let target = from + 0.9 * (to - from);
            let blocks = loud.iter().position(|g| *g >= target).unwrap_or(loud.len());
            blocks as f64 * block as f64 / FS
        };

        for attack_ms in [0.05f32, 1.0, 10.0] {
            let measured = time_to_ninety(Sense::Rms, attack_ms);
            assert!(
                measured > 0.005,
                "an RMS detector with a {attack_ms} ms attack reached 90% in {:.2} ms, \
                 which is faster than its own window",
                measured * 1000.0
            );
        }
        let fast_peak = time_to_ninety(Sense::Peak, 0.05);
        assert!(
            fast_peak < 0.001,
            "the peak position took {:.3} ms to reach 90% at a 0.05 ms attack, so the \
             contrast above is not the window's doing",
            fast_peak * 1000.0
        );
    }

    // ── V14: overshoot ──

    /// **Transient overshoot is measured, not assumed.**
    ///
    /// The documented cost of having no lookahead, and it is not a small one.
    /// At ∞:1 the first `τ_A` of a step goes past with the gain still on its
    /// way down, and *the very first sample goes past at unity* because the
    /// gain that sample gets is computed from that same sample. There is no
    /// arrangement of a zero-latency feed-forward compressor in which that is
    /// not true; only a lookahead buffer can catch it, and a lookahead buffer
    /// is a latency this mixer cannot compensate for.
    ///
    /// Measured at 44.1 kHz on a −6 dBFS step against a −30 dB threshold, so
    /// the whole step is 24 dB:
    ///
    /// | attack | peak overshoot above the static curve |
    /// |---|---|
    /// | 0.05 ms | 15.25 dB |
    /// | 1 ms | 23.46 dB |
    /// | 10 ms | 23.95 dB |
    /// | 100 ms | 23.99 dB |
    ///
    /// It saturates at the height of the step, and that is the honest reading:
    /// past about a millisecond of attack, a step's first sample arrives
    /// completely uncompressed. The master safety limiter is what stands
    /// between that and the converter, and this is the table that says why it
    /// has to stay there.
    #[test]
    fn transient_overshoot_is_measured_not_assumed() {
        let mut previous = -1.0f64;
        for attack_ms in [0.05f32, 1.0, 10.0, 100.0] {
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, -30.0);
            c.set_param_natural(PARAM_RATIO, 100.0);
            c.set_param_natural(PARAM_KNEE_DB, 0.0);
            c.set_param_natural(PARAM_ATTACK_MS, attack_ms);
            c.set_param_natural(PARAM_RELEASE_MS, 1_000.0);
            c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
            c.snap();

            let block = 64usize;
            let mut l = vec![0.0f32; block];
            let mut r = vec![0.0f32; block];
            let amplitude = db_to_gain(-6.0) as f32;
            let mut peak = 0.0f64;
            for _ in 0..((0.5 * FS) as usize / block) {
                l.fill(amplitude);
                r.fill(amplitude);
                c.process(&mut l, &mut r, None);
                for &s in &l {
                    peak = peak.max(f64::from(s.abs()));
                }
            }
            let overshoot = 20.0 * peak.log10() - (-30.0);
            assert!(
                overshoot > previous,
                "{attack_ms} ms overshot {overshoot:.2} dB, not more than the setting below it"
            );
            assert!(
                overshoot < 24.05,
                "{attack_ms} ms overshot {overshoot:.2} dB, which is more than the whole step"
            );
            previous = overshoot;
        }
        // ...and the fastest setting is the one that actually buys anything:
        // 0.05 ms takes nine decibels off the first sample, and every setting
        // above a millisecond takes almost none.
        assert!(previous > 23.0, "the slow settings did not saturate: {previous:.2} dB");
    }

    // ── V15: zipper ──

    /// **No parameter sweep zippers.**
    ///
    /// Every control swept end to end over a second, with the control moving
    /// once per 256-frame block — which is what a hand on a knob and a
    /// 5.8 ms callback actually produce — and compared against *the same
    /// sweep moving once per sample*, which is the ideal a block-based host
    /// cannot reach. If the block boundary contributed anything, the two would
    /// differ.
    ///
    /// What is measured is the worst sample-to-sample jump in the output,
    /// because that is what a click *is*. The tone is 60 Hz so that the
    /// signal's own slope is small and a gain step has room to stand out: a
    /// four-percent step in gain would be five times the sine's own
    /// sample-to-sample motion, and would fail this by a mile.
    ///
    /// The threshold, the ratio and the knee cannot zipper by construction —
    /// they move the gain computer, and the detector's attack filter is
    /// between them and the output. The makeup and the mix can, and do not,
    /// because they are ramped across the block. All five are asserted, so an
    /// edit that moves one of them past the detector or drops a ramp is
    /// caught.
    #[test]
    fn no_zipper_on_any_parameter_sweep() {
        let worst_jump = |render: &[f32]| -> f64 {
            let mut worst = 0.0f64;
            for pair in render.windows(2) {
                worst = worst.max(f64::from(pair[1] - pair[0]).abs());
            }
            worst
        };

        let sweep = |param: usize, from: f32, to: f32, block: usize| -> Vec<f32> {
            let blocks = (FS / block as f64) as usize;
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, -24.0);
            c.set_param_natural(PARAM_RATIO, ratio_to_percent(4.0));
            c.set_param_natural(PARAM_ATTACK_MS, 50.0);
            c.set_param_natural(PARAM_RELEASE_MS, 500.0);
            c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
            c.set_param_natural(param, from);
            c.snap();
            let mut out = Vec::with_capacity(block * blocks);
            let mut l = vec![0.0f32; block];
            let mut r = vec![0.0f32; block];
            let mut n = 0usize;
            for step in 0..blocks {
                let t = step as f32 / (blocks - 1) as f32;
                c.set_param_natural(param, from + (to - from) * t);
                for i in 0..block {
                    l[i] = 0.4 * (TAU * 60.0 * n as f64 / FS).sin() as f32;
                    r[i] = l[i];
                    n += 1;
                }
                c.process(&mut l, &mut r, None);
                out.extend_from_slice(&l);
            }
            out
        };

        for (param, from, to, name) in [
            (PARAM_THRESHOLD_DB, -60.0f32, 0.0f32, "thresh"),
            (PARAM_RATIO, 0.0, 100.0, "ratio"),
            (PARAM_KNEE_DB, 0.0, 24.0, "knee"),
            (PARAM_MAKEUP_DB, -30.0, 30.0, "makeup"),
            (PARAM_MIX, 0.0, 100.0, "mix"),
        ] {
            let blocked = worst_jump(&sweep(param, from, to, 256));
            let ideal = worst_jump(&sweep(param, from, to, 1));
            assert!(
                blocked <= ideal * 1.15,
                "sweeping {name} once per block jumped {blocked:.6}, against the \
                 {ideal:.6} the same sweep makes moving every sample"
            );
        }
    }

    /// **The negative control for the two ramps.**
    ///
    /// Makeup and mix are the only controls downstream of the detector, so
    /// they are the only two that *would* zipper without a ramp — the other
    /// three are smoothed by the detector itself, which is the whole reason
    /// the detector sits where it does.
    ///
    /// The makeup alternates between 0 and −6 dB every block — no net level
    /// change, so the two renders are level-matched and only the
    /// discontinuity is under test. Ramped, it is a triangle glide; stepped,
    /// it is a square wave of gain, and the worst sample-to-sample jump says
    /// so by a factor of several.
    #[test]
    fn the_makeup_and_mix_ramps_are_load_bearing() {
        let block = 256usize;
        let blocks = 64usize;
        let worst_jump = |render: &[f32]| -> f64 {
            let mut worst = 0.0f64;
            for pair in render.windows(2) {
                worst = worst.max(f64::from(pair[1] - pair[0]).abs());
            }
            worst
        };
        let makeup_for = |step: usize| -> f32 {
            if step % 2 == 0 { 0.0 } else { -6.0 }
        };

        // The stepped version: the same travel, applied as a plain multiply
        // at the block boundary rather than through the ramp.
        let mut stepped = Vec::with_capacity(block * blocks);
        let mut n = 0usize;
        for step in 0..blocks {
            let gain = db_to_gain(f64::from(makeup_for(step))) as f32;
            for _ in 0..block {
                stepped.push(0.4 * (TAU * 60.0 * n as f64 / FS).sin() as f32 * gain);
                n += 1;
            }
        }

        // The same travel through the compressor's ramp, at 1:1 so that
        // nothing but the makeup is moving.
        let mut smooth = comp();
        smooth.set_param_natural(PARAM_RATIO, 0.0);
        smooth.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
        smooth.snap();
        let mut ramped = Vec::with_capacity(block * blocks);
        let mut l = vec![0.0f32; block];
        let mut r = vec![0.0f32; block];
        let mut n = 0usize;
        for step in 0..blocks {
            smooth.set_param_natural(PARAM_MAKEUP_DB, makeup_for(step));
            for i in 0..block {
                l[i] = 0.4 * (TAU * 60.0 * n as f64 / FS).sin() as f32;
                r[i] = l[i];
                n += 1;
            }
            smooth.process(&mut l, &mut r, None);
            ramped.extend_from_slice(&l);
        }

        let stepped_jump = worst_jump(&stepped);
        let ramped_jump = worst_jump(&ramped);
        assert!(
            stepped_jump > ramped_jump * 3.0,
            "a stepped makeup jumped {stepped_jump:.6} against the ramp's {ramped_jump:.6}, \
             which is not enough of a difference for the ramp to be what is keeping the \
             sweep clean"
        );
    }

    // ── V16: the effective ratio ──

    /// **The effective compression ratio tracks the setting.**
    ///
    /// GMR's own objective measure, their Eq. (24): an amplitude-modulated
    /// carrier in, and what the compressor did to the *sidebands* out. At a
    /// slow modulation the compressor tracks the envelope and the measured
    /// ratio is the one that was dialled; as the modulation gets faster than
    /// the ballistics it stops tracking and the measured ratio walks back
    /// towards 1. Both halves are asserted, and the numbers are the
    /// fingerprint.
    ///
    /// **One deliberate departure from the paper's wording.** The text says
    /// the effective ratio "is then given by `ΔS_i/ΔS_o`", where `ΔS` is the
    /// sideband-to-carrier difference in decibels. That quotient cannot
    /// produce the 7:1 their own Fig. 10 converges on: for a modulation index
    /// `m` the sidebands sit at `m/2` of the carrier, a compressor of ratio
    /// `R` divides `m` by `R`, and so `ΔS_o = ΔS_i − 20 log₁₀ R`. What
    /// recovers `R` is the *difference* of the two, converted back:
    ///
    /// ```text
    /// R_eff = 10^((ΔS_i − ΔS_o) / 20)
    /// ```
    ///
    /// which is what is measured here, and which does land on the setting.
    #[test]
    fn the_effective_compression_ratio_tracks_the_setting() {
        let carrier = 1_000.0f64;
        let set_ratio = 4.0f64;

        let r_eff = |f_mod: f64, depth: f64| -> f64 {
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, -40.0);
            c.set_param_natural(PARAM_RATIO, ratio_to_percent(set_ratio));
            c.set_param_natural(PARAM_KNEE_DB, 0.0);
            c.set_param_natural(PARAM_ATTACK_MS, 1.0);
            c.set_param_natural(PARAM_RELEASE_MS, 20.0);
            c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
            c.snap();

            // **The record length is chosen, not taken.** 88 200 samples at
            // 44.1 kHz puts 1 kHz on bin 2000 and every modulation frequency
            // used here on a whole number of bins too, so a plain rectangular
            // window leaks *nothing* from the carrier into the sidebands. It
            // matters: at half a hertz the sidebands are seven bins from a
            // carrier fifty decibels louder, and the leakage of an unaligned
            // record would be all anyone measured.
            const ALIGNED: usize = 88_200;
            let records = ((1.0 / f_mod).ceil() as usize).max(1);
            let frames = ALIGNED * records;
            let settle = (0.5 * FS) as usize;
            let block = 256usize;

            let mut input = Vec::with_capacity(frames);
            let mut output = Vec::with_capacity(frames);
            let mut l = vec![0.0f32; block];
            let mut r = vec![0.0f32; block];
            let mut n = 0usize;
            while n < settle + frames {
                for i in 0..block {
                    let t = n as f64 / FS;
                    let s = (1.0 + depth * (TAU * f_mod * t).cos()) * (TAU * carrier * t).cos();
                    l[i] = (0.3 * s) as f32;
                    r[i] = l[i];
                    if n >= settle && input.len() < frames {
                        input.push(0.3 * s);
                    }
                    n += 1;
                }
                let before = n - block;
                c.process(&mut l, &mut r, None);
                for (i, &s) in l.iter().enumerate() {
                    if before + i >= settle && output.len() < frames {
                        output.push(f64::from(s));
                    }
                }
            }

            let bin = |x: &[f64], hz: f64| -> f64 {
                let mut re = 0.0f64;
                let mut im = 0.0f64;
                for (i, &s) in x.iter().enumerate() {
                    let phase = TAU * hz * i as f64 / FS;
                    re += s * phase.cos();
                    im -= s * phase.sin();
                }
                (re * re + im * im).sqrt() / x.len() as f64
            };
            let delta = |x: &[f64]| -> f64 {
                let c0 = bin(x, carrier);
                let side = bin(x, carrier + f_mod);
                20.0 * (side / c0.max(1.0e-18)).max(1.0e-18).log10()
            };
            10f64.powf((delta(&input) - delta(&output)) / 20.0)
        };

        // The sideband model is first order in the modulation index, so the
        // shallow modulation is where it is exact. Half a hertz is far slower
        // than the ballistics, so the compressor is tracking the envelope and
        // what it measures is the static curve's own slope.
        let shallow = r_eff(0.5, 0.1);
        assert!(
            (shallow - set_ratio).abs() / set_ratio < 0.10,
            "at half a hertz and a shallow modulation a {set_ratio}:1 compressor measured \
             {shallow:.3}:1"
        );

        // At the paper's own `m = 0.5` the envelope swings 9.5 dB peak to
        // trough, which is more than the first-order sideband model strictly
        // covers and more than a 1 ms attack tracks perfectly at the corners.
        // It still lands inside ten percent.
        let deep = r_eff(0.5, 0.5);
        assert!(
            (deep - set_ratio).abs() / set_ratio < 0.10,
            "at a deep modulation a {set_ratio}:1 compressor measured {deep:.3}:1"
        );

        // As the modulation outruns the ballistics the compressor stops
        // tracking it, and the effective ratio walks back towards unity.
        let fast = r_eff(32.0, 0.5);
        assert!(
            fast < deep,
            "a faster modulation did not degrade the effective ratio: {fast:.3} against {deep:.3}"
        );
        assert!(fast > 1.0, "the effective ratio fell below unity: {fast:.3}");
    }

    // ── V17: the rate ──

    /// **The same settings sound the same at every sample rate.**
    ///
    /// The static curve, the attack and release times, and the automatic
    /// makeup, at four rates. Everything in the device is derived from a
    /// *time* rather than from a sample count, and this is what proves it.
    #[test]
    fn the_same_settings_sound_the_same_at_every_sample_rate() {
        for rate in [22_050.0f64, 44_100.0, 48_000.0, 96_000.0] {
            let mut c = Compressor::new(rate);
            c.set_param_natural(PARAM_THRESHOLD_DB, -24.0);
            c.set_param_natural(PARAM_RATIO, ratio_to_percent(4.0));
            c.set_param_natural(PARAM_KNEE_DB, 6.0);
            c.set_param_natural(PARAM_ATTACK_MS, 10.0);
            c.set_param_natural(PARAM_RELEASE_MS, 200.0);
            c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
            c.snap();

            // The curve, at three levels.
            let block = 256usize;
            for level in [-30.0f64, -20.0, -6.0] {
                let amplitude = db_to_gain(level);
                let mut l = vec![0.0f32; block];
                let mut r = vec![0.0f32; block];
                let mut n = 0usize;
                let mut peak = 0.0f64;
                let settle = (3.0 * rate / block as f64) as usize;
                for pass in 0..settle + 40 {
                    for i in 0..block {
                        l[i] = (amplitude * (TAU * 1_000.0 * n as f64 / rate).sin()) as f32;
                        r[i] = l[i];
                        n += 1;
                    }
                    c.process(&mut l, &mut r, None);
                    if pass >= settle {
                        for &s in &l {
                            peak = peak.max(f64::from(s.abs()));
                        }
                    }
                }
                let measured = 20.0 * peak.log10();
                let wanted = level - static_reduction(level, -24.0, -0.75, 6.0);
                assert!(
                    (measured - wanted).abs() < 0.2,
                    "{rate} Hz, {level} dB in: {measured:.3} out, the curve says {wanted:.3}"
                );
            }

            // The ballistics, on the isolated detector at this rate.
            let a_att = one_pole(0.010, rate);
            let a_rel = compensated_release(0.010, 0.200, rate);
            let mut d = PeakDetector::new();
            let mut n = 0usize;
            while d.tick(1.0, a_att, a_rel) < 1.0 - std::f64::consts::E.recip() {
                n += 1;
                assert!(n < (10.0 * rate) as usize);
            }
            let attack = n as f64 / rate;
            assert!(
                (attack - 0.010).abs() / 0.010 < 0.03,
                "{rate} Hz: a 10 ms attack measured {:.4} ms",
                attack * 1000.0
            );
        }
    }

    // ── V18: allocation ──

    /// **`process` never reaches the allocator**, including on blocks where
    /// every control moves and the key comes and goes.
    #[test]
    fn process_never_reaches_the_allocator() {
        let block = 128usize;
        let mut c = comp();
        c.set_param_natural(PARAM_SC_HPF_HZ, 80.0);
        c.snap();
        let mut l = vec![0.1f32; block];
        let mut r = vec![0.1f32; block];
        let key_l = vec![0.5f32; block];
        let key_r = vec![0.4f32; block];
        c.process(&mut l, &mut r, None);

        let allocations = crate::synth::tests::allocations_during(|| {
            for pass in 0..64 {
                for index in 0..PARAM_COUNT {
                    let info = natural_param(index).unwrap();
                    let t = (pass % 8) as f32 / 7.0;
                    c.set_param_natural(index, info.min + (info.max - info.min) * t);
                }
                let key = if pass % 2 == 0 {
                    Some((key_l.as_slice(), key_r.as_slice()))
                } else {
                    None
                };
                c.process(&mut l, &mut r, key);
            }
        });
        assert_eq!(allocations, 0, "process allocated {allocations} times");
    }

    // ── The sidechain, at this level ──

    /// **The key input is what the detector hears, and the input is what gets
    /// the gain.**
    ///
    /// A silent track keyed off a loud one ducks nothing, because there is
    /// nothing to duck; a loud track keyed off a silent one is untouched. The
    /// test that catches a detector wired to the wrong slice.
    #[test]
    fn the_detector_reads_the_key_and_the_gain_lands_on_the_input() {
        let block = 512usize;
        let mut c = comp();
        c.set_param_natural(PARAM_THRESHOLD_DB, -40.0);
        c.set_param_natural(PARAM_RATIO, ratio_to_percent(8.0));
        c.set_param_natural(PARAM_KNEE_DB, 0.0);
        c.set_param_natural(PARAM_ATTACK_MS, 1.0);
        c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
        c.snap();

        // A quiet pad, keyed off a loud kick.
        let pad = vec![0.05f32; block];
        let loud = vec![0.9f32; block];
        let silent = vec![0.0f32; block];

        let mut l = pad.clone();
        let mut r = pad.clone();
        for _ in 0..40 {
            l.copy_from_slice(&pad);
            r.copy_from_slice(&pad);
            c.process(&mut l, &mut r, Some((&loud, &loud)));
        }
        let ducked = f64::from(l[block - 1]) / f64::from(pad[0]);
        assert!(
            ducked < 0.2,
            "a loud key only took the pad down to {:.3} of itself",
            ducked
        );

        // The same pad, keyed off silence: untouched, and the internal key
        // would not have touched it either at this threshold.
        c.reset();
        let mut l = pad.clone();
        let mut r = pad.clone();
        for _ in 0..40 {
            l.copy_from_slice(&pad);
            r.copy_from_slice(&pad);
            c.process(&mut l, &mut r, Some((&silent, &silent)));
        }
        assert!(
            (f64::from(l[block - 1]) - f64::from(pad[0])).abs() < 1.0e-6,
            "a silent key still moved the gain"
        );
    }

    /// **The stereo link is the dominant channel, not the average.**
    ///
    /// A signal hard on the left gets the same reduction as the same signal in
    /// the middle. An average would under-read it by 6 dB, so a snare panned
    /// left would escape the compressor.
    #[test]
    fn the_stereo_link_takes_the_louder_channel() {
        let block = 256usize;
        let measure = |left: f32, right: f32| -> f64 {
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, -30.0);
            c.set_param_natural(PARAM_RATIO, ratio_to_percent(4.0));
            c.set_param_natural(PARAM_KNEE_DB, 0.0);
            c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
            c.snap();
            let mut l = vec![0.0f32; block];
            let mut r = vec![0.0f32; block];
            let mut worst = 0.0;
            for _ in 0..400 {
                l.fill(left);
                r.fill(right);
                c.process(&mut l, &mut r, None);
                worst = -20.0 * f64::from(c.block_min_gain()).max(1.0e-12).log10();
            }
            worst
        };
        let centred = measure(0.4, 0.4);
        let panned = measure(0.4, 0.0);
        assert!(
            (centred - panned).abs() < 0.05,
            "a hard-panned signal got {panned:.3} dB against the {centred:.3} dB a centred one \
             gets"
        );
        assert!(centred > 3.0, "the rig was not compressing: {centred:.3} dB");
    }

    // ── The parameter surface ──

    /// The twelve controls are the ones the compressor declares, in its
    /// order, with the defaults `../FX.md` settled.
    #[test]
    fn it_declares_twelve_controls_with_the_house_defaults() {
        assert_eq!(PARAM_COUNT, 12);
        assert!(natural_param(PARAM_COUNT).is_none());
        let defaults = default_natural_params();
        assert_eq!(defaults[PARAM_THRESHOLD_DB], -18.0);
        assert_eq!(percent_to_ratio(defaults[PARAM_RATIO]).round(), 3.0);
        assert_eq!(defaults[PARAM_KNEE_DB], 6.0);
        assert_eq!(defaults[PARAM_ATTACK_MS], 10.0);
        assert_eq!(defaults[PARAM_RELEASE_MS], 120.0);
        assert_eq!(defaults[PARAM_AUTO_MAKEUP], 1.0, "auto makeup ships on");
        assert_eq!(defaults[PARAM_MIX], 100.0);
        assert_eq!(defaults[PARAM_SENSE], 0.0, "peak sensing ships");
        assert_eq!(defaults[PARAM_SC_HPF_HZ], 0.0, "the detector filter ships off");
        assert_eq!(defaults[PARAM_AUTO_RELEASE], 0.0);

        // Names are the preset layout's fingerprint. Changing one orphans
        // every saved compressor preset, so they are asserted rather than
        // assumed.
        let names: Vec<&str> = (0..PARAM_COUNT).map(param_name).collect();
        assert_eq!(
            names,
            [
                "char", "thresh", "ratio", "knee", "attack", "releas", "arel", "makeup",
                "mkauto", "mix", "sense", "schpf"
            ]
        );
        for name in &names {
            assert!(name.len() <= 6, "{name} does not fit the panel's six columns");
        }
    }

    /// **The ratio law is linear in slope**, and both ends of it are exact.
    #[test]
    fn the_ratio_law_is_linear_in_slope() {
        assert_eq!(ratio_to_percent(1.0), 0.0);
        assert_eq!(ratio_to_percent(f64::INFINITY), 100.0);
        assert_eq!(percent_to_ratio(0.0), 1.0);
        assert!(percent_to_ratio(100.0).is_infinite());
        assert_eq!(ratio_to_percent(2.0), 50.0);
        assert_eq!(ratio_to_percent(4.0), 75.0);
        assert_eq!(ratio_to_percent(10.0), 90.0);
        // The table's own default has to be the number the law produces, or
        // the panel and the arithmetic disagree about what 3:1 is.
        assert_eq!(PARAMS[PARAM_RATIO].default, ratio_to_percent(3.0));

        // Linear in slope, and therefore linear in effect: equal steps of the
        // control are equal steps of dB-per-dB.
        for percent in [0.0f32, 25.0, 50.0, 75.0, 100.0] {
            let ratio = percent_to_ratio(percent);
            let slope = if ratio.is_infinite() { 0.0 } else { 1.0 / ratio };
            assert!(
                (slope - (1.0 - f64::from(percent) / 100.0)).abs() < 1.0e-6,
                "{percent}% is not 1/R = {slope}"
            );
        }

        // The stops read as a manual prints them.
        assert_eq!(ratio_label(ratio_to_percent(1.0)), "1.0:1");
        assert_eq!(ratio_label(ratio_to_percent(3.0)), "3.0:1");
        assert_eq!(ratio_label(ratio_to_percent(20.0)), "20:1");
        assert_eq!(ratio_label(100.0), "\u{221e}:1");
    }

    /// **The automatic makeup takes back half of what the threshold costs.**
    ///
    /// `M = −T·(1 − 1/R)/2` is GMR's own estimate of the average gain
    /// reduction, negated — and the `/2` is load-bearing rather than a fudge:
    /// their estimate is that a programme spends its time about half way
    /// between silence and full scale, so half the reduction a full-scale
    /// signal would get is the honest guess at the average.
    ///
    /// What that buys, and what it does not, measured on a −12 dBFS tone
    /// across an 18 dB threshold sweep at 3:1: without the automatic the
    /// output falls 12 dB across the sweep; with it, 5.5 dB. It *halves* the
    /// drift. It cannot remove it, and a version that did would have to
    /// measure the programme — which would make the output level depend on
    /// the material, break the parallel-sum test, break an honest A/B, and
    /// make a session non-reproducible. Half, deterministically, is the trade.
    #[test]
    fn the_automatic_makeup_takes_back_half_of_what_the_threshold_costs() {
        assert!((auto_makeup_for(-18.0, 2.0 / 3.0) - 6.0).abs() < 0.01);
        assert!((auto_makeup_for(-24.0, 0.9) - 10.8).abs() < 0.01);
        assert_eq!(auto_makeup_for(-18.0, 0.0), 0.0, "1:1 asks for no makeup");

        // A signal that is being compressed, at four thresholds, with the
        // automatic on: the output level has to stay inside a few decibels
        // rather than falling away with the threshold.
        let mut levels = Vec::new();
        for threshold in [-30.0f32, -24.0, -18.0, -12.0] {
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, threshold);
            c.set_param_natural(PARAM_RATIO, ratio_to_percent(3.0));
            c.set_param_natural(PARAM_KNEE_DB, 6.0);
            c.snap();
            levels.push(settled_output_db(&mut c, -12.0, 1_000.0));
        }
        let spread = levels
            .iter()
            .fold(f64::NEG_INFINITY, |a, b| a.max(*b))
            - levels.iter().fold(f64::INFINITY, |a, b| a.min(*b));

        // Without it, the same sweep falls away twice as far.
        let mut bare = Vec::new();
        for threshold in [-30.0f32, -24.0, -18.0, -12.0] {
            let mut c = comp();
            c.set_param_natural(PARAM_THRESHOLD_DB, threshold);
            c.set_param_natural(PARAM_RATIO, ratio_to_percent(3.0));
            c.set_param_natural(PARAM_KNEE_DB, 6.0);
            c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
            c.snap();
            bare.push(settled_output_db(&mut c, -12.0, 1_000.0));
        }
        let bare_spread = bare.iter().fold(f64::NEG_INFINITY, |a, b| a.max(*b))
            - bare.iter().fold(f64::INFINITY, |a, b| a.min(*b));
        assert!(
            bare_spread > 10.0,
            "the sweep did not move the level to begin with: {bare_spread:.2} dB"
        );
        assert!(
            spread < bare_spread * 0.55,
            "the automatic makeup took back {:.0}% of the {bare_spread:.2} dB the threshold \
             cost, not half: {levels:?}",
            (1.0 - spread / bare_spread) * 100.0
        );
    }

    /// The characters are a macro: the selector stores which one, and
    /// recalling one is the caller's job.
    #[test]
    fn the_characters_are_parameter_sets() {
        assert_eq!(CHARACTER_COUNT, 9);
        assert_eq!(CHARACTERS[0].name, "basic");
        // Index zero is the factory setting itself, so the control has a home
        // to come back to.
        assert_eq!(character_params(0), default_natural_params());

        for (index, character) in CHARACTERS.iter().enumerate() {
            let params = character_params(index);
            assert_eq!(params[PARAM_CHARACTER], index as f32, "{}", character.name);
            for (i, &value) in params.iter().enumerate() {
                let info = natural_param(i).unwrap();
                assert!(
                    value >= info.min && value <= info.max,
                    "{}: {} is outside {}..{}",
                    character.name,
                    info.name,
                    info.min,
                    info.max
                );
            }
            assert!(matches_character(&params), "{} does not match itself", character.name);
            assert_eq!(character_name(&params), character.name);
            assert!(!character_note(&params).is_empty());
        }

        // The selector is inert on its own: writing it does not rewrite the
        // other eleven, so a session load cannot depend on the order the
        // controls are written in.
        let mut c = comp();
        c.set_param_natural(PARAM_THRESHOLD_DB, -42.0);
        c.set_param_natural(PARAM_CHARACTER, 3.0);
        assert_eq!(c.param_natural(PARAM_THRESHOLD_DB), -42.0);
        assert_eq!(c.param_natural(PARAM_CHARACTER), 3.0);
        // ...and the panel can therefore see that it is no longer that
        // character.
        let params: Vec<f32> = (0..PARAM_COUNT).map(|i| c.param_natural(i)).collect();
        assert!(!matches_character(&params), "an edited character still claims to be one");
    }

    /// The two automatics grey their knobs out, and nothing else does.
    #[test]
    fn the_automatics_grey_out_the_knobs_they_have_taken_over() {
        let mut params = default_natural_params();
        assert!(!uses(&params, PARAM_MAKEUP_DB), "auto makeup ships on, so the knob is greyed");
        assert!(uses(&params, PARAM_RELEASE_MS));
        params[PARAM_AUTO_MAKEUP] = 0.0;
        assert!(uses(&params, PARAM_MAKEUP_DB));
        params[PARAM_AUTO_RELEASE] = 1.0;
        assert!(!uses(&params, PARAM_RELEASE_MS));
        // Everything else is always live, including the detector filter — a
        // control you cannot turn back on is worse than one doing nothing.
        for index in 0..PARAM_COUNT {
            if index != PARAM_MAKEUP_DB && index != PARAM_RELEASE_MS {
                assert!(uses(&params, index), "index {index} greyed out for no reason");
            }
        }
        assert!(!uses(&params, PARAM_COUNT));
    }

    /// Nonsense from a UI or a hand-edited session file is refused, not
    /// propagated into a gain.
    #[test]
    fn it_survives_nonsense() {
        let mut c = comp();
        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| c.param_natural(i)).collect();
        c.set_param_natural(PARAM_COUNT, 1.0);
        c.set_param_natural(usize::MAX, 1.0);
        for index in 0..PARAM_COUNT {
            c.set_param_natural(index, f32::NAN);
            c.set_param_natural(index, f32::INFINITY);
        }
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| c.param_natural(i)).collect();
        assert_eq!(before, after);
        assert_eq!(c.param_natural(PARAM_COUNT), 0.0);

        // Values well outside the travel are clamped into it.
        c.set_param_natural(PARAM_THRESHOLD_DB, 200.0);
        assert_eq!(c.param_natural(PARAM_THRESHOLD_DB), 0.0);
        c.set_param_natural(PARAM_RATIO, -50.0);
        assert_eq!(c.param_natural(PARAM_RATIO), 0.0);

        // A rate the device could not have asked for leaves it built at the
        // last one it was given, and still compressing.
        let rate = c.sample_rate();
        c.set_sample_rate(0.0);
        c.set_sample_rate(f64::NAN);
        c.set_sample_rate(-1.0);
        assert_eq!(c.sample_rate(), rate);

        // An empty block, and a key shorter than the block it is keying.
        let mut l = Vec::<f32>::new();
        let mut r = Vec::<f32>::new();
        c.process(&mut l, &mut r, None);
        let mut l = vec![0.5f32; 64];
        let mut r = vec![0.5f32; 64];
        let short = vec![1.0f32; 8];
        c.process(&mut l, &mut r, Some((&short, &short)));
        assert!(l.iter().all(|s| s.is_finite()));
    }

    /// Reset drops the tails and keeps the controls, which is what a bypass
    /// and a panic both need.
    #[test]
    fn reset_drops_the_tail_and_keeps_the_controls() {
        let mut c = comp();
        c.set_param_natural(PARAM_THRESHOLD_DB, -50.0);
        c.set_param_natural(PARAM_RATIO, 100.0);
        c.set_param_natural(PARAM_RELEASE_MS, 3_000.0);
        c.set_param_natural(PARAM_AUTO_MAKEUP, 0.0);
        c.snap();
        let mut l = vec![0.9f32; 256];
        let mut r = vec![0.9f32; 256];
        c.process(&mut l, &mut r, None);
        assert!(c.block_min_gain() < 0.5, "the rig was not compressing");

        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| c.param_natural(i)).collect();
        c.reset();
        assert_eq!(c.block_min_gain(), 1.0);
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| c.param_natural(i)).collect();
        assert_eq!(before, after, "the flush moved a control");

        // ...and the next block starts from unity rather than from where the
        // gain had got to.
        let mut l = vec![0.001f32; 8];
        let mut r = vec![0.001f32; 8];
        c.process(&mut l, &mut r, None);
        assert!(
            (f64::from(l[0]) / 0.001 - 1.0).abs() < 0.01,
            "the first sample after a reset was {}",
            l[0]
        );
    }

    /// The knob-torture standard: every envelope-shaping control flicked
    /// hard under a loud tone. There is no feedback here to run away, but
    /// a detector fed inconsistent coefficients can still go non-finite or
    /// slam the makeup — the output must stay bounded and settle after.
    #[test]
    fn knob_torture_stays_bounded() {
        let mut comp = Compressor::new(48_000.0);
        comp.set_param_natural(PARAM_MIX, 100.0);
        let fs = 48_000.0;
        let mut toggle = false;
        let mut peak = 0.0f32;
        let frames = (2.0 * fs) as usize;
        for n in 0..frames {
            if n % ((fs * 0.02) as usize) == 0 {
                toggle = !toggle;
                comp.set_param_natural(PARAM_THRESHOLD_DB, if toggle { -50.0 } else { 0.0 });
                comp.set_param_natural(PARAM_RATIO, if toggle { 20.0 } else { 1.5 });
                comp.set_param_natural(PARAM_ATTACK_MS, if toggle { 0.1 } else { 80.0 });
                comp.set_param_natural(PARAM_RELEASE_MS, if toggle { 20.0 } else { 800.0 });
                comp.set_param_natural(PARAM_MAKEUP_DB, if toggle { 12.0 } else { 0.0 });
            }
            let x = 0.5 * (2.0 * std::f64::consts::PI * 220.0 * n as f64 / fs).sin() as f32;
            let mut l = [x];
            let mut r = [x];
            comp.process(&mut l, &mut r, None);
            assert!(l[0].is_finite() && r[0].is_finite(), "the detector went non-finite");
            peak = peak.max(l[0].abs()).max(r[0].abs());
        }
        // +12 dB makeup over a −6 dB tone bounds near 2; a blow-up is far past.
        assert!(peak < 4.0, "the torture blew up: peak {peak}");
    }
}
