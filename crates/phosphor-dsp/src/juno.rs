//! Roland Juno-60 style single-DCO poly synthesizer with BBD chorus.
//!
//! The whole front panel, in the order it appears on the instrument: LFO,
//! DCO, HPF, VCF, VCA, ENV, CHORUS. One DCO per voice (falling saw, pulse
//! phase-locked to it, divide-by-two sub, noise), the four-position
//! non-resonant HPF, an IR3109 four-pole low-pass, one ADSR shared by the
//! filter and the amplifier, and the BBD chorus in its three modes.
//!
//! Where a number here came from a measurement of the real instrument it says
//! so at the constant. The measurements are Andy Harman's Juno-60 analysis
//! (github.com/pendragon-andyh/Juno60), which is a slider-by-slider capture of
//! a working unit rather than a reading of the service notes.
//!
//! The preset bank is the instrument's own: all 56 factory patches, 11 to 78,
//! read off Roland's published patch charts. Every slider position and switch
//! state in `BANK` was measured from the printed panel of its chart rather
//! than dialled in by ear, which is why the presets are held in panel units
//! here and converted to seconds and hertz on the way into the engine — the
//! chart records where the cap sat, and the seconds are what that means.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

use crate::level::soft_saturate;

const MAX_VOICES: usize = 6;
const TWO_PI: f64 = std::f64::consts::TAU;

/// Fixed headroom trim on the chorus output, applied after the level slider.
///
/// Sized on ordinary playing, in step with the other four — see `OUTPUT_TRIM`
/// in dx7.rs, which carries the full reasoning. The trim lands this synth's
/// median patch at the same loudness as theirs.
///
/// It moved when the factory bank replaced the hand-voiced one, because that
/// is what it is for: the factory patches use their LEVEL slider as a mix
/// control and sit at 0.51 in the middle of the bank where every hand-voiced
/// patch had been left at 0.75, so the same trim put the whole instrument
/// 3.9 dB below the other four. `instruments_are_level_matched` is the
/// assertion that noticed.
///
/// Measured across all 56 on an eight-note chord at velocity 127: the loudest
/// attack is 44 TUBA at 0.515 out of the instrument, and the loudest sample
/// anywhere is 77 SURF at 0.687, which arrives 4.7 s into the chord because
/// the patch is a noise wash swept by an 0.31 Hz LFO. Held for thirty seconds
/// it goes no further. Six voices, so an eight-note chord steals two of them —
/// these are six-voice peaks. Nothing in the bank reaches the saturator's
/// knee, so every patch on every voicing is the trimmed voice sum sample for
/// sample.
const OUTPUT_TRIM: f32 = 0.277;

// ── Parameter indices ──
//
// Front-panel order, left to right, because that is the order a player reaches
// for them. `patch` is first because the panel selector on the instrument is
// the patch bank, and because index 0 is where the editor looks for a preset
// selector.

pub const P_PATCH: usize = 0;
// LFO
pub const P_LFO_RATE: usize = 1;
pub const P_LFO_DELAY: usize = 2;
// DCO
pub const P_DCO_LFO: usize = 3;
pub const P_PWM: usize = 4;
pub const P_PWM_MODE: usize = 5;
pub const P_PULSE: usize = 6;
pub const P_SAW: usize = 7;
pub const P_SUB: usize = 8;
pub const P_NOISE: usize = 9;
pub const P_RANGE: usize = 10;
// HPF
pub const P_HPF: usize = 11;
// VCF
pub const P_CUTOFF: usize = 12;
pub const P_RESO: usize = 13;
pub const P_ENV_POLARITY: usize = 14;
pub const P_ENV_MOD: usize = 15;
pub const P_VCF_LFO: usize = 16;
pub const P_KEY_FOLLOW: usize = 17;
// VCA
pub const P_VCA_MODE: usize = 18;
pub const P_LEVEL: usize = 19;
// ENV
pub const P_ATTACK: usize = 20;
pub const P_DECAY: usize = 21;
pub const P_SUSTAIN: usize = 22;
pub const P_RELEASE: usize = 23;
// CHORUS
pub const P_CHORUS: usize = 24;
pub const PARAM_COUNT: usize = 25;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "patch",
    "lfo rate", "lfo dly",
    "dco lfo", "pwm", "pwm mode", "pulse", "saw", "sub", "noise", "range",
    "hpf",
    "freq", "res", "env pol", "env mod", "vcf lfo", "kybd",
    "vca", "level",
    "attack", "decay", "sustain", "release",
    "chorus",
];

/// Patch 0, factory patch 11 "STRINGS 1", the preset the instrument loads
/// with, in panel units. `patch_zero_is_the_default_parameter_block` holds
/// these and the first row of `BANK` together.
pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    0.0,      // patch: 11 STRINGS 1
    0.608,    // lfo rate: 6.9 Hz
    0.0,      // lfo delay: none
    0.0,      // dco lfo: no vibrato
    0.0,      // pwm: none
    0.16667,  // pwm mode: LFO
    0.25,     // pulse: off
    0.75,     // saw: on
    0.0,      // sub: the button is out
    0.0,      // noise: off
    0.5,      // range: 8'
    0.125,    // hpf: 0, no filtering
    0.714,    // freq
    0.014,    // res
    0.25,     // env polarity: normal
    0.028,    // env mod
    0.014,    // vcf lfo
    1.0,      // kybd follow
    0.25,     // vca: env
    0.511,    // level
    0.412,    // attack: 0.15 s
    0.008,    // decay: 2 ms
    0.992,    // sustain
    0.44,     // release: 0.69 s
    0.375,    // chorus: I
];

// ── Patches ──
//
// The factory bank, in the instrument's own order: seven banks of eight,
// numbered 11-18, 21-28 and so on to 78, which is how a Juno player refers to
// them and so how they are labelled here.

pub const PATCH_COUNT: usize = 56;

/// The factory names, in full. Up to twenty characters, so a caller with a
/// column to fill wants [`PATCH_LABELS`] instead.
pub const PATCH_NAMES: [&str; PATCH_COUNT] = derive_names();

/// The patch numbers as the instrument prints them: bank digit, patch digit.
pub const PATCH_NUMBERS: [&str; PATCH_COUNT] = derive_numbers();

/// Number and name, abbreviated to the twelve columns the editor's selector
/// row leaves for a label. `every_juno_patch_label_fits_the_fx_panel` in the
/// editor holds these to that width.
pub const PATCH_LABELS: [&str; PATCH_COUNT] = derive_labels();

/// The knob position that selects patch `index`, for a caller sweeping the
/// bank from outside — a level measurement, an export, a test.
///
/// The midpoint of the step, which is the one position in it that no amount
/// of float rounding can push into a neighbour, and the same position
/// [`step_discrete`] moves between. `index / (count - 0.01)`, the obvious
/// alternative, is not reliable at every bank size: it happens to be exact
/// for 56 and misses seven of its own indices at 42.
#[must_use]
pub fn patch_knob(index: usize) -> f32 {
    knob_for(index.min(PATCH_COUNT - 1), PATCH_COUNT)
}

/// Which patch a knob position selects. Total: every float lands on a patch,
/// because `params` is public and the knob can arrive as anything.
#[must_use]
pub fn patch_index(value: f32) -> usize {
    selector(value, PATCH_COUNT)
}

// ── Discrete controls ──
//
// Eight of the panel's controls are switches rather than sliders, and the
// patch selector makes nine indices that step. They are stored in the same
// 0..1 parameter block as everything else, so a switch is a knob divided into
// `n` equal steps.

/// How many positions a switch has, or `None` for a slider.
fn discrete_steps(index: usize) -> Option<usize> {
    match index {
        P_PATCH => Some(PATCH_COUNT),
        P_PULSE | P_SAW | P_ENV_POLARITY | P_VCA_MODE => Some(2),
        P_PWM_MODE | P_RANGE => Some(3),
        P_HPF | P_CHORUS => Some(4),
        _ => None,
    }
}

/// One knob into one of `count` equal steps.
///
/// Total by construction: `params` is public, so the knob can arrive as
/// anything at all. The float-to-int cast saturates in both directions and
/// turns NaN into zero, so every input lands on a real position.
fn selector(value: f32, count: usize) -> usize {
    ((value * (count as f32 - 0.01)) as usize).min(count - 1)
}

/// The knob position in the middle of step `index` of `count` — the one
/// position in the step that no amount of float rounding can push into a
/// neighbour. The inverse of [`selector`].
fn knob_for(index: usize, count: usize) -> f32 {
    (index as f32 + 0.5) / count as f32
}

/// Which parameter indices are switches (rendered as labels, not bars).
pub fn is_discrete(index: usize) -> bool {
    discrete_steps(index).is_some()
}

/// The knob position one step up or down from `value`. Sliders are unchanged.
///
/// Steps by *index* rather than by adding a fraction of the travel. Adding
/// 1/56 of the range 56 times does not arrive at 1.0 — the error is a few ulps
/// either way, and a step boundary missed by one ulp is a keypress that
/// visibly does nothing. The DX7's bank knob stalled that way.
pub fn step_discrete(index: usize, value: f32, up: bool) -> f32 {
    let Some(count) = discrete_steps(index) else { return value };
    let current = selector(value, count);
    knob_for(
        if up { (current + 1).min(count - 1) } else { current.saturating_sub(1) },
        count,
    )
}

/// Label for a switch position, or `None` for a slider.
pub fn discrete_label(index: usize, value: f32) -> Option<&'static str> {
    let count = discrete_steps(index)?;
    let step = selector(value, count);
    Some(match index {
        P_PATCH => PATCH_LABELS[step],
        P_PWM_MODE => ["LFO", "MAN", "ENV"][step],
        P_PULSE | P_SAW => ["off", "on"][step],
        P_RANGE => ["16'", "8'", "4'"][step],
        P_HPF => ["0", "1", "2", "3"][step],
        P_ENV_POLARITY => ["norm", "inv"][step],
        P_VCA_MODE => ["ENV", "GATE"][step],
        P_CHORUS => ["off", "I", "II", "I+II"][step],
        _ => return None,
    })
}

/// A slider's value in seconds, for the three that measure time. `None` for
/// the ones that read as a percentage.
pub fn param_seconds(index: usize, value: f32) -> Option<f64> {
    match index {
        P_ATTACK => Some(attack_seconds(f64::from(value))),
        P_DECAY | P_RELEASE => Some(decay_seconds(f64::from(value))),
        _ => None,
    }
}

// ── Panel tapers ──
//
// The sliders are not linear in time or frequency, and on this instrument the
// curves were measured rather than guessed. Attack, decay and release below
// are fits to a real Juno-60; the fit and the measurements it came from are in
// the analysis linked at the top of the file.

/// Attack slider to seconds. Measured: 0 → 1 ms, 2.5 → 30 ms, 5 → 240 ms,
/// 7.5 → 650 ms, 10 → 3.25 s on the instrument's 0-10 scale.
const ATTACK_MIN: f64 = 0.001;
const ATTACK_MAX: f64 = 3.25;
const ATTACK_CURVE: f64 = 5.0;

fn attack_seconds(slider: f64) -> f64 {
    let s = slider.clamp(0.0, 1.0);
    ATTACK_MIN + (ATTACK_CURVE * s).exp_m1() / ATTACK_CURVE.exp_m1() * ATTACK_MAX
}

/// Decay and release share one taper. Measured: 0 → 2 ms, 2.5 → 96 ms,
/// 5 → 984 ms, 7.5 → 4.45 s, 10 → 19.8 s. The fit runs to 17.5 s, which is
/// inside the spread of the two runs that were taken at the top of the slider.
const DECAY_MIN: f64 = 0.002;
const DECAY_MAX: f64 = 17.46;
const DECAY_CURVE: f64 = 4.0;

fn decay_seconds(slider: f64) -> f64 {
    let s = slider.clamp(0.0, 1.0);
    DECAY_MIN + (DECAY_CURVE * s).exp_m1() / DECAY_CURVE.exp_m1() * s * DECAY_MAX
}

/// Straight-line interpolation into a table measured at the five slider
/// positions the analysis used — 0, 2.5, 5, 7.5 and 10 on the instrument's own
/// scale. Interpolated rather than fitted, because a fit through five points
/// would only be smoothing the measurement.
fn interpolate5(table: &[f64; 5], slider: f64) -> f64 {
    let x = slider.clamp(0.0, 1.0) * 4.0;
    let i = (x as usize).min(3);
    let f = x - i as f64;
    table[i] + f * (table[i + 1] - table[i])
}

/// LFO rate slider to Hz. Measured: 0 → 0.3 Hz, 2.5 → 0.85, 5 → 3.39,
/// 7.5 → 11.49, 10 → 22.22, which brackets the 0.3-20 Hz Roland quote.
///
/// This used to be a straight line between the two ends, on the grounds that
/// no measurement of the taper was to hand. There is one — the recordings the
/// rest of the analysis came from — and the taper is nothing like a straight
/// line: the middle of the slider is 3.4 Hz where a straight line puts 10.2.
/// It matters because the factory bank leaves this slider between 0.4 and
/// 0.65 on 38 of its 56 patches, so 38 patches were modulating three times
/// too fast.
const LFO_RATE_HZ: [f64; 5] = [0.3, 0.85, 3.39, 11.49, 22.22];

fn lfo_hz(slider: f64) -> f64 {
    interpolate5(&LFO_RATE_HZ, slider)
}

/// LFO delay slider to (silence, fade) seconds.
///
/// Two stages, not one: the measured instrument holds the LFO off entirely and
/// *then* fades it in.
const LFO_DELAY_HOLD: [f64; 5] = [0.0, 0.0639, 0.85, 1.2, 2.786];
const LFO_DELAY_FADE: [f64; 5] = [0.0, 0.053, 0.188, 0.348, 1.0];

fn lfo_delay_times(slider: f64) -> (f64, f64) {
    (interpolate5(&LFO_DELAY_HOLD, slider), interpolate5(&LFO_DELAY_FADE, slider))
}

