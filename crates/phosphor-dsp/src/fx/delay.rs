//! The delay: three characters, three routings, and a feedback loop with a
//! proof behind it.
//!
//! # Two axes, not one
//!
//! Most delays put `Tape`, `BBD` and `Ping-Pong` in the same selector, which
//! is why nobody has ever heard a tape ping-pong. They are two different
//! questions and they are asked separately here:
//!
//! ```text
//! mode:    Digital | BBD | Tape        — what the repeats sound like
//! routing: Stereo  | Ping-Pong | Mono  — where they go
//! ```
//!
//! Nine combinations, and every one of them is a delay somebody wants. The
//! cost of keeping the axes apart is one extra `match`.
//!
//! # Why the feedback goes to 200%
//!
//! The usual answer is a clamp: hold feedback below unity and the loop cannot
//! run away. That is a *guess* dressed as a safety rail, and it costs the one
//! sound a delay has that nothing else does — the rising, filtering scream of
//! a line fed slightly more than it loses.
//!
//! The bound here is arithmetic. The last thing in the loop before the write
//! is a saturator
//!
//! ```text
//! sat(x) = tanh(g·x)/g
//! ```
//!
//! and `|tanh| ≤ 1`, so `|sat(x)| ≤ 1/g` for **any** input at all. The line's
//! write is `in + fb·sat(…)`, therefore
//!
//! ```text
//! |line| ≤ |in|_max + fb/g          for any fb, including fb > 1
//! ```
//!
//! It is a bound, not an attenuation: no amount of feedback, no filter
//! setting, no mode and no signal can put the loop above it. Measured against
//! the analytic value at 250 ms, `g = 1`, a 0.7-peak tone and then a minute of
//! silence, the loop peaks at 1.562 against a bound of 1.650 at 95% feedback,
//! 1.724 against 1.800 at 110%, and 2.679 against 2.700 at 200% — where it
//! settles into a stable self-oscillation rather than running away.
//!
//! Three details carry that, and none of them are optional:
//!
//! * **The saturator is the last thing in the loop.** Anything after it —
//!   including a DC blocker, whose gain at Nyquist is `2/(1+a)` and therefore
//!   just above one — turns an exact bound into an approximate one. The DC
//!   blocker is therefore *before* it.
//! * **The DC blocker is in the loop at all.** Asymmetric material saturates
//!   asymmetrically, the offset is multiplied by the feedback and written
//!   back, and a loop with an offset walks away. It does not show up in a
//!   short test.
//! * **The `/g` normalisation.** `tanh(g·x)` has small-signal gain `g`, so at
//!   `g > 1` a repeat comes back *louder* than the one before it. This house
//!   has shipped that bug once already, in the bucket-brigade delay of the
//!   TEO-5, and fixed it the same way.
//!
//! Freeze is the one place the loop gain is exactly one and the saturator and
//! the filters are out of the path: an "endlessly cycling" buffer that darkens
//! and quietens away is not what the word means.
//!
//! # What each mode does that the others do not
//!
//! * **Digital** — the line, the loop filters, and nothing else. The `time`
//!   knob crossfades, because a digital delay that bends pitch is wrong.
//! * **BBD** — the loop's low-pass corner follows the *clock*, and the clock
//!   is `N/(2·τ)` with `N = 4096` stages. Longer delay, slower clock, darker
//!   repeats — and it compounds, which a fixed corner cannot imitate.
//!   Measured, 5 kHz relative to 1 kHz per repeat: −3.3 dB per repeat at
//!   120 ms against −13.7 dB at 600 ms. The `time` knob repitches over 10 ms,
//!   because a bucket brigade's clock rate *is* its delay time.
//! * **Tape** — three heads at 1 : 2 : 3, wow and flutter *multiplied* into
//!   the read offset rather than added to it, and a head bump inside the loop
//!   so each repeat gets a little boomier as it gets darker. The `time` knob
//!   repitches over 120 ms, because a transport has inertia.
//!
//! The multiplicative wow is the part that is easy to get wrong. A tape
//! *recorder*'s heads are a fixed distance apart, so a speed error moves the
//! read head by `D/(2πf)` regardless of any delay. A tape *echo*'s delay time
//! **is** `d_head/v`, so the same speed error scales it: `Δτ = τ₀·D`. That is
//! why a long Space Echo setting warbles harder than a short one, and it is
//! why this file multiplies where a tape-machine emulation would add.

use std::f64::consts::{PI, TAU};

// ---------------------------------------------------------------------------
// The shape of the thing
// ---------------------------------------------------------------------------

/// The longest delay a line is built for, in seconds.
///
/// Also the ceiling the sync law folds musical divisions down into: a whole
/// note at 40 BPM is six seconds, and it ships as three with the panel saying
/// so.
pub const MAX_DELAY_S: f64 = 5.0;

/// The shortest. One millisecond is not a rounding of zero — it is what lets
/// this device double as a comb filter and as a Haas widener.
pub const MIN_DELAY_S: f64 = 0.001;

/// How much room above [`MAX_DELAY_S`] a line carries for the modulation to
/// move the read head into.
///
/// Without it a maximum-length delay reads against the end of its own buffer:
/// the wander is clamped on the way out and free on the way back, which is
/// half a wow, and half a wow is not one.
const LINE_SPAN: f64 = 0.02;

/// Anything below this in a filter state or on its way into a line is flushed
/// to zero. A delay's whole business is tails, and a tail that decays into the
/// subnormal range costs a microcoded fault per sample forever.
const DENORMAL_FLOOR: f64 = 1.0e-30;

/// The corner of the in-loop DC blocker.
const DC_BLOCK_HZ: f64 = 12.0;

/// Time constant of the control smoothers, matching the reverb's and the EQ's.
const SMOOTH_SECONDS: f64 = 0.015;

/// Below this, a smoother chasing zero is snapped to it — so a mix knob turned
/// to zero actually reaches zero and the dry null is exact rather than nearly.
const SMOOTH_SNAP: f64 = 1.0e-6;

/// How long a `Fade` time change takes to cross over.
const FADE_SECONDS: f64 = 0.020;

/// How fast a `Repitch` read head walks to its new length, per mode.
///
/// The tape and the bucket brigade are the two numbers this brief settles:
/// a BBD's clock changes the instant its control voltage does, and a tape
/// transport has a capstan and a reel to accelerate. Most of why the two modes
/// *feel* different under a sweep is these two constants.
const SLEW_DIGITAL_S: f64 = 0.020;
const SLEW_BBD_S: f64 = 0.010;
const SLEW_TAPE_S: f64 = 0.120;

/// How far a delay length has to move before the read path treats it as a new
/// target at all — a quarter of a sample. Below that a `Fade` would retrigger
/// on floating-point noise in the tempo.
const TARGET_EPSILON: f64 = 0.25;

// ── The three modes ──

/// Stages in the modelled bucket-brigade line. A 4096-stage device is the
/// common one, and the number sets both the clock and the distortion.
const BBD_STAGES: f64 = 4096.0;

/// Where the loop's low-pass sits relative to the clock, and the two ends it
/// is held between. Raffel and Smith put the anti-alias corner between
/// `f_clk/3` and `f_clk/2`; the lower end is the darker and the more
/// characteristic.
const BBD_CLOCK_DIVISOR: f64 = 3.0;
const BBD_LP_MIN_HZ: f64 = 800.0;
const BBD_LP_MAX_HZ: f64 = 12_000.0;

/// How far the bucket-brigade clock drifts at the `wander` knob's maximum,
/// and how often it picks a new place to drift to.
///
/// The span is multiplicative on the read offset — the echo form — and the
/// walk is a smoothstep between random targets rather than filtered noise.
/// That difference is the whole of what separates wow from hiss: a one-pole
/// *attenuates* the top of the band and does not remove it, and whatever is
/// left gets multiplied by the delay length in samples on its way to the read
/// head. A smoothstep walk's slope is bounded by `1.5·span·hz` per second by
/// construction, which is a bound rather than an attenuation.
const BBD_WANDER_SPAN: f64 = 0.0025;
const BBD_WANDER_HZ: f64 = 0.6;

/// Raffel and Smith's third-order fit to measured bucket-brigade spectra.
///
/// Their published form carries a `+a` constant, which is a DC offset that a
/// DC blocker takes straight back out. It is dropped here so that the whole
/// loop is zero-preserving and "silence in, silence out" is exact rather than
/// asymptotic.
const BBD_SHAPE_A: f64 = 1.0 / 8.0;
const BBD_SHAPE_B: f64 = 1.0 / 18.0;

/// The compander wrapped around the line, and the bounds it is held inside.
///
/// A real bucket brigade companded to get its noise floor under control. Ours
/// has no noise floor to fix — it is a clean digital line — so what survives
/// is the level-dependent grit, which is the audible part anyway.
const BBD_COMPAND_HZ: f64 = 30.0;
const BBD_COMPAND_FLOOR: f64 = 0.05;
const BBD_COMPAND_MIN: f64 = 0.5;
const BBD_COMPAND_MAX: f64 = 4.0;

/// The Space Echo's heads are evenly spaced, so head two and head three sit at
/// exactly twice and three times head one's delay.
const HEAD_RATIOS: [f64; 3] = [1.0, 2.0, 3.0];

/// Wow, flutter and scrape on a tape echo's read offset: speed deviation and
/// rate, as the measurement standards specify them.
///
/// Fixed and unexposed. These are a machine's specification, not a taste: a
/// professional recorder reaches 0.03% weighted and a good cassette deck
/// 0.08%, and a Space Echo is somewhere in between with more of it down at the
/// capstan's own rate.
///
/// # Only the wow is multiplicative, and that is physics rather than taste
///
/// A tape echo's delay time **is** the head spacing over the tape speed, so a
/// speed error scales it: `Δτ = τ₀·D`, and the pitch deviation you hear is
/// `τ₀·2πf·D` — larger than the machine's own speed error by `τ₀·2πf`. At
/// 375 ms and 0.6 Hz that factor is 1.41, which is why a long Space Echo
/// setting warbles harder than a short one. That is the wow, and it is the
/// whole point of the mode.
///
/// Applying the same rule to flutter and to scrape gives nonsense, and the
/// arithmetic says so out loud: at 250 ms the factor is 11 for flutter and
/// **91** for scrape, which would put 31 cents of 58 Hz wobble on every echo.
/// The physical reason it is nonsense is that scrape flutter is stick-slip
/// *at a head*, and bearing flutter is largely local too — the ten inches of
/// tape between the record and the replay head does not stretch and shrink
/// seventy times a second. Those two are therefore an **additive** excursion
/// of `D/(2πf)`, which is the tape-recorder form and delivers exactly the
/// deviation they are specified as: 0.327 samples for the flutter and 0.026
/// for the scrape, at 48 kHz.
const TAPE_WOW: (f64, f64) = (0.0010, 0.6);
const TAPE_FLUTTER: (f64, f64) = (0.0003, 7.0);
const TAPE_SCRAPE: (f64, f64) = (0.0002, 58.0);

/// The additive excursion a speed deviation at a rate asks for, in seconds.
///
/// For a sinusoidal speed deviation `D·sin(2πft)` the instantaneous frequency
/// ratio of a modulated delay is `1 − dτ/dt`, so `dτ/dt = −D·sin(2πft)` and
/// the peak excursion is `D/(2πf)`.
#[inline]
fn excursion_seconds(depth: f64, hz: f64) -> f64 {
    depth / (TAU * hz)
}

/// The head bump inside a tape echo's loop: a gentle low resonance that each
/// repeat gets one more helping of.
const TAPE_BUMP_HZ: f64 = 70.0;
const TAPE_BUMP_Q: f64 = 1.2;
const TAPE_BUMP_DB: f64 = 1.5;

/// The in-loop saturator's drive, per mode.
///
/// This is the `g` of the bound in the module documentation, so it is also the
/// most feedback-dependent number in the file: at `fb = 2.0` the tape's loop
/// can hold `2.0/1.6 = 1.25` where the digital one holds `2.0`.
const DRIVE_DIGITAL: f64 = 1.0;
const DRIVE_BBD: f64 = 1.0;
const DRIVE_TAPE: f64 = 1.6;

// ── Ducking ──

/// The ducker's ballistics and its two fixed levels.
///
/// Threshold-less by design: one knob, and no control a player has to re-set
/// every time the source's level moves. The envelope is normalised against a
/// fixed floor and the reduction it implies is scaled by the knob.
///
/// Two milliseconds of attack is not negotiable — slower and the wet pokes
/// through the front of every note, which is the exact artifact ducking exists
/// to remove. The release constant is picked so the wet returns from 10% to
/// 90% of full duck in 200 ms.
const DUCK_ATTACK_S: f64 = 0.002;
const DUCK_RELEASE_S: f64 = 0.090;
pub const DUCK_FLOOR_DB: f64 = -30.0;
pub const DUCK_MAX_DB: f64 = 24.0;
/// The two levels above, as the linear numbers the detector compares against.
/// Written out rather than computed, because `powf` per sample is exactly what
/// the fast logarithm below exists to avoid.
const DUCK_FLOOR: f64 = 0.031_622_776_601_683_79; // 10^(−30/20)
const DUCK_CEILING: f64 = 15.848_931_924_611_133; // 10^( 24/20)

// ---------------------------------------------------------------------------
// The sync table
// ---------------------------------------------------------------------------

/// The sixteen musical divisions, in beats.
///
/// Grouped straight-dotted-triplet inside each base division, which is the
/// house walk: `FX_SYNC_BEATS` already ships in the Prophet-6 and the TEO-5
/// with exactly this order and players have learned it. A strictly ascending
/// list would make `h`/`l` mean "shorter/longer", which is a real argument —
/// and two different meanings for "the next entry" inside one application is a
/// worse bug than one non-monotonic list.
pub const SYNC_BEATS: [f64; 16] = [
    0.125,
    0.1875,
    1.0 / 6.0,
    0.25,
    0.375,
    1.0 / 3.0,
    0.5,
    0.75,
    2.0 / 3.0,
    1.0,
    1.5,
    4.0 / 3.0,
    2.0,
    3.0,
    8.0 / 3.0,
    4.0,
];

/// What each division is called.
pub const SYNC_LABELS: [&str; 16] = [
    "1/32", "1/32D", "1/16T", "1/16", "1/16D", "1/8T", "1/8", "1/8D", "1/4T", "1/4", "1/4D",
    "1/2T", "1/2", "1/2D", "1/1T", "1/1",
];

/// How many divisions there are.
pub const SYNC_COUNT: usize = SYNC_BEATS.len();

/// The division the delay opens on: a dotted eighth, which is the one setting
/// that makes a straight part sound like it was played by two people.
pub const SYNC_DEFAULT: usize = 7;

/// The tempo range the grid is resolved against. Outside it the arithmetic is
/// still finite; a session with a corrupt tempo does not get a delay of
/// infinity.
const BPM_MIN: f64 = 20.0;
const BPM_MAX: f64 = 999.0;

/// The delay time a division asks for at a tempo, and how many times it had to
/// be halved to fit the line.
///
/// The halving is the existing house law — `while s > DELAY_MAX_S { s *= 0.5 }`
/// from the Prophet-6 — and the count comes back so the panel can say so. A
/// grid that silently breaks is worse than one that says it moved: at 40 BPM a
/// whole note is 6.0 s and ships as 3.0 s.
#[must_use]
pub fn synced_seconds(division: usize, tempo_bpm: f64) -> (f64, u32) {
    let beats = SYNC_BEATS[division.min(SYNC_COUNT - 1)];
    let bpm = if tempo_bpm.is_finite() {
        tempo_bpm.clamp(BPM_MIN, BPM_MAX)
    } else {
        120.0
    };
    let mut seconds = beats * 60.0 / bpm;
    let mut halvings = 0;
    while seconds > MAX_DELAY_S {
        seconds *= 0.5;
        halvings += 1;
    }
    (seconds.max(MIN_DELAY_S), halvings)
}

/// The division whose resolved time is nearest to `seconds`, in the ratio
/// sense rather than the difference sense.
///
/// What the sync switch uses to carry a hand-dialled time over onto the grid.
/// Ratio rather than difference because "close" at 30 ms and "close" at 3 s
/// are not the same number of milliseconds, and the ear agrees with the ratio.
#[must_use]
pub fn nearest_division(seconds: f64, tempo_bpm: f64) -> usize {
    let wanted = seconds.max(MIN_DELAY_S);
    let mut best = SYNC_DEFAULT;
    let mut best_error = f64::INFINITY;
    for division in 0..SYNC_COUNT {
        let (candidate, _) = synced_seconds(division, tempo_bpm);
        let error = (candidate / wanted).ln().abs();
        if error < best_error {
            best_error = error;
            best = division;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// The two selectors
// ---------------------------------------------------------------------------

/// What the repeats sound like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Digital,
    Bbd,
    Tape,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Digital, Mode::Bbd, Mode::Tape];

    #[must_use]
    pub fn from_index(index: usize) -> Self {
        Self::ALL[index.min(Self::ALL.len() - 1)]
    }

    #[must_use]
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Digital => "digital",
            Self::Bbd => "bbd",
            Self::Tape => "tape",
        }
    }

    /// The in-loop saturator's drive — the `g` of the bound.
    #[must_use]
    fn drive(self) -> f64 {
        match self {
            Self::Digital => DRIVE_DIGITAL,
            Self::Bbd => DRIVE_BBD,
            Self::Tape => DRIVE_TAPE,
        }
    }

    /// What `Auto` resolves to on this mode, and how fast.
    ///
    /// For two of the three it is not a preference. A bucket brigade's clock
    /// rate *is* its delay time and a Space Echo moves a capstan, so a user
    /// who picks either and gets a clean crossfade will file it as a bug.
    /// Equally, a digital delay that repitches is wrong.
    #[must_use]
    fn auto_time_mode(self) -> TimeMode {
        match self {
            Self::Digital => TimeMode::Fade,
            Self::Bbd | Self::Tape => TimeMode::Repitch,
        }
    }

    /// How long a repitch takes on this mode.
    #[must_use]
    fn slew_seconds(self) -> f64 {
        match self {
            Self::Digital => SLEW_DIGITAL_S,
            Self::Bbd => SLEW_BBD_S,
            Self::Tape => SLEW_TAPE_S,
        }
    }
}

/// Where the repeats go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Routing {
    Stereo,
    PingPong,
    Mono,
}

impl Routing {
    pub const ALL: [Routing; 3] = [Routing::Stereo, Routing::PingPong, Routing::Mono];

    #[must_use]
    pub fn from_index(index: usize) -> Self {
        Self::ALL[index.min(Self::ALL.len() - 1)]
    }

    #[must_use]
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|r| *r == self).unwrap_or(0)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Stereo => "stereo",
            Self::PingPong => "ping-pong",
            Self::Mono => "mono",
        }
    }
}