/// Cutoff slider to Hz: 10 Hz to 10 kHz, exponential. This is the one range on
/// the panel with no measurement behind it — Roland do not publish the VCF
/// sweep and the analysis does not cover it.
const CUTOFF_MIN_HZ: f64 = 10.0;
const CUTOFF_DECADES: f64 = 3.0;
/// How many octaves the cutoff slider covers end to end, which is what turns
/// a keyboard-follow amount into a slider offset.
const CUTOFF_OCTAVES: f64 = CUTOFF_DECADES * std::f64::consts::LOG2_10;

fn cutoff_hz(slider: f64) -> f64 {
    CUTOFF_MIN_HZ * 10.0f64.powf(CUTOFF_DECADES * slider.clamp(0.0, 1.0))
}

/// Full LFO-to-pitch is a semitone of vibrato either way.
const LFO_PITCH_CENTS: f64 = 100.0;

/// Pulse width swings this far either side of square at full PWM. Past about
/// 0.45 the pulse collapses to nothing at the ends of the sweep.
const PW_SWING: f64 = 0.45;

// ── The factory bank ──
//
// One row per patch, in panel units: where each slider sat and which way each
// switch was thrown on Roland's chart for that patch. That is what the chart
// records, so that is what is stored — the seconds and hertz behind a row come
// out of the same tapers a player's own hand on the panel goes through, in
// `active_patch`.
//
// Two things the chart does not record, and where they came from:
//
// * the OCTAVE TRANSPOSE switch, which the manual says is memorised with the
//   patch but which the printed panel on the chart leaves out. The `range`
//   column is Andy Harman's transcription of the same bank;
// * the HPF, drawn on the chart as a slider though the control has four
//   detents. The caps cluster on four heights and `hpf` is which one, with
//   the bass patches at the bottom of the travel and the reeds at the top.
//
// The chart also records knob positions that do nothing on their patch —
// Roland say as much in the manual — the commonest being a SUB OSC fader left
// up with the sub button out. `params_for_patch` gates the fader on the
// button, since the panel here has no button to leave out.

/// One row of the factory chart.
#[derive(Debug, Clone, Copy)]
struct Chart {
    /// The instrument's own patch number, "11" to "78".
    number: &'static str,
    name: &'static str,
    /// Number and name, cut to twelve columns for the editor.
    label: &'static str,
    /// RATE, DELAY TIME.
    lfo: [f32; 2],
    pwm_mode: PwmSource,
    /// LFO, PWM, SUB OSC, NOISE.
    dco: [f32; 4],
    /// The pulse, sawtooth and sub-oscillator buttons.
    waves: [bool; 3],
    /// OCTAVE TRANSPOSE: 0 = down, 1 = normal, 2 = up.
    range: u8,
    /// HPF: 0 = no filtering, 1-3 = the three corners.
    hpf: u8,
    /// The VCF's ENV polarity switch.
    invert: bool,
    /// FREQ, RES, ENV, LFO, KYBD.
    vcf: [f32; 5],
    /// The VCA's ENV/GATE switch.
    gate: bool,
    level: f32,
    /// A, D, S, R.
    env: [f32; 4],
    /// 0 = off, 1 = I, 2 = II. No factory patch presses both.
    chorus: u8,
}

// Unqualified in the table below, where a column of `PwmSource::Manual` would
// be three times the width of the thing it says.
use PwmSource::{Env, Lfo, Manual};

const BANK: [Chart; PATCH_COUNT] = [
    Chart { number: "11", name: "STRINGS 1", label: "11 STRINGS 1",
        lfo: [0.608, 0.000], pwm_mode: Lfo, dco: [0.000, 0.000, 0.008, 0.000],
        waves: [false, true , false], range: 1, hpf: 0, invert: false,
        vcf: [0.714, 0.014, 0.028, 0.014, 1.000], gate: false, level: 0.511,
        env: [0.412, 0.008, 0.992, 0.440], chorus: 1 },
    Chart { number: "12", name: "STRINGS 2", label: "12 STRINGS 2",
        lfo: [0.398, 0.008], pwm_mode: Lfo, dco: [0.008, 0.608, 0.000, 0.014],
        waves: [true , true , false], range: 1, hpf: 0, invert: false,
        vcf: [0.497, 0.588, 0.000, 0.008, 0.986], gate: false, level: 0.300,
        env: [0.406, 0.406, 0.992, 0.805], chorus: 2 },
    Chart { number: "13", name: "STRINGS 3", label: "13 STRINGS 3",
        lfo: [0.329, 0.805], pwm_mode: Lfo, dco: [0.008, 0.714, 0.992, 0.000],
        waves: [true , true , true ], range: 1, hpf: 0, invert: false,
        vcf: [0.497, 0.008, 0.008, 0.022, 1.000], gate: false, level: 0.329,
        env: [0.315, 0.000, 0.992, 0.602], chorus: 2 },
    Chart { number: "14", name: "ORGAN 1", label: "14 ORGAN 1",
        lfo: [0.308, 0.805], pwm_mode: Manual, dco: [0.008, 0.511, 0.992, 0.000],
        waves: [true , false, true ], range: 1, hpf: 0, invert: false,
        vcf: [0.398, 0.594, 0.454, 0.008, 1.000], gate: true , level: 0.511,
        env: [0.000, 0.000, 0.000, 0.000], chorus: 1 },
    Chart { number: "15", name: "ORGAN 2", label: "15 ORGAN 2",
        lfo: [0.511, 0.252], pwm_mode: Lfo, dco: [0.000, 0.560, 0.818, 0.000],
        waves: [true , false, true ], range: 1, hpf: 0, invert: false,
        vcf: [0.371, 0.560, 0.028, 0.014, 1.000], gate: true , level: 0.511,
        env: [0.008, 0.126, 0.000, 0.118], chorus: 1 },
    Chart { number: "16", name: "ORGAN 3", label: "16 ORGAN 3",
        lfo: [0.517, 0.385], pwm_mode: Lfo, dco: [0.008, 0.566, 0.805, 0.014],
        waves: [true , false, true ], range: 2, hpf: 0, invert: false,
        vcf: [0.357, 0.552, 0.357, 0.000, 0.986], gate: true , level: 0.497,
        env: [0.000, 0.000, 0.105, 0.000], chorus: 2 },
    Chart { number: "17", name: "BRASS", label: "17 BRASS",
        lfo: [0.511, 0.665], pwm_mode: Manual, dco: [0.168, 0.008, 0.000, 0.000],
        waves: [false, true , false], range: 1, hpf: 0, invert: false,
        vcf: [0.000, 0.008, 0.854, 0.022, 0.412], gate: false, level: 0.692,
        env: [0.258, 0.398, 0.623, 0.217], chorus: 1 },
    Chart { number: "18", name: "PHASE BRASS", label: "18 PHS BRASS",
        lfo: [0.608, 0.000], pwm_mode: Env, dco: [0.008, 1.000, 0.992, 0.000],
        waves: [true , true , false], range: 1, hpf: 0, invert: false,
        vcf: [0.322, 0.105, 0.552, 0.008, 1.000], gate: true , level: 0.412,
        env: [0.238, 0.440, 0.440, 0.322], chorus: 1 },
    Chart { number: "21", name: "PIANO 1", label: "21 PIANO 1",
        lfo: [0.608, 0.069], pwm_mode: Manual, dco: [0.371, 0.602, 0.008, 0.000],
        waves: [true , false, false], range: 1, hpf: 0, invert: false,
        vcf: [0.105, 0.000, 0.692, 0.014, 0.392], gate: false, level: 0.692,
        env: [0.008, 0.818, 0.175, 0.315], chorus: 0 },
    Chart { number: "22", name: "PIANO 2", label: "22 PIANO 2",
        lfo: [0.398, 0.000], pwm_mode: Manual, dco: [0.008, 0.406, 0.440, 0.014],
        waves: [true , false, true ], range: 2, hpf: 0, invert: false,
        vcf: [0.357, 0.000, 0.258, 0.189, 0.783], gate: false, level: 0.805,
        env: [0.000, 0.734, 0.000, 0.357], chorus: 0 },
    Chart { number: "23", name: "CELESTA", label: "23 CELESTA",
        lfo: [0.335, 0.588], pwm_mode: Env, dco: [0.000, 0.497, 0.978, 0.000],
        waves: [true , true , false], range: 1, hpf: 0, invert: false,
        vcf: [0.085, 0.777, 0.000, 0.008, 0.958], gate: false, level: 0.580,
        env: [0.000, 0.651, 0.203, 0.846], chorus: 1 },
    Chart { number: "24", name: "MELLOW PIANO", label: "24 MEL PIANO",
        lfo: [0.511, 0.000], pwm_mode: Manual, dco: [0.008, 0.511, 0.992, 0.000],
        waves: [true , false, false], range: 1, hpf: 0, invert: false,
        vcf: [0.322, 0.008, 0.272, 0.105, 0.895], gate: false, level: 0.692,
        env: [0.118, 0.762, 0.217, 0.846], chorus: 1 },
    Chart { number: "25", name: "HARPSICHORD 1", label: "25 HARPSI 1",
        lfo: [0.511, 0.412], pwm_mode: Manual, dco: [0.008, 0.343, 0.692, 0.008],
        waves: [true , false, true ], range: 2, hpf: 1, invert: false,
        vcf: [0.308, 0.000, 0.511, 0.014, 0.714], gate: false, level: 0.412,
        env: [0.008, 0.608, 0.377, 0.280], chorus: 1 },
    Chart { number: "26", name: "HARPSICHORD 2", label: "26 HARPSI 2",
        lfo: [0.538, 0.588], pwm_mode: Manual, dco: [0.008, 0.203, 0.818, 0.014],
        waves: [true , false, true ], range: 2, hpf: 1, invert: false,
        vcf: [0.483, 0.231, 0.300, 0.000, 0.965], gate: false, level: 0.483,
        env: [0.000, 0.511, 0.168, 0.497], chorus: 2 },
    Chart { number: "27", name: "GUITAR", label: "27 GUITAR",
        lfo: [0.580, 0.566], pwm_mode: Manual, dco: [0.000, 0.594, 0.992, 0.000],
        waves: [true , false, false], range: 1, hpf: 2, invert: false,
        vcf: [0.300, 0.008, 0.434, 0.154, 0.497], gate: false, level: 0.902,
        env: [0.000, 0.566, 0.343, 0.629], chorus: 0 },
    Chart { number: "28", name: "SYNTHESIZER HARP", label: "28 SYN HARP",
        lfo: [0.308, 0.805], pwm_mode: Manual, dco: [0.008, 0.008, 1.000, 0.000],
        waves: [false, true , false], range: 1, hpf: 0, invert: false,
        vcf: [0.322, 0.008, 0.511, 0.008, 0.818], gate: false, level: 0.608,
        env: [0.000, 0.580, 0.322, 0.525], chorus: 1 },
    Chart { number: "31", name: "BASS 1", label: "31 BASS 1",
        lfo: [0.511, 0.475], pwm_mode: Manual, dco: [0.008, 0.511, 0.308, 0.008],
        waves: [true , true , true ], range: 0, hpf: 0, invert: false,
        vcf: [0.308, 0.238, 0.371, 0.014, 0.008], gate: true , level: 0.511,
        env: [0.008, 0.392, 0.118, 0.280], chorus: 1 },
    Chart { number: "32", name: "BASS 2", label: "32 BASS 2",
        lfo: [0.483, 0.588], pwm_mode: Manual, dco: [0.008, 0.489, 0.300, 0.014],
        waves: [true , true , false], range: 0, hpf: 0, invert: false,
        vcf: [0.315, 0.469, 0.440, 0.000, 0.497], gate: true , level: 0.398,
        env: [0.000, 0.308, 0.349, 0.258], chorus: 1 },
    Chart { number: "33", name: "CLAVICHORD 1", label: "33 CLAVI 1",
        lfo: [0.608, 0.280], pwm_mode: Manual, dco: [0.008, 0.511, 0.014, 0.000],
        waves: [true , false, false], range: 0, hpf: 0, invert: false,
        vcf: [0.000, 0.308, 0.791, 0.008, 0.629], gate: false, level: 0.714,
        env: [0.014, 0.497, 0.377, 0.175], chorus: 1 },
    Chart { number: "34", name: "CLAVICHORD 2", label: "34 CLAVI 2",
        lfo: [0.580, 0.791], pwm_mode: Manual, dco: [0.000, 0.497, 0.992, 0.000],
        waves: [true , false, false], range: 0, hpf: 1, invert: false,
        vcf: [0.566, 0.678, 0.195, 0.258, 0.678], gate: false, level: 0.986,
        env: [0.000, 0.469, 0.203, 0.203], chorus: 0 },
    Chart { number: "35", name: "PIZZICATO SOUND 1", label: "35 PIZZ 1",
        lfo: [0.497, 0.462], pwm_mode: Manual, dco: [0.000, 0.335, 0.294, 0.000],
        waves: [true , false, false], range: 1, hpf: 0, invert: false,
        vcf: [0.440, 0.300, 0.335, 0.300, 0.986], gate: false, level: 0.805,
        env: [0.000, 0.195, 0.343, 0.566], chorus: 1 },
    Chart { number: "36", name: "PIZZICATO SOUND 2", label: "36 PIZZ 2",
        lfo: [0.483, 0.588], pwm_mode: Manual, dco: [0.008, 0.203, 0.300, 0.014],
        waves: [true , false, true ], range: 2, hpf: 0, invert: false,
        vcf: [0.497, 0.308, 0.315, 0.000, 0.986], gate: false, level: 0.602,
        env: [0.000, 0.308, 0.349, 0.377], chorus: 2 },
    Chart { number: "37", name: "XYLOPHONE", label: "37 XYLOPHONE",
        lfo: [0.511, 0.000], pwm_mode: Manual, dco: [0.008, 0.511, 0.992, 0.000],
        waves: [false, false, true ], range: 2, hpf: 1, invert: false,
        vcf: [0.440, 0.511, 0.308, 0.008, 0.629], gate: false, level: 1.000,
        env: [0.014, 0.357, 0.000, 0.357], chorus: 0 },
    Chart { number: "38", name: "GLOCKENSPEIL", label: "38 GLOCKEN",
        lfo: [0.511, 0.000], pwm_mode: Manual, dco: [0.008, 0.028, 0.000, 0.000],
        waves: [true , false, false], range: 2, hpf: 1, invert: false,
        vcf: [0.580, 0.511, 0.329, 0.008, 0.594], gate: false, level: 0.895,
        env: [0.000, 0.343, 0.300, 0.503], chorus: 0 },
    Chart { number: "41", name: "VIOLINE", label: "41 VIOLINE",
        lfo: [0.594, 0.475], pwm_mode: Lfo, dco: [0.231, 0.008, 0.000, 0.008],
        waves: [false, true , false], range: 1, hpf: 1, invert: false,
        vcf: [0.651, 0.000, 0.008, 0.000, 1.000], gate: false, level: 0.692,
        env: [0.412, 0.008, 1.000, 0.440], chorus: 0 },
    Chart { number: "42", name: "TRUMPET", label: "42 TRUMPET",
        lfo: [0.258, 0.629], pwm_mode: Manual, dco: [0.168, 0.008, 0.000, 0.014],
        waves: [false, true , false], range: 1, hpf: 0, invert: false,
        vcf: [0.014, 0.022, 0.846, 0.000, 0.420], gate: false, level: 0.700,
        env: [0.146, 0.406, 0.588, 0.217], chorus: 0 },
    Chart { number: "43", name: "HORN", label: "43 HORN",
        lfo: [0.266, 0.678], pwm_mode: Manual, dco: [0.008, 0.008, 0.014, 0.000],
        waves: [false, true , false], range: 1, hpf: 0, invert: false,
        vcf: [0.217, 0.008, 0.552, 0.203, 0.412], gate: false, level: 0.714,
        env: [0.398, 0.497, 0.580, 0.343], chorus: 0 },
    Chart { number: "44", name: "TUBA", label: "44 TUBA",
        lfo: [0.272, 0.706], pwm_mode: Manual, dco: [0.168, 0.028, 0.000, 0.000],
        waves: [false, true , false], range: 0, hpf: 0, invert: false,
        vcf: [0.162, 0.008, 0.594, 0.008, 0.412], gate: false, level: 1.000,
        env: [0.322, 0.398, 0.398, 0.322], chorus: 0 },
    Chart { number: "45", name: "FLUTE", label: "45 FLUTE",
        lfo: [0.538, 0.497], pwm_mode: Manual, dco: [0.000, 0.000, 0.000, 0.154],
        waves: [false, true , false], range: 2, hpf: 1, invert: false,
        vcf: [0.475, 0.000, 0.000, 0.182, 0.594], gate: false, level: 0.986,
        env: [0.195, 0.594, 0.483, 0.266], chorus: 0 },
    Chart { number: "46", name: "CLARINET", label: "46 CLARINET",
        lfo: [0.497, 0.671], pwm_mode: Manual, dco: [0.168, 0.008, 0.000, 0.014],
        waves: [true , false, false], range: 1, hpf: 1, invert: false,
        vcf: [0.483, 0.308, 0.258, 0.000, 0.580], gate: false, level: 0.580,
        env: [0.146, 0.588, 0.588, 0.258], chorus: 0 },
    Chart { number: "47", name: "OBOE", label: "47 OBOE",
        lfo: [0.538, 0.651], pwm_mode: Manual, dco: [0.154, 0.637, 0.000, 0.000],
        waves: [true , false, false], range: 1, hpf: 3, invert: false,
        vcf: [0.448, 0.497, 0.252, 0.000, 0.517], gate: false, level: 0.700,
        env: [0.182, 0.588, 0.566, 0.266], chorus: 0 },
    Chart { number: "48", name: "ENGLISH HORN", label: "48 ENG HORN",
        lfo: [0.497, 0.692], pwm_mode: Manual, dco: [0.217, 0.637, 0.000, 0.000],
        waves: [true , false, false], range: 0, hpf: 3, invert: false,
        vcf: [0.489, 0.700, 0.000, 0.154, 0.497], gate: false, level: 0.986,
        env: [0.189, 0.588, 0.566, 0.245], chorus: 0 },
    Chart { number: "51", name: "FUNNY CAT", label: "51 FUNNY CAT",
        lfo: [0.623, 0.055], pwm_mode: Manual, dco: [0.294, 0.000, 0.000, 0.000],
        waves: [false, true , false], range: 1, hpf: 1, invert: false,
        vcf: [0.154, 0.748, 0.497, 0.223, 0.497], gate: false, level: 0.805,
        env: [0.258, 0.398, 0.978, 0.105], chorus: 0 },
    Chart { number: "52", name: "WAH BRASS", label: "52 WAH BRASS",
        lfo: [0.602, 0.189], pwm_mode: Manual, dco: [0.308, 0.008, 0.000, 0.014],
        waves: [false, true , false], range: 1, hpf: 0, invert: false,
        vcf: [0.315, 0.706, 0.440, 0.000, 0.580], gate: true , level: 0.678,
        env: [0.175, 0.286, 0.406, 0.258], chorus: 0 },
    Chart { number: "53", name: "PHASE COMBINATION", label: "53 PHS COMB",
        lfo: [0.608, 0.217], pwm_mode: Manual, dco: [0.008, 0.811, 0.014, 0.000],
        waves: [true , true , false], range: 1, hpf: 0, invert: false,
        vcf: [0.643, 0.231, 0.329, 0.008, 0.203], gate: false, level: 0.308,
        env: [0.014, 0.720, 0.217, 0.238], chorus: 1 },
    Chart { number: "54", name: "REED 1", label: "54 REED 1",
        lfo: [0.608, 0.217], pwm_mode: Manual, dco: [0.412, 0.008, 0.000, 0.000],
        waves: [true , false, false], range: 1, hpf: 0, invert: false,
        vcf: [0.098, 0.608, 0.692, 0.008, 0.511], gate: true , level: 0.608,
        env: [0.000, 0.888, 0.503, 0.140], chorus: 1 },
    Chart { number: "55", name: "POPCORN", label: "55 POPCORN",
        lfo: [0.014, 0.000], pwm_mode: Manual, dco: [0.000, 0.008, 1.000, 0.008],
        waves: [false, false, true ], range: 2, hpf: 1, invert: false,
        vcf: [0.272, 0.217, 0.552, 0.014, 1.000], gate: false, level: 0.818,
        env: [0.028, 0.329, 0.238, 0.014], chorus: 0 },
    Chart { number: "56", name: "REED 2", label: "56 REED 2",
        lfo: [0.343, 0.805], pwm_mode: Manual, dco: [0.000, 0.022, 1.000, 0.028],
        waves: [false, false, true ], range: 2, hpf: 0, invert: false,
        vcf: [0.231, 0.022, 0.594, 0.022, 0.818], gate: false, level: 0.511,
        env: [0.000, 0.580, 0.322, 0.637], chorus: 1 },
    Chart { number: "57", name: "REED 3", label: "57 REED 3",
        lfo: [0.594, 0.203], pwm_mode: Manual, dco: [0.189, 0.517, 0.978, 0.000],
        waves: [false, false, true ], range: 2, hpf: 0, invert: false,
        vcf: [0.329, 0.217, 0.315, 0.000, 0.986], gate: false, level: 0.594,
        env: [0.286, 0.000, 0.978, 0.223], chorus: 0 },
    Chart { number: "58", name: "PWM CHORUS", label: "58 PWM CHOR",
        lfo: [0.308, 0.000], pwm_mode: Lfo, dco: [0.008, 0.511, 0.992, 0.000],
        waves: [true , false, true ], range: 1, hpf: 0, invert: false,
        vcf: [0.805, 0.008, 0.028, 0.008, 0.972], gate: false, level: 0.209,
        env: [0.300, 0.000, 0.992, 0.440], chorus: 2 },
    Chart { number: "61", name: "SYNTHESIZER ORGAN", label: "61 SYN ORGAN",
        lfo: [0.448, 0.462], pwm_mode: Manual, dco: [0.000, 0.637, 0.742, 0.000],
        waves: [true , false, true ], range: 1, hpf: 0, invert: false,
        vcf: [0.258, 0.000, 0.497, 0.203, 0.678], gate: true , level: 0.497,
        env: [0.014, 0.195, 0.511, 0.266], chorus: 2 },
    Chart { number: "62", name: "EFFECT SOUND 1", label: "62 EFFECT 1",
        lfo: [0.426, 0.608], pwm_mode: Manual, dco: [0.168, 0.978, 0.678, 0.014],
        waves: [true , true , true ], range: 2, hpf: 0, invert: true ,
        vcf: [0.623, 0.008, 0.440, 0.008, 0.700], gate: false, level: 0.678,
        env: [0.000, 0.511, 0.000, 0.580], chorus: 1 },
    Chart { number: "63", name: "EFFECT SOUND 2", label: "63 EFFECT 2",
        lfo: [0.552, 0.888], pwm_mode: Lfo, dco: [0.000, 0.315, 0.651, 0.000],
        waves: [false, true , true ], range: 1, hpf: 0, invert: true ,
        vcf: [0.629, 0.315, 0.398, 0.000, 0.112], gate: true , level: 0.398,
        env: [0.629, 0.525, 0.223, 0.651], chorus: 1 },
    Chart { number: "64", name: "SPACE HARP", label: "64 SPC HARP",
        lfo: [0.552, 0.000], pwm_mode: Env, dco: [0.231, 0.008, 0.022, 0.000],
        waves: [false, true , false], range: 1, hpf: 0, invert: false,
        vcf: [0.665, 0.511, 0.552, 0.008, 0.972], gate: false, level: 0.608,
        env: [0.022, 0.825, 0.825, 0.902], chorus: 1 },
    Chart { number: "65", name: "FUNK", label: "65 FUNK",
        lfo: [0.315, 0.272], pwm_mode: Manual, dco: [0.000, 0.608, 1.000, 0.008],
        waves: [true , true , true ], range: 1, hpf: 0, invert: true ,
        vcf: [0.734, 0.602, 0.511, 0.000, 0.469], gate: true , level: 0.209,
        env: [0.594, 0.511, 0.014, 0.014], chorus: 1 },
    Chart { number: "66", name: "SPACE SOUND 1", label: "66 SPACE 1",
        lfo: [0.608, 0.692], pwm_mode: Manual, dco: [0.203, 0.448, 0.986, 0.014],
        waves: [true , false, true ], range: 1, hpf: 0, invert: true ,
        vcf: [0.665, 0.692, 0.560, 0.008, 0.986], gate: true , level: 0.300,
        env: [0.000, 0.511, 0.000, 0.300], chorus: 1 },
    Chart { number: "67", name: "MYSTERIOUS INVENTION", label: "67 MYSTERY",
        lfo: [0.594, 0.811], pwm_mode: Env, dco: [0.189, 0.797, 0.978, 0.000],
        waves: [true , true , false], range: 1, hpf: 0, invert: true ,
        vcf: [0.791, 0.678, 0.594, 0.272, 0.000], gate: false, level: 0.475,
        env: [0.000, 0.978, 0.000, 0.992], chorus: 0 },
    Chart { number: "68", name: "SPACE SOUND 2", label: "68 SPACE 2",
        lfo: [0.294, 0.308], pwm_mode: Manual, dco: [0.000, 0.594, 0.791, 0.000],
        waves: [true , true , false], range: 1, hpf: 0, invert: false,
        vcf: [0.203, 0.838, 0.594, 0.000, 0.958], gate: false, level: 0.217,
        env: [0.992, 0.992, 0.992, 0.992], chorus: 1 },
    Chart { number: "71", name: "PERCUSSIVE SOUND 1", label: "71 PERC 1",
        lfo: [0.014, 0.000], pwm_mode: Env, dco: [0.000, 0.000, 0.000, 1.000],
        waves: [false, false, false], range: 2, hpf: 1, invert: false,
        vcf: [0.392, 1.000, 0.168, 0.000, 1.000], gate: false, level: 1.000,
        env: [0.008, 0.329, 0.014, 0.440], chorus: 0 },
    Chart { number: "72", name: "PERCUSSIVE SOUND 2", label: "72 PERC 2",
        lfo: [0.022, 0.022], pwm_mode: Env, dco: [0.022, 0.000, 0.008, 0.028],
        waves: [false, false, false], range: 1, hpf: 1, invert: true ,
        vcf: [0.511, 1.000, 0.371, 0.022, 1.000], gate: false, level: 1.000,
        env: [0.000, 0.322, 0.000, 0.412], chorus: 0 },
    Chart { number: "73", name: "WHISTLE", label: "73 WHISTLE",
        lfo: [0.552, 0.483], pwm_mode: Manual, dco: [0.000, 0.000, 0.000, 0.203],
        waves: [false, false, false], range: 2, hpf: 1, invert: false,
        vcf: [0.363, 0.958, 0.154, 0.217, 0.986], gate: false, level: 0.797,
        env: [0.300, 0.000, 0.992, 0.085], chorus: 0 },
    Chart { number: "74", name: "EFFECT SOUND 3", label: "74 EFFECT 3",
        lfo: [0.538, 0.406], pwm_mode: Env, dco: [0.000, 0.000, 0.000, 0.000],
        waves: [false, false, false], range: 1, hpf: 1, invert: false,
        vcf: [0.343, 1.000, 0.014, 0.195, 0.986], gate: false, level: 0.958,
        env: [0.000, 0.426, 0.566, 0.692], chorus: 0 },
    Chart { number: "75", name: "UFO", label: "75 UFO",
        lfo: [0.602, 0.000], pwm_mode: Manual, dco: [0.000, 0.000, 0.000, 0.209],
        waves: [false, false, false], range: 2, hpf: 0, invert: false,
        vcf: [0.008, 0.965, 0.692, 0.398, 1.000], gate: false, level: 0.426,
        env: [0.008, 0.608, 0.992, 0.825], chorus: 1 },
    Chart { number: "76", name: "SPACE SOUND 3", label: "76 SPACE 3",
        lfo: [0.580, 0.022], pwm_mode: Manual, dco: [0.022, 0.000, 0.008, 0.209],
        waves: [false, false, false], range: 2, hpf: 0, invert: true ,
        vcf: [0.511, 0.992, 0.412, 0.022, 1.000], gate: false, level: 0.511,
        env: [0.000, 0.992, 0.000, 0.818], chorus: 1 },
    Chart { number: "77", name: "SURF", label: "77 SURF",
        lfo: [0.008, 0.000], pwm_mode: Env, dco: [0.008, 0.008, 0.965, 0.965],
        waves: [false, false, false], range: 1, hpf: 0, invert: false,
        vcf: [0.602, 0.008, 0.022, 0.608, 1.000], gate: false, level: 0.915,
        env: [0.000, 0.398, 1.000, 0.825], chorus: 0 },
    Chart { number: "78", name: "SYNTHESIZER DRUM", label: "78 SYN DRUM",
        lfo: [0.008, 0.000], pwm_mode: Env, dco: [0.008, 0.008, 0.000, 0.000],
        waves: [false, false, false], range: 1, hpf: 0, invert: false,
        vcf: [0.217, 1.000, 0.412, 0.008, 1.000], gate: false, level: 0.608,
        env: [0.000, 0.525, 0.000, 0.602], chorus: 0 },
];