/// What happens when the time knob moves.
///
/// Live's vocabulary, verbatim: Repitch "produces a pitch variation when the
/// delay time is changed", Fade "creates a crossfade between the old and new
/// delay times", Jump "immediately switches". Inventing synonyms for a
/// vocabulary every player already has is a cost with no benefit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeMode {
    Auto,
    Repitch,
    Fade,
    Jump,
}

impl TimeMode {
    pub const ALL: [TimeMode; 4] =
        [TimeMode::Auto, TimeMode::Repitch, TimeMode::Fade, TimeMode::Jump];

    #[must_use]
    pub fn from_index(index: usize) -> Self {
        Self::ALL[index.min(Self::ALL.len() - 1)]
    }

    #[must_use]
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Repitch => "repitch",
            Self::Fade => "fade",
            Self::Jump => "jump",
        }
    }

    /// What this setting means on a given mode.
    #[must_use]
    pub fn resolve(self, mode: Mode) -> TimeMode {
        match self {
            Self::Auto => mode.auto_time_mode(),
            other => other,
        }
    }
}

/// Which of the three tape heads are reading.
///
/// Seven combinations, which is what three heads with at least one enabled
/// gives you. The RE-201's own selector has twelve because it folds the
/// reverb and some duplicates in; the seven that are about heads are these.
pub const HEAD_SETS: [[bool; 3]; 7] = [
    [true, false, false],
    [false, true, false],
    [false, false, true],
    [true, true, false],
    [true, false, true],
    [false, true, true],
    [true, true, true],
];

/// What each head combination is called.
pub const HEAD_LABELS: [&str; 7] = ["1", "2", "3", "1+2", "1+3", "2+3", "1+2+3"];

/// The heads a selector position turns on.
#[must_use]
pub fn head_set(index: usize) -> [bool; 3] {
    HEAD_SETS[index.min(HEAD_SETS.len() - 1)]
}

/// The longest ratio a head combination reads at.
///
/// Also the divisor on the base delay, because a line that is five seconds
/// long cannot hold a third head at three times a five-second base. Clamping
/// the base rather than the tap is what keeps 1 : 2 : 3 exactly 1 : 2 : 3.
#[must_use]
pub fn head_span(index: usize) -> f64 {
    head_set(index)
        .iter()
        .enumerate()
        .filter(|(_, on)| **on)
        .map(|(head, _)| HEAD_RATIOS[head])
        .fold(1.0, f64::max)
}

// ---------------------------------------------------------------------------
// The flat parameter surface, in natural units
// ---------------------------------------------------------------------------
//
// Percent, hertz and milliseconds — never a 0..1 knob fraction. A session
// stores what a control *meant*, so a range that moves later cannot silently
// re-point every saved file.

pub const PARAM_MODE: usize = 0;
pub const PARAM_ROUTING: usize = 1;
pub const PARAM_SYNC: usize = 2;
pub const PARAM_DIVISION: usize = 3;
pub const PARAM_TIME_MS: usize = 4;
pub const PARAM_OFFSET: usize = 5;
pub const PARAM_TIME_MODE: usize = 6;
pub const PARAM_FEEDBACK: usize = 7;
pub const PARAM_FREEZE: usize = 8;
pub const PARAM_LOW_CUT_HZ: usize = 9;
pub const PARAM_HIGH_CUT_HZ: usize = 10;
pub const PARAM_DUCK: usize = 11;
pub const PARAM_WIDTH: usize = 12;
pub const PARAM_HEADS: usize = 13;
pub const PARAM_WANDER: usize = 14;
pub const PARAM_MIX: usize = 15;

/// How many controls a delay has.
pub const PARAM_COUNT: usize = 16;

/// One control, as a host sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NaturalParam {
    pub name: &'static str,
    /// `"Hz"`, `"ms"`, `"%"`, or empty for the counted controls and switches.
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