// The three public name tables are the bank's own columns rather than copies
// of them, so a patch cannot be renamed in one place and not the other.

const fn derive_names() -> [&'static str; PATCH_COUNT] {
    let mut out = [""; PATCH_COUNT];
    let mut i = 0;
    while i < PATCH_COUNT {
        out[i] = BANK[i].name;
        i += 1;
    }
    out
}

const fn derive_numbers() -> [&'static str; PATCH_COUNT] {
    let mut out = [""; PATCH_COUNT];
    let mut i = 0;
    while i < PATCH_COUNT {
        out[i] = BANK[i].number;
        i += 1;
    }
    out
}

const fn derive_labels() -> [&'static str; PATCH_COUNT] {
    let mut out = [""; PATCH_COUNT];
    let mut i = 0;
    while i < PATCH_COUNT {
        out[i] = BANK[i].label;
        i += 1;
    }
    out
}

// ── Internal preset ──
//
// Physical units — seconds, hertz, normalised cutoff — because that is what
// the engine runs on. The panel block above is the f32 parameter vector;
// `params_for_patch` loads a chart row into it and `active_patch` reads the
// whole block back out in these units, so every control stays live after a
// preset is loaded. `preset_round_trip` holds the two directions together.

/// Where the pulse width comes from. Roland's specification for the
/// instrument gives the switch as ENV/MANUAL/LFO — three positions, which is
/// one more than this had before the factory bank arrived using all three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PwmSource {
    /// The LFO sweeps the width either side of square.
    Lfo = 0,
    /// The slider sets the width and nothing moves it.
    Manual = 1,
    /// The envelope sweeps the width, once per note.
    Env = 2,
}

impl PwmSource {
    /// Total: the switch is a public parameter, so anything can arrive here.
    fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Lfo,
            2 => Self::Env,
            _ => Self::Manual,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct JunoPatch {
    // LFO
    lfo_rate: f64,       // Hz
    lfo_delay: f64,      // panel 0..1: hold and fade come from the measured table
    // DCO
    lfo_to_pitch: f64,   // 0..1, full = ±1 semitone
    pwm_depth: f64,      // width swing either side of square
    pwm_mode: PwmSource,
    pulse: bool,
    saw: bool,
    sub_level: f64,
    noise_level: f64,
    range: u8,           // 0 = 16', 1 = 8', 2 = 4'
    // HPF
    hpf: u8,             // 0 = off, 1-3 = 154/339/720 Hz
    // VCF
    cutoff: f64,
    resonance: f64,
    env_mod: f64,        // signed: the polarity switch is the sign
    lfo_to_filter: f64,
    key_follow: f64,     // 1.0 = one octave of cutoff per octave of keyboard
    // VCA
    vca_gate: bool,
    level: f64,
    // ENV
    attack: f64, decay: f64, sustain: f64, release: f64,  // seconds
    // CHORUS
    chorus: u8,          // 0 = off, 1 = I, 2 = II, 3 = I+II
}


// ── PolyBLEP ──

#[inline]
fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let t = t / dt;
        2.0 * t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

#[inline]
fn tanh_approx(x: f64) -> f64 {
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

// ── DCO ──
//
// One ramp per voice, reset by the divider. On a Juno-6/60 the reset is a PNP
// transistor fired on the falling edge of the clock and the integrator charges
// positive, so the ramp is a *falling* sawtooth; the pulse is a comparator on
// that same ramp, so it is phase-locked to it and its edge lands wherever the
// threshold crosses; the sub is a D-type flip-flop on the clock, so it is
// exactly half the frequency and in phase with the reset.

#[derive(Debug, Clone)]
struct JunoDco {
    phase: f64,
    sub_phase: bool, // flip-flop for sub-oscillator
    freq: f64,
    dt: f64,
}

impl JunoDco {
    fn new() -> Self {
        Self { phase: 0.0, sub_phase: false, freq: 440.0, dt: 0.01 }
    }

    fn set_freq(&mut self, freq: f64, sr: f64) {
        self.freq = freq.clamp(0.1, sr * 0.45);
        self.dt = self.freq / sr;
    }

    /// Returns (saw, pulse, sub_osc).
    fn tick(&mut self, pulse_width: f64) -> (f64, f64, f64) {
        let dt = self.dt;
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.sub_phase = !self.sub_phase; // divide by 2
        }
        let t = self.phase;

        // Sawtooth: falling, 1 down to -1 across the period
        let saw = 1.0 - 2.0 * t + poly_blep(t, dt);

        // Pulse: the comparator threshold on the same ramp
        let pw = pulse_width.clamp(0.05, 0.95);
        let mut pulse = if t < pw { 1.0 } else { -1.0 };
        pulse += poly_blep(t, dt);
        pulse -= poly_blep((t - pw).rem_euclid(1.0), dt);

        // Sub-oscillator: square wave, one octave down
        let sub = if self.sub_phase { 1.0 } else { -1.0 };

        (saw, pulse, sub)
    }
}

// ── IR3109 filter ──
//
// Four one-pole sections round a feedback loop, which is the chip's own
// topology. The integrators are topology-preserving (the `g/(1+g)` form), so
// a section's pole lands on the frequency it was asked for; the naive
// `s += g*(x-s)` form it replaced sat an octave below its own coefficient and
// took the whole cascade with it. The nonlinearity is kept where the
// hardware's is — the input summer and the feedback path — rather than inside
// every integrator.

#[derive(Debug, Clone)]
struct JunoFilter {
    s: [f64; 4],
}

impl JunoFilter {
    fn new() -> Self { Self { s: [0.0; 4] } }

    fn process(&mut self, input: f64, cutoff_norm: f64, resonance: f64, sr: f64) -> f64 {
        let freq = cutoff_hz(cutoff_norm).min(sr * 0.45);
        let g = (std::f64::consts::PI * freq / sr).tan();
        let gg = g / (1.0 + g);
        let res = resonance.clamp(0.0, 1.0) * 4.0;
        let compensation = 1.0 + resonance * 0.5;
        let fb = tanh_approx(self.s[3]);
        let mut x = tanh_approx(input * compensation - res * fb);

        for s in &mut self.s {
            let v = (x - *s) * gg;
            let y = v + *s;
            *s = y + v;
            if s.abs() < 1e-18 { *s = 0.0; }
            x = y;
        }
        // The loop is already contracting — `tanh_approx` tends to x/9 rather
        // than to 1, so four units of feedback becomes less than one unit of
        // state — but the one number that closes the loop is worth bounding
        // outright, since a self-oscillating filter has no input to bound it.
        self.s[3] = self.s[3].clamp(-4.0, 4.0);
        x
    }

    fn reset(&mut self) { self.s = [0.0; 4]; }

    /// Start a note with the filter already ringing, if the resonance is far
    /// enough up its travel for the loop to keep it going. See
    /// [`SELF_OSC_SEED`].
    fn start(&mut self, resonance: f64) {
        let past_knee = (resonance - SELF_OSC_KNEE) / (1.0 - SELF_OSC_KNEE);
        self.s = [past_knee.clamp(0.0, 1.0) * SELF_OSC_SEED; 4];
    }
}

// ── HPF ──
//
// Four positions, non-resonant, 6 dB/octave. The three corners are the
// measured ones for a Juno-60; a Juno-106 is not the same instrument here —
// its second position sits at 225 Hz and its first is a bass *boost*.

const HPF_CORNERS: [f64; 3] = [154.0, 339.0, 720.0];

#[derive(Debug, Clone)]
struct JunoHpf {
    state: f64,
}

impl JunoHpf {
    fn new() -> Self { Self { state: 0.0 } }

    fn process(&mut self, input: f64, position: u8, sr: f64) -> f64 {
        if position == 0 { return input; }
        let freq = HPF_CORNERS[(position as usize - 1).min(2)];
        let g = (std::f64::consts::PI * freq / sr).tan();
        let v = (input - self.state) * g / (1.0 + g);
        let lp = v + self.state;
        self.state = lp + v;
        if self.state.abs() < 1e-18 { self.state = 0.0; }
        input - lp
    }

    fn reset(&mut self) { self.state = 0.0; }
}

// ── ADSR envelope ──
//
// One envelope per voice, shared by the filter and the amplifier, as on the
// instrument. Every segment is a capacitor charging towards something, which
// is why none of them are straight lines:
//
// * attack charges towards 1.58 and the stage ends when it passes 1.0, so the
//   segment is the first time-constant of an exponential — measured on the
//   real thing as (1-e^-x)/0.632, which is the same curve;
// * decay and release charge towards slightly past their target and stop when
//   they reach it, which is 3.5 time constants across the segment. That is the
//   measured shape, and it is what makes the slider's number the time the
//   segment actually takes.

#[derive(Debug, Clone, Copy, PartialEq)]
enum EnvStage { Idle, Attack, Decay, Sustain, Release }

/// 1/(1-e^-1): the attack aims here so that it arrives at 1.0 after exactly
/// one time constant.
const ATTACK_AIM: f64 = 1.581_976_706_869_326;
/// Time constants spanned by a decay or release segment, measured.
const ENV_CONSTANTS: f64 = 3.5;
/// How far past its target a decay or release aims so that it arrives after
/// `ENV_CONSTANTS` of them: e^-3.5 / (1 - e^-3.5).
const ENV_UNDERSHOOT: f64 = 0.031_144_869_855_006_6;

#[derive(Debug, Clone)]
struct JunoEnvelope {
    stage: EnvStage,
    level: f64,
    aim: f64,
    attack: f64, decay: f64, sustain: f64, release: f64,
    /// Per-sample coefficients for the three timed segments, recomputed only
    /// when a slider moves. The exponential in `env_rate` is not something to
    /// evaluate six times a sample for an answer that changes when a finger
    /// does.
    rates: [f64; 3],
    sample_rate: f64,
}

/// One-pole coefficient for a segment of `seconds` spanning `constants` time
/// constants. Saturates at 1.0, so a zero-length segment is a jump.
fn env_rate(seconds: f64, constants: f64, sr: f64) -> f64 {
    if seconds <= 0.0 { return 1.0; }
    (1.0 - (-constants / (seconds * sr)).exp()).min(1.0)
}

impl JunoEnvelope {
    fn new(sr: f64) -> Self {
        let mut env = Self { stage: EnvStage::Idle, level: 0.0, aim: 0.0,
               attack: 0.01, decay: 0.3, sustain: 0.7, release: 0.2,
               rates: [0.0; 3], sample_rate: sr };
        env.retime();
        env
    }

    fn retime(&mut self) {
        let sr = self.sample_rate;
        self.rates = [
            env_rate(self.attack, 1.0, sr),
            env_rate(self.decay, ENV_CONSTANTS, sr),
            env_rate(self.release, ENV_CONSTANTS, sr),
        ];
    }

    /// Follow the three time sliders. A no-op unless one of them moved, which
    /// is what keeps the exponentials off the per-sample path.
    fn set_times(&mut self, attack: f64, decay: f64, release: f64) {
        if attack != self.attack || decay != self.decay || release != self.release {
            self.attack = attack;
            self.decay = decay;
            self.release = release;
            self.retime();
        }
    }

    fn trigger(&mut self) {
        self.stage = EnvStage::Attack;
        self.aim = ATTACK_AIM;
    }

    fn release_env(&mut self) {
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
            self.aim = -ENV_UNDERSHOOT * self.level;
        }
    }

    fn kill(&mut self) { self.stage = EnvStage::Idle; self.level = 0.0; }
    fn is_active(&self) -> bool { self.stage != EnvStage::Idle }
    fn is_held(&self) -> bool {
        matches!(self.stage, EnvStage::Attack | EnvStage::Decay | EnvStage::Sustain)
    }

    fn enter_decay(&mut self) {
        self.stage = EnvStage::Decay;
        self.aim = self.sustain - ENV_UNDERSHOOT * (self.level - self.sustain);
    }

    fn tick(&mut self) -> f64 {
        match self.stage {
            EnvStage::Idle => 0.0,
            EnvStage::Attack => {
                self.level += self.rates[0] * (self.aim - self.level);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.enter_decay();
                    if self.level <= self.sustain {
                        self.level = self.sustain;
                        self.stage = EnvStage::Sustain;
                    }
                }
                self.level
            }
            EnvStage::Decay => {
                self.level += self.rates[1] * (self.aim - self.level);
                if self.level <= self.sustain {
                    self.level = self.sustain;
                    self.stage = EnvStage::Sustain;
                }
                self.level
            }
            EnvStage::Sustain => self.sustain,
            EnvStage::Release => {
                self.level += self.rates[2] * (self.aim - self.level);
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = EnvStage::Idle;
                }
                self.level
            }
        }
    }
}

// ── VCA gate ──
//
// The VCA's other position bypasses the envelope and follows the key. It is
// not a hard switch on the instrument either: the gate measures 3 ms up and
// 6 ms down, which is exactly enough to keep the edges out of the speakers.

const GATE_ATTACK: f64 = 0.003;
const GATE_RELEASE: f64 = 0.006;

#[derive(Debug, Clone)]
struct JunoGate {
    level: f64,
    /// Fixed by the hardware, so both coefficients are settled at init.
    rates: [f64; 2],
}

impl JunoGate {
    fn new(sr: f64) -> Self {
        Self {
            level: 0.0,
            rates: [
                env_rate(GATE_ATTACK, ENV_CONSTANTS, sr),
                env_rate(GATE_RELEASE, ENV_CONSTANTS, sr),
            ],
        }
    }

    fn tick(&mut self, held: bool) -> f64 {
        let (aim, rate) = if held { (1.0, self.rates[0]) } else { (0.0, self.rates[1]) };
        self.level += rate * (aim - self.level);
        self.level
    }

    fn reset(&mut self) { self.level = 0.0; }
}

// ── LFO ──

#[derive(Debug, Clone)]
struct JunoLfo {
    phase: f64,
    rate: f64,
    hold: f64,
    fade: f64,
    elapsed: f64,
    level: f64,
}

impl JunoLfo {
    fn new() -> Self {
        Self { phase: 0.0, rate: 1.0, hold: 0.0, fade: 0.0, elapsed: 0.0, level: 1.0 }
    }

    /// Restart the delay. Called on the first key of a phrase, not on every
    /// note: on the instrument the delay runs from the gate going high, so a
    /// second note added to a held chord does not reset the vibrato.
    fn trigger_delay(&mut self) {
        self.elapsed = 0.0;
        self.level = if self.hold + self.fade <= 0.0 { 1.0 } else { 0.0 };
    }

    /// Returns triangle LFO output (-1..1).
    fn tick(&mut self, sr: f64) -> f64 {
        self.phase += self.rate / sr;
        if self.phase >= 1.0 { self.phase -= 1.0; }
        if self.level < 1.0 {
            self.elapsed += 1.0 / sr;
            self.level = ((self.elapsed - self.hold) / self.fade.max(1e-6)).clamp(0.0, 1.0);
        }
        // Triangle wave
        let tri = if self.phase < 0.5 { 4.0 * self.phase - 1.0 } else { 3.0 - 4.0 * self.phase };
        tri * self.level
    }
}

// ── BBD chorus ──
//
// Two bucket-brigade lines swept in antiphase, which is where the stereo comes
// from. Rates and delay ranges are measured off a Juno-60: 0.513 Hz and
// 0.863 Hz for the two switches, and 9.75 Hz when both are down, where the
// sweep narrows to 3.3-3.7 ms and turns into a vibrato.

#[derive(Debug, Clone)]
struct BbdChorus {
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    write_pos: usize,
    lfo_phase: f64,
}

/// (rate Hz, min delay s, max delay s, invert the right channel).
fn chorus_mode(mode: u8) -> (f64, f64, f64, bool) {
    match mode {
        1 => (0.513, 0.00166, 0.00535, true),
        2 => (0.863, 0.00166, 0.00535, true),
        _ => (9.75, 0.0033, 0.0037, false),
    }
}

impl BbdChorus {
    fn new(sr: f64) -> Self {
        let max_samples = (sr * 0.01) as usize + 2; // ~10ms max
        Self {
            delay_l: vec![0.0; max_samples],
            delay_r: vec![0.0; max_samples],
            write_pos: 0,
            lfo_phase: 0.0,
        }
    }

    /// Process mono input, return (left, right) stereo output.
    fn process(&mut self, input: f32, mode: u8, sr: f64) -> (f32, f32) {
        if mode == 0 { return (input, input); } // bypass

        let (rate, min_delay, max_delay, invert_r) = chorus_mode(mode);

        // LFO
        self.lfo_phase += rate / sr;
        if self.lfo_phase >= 1.0 { self.lfo_phase -= 1.0; }
        let lfo = if mode <= 2 {
            // Triangle
            if self.lfo_phase < 0.5 { 4.0 * self.lfo_phase - 1.0 } else { 3.0 - 4.0 * self.lfo_phase }
        } else {
            // Sine-like for I+II
            (self.lfo_phase * TWO_PI).sin()
        };

        let center = (min_delay + max_delay) / 2.0;
        let sweep = (max_delay - min_delay) / 2.0;

        let delay_l_time = center + lfo * sweep;
        let delay_r_time = if invert_r { center - lfo * sweep } else { center + lfo * sweep };

        // Write input (with subtle BBD saturation)
        let saturated = tanh_approx(input as f64 * 0.8) as f32;
        let len = self.delay_l.len();
        self.delay_l[self.write_pos] = saturated;
        self.delay_r[self.write_pos] = saturated;
        self.write_pos = (self.write_pos + 1) % len;

        // Read with linear interpolation
        let read_l = self.read_delay(&self.delay_l, delay_l_time, sr);
        let read_r = self.read_delay(&self.delay_r, delay_r_time, sr);

        // 50/50 wet/dry mix
        let out_l = input * 0.5 + read_l * 0.5;
        let out_r = input * 0.5 + read_r * 0.5;

        (out_l, out_r)
    }

    fn read_delay(&self, buf: &[f32], delay_secs: f64, sr: f64) -> f32 {
        let delay_samples = delay_secs * sr;
        let len = buf.len() as f64;
        let read_pos = self.write_pos as f64 - delay_samples - 1.0;
        let read_pos = ((read_pos % len) + len) % len;
        let idx0 = read_pos as usize % buf.len();
        let idx1 = (idx0 + 1) % buf.len();
        let frac = (read_pos - read_pos.floor()) as f32;
        buf[idx0] * (1.0 - frac) + buf[idx1] * frac
    }
}

// ── Noise generator ──
//
// One source shared by all six voices, as on the instrument, low-passed at
// 5 kHz — the noise on a Juno is a soft wash rather than a hiss, and the
// filter is in the voice board ahead of the mixer.

const NOISE_CORNER_HZ: f64 = 5000.0;

#[derive(Debug, Clone)]
struct NoiseGen {
    state: u32,
    lp: f64,
    coeff: f64,
    makeup: f64,
}

impl NoiseGen {
    fn new(sr: f64) -> Self {
        let coeff = 1.0 - (-TWO_PI * NOISE_CORNER_HZ / sr).exp();
        Self {
            state: 12345,
            lp: 0.0,
            coeff,
            // The filter costs the source most of its power; this puts the
            // level back where the slider expects it.
            makeup: ((2.0 - coeff) / coeff).sqrt(),
        }
    }

    fn tick(&mut self) -> f64 {
        self.state = self.state.wrapping_mul(1103515245).wrapping_add(12345);
        let white = f64::from(self.state as i32) / f64::from(i32::MAX);
        self.lp += self.coeff * (white - self.lp);
        self.lp * self.makeup
    }
}

// ── Voice ──

/// Every voice's contribution is halved on the way out of the VCA, which is
/// where the six-voice sum gets its room.
const VOICE_TRIM: f64 = 0.5;

/// How much of the filter's own oscillation a note starts with, and the
/// resonance above which it gets any.
///
/// The VCF on the instrument is an oscillator that does not stop: with the
/// resonance at the top of its travel it is ringing whether or not a key is
/// down, which is why a note on one of bank 7's patches — "the patches whose
/// sound sources are VCF self-oscillation", in Roland's words — is loud from
/// its first sample. A filter whose state is zeroed at the note-on has
/// nothing to ring, because the loop multiplies zero by four and gets zero,
/// so those patches came out silent.
///
/// The knee is where the four-pole loop measurably starts to sustain rather
/// than ring down: at 0.95 a kick is gone in 200 ms and at 0.9 in 50, so a
/// patch below the knee gets nothing and hears no click for it.
const SELF_OSC_KNEE: f64 = 0.9;
const SELF_OSC_SEED: f64 = 0.05;

#[derive(Debug, Clone)]
struct JunoVoice {
    dco: JunoDco,
    hpf: JunoHpf,
    filter: JunoFilter,
    env: JunoEnvelope,
    gate: JunoGate,
    note: u8,
    age: u64,
    sample_rate: f64,
}

impl JunoVoice {
    fn new(sr: f64) -> Self {
        Self {
            dco: JunoDco::new(),
            hpf: JunoHpf::new(),
            filter: JunoFilter::new(),
            env: JunoEnvelope::new(sr),
            gate: JunoGate::new(sr),
            note: 255,
            age: 0,
            sample_rate: sr,
        }
    }

    fn note_on(&mut self, note: u8, patch: &JunoPatch, age: u64) {
        self.note = note;
        self.age = age;
        self.env.set_times(patch.attack, patch.decay, patch.release);
        self.env.sustain = patch.sustain;
        self.env.trigger();
        self.filter.start(patch.resonance);
        self.hpf.reset();
    }

    fn note_off(&mut self) { self.env.release_env(); }
    fn kill(&mut self) {
        self.note = 255;
        self.env.kill();
        self.filter.reset();
        self.hpf.reset();
        self.gate.reset();
    }
    fn is_sounding(&self) -> bool { self.env.is_active() }
    fn is_held(&self) -> bool { self.env.is_held() }

    /// Process one sample. Returns mono output (pre-chorus).
    fn tick(&mut self, patch: &JunoPatch, lfo_out: f64, noise: f64) -> f64 {
        if !self.is_sounding() { return 0.0; }

        let sr = self.sample_rate;
        let freq = note_to_freq(self.note, patch.range);

        // The three time sliders are live, so a note already sounding follows
        // them; the coefficients behind them are only recomputed when one moves.
        self.env.set_times(patch.attack, patch.decay, patch.release);

        // LFO pitch modulation. Skipped rather than raised to the power of
        // zero when the DCO's LFO slider is down, which is most patches.
        let modulated_freq = if patch.lfo_to_pitch > 0.0 {
            let pitch_mod = lfo_out * patch.lfo_to_pitch * LFO_PITCH_CENTS;
            freq * 2.0f64.powf(pitch_mod / 1200.0)
        } else {
            freq
        };
        self.dco.set_freq(modulated_freq, sr);

        // Envelope, before the DCO because the PWM switch's third position
        // hands it to the comparator.
        let env_val = self.env.tick();

        // Pulse width: the comparator threshold comes from the LFO, from the
        // slider alone, or from the envelope, as the PWM switch says. The two
        // moving sources start from square; the slider is the direction the
        // panel's own scale runs, up for narrower.
        let effective_pw = match patch.pwm_mode {
            PwmSource::Lfo => 0.5 + lfo_out * patch.pwm_depth,
            PwmSource::Manual => 0.5 - patch.pwm_depth,
            PwmSource::Env => 0.5 - env_val * patch.pwm_depth,
        };

        let (saw, pulse, sub) = self.dco.tick(effective_pw);

        // The DCO mixer: two switches and two faders.
        let mut mixed = 0.0;
        let mut weight = 0.0;
        if patch.saw { mixed += saw; weight += 1.0; }
        if patch.pulse { mixed += pulse; weight += 1.0; }
        mixed += sub * patch.sub_level;
        weight += patch.sub_level;
        mixed += noise * patch.noise_level;
        weight += patch.noise_level;
        // Scale so that adding a source raises the level rather than the peak.
        if weight > 1.0 { mixed /= weight.sqrt(); }

        // HPF, ahead of the VCF as on the voice board
        let source = self.hpf.process(mixed, patch.hpf, sr);

        // Filter cutoff: panel + envelope + keyboard + LFO. Key follow is in
        // octaves of cutoff per octave of keyboard, so it has to be scaled by
        // how many octaves the cutoff slider spans.
        let key_follow =
            (f64::from(self.note) - 60.0) / 12.0 / CUTOFF_OCTAVES * patch.key_follow;
        let effective_cutoff = (patch.cutoff
            + env_val * patch.env_mod
            + key_follow
            + lfo_out * patch.lfo_to_filter
        ).clamp(0.0, 1.0);

        let filtered = self.filter.process(source, effective_cutoff, patch.resonance, sr);

        // VCA
        let gate = self.gate.tick(self.env.is_held());
        let vca = if patch.vca_gate {
            // A gated voice is finished when the gate has closed, whatever the
            // envelope is still doing for the filter.
            if !self.env.is_held() && gate < 1e-4 { self.env.kill(); }
            gate
        } else {
            env_val
        };
        filtered * vca * VOICE_TRIM
    }
}

// ── Juno-60 Synth ──

pub struct Juno60Synth {
    voices: Vec<JunoVoice>,
    lfo: JunoLfo,
    chorus: Option<BbdChorus>,
    noise: NoiseGen,
    sample_rate: f64,
    pub params: [f32; PARAM_COUNT],
    voice_counter: u64,
    last_patch_index: usize,
}

impl Juno60Synth {
    pub fn new() -> Self {
        let mut s = Self {
            voices: Vec::new(),
            lfo: JunoLfo::new(),
            chorus: None,
            noise: NoiseGen::new(44100.0),
            sample_rate: 44100.0,
            params: PARAM_DEFAULTS,
            voice_counter: 0,
            last_patch_index: usize::MAX,
        };
        s.sync_params_from_patch();
        s
    }

    fn current_patch_index(&self) -> usize {
        selector(self.params[P_PATCH], PATCH_COUNT)
    }