/// The table every other view of the parameters is generated from.
///
/// The defaults are the house's: *sync on, 1/8 dotted, feedback 30%, loop
/// filters on at 200 Hz and 6 kHz, ping-pong off, 22% wet on an insert.* The
/// loop filters being **on** is the amateur-versus-professional default — a
/// delay whose repeats keep every scrap of top and bottom fights the source
/// for the same space and never recedes behind it.
const PARAMS: [NaturalParam; PARAM_COUNT] = [
    NaturalParam { name: "mode", unit: "", min: 0.0, max: 2.0, default: 0.0 },
    NaturalParam { name: "route", unit: "", min: 0.0, max: 2.0, default: 0.0 },
    NaturalParam { name: "sync", unit: "", min: 0.0, max: 1.0, default: 1.0 },
    NaturalParam {
        name: "div",
        unit: "",
        min: 0.0,
        max: (SYNC_COUNT - 1) as f32,
        default: SYNC_DEFAULT as f32,
    },
    NaturalParam {
        name: "time",
        unit: "ms",
        min: (MIN_DELAY_S * 1000.0) as f32,
        max: (MAX_DELAY_S * 1000.0) as f32,
        default: 375.0,
    },
    NaturalParam { name: "offset", unit: "%", min: -50.0, max: 50.0, default: 0.0 },
    NaturalParam { name: "tmode", unit: "", min: 0.0, max: 3.0, default: 0.0 },
    NaturalParam { name: "fb", unit: "%", min: 0.0, max: 200.0, default: 30.0 },
    NaturalParam { name: "freeze", unit: "", min: 0.0, max: 1.0, default: 0.0 },
    NaturalParam { name: "locut", unit: "Hz", min: 20.0, max: 2_000.0, default: 200.0 },
    NaturalParam { name: "hicut", unit: "Hz", min: 200.0, max: 20_000.0, default: 6_000.0 },
    NaturalParam { name: "duck", unit: "%", min: 0.0, max: 100.0, default: 0.0 },
    NaturalParam { name: "width", unit: "%", min: 0.0, max: 200.0, default: 100.0 },
    NaturalParam {
        name: "heads",
        unit: "",
        min: 0.0,
        max: (HEAD_SETS.len() - 1) as f32,
        default: 0.0,
    },
    NaturalParam { name: "wander", unit: "%", min: 0.0, max: 100.0, default: 100.0 },
    NaturalParam { name: "mix", unit: "%", min: 0.0, max: 100.0, default: 22.0 },
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

/// The mode a parameter vector names.
#[must_use]
pub fn mode_of(params: &[f32]) -> Mode {
    Mode::from_index(at(params, PARAM_MODE).round().max(0.0) as usize)
}

/// The routing a parameter vector names.
#[must_use]
pub fn routing_of(params: &[f32]) -> Routing {
    Routing::from_index(at(params, PARAM_ROUTING).round().max(0.0) as usize)
}

/// The time-change behaviour a parameter vector names, before it is resolved
/// against the mode.
#[must_use]
pub fn time_mode_of(params: &[f32]) -> TimeMode {
    TimeMode::from_index(at(params, PARAM_TIME_MODE).round().max(0.0) as usize)
}

/// The head combination a parameter vector names.
#[must_use]
pub fn heads_of(params: &[f32]) -> usize {
    (at(params, PARAM_HEADS).round().max(0.0) as usize).min(HEAD_SETS.len() - 1)
}

/// Whether the clock is following the transport.
#[must_use]
pub fn is_synced(params: &[f32]) -> bool {
    at(params, PARAM_SYNC) >= 0.5
}

fn at(params: &[f32], index: usize) -> f32 {
    params.get(index).copied().unwrap_or(0.0)
}

/// Whether a control does anything at these settings.
///
/// Three controls are conditional, and each one is conditional on something a
/// player can see on the same panel: `div` and `time` are the two halves of
/// one clock and only one of them is live at a time, `heads` is a tape
/// transport's and nothing else has three of them, and `wander` is the
/// bucket-brigade clock's drift. Everything else is always live.
///
/// The panel greys what this refuses and the keys refuse to move it, so a
/// control that reads as inert is inert.
#[must_use]
pub fn uses(params: &[f32], index: usize) -> bool {
    match index {
        PARAM_DIVISION => is_synced(params),
        PARAM_TIME_MS => !is_synced(params),
        PARAM_HEADS => mode_of(params) == Mode::Tape,
        PARAM_WANDER => mode_of(params) == Mode::Bbd,
        _ => true,
    }
}

/// How many repeats it takes to fall 60 dB at a feedback setting.
///
/// Worth more on a panel than any taper cleverness: `fb 45% · ~9 repeats` says
/// what the number does. Answers `None` at and above unity, where the honest
/// answer is "it does not stop".
#[must_use]
pub fn repeats_to_silence(feedback_percent: f32) -> Option<f64> {
    let fb = f64::from(feedback_percent) / 100.0;
    if fb <= 0.0 || fb >= 1.0 {
        return None;
    }
    Some(60.0 / (-20.0 * fb.log10()))
}

/// The corner the bucket-brigade loop filter sits at for a delay time.
///
/// `f_clk = N/(2τ)` with 4096 stages, and the loop's anti-alias filter a third
/// of the way up it. Published as a function because it is what makes BBD mode
/// a different effect rather than a darker one, and a test has to be able to
/// ask for the number rather than re-derive it.
#[must_use]
pub fn bbd_corner_hz(delay_seconds: f64) -> f64 {
    if !delay_seconds.is_finite() || delay_seconds <= 0.0 {
        return BBD_LP_MAX_HZ;
    }
    let clock = BBD_STAGES / (2.0 * delay_seconds);
    (clock / BBD_CLOCK_DIVISOR).clamp(BBD_LP_MIN_HZ, BBD_LP_MAX_HZ)
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

/// Rational tanh: one divide instead of a library call, within 0.5% of the
/// real thing over the range it is defined on.
///
/// The Padé form is a tanh up to ±3, where it reaches exactly 1 with exactly
/// zero slope — and past that it climbs again, which inside a feedback loop
/// would break the bound this whole file rests on. Clamping the input at 3 is
/// therefore not a shortcut: it is what makes `|tanh_approx(x)| ≤ 1` true for
/// every input, which is the premise of `|sat(x)| ≤ 1/g`.
#[inline]
fn tanh_approx(x: f64) -> f64 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// The pole of a one-pole at a corner frequency, the only correct way to move
/// a filter to another sample rate.
#[inline]
fn pole_at(hz: f64, sample_rate: f64) -> f64 {
    if !(hz.is_finite() && sample_rate > 0.0) {
        return 0.0;
    }
    (-TAU * hz.max(0.0) / sample_rate).exp().clamp(0.0, 0.9999)
}

/// The integrator coefficient of a trapezoidal one-pole low-pass.
///
/// The trapezoidal form rather than the impulse-invariant one because this
/// filter sits *inside a feedback loop*: its gain at Nyquist is exactly zero
/// at every sample rate, where the naive form's is `(1−p)/(1+p)` and therefore
/// rate-dependent — and a rate-dependent number inside a loop is multiplied by
/// every circulation.
#[inline]
fn tpt_gain(hz: f64, sample_rate: f64) -> f64 {
    if !(hz.is_finite() && sample_rate > 0.0) {
        return 1.0;
    }
    let corner = hz.clamp(1.0, sample_rate * 0.49);
    let g = (PI * corner / sample_rate).tan();
    g / (1.0 + g)
}

/// A base-two logarithm accurate to a millionth of a decibel, without reaching
/// the maths library.
///
/// The ducker needs a logarithm and an exponential *per sample*, and the two
/// library calls together cost more than the entire rest of this effect.
///
/// The exponent comes out of the bit pattern for free. The mantissa is folded
/// into `[1/√2, √2)` — one compare, and it is what makes the rest cheap — and
/// then run through the `atanh` series, `log2 m = (2/ln2)·(t + t³/3 + t⁵/5 +
/// t⁷/7)` with `t = (m−1)/(m+1)`. Folding puts `|t| ≤ 0.1716`, so the first
/// dropped term is under six parts in a billion. A minimax polynomial on the
/// unfolded `[1, 2)` needs twice the degree for a hundredth of the accuracy.
///
/// `x` must be positive and finite; the one caller clamps it to `[1, 15.85]`.
#[inline]
fn log2_fast(x: f32) -> f32 {
    let bits = x.to_bits();
    let mut exponent = ((bits >> 23) & 0xff) as i32 - 127;
    let mut m = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000);
    if m > std::f32::consts::SQRT_2 {
        m *= 0.5;
        exponent += 1;
    }
    let t = (m - 1.0) / (m + 1.0);
    let t2 = t * t;
    let series = t * (1.0 + t2 * (1.0 / 3.0 + t2 * (0.2 + t2 / 7.0)));
    exponent as f32 + TWO_OVER_LN_2 * series
}

/// The `2/ln 2` that turns the `atanh` series into a base-two logarithm, and
/// the first two terms of `2^f`, written as what they are rather than as
/// decimals nobody can check.
const TWO_OVER_LN_2: f32 = 2.0 / std::f32::consts::LN_2;
const LN_2: f32 = std::f32::consts::LN_2;
const LN_2_SQUARED_HALF: f32 = LN_2 * LN_2 * 0.5;

/// Two to the power of, to a millionth, and by the same trick in reverse: the
/// whole part is written straight into the exponent field and the fraction is
/// the Taylor series of `2^f`, which converges fast enough on `[0, 1)` that
/// eight terms leave under a part in a million.
#[inline]
fn exp2_fast(x: f32) -> f32 {
    let x = x.clamp(-60.0, 60.0);
    let whole = x.floor();
    let f = x - whole;
    let poly = 1.0
        + f * (LN_2
            + f * (LN_2_SQUARED_HALF
                + f * (0.055_504_1
                    + f * (0.009_618_1
                        + f * (0.001_333_4 + f * (0.000_154_0 + f * 0.000_015_3))))));
    let scale = f32::from_bits(((whole as i32 + 127) as u32) << 23);
    scale * poly
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

/// One-pole DC blocker, its corner set from the sample rate so it is the same
/// frequency at every rate.
#[derive(Clone, Copy, Default)]
struct DcBlock {
    x1: f64,
    y1: f64,
}

impl DcBlock {
    #[inline]
    fn tick(&mut self, x: f64, a: f64) -> f64 {
        let y = x - self.x1 + a * self.y1;
        self.x1 = flush(x);
        self.y1 = flush(y);
        y
    }

    fn clear(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

/// An RBJ peaking biquad in transposed direct form II — the head bump.
#[derive(Clone, Copy)]
struct Peaking {
    b: [f64; 3],
    a: [f64; 2],
    z1: f64,
    z2: f64,
}

impl Peaking {
    fn new() -> Self {
        Self { b: [1.0, 0.0, 0.0], a: [0.0, 0.0], z1: 0.0, z2: 0.0 }
    }

    fn design(&mut self, hz: f64, q: f64, gain_db: f64, sample_rate: f64) {
        if !(hz.is_finite() && sample_rate > 0.0) {
            return;
        }
        let a = 10.0f64.powf(gain_db / 40.0);
        let w = TAU * hz.clamp(1.0, sample_rate * 0.49) / sample_rate;
        let (sin_w, cos_w) = w.sin_cos();
        let alpha = sin_w / (2.0 * q.max(0.05));
        let a0 = 1.0 + alpha / a;
        self.b = [
            (1.0 + alpha * a) / a0,
            (-2.0 * cos_w) / a0,
            (1.0 - alpha * a) / a0,
        ];
        self.a = [(-2.0 * cos_w) / a0, (1.0 - alpha / a) / a0];
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
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

/// The three filters one head's signal passes on its way round the loop.
///
/// Each enabled head gets its own set. The alternative — one chain on the
/// summed heads — would put the loop filter outside the loop for two taps out
/// of three and make head one measurably brighter than head three, which is
/// not what one piece of tape past three heads sounds like.
#[derive(Clone, Copy)]
struct LoopFilter {
    lp: f64,
    hp: f64,
    bump: Peaking,
}

impl LoopFilter {
    fn new() -> Self {
        Self { lp: 0.0, hp: 0.0, bump: Peaking::new() }
    }

    /// Low-pass then high-pass, both one-pole.
    ///
    /// One pole and not two, deliberately: a resonant filter inside a loop
    /// whose gain is near one gives a howling narrow band at a level nobody
    /// can predict.
    #[inline]
    fn tick(&mut self, x: f64, lp_g: f64, hp_a: f64) -> f64 {
        let v = (x - self.lp) * lp_g;
        let low = v + self.lp;
        self.lp = flush(low + v);
        // The high-pass is the low-pass's complement, taken from a separate
        // one-pole so the two corners are independent.
        let high_state = self.hp + (low - self.hp) * hp_a;
        self.hp = flush(high_state);
        low - high_state
    }

    fn clear(&mut self) {
        self.lp = 0.0;
        self.hp = 0.0;
        self.bump.clear();
    }
}

/// A delay line with a fractional read.
///
/// `pos` is where the *next* sample goes, so a delay of `m` samples is a read
/// of `tap(m)`: everything reads before it writes, which is one rule rather
/// than two off-by-one conventions to get wrong. The capacity is a power of
/// two so wrapping a read index is one `AND` — worth the memory on a structure
/// that is read up to six times a frame.
struct Line {
    buf: Vec<f32>,
    mask: usize,
    pos: usize,
    /// The largest `back` a read may ask for and still have its four taps
    /// inside the buffer.
    limit: f64,
}

impl Line {
    fn new(len: usize) -> Self {
        let capacity = len.max(16).next_power_of_two();
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

    #[inline]
    fn write(&mut self, x: f64) {
        let v = x as f32;
        self.buf[self.pos] = if f64::from(v).abs() < DENORMAL_FLOOR { 0.0 } else { v };
        self.pos = (self.pos + 1) & self.mask;
    }

    /// A fractional read, four-point third-order Lagrange.
    ///
    /// The four taps straddle the requested delay so the fraction always lands
    /// in the middle interval, which is the region where `|H| ≤ 1` and the
    /// kernel is therefore safe inside a feedback loop. Stateless, so a read
    /// head that moves costs no transient.
    #[inline]
    fn tap_cubic(&self, back: f64) -> f64 {
        // `max` then `min` rather than `clamp`: two instructions instead of a
        // branch, and it lands a NaN on the floor rather than in an index.
        let back = back.max(2.0).min(self.limit);
        let whole = back.floor();
        let d = back - whole;
        let base = self.pos.wrapping_sub(whole as usize);
        let mask = self.mask;
        let ym1 = f64::from(self.buf[base.wrapping_add(1) & mask]);
        let y0 = f64::from(self.buf[base & mask]);
        let y1 = f64::from(self.buf[base.wrapping_sub(1) & mask]);
        let y2 = f64::from(self.buf[base.wrapping_sub(2) & mask]);
        let c0 = -d * (d - 1.0) * (d - 2.0) / 6.0;
        let c1 = (d + 1.0) * (d - 1.0) * (d - 2.0) * 0.5;
        let c2 = -(d + 1.0) * d * (d - 2.0) * 0.5;
        let c3 = (d + 1.0) * d * (d - 1.0) / 6.0;
        c0.mul_add(ym1, c1.mul_add(y0, c2.mul_add(y1, c3 * y2)))
    }
}

/// One channel's read head, and everything the `timemode` control does.
///
/// Repitch is a slewed pointer and Fade is two pointers with a crossfade
/// between them. Both are here from the first line rather than one of them
/// being added later, because adding Repitch to a Fade-only read path is a
/// rewrite of the read path rather than an addition to it.
#[derive(Clone, Copy)]
struct Head {
    /// Where the head is now, in samples, before modulation.
    now: f64,
    /// Where it is going.
    target: f64,
    /// Where it was, for the duration of a crossfade.
    old: f64,
    fade: f64,
    fade_step: f64,
    fading: bool,
    /// A target that arrived while a crossfade was still running. Queued
    /// rather than applied, so a tempo ramp becomes a staircase of clean
    /// crossfades instead of a crossfade that restarts mid-flight.
    pending: Option<f64>,
    /// How far a repitch walks per sample, as a one-pole coefficient.
    slew: f64,
}

impl Head {
    fn new() -> Self {
        Self {
            now: 1.0,
            target: 1.0,
            old: 1.0,
            fade: 1.0,
            fade_step: 1.0,
            fading: false,
            pending: None,
            slew: 1.0,
        }
    }

    fn snap(&mut self, samples: f64) {
        self.now = samples;
        self.target = samples;
        self.old = samples;
        self.fade = 1.0;
        self.fading = false;
        self.pending = None;
    }

    /// Point the head somewhere new, in whatever way this mode asks for.
    fn aim(&mut self, samples: f64, mode: TimeMode) {
        if (samples - self.destination()).abs() < TARGET_EPSILON {
            return;
        }
        match mode {
            TimeMode::Jump => {
                self.snap(samples);
            }
            TimeMode::Repitch | TimeMode::Auto => {
                self.target = samples;
                self.fading = false;
                self.pending = None;
            }
            TimeMode::Fade => {
                if self.fading {
                    self.pending = Some(samples);
                } else {
                    self.old = self.now;
                    self.now = samples;
                    self.target = samples;
                    self.fade = 0.0;
                    self.fading = true;
                }
            }
        }
    }

    /// Where the head will end up if nothing else changes.
    fn destination(&self) -> f64 {
        self.pending.unwrap_or(if self.fading { self.now } else { self.target })
    }

    /// One sample of movement, answering the two read positions and their
    /// gains. Outside a crossfade the second gain is zero and the second read
    /// is never taken.
    #[inline]
    fn advance(&mut self, repitching: bool) -> (f64, f64, f64, f64) {
        if repitching {
            self.now += (self.target - self.now) * self.slew;
            return (self.now, 1.0, 0.0, 0.0);
        }
        if !self.fading {
            return (self.now, 1.0, 0.0, 0.0);
        }
        self.fade += self.fade_step;
        if self.fade >= 1.0 {
            self.fading = false;
            self.fade = 1.0;
            if let Some(next) = self.pending.take() {
                self.old = self.now;
                self.now = next;
                self.target = next;
                self.fade = 0.0;
                self.fading = true;
            }
        }
        // Equal power, so the sum of the two reads holds its level across the
        // cross rather than dipping in the middle of it.
        let angle = self.fade * std::f64::consts::FRAC_PI_2;
        let (new_gain, old_gain) = angle.sin_cos();
        (self.now, new_gain, self.old, old_gain)
    }
}

/// The slope-bounded random walk the bucket-brigade clock drifts on.
struct Wander {
    state: u32,
    stride: u32,
    from: f64,
    to: f64,
    phase: f64,
}

impl Wander {
    fn new(seed: u32) -> Self {
        Self { state: mix32(seed), stride: seed | 1, from: 0.0, to: 0.0, phase: 0.0 }
    }

    #[inline]
    fn next_target(&mut self) -> f64 {
        self.state = self.state.wrapping_add(self.stride);
        f64::from(mix32(self.state) >> 8) / f64::from(1u32 << 23) - 1.0
    }

    /// A drift in ±1 with no high end at all: a new random target `hz` times a
    /// second and a smoothstep in between, so the value is continuous, its
    /// first derivative is continuous, and the fastest it can move is
    /// `1.5 · span · hz` per second.
    #[inline]
    fn tick(&mut self, hz: f64, sample_rate: f64) -> f64 {
        self.phase += hz / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            self.from = self.to;
            self.to = self.next_target();
        }
        let t = self.phase;
        self.from + (self.to - self.from) * t * t * (3.0 - 2.0 * t)
    }

    fn clear(&mut self) {
        self.from = 0.0;
        self.to = 0.0;
        self.phase = 0.0;
    }
}

#[inline]
fn mix32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

// ---------------------------------------------------------------------------
// The delay
// ---------------------------------------------------------------------------

/// A stereo delay: mode, routing, a synced clock and a bounded loop.
pub struct Delay {
    sample_rate: f64,
    params: [f32; PARAM_COUNT],

    // ── The lines ──
    line: [Line; 2],
    filters: [[LoopFilter; 3]; 2],
    dc: [DcBlock; 2],
    compander: [f64; 2],

    // ── The read heads ──
    head: [Head; 2],

    // ── Modulation ──
    wow_phase: f64,
    flutter_phase: f64,
    scrape_phase: f64,
    /// The two head-local excursions, in samples at this rate.
    flutter_samples: f64,
    scrape_samples: f64,
    wander: Wander,

    // ── Ducking ──
    duck_env: f64,
    duck_attack: f64,
    duck_release: f64,

    // ── Per-block state, resolved from the tempo and the controls ──
    mode: Mode,
    routing: Routing,
    resolved_time_mode: TimeMode,
    base_seconds: f64,
    heads: [bool; 3],
    head_norm: f64,
    longest_head: usize,
    lp_g: f64,
    hp_a: f64,
    bbd_lp_g: f64,
    drive: f64,
    inv_drive: f64,
    frozen: bool,
    wander_span: f64,
    dc_a: f64,
    compand_a: f64,
    /// Set by [`Delay::snap`]: the next block points the read heads straight
    /// at the tempo it is handed rather than crossfading to it. A session load
    /// resolves its grid against a placeholder tempo before the effect is in a
    /// slot, and the first real block should not spend twenty milliseconds
    /// arriving at the delay time the file asked for.
    snap_heads: bool,

    // ── Smoothed controls ──
    smooth_a: f64,
    mix: Smoother,
    width: Smoother,
    feedback: Smoother,
    duck: Smoother,
}

impl Delay {
    /// Build one at a sample rate, with every buffer it will ever need.
    #[must_use]
    pub fn new(sample_rate: f64) -> Self {
        let mut delay = Self {
            sample_rate: 48_000.0,
            params: default_natural_params(),
            line: [Line::new(16), Line::new(16)],
            filters: [[LoopFilter::new(); 3]; 2],
            dc: [DcBlock::default(); 2],
            compander: [0.0; 2],
            head: [Head::new(); 2],
            wow_phase: 0.0,
            flutter_phase: 0.0,
            scrape_phase: 0.0,
            flutter_samples: 0.0,
            scrape_samples: 0.0,
            wander: Wander::new(0x51ED_2701),
            duck_env: 0.0,
            duck_attack: 1.0,
            duck_release: 1.0,
            mode: Mode::Digital,
            routing: Routing::Stereo,
            resolved_time_mode: TimeMode::Fade,
            base_seconds: 0.375,
            heads: [true, false, false],
            head_norm: 1.0,
            longest_head: 0,
            lp_g: 1.0,
            hp_a: 0.0,
            bbd_lp_g: 1.0,
            drive: DRIVE_DIGITAL,
            inv_drive: 1.0,
            frozen: false,
            wander_span: BBD_WANDER_SPAN,
            dc_a: 0.999,
            compand_a: 0.01,
            snap_heads: true,
            smooth_a: 1.0,
            mix: Smoother::default(),
            width: Smoother::default(),
            feedback: Smoother::default(),
            duck: Smoother::default(),
        };
        delay.build(sample_rate);
        delay.snap();
        delay
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

        // The line, with margin for the modulation to move the read head into.
        let frames = (fs * MAX_DELAY_S * (1.0 + LINE_SPAN)).ceil() as usize + 8;
        self.line = [Line::new(frames), Line::new(frames)];

        self.smooth_a = 1.0 - (-1.0 / (SMOOTH_SECONDS * fs)).exp();
        self.dc_a = pole_at(DC_BLOCK_HZ, fs);
        self.compand_a = 1.0 - pole_at(BBD_COMPAND_HZ, fs);
        self.duck_attack = 1.0 - (-1.0 / (DUCK_ATTACK_S * fs)).exp();
        self.duck_release = 1.0 - (-1.0 / (DUCK_RELEASE_S * fs)).exp();
        self.flutter_samples = excursion_seconds(TAPE_FLUTTER.0, TAPE_FLUTTER.1) * fs;
        self.scrape_samples = excursion_seconds(TAPE_SCRAPE.0, TAPE_SCRAPE.1) * fs;

        for channel in &mut self.filters {
            for filter in channel.iter_mut() {
                filter.bump.design(TAPE_BUMP_HZ, TAPE_BUMP_Q, TAPE_BUMP_DB, fs);
            }
        }
        for head in &mut self.head {
            head.fade_step = 1.0 / (FADE_SECONDS * fs);
            head.slew = 1.0 - (-1.0 / (SLEW_DIGITAL_S * fs)).exp();
        }
        self.resolve(120.0);
        self.reset();
    }

    /// Drop every tail: lines to silence, filters and detectors to rest.
    pub fn reset(&mut self) {
        for line in &mut self.line {
            line.clear();
        }
        for channel in &mut self.filters {
            for filter in channel.iter_mut() {
                filter.clear();
            }
        }
        for block in &mut self.dc {
            block.clear();
        }
        self.compander = [0.0; 2];
        self.wow_phase = 0.0;
        self.flutter_phase = 0.0;
        self.scrape_phase = 0.0;
        self.wander.clear();
        self.duck_env = 0.0;
        let samples = (self.base_seconds * self.sample_rate).max(2.0);
        self.head[0].snap(samples);
        self.head[1].snap(samples);
    }

    /// Take every smoothed control straight to its target.
    ///
    /// A session load sets the controls before the effect is in a slot, and
    /// those controls are glide targets. Snapping them means the first block a
    /// loaded session renders is the delay that was saved rather than the
    /// factory one gliding towards it.
    pub fn snap(&mut self) {
        self.mix.snap(self.mix.target);
        self.width.snap(self.width.target);
        self.feedback.snap(self.feedback.target);
        self.duck.snap(self.duck.target);
        let samples = (self.base_seconds * self.sample_rate).max(2.0);
        self.head[0].snap(samples);
        self.head[1].snap(samples * self.offset_factor());
        self.snap_heads = true;
    }

    // ── Parameters ──

    /// One control, in its own unit. Real-time safe.
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

    /// The mode the delay is running.
    #[must_use]
    pub fn mode(&self) -> Mode {
        mode_of(&self.params)
    }

    /// The routing it is running.
    #[must_use]
    pub fn routing(&self) -> Routing {
        routing_of(&self.params)
    }

    /// The delay time in force, in seconds, at the last tempo it was told.
    #[must_use]
    pub fn delay_seconds(&self) -> f64 {
        self.base_seconds
    }

    /// What `timemode` resolved to on this mode.
    #[must_use]
    pub fn time_behaviour(&self) -> TimeMode {
        self.resolved_time_mode
    }

    fn push_targets(&mut self) {
        self.mix.target = f64::from(self.params[PARAM_MIX]) / 100.0;
        self.width.target = f64::from(self.params[PARAM_WIDTH]) / 100.0;
        self.feedback.target = f64::from(self.params[PARAM_FEEDBACK]) / 100.0;
        self.duck.target = f64::from(self.params[PARAM_DUCK]) / 100.0;
    }

    fn offset_factor(&self) -> f64 {
        if routing_of(&self.params) == Routing::Mono {
            1.0
        } else {
            1.0 + f64::from(self.params[PARAM_OFFSET]) / 100.0
        }
    }

    /// Everything that is settled once per block: the tempo, the mode, the
    /// filter coefficients and where the read heads are pointed.
    fn resolve(&mut self, tempo_bpm: f64) {
        let params = self.params;
        self.mode = mode_of(&params);
        self.routing = routing_of(&params);
        self.resolved_time_mode = time_mode_of(&params).resolve(self.mode);
        self.frozen = params[PARAM_FREEZE] >= 0.5;
        self.drive = self.mode.drive();
        self.inv_drive = 1.0 / self.drive;

        let heads_index = heads_of(&params);
        self.heads = if self.mode == Mode::Tape { head_set(heads_index) } else { [true, false, false] };
        let enabled = self.heads.iter().filter(|on| **on).count().max(1);
        // Power-preserving rather than amplitude-preserving: three heads read
        // three different pieces of tape, which are uncorrelated by the time
        // the second one has anything on it.
        self.head_norm = 1.0 / (enabled as f64).sqrt();
        self.longest_head =
            self.heads.iter().rposition(|on| *on).unwrap_or(0);

        // The time, from whichever clock is running.
        let mut seconds = if is_synced(&params) {
            synced_seconds(params[PARAM_DIVISION].round().max(0.0) as usize, tempo_bpm).0
        } else {
            f64::from(params[PARAM_TIME_MS]) / 1000.0
        };
        if self.mode == Mode::Tape {
            // A five-second line cannot hold a third head at three times a
            // five-second base. Clamping the *base* is what keeps the head
            // ratios exactly 1 : 2 : 3 instead of silently collapsing them.
            seconds = seconds.min(MAX_DELAY_S / head_span(heads_index));
        }
        self.base_seconds = seconds.clamp(MIN_DELAY_S, MAX_DELAY_S);

        let samples = (self.base_seconds * self.sample_rate).max(2.0);
        let slew = 1.0 - (-1.0 / (self.mode.slew_seconds() * self.sample_rate)).exp();
        for head in &mut self.head {
            head.slew = slew;
        }
        let offset = self.offset_factor();
        let behaviour =
            if self.snap_heads { TimeMode::Jump } else { self.resolved_time_mode };
        self.snap_heads = false;
        self.head[0].aim(samples, behaviour);
        self.head[1].aim(samples * offset, behaviour);

        // The loop filters. In BBD mode the clock's own anti-alias corner
        // rides on top of whatever the player asked for, which is the whole
        // of why longer means darker there and nowhere else.
        self.lp_g = tpt_gain(f64::from(params[PARAM_HIGH_CUT_HZ]), self.sample_rate);
        self.hp_a = 1.0 - pole_at(f64::from(params[PARAM_LOW_CUT_HZ]), self.sample_rate);
        self.bbd_lp_g = tpt_gain(bbd_corner_hz(self.base_seconds), self.sample_rate);
        self.wander_span = BBD_WANDER_SPAN * f64::from(params[PARAM_WANDER]) / 100.0;

        self.push_targets();
    }

    // ── Rendering ──

    /// Rewrite one block in place, at a tempo.
    ///
    /// The tempo is read once per block and the grid is resolved from it here,
    /// so a delay follows a tempo automation ramp rather than lagging it by
    /// however long a UI takes to notice. The re-resolve goes through
    /// `timemode`, so a ramp does not click.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], tempo_bpm: f64) {
        self.resolve(tempo_bpm);
        let frames = left.len().min(right.len());
        for i in 0..frames {
            let (l, r) = self.process_sample(left[i], right[i]);
            left[i] = l;
            right[i] = r;
        }
    }

    /// One frame.
    ///
    /// At `mix == 0` the lines still run — a tail must not glitch when the
    /// knob comes back — and the input is returned *itself* rather than the
    /// input plus zero times the wet. Bit-identical dry is a property of the
    /// control flow rather than of floating-point luck, and `−0.0 + 0.0` is
    /// `+0.0`, which is why the difference matters.
    #[inline]
    pub fn process_sample(&mut self, left: f32, right: f32) -> (f32, f32) {
        let a = self.smooth_a;
        let mix = self.mix.advance(a);
        let width = self.width.advance(a);
        let feedback = self.feedback.advance(a);
        let duck_amount = self.duck.advance(a);

        let dry_l = f64::from(left);
        let dry_r = f64::from(right);
        let mono = (dry_l + dry_r) * 0.5;

        // What goes into each line, before the feedback is added to it.
        let (in_l, in_r) = match self.routing {
            Routing::Stereo => (dry_l, dry_r),
            // A straight sum, not an equal-power one: correlated material is
            // what a mono sum is usually given, and −3 dB of it is a level
            // change nobody asked for.
            Routing::PingPong => (mono, 0.0),
            Routing::Mono => (mono, mono),
        };

        let modulation = self.modulation();

        let (wet_l, fb_l) = self.read_channel(0, modulation);
        let (wet_r, fb_r) = self.read_channel(1, modulation);

        // Ping-pong crosses the feedback, which is what makes the repeats
        // alternate. The input enters on the left only, so the first repeat is
        // left — fixed, because a delay whose first repeat moves is a delay
        // whose rhythm moves.
        let (src_l, src_r) = match self.routing {
            Routing::PingPong => (fb_r, fb_l),
            _ => (fb_l, fb_r),
        };

        let (write_l, write_r) = if self.frozen {
            // Loop gain exactly one, saturator and filters out of the path,
            // input gain zero. Anything else and an "endlessly cycling" buffer
            // darkens and quietens away, which is not what the word means.
            (src_l, src_r)
        } else {
            (
                in_l + feedback * self.saturate(0, src_l),
                in_r + feedback * self.saturate(1, src_r),
            )
        };
        self.line[0].write(write_l);
        self.line[1].write(write_r);

        // ── The wet, on its way out ──
        //
        // Mid/side on the wet only. A width control that narrows the dry is a
        // bug report waiting to be filed.
        let (mut out_l, mut out_r) = if self.routing == Routing::Mono {
            let m = (wet_l + wet_r) * 0.5;
            (m, m)
        } else {
            let mid = (wet_l + wet_r) * 0.5;
            let side = (wet_l - wet_r) * 0.5 * width;
            (mid + side, mid - side)
        };

        if duck_amount > 0.0 {
            let gain = self.duck_gain(mono.abs(), duck_amount);
            out_l *= gain;
            out_r *= gain;
        } else {
            // The detector still runs, so turning the knob up does not start
            // from a cold envelope in the middle of a phrase.
            self.duck_gain(mono.abs(), 0.0);
        }

        if mix == 0.0 {
            return (left, right);
        }
        // A crossfade, not an addition. `dry + wet·mix` looks like the same
        // control and is not: at 100% it is *dry plus a full delay*, so a send
        // bus set to fully wet returns the source a second time a few
        // milliseconds late — which is the phasey-send trap, and the reason a
        // player who tries a send once never tries it again.
        let dry = 1.0 - mix as f32;
        (
            (out_l as f32).mul_add(mix as f32, left * dry),
            (out_r as f32).mul_add(mix as f32, right * dry),
        )
    }

    /// What the modulation does to the read offset this sample: a multiplier
    /// on the delay time, and an excursion in samples on top of it.
    ///
    /// The two are not interchangeable — see [`TAPE_WOW`] for why the capstan
    /// scales the delay and the head does not.
    ///
    /// The additive pair uses `Δτ·(1 − cos)` rather than a centred sine, which
    /// puts the excursion on `[0, 2Δτ]` — minimum offset zero — while leaving
    /// the *pitch* modulation exactly symmetric, because pitch is `−dτ/dt` and
    /// the derivative of `Δτ(1 − cos 2πft)` is the `D·sin(2πft)` that was
    /// asked for. A centred sine would need a positive offset at least as
    /// large as its deepest negative excursion, and that offset is latency.
    #[inline]
    fn modulation(&mut self) -> (f64, f64) {
        match self.mode {
            Mode::Digital => (1.0, 0.0),
            Mode::Bbd => (
                1.0 + self.wander.tick(BBD_WANDER_HZ, self.sample_rate) * self.wander_span,
                0.0,
            ),
            Mode::Tape => {
                let fs = self.sample_rate;
                self.wow_phase = wrap_phase(self.wow_phase + TAPE_WOW.1 / fs);
                self.flutter_phase = wrap_phase(self.flutter_phase + TAPE_FLUTTER.1 / fs);
                self.scrape_phase = wrap_phase(self.scrape_phase + TAPE_SCRAPE.1 / fs);
                let multiplier = 1.0 + TAPE_WOW.0 * (TAU * self.wow_phase).sin();
                let additive = self.flutter_samples
                    * (1.0 - (TAU * self.flutter_phase).cos())
                    + self.scrape_samples * (1.0 - (TAU * self.scrape_phase).cos());
                (multiplier, additive)
            }
        }
    }

    /// One channel's heads, filtered: the wet the output takes and the tap the
    /// loop takes.
    #[inline]
    fn read_channel(&mut self, channel: usize, modulation: (f64, f64)) -> (f64, f64) {
        let (now, gain_now, old, gain_old) =
            self.head[channel].advance(self.resolved_time_mode == TimeMode::Repitch);
        let (multiplier, additive) = modulation;

        let mut wet = 0.0;
        let mut loop_tap = 0.0;
        for (head, base_ratio) in HEAD_RATIOS.iter().enumerate() {
            if !self.heads[head] {
                continue;
            }
            let ratio = base_ratio * multiplier;
            let mut tap = self.line[channel].tap_cubic(now * ratio + additive) * gain_now;
            if gain_old > 0.0 {
                tap += self.line[channel].tap_cubic(old * ratio + additive) * gain_old;
            }
            if !self.frozen {
                tap = self.colour(channel, head, tap);
            }
            wet += tap;
            if head == self.longest_head {
                loop_tap = tap;
            }
        }
        (wet * self.head_norm, loop_tap)
    }

    /// The filtering and the character one tap picks up on its way round.
    #[inline]
    fn colour(&mut self, channel: usize, head: usize, tap: f64) -> f64 {
        let lp_g = if self.mode == Mode::Bbd { self.lp_g.min(self.bbd_lp_g) } else { self.lp_g };
        let mut y = self.filters[channel][head].tick(tap, lp_g, self.hp_a);
        match self.mode {
            Mode::Bbd => {
                // Compand around the line, and the line's own third-order
                // nonlinearity inside it. The compander's audible contribution
                // here is the level-dependence of the grit, because a digital
                // line has no noise floor for it to fix.
                let env = &mut self.compander[channel];
                *env += (y.abs() - *env) * self.compand_a;
                let gain =
                    (1.0 / (*env + BBD_COMPAND_FLOOR)).clamp(BBD_COMPAND_MIN, BBD_COMPAND_MAX);
                let x = (y * gain).clamp(-1.5, 1.5);
                y = (x - BBD_SHAPE_A * x * x - BBD_SHAPE_B * x * x * x) / gain;
            }
            Mode::Tape => {
                y = self.filters[channel][head].bump.tick(y);
            }
            Mode::Digital => {}
        }
        y
    }

    /// The last thing in the loop, and the reason the feedback range is what
    /// it is. See the module documentation for the bound.
    #[inline]
    fn saturate(&mut self, channel: usize, x: f64) -> f64 {
        // The DC blocker goes *before* the saturator so that the saturator is
        // strictly last: anything after it turns `|line| ≤ |in| + fb/g` from
        // an equality-tight bound into an approximate one.
        let blocked = self.dc[channel].tick(x, self.dc_a);
        tanh_approx(self.drive * blocked) * self.inv_drive
    }

    /// The wet's gain, from the dry input's own envelope.
    ///
    /// Threshold-less: the envelope is normalised against a fixed floor and
    /// the reduction that implies is scaled by the knob, to a maximum of 24 dB.
    /// The key is the device's **dry input**, mono-summed and pre-delay —
    /// never the track's output, which contains the wet and would make the
    /// ducker key off itself.
    #[inline]
    fn duck_gain(&mut self, key: f64, amount: f64) -> f64 {
        let coefficient = if key > self.duck_env { self.duck_attack } else { self.duck_release };
        self.duck_env += (key - self.duck_env) * coefficient;
        self.duck_env = flush(self.duck_env);
        if amount <= 0.0 {
            return 1.0;
        }
        let ratio = (self.duck_env / DUCK_FLOOR).clamp(1.0, DUCK_CEILING) as f32;
        // `ratio^(−amount)`, which is exactly `−amount · 20·log10(ratio)`
        // decibels of reduction, without two library calls per sample.
        f64::from(exp2_fast(-(amount as f32) * log2_fast(ratio)))
    }
}

#[inline]
fn wrap_phase(phase: f64) -> f64 {
    if phase >= 1.0 {
        phase - phase.floor()
    } else {
        phase
    }
}

impl Default for Delay {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const FS: f64 = 48_000.0;
    /// The block the tests render in, and the one the engine defaults to.
    const BLOCK: usize = 64;

    /// A delay with the dry taken out, so what comes back is the wet.
    pub(crate) fn wet_only(fs: f64) -> Delay {
        let mut delay = Delay::new(fs);
        delay.set_param_natural(PARAM_MIX, 100.0);
        delay.snap();
        delay
    }

    /// The same, with the loop filters as near to transparent as their travel
    /// allows — what every measurement of the *loop* rather than of the
    /// filters wants.
    pub(crate) fn open_loop(fs: f64) -> Delay {
        let mut delay = wet_only(fs);
        delay.set_param_natural(PARAM_LOW_CUT_HZ, 20.0);
        delay.set_param_natural(PARAM_HIGH_CUT_HZ, 20_000.0);
        delay
    }

    pub(crate) fn set(delay: &mut Delay, index: usize, value: f32) {
        delay.set_param_natural(index, value);
    }

    /// Push a mono signal through, in blocks, at a tempo.
    pub(crate) fn render(delay: &mut Delay, input: &[f32], bpm: f64) -> (Vec<f32>, Vec<f32>) {
        render_stereo(delay, input, input, bpm)
    }

    pub(crate) fn render_stereo(
        delay: &mut Delay,
        left: &[f32],
        right: &[f32],
        bpm: f64,
    ) -> (Vec<f32>, Vec<f32>) {
        let frames = left.len().min(right.len());
        let mut out_l = Vec::with_capacity(frames);
        let mut out_r = Vec::with_capacity(frames);
        let mut at = 0;
        while at < frames {
            let end = (at + BLOCK).min(frames);
            let mut block_l = left[at..end].to_vec();
            let mut block_r = right[at..end].to_vec();
            delay.process(&mut block_l, &mut block_r, bpm);
            out_l.extend_from_slice(&block_l);
            out_r.extend_from_slice(&block_r);
            at = end;
        }
        (out_l, out_r)
    }

    /// One impulse, then silence.
    pub(crate) fn impulse(frames: usize) -> Vec<f32> {
        let mut input = vec![0.0f32; frames];
        input[0] = 1.0;
        input
    }

    /// A tone burst of `hz` for `burst_s`, then silence.
    pub(crate) fn burst(hz: f64, amplitude: f64, burst_s: f64, total_s: f64, fs: f64) -> Vec<f32> {
        let frames = (total_s * fs) as usize;
        let driven = (burst_s * fs) as usize;
        (0..frames)
            .map(|n| {
                if n < driven {
                    (amplitude * (TAU * hz * n as f64 / fs).sin()) as f32
                } else {
                    0.0
                }
            })
            .collect()
    }

    /// Two tones at once, for measuring what a repeat did to the top against
    /// what it did to the middle.
    pub(crate) fn two_tone(
        low: f64,
        high: f64,
        amplitude: f64,
        burst_s: f64,
        total_s: f64,
        fs: f64,
    ) -> Vec<f32> {
        let frames = (total_s * fs) as usize;
        let driven = (burst_s * fs) as usize;
        (0..frames)
            .map(|n| {
                if n < driven {
                    let t = n as f64 / fs;
                    (amplitude * ((TAU * low * t).sin() + (TAU * high * t).sin())) as f32
                } else {
                    0.0
                }
            })
            .collect()
    }

    pub(crate) fn peak(x: &[f32]) -> f64 {
        x.iter().map(|v| f64::from(v.abs())).fold(0.0, f64::max)
    }

    pub(crate) fn rms(x: &[f32]) -> f64 {
        if x.is_empty() {
            return 0.0;
        }
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64).sqrt()
    }

    /// The amplitude of one frequency in a window, by a single DFT bin.
    ///
    /// No FFT crate: one bin is all any of these measurements ask for, and a
    /// Hann window keeps a neighbouring tone out of the answer.
    pub(crate) fn tone_amplitude(x: &[f32], hz: f64, fs: f64) -> f64 {
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

    /// Where the energy in a window lands, in samples from its start, by a
    /// parabolic fit around the largest sample.
    pub(crate) fn arrival(x: &[f32], from: usize, to: usize) -> f64 {
        let to = to.min(x.len());
        if to <= from + 2 {
            return 0.0;
        }
        let mut best = from;
        for index in from..to {
            if x[index].abs() > x[best].abs() {
                best = index;
            }
        }
        if best == 0 || best + 1 >= x.len() {
            return best as f64;
        }
        let (a, b, c) = (
            f64::from(x[best - 1].abs()),
            f64::from(x[best].abs()),
            f64::from(x[best + 1].abs()),
        );
        let denominator = a - 2.0 * b + c;
        let shift = if denominator.abs() > 1.0e-12 { 0.5 * (a - c) / denominator } else { 0.0 };
        best as f64 + shift.clamp(-1.0, 1.0)
    }


    /// The instantaneous frequency of a tone, by quadrature demodulation.
    ///
    /// Zero-crossing intervals are the obvious estimator and they are not good
    /// enough here: at 5 kHz and 48 kHz there are nine samples in a cycle, and
    /// the interpolated crossing carries a quarter of a percent of noise —
    /// which is the same size as the wow being measured. Mixing the tone down
    /// to baseband against its own carrier, low-passing hard, and
    /// differentiating the unwrapped phase leaves a noise floor at the
    /// arithmetic's own precision.
    ///
    /// Answers the deviation from the carrier, per sample, as a fraction.
    pub(crate) fn fm_deviation(x: &[f32], carrier_hz: f64, fs: f64) -> Vec<f64> {
        // A one-pole at 120 Hz on each quadrature arm: well above the fastest
        // modulation being looked for and well below the carrier.
        let a = 1.0 - (-TAU * 120.0 / fs).exp();
        let (mut i_state, mut q_state) = (0.0f64, 0.0f64);
        let mut phases = Vec::with_capacity(x.len());
        for (n, sample) in x.iter().enumerate() {
            let phase = TAU * carrier_hz * n as f64 / fs;
            let v = f64::from(*sample);
            i_state += (v * phase.cos() - i_state) * a;
            q_state += (-v * phase.sin() - q_state) * a;
            phases.push(q_state.atan2(i_state));
        }
        let mut unwrapped = Vec::with_capacity(phases.len());
        let mut offset = 0.0f64;
        for pair in phases.windows(2) {
            let mut step = pair[1] - pair[0];
            if step > PI {
                step -= TAU;
            } else if step < -PI {
                step += TAU;
            }
            offset += step;
            unwrapped.push(offset);
        }
        // The derivative, as a fraction of the carrier.
        unwrapped
            .windows(2)
            .map(|pair| (pair[1] - pair[0]) * fs / TAU / carrier_hz)
            .collect()
    }


    /// How much of a track sits at one rate, by a single DFT bin.
    pub(crate) fn track_component(track: &[f64], hz: f64, fs: f64) -> f64 {
        let n = track.len();
        if n < 16 {
            return 0.0;
        }
        let mean = track.iter().sum::<f64>() / n as f64;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (index, value) in track.iter().enumerate() {
            let w = 0.5 - 0.5 * (TAU * index as f64 / n as f64).cos();
            let phase = TAU * hz * index as f64 / fs;
            re += (value - mean) * w * phase.cos();
            im -= (value - mean) * w * phase.sin();
        }
        2.0 * (re * re + im * im).sqrt() / (n as f64 * 0.5)
    }



    // ── The control surface ──

    /// The published table is what the effect answers with — sixteen
    /// controls, in natural units, at the house's own defaults.
    #[test]
    fn the_control_surface_is_the_published_one() {
        let delay = Delay::new(FS);
        assert_eq!(PARAM_COUNT, 16);
        assert!(natural_param(PARAM_COUNT).is_none());

        let expected: [(&str, &str, f32); PARAM_COUNT] = [
            ("mode", "", 0.0),
            ("route", "", 0.0),
            ("sync", "", 1.0),
            ("div", "", 7.0),
            ("time", "ms", 375.0),
            ("offset", "%", 0.0),
            ("tmode", "", 0.0),
            ("fb", "%", 30.0),
            ("freeze", "", 0.0),
            ("locut", "Hz", 200.0),
            ("hicut", "Hz", 6_000.0),
            ("duck", "%", 0.0),
            ("width", "%", 100.0),
            ("heads", "", 0.0),
            ("wander", "%", 100.0),
            ("mix", "%", 22.0),
        ];
        for (index, (name, unit, default)) in expected.iter().enumerate() {
            let info = natural_param(index).expect("a control at every index");
            assert_eq!((info.name, info.unit, info.default), (*name, *unit, *default), "index {index}");
            assert_eq!(delay.param_natural(index), *default, "index {index} value");
            assert_eq!(param_name(index), *name);
        }
        // The two the brief argues for out loud.
        assert_eq!(natural_param(PARAM_FEEDBACK).unwrap().max, 200.0, "the loop is bounded, so it goes past unity");
        assert_eq!(SYNC_LABELS[SYNC_DEFAULT], "1/8D");
        assert_eq!(default_natural_params(), delay.params);
    }

    /// The two axes are independent: nine combinations, all reachable, and
    /// choosing a mode never moves the routing.
    #[test]
    fn mode_and_routing_are_two_axes() {
        let mut delay = Delay::new(FS);
        for mode in Mode::ALL {
            for routing in Routing::ALL {
                set(&mut delay, PARAM_MODE, mode.index() as f32);
                set(&mut delay, PARAM_ROUTING, routing.index() as f32);
                assert_eq!(delay.mode(), mode);
                assert_eq!(delay.routing(), routing);
            }
        }
        assert_eq!(Mode::from_index(99), Mode::Tape);
        assert_eq!(Routing::from_index(99), Routing::Mono);
        assert_eq!(TimeMode::from_index(99), TimeMode::Jump);
        // Auto is not a fourth behaviour, it is the mode's own answer.
        assert_eq!(TimeMode::Auto.resolve(Mode::Digital), TimeMode::Fade);
        assert_eq!(TimeMode::Auto.resolve(Mode::Bbd), TimeMode::Repitch);
        assert_eq!(TimeMode::Auto.resolve(Mode::Tape), TimeMode::Repitch);
        assert_eq!(TimeMode::Fade.resolve(Mode::Tape), TimeMode::Fade);
    }

    /// The three conditional controls are conditional on what the panel shows,
    /// and nothing else is conditional at all.
    #[test]
    fn only_the_conditional_controls_are_conditional() {
        let mut params = default_natural_params();
        assert!(uses(&params, PARAM_DIVISION), "sync is on, so the grid is live");
        assert!(!uses(&params, PARAM_TIME_MS), "sync is on, so the free time is not");
        assert!(!uses(&params, PARAM_HEADS), "digital mode has no heads");
        assert!(!uses(&params, PARAM_WANDER), "digital mode has no clock to drift");

        params[PARAM_SYNC] = 0.0;
        assert!(!uses(&params, PARAM_DIVISION));
        assert!(uses(&params, PARAM_TIME_MS));

        params[PARAM_MODE] = Mode::Tape.index() as f32;
        assert!(uses(&params, PARAM_HEADS));
        assert!(!uses(&params, PARAM_WANDER));

        params[PARAM_MODE] = Mode::Bbd.index() as f32;
        assert!(!uses(&params, PARAM_HEADS));
        assert!(uses(&params, PARAM_WANDER));

        for index in 0..PARAM_COUNT {
            if !matches!(index, PARAM_DIVISION | PARAM_TIME_MS | PARAM_HEADS | PARAM_WANDER) {
                assert!(uses(&params, index), "control {index} should always be live");
            }
        }
    }

    // ── The grid ──

    /// **The echo lands on the beat, exactly.**
    ///
    /// Three tempos and four divisions, against the transport's own
    /// arithmetic rather than against a table copied out of it: the delay
    /// resolves `beats · 60/bpm` and nothing rounds it to a buffer size on the
    /// way. Sixteen divisions at five tempos is eighty numbers and they are
    /// all checked; the four in the name are the ones a player uses.
    #[test]
    fn the_grid_is_the_transports_own_arithmetic() {
        let mut delay = wet_only(FS);
        for bpm in [78.0, 100.0, 120.0, 137.0, 174.0] {
            for division in 0..SYNC_COUNT {
                let mut left = vec![0.0f32; BLOCK];
                let mut right = vec![0.0f32; BLOCK];
                set(&mut delay, PARAM_DIVISION, division as f32);
                delay.process(&mut left, &mut right, bpm);

                let (wanted, halvings) = synced_seconds(division, bpm);
                let raw = SYNC_BEATS[division] * 60.0 / bpm;
                assert_eq!(wanted, raw / f64::from(1u32 << halvings), "{bpm} bpm, {division}");
                assert!(
                    (delay.delay_seconds() - wanted).abs() < 1.0e-12,
                    "{bpm} bpm at {}: resolved {} s against {wanted} s",
                    SYNC_LABELS[division],
                    delay.delay_seconds()
                );
            }
        }
    }

    /// The read head is where the grid says it is, in samples.
    ///
    /// Measured as a *difference* between two divisions, which is what makes
    /// it exact: the loop filters have a group delay of their own and it is
    /// the same delay at every setting, so it cancels. Without that
    /// subtraction the answer would carry a fixed fraction of a sample that
    /// says nothing about the grid.
    #[test]
    fn the_read_head_lands_where_the_grid_says() {
        let fs = FS;
        for bpm in [78.0, 120.0, 174.0] {
            let mut arrivals = Vec::new();
            for division in [3usize, 6, 7, 9] {
                let mut delay = open_loop(fs);
                set(&mut delay, PARAM_FEEDBACK, 0.0);
                set(&mut delay, PARAM_DIVISION, division as f32);
                let seconds = synced_seconds(division, bpm).0;
                let input = impulse((seconds * fs * 1.4) as usize + 512);
                let (wet, _) = render(&mut delay, &input, bpm);
                let centre = (seconds * fs) as usize;
                let measured = arrival(&wet, centre.saturating_sub(64), centre + 64);
                arrivals.push((seconds * fs, measured));
            }
            let (base_ideal, base_measured) = arrivals[0];
            for (ideal, measured) in &arrivals[1..] {
                let wanted = ideal - base_ideal;
                let got = measured - base_measured;
                assert!(
                    (got - wanted).abs() < 0.5,
                    "{bpm} bpm: the head moved {got:.3} samples where the grid asked for {wanted:.3}"
                );
            }
        }
    }

    /// **The clamp is applied and it is announced.** A whole note at 40 BPM is
    /// six seconds, the line is five, and it ships as three — halved until it
    /// fits, which is the existing house law.
    #[test]
    fn a_division_too_long_for_the_line_is_halved_until_it_fits() {
        let (seconds, halvings) = synced_seconds(15, 40.0);
        assert_eq!(halvings, 1, "a 6 s whole note was not folded");
        assert!((seconds - 3.0).abs() < 1.0e-12, "{seconds} s");

        let (seconds, halvings) = synced_seconds(15, 120.0);
        assert_eq!(halvings, 0, "a 2 s whole note fits and must not be touched");
        assert!((seconds - 2.0).abs() < 1.0e-12);

        // Every division at every plausible tempo fits the line afterwards.
        for bpm in [20.0, 40.0, 60.0, 120.0, 300.0, 999.0] {
            for division in 0..SYNC_COUNT {
                let (seconds, _) = synced_seconds(division, bpm);
                assert!((MIN_DELAY_S..=MAX_DELAY_S).contains(&seconds), "{bpm} {division}");
            }
        }
        // Nonsense from a corrupt session does not become a delay of infinity.
        assert!(synced_seconds(15, f64::NAN).0.is_finite());
        assert!(synced_seconds(99, 0.0).0.is_finite());
    }

    /// **A tempo change re-tracks, live.** The delay is not told twice — it
    /// reads the tempo out of the block it is handed, so an automation ramp
    /// moves the grid rather than the UI having to notice.
    #[test]
    fn a_tempo_change_re_tracks_the_grid() {
        let fs = FS;
        let mut delay = open_loop(fs);
        set(&mut delay, PARAM_FEEDBACK, 0.0);
        set(&mut delay, PARAM_DIVISION, 9.0); // a quarter note

        // Settle at 120, where a quarter is 500 ms.
        let quiet = vec![0.0f32; (fs * 2.0) as usize];
        let _ = render(&mut delay, &quiet, 120.0);
        assert!((delay.delay_seconds() - 0.5).abs() < 1.0e-12);

        // Now hand it 90 BPM and let the read head walk over.
        let _ = render(&mut delay, &quiet, 90.0);
        assert!(
            (delay.delay_seconds() - 60.0 / 90.0).abs() < 1.0e-12,
            "the grid did not follow the tempo: {} s",
            delay.delay_seconds()
        );

        let input = impulse((fs * 1.4) as usize);
        let (wet, _) = render(&mut delay, &input, 90.0);
        let centre = (60.0 / 90.0 * fs) as usize;
        let measured = arrival(&wet, centre - 200, centre + 200);
        assert!(
            (measured - centre as f64).abs() < 3.0,
            "the echo landed at {measured:.1} where 90 bpm asks for {centre}"
        );

        // ...and a ramp through it never clicks: no sample-to-sample step in
        // the wet bigger than the tone itself could make.
        let mut ramped = open_loop(fs);
        set(&mut ramped, PARAM_DIVISION, 9.0);
        let tone: Vec<f32> = (0..(fs * 4.0) as usize)
            .map(|n| 0.2 * (TAU * 220.0 * n as f64 / fs).sin() as f32)
            .collect();
        let mut out = Vec::new();
        let mut at = 0;
        while at < tone.len() {
            let end = (at + BLOCK).min(tone.len());
            let bpm = 90.0 + 50.0 * (at as f64 / tone.len() as f64);
            let mut l = tone[at..end].to_vec();
            let mut r = l.clone();
            ramped.process(&mut l, &mut r, bpm);
            out.extend_from_slice(&l);
            at = end;
        }
        let step = out
            .windows(2)
            .map(|w| f64::from((w[1] - w[0]).abs()))
            .fold(0.0, f64::max);
        assert!(step < 0.1, "a tempo ramp put a {step:.4} step in the wet");
        assert!(out.iter().all(|s| s.is_finite()));
    }

    /// Sync and free-run are two halves of one clock, and the switch between
    /// them carries the time over rather than jumping to a hidden value.
    #[test]
    fn the_sync_switch_can_carry_a_time_over() {
        for bpm in [90.0, 120.0, 174.0] {
            for (division, label) in SYNC_LABELS.iter().enumerate() {
                let (seconds, _) = synced_seconds(division, bpm);
                assert_eq!(
                    nearest_division(seconds, bpm),
                    division,
                    "{bpm} bpm: {label} did not come back"
                );
            }
        }
        // A hand-dialled time lands on the division a player would have picked.
        assert_eq!(SYNC_LABELS[nearest_division(0.5, 120.0)], "1/4");
        assert_eq!(SYNC_LABELS[nearest_division(0.26, 120.0)], "1/8");
        assert_eq!(SYNC_LABELS[nearest_division(0.37, 120.0)], "1/8D");
        assert_eq!(SYNC_LABELS[nearest_division(0.74, 120.0)], "1/4D");
        // And nonsense lands somewhere rather than panicking.
        assert!(nearest_division(0.0, 120.0) < SYNC_COUNT);
        assert!(nearest_division(1.0e9, 120.0) < SYNC_COUNT);
    }

    // ── The loop ──

    /// **The bound is arithmetic, not a clamp.**
    ///
    /// `|tanh(g·x)/g| ≤ 1/g` for every input there is, so the last thing in
    /// the loop cannot pass more than `1/g` however hard it is driven. This is
    /// the premise the whole feedback range rests on, and it is checked here
    /// against numbers no signal could reach.
    #[test]
    fn the_saturator_cannot_pass_more_than_one_over_g() {
        for mode in Mode::ALL {
            let g = mode.drive();
            let ceiling = 1.0 / g;
            for x in [
                0.0,
                1.0e-30,
                0.5,
                1.0,
                3.0,
                10.0,
                1.0e6,
                1.0e30,
                f64::MAX,
                f64::INFINITY,
            ] {
                for signed in [x, -x] {
                    let out = tanh_approx(g * signed) / g;
                    assert!(
                        out.abs() <= ceiling + 1.0e-12 && out.is_finite(),
                        "{}: sat({signed}) = {out}, over 1/g = {ceiling}",
                        mode.label()
                    );
                }
            }
            // ...and it is a *saturator*, not a limiter: unity for small
            // signals, so a repeat never comes back louder than the one before.
            let small = tanh_approx(g * 1.0e-4) / g;
            assert!((small / 1.0e-4 - 1.0).abs() < 1.0e-6, "{}: small-signal gain is not one", mode.label());
        }
    }

    /// **The loop holds its bound at every feedback setting, including the
    /// ones past unity.**
    ///
    /// A second of tone at 0.7 peak, then thirty of silence, at 250 ms. The
    /// analytic ceiling on what goes *into* the line is `|in| + fb/g`; what
    /// comes back out has been through an interpolator and two one-poles, and
    /// a filter with `|H| ≤ 1` bounds an amplitude rather than a peak — so the
    /// output is allowed 1% over the line's own ceiling, and at every setting
    /// short of the very top it does not use it.
    #[test]
    fn the_loop_is_bounded_at_every_feedback_setting() {
        let fs = FS;
        for fb in [50.0f32, 95.0, 100.0, 110.0, 150.0, 200.0] {
            let mut delay = open_loop(fs);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, 250.0);
            set(&mut delay, PARAM_FEEDBACK, fb);
            let input = burst(300.0, 0.7, 1.0, 31.0, fs);
            let (wet, right) = render(&mut delay, &input, 120.0);
            let bound = 0.7 + f64::from(fb) / 100.0;
            let measured = peak(&wet).max(peak(&right));
            assert!(
                measured <= bound * 1.01,
                "fb {fb}%: the loop reached {measured:.4} against a bound of {bound:.4}"
            );
            assert!(wet.iter().all(|s| s.is_finite()), "fb {fb}%: the loop went non-finite");
        }
    }

    /// Below unity the tail goes away; above it the loop sings and comes back
    /// the moment the knob does.
    #[test]
    fn a_screaming_loop_stops_when_the_feedback_comes_down() {
        let fs = FS;
        // 95% still decays to nothing, because the loop filter takes energy
        // out on every pass — unity feedback is not unity loop gain.
        let mut delay = wet_only(fs);
        set(&mut delay, PARAM_SYNC, 0.0);
        set(&mut delay, PARAM_TIME_MS, 250.0);
        set(&mut delay, PARAM_FEEDBACK, 95.0);
        let input = burst(300.0, 0.7, 1.0, 121.0, fs);
        let (wet, _) = render(&mut delay, &input, 120.0);
        let end = peak(&wet[(fs * 110.0) as usize..]);
        assert!(end < 1.0e-5, "95% feedback left {end:.3e} after two minutes");

        // 200% sings...
        let mut delay = wet_only(fs);
        set(&mut delay, PARAM_SYNC, 0.0);
        set(&mut delay, PARAM_TIME_MS, 250.0);
        set(&mut delay, PARAM_FEEDBACK, 200.0);
        let (wet, _) = render(&mut delay, &input[..(fs * 20.0) as usize], 120.0);
        let singing = peak(&wet[(fs * 15.0) as usize..]);
        assert!(singing > 0.5, "200% feedback did not self-oscillate: {singing:.4}");

        // ...and stops, without a reset, when the knob comes back.
        set(&mut delay, PARAM_FEEDBACK, 20.0);
        let (wet, _) = render(&mut delay, &vec![0.0f32; (fs * 20.0) as usize], 120.0);
        let after = peak(&wet[(fs * 15.0) as usize..]);
        assert!(after < 1.0e-6, "the scream did not stop: {after:.3e}");
    }

    /// **The repeats decay at the rate the knob says.**
    ///
    /// Measured on the *steady middle* of each repeat rather than on its peak
    /// or on a fixed grid. Both of the obvious estimators are biased, and both
    /// are the sustain-target trap in another costume: a fixed window
    /// straddling a repeat reads the ratio low, and the peak of a repeat picks
    /// up the loop filter's transient at the burst's own edges and reads it
    /// 2.3% *high*, consistently, at every feedback setting. The middle
    /// twenty-five milliseconds of a forty-millisecond burst is neither.
    #[test]
    fn the_repeats_decay_at_the_rate_the_knob_says() {
        let fs = FS;
        for fb in [20.0f32, 40.0, 60.0, 80.0, 95.0] {
            let mut delay = open_loop(fs);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, 250.0);
            set(&mut delay, PARAM_FEEDBACK, fb);
            let input = burst(1000.0, 0.05, 0.04, 3.0, fs);
            let (wet, _) = render(&mut delay, &input, 120.0);

            let tau = (0.25 * fs) as usize;
            let levels: Vec<f64> = (1..=6)
                .map(|n| {
                    let from = n * tau + (0.010 * fs) as usize;
                    let to = (n * tau + (0.035 * fs) as usize).min(wet.len());
                    rms(&wet[from..to])
                })
                .collect();
            let wanted = f64::from(fb) / 100.0;
            assert!(levels[0] > 1.0e-4, "fb {fb}%: there was no first repeat");
            for (index, pair) in levels.windows(2).enumerate() {
                let ratio = pair[1] / pair[0];
                assert!(
                    (ratio / wanted - 1.0).abs() < 0.01,
                    "fb {fb}%: repeat {} to {} was {ratio:.4}, not {wanted:.4}",
                    index + 1,
                    index + 2
                );
            }
            // ...and the published repeat count is the same arithmetic.
            if let Some(repeats) = repeats_to_silence(fb) {
                let predicted = 60.0 / (-20.0 * wanted.log10());
                assert!((repeats - predicted).abs() < 1.0e-9);
            }
        }
        assert_eq!(repeats_to_silence(0.0), None, "silence has no repeats to count");
        assert_eq!(repeats_to_silence(120.0), None, "past unity it does not stop");
    }

    /// **No DC accumulates in the loop.** Asymmetric material saturates
    /// asymmetrically, and without a blocker inside the loop the offset is
    /// multiplied by the feedback and written back until the line walks away.
    /// It does not show up in a short test, so this one is not short.
    #[test]
    fn no_dc_accumulates_in_the_loop() {
        let fs = FS;
        let mut delay = wet_only(fs);
        set(&mut delay, PARAM_SYNC, 0.0);
        set(&mut delay, PARAM_TIME_MS, 250.0);
        set(&mut delay, PARAM_FEEDBACK, 105.0);
        // Half-wave rectified: as asymmetric as a signal gets.
        let frames = (fs * 60.0) as usize;
        let input: Vec<f32> = (0..frames)
            .map(|n| (0.8 * (TAU * 120.0 * n as f64 / fs).sin().max(0.0)) as f32)
            .collect();
        let (wet, _) = render(&mut delay, &input, 120.0);
        let last = &wet[frames - fs as usize..];
        let mean = last.iter().map(|v| f64::from(*v)).sum::<f64>() / last.len() as f64;
        assert!(mean.abs() < 1.0e-4, "the loop drifted to a {mean:.3e} offset");
        assert!(wet.iter().all(|s| s.is_finite()));
    }

    /// **Silence in, silence out** — at the top of the feedback range, in
    /// every mode, with every filter where it lands.
    #[test]
    fn silence_in_is_silence_out() {
        for mode in Mode::ALL {
            for routing in Routing::ALL {
                let mut delay = wet_only(FS);
                set(&mut delay, PARAM_MODE, mode.index() as f32);
                set(&mut delay, PARAM_ROUTING, routing.index() as f32);
                set(&mut delay, PARAM_FEEDBACK, 200.0);
                set(&mut delay, PARAM_HEADS, 6.0);
                set(&mut delay, PARAM_DUCK, 100.0);
                let (left, right) = render(&mut delay, &vec![0.0f32; (FS * 10.0) as usize], 120.0);
                assert_eq!(peak(&left), 0.0, "{} {} made something out of silence", mode.label(), routing.label());
                assert_eq!(peak(&right), 0.0, "{} {} right", mode.label(), routing.label());
            }
        }
    }

    /// **A delay nobody has turned up is inaudible, sample for sample.**
    ///
    /// Including the awkward values: `−0.0` through `dry·1.0 + wet·0.0` comes
    /// back as `+0.0`, which is why the zero case is a branch and not
    /// arithmetic.
    #[test]
    fn wet_at_zero_is_bit_identical_dry() {
        let mut delay = Delay::new(FS);
        set(&mut delay, PARAM_MIX, 0.0);
        set(&mut delay, PARAM_FEEDBACK, 150.0);
        delay.snap();
        let source: Vec<f32> = (0..4096)
            .map(|i| (i as f32 * 0.021).sin() * 0.6 + (i as f32 * 0.37).cos() * 0.2)
            .chain([0.0, -0.0, f32::MIN_POSITIVE, -f32::MIN_POSITIVE])
            .collect();
        let (left, right) = render(&mut delay, &source, 120.0);
        for (index, (before, after)) in source.iter().zip(&left).enumerate() {
            assert_eq!(before.to_bits(), after.to_bits(), "sample {index}: {before} -> {after}");
        }
        assert_eq!(right, source);
    }

    /// A mix turned down while the delay is ringing settles to an exact null
    /// rather than to a millionth of one.
    #[test]
    fn a_mix_turned_down_settles_to_an_exact_null() {
        let mut delay = wet_only(FS);
        let tone: Vec<f32> = (0..(FS * 1.0) as usize)
            .map(|n| 0.3 * (TAU * 220.0 * n as f64 / FS).sin() as f32)
            .collect();
        let _ = render(&mut delay, &tone, 120.0);
        set(&mut delay, PARAM_MIX, 0.0);
        // Past the 15 ms glide by a wide margin.
        let _ = render(&mut delay, &tone, 120.0);
        let (left, _) = render(&mut delay, &tone, 120.0);
        for (index, (before, after)) in tone.iter().zip(&left).enumerate() {
            assert_eq!(before.to_bits(), after.to_bits(), "sample {index}");
        }
    }

    // ── Routing ──

    /// **Ping-pong alternates, starting left, with exact zeros on the other
    /// side** — and it leaves the dry image alone, which is the classic bug.
    #[test]
    fn ping_pong_alternates_and_never_touches_the_dry() {
        let fs = FS;
        let mut delay = wet_only(fs);
        set(&mut delay, PARAM_ROUTING, Routing::PingPong.index() as f32);
        set(&mut delay, PARAM_SYNC, 0.0);
        set(&mut delay, PARAM_TIME_MS, 150.0);
        set(&mut delay, PARAM_FEEDBACK, 60.0);
        let input = impulse((fs * 1.5) as usize);
        let (left, right) = render(&mut delay, &input, 120.0);

        let tau = (0.15 * fs) as usize;
        let mut previous = f64::INFINITY;
        for n in 1..=6 {
            let from = n * tau - 64;
            let to = (n * tau + 2048).min(left.len());
            let (here, there) = if n % 2 == 1 {
                (peak(&left[from..to]), peak(&right[from..to]))
            } else {
                (peak(&right[from..to]), peak(&left[from..to]))
            };
            assert!(here > 1.0e-3, "repeat {n} is missing from the side it belongs on: {here:.6}");
            // Eighty decibels down, which is the bar, and it is met by a
            // factor of a million: what is left on the silent side is the
            // in-loop DC blocker's own state decaying, not signal.
            assert!(
                there < here * 1.0e-4,
                "repeat {n} leaked {there:.3e} onto the other side against {here:.6}"
            );
            assert!(here < previous, "repeat {n} came back louder than the one before");
            previous = here;
        }

        // The dry image survives. Measured before the first repeat arrives,
        // where the wet is exactly zero and any difference is the dry's.
        let mut delay = Delay::new(fs);
        set(&mut delay, PARAM_ROUTING, Routing::PingPong.index() as f32);
        set(&mut delay, PARAM_SYNC, 0.0);
        set(&mut delay, PARAM_TIME_MS, 150.0);
        set(&mut delay, PARAM_MIX, 50.0);
        delay.snap();
        let frames = (fs * 0.1) as usize;
        let wide_l: Vec<f32> = (0..frames).map(|n| 0.5 * (TAU * 300.0 * n as f64 / fs).sin() as f32).collect();
        let wide_r: Vec<f32> = (0..frames).map(|n| -0.5 * (TAU * 300.0 * n as f64 / fs).sin() as f32).collect();
        let (out_l, out_r) = render_stereo(&mut delay, &wide_l, &wide_r, 120.0);
        for index in 0..frames {
            let wanted = f64::from(wide_l[index] - wide_r[index]) * 0.5;
            let got = f64::from(out_l[index] - out_r[index]);
            assert!(
                (got - wanted).abs() < 1.0e-7,
                "the dry image collapsed at sample {index}: {got} against {wanted}"
            );
        }
    }

    /// Mono routing is exactly centred, and stereo routing is not.
    #[test]
    fn mono_routing_is_centred_and_stereo_is_not() {
        let fs = FS;
        let frames = (fs * 1.0) as usize;
        let left: Vec<f32> = (0..frames).map(|n| 0.4 * (TAU * 300.0 * n as f64 / fs).sin() as f32).collect();
        let right: Vec<f32> = (0..frames).map(|n| 0.4 * (TAU * 700.0 * n as f64 / fs).sin() as f32).collect();

        let mut mono = wet_only(fs);
        set(&mut mono, PARAM_ROUTING, Routing::Mono.index() as f32);
        set(&mut mono, PARAM_SYNC, 0.0);
        set(&mut mono, PARAM_TIME_MS, 100.0);
        set(&mut mono, PARAM_OFFSET, 50.0);
        let (out_l, out_r) = render_stereo(&mut mono, &left, &right, 120.0);
        assert_eq!(out_l, out_r, "a mono delay came out of two different speakers");
        assert!(rms(&out_l) > 1.0e-4, "the mono delay made no sound");

        let mut stereo = wet_only(fs);
        set(&mut stereo, PARAM_SYNC, 0.0);
        set(&mut stereo, PARAM_TIME_MS, 100.0);
        let (out_l, out_r) = render_stereo(&mut stereo, &left, &right, 120.0);
        assert_ne!(out_l, out_r, "a stereo delay collapsed its two lines into one");
    }

    /// The stereo offset moves the right line only, by a percentage of the
    /// delay rather than by a fixed number of milliseconds.
    #[test]
    fn the_offset_moves_the_right_line_by_a_percentage() {
        let fs = FS;
        for (offset, factor) in [(0.0f32, 1.0f64), (25.0, 1.25), (-40.0, 0.60)] {
            let mut delay = open_loop(fs);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, 200.0);
            set(&mut delay, PARAM_FEEDBACK, 0.0);
            set(&mut delay, PARAM_WIDTH, 100.0);
            set(&mut delay, PARAM_OFFSET, offset);
            let input = impulse((fs * 1.0) as usize);
            let (left, right) = render(&mut delay, &input, 120.0);
            let wanted_l = 0.2 * fs;
            let wanted_r = 0.2 * factor * fs;
            let got_l = arrival(&left, (wanted_l as usize).saturating_sub(400), wanted_l as usize + 400);
            let got_r = arrival(&right, (wanted_r as usize).saturating_sub(400), wanted_r as usize + 400);
            assert!((got_l - wanted_l).abs() < 4.0, "offset {offset}%: left at {got_l:.1}");
            assert!(
                ((got_r - got_l) - (wanted_r - wanted_l)).abs() < 1.0,
                "offset {offset}%: the right line sits {:.1} samples from the left, not {:.1}",
                got_r - got_l,
                wanted_r - wanted_l
            );
        }
    }

    /// Width is mid/side on the wet only: at zero the wet is mono and the dry
    /// is untouched, and above 100% it opens past the speakers.
    #[test]
    fn width_narrows_the_wet_and_nothing_else() {
        let fs = FS;
        let frames = (fs * 0.6) as usize;
        let left: Vec<f32> = (0..frames).map(|n| 0.4 * (TAU * 300.0 * n as f64 / fs).sin() as f32).collect();
        let right: Vec<f32> = (0..frames).map(|n| 0.4 * (TAU * 700.0 * n as f64 / fs).sin() as f32).collect();

        let mut narrow = wet_only(fs);
        set(&mut narrow, PARAM_SYNC, 0.0);
        set(&mut narrow, PARAM_TIME_MS, 100.0);
        set(&mut narrow, PARAM_WIDTH, 0.0);
        narrow.snap();
        let (out_l, out_r) = render_stereo(&mut narrow, &left, &right, 120.0);
        for index in 0..frames {
            assert!(
                (out_l[index] - out_r[index]).abs() < 1.0e-7,
                "width 0 left a difference at sample {index}"
            );
        }

        let mut wide = wet_only(fs);
        set(&mut wide, PARAM_SYNC, 0.0);
        set(&mut wide, PARAM_TIME_MS, 100.0);
        set(&mut wide, PARAM_WIDTH, 200.0);
        let (wide_l, wide_r) = render_stereo(&mut wide, &left, &right, 120.0);
        let mut plain = wet_only(fs);
        set(&mut plain, PARAM_SYNC, 0.0);
        set(&mut plain, PARAM_TIME_MS, 100.0);
        let (plain_l, plain_r) = render_stereo(&mut plain, &left, &right, 120.0);
        let side = |l: &[f32], r: &[f32]| {
            rms(&l.iter().zip(r).map(|(a, b)| a - b).collect::<Vec<f32>>())
        };
        assert!(
            side(&wide_l, &wide_r) > side(&plain_l, &plain_r) * 1.5,
            "200% width did not widen the wet"
        );
    }

    // ── The modes ──

    /// **The bucket brigade's corner is its clock's.** `f_clk = 4096/(2τ)`,
    /// and the loop's anti-alias filter a third of the way up it, held between
    /// 800 Hz and 12 kHz.
    #[test]
    fn the_bucket_brigade_corner_tracks_the_clock() {
        for (seconds, wanted) in [(0.12, 5688.9), (0.3, 2275.6), (0.6, 1137.8)] {
            let got = bbd_corner_hz(seconds);
            assert!((got / wanted - 1.0).abs() < 0.001, "{seconds} s gave {got:.1} Hz, not {wanted}");
            let clock = BBD_STAGES / (2.0 * seconds);
            assert!((got - clock / 3.0).abs() < 1.0e-6);
        }
        // Held at both ends rather than running off them.
        assert_eq!(bbd_corner_hz(10.0), BBD_LP_MIN_HZ);
        assert_eq!(bbd_corner_hz(0.001), BBD_LP_MAX_HZ);
        assert_eq!(bbd_corner_hz(0.0), BBD_LP_MAX_HZ);
        assert_eq!(bbd_corner_hz(f64::NAN), BBD_LP_MAX_HZ);
    }

    /// **Longer delay, darker repeats — and it compounds.**
    ///
    /// This is the one thing a fixed loop corner cannot imitate, and it is the
    /// whole reason BBD is a mode rather than a preset. Measured with a
    /// two-tone burst and a ratio, never with a click: the compander's own
    /// envelope ramp injects enough low-frequency energy to swamp a corner
    /// estimated from an impulse.
    #[test]
    fn the_bucket_brigade_darkens_faster_at_longer_delays() {
        let fs = FS;
        let mut slopes = Vec::new();
        for time_ms in [120.0f32, 300.0, 600.0] {
            let mut delay = wet_only(fs);
            set(&mut delay, PARAM_MODE, Mode::Bbd.index() as f32);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, time_ms);
            set(&mut delay, PARAM_FEEDBACK, 70.0);
            set(&mut delay, PARAM_LOW_CUT_HZ, 20.0);
            set(&mut delay, PARAM_HIGH_CUT_HZ, 20_000.0);
            let input = two_tone(1000.0, 5000.0, 0.2, 0.04, 6.0, fs);
            let (wet, _) = render(&mut delay, &input, 120.0);

            let tau = (f64::from(time_ms) / 1000.0 * fs) as usize;
            let mut ratios = Vec::new();
            for n in 1..=5 {
                let from = n * tau;
                let to = (from + (0.04 * fs) as usize).min(wet.len());
                let window = &wet[from..to];
                let low = tone_amplitude(window, 1000.0, fs);
                let high = tone_amplitude(window, 5000.0, fs);
                ratios.push(20.0 * (high / low.max(1.0e-12)).log10());
            }
            for pair in ratios.windows(2) {
                assert!(pair[1] < pair[0], "{time_ms} ms: a repeat came back brighter");
            }
            slopes.push((ratios[4] - ratios[0]) / 4.0);
        }
        assert!(
            slopes[2] / slopes[0] > 3.0,
            "600 ms darkens at {:.2} dB a repeat against 120 ms at {:.2} — only {:.1}x",
            slopes[2],
            slopes[0],
            slopes[2] / slopes[0]
        );
        // ...and the short setting really is nearly clean.
        assert!(slopes[0] > -4.0, "120 ms lost {:.2} dB a repeat", slopes[0]);
    }

    /// **The loop filters are where their labels say, and they compound.**
    ///
    /// In the loop, not on the wet output, and that is the whole point: a
    /// compounding decay is what makes a delay recede into a space rather than
    /// repeat a static copy. Measured at both corners with a two-tone burst —
    /// the corner against a 1 kHz reference — the attenuation per repeat is
    /// the same number every time and the total is exactly `n` times it.
    ///
    /// The reference is not free either: a 6 kHz one-pole takes 0.12 dB off
    /// 1 kHz and a 200 Hz one takes 0.17 dB, so the measured 2.74 dB against
    /// the reference is 3.03 dB absolute — which is the corner, to a
    /// hundredth.
    #[test]
    fn the_loop_filters_are_at_their_corners_and_they_compound() {
        let fs = FS;
        for probe in [6_000.0f64, 200.0] {
            let mut delay = wet_only(fs);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, 250.0);
            set(&mut delay, PARAM_FEEDBACK, 60.0);
            // The defaults, which is the point: 200 Hz and 6 kHz, both on.
            assert_eq!(delay.param_natural(PARAM_LOW_CUT_HZ), 200.0);
            assert_eq!(delay.param_natural(PARAM_HIGH_CUT_HZ), 6_000.0);

            let input = two_tone(1000.0, probe, 0.15, 0.04, 3.0, fs);
            let (wet, _) = render(&mut delay, &input, 120.0);
            let tau = (0.25 * fs) as usize;
            let levels: Vec<f64> = (1..=4)
                .map(|n| {
                    let from = n * tau;
                    let to = (from + (0.04 * fs) as usize).min(wet.len());
                    let window = &wet[from..to];
                    let reference = tone_amplitude(window, 1000.0, fs);
                    let here = tone_amplitude(window, probe, fs);
                    20.0 * (here / reference.max(1.0e-12)).log10()
                })
                .collect();

            // One pass is the corner, minus what the reference itself loses.
            assert!(
                (levels[0] + 2.74).abs() < 0.30,
                "{probe} Hz lost {:.2} dB in one pass, not 2.74",
                -levels[0]
            );
            // ...and every pass after it loses the same again.
            for (index, level) in levels.iter().enumerate() {
                let wanted = levels[0] * (index + 1) as f64;
                assert!(
                    (level - wanted).abs() < 0.10,
                    "{probe} Hz: repeat {} is {level:.2} dB down where {} passes ask for {wanted:.2}",
                    index + 1,
                    index + 1
                );
            }
        }

        // Opened right up, a repeat keeps its top and its bottom.
        let mut open = open_loop(FS);
        set(&mut open, PARAM_SYNC, 0.0);
        set(&mut open, PARAM_TIME_MS, 250.0);
        set(&mut open, PARAM_FEEDBACK, 60.0);
        let input = two_tone(1000.0, 6_000.0, 0.15, 0.04, 3.0, FS);
        let (wet, _) = render(&mut open, &input, 120.0);
        let tau = (0.25 * FS) as usize;
        let window = &wet[4 * tau..(4 * tau + (0.04 * FS) as usize).min(wet.len())];
        let kept = 20.0
            * (tone_amplitude(window, 6_000.0, FS) / tone_amplitude(window, 1000.0, FS)).log10();
        assert!(kept > -1.0, "four repeats with the filters open still lost {:.2} dB", -kept);
    }

    /// The wander knob is the bucket brigade's and nothing else's: at zero the
    /// clock is steady, and it never becomes a noise generator.
    #[test]
    fn the_bucket_brigade_clock_wanders_rather_than_scrambles() {
        let fs = FS;
        let mut steady = wet_only(fs);
        set(&mut steady, PARAM_MODE, Mode::Bbd.index() as f32);
        set(&mut steady, PARAM_SYNC, 0.0);
        set(&mut steady, PARAM_TIME_MS, 500.0);
        set(&mut steady, PARAM_FEEDBACK, 0.0);
        set(&mut steady, PARAM_WANDER, 0.0);
        let tone: Vec<f32> = (0..(fs * 6.0) as usize)
            .map(|n| 0.4 * (TAU * 2000.0 * n as f64 / fs).sin() as f32)
            .collect();
        let (wet, _) = render(&mut steady, &tone, 120.0);
        let track = fm_deviation(&wet[(fs * 2.0) as usize..], 2000.0, fs);
        let quiet = track_component(&track, BBD_WANDER_HZ, fs);
        assert!(quiet < 1.0e-6, "a steady clock drifted {:.5}%", quiet * 100.0);

        let mut drifting = wet_only(fs);
        set(&mut drifting, PARAM_MODE, Mode::Bbd.index() as f32);
        set(&mut drifting, PARAM_SYNC, 0.0);
        set(&mut drifting, PARAM_TIME_MS, 500.0);
        set(&mut drifting, PARAM_FEEDBACK, 0.0);
        let (wet, _) = render(&mut drifting, &tone, 120.0);
        let track = fm_deviation(&wet[(fs * 2.0) as usize..], 2000.0, fs);
        let moving = track_component(&track, BBD_WANDER_HZ, fs);
        assert!(moving > quiet * 100.0, "the clock did not drift at all: {:.5}%", moving * 100.0);

        // **The bound is the point.** A smoothstep walk cannot move the read
        // head faster than `1.5 · span · hz` of its own length per second,
        // whatever random numbers it draws — a bound, not an attenuation,
        // which is the part a low-pass on white noise cannot give you. The
        // house has already shipped the hiss carpet that results from getting
        // this wrong once.
        let ceiling = 1.5 * 2.0 * BBD_WANDER_SPAN * BBD_WANDER_HZ;
        assert!(
            moving < ceiling,
            "the clock moved {:.5}% where the walk's own slope bound is {:.5}%",
            moving * 100.0,
            ceiling * 100.0
        );
    }

    /// **The tape heads are at one, two and three**, and the feedback comes
    /// off the longest one — which is what makes the rhythm work.
    #[test]
    fn the_tape_heads_are_at_one_two_and_three() {
        let fs = FS;
        let mut delay = wet_only(fs);
        set(&mut delay, PARAM_MODE, Mode::Tape.index() as f32);
        set(&mut delay, PARAM_SYNC, 0.0);
        set(&mut delay, PARAM_TIME_MS, 200.0);
        set(&mut delay, PARAM_FEEDBACK, 0.0);
        set(&mut delay, PARAM_HEADS, 6.0);
        let input = impulse((fs * 1.2) as usize);
        let (wet, _) = render(&mut delay, &input, 120.0);

        let mut arrivals = Vec::new();
        for head in 1..=3 {
            let centre = (0.2 * f64::from(head) * fs) as usize;
            let measured = arrival(&wet, centre - 400, centre + 400);
            assert!(
                (measured / centre as f64 - 1.0).abs() < 0.003,
                "head {head} landed at {measured:.1} rather than {centre}"
            );
            arrivals.push(measured);
        }
        assert!((arrivals[1] / arrivals[0] - 2.0).abs() < 0.01, "head 2 is not twice head 1");
        assert!((arrivals[2] / arrivals[0] - 3.0).abs() < 0.01, "head 3 is not three times head 1");

        // Each selector position reads the heads it says it does.
        for (index, label) in HEAD_LABELS.iter().enumerate() {
            let wanted: Vec<usize> = label
                .split('+')
                .map(|h| h.parse::<usize>().expect("a head number") - 1)
                .collect();
            for (head, on) in head_set(index).iter().enumerate() {
                assert_eq!(*on, wanted.contains(&head), "{label} head {}", head + 1);
            }
            assert_eq!(head_span(index), HEAD_RATIOS[*wanted.iter().max().unwrap()]);
        }
    }

    /// A tape echo whose heads would read past the end of the line clamps the
    /// *base*, not the taps — which is what keeps 1 : 2 : 3 exactly 1 : 2 : 3
    /// instead of collapsing them into each other at the far end of the knob.
    #[test]
    fn a_tape_head_that_would_not_fit_shortens_the_base() {
        let mut delay = wet_only(FS);
        set(&mut delay, PARAM_MODE, Mode::Tape.index() as f32);
        set(&mut delay, PARAM_SYNC, 0.0);
        set(&mut delay, PARAM_TIME_MS, 5_000.0);
        let mut left = vec![0.0f32; BLOCK];
        let mut right = vec![0.0f32; BLOCK];

        set(&mut delay, PARAM_HEADS, 0.0);
        delay.process(&mut left, &mut right, 120.0);
        assert!((delay.delay_seconds() - 5.0).abs() < 1.0e-9, "one head fits the whole line");

        set(&mut delay, PARAM_HEADS, 6.0);
        delay.process(&mut left, &mut right, 120.0);
        assert!(
            (delay.delay_seconds() - 5.0 / 3.0).abs() < 1.0e-9,
            "three heads did not shorten the base: {} s",
            delay.delay_seconds()
        );

        // ...and nothing else pays for it: the same base in digital mode is
        // the whole five seconds.
        set(&mut delay, PARAM_MODE, Mode::Digital.index() as f32);
        delay.process(&mut left, &mut right, 120.0);
        assert!((delay.delay_seconds() - 5.0).abs() < 1.0e-9);
    }

    /// **The tape's wow is the echo's, not the recorder's.**
    ///
    /// A tape echo's delay time *is* the head spacing over the tape speed, so
    /// the pitch deviation is `τ₀·2πf·D` and therefore **grows with the delay
    /// time**. A recorder's is `D` and does not. Measured at two delay times a
    /// factor of four apart, against the closed form, and against a digital
    /// delay which does none of it.
    #[test]
    fn the_tape_wow_is_the_echo_form() {
        let fs = FS;
        for (time_ms, wanted) in [(250.0f32, 0.25f64), (1000.0, 1.0)] {
            let mut delay = wet_only(fs);
            set(&mut delay, PARAM_MODE, Mode::Tape.index() as f32);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, time_ms);
            set(&mut delay, PARAM_FEEDBACK, 0.0);
            set(&mut delay, PARAM_LOW_CUT_HZ, 20.0);
            set(&mut delay, PARAM_HIGH_CUT_HZ, 20_000.0);
            let tone: Vec<f32> = (0..(fs * 12.0) as usize)
                .map(|n| 0.4 * (TAU * 2000.0 * n as f64 / fs).sin() as f32)
                .collect();
            let (wet, _) = render(&mut delay, &tone, 120.0);
            let track = fm_deviation(&wet[(fs * 2.0) as usize..], 2000.0, fs);

            let wow = track_component(&track, TAPE_WOW.1, fs);
            let predicted = wanted * TAU * TAPE_WOW.1 * TAPE_WOW.0;
            assert!(
                (wow / predicted - 1.0).abs() < 0.05,
                "{time_ms} ms: the wow measured {:.5}% where the echo form asks for {:.5}%",
                wow * 100.0,
                predicted * 100.0
            );
            // The other two ride at the depth they are specified as,
            // regardless of the delay time, because they are head-local.
            let flutter = track_component(&track, TAPE_FLUTTER.1, fs);
            assert!(
                (flutter / TAPE_FLUTTER.0 - 1.0).abs() < 0.10,
                "{time_ms} ms: flutter measured {:.5}%, not {:.5}%",
                flutter * 100.0,
                TAPE_FLUTTER.0 * 100.0
            );
            let scrape = track_component(&track, TAPE_SCRAPE.1, fs);
            assert!(
                (scrape / TAPE_SCRAPE.0 - 1.0).abs() < 0.15,
                "{time_ms} ms: scrape measured {:.5}%, not {:.5}%",
                scrape * 100.0,
                TAPE_SCRAPE.0 * 100.0
            );
        }

        // A digital delay does none of it.
        let mut digital = wet_only(fs);
        set(&mut digital, PARAM_SYNC, 0.0);
        set(&mut digital, PARAM_TIME_MS, 250.0);
        set(&mut digital, PARAM_FEEDBACK, 0.0);
        let tone: Vec<f32> = (0..(fs * 8.0) as usize)
            .map(|n| 0.4 * (TAU * 2000.0 * n as f64 / fs).sin() as f32)
            .collect();
        let (wet, _) = render(&mut digital, &tone, 120.0);
        let track = fm_deviation(&wet[(fs * 2.0) as usize..], 2000.0, fs);
        let floor = track_component(&track, TAPE_WOW.1, fs);
        assert!(
            floor < 1.0e-6,
            "a digital delay wowed {:.3e}% — the demodulator's own floor is a hundredth of that",
            floor * 100.0
        );
    }

    /// **And the wow is inside the loop**, which is what makes a tape echo
    /// wander further the longer you let it run: the second repeat has been
    /// past the capstan twice and carries twice the deviation.
    #[test]
    fn the_tape_wow_compounds_on_the_repeats() {
        let fs = FS;
        let mut delay = wet_only(fs);
        set(&mut delay, PARAM_MODE, Mode::Tape.index() as f32);
        set(&mut delay, PARAM_SYNC, 0.0);
        set(&mut delay, PARAM_TIME_MS, 2_000.0);
        set(&mut delay, PARAM_FEEDBACK, 90.0);
        set(&mut delay, PARAM_LOW_CUT_HZ, 20.0);
        set(&mut delay, PARAM_HIGH_CUT_HZ, 20_000.0);
        // A burst short enough that the repeats do not overlap.
        let frames = (fs * 10.0) as usize;
        let driven = (fs * 1.6) as usize;
        let input: Vec<f32> = (0..frames)
            .map(|n| if n < driven { 0.5 * (TAU * 2000.0 * n as f64 / fs).sin() as f32 } else { 0.0 })
            .collect();
        let (wet, _) = render(&mut delay, &input, 120.0);

        let deviation = |repeat: usize| {
            let from = (repeat as f64 * 2.0 * fs) as usize + (0.2 * fs) as usize;
            let to = (from + (1.2 * fs) as usize).min(wet.len());
            let track = fm_deviation(&wet[from..to], 2000.0, fs);
            track_component(&track[track.len() / 3..], TAPE_WOW.1, fs)
        };
        let first = deviation(1);
        let second = deviation(2);
        assert!(first > 1.0e-4, "there was no first repeat to measure: {first:.6}");
        assert!(
            second > first * 1.25,
            "the second repeat deviated {:.5}% against the first's {:.5}% — the wow is not in the loop",
            second * 100.0,
            first * 100.0
        );
    }

    // ── When the time knob moves ──

    /// **Repitch bends the pitch; fade does not.**
    ///
    /// Sweeping 250 → 300 ms over a second is `dτ/dt = 0.05`, so a repitching
    /// read head plays back at 0.95× — which is 88.8 cents down, and the
    /// closed form says −88.7. A crossfade holds the pitch exactly.
    #[test]
    fn repitch_bends_the_pitch_and_fade_does_not() {
        let fs = FS;
        let cents = |tmode: TimeMode| {
            let mut delay = open_loop(fs);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, 250.0);
            set(&mut delay, PARAM_FEEDBACK, 0.0);
            set(&mut delay, PARAM_TIME_MODE, tmode.index() as f32);
            let frames = (fs * 3.0) as usize;
            let tone: Vec<f32> = (0..frames)
                .map(|n| 0.4 * (TAU * 1000.0 * n as f64 / fs).sin() as f32)
                .collect();
            let mut out = Vec::with_capacity(frames);
            let mut at = 0;
            while at < frames {
                let end = (at + BLOCK).min(frames);
                let t = at as f64 / fs;
                let ms = if t < 1.0 {
                    250.0
                } else if t < 2.0 {
                    250.0 + 50.0 * (t - 1.0)
                } else {
                    300.0
                };
                set(&mut delay, PARAM_TIME_MS, ms as f32);
                let mut l = tone[at..end].to_vec();
                let mut r = l.clone();
                delay.process(&mut l, &mut r, 120.0);
                out.extend_from_slice(&l);
                at = end;
            }
            let track = fm_deviation(&out[(fs * 1.4) as usize..(fs * 1.9) as usize], 1000.0, fs);
            let tail = &track[track.len() / 3..];
            let mean = tail.iter().sum::<f64>() / tail.len() as f64;
            1200.0 * (1.0 + mean).log2()
        };

        let repitched = cents(TimeMode::Repitch);
        assert!(
            (repitched + 88.7).abs() < 10.0,
            "repitch bent the tone {repitched:+.1} cents, not −88.7"
        );
        let faded = cents(TimeMode::Fade);
        assert!(faded.abs() < 5.0, "a crossfade bent the tone {faded:+.1} cents");

        // Auto is the mode's own answer rather than a fourth behaviour.
        let mut delay = Delay::new(fs);
        for (mode, wanted) in [
            (Mode::Digital, TimeMode::Fade),
            (Mode::Bbd, TimeMode::Repitch),
            (Mode::Tape, TimeMode::Repitch),
        ] {
            set(&mut delay, PARAM_MODE, mode.index() as f32);
            let mut l = vec![0.0f32; BLOCK];
            let mut r = vec![0.0f32; BLOCK];
            delay.process(&mut l, &mut r, 120.0);
            assert_eq!(delay.time_behaviour(), wanted, "auto on {}", mode.label());
        }
    }

    /// Jump is the one that clicks, and that is what it is for. Fade and
    /// repitch move the same distance without a step in the waveform.
    ///
    /// The move is a twelve-millisecond one rather than a big one, and that is
    /// the honest test: repitching a quarter of a second in twenty
    /// milliseconds is a *swoop* — continuous, but full of legitimate high
    /// frequencies, and a sample-to-sample slope cannot tell that apart from a
    /// click. A small move keeps the swoop below the tone's own slope, so
    /// what is left to measure is the discontinuity.
    #[test]
    fn only_jump_steps_the_waveform() {
        let fs = FS;
        let step_for = |tmode: TimeMode| {
            let mut delay = open_loop(fs);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, 250.0);
            set(&mut delay, PARAM_FEEDBACK, 0.0);
            set(&mut delay, PARAM_TIME_MODE, tmode.index() as f32);
            // 997 Hz, so that the jump lands on a different phase rather than
            // coincidentally on the same one.
            let frames = (fs * 2.0) as usize;
            let tone: Vec<f32> = (0..frames)
                .map(|n| 0.4 * (TAU * 997.0 * n as f64 / fs).sin() as f32)
                .collect();
            let mut out = Vec::with_capacity(frames);
            let mut at = 0;
            while at < frames {
                let end = (at + BLOCK).min(frames);
                if at as f64 / fs >= 1.0 {
                    set(&mut delay, PARAM_TIME_MS, 262.5);
                }
                let mut l = tone[at..end].to_vec();
                let mut r = l.clone();
                delay.process(&mut l, &mut r, 120.0);
                out.extend_from_slice(&l);
                at = end;
            }
            let from = (fs * 0.95) as usize;
            out[from..]
                .windows(2)
                .map(|w| f64::from((w[1] - w[0]).abs()))
                .fold(0.0, f64::max)
        };

        // What the tone itself does between two samples, and therefore the
        // floor nothing can be measured below.
        let natural = 0.4 * TAU * 997.0 / fs;
        let jumped = step_for(TimeMode::Jump);
        assert!(jumped > natural * 3.0, "jump did not step: {jumped:.4}");
        for tmode in [TimeMode::Fade, TimeMode::Repitch] {
            let step = step_for(tmode);
            assert!(
                step < natural * 1.5,
                "{} stepped {step:.4} where the tone's own slope is {natural:.4}",
                tmode.label()
            );
        }
    }

    // ── Ducking ──

    /// The wet's gain against the dry's, sample for sample: what the ducker
    /// did, in decibels, measured against the same render with the knob at
    /// zero.
    fn duck_curve(fs: f64, amount: f32) -> Vec<f64> {
        let frames = (fs * 3.0) as usize;
        let driven = (fs * 1.0) as usize;
        let input: Vec<f32> = (0..frames)
            .map(|n| {
                if n < driven {
                    (0.7 * (TAU * 200.0 * n as f64 / fs).sin()) as f32
                } else {
                    0.0
                }
            })
            .collect();
        let render_with = |duck: f32| {
            let mut delay = wet_only(fs);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, 500.0);
            set(&mut delay, PARAM_FEEDBACK, 0.0);
            set(&mut delay, PARAM_DUCK, duck);
            delay.snap();
            render(&mut delay, &input, 120.0).0
        };
        let ducked = render_with(amount);
        let plain = render_with(0.0);
        let window = (0.005 * fs) as usize;
        (0..frames / window)
            .map(|block| {
                let from = block * window;
                let to = (from + window).min(frames);
                let a = peak(&ducked[from..to]);
                let b = peak(&plain[from..to]);
                if b < 1.0e-6 {
                    0.0
                } else {
                    20.0 * (a / b).log10()
                }
            })
            .collect()
    }

    /// **Ducking follows the input's envelope, and only the wet's gain.**
    ///
    /// One knob, no threshold: the envelope is normalised against a fixed
    /// −30 dBFS floor and the reduction that implies is scaled by the knob, to
    /// a ceiling of −24 dB. Off by default, so the device sounds like a plain
    /// delay out of the box.
    #[test]
    fn ducking_follows_the_input_envelope() {
        let fs = FS;
        assert_eq!(natural_param(PARAM_DUCK).unwrap().default, 0.0, "ducking is not off by default");

        // With the knob at zero nothing happens at all, sample for sample.
        for value in duck_curve(fs, 0.0) {
            assert_eq!(value, 0.0, "the ducker moved with its knob at zero");
        }

        let curve = duck_curve(fs, 100.0);
        let window = 0.005;
        let at = |seconds: f64| curve[(seconds / window) as usize];

        // While the key is playing the wet is held down, hard.
        let held = at(0.7);
        assert!(
            held < -20.0 && held > -DUCK_MAX_DB - 1.0,
            "a −3 dBFS key ducked the wet {held:.2} dB"
        );
        // ...and the key stops at one second, after which it comes back.
        assert!(at(1.05) > held, "the ducker did not start letting go");
        assert!(at(1.30).abs() < 0.5, "the ducker had not let go after 300 ms: {:.2} dB", at(1.30));

        // The published recovery: 10% to 90% of the way back in 200 ms.
        let time_at = |fraction: f64| {
            let target = held * (1.0 - fraction);
            let mut found = 1.0;
            let mut seconds = 1.0;
            while seconds < 1.6 {
                if at(seconds) >= target {
                    found = seconds;
                    break;
                }
                seconds += window;
            }
            found
        };
        let recovery = time_at(0.9) - time_at(0.1);
        assert!(
            (recovery - 0.200).abs() < 0.040,
            "the wet came back 10% to 90% in {recovery:.3} s, not 0.200"
        );

        // Half the knob is about half the reduction, in decibels.
        let half = duck_curve(fs, 50.0);
        let at_half = half[(0.7 / window) as usize];
        assert!(
            (at_half / held - 0.5).abs() < 0.05,
            "half the knob gave {at_half:.2} dB against the full knob's {held:.2}"
        );
    }

    /// **Ducking does not change the decay.** The wet's *output* gain moves;
    /// the loop does not. Ducking the feedback as well sounds good and makes
    /// the repeat count vary with the performance, which is a thing no status
    /// bar can explain.
    #[test]
    fn ducking_does_not_change_the_decay() {
        let fs = FS;
        let ratios = |duck: f32| {
            let mut delay = open_loop(fs);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, 250.0);
            set(&mut delay, PARAM_FEEDBACK, 60.0);
            set(&mut delay, PARAM_DUCK, duck);
            delay.snap();
            let input = burst(1000.0, 0.05, 0.04, 3.0, fs);
            let (wet, _) = render(&mut delay, &input, 120.0);
            let tau = (0.25 * fs) as usize;
            let levels: Vec<f64> = (1..=6)
                .map(|n| {
                    let from = n * tau + (0.010 * fs) as usize;
                    let to = (n * tau + (0.035 * fs) as usize).min(wet.len());
                    rms(&wet[from..to])
                })
                .collect();
            levels.windows(2).map(|pair| pair[1] / pair[0]).collect::<Vec<f64>>()
        };
        let plain = ratios(0.0);
        let ducked = ratios(100.0);
        for (index, (a, b)) in plain.iter().zip(&ducked).enumerate() {
            assert!(
                (a / b - 1.0).abs() < 0.01,
                "repeat {index}: the decay was {a:.4} without ducking and {b:.4} with it"
            );
        }
    }

    // ── Freeze ──

    /// **Freeze means freeze.** Input gain zero, loop gain exactly one, and
    /// the filters and the saturator out of the path — otherwise an
    /// "endlessly cycling" buffer darkens and quietens away, which is not what
    /// the word means.
    #[test]
    fn freeze_holds_the_buffer_and_the_dry_goes_through() {
        let fs = FS;
        let mut delay = wet_only(fs);
        set(&mut delay, PARAM_SYNC, 0.0);
        set(&mut delay, PARAM_TIME_MS, 500.0);
        set(&mut delay, PARAM_FEEDBACK, 60.0);
        // Fill the line.
        let input = burst(400.0, 0.4, 0.5, 1.0, fs);
        let (_, _) = render(&mut delay, &input, 120.0);

        set(&mut delay, PARAM_FREEZE, 1.0);
        let (held, _) = render(&mut delay, &vec![0.0f32; (fs * 120.0) as usize], 120.0);

        let early = rms(&held[(fs * 2.0) as usize..(fs * 3.0) as usize]);
        let late = rms(&held[(fs * 118.0) as usize..(fs * 119.0) as usize]);
        assert!(early > 1.0e-3, "the freeze had nothing in it: {early:.3e}");
        assert!(
            (20.0 * (late / early).log10()).abs() < 0.1,
            "two minutes of freeze moved the level by {:.3} dB",
            20.0 * (late / early).log10()
        );
        // ...and it did not go dark either. The centroid is the top half's
        // share of the energy, which is enough to catch a loop that filters.
        let centroid = |window: &[f32]| {
            let mut sum = 0.0f64;
            let mut weighted = 0.0f64;
            for hz in [200.0, 400.0, 800.0, 1600.0, 3200.0] {
                let amplitude = tone_amplitude(window, hz, fs);
                sum += amplitude;
                weighted += amplitude * hz;
            }
            weighted / sum.max(1.0e-12)
        };
        let a = centroid(&held[(fs * 2.0) as usize..(fs * 3.0) as usize]);
        let b = centroid(&held[(fs * 118.0) as usize..(fs * 119.0) as usize]);
        assert!((b / a - 1.0).abs() < 0.02, "the frozen buffer's centroid moved from {a:.0} to {b:.0} Hz");

        // The dry path is untouched by any of it.
        let mut delay = Delay::new(fs);
        set(&mut delay, PARAM_FREEZE, 1.0);
        set(&mut delay, PARAM_MIX, 0.0);
        delay.snap();
        let source: Vec<f32> = (0..2048).map(|n| (n as f32 * 0.03).sin() * 0.5).collect();
        let (left, _) = render(&mut delay, &source, 120.0);
        assert_eq!(left, source, "freeze changed the dry signal");
    }

    // ── The house bars ──

    /// **The same delay at every rate.** Not sample-for-sample — the
    /// interpolator's fractions differ — but the same effect: the same level,
    /// the same decay and the same balance of top to bottom.
    #[test]
    fn the_delay_is_the_same_delay_at_every_rate() {
        let mut fingerprints = Vec::new();
        for fs in [44_100.0, 48_000.0, 96_000.0] {
            for mode in Mode::ALL {
                let mut delay = wet_only(fs);
                set(&mut delay, PARAM_MODE, mode.index() as f32);
                set(&mut delay, PARAM_SYNC, 0.0);
                set(&mut delay, PARAM_TIME_MS, 250.0);
                set(&mut delay, PARAM_FEEDBACK, 70.0);
                // A chord rather than one tone: a single frequency lands
                // wherever the loop filter happens to put it and reads as a
                // rate dependence that is not there.
                let frames = (fs * 4.0) as usize;
                let driven = (fs * 0.25) as usize;
                let input: Vec<f32> = (0..frames)
                    .map(|n| {
                        if n < driven {
                            let t = n as f64 / fs;
                            let sum: f64 = [110.0, 220.0, 440.0, 880.0, 1760.0, 3520.0]
                                .iter()
                                .map(|hz| (TAU * hz * t).sin())
                                .sum();
                            (0.25 * sum / 6.0) as f32
                        } else {
                            0.0
                        }
                    })
                    .collect();
                let (wet, _) = render(&mut delay, &input, 120.0);
                let from = (fs * 1.0) as usize;
                let to = (fs * 3.0) as usize;
                let window = &wet[from..to];
                let low = tone_amplitude(window, 220.0, fs);
                let high = tone_amplitude(window, 1760.0, fs);
                fingerprints.push((fs, mode, rms(window), high / low.max(1.0e-12)));
            }
        }
        for mode in Mode::ALL {
            let here: Vec<&(f64, Mode, f64, f64)> =
                fingerprints.iter().filter(|f| f.1 == mode).collect();
            let level: Vec<f64> = here.iter().map(|f| f.2).collect();
            let tilt: Vec<f64> = here.iter().map(|f| f.3).collect();
            let spread = |v: &[f64]| {
                let high = v.iter().copied().fold(f64::MIN, f64::max);
                let low = v.iter().copied().fold(f64::MAX, f64::min);
                (high - low) / low
            };
            assert!(level[0] > 1.0e-4, "{}: nothing came out", mode.label());
            assert!(
                spread(&level) < 0.05,
                "{}: the level spreads {:.1}% across 44.1, 48 and 96 kHz: {level:?}",
                mode.label(),
                spread(&level) * 100.0
            );
            assert!(
                spread(&tilt) < 0.10,
                "{}: the tilt spreads {:.1}% across the three rates: {tilt:?}",
                mode.label(),
                spread(&tilt) * 100.0
            );
        }
    }

    /// **Nothing in the audio path allocates** — including a mode change, a
    /// routing change and a head change while the line is sounding, which is
    /// exactly when a `Vec` that resized would be heard.
    #[test]
    fn nothing_in_the_audio_path_allocates() {
        let mut delay = wet_only(FS);
        set(&mut delay, PARAM_FEEDBACK, 80.0);
        let mut left = vec![0.0f32; 512];
        let mut right = vec![0.0f32; 512];
        for (index, sample) in left.iter_mut().enumerate() {
            *sample = ((index as f32) * 0.07).sin() * 0.3;
        }
        right.copy_from_slice(&left);
        // Warm the line up outside the count.
        for _ in 0..64 {
            delay.process(&mut left, &mut right, 120.0);
        }

        let allocations = crate::synth::tests::allocations_during(|| {
            for block in 0..600 {
                delay.set_param_natural(PARAM_MODE, (block % 3) as f32);
                delay.set_param_natural(PARAM_ROUTING, ((block / 3) % 3) as f32);
                delay.set_param_natural(PARAM_HEADS, (block % 7) as f32);
                delay.set_param_natural(PARAM_FREEZE, f32::from(block % 97 == 0));
                delay.set_param_natural(PARAM_TIME_MS, 40.0 + (block % 400) as f32 * 12.0);
                delay.set_param_natural(PARAM_TIME_MODE, (block % 4) as f32);
                delay.process(&mut left, &mut right, 90.0 + (block % 60) as f64);
            }
        });
        assert_eq!(allocations, 0, "the audio path allocated {allocations} times");
    }

    /// A tail that has decayed away leaves nothing subnormal behind it, and a
    /// hundred seconds of silence after a loud burst costs the same as the
    /// first block did.
    #[test]
    fn a_long_tail_never_goes_subnormal() {
        for mode in Mode::ALL {
            let mut delay = wet_only(FS);
            set(&mut delay, PARAM_MODE, mode.index() as f32);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, 200.0);
            set(&mut delay, PARAM_FEEDBACK, 90.0);
            let input = burst(400.0, 0.8, 0.5, 100.0, FS);
            let (wet, right) = render(&mut delay, &input, 120.0);
            let tail = &wet[(FS * 95.0) as usize..];
            assert_eq!(peak(tail), 0.0, "{}: the tail did not reach zero", mode.label());
            for sample in wet.iter().chain(&right) {
                assert!(
                    sample.is_finite() && (*sample == 0.0 || sample.abs() > f32::MIN_POSITIVE),
                    "{}: a subnormal reached the output", mode.label()
                );
            }
        }
    }

    /// Nonsense from a UI or a hand-edited session is refused rather than
    /// propagated into a delay length.
    #[test]
    fn it_survives_nonsense() {
        let mut delay = Delay::new(FS);
        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| delay.param_natural(i)).collect();
        delay.set_param_natural(PARAM_COUNT, 1.0);
        delay.set_param_natural(usize::MAX, 1.0);
        for index in 0..PARAM_COUNT {
            delay.set_param_natural(index, f32::NAN);
            delay.set_param_natural(index, f32::INFINITY);
        }
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| delay.param_natural(i)).collect();
        assert_eq!(before, after);
        assert_eq!(delay.param_natural(PARAM_COUNT), 0.0);

        // Out-of-range values are clamped to the published travel.
        delay.set_param_natural(PARAM_FEEDBACK, 1.0e9);
        assert_eq!(delay.param_natural(PARAM_FEEDBACK), 200.0);
        delay.set_param_natural(PARAM_TIME_MS, -1.0e9);
        assert_eq!(delay.param_natural(PARAM_TIME_MS), (MIN_DELAY_S * 1000.0) as f32);

        // A rate the device could not have asked for leaves the delay built at
        // the last one it was given, and still sounding.
        delay.set_sample_rate(0.0);
        delay.set_sample_rate(f64::NAN);
        delay.set_sample_rate(-48_000.0);
        assert_eq!(delay.sample_rate(), FS);
        set(&mut delay, PARAM_MIX, 100.0);
        set(&mut delay, PARAM_SYNC, 0.0);
        set(&mut delay, PARAM_TIME_MS, 100.0);
        delay.snap();
        let (wet, _) = render(&mut delay, &burst(400.0, 0.3, 0.2, 1.0, FS), 120.0);
        assert!(rms(&wet) > 0.0, "the delay went silent");
        assert!(wet.iter().all(|s| s.is_finite()));

        // And a tempo no transport would report does not become an infinite
        // delay or a zero one.
        for bpm in [0.0, -120.0, f64::NAN, f64::INFINITY, 1.0e12] {
            let mut left = vec![0.1f32; BLOCK];
            let mut right = vec![0.1f32; BLOCK];
            set(&mut delay, PARAM_SYNC, 1.0);
            delay.process(&mut left, &mut right, bpm);
            assert!(delay.delay_seconds().is_finite() && delay.delay_seconds() > 0.0, "bpm {bpm}");
            assert!(left.iter().all(|s| s.is_finite()), "bpm {bpm}");
        }
    }

    /// Reset drops the tail and keeps the controls.
    #[test]
    fn reset_silences_the_line_and_keeps_the_controls() {
        let mut delay = wet_only(FS);
        set(&mut delay, PARAM_FEEDBACK, 90.0);
        set(&mut delay, PARAM_MODE, Mode::Tape.index() as f32);
        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| delay.param_natural(i)).collect();
        let _ = render(&mut delay, &burst(400.0, 0.5, 0.5, 1.0, FS), 120.0);

        delay.reset();
        let (wet, right) = render(&mut delay, &vec![0.0f32; (FS * 3.0) as usize], 120.0);
        assert_eq!(peak(&wet), 0.0, "the tail survived a reset");
        assert_eq!(peak(&right), 0.0);
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| delay.param_natural(i)).collect();
        assert_eq!(before, after, "a reset moved a control");
    }

    /// The two bit-trick transcendentals are accurate enough to be a ducker's,
    /// which is what lets the detector run without reaching the maths library
    /// twice a sample.
    #[test]
    fn the_fast_transcendentals_are_accurate_enough() {
        let mut worst_log = 0.0f64;
        let mut x = 1.0e-6f32;
        while x < 1.0e6 {
            let error = (f64::from(log2_fast(x)) - f64::from(x).log2()).abs();
            worst_log = worst_log.max(error);
            x *= 1.037;
        }
        assert!(worst_log < 1.0e-6, "log2 was off by {worst_log:.2e} bits");

        let mut worst_exp = 0.0f64;
        let mut e = -40.0f32;
        while e < 40.0 {
            let got = f64::from(exp2_fast(e));
            let wanted = f64::from(e).exp2();
            worst_exp = worst_exp.max(((got - wanted) / wanted).abs());
            e += 0.013;
        }
        assert!(worst_exp < 1.0e-5, "exp2 was off by {worst_exp:.2e} relative");
        assert_eq!(exp2_fast(0.0), 1.0, "two to the nothing is one");

        // What they are actually asked for: a reduction in decibels.
        for amount in [0.0f64, 0.25, 0.5, 1.0] {
            for ratio in [1.0f64, 2.0, 5.0, DUCK_CEILING] {
                let got = f64::from(exp2_fast(-(amount as f32) * log2_fast(ratio as f32)));
                let wanted = ratio.powf(-amount);
                assert!(
                    (20.0 * (got / wanted).log10()).abs() < 0.005,
                    "amount {amount}, ratio {ratio}: {got} against {wanted}"
                );
            }
        }
        // The two published levels are the numbers they are written as.
        assert!((DUCK_FLOOR - 10.0f64.powf(DUCK_FLOOR_DB / 20.0)).abs() < 1.0e-15);
        assert!((DUCK_CEILING - 10.0f64.powf(DUCK_MAX_DB / 20.0)).abs() < 1.0e-13);
    }
}