    /// The whole panel as the factory chart for this patch sets it.
    ///
    /// A straight copy for the sliders — the chart's readings are panel
    /// positions already — and a midpoint for each switch, so that a switch
    /// loaded from a preset sits where [`step_discrete`] would leave it.
    pub fn params_for_patch(patch_value: f32) -> [f32; PARAM_COUNT] {
        let c = &BANK[selector(patch_value, PATCH_COUNT)];
        let mut params = [0.0f32; PARAM_COUNT];
        params[P_PATCH] = patch_value;
        params[P_LFO_RATE] = c.lfo[0];
        params[P_LFO_DELAY] = c.lfo[1];
        params[P_DCO_LFO] = c.dco[0];
        params[P_PWM] = c.dco[1];
        params[P_PWM_MODE] = knob_for(c.pwm_mode as usize, 3);
        params[P_PULSE] = knob_for(usize::from(c.waves[0]), 2);
        params[P_SAW] = knob_for(usize::from(c.waves[1]), 2);
        // The sub is a button and a fader on the instrument and a fader alone
        // here, so a sub switched out is a fader at zero. Three patches leave
        // the fader up with the button out; taking the chart at face value
        // would give them an octave of bottom end they do not have.
        params[P_SUB] = if c.waves[2] { c.dco[2] } else { 0.0 };
        params[P_NOISE] = c.dco[3];
        params[P_RANGE] = knob_for(c.range as usize, 3);
        params[P_HPF] = knob_for(c.hpf as usize, 4);
        params[P_CUTOFF] = c.vcf[0];
        params[P_RESO] = c.vcf[1];
        params[P_ENV_POLARITY] = knob_for(usize::from(c.invert), 2);
        params[P_ENV_MOD] = c.vcf[2];
        params[P_VCF_LFO] = c.vcf[3];
        params[P_KEY_FOLLOW] = c.vcf[4];
        params[P_VCA_MODE] = knob_for(usize::from(c.gate), 2);
        params[P_LEVEL] = c.level;
        params[P_ATTACK] = c.env[0];
        params[P_DECAY] = c.env[1];
        params[P_SUSTAIN] = c.env[2];
        params[P_RELEASE] = c.env[3];
        params[P_CHORUS] = knob_for(c.chorus as usize, 4);
        params
    }

    fn sync_params_from_patch(&mut self) {
        let idx = self.current_patch_index();
        if idx == self.last_patch_index { return; }
        self.last_patch_index = idx;
        let new_params = Self::params_for_patch(self.params[P_PATCH]);
        for (i, &v) in new_params.iter().enumerate() {
            if i != P_PATCH { self.params[i] = v; }
        }
    }

    fn next_age(&mut self) -> u64 { self.voice_counter += 1; self.voice_counter }

    fn allocate_voice(&mut self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| !v.is_sounding()) { return i; }
        if let Some((i, _)) = self.voices.iter().enumerate()
            .filter(|(_, v)| !v.is_held()).min_by_key(|(_, v)| v.age) { return i; }
        self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap_or(0)
    }

    fn release_note(&mut self, note: u8) {
        for v in &mut self.voices { if v.note == note && v.is_held() { v.note_off(); } }
    }

    fn any_key_held(&self) -> bool {
        self.voices.iter().any(JunoVoice::is_held)
    }

    fn kill_all(&mut self) {
        for v in &mut self.voices { v.kill(); }
        if let Some(ref mut chorus) = self.chorus {
            for s in &mut chorus.delay_l { *s = 0.0; }
            for s in &mut chorus.delay_r { *s = 0.0; }
        }
    }

    /// The panel as it stands, in the units the engine works in. Every control
    /// is live — the preset is only where the knobs started.
    fn active_patch(&self) -> JunoPatch {
        let p = &self.params;
        let pwm = f64::from(p[P_PWM]);
        let env_sign = if selector(p[P_ENV_POLARITY], 2) == 1 { -1.0 } else { 1.0 };
        JunoPatch {
            lfo_rate: lfo_hz(f64::from(p[P_LFO_RATE])),
            lfo_delay: f64::from(p[P_LFO_DELAY]),
            lfo_to_pitch: f64::from(p[P_DCO_LFO]),
            pwm_depth: pwm * PW_SWING,
            pwm_mode: PwmSource::from_index(selector(p[P_PWM_MODE], 3)),
            pulse: selector(p[P_PULSE], 2) == 1,
            saw: selector(p[P_SAW], 2) == 1,
            sub_level: f64::from(p[P_SUB]),
            noise_level: f64::from(p[P_NOISE]),
            range: selector(p[P_RANGE], 3) as u8,
            hpf: selector(p[P_HPF], 4) as u8,
            cutoff: f64::from(p[P_CUTOFF]),
            resonance: f64::from(p[P_RESO]),
            env_mod: env_sign * f64::from(p[P_ENV_MOD]),
            lfo_to_filter: f64::from(p[P_VCF_LFO]),
            key_follow: f64::from(p[P_KEY_FOLLOW]),
            vca_gate: selector(p[P_VCA_MODE], 2) == 1,
            level: f64::from(p[P_LEVEL]),
            attack: attack_seconds(f64::from(p[P_ATTACK])),
            decay: decay_seconds(f64::from(p[P_DECAY])),
            sustain: f64::from(p[P_SUSTAIN]),
            release: decay_seconds(f64::from(p[P_RELEASE])),
            chorus: selector(p[P_CHORUS], 4) as u8,
        }
    }
}

impl Default for Juno60Synth {
    fn default() -> Self { Self::new() }
}

impl Plugin for Juno60Synth {
    fn info(&self) -> PluginInfo {
        PluginInfo { name: "Juno-60".into(), version: "0.1.0".into(),
                     author: "Phosphor".into(), category: PluginCategory::Instrument }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..MAX_VOICES).map(|_| JunoVoice::new(sample_rate)).collect();
        self.chorus = Some(BbdChorus::new(sample_rate));
        self.noise = NoiseGen::new(sample_rate);
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() { return; }

        let buf_len = outputs[0].len();
        let patch = self.active_patch();
        let gain = (patch.level as f32) * OUTPUT_TRIM;
        let sr = self.sample_rate;

        self.lfo.rate = patch.lfo_rate;
        let (hold, fade) = lfo_delay_times(patch.lfo_delay);
        self.lfo.hold = hold;
        self.lfo.fade = fade;

        // MIDI event sorting (allocation-free)
        let mut event_indices: [usize; 256] = [0; 256];
        let event_count = midi_events.len().min(256);
        for i in 0..event_count { event_indices[i] = i; }
        for i in 1..event_count {
            let mut j = i;
            while j > 0 && midi_events[event_indices[j]].sample_offset < midi_events[event_indices[j-1]].sample_offset {
                event_indices.swap(j, j - 1);
                j -= 1;
            }
        }
        let mut ei = 0;

        let stereo = outputs.len() >= 2;

        for i in 0..buf_len {
            while ei < event_count && midi_events[event_indices[ei]].sample_offset as usize <= i {
                let ev = &midi_events[event_indices[ei]];
                match ev.status & 0xF0 {
                    0x90 => {
                        if ev.data2 > 0 {
                            let first_key = !self.any_key_held();
                            self.release_note(ev.data1);
                            let age = self.next_age();
                            let idx = self.allocate_voice();
                            self.voices[idx].note_on(ev.data1, &patch, age);
                            if first_key { self.lfo.trigger_delay(); }
                        } else {
                            self.release_note(ev.data1);
                        }
                    }
                    0x80 => self.release_note(ev.data1),
                    0xB0 => match ev.data1 {
                        120 => self.kill_all(),
                        123 => { for v in &mut self.voices { if v.is_held() { v.note_off(); } } }
                        _ => {}
                    }
                    _ => {}
                }
                ei += 1;
            }

            // Global LFO and the shared noise source
            let lfo_out = self.lfo.tick(sr);
            let noise_val = self.noise.tick();

            // Sum all voices (mono)
            let mut mono = 0.0f32;
            for v in &mut self.voices {
                mono += v.tick(&patch, lfo_out, noise_val) as f32;
            }

            // BBD Chorus
            let (left, right) = if let Some(ref mut chorus) = self.chorus {
                chorus.process(mono, patch.chorus, sr)
            } else {
                (mono, mono)
            };

            // Bound both channels without hard clipping them. The trim above
            // keeps ordinary playing under the knee, so this is the identity
            // for everything except a patch pushed past it by the level knob.
            let left = soft_saturate(left * gain);
            let right = soft_saturate(right * gain);

            if stereo {
                outputs[0][i] = left;
                outputs[1][i] = right;
            } else {
                outputs[0][i] = (left + right) * 0.5;
            }
        }
    }

    fn parameter_count(&self) -> usize { PARAM_COUNT }

    fn parameter_info(&self, index: usize) -> Option<ParameterInfo> {
        if index >= PARAM_COUNT { return None; }
        Some(ParameterInfo {
            name: PARAM_NAMES[index].into(),
            min: 0.0, max: 1.0,
            default: PARAM_DEFAULTS[index],
            unit: match index {
                P_ATTACK | P_DECAY | P_RELEASE => "s".into(),
                P_LFO_RATE => "Hz".into(),
                _ => "".into(),
            },
        })
    }

    fn get_parameter(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    fn set_parameter(&mut self, index: usize, value: f32) {
        if let Some(p) = self.params.get_mut(index) {
            *p = phosphor_plugin::clamp_parameter(value);
        }
        if index == P_PATCH { self.sync_params_from_patch(); }
    }

    fn reset(&mut self) {
        self.kill_all();
        self.voice_counter = 0;
        if let Some(ref mut chorus) = self.chorus {
            for s in &mut chorus.delay_l { *s = 0.0; }
            for s in &mut chorus.delay_r { *s = 0.0; }
        }
    }
}

/// Note to hertz, through the DCO range switch: 16', 8' or 4'.
fn note_to_freq(note: u8, range: u8) -> f64 {
    let octave = match range { 0 => 0.5, 2 => 2.0, _ => 1.0 };
    440.0 * 2.0f64.powf((f64::from(note) - 69.0) / 12.0) * octave
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_on(note: u8, vel: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x90, data1: note, data2: vel }
    }
    fn note_off(note: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x80, data1: note, data2: 0 }
    }
    fn cc(cc_num: u8, val: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0xB0, data1: cc_num, data2: val }
    }

    fn process_buffers(synth: &mut Juno60Synth, events: &[MidiEvent], count: usize) -> Vec<f32> {
        let mut all = Vec::new();
        let mut out = vec![0.0f32; 64];
        synth.process(&[], &mut [&mut out], events);
        all.extend_from_slice(&out);
        for _ in 1..count {
            out.fill(0.0);
            synth.process(&[], &mut [&mut out], &[]);
            all.extend_from_slice(&out);
        }
        all
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().map(|v| v.abs()).fold(0.0f32, f32::max)
    }

    #[test]
    fn silence_with_no_input() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        // 200 buffers, not 4: the output carries a headroom trim, so five
        // milliseconds of attack is not enough signal to tell "sounding" from
        // "silent" with any margin.
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 200);
        assert!(peak(&out) > 0.005, "peak={}", peak(&out));
    }

    #[test]
    fn silent_after_release() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        s.set_parameter(P_RELEASE, 0.05);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[note_off(60, 0)], 3000);
        let out = process_buffers(&mut s, &[], 1);
        assert!(peak(&out) < 0.001, "peak={}", peak(&out));
    }

    #[test]
    fn output_is_finite() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 1000);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn polyphony() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        let events = [note_on(60, 100, 0), note_on(64, 100, 0), note_on(67, 100, 0)];
        let out = process_buffers(&mut s, &events, 200);
        assert!(peak(&out) > 0.005 && peak(&out) < 1.0, "peak={}", peak(&out));
    }

    #[test]
    fn all_patches_produce_sound() {
        for pi in 0..PATCH_COUNT {
            let mut s = Juno60Synth::new();
            s.init(44100.0, 64);
            s.set_parameter(P_PATCH, patch_knob(pi));
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 2000);
            assert!(
                peak(&out) > 0.001,
                "patch {} {} {} is silent: peak={}",
                pi, PATCH_NUMBERS[pi], PATCH_NAMES[pi], peak(&out)
            );
        }
    }

    #[test]
    fn all_patches_finite() {
        for pi in 0..PATCH_COUNT {
            let mut s = Juno60Synth::new();
            s.init(44100.0, 64);
            s.set_parameter(P_PATCH, patch_knob(pi));
            let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 500);
            assert!(out.iter().all(|v| v.is_finite()), "Patch {} finite", pi);
        }
    }

    #[test]
    fn the_patch_knob_lands_on_the_patch_it_names() {
        // 56 patches is enough that dividing the index by the count misses:
        // the quotient can land a hair below its own step and select the patch
        // before it. Every index has to answer to its own name, both through
        // the label and through the block that loads.
        for (pi, label) in PATCH_LABELS.iter().enumerate() {
            let knob = patch_knob(pi);
            assert_eq!(selector(knob, PATCH_COUNT), pi, "patch {pi} knob {knob}");
            assert_eq!(discrete_label(P_PATCH, knob), Some(*label));
            let mut s = Juno60Synth::new();
            s.set_parameter(P_PATCH, knob);
            assert_eq!(s.current_patch_index(), pi);
        }
    }

    #[test]
    fn the_bank_is_the_factory_bank() {
        // Seven banks of eight, numbered as the instrument numbers them, in
        // factory order. A row inserted or dropped shows up here rather than
        // as a patch that answers to the wrong name.
        assert_eq!(PATCH_COUNT, 56);
        for (i, number) in PATCH_NUMBERS.iter().enumerate() {
            let want = format!("{}{}", i / 8 + 1, i % 8 + 1);
            assert_eq!(*number, want, "patch {i} is numbered {number}");
        }
        assert_eq!(PATCH_NAMES[0], "STRINGS 1");
        assert_eq!(PATCH_NAMES[PATCH_COUNT - 1], "SYNTHESIZER DRUM");
        // The number is what a player calls the patch, so it leads the label.
        for (i, label) in PATCH_LABELS.iter().enumerate() {
            assert!(
                label.starts_with(PATCH_NUMBERS[i]),
                "label {label:?} does not start with its patch number"
            );
            assert!(label.chars().count() <= 12, "label {label:?} is too wide for the panel");
        }
    }

    #[test]
    fn the_self_oscillating_bank_speaks() {
        // "Bank 7 (71 to 78) includes the patches whose sound sources are VCF
        // self-oscillation" — the manual. Two of the eight have no oscillator,
        // no sub and no noise at all, so the only thing that can make a sound
        // is the filter ringing, and the only thing that can start it is the
        // ringing it was already doing when the key went down. All eight have
        // to speak, and inside their own attack rather than eventually: they
        // are percussive, and the quickest of them has decayed away by 300 ms.
        //
        // The floor is 12 dB under the quietest of the eight as measured
        // (0.0018, 77 SURF), not a round number: these patches are quiet, and
        // the point of the assertion is to catch silence, not to pin a level.
        for pi in 48..PATCH_COUNT {
            let mut s = Juno60Synth::new();
            s.init(44100.0, 64);
            s.set_parameter(P_PATCH, patch_knob(pi));
            // 116 ms, which is inside the shortest decay in the bank.
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 80);
            assert!(
                peak(&out) > 4e-4,
                "patch {} {} peaks at {} in its first 116 ms",
                PATCH_NUMBERS[pi], PATCH_NAMES[pi], peak(&out)
            );
        }
    }

    #[test]
    fn chorus_changes_sound() {
        let mut s1 = Juno60Synth::new();
        s1.init(44100.0, 64);
        s1.set_parameter(P_CHORUS, knob_for(0, 4)); // off
        let dry = process_buffers(&mut s1, &[note_on(60, 100, 0)], 8);

        let mut s2 = Juno60Synth::new();
        s2.init(44100.0, 64);
        s2.set_parameter(P_CHORUS, knob_for(3, 4)); // I+II
        let wet = process_buffers(&mut s2, &[note_on(60, 100, 0)], 8);

        let diff: f32 = dry.iter().zip(wet.iter()).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 0.01, "Chorus should change sound, diff={diff}");
    }

    #[test]
    fn cc120_kills() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[cc(120, 0, 0)], 1);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn all_params_readable() {
        let s = Juno60Synth::new();
        assert_eq!(s.parameter_count(), PARAM_COUNT);
        for i in 0..PARAM_COUNT {
            assert!(s.parameter_info(i).is_some());
            let val = s.get_parameter(i);
            assert!((0.0..=1.0).contains(&val), "param {i} = {val}");
        }
    }

    #[test]
    fn sample_accurate_midi() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 128);
        s.set_parameter(P_ATTACK, 0.0);
        let mut out = vec![0.0f32; 128];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 64)]);
        let pre = peak(&out[..64]);
        let post = peak(&out[64..]);
        assert!(pre < 0.001, "pre={pre}");
        assert!(post > 0.001, "post={post}");
    }

    // ── Panel ──

    #[test]
    fn the_panel_is_in_front_panel_order() {
        // The order is the instrument's, left to right, and the editor shows
        // the parameter block in index order — so this is the layout a player
        // sees. Sessions store the block positionally, which is what makes the
        // order worth pinning down in a test.
        assert_eq!(PARAM_NAMES[P_PATCH], "patch");
        assert_eq!(&PARAM_NAMES[P_LFO_RATE..=P_LFO_DELAY], &["lfo rate", "lfo dly"]);
        assert_eq!(
            &PARAM_NAMES[P_DCO_LFO..=P_RANGE],
            &["dco lfo", "pwm", "pwm mode", "pulse", "saw", "sub", "noise", "range"]
        );
        assert_eq!(PARAM_NAMES[P_HPF], "hpf");
        assert_eq!(
            &PARAM_NAMES[P_CUTOFF..=P_KEY_FOLLOW],
            &["freq", "res", "env pol", "env mod", "vcf lfo", "kybd"]
        );
        assert_eq!(&PARAM_NAMES[P_VCA_MODE..=P_LEVEL], &["vca", "level"]);
        assert_eq!(
            &PARAM_NAMES[P_ATTACK..=P_RELEASE],
            &["attack", "decay", "sustain", "release"]
        );
        assert_eq!(PARAM_NAMES[P_CHORUS], "chorus");
        assert_eq!(PARAM_COUNT, 25);
    }

    #[test]
    fn every_engine_control_is_reachable() {
        // The defect this guards: nine of the panel's controls existed in the
        // engine and had no parameter, so PWM, noise, the HPF, keyboard
        // follow, filter LFO, the LFO delay and the VCA switch could not be
        // touched. A control the engine reads has to have an index.
        // Held, then released, so the release slider has something to do.
        fn render(s: &mut Juno60Synth) -> Vec<f32> {
            let mut out = process_buffers(s, &[note_on(72, 100, 0)], 40);
            out.extend(process_buffers(s, &[note_off(72, 0)], 40));
            out
        }
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            if index == P_PATCH { continue; }
            let mut low = Juno60Synth::new();
            low.init(44100.0, 64);
            let mut high = Juno60Synth::new();
            high.init(44100.0, 64);
            // Every source running, so no control is masked by a dead path.
            for synth in [&mut low, &mut high] {
                synth.set_parameter(P_SAW, knob_for(1, 2));
                synth.set_parameter(P_PULSE, knob_for(1, 2));
                synth.set_parameter(P_SUB, 0.5);
                synth.set_parameter(P_NOISE, 0.2);
                synth.set_parameter(P_DCO_LFO, 0.3);
                synth.set_parameter(P_VCF_LFO, 0.3);
                synth.set_parameter(P_KEY_FOLLOW, 0.5);
                synth.set_parameter(P_ENV_MOD, 0.4);
                synth.set_parameter(P_PWM, 0.5);
                synth.set_parameter(P_LFO_RATE, 0.3);
                // A short envelope, so that 58 ms of held note reaches the
                // decay segment and the sustain level.
                synth.set_parameter(P_ATTACK, 0.0);
                synth.set_parameter(P_DECAY, 0.3);
                synth.set_parameter(P_SUSTAIN, 0.4);
                synth.set_parameter(P_RELEASE, 0.3);
            }
            low.set_parameter(index, 0.0);
            high.set_parameter(index, 1.0);
            let a = render(&mut low);
            let b = render(&mut high);
            let diff: f32 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum();
            assert!(diff > 1e-4, "parameter {index} ({name}) changes nothing: diff={diff}");
        }
    }

    #[test]
    fn switches_step_one_position_per_press() {
        // A float-fraction stepper walks a switch a fraction of a position at
        // a time and stalls on the boundary; the DX7's bank knob did exactly
        // that. Every switch here steps by index.
        for index in 0..PARAM_COUNT {
            let Some(count) = discrete_steps(index) else {
                assert_eq!(step_discrete(index, 0.42, true), 0.42, "slider {index} moved");
                continue;
            };
            let mut knob = knob_for(0, count);
            for step in 1..count {
                knob = step_discrete(index, knob, true);
                assert_eq!(selector(knob, count), step, "param {index} up to {step}");
            }
            knob = step_discrete(index, knob, true);
            assert_eq!(selector(knob, count), count - 1, "param {index} ran off the top");
            for step in (0..count - 1).rev() {
                knob = step_discrete(index, knob, false);
                assert_eq!(selector(knob, count), step, "param {index} down to {step}");
            }
            knob = step_discrete(index, knob, false);
            assert_eq!(selector(knob, count), 0, "param {index} ran off the bottom");
        }
    }

    #[test]
    fn switch_labels_read_as_the_panel_does() {
        assert_eq!(discrete_label(P_PATCH, 0.0), Some("11 STRINGS 1"));
        assert_eq!(discrete_label(P_RANGE, knob_for(0, 3)), Some("16'"));
        assert_eq!(discrete_label(P_RANGE, knob_for(1, 3)), Some("8'"));
        assert_eq!(discrete_label(P_RANGE, knob_for(2, 3)), Some("4'"));
        // The PWM switch reads down the panel, LFO at the top.
        assert_eq!(discrete_label(P_PWM_MODE, knob_for(0, 3)), Some("LFO"));
        assert_eq!(discrete_label(P_PWM_MODE, knob_for(1, 3)), Some("MAN"));
        assert_eq!(discrete_label(P_PWM_MODE, knob_for(2, 3)), Some("ENV"));
        assert_eq!(discrete_label(P_VCA_MODE, knob_for(0, 2)), Some("ENV"));
        assert_eq!(discrete_label(P_VCA_MODE, knob_for(1, 2)), Some("GATE"));
        assert_eq!(discrete_label(P_ENV_POLARITY, knob_for(1, 2)), Some("inv"));
        assert_eq!(discrete_label(P_CHORUS, knob_for(3, 4)), Some("I+II"));
        assert_eq!(discrete_label(P_HPF, knob_for(2, 4)), Some("2"));
        assert_eq!(discrete_label(P_CUTOFF, 0.5), None);
        // Out-of-range knobs are labelled, not panicked on: `params` is public.
        assert_eq!(discrete_label(P_RANGE, 9.0), Some("4'"));
        assert_eq!(discrete_label(P_RANGE, -1.0), Some("16'"));
    }

    #[test]
    fn patch_zero_is_the_default_parameter_block() {
        let loaded = Juno60Synth::params_for_patch(0.0);
        for i in 0..PARAM_COUNT {
            assert!(
                (loaded[i] - PARAM_DEFAULTS[i]).abs() < 5e-4,
                "default {i} ({}) is {} but patch 0 loads {}",
                PARAM_NAMES[i], PARAM_DEFAULTS[i], loaded[i]
            );
        }
    }

    #[test]
    fn preset_round_trip() {
        // Chart to panel to engine. Every column of every row has to arrive in
        // the control it belongs to and come back out in the engine's units —
        // the defect this catches is a preset loaded one slot out, which is
        // silent about itself and audible about nothing else.
        for (pi, c) in BANK.iter().enumerate() {
            let mut s = Juno60Synth::new();
            s.init(44100.0, 64);
            s.set_parameter(P_PATCH, patch_knob(pi));
            let name = c.name;
            let got = s.active_patch();
            let close = |got: f64, want: f32, what: &str| {
                assert!(
                    (got - f64::from(want)).abs() < 1e-6,
                    "{name} {what}: {got} where the chart says {want}"
                );
            };
            assert!((got.lfo_rate - lfo_hz(f64::from(c.lfo[0]))).abs() < 1e-9, "{name} lfo rate");
            close(got.lfo_delay, c.lfo[1], "lfo delay");
            close(got.lfo_to_pitch, c.dco[0], "dco lfo");
            close(got.pwm_depth, c.dco[1] * PW_SWING as f32, "pwm depth");
            assert_eq!(got.pwm_mode, c.pwm_mode, "{name} pwm mode");
            assert_eq!(got.pulse, c.waves[0], "{name} pulse");
            assert_eq!(got.saw, c.waves[1], "{name} saw");
            close(got.sub_level, if c.waves[2] { c.dco[2] } else { 0.0 }, "sub");
            close(got.noise_level, c.dco[3], "noise");
            assert_eq!(got.range, c.range, "{name} range");
            assert_eq!(got.hpf, c.hpf, "{name} hpf");
            close(got.cutoff, c.vcf[0], "cutoff");
            close(got.resonance, c.vcf[1], "resonance");
            close(got.env_mod.abs(), c.vcf[2], "env mod");
            assert_eq!(got.env_mod < 0.0, c.invert && c.vcf[2] > 0.0, "{name} env polarity");
            close(got.lfo_to_filter, c.vcf[3], "vcf lfo");
            close(got.key_follow, c.vcf[4], "kybd");
            assert_eq!(got.vca_gate, c.gate, "{name} vca");
            close(got.level, c.level, "level");
            for (i, (seconds, want)) in [
                (got.attack, attack_seconds(f64::from(c.env[0]))),
                (got.decay, decay_seconds(f64::from(c.env[1]))),
                (got.release, decay_seconds(f64::from(c.env[3]))),
            ].iter().enumerate() {
                assert!((seconds - want).abs() < 1e-9, "{name} env time {i}");
            }
            close(got.sustain, c.env[2], "sustain");
            assert_eq!(got.chorus, c.chorus, "{name} chorus");
        }
    }

    // ── DCO ──

    #[test]
    fn the_saw_falls_and_the_pulse_is_locked_to_it() {
        // A Juno-6/60 resets its ramp with a PNP on the falling edge of the
        // clock, so the sawtooth falls; the pulse is a comparator on that same
        // ramp, so its edges are the ramp's; the sub is a flip-flop on the
        // clock, so it is exactly half the frequency.
        let mut dco = JunoDco::new();
        dco.set_freq(441.0, 44100.0); // 100 samples per period
        let mut saw = Vec::new();
        let mut pulse = Vec::new();
        let mut sub = Vec::new();
        for _ in 0..400 {
            let (s, p, u) = dco.tick(0.5);
            saw.push(s);
            pulse.push(p);
            sub.push(u);
        }
        assert!(saw[20] > saw[80], "the sawtooth rises: {} then {}", saw[20], saw[80]);
        // The ramp only ever rises across a reset, and the reset is band
        // limited across two samples, so each run of rises is one reset.
        let rises: Vec<usize> = (1..400).filter(|&i| saw[i] > saw[i - 1]).collect();
        let resets: Vec<usize> =
            rises.iter().copied().filter(|i| !rises.contains(&(i - 1))).collect();
        assert_eq!(resets, [99, 199, 299, 399], "the ramp does not reset once a period");
        let pulse_rises: Vec<usize> =
            (1..400).filter(|&i| pulse[i] > 0.0 && pulse[i - 1] <= 0.0).collect();
        assert_eq!(resets, pulse_rises, "the pulse is not locked to the ramp");
        // The flip-flop toggles on the reset, so the sub's edges are the
        // ramp's and its period is two of them.
        let sub_edges: Vec<usize> = (1..400).filter(|&i| sub[i] != sub[i - 1]).collect();
        assert_eq!(sub_edges, resets, "the sub is not clocked by the ramp");
        assert_eq!(sub[99], -sub[199], "the sub does not divide by two");
        assert_eq!(sub[99], sub[299], "the sub does not divide by two");
    }

    #[test]
    fn the_pulse_width_is_the_duty_cycle() {
        for pw in [0.1, 0.25, 0.5, 0.75, 0.9] {
            let mut d = JunoDco::new();
            d.set_freq(441.0, 44100.0);
            let high = (0..1000).filter(|_| d.tick(pw).1 > 0.0).count();
            assert!((high as f64 / 1000.0 - pw).abs() < 0.01, "pw {pw} measured {high}/1000");
        }
    }

    #[test]
    fn the_range_switch_moves_an_octave_each_way() {
        assert!((note_to_freq(69, 1) - 440.0).abs() < 1e-9);
        assert!((note_to_freq(69, 0) - 220.0).abs() < 1e-9);
        assert!((note_to_freq(69, 2) - 880.0).abs() < 1e-9);
    }

    // ── Filter ──

    /// Magnitude of the response of `f` at `hz`, by impulse response.
    fn magnitude_at(response: &[f64], hz: f64, sr: f64) -> f64 {
        let w = TWO_PI * hz / sr;
        let (mut re, mut im) = (0.0, 0.0);
        for (n, v) in response.iter().enumerate() {
            let p = w * n as f64;
            re += v * p.cos();
            im -= v * p.sin();
        }
        (re * re + im * im).sqrt()
    }

    fn filter_response(cutoff_norm: f64, res: f64) -> Vec<f64> {
        let mut f = JunoFilter::new();
        (0..16384)
            .map(|i| f.process(if i == 0 { 1e-3 } else { 0.0 }, cutoff_norm, res, 44100.0) / 1e-3)
            .collect()
    }

    #[test]
    fn the_filter_is_four_pole_at_the_frequency_it_says() {
        // The shape was always four poles; the frequency was not. A naive
        // integrator puts a section half an octave-count below its
        // coefficient, so the slider marked 316 Hz measured 74 Hz at -3 dB
        // where four poles put it at 137, and the response at the marked
        // frequency was 27 dB down instead of 12.
        //
        // The reference is the analog cascade itself, 1/(1+(f/f0)^2)^2, which
        // is 12 dB down at the cutoff and only reaches its full 24 dB/octave
        // well above it — 21.3 dB across the first octave, 23.3 across the
        // second. Matching the curve is the stronger claim than matching an
        // asymptote.
        let sr = 44100.0;
        for norm in [0.3, 0.5] {
            let ir = filter_response(norm, 0.0);
            let f0 = cutoff_hz(norm);
            let at_dc = magnitude_at(&ir, 5.0, sr);
            for multiple in [1.0f64, 2.0, 4.0, 8.0] {
                let want = -40.0 * (1.0 + multiple * multiple).log10();
                let got = 20.0 * (magnitude_at(&ir, f0 * multiple, sr) / at_dc).log10();
                assert!(
                    (got - want).abs() < 0.6,
                    "cutoff {f0:.0} Hz at {multiple}x: {got:.1} dB, four poles owe {want:.1} dB"
                );
            }
        }
    }

    #[test]
    fn resonance_peaks_and_then_oscillates() {
        let sr = 44100.0;
        let norm = 0.5;
        let f0 = cutoff_hz(norm);
        let flat = filter_response(norm, 0.0);
        let mut previous = 20.0 * (magnitude_at(&flat, f0, sr) / magnitude_at(&flat, 5.0, sr)).log10();
        for res in [0.4, 0.7, 0.9] {
            let ir = filter_response(norm, res);
            let peak = 20.0 * (magnitude_at(&ir, f0, sr) / magnitude_at(&flat, 5.0, sr)).log10();
            assert!(peak > previous + 3.0, "resonance {res} added {:.1} dB", peak - previous);
            previous = peak;
        }
        // Self-oscillation: kicked once, still ringing a second later with no
        // input at all.
        let mut f = JunoFilter::new();
        for _ in 0..64 { f.process(0.5, norm, 1.0, sr); }
        let mut tail = 0.0f64;
        for i in 0..(sr as usize) {
            let out = f.process(0.0, norm, 1.0, sr);
            if i > sr as usize - 4410 { tail = tail.max(out.abs()); }
        }
        assert!(tail > 0.01, "the filter does not self-oscillate: tail {tail:.5}");
    }

    #[test]
    fn the_hpf_corners_are_the_measured_ones() {
        // 154, 339 and 720 Hz on a Juno-60. The first of the three used to be
        // 225 Hz, which is the Juno-106's corner, not this instrument's.
        let sr = 44100.0;
        for (position, want) in HPF_CORNERS.iter().enumerate() {
            let mut h = JunoHpf::new();
            let ir: Vec<f64> = (0..16384)
                .map(|i| h.process(if i == 0 { 1.0 } else { 0.0 }, position as u8 + 1, sr))
                .collect();
            let passband = magnitude_at(&ir, 15000.0, sr);
            let at_corner = magnitude_at(&ir, *want, sr);
            let drop = 20.0 * (at_corner / passband).log10();
            assert!((drop + 3.0).abs() < 0.3, "position {position}: {drop:.2} dB at {want} Hz");
            let a = magnitude_at(&ir, want / 4.0, sr);
            let b = magnitude_at(&ir, want / 8.0, sr);
            let slope = 20.0 * (a / b).log10();
            assert!((slope - 6.0).abs() < 0.5, "position {position}: {slope:.1} dB/oct");
        }
        let mut h = JunoHpf::new();
        assert_eq!(h.process(0.7, 0, sr), 0.7, "position 0 is not a bypass");
    }

    // ── Envelope ──

    fn stage_seconds(mut env: JunoEnvelope, stage: EnvStage) -> f64 {
        let mut n = 0u64;
        while env.stage == stage && n < 44100 * 200 {
            env.tick();
            n += 1;
        }
        n as f64 / 44100.0
    }

    #[test]
    fn the_envelope_takes_the_time_the_slider_says() {
        // The defect: decay and release used their slider value as a *time
        // constant* and ran on for 6.9 of them, so the panel's 12 s took 83 s
        // to finish. Every segment now takes the time it advertises.
        for slider in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let want_attack = attack_seconds(slider);
            let want = decay_seconds(slider);

            let mut e = JunoEnvelope::new(44100.0);
            e.set_times(want_attack, 100.0, 100.0);
            e.sustain = 1.0;
            e.trigger();
            let measured = stage_seconds(e, EnvStage::Attack);
            assert!((measured - want_attack).abs() < want_attack * 0.02 + 0.001,
                    "attack {slider}: {measured:.3} s for a {want_attack:.3} s setting");

            let mut e = JunoEnvelope::new(44100.0);
            e.set_times(0.0005, want, 100.0);
            e.sustain = 0.0;
            e.level = 1.0;
            e.enter_decay();
            let measured = stage_seconds(e, EnvStage::Decay);
            assert!((measured - want).abs() < want * 0.02 + 0.002,
                    "decay {slider}: {measured:.3} s for a {want:.3} s setting");

            let mut e = JunoEnvelope::new(44100.0);
            e.set_times(0.0005, 100.0, want);
            e.level = 1.0;
            e.stage = EnvStage::Sustain;
            e.release_env();
            let measured = stage_seconds(e, EnvStage::Release);
            assert!((measured - want).abs() < want * 0.02 + 0.002,
                    "release {slider}: {measured:.3} s for a {want:.3} s setting");
        }
    }

    #[test]
    fn the_envelope_taper_matches_the_measured_instrument() {
        // Slider position against measured seconds, from a real Juno-60. The
        // taper used to be linear, which put the middle of the decay slider at
        // 6 s where the instrument gives 1.
        for (slider, want) in [(0.0, 0.001), (0.25, 0.03), (0.5, 0.24), (0.75, 0.65), (1.0, 3.25)] {
            let got = attack_seconds(slider);
            assert!(got > want * 0.4 && got < want * 2.5,
                    "attack at {slider}: {got:.3} s, instrument {want} s");
        }
        for (slider, want) in
            [(0.0, 0.002), (0.25, 0.096), (0.5, 0.984), (0.75, 4.449), (1.0, 19.783)]
        {
            let got = decay_seconds(slider);
            assert!(got > want * 0.4 && got < want * 2.5,
                    "decay at {slider}: {got:.3} s, instrument {want} s");
        }
    }

    #[test]
    fn the_envelope_segments_are_curved_like_a_capacitor() {
        // Measured on the instrument: the attack is 62% of the way up at the
        // half-way point, not 50%, and the decay is at 15%.
        let mut e = JunoEnvelope::new(44100.0);
        e.set_times(2.0, 100.0, 100.0);
        e.sustain = 1.0;
        e.trigger();
        let mut level = 0.0;
        for _ in 0..44100 { level = e.tick(); }
        assert!((level - 0.632).abs() < 0.02, "attack half way: {level:.3}");

        let mut e = JunoEnvelope::new(44100.0);
        e.set_times(0.001, 2.0, 100.0);
        e.sustain = 0.0;
        e.level = 1.0;
        e.enter_decay();
        let mut level = 1.0;
        for _ in 0..44100 { level = e.tick(); }
        assert!((level - 0.148).abs() < 0.02, "decay half way: {level:.3}");
    }

    #[test]
    fn the_gate_switch_replaces_the_envelope_without_clicking() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        s.set_parameter(P_VCA_MODE, knob_for(1, 2));
        s.set_parameter(P_ATTACK, 1.0); // a slow envelope the gate must ignore
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 8);
        // Open inside the gate's 3 ms, and no step at the note-on. The window
        // has to sit inside that ramp to say anything: the gate is 3.5 time
        // constants wide, so it is already past half way after 0.7 ms.
        assert!(out[0].abs() < 0.01, "gate stepped on at the note-on: {}", out[0]);
        assert!(peak(&out[..8]) < 0.25 * peak(&out[400..]), "gate did not ramp");
        assert!(peak(&out[400..]) > 0.01, "gate never opened");
    }

    // ── LFO ──

    #[test]
    fn the_lfo_delay_holds_then_fades() {
        // Measured on the instrument as two stages, not one: silence, then a
        // fade. At the top of the slider that is 2.79 s before anything is
        // audible and another second to full depth.
        let (hold, fade) = lfo_delay_times(1.0);
        assert!((hold - 2.786).abs() < 1e-6 && (fade - 1.0).abs() < 1e-6);
        let (hold, fade) = lfo_delay_times(0.0);
        assert_eq!((hold, fade), (0.0, 0.0));

        let mut lfo = JunoLfo::new();
        lfo.rate = 5.0;
        lfo.hold = 0.5;
        lfo.fade = 0.5;
        lfo.trigger_delay();
        let mut silent = 0.0f64;
        for _ in 0..22050 { silent = silent.max(lfo.tick(44100.0).abs()); }
        assert!(silent < 1e-9, "the LFO was audible during the hold: {silent}");
        let mut faded = 0.0f64;
        for _ in 0..22050 { faded = faded.max(lfo.tick(44100.0).abs()); }
        assert!(faded > 0.9, "the LFO did not reach full depth: {faded}");
    }

    #[test]
    fn the_lfo_delay_does_not_restart_mid_chord() {
        // The delay runs from the gate going high. A note added to a held
        // chord used to reset it, which restarted the vibrato under the notes
        // already sounding.
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        s.set_parameter(P_LFO_DELAY, 1.0);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 200);
        let before = s.lfo.elapsed;
        process_buffers(&mut s, &[note_on(64, 100, 0)], 1);
        assert!(s.lfo.elapsed > before, "the second key restarted the delay");
        process_buffers(&mut s, &[note_off(60, 0), note_off(64, 1)], 400);
        process_buffers(&mut s, &[note_on(67, 100, 0)], 1);
        assert!(s.lfo.elapsed < before, "a fresh phrase did not restart the delay");
    }

    // ── Chorus ──

    #[test]
    fn the_chorus_runs_at_the_measured_rates() {
        // 0.513 Hz, 0.863 Hz, and 9.75 Hz with both switches down, measured on
        // a Juno-60. The third mode's sweep used to be scaled to 8% of its
        // range, which left it 12 dB shallower than the instrument.
        let sr = 44100.0;
        for (mode, want_rate, want_sweep_ms) in [(1u8, 0.513, 3.69), (2, 0.863, 3.69), (3, 9.75, 0.4)] {
            let mut c = BbdChorus::new(sr);
            let mut wraps = Vec::new();
            let mut previous = c.lfo_phase;
            for i in 0..(sr as usize * 4) {
                c.process(0.0, mode, sr);
                if c.lfo_phase < previous { wraps.push(i); }
                previous = c.lfo_phase;
            }
            let period = (wraps[wraps.len() - 1] - wraps[0]) as f64 / (wraps.len() - 1) as f64;
            let measured = sr / period;
            assert!((measured - want_rate).abs() < 0.01, "mode {mode}: {measured:.3} Hz");
            let (_, min_d, max_d, _) = chorus_mode(mode);
            let sweep = (max_d - min_d) * 1000.0;
            assert!((sweep - want_sweep_ms).abs() < 0.01, "mode {mode}: {sweep:.2} ms sweep");
        }
    }

    // ── Level ──

    #[test]
    fn no_setting_of_the_new_controls_reaches_full_scale() {
        // The controls that were added are the ones that can add level: noise
        // and the sub are faders now rather than switches, and resonance is
        // reachable from the panel on every patch. Six voices, every source
        // up, the filter wide open and ringing.
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        s.set_parameter(P_SAW, knob_for(1, 2));
        s.set_parameter(P_PULSE, knob_for(1, 2));
        s.set_parameter(P_SUB, 1.0);
        s.set_parameter(P_NOISE, 1.0);
        s.set_parameter(P_CUTOFF, 1.0);
        s.set_parameter(P_RESO, 1.0);
        s.set_parameter(P_ENV_MOD, 1.0);
        s.set_parameter(P_HPF, knob_for(0, 4));
        s.set_parameter(P_LEVEL, 1.0);
        s.set_parameter(P_ATTACK, 0.0);
        s.set_parameter(P_SUSTAIN, 1.0);
        s.set_parameter(P_CHORUS, knob_for(3, 4));
        let events: Vec<MidiEvent> = [36, 43, 48, 55, 60, 64, 67, 72]
            .iter()
            .map(|&n| note_on(n, 127, 0))
            .collect();
        let out = process_buffers(&mut s, &events, 400);
        assert!(out.iter().all(|v| v.is_finite()), "not finite");
        assert!(peak(&out) < 1.0, "peak {} is at full scale", peak(&out));
    }
}