#[cfg(test)]
mod measure {
    use super::tests::*;
    use super::*;

    /// `cargo test -p phosphor-dsp --lib -- --ignored report_delay --nocapture`
    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_delay_decay() {
        println!("{:>6} {:>44}", "fb %", "ratio of one repeat to the last");
        for fb in [20.0f32, 40.0, 60.0, 80.0, 95.0] {
            let mut delay = open_loop(FS);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, 250.0);
            set(&mut delay, PARAM_FEEDBACK, fb);
            let input = burst(1000.0, 0.05, 0.04, 3.0, FS);
            let (wet, _) = render(&mut delay, &input, 120.0);
            let tau = (0.25 * FS) as usize;
            let levels: Vec<f64> = (1..=8)
                .map(|n| {
                    let from = n * tau + (0.010 * FS) as usize;
                    let to = (n * tau + (0.035 * FS) as usize).min(wet.len());
                    rms(&wet[from.min(wet.len())..to])
                })
                .collect();
            let ratios: Vec<String> =
                levels.windows(2).map(|w| format!("{:.4}", w[1] / w[0])).collect();
            println!("{fb:>6} {}", ratios.join(" "));
        }
    }

    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_delay_bound() {
        println!("{:>6} {:>12} {:>12} {:>16}", "fb %", "peak", "bound", "after 30 s");
        for fb in [50.0f32, 95.0, 100.0, 110.0, 150.0, 200.0] {
            let mut delay = open_loop(FS);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, 250.0);
            set(&mut delay, PARAM_FEEDBACK, fb);
            let input = burst(300.0, 0.7, 1.0, 31.0, FS);
            let (wet, _) = render(&mut delay, &input, 120.0);
            println!(
                "{fb:>6} {:>12.4} {:>12.4} {:>16.4}",
                peak(&wet),
                0.7 + f64::from(fb) / 100.0,
                peak(&wet[(FS * 25.0) as usize..])
            );
        }
    }

    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_delay_loop_filters() {
        println!("{:>10} {:>10} {:>34}", "corner", "probe", "dB relative to 1 kHz, per repeat");
        for (probe, name) in [(6_000.0f64, "hicut"), (200.0, "locut")] {
            let mut delay = wet_only(FS);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, 250.0);
            set(&mut delay, PARAM_FEEDBACK, 60.0);
            let input = two_tone(1000.0, probe, 0.15, 0.04, 3.0, FS);
            let (wet, _) = render(&mut delay, &input, 120.0);
            let tau = (0.25 * FS) as usize;
            let mut line = String::new();
            for n in 1..=4 {
                let from = n * tau;
                let to = (from + (0.04 * FS) as usize).min(wet.len());
                let window = &wet[from..to];
                let reference = tone_amplitude(window, 1000.0, FS);
                let here = tone_amplitude(window, probe, FS);
                line.push_str(&format!("{:9.2}", 20.0 * (here / reference.max(1e-12)).log10()));
            }
            println!("{name:>10} {probe:>9.0} {line}");
        }
    }

    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_delay_bbd() {
        println!("{:>8} {:>10} {:>34}", "delay", "corner", "5k against 1k, per repeat (dB)");
        for time_ms in [120.0f32, 300.0, 600.0] {
            let mut delay = wet_only(FS);
            set(&mut delay, PARAM_MODE, Mode::Bbd.index() as f32);
            set(&mut delay, PARAM_SYNC, 0.0);
            set(&mut delay, PARAM_TIME_MS, time_ms);
            set(&mut delay, PARAM_FEEDBACK, 70.0);
            set(&mut delay, PARAM_LOW_CUT_HZ, 20.0);
            set(&mut delay, PARAM_HIGH_CUT_HZ, 20_000.0);
            let input = two_tone(1000.0, 5000.0, 0.2, 0.04, 6.0, FS);
            let (wet, _) = render(&mut delay, &input, 120.0);
            let tau = (f64::from(time_ms) / 1000.0 * FS) as usize;
            let mut line = String::new();
            for n in 1..=5 {
                let from = n * tau;
                let to = (from + (0.04 * FS) as usize).min(wet.len());
                let window = &wet[from..to];
                let low = tone_amplitude(window, 1000.0, FS);
                let high = tone_amplitude(window, 5000.0, FS);
                line.push_str(&format!("{:8.1}", 20.0 * (high / low.max(1.0e-12)).log10()));
            }
            println!(
                "{time_ms:>6} ms {:>8.0} Hz {line}",
                bbd_corner_hz(f64::from(time_ms) / 1000.0)
            );
        }
    }

    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_delay_tape() {
        println!("{:>8} {:>8} {:>12} {:>12} {:>12} {:>14}", "mode", "delay", "0.6 Hz %", "7 Hz %", "58 Hz %", "echo form %");
        for time_ms in [250.0f32, 1000.0] {
            for mode in [Mode::Digital, Mode::Tape] {
                let mut delay = wet_only(FS);
                set(&mut delay, PARAM_MODE, mode.index() as f32);
                set(&mut delay, PARAM_SYNC, 0.0);
                set(&mut delay, PARAM_TIME_MS, time_ms);
                set(&mut delay, PARAM_FEEDBACK, 0.0);
                set(&mut delay, PARAM_LOW_CUT_HZ, 20.0);
                set(&mut delay, PARAM_HIGH_CUT_HZ, 20_000.0);
                let tone: Vec<f32> = (0..(FS * 12.0) as usize)
                    .map(|n| 0.4 * (TAU * 2000.0 * n as f64 / FS).sin() as f32)
                    .collect();
                let (wet, _) = render(&mut delay, &tone, 120.0);
                let track = fm_deviation(&wet[(FS * 2.0) as usize..], 2000.0, FS);
                println!(
                    "{:>8} {time_ms:>6} ms {:>12.5} {:>12.5} {:>12.5} {:>14.5}",
                    mode.label(),
                    track_component(&track, 0.6, FS) * 100.0,
                    track_component(&track, 7.0, FS) * 100.0,
                    track_component(&track, 58.0, FS) * 100.0,
                    f64::from(time_ms) / 1000.0 * TAU * TAPE_WOW.1 * TAPE_WOW.0 * 100.0
                );
            }
        }
    }

    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_delay_ping_pong() {
        println!("{:>8} {:>12} {:>12}", "repeat", "L", "R");
        let mut delay = wet_only(FS);
        set(&mut delay, PARAM_ROUTING, Routing::PingPong.index() as f32);
        set(&mut delay, PARAM_SYNC, 0.0);
        set(&mut delay, PARAM_TIME_MS, 150.0);
        set(&mut delay, PARAM_FEEDBACK, 60.0);
        let input = impulse((FS * 1.5) as usize);
        let (left, right) = render(&mut delay, &input, 120.0);
        let tau = (0.15 * FS) as usize;
        for n in 1..=6 {
            let from = n * tau - 64;
            let to = (n * tau + 2048).min(left.len());
            println!(
                "{n:>8} {:>12.6} {:>12.6}",
                peak(&left[from..to]),
                peak(&right[from..to])
            );
        }
    }

    /// What each mode costs, in nanoseconds a stereo frame and as a share of
    /// one core at 48 kHz. The brief's budgets are 0.31% digital, 0.44% BBD
    /// and 0.48% tape.
    ///
    /// A debug build reports several times the shipped number; run it with
    /// `--release` for anything to be read from it.
    #[test]
    #[ignore = "measurement report, not an assertion"]
    fn report_delay_cost() {
        println!(
            "{:>10} {:>12} {:>8} {:>14} {:>14}",
            "mode", "routing", "heads", "ns / frame", "% of a core"
        );
        for mode in Mode::ALL {
            for routing in [Routing::Stereo, Routing::PingPong] {
                for heads in if mode == Mode::Tape { &[0.0f32, 6.0][..] } else { &[0.0][..] } {
                let mut delay = wet_only(FS);
                set(&mut delay, PARAM_MODE, mode.index() as f32);
                set(&mut delay, PARAM_ROUTING, routing.index() as f32);
                set(&mut delay, PARAM_FEEDBACK, 70.0);
                set(&mut delay, PARAM_HEADS, *heads);
                let mut left: Vec<f32> = (0..512)
                    .map(|n| 0.3 * (TAU * 220.0 * f64::from(n) / FS).sin() as f32)
                    .collect();
                let mut right = left.clone();
                for _ in 0..64 {
                    delay.process(&mut left, &mut right, 120.0);
                }
                let blocks = 4_000;
                let started = std::time::Instant::now();
                for _ in 0..blocks {
                    delay.process(&mut left, &mut right, 120.0);
                }
                let per_frame = started.elapsed().as_secs_f64() / (blocks * 512) as f64;
                println!(
                    "{:>10} {:>12} {:>8} {:>14.1} {:>14.3}",
                    mode.label(),
                    routing.label(),
                    HEAD_LABELS[*heads as usize],
                    per_frame * 1.0e9,
                    per_frame * FS * 100.0
                );
                }
            }
        }
    }
}
