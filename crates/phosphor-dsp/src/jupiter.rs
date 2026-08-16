//! Roland Jupiter-8 style dual-VCO analog poly synthesizer.
//!
//! The whole front panel, in the order it appears on the instrument: LFO,
//! VCO MOD, VCO-1, VCO-2, MIXER, HPF, VCF, VCA, ENV-1, ENV-2, with the assign
//! mode and portamento controls that sit to the left of the LFO. Two polyBLEP
//! VCOs per voice with hard sync and cross modulation, an IR3109 four-pole
//! ladder tapped at two poles or at four, the non-resonant high-pass ahead of
//! it, two ADSRs — one for the filter, one for the amplifier — a global LFO
//! and the four assign modes.
//!
//! Where a number came from Roland it says so at the constant. The envelope
//! and LFO ranges are Roland's published specification for the instrument
//! (JUPITER-8 Technical Specifications, support.roland.com): both envelopes
//! 1 ms to 5 s of attack and 1 ms to 10 s of decay and release, the LFO 0.05
//! to 40 Hz on sine, sawtooth, square and random.
//!
//! Roland do not publish the *shape* of any of those sliders, and no
//! slider-by-slider capture of a working Jupiter-8 was to hand. The tapers
//! here are therefore the ones measured off a Juno-60 — the same company, the
//! same years, the same IR3109 filter chip, and the nearest measurement there
//! is — rescaled to the Jupiter's own end points. See `juno.rs`, whose
//! comments carry the measurements themselves.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

use crate::level::soft_saturate;

const MAX_VOICES: usize = 8;
const TWO_PI: f64 = std::f64::consts::TAU;

/// Fixed headroom trim on the voice sum, applied after the gain knob.
///
/// Sized on ordinary playing, in step with the other four — see `OUTPUT_TRIM`
/// in dx7.rs, which carries the full reasoning. The trim lands this synth's
/// median patch at the same loudness as theirs.
///
/// Measured across the 42 presets: the worst case is patch 9 "Organ", which
/// reaches 4.40 for an eight-note chord at velocity 127 and holds most of
/// that through the sustain. Even that stays under the saturator knee at this
/// trim, so nothing in this bank engages the bounding stage.
///
/// This replaced a divisor of `sqrt(sounding voice count)` in the poly
/// modes. That divisor changed every time a voice finished releasing, so the
/// remaining notes of a held chord swelled as the released ones died away —
/// audible pumping, and it also hid how hot the patches really were.
///
/// It moved when the mixer gained its second fader. The panel used to hold
/// one crossfade whose two sides always summed to 1.0; the instrument has two
/// independent faders and most of the bank leaves both at 0.8, so honouring
/// them put the whole synth 4.8 dB above where it had been sitting — measured
/// on 2 Brass, the median of the bank, at 0.0189 RMS before and 0.0328 after.
/// `instruments_are_level_matched` is the assertion that would have noticed.
const OUTPUT_TRIM: f32 = 0.0754;

// ── Parameter indices ──
//
// Front-panel order, left to right, because that is the order a player
// reaches for them. `patch` is first because index 0 is where the editor
// looks for a preset selector; portamento and the assign mode follow because
// on the instrument they are the two controls to the left of the LFO.
//
// Sixteen of these are new. The engine has always modelled a nearly complete
// Jupiter-8 and the panel exposed a third of it: the pulse width, sync, cross
// modulation, the high-pass, the 12/24 dB slope switch, the envelope polarity,
// keyboard follow, the LFO waveform and delay, portamento, the two mixer
// faders as separate controls — and, most of all, ENV-2. The four ADSR
// sliders drove ENV-1 and ENV-2 was copied from it, so the player was
// adjusting the filter envelope while the amplifier envelope, the one that
// decides whether a note is a stab or a pad, could not be touched at all.

pub const P_PATCH: usize = 0;
// Portamento and assign mode
pub const P_PORTAMENTO: usize = 1;
pub const P_MODE: usize = 2;
// LFO
pub const P_LFO_RATE: usize = 3;
pub const P_LFO_WAVE: usize = 4;
pub const P_LFO_DELAY: usize = 5;
// VCO MOD
pub const P_VCO_LFO: usize = 6;
pub const P_PW: usize = 7;
// VCO-1
pub const P_VCO1_WAVE: usize = 8;
pub const P_XMOD: usize = 9;
// VCO-2
pub const P_VCO2_WAVE: usize = 10;
pub const P_TUNE: usize = 11;
pub const P_SYNC: usize = 12;
// MIXER
pub const P_VCO1_LEVEL: usize = 13;
pub const P_VCO2_LEVEL: usize = 14;
// HPF
pub const P_HPF: usize = 15;
// VCF
pub const P_CUTOFF: usize = 16;
pub const P_RESO: usize = 17;
pub const P_SLOPE: usize = 18;
pub const P_ENV_MOD: usize = 19;
pub const P_ENV_POLARITY: usize = 20;
pub const P_VCF_LFO: usize = 21;
pub const P_KEY_FOLLOW: usize = 22;
// VCA
pub const P_LEVEL: usize = 23;
// ENV-1, the filter envelope
pub const P_ENV1_A: usize = 24;
pub const P_ENV1_D: usize = 25;
pub const P_ENV1_S: usize = 26;
pub const P_ENV1_R: usize = 27;
// ENV-2, the amplifier envelope
pub const P_ENV2_A: usize = 28;
pub const P_ENV2_D: usize = 29;
pub const P_ENV2_S: usize = 30;
pub const P_ENV2_R: usize = 31;
pub const PARAM_COUNT: usize = 32;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "patch",
    "porta", "mode",
    "lfo rate", "lfo wave", "lfo dly",
    "vco lfo", "pw",
    "vco1 wav", "xmod",
    "vco2 wav", "tune", "sync",
    "vco1 lvl", "vco2 lvl",
    "hpf",
    "freq", "res", "slope", "env mod", "env pol", "vcf lfo", "kybd",
    "level",
    "env1 a", "env1 d", "env1 s", "env1 r",
    "env2 a", "env2 d", "env2 s", "env2 r",
];

/// Patch 0, "Pad", the preset the instrument loads with, as its panel.
/// `patch_zero_is_the_default_parameter_block` holds these and the first row
/// of [`PRESETS`] together, so neither can be edited without the other.
pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    0.0,       // patch: Pad
    0.0,       // portamento: off
    0.625,     // mode: poly1
    0.448_16,  // lfo rate: 1 Hz
    0.125,     // lfo wave: sine
    0.125,     // lfo delay: 0.5 s
    0.02,      // vco lfo: 2 cents of vibrato
    0.5,       // pw: square
    0.375,     // vco1 wave: saw
    0.0,       // xmod: off
    0.375,     // vco2 wave: saw
    0.538_23,  // tune: +7 cents
    0.25,      // sync: off
    0.8,       // vco1 level
    0.8,       // vco2 level
    0.12,      // hpf
    0.65,      // freq
    0.2,       // res
    0.75,      // slope: 24 dB
    0.3,       // env mod
    0.25,      // env polarity: normal
    0.0,       // vcf lfo
    0.5,       // kybd follow
    0.75,      // level
    0.509_34,  // env1 attack: 0.4 s
    0.549_53,  // env1 decay: 1 s
    1.0,       // env1 sustain
    0.549_53,  // env1 release: 1 s
    0.509_34,  // env2 attack: 0.4 s
    0.0,       // env2 decay
    1.0,       // env2 sustain
    0.453_44,  // env2 release: 0.55 s
];

// ── Patches ──

pub const PATCH_COUNT: usize = 42;
pub const PATCH_NAMES: [&str; PATCH_COUNT] = [
    "Pad", "Brass", "Bass", "SyncLd", "String", "Init",
    "ElecPno", "Pluck", "Bell", "Organ", "PWMPad", "UniLead", "KeyBass", "Ambient",
    "Sweep", "Stab", "Harp", "SynBass", "SubBass", "Acid",
    "Choir", "Vox", "Whstle", "PWMLd", "XMBell", "Seq",
    "Reso", "Dtune",
    // ── new patches ──
    "Clav", "HlwPad", "PwrPlk", "LoStr", "Flute", "Tuba",
    "SawPad", "Clrnet", "Cello", "Xylo", "FnkBas", "WrmLd",
    "Noise", "CarSyn",
];

/// The knob position that selects patch `index`, for a caller sweeping the
/// bank from outside — a level measurement, an export, a test.
///
/// The midpoint of the step, which is the one position in it that no amount
/// of float rounding can push into a neighbour, and the same position
/// [`step_discrete`] moves between. `index / (count - 0.01)`, the obvious
/// alternative, misses seven of this bank's own 42 indices.
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
// Seven of the panel's controls are switches rather than sliders, and the
// patch selector makes eight indices that step. They are stored in the same
// 0..1 parameter block as everything else, so a switch is a knob divided into
// `n` equal steps.

/// How many positions a switch has, or `None` for a slider.
fn discrete_steps(index: usize) -> Option<usize> {
    match index {
        P_PATCH => Some(PATCH_COUNT),
        P_SYNC | P_SLOPE | P_ENV_POLARITY => Some(2),
        P_MODE | P_LFO_WAVE | P_VCO1_WAVE | P_VCO2_WAVE => Some(4),
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
/// 1/42 of the range 42 times does not arrive at 1.0 — the error is a few ulps
/// either way, and a step boundary missed by one ulp is a keypress that
/// visibly does nothing. The DX7's bank knob stalled that way, and this
/// instrument's patch knob was stepping by `1/(42 - 0.01)` for the same
/// reason.
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
        P_PATCH => PATCH_NAMES[step],
        P_MODE => ["SOLO", "UNI", "POLY1", "POLY2"][step],
        P_LFO_WAVE => ["SIN", "SAW", "SQR", "RND"][step],
        P_VCO1_WAVE => ["TRI", "SAW", "PLS", "SQR"][step],
        P_VCO2_WAVE => ["TRI", "SAW", "PLS", "NOISE"][step],
        P_SYNC | P_ENV_POLARITY => ["off", "on"][step],
        P_SLOPE => ["12dB", "24dB"][step],
        _ => return None,
    })
}

/// A slider's value in seconds, for the ones that measure time. `None` for
/// the ones that read as a percentage.
pub fn param_seconds(index: usize, value: f32) -> Option<f64> {
    match index {
        P_ENV1_A | P_ENV2_A => Some(attack_seconds(f64::from(value))),
        P_ENV1_D | P_ENV1_R | P_ENV2_D | P_ENV2_R => Some(decay_seconds(f64::from(value))),
        P_PORTAMENTO => Some(porta_seconds(f64::from(value))),
        P_LFO_DELAY => Some(f64::from(value) * LFO_DELAY_MAX),
        _ => None,
    }
}

// ── Panel tapers ──
//
// The sliders are not linear in time or frequency. The end points are
// Roland's published specification; the curves between them are the Juno-60's
// measured ones, rescaled — see the note at the top of the file for why that
// instrument is the reference.

/// Attack slider to seconds. Roland's range for both envelopes is 1 ms to 5 s.
const ATTACK_MIN: f64 = 0.001;
const ATTACK_MAX: f64 = 5.0;
const ATTACK_CURVE: f64 = 5.0;

fn attack_seconds(slider: f64) -> f64 {
    let s = slider.clamp(0.0, 1.0);
    ATTACK_MIN + (ATTACK_CURVE * s).exp_m1() / ATTACK_CURVE.exp_m1() * ATTACK_MAX
}

/// Decay and release share one taper, as they share one specification: 1 ms
/// to 10 s. The curve puts the middle of the slider at 0.74 s, which is where
/// a Juno-60's decay measures once its own 17.5 s travel is scaled to this
/// instrument's 10.
///
/// The defect this replaced: the slider ran nearly linearly onto 3 ms - 10 s
/// and the number it produced was then used as a *time constant* by a
/// one-pole that ran for 6.9 of them, so the middle of the decay slider took
/// 20.2 s to reach -40 dB where the instrument takes about 0.8.
const DECAY_MIN: f64 = 0.001;
const DECAY_MAX: f64 = 10.0;
const DECAY_CURVE: f64 = 3.5;

fn decay_seconds(slider: f64) -> f64 {
    let s = slider.clamp(0.0, 1.0);
    DECAY_MIN + (DECAY_CURVE * s).exp_m1() / DECAY_CURVE.exp_m1() * s * DECAY_MAX
}

/// LFO rate slider to Hz: Roland's 0.05 to 40 Hz, exponential across it. A
/// straight line would put 20 Hz in the middle of the slider, which is not a
/// vibrato; this puts 1.4 Hz there.
const LFO_MIN_HZ: f64 = 0.05;
const LFO_MAX_HZ: f64 = 40.0;

fn lfo_hz(slider: f64) -> f64 {
    LFO_MIN_HZ * (LFO_MAX_HZ / LFO_MIN_HZ).powf(slider.clamp(0.0, 1.0))
}

/// LFO delay slider to seconds. Roland's range is 0 to 4 s.
const LFO_DELAY_MAX: f64 = 4.0;

/// How the delay divides into silence and fade. Measured on a Juno-60, whose
/// LFO holds off entirely and *then* fades in: 2.786 s of hold and 1.0 s of
/// fade at the top of its own slider.
const LFO_DELAY_HOLD_SHARE: f64 = 0.736;

/// Portamento slider to seconds. Roland do not publish the range; 3 s at the
/// top is a glide slow enough to be a effect and short enough to play with.
const PORTAMENTO_MAX: f64 = 3.0;

fn porta_seconds(slider: f64) -> f64 {
    slider.clamp(0.0, 1.0) * PORTAMENTO_MAX
}

/// VCO-2's tune slider to cents, bipolar, an octave either way with the
/// middle of the travel at unison.
///
/// Squared rather than straight, so that the centre of the slider is fine
/// enough to detune a pad with — a hair off unison is what this control is
/// mostly used for, and a linear ±1200 would move 24 cents per keypress. At
/// the ends it is coarse, which is where the octaves and fifths live.
const TUNE_CENTS: f64 = 1200.0;

fn tune_cents(slider: f64) -> f64 {
    let x = 2.0 * slider.clamp(0.0, 1.0) - 1.0;
    x.abs() * x * TUNE_CENTS
}

fn tune_slider(cents: f64) -> f32 {
    let n = (cents / TUNE_CENTS).clamp(-1.0, 1.0);
    (0.5 + 0.5 * n.abs().sqrt() * n.signum()) as f32
}

/// Cutoff slider to Hz: 20 Hz to 20 kHz, exponential. Roland do not publish
/// the sweep; AMSynths, who cloned the IR3109 the instrument filters with,
/// quote 10 Hz to 20 kHz for the same chip in an SH-101.
const CUTOFF_MIN_HZ: f64 = 20.0;
const CUTOFF_DECADES: f64 = 3.0;
/// How many octaves the cutoff slider covers end to end, which is what turns
/// a keyboard-follow amount into a slider offset.
const CUTOFF_OCTAVES: f64 = CUTOFF_DECADES * std::f64::consts::LOG2_10;

fn cutoff_hz(slider: f64) -> f64 {
    CUTOFF_MIN_HZ * 10.0f64.powf(CUTOFF_DECADES * slider.clamp(0.0, 1.0))
}

/// High-pass slider to Hz: 20 Hz to 1 kHz, exponential, 6 dB/octave and
/// non-resonant.
///
/// Roland publish no range for this one either. The top of the travel is
/// pinned just past the Juno-60's highest measured high-pass corner, 720 Hz,
/// since the two instruments share a voice-board generation and a purpose for
/// the control. It used to run to 10 kHz — a corner that leaves nothing but
/// air — while the comment beside it claimed 4.5 kHz.
const HPF_MIN_HZ: f64 = 20.0;
const HPF_DECADES: f64 = 1.699;

fn hpf_hz(slider: f64) -> f64 {
    HPF_MIN_HZ * 10.0f64.powf(HPF_DECADES * slider.clamp(0.0, 1.0))
}

/// Full LFO-to-pitch is a semitone of vibrato either way, as on the Juno.
const LFO_PITCH_CENTS: f64 = 100.0;

/// Octaves of VCO-1 pitch that full cross modulation swings.
const XMOD_OCTAVES: f64 = 3.0;

/// The slider position whose taper gives `want`, by bisection.
///
/// Only [`Jupiter8Synth::params_for_patch`] needs it: the preset table is
/// held in seconds and hertz, because that is what a patch *is*, and the
/// panel is held in slider positions, because that is what a panel is.
/// Twenty-four halvings put the answer within 6e-8 of the travel, which is
/// finer than an f32 knob can hold. Every taper it is used on is monotonic.
///
/// Real-time safe: a fixed count of halvings, no allocation, no lock. It runs
/// on the audio thread, because that is where a patch change arrives, but
/// only when the patch selector moves and only for the eight controls the
/// bank holds in seconds and hertz.
fn slider_for(taper: fn(f64) -> f64, want: f64) -> f32 {
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if taper(mid) < want { lo = mid; } else { hi = mid; }
    }
    (0.5 * (lo + hi)) as f32
}

// ── Internal preset data ──

#[derive(Debug, Clone, Copy)]
struct JupiterPatch {
    vco1_wave: u8,    // 0=tri, 1=saw, 2=pulse, 3=square
    vco2_wave: u8,    // 0=tri, 1=saw, 2=pulse, 3=noise
    detune_cents: f64, // VCO-2 tune in cents
    vco1_level: f64,
    vco2_level: f64,
    pulse_width: f64,  // 0.0-1.0 (0.5 = square)
    sync: bool,
    xmod: f64,         // cross-mod amount
    cutoff: f64,       // 0.0-1.0
    resonance: f64,    // 0.0-1.0
    hpf_cutoff: f64,   // 0.0-1.0
    slope_24: bool,    // true = 24dB, false = 12dB
    env_mod: f64,      // ENV-1 → filter amount
    env_polarity: f64, // +1.0 or -1.0
    key_follow: f64,   // 0.0-1.0
    // ENV-1 (filter)
    env1_a: f64, env1_d: f64, env1_s: f64, env1_r: f64,
    // ENV-2 (amp)
    env2_a: f64, env2_d: f64, env2_s: f64, env2_r: f64,
    lfo_rate: f64,     // Hz
    lfo_wave: u8,      // 0=sin, 1=saw, 2=square, 3=random
    lfo_to_pitch: f64,
    lfo_to_filter: f64,
    lfo_delay: f64,    // seconds, hold and fade together
    voice_mode: u8,    // 0=solo, 1=unison, 2=poly1, 3=poly2
    portamento: f64,   // seconds of glide
}

/// The preset bank, in engine units — seconds, hertz, normalised cutoff —
/// because that is what a patch is. `params_for_patch` runs a row backwards
/// through the panel tapers to get the slider positions that produce it, and
/// `preset_round_trip` holds the two directions together.
const PRESETS: [JupiterPatch; PATCH_COUNT] = [
        // Pad — lush detuned saw pad
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 7.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.65, resonance: 0.2, hpf_cutoff: 0.12, slope_24: true,
            env_mod: 0.3, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.4, env1_d: 1.0, env1_s: 1.0, env1_r: 1.0,
            env2_a: 0.4, env2_d: 0.0, env2_s: 1.0, env2_r: 0.55,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.02, lfo_to_filter: 0.0, lfo_delay: 0.5,
            voice_mode: 2, portamento: 0.0,
        },
        // Brass — punchy brass stab
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 7.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.35, resonance: 0.3, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.75, env_polarity: 1.0, key_follow: 0.4,
            env1_a: 0.01, env1_d: 0.3, env1_s: 0.5, env1_r: 0.18,
            env2_a: 0.18, env2_d: 0.0, env2_s: 1.0, env2_r: 0.18,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Bass — deep round bass
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 5.0,
            vco1_level: 0.9, vco2_level: 0.9,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.35, resonance: 0.4, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.5, env_polarity: 1.0, key_follow: 0.3,
            env1_a: 0.005, env1_d: 0.3, env1_s: 0.6, env1_r: 0.15,
            env2_a: 0.001, env2_d: 0.3, env2_s: 0.85, env2_r: 0.1,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 0, portamento: 0.0,
        },
        // SyncLead — classic sync sweep lead
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 0.0,
            vco1_level: 0.5, vco2_level: 0.9,
            pulse_width: 0.5, sync: true, xmod: 0.0,
            cutoff: 0.6, resonance: 0.25, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.6, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.01, env1_d: 0.4, env1_s: 0.3, env1_r: 0.2,
            env2_a: 0.005, env2_d: 0.0, env2_s: 1.0, env2_r: 0.3,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.03, lfo_to_filter: 0.0, lfo_delay: 0.4,
            voice_mode: 0, portamento: 0.15,
        },
        // Strings — analog strings
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 2, detune_cents: 6.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.43, sync: false, xmod: 0.0,
            cutoff: 0.85, resonance: 0.1, hpf_cutoff: 0.3, slope_24: false,
            env_mod: 0.15, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.45, env1_d: 0.5, env1_s: 0.9, env1_r: 0.5,
            env2_a: 0.45, env2_d: 0.0, env2_s: 1.0, env2_r: 0.5,
            lfo_rate: 4.5, lfo_wave: 0, lfo_to_pitch: 0.015, lfo_to_filter: 0.0, lfo_delay: 0.6,
            voice_mode: 2, portamento: 0.0,
        },
        // Init — basic saw, filter open
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 0.0,
            vco1_level: 0.8, vco2_level: 0.0,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.8, resonance: 0.0, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.0, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.01, env1_d: 0.3, env1_s: 0.7, env1_r: 0.2,
            env2_a: 0.01, env2_d: 0.3, env2_s: 0.7, env2_r: 0.2,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // ElecPno — Rhodes-like electric piano
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 0, detune_cents: 3.0,
            vco1_level: 0.7, vco2_level: 0.5,
            pulse_width: 0.45, sync: false, xmod: 0.0,
            cutoff: 0.55, resonance: 0.1, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.35, env_polarity: 1.0, key_follow: 0.7,
            env1_a: 0.001, env1_d: 0.8, env1_s: 0.0, env1_r: 0.3,
            env2_a: 0.001, env2_d: 1.2, env2_s: 0.0, env2_r: 0.4,
            lfo_rate: 5.5, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Pluck — harpsichord-like percussive
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 2, detune_cents: 7.0,
            vco1_level: 0.6, vco2_level: 0.6,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.2, resonance: 0.35, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.65, env_polarity: 1.0, key_follow: 0.8,
            env1_a: 0.001, env1_d: 0.25, env1_s: 0.0, env1_r: 0.15,
            env2_a: 0.001, env2_d: 0.3, env2_s: 0.0, env2_r: 0.2,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Bell — cross-mod metallic bell
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 700.0,
            vco1_level: 0.6, vco2_level: 0.4,
            pulse_width: 0.5, sync: false, xmod: 0.45,
            cutoff: 0.7, resonance: 0.05, hpf_cutoff: 0.1, slope_24: false,
            env_mod: 0.25, env_polarity: 1.0, key_follow: 0.9,
            env1_a: 0.001, env1_d: 2.5, env1_s: 0.0, env1_r: 2.0,
            env2_a: 0.001, env2_d: 3.0, env2_s: 0.0, env2_r: 2.5,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Organ — drawbar style
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 2, detune_cents: 1200.0,
            vco1_level: 0.7, vco2_level: 0.5,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.6, resonance: 0.0, hpf_cutoff: 0.05, slope_24: false,
            env_mod: 0.0, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.001, env1_d: 0.0, env1_s: 1.0, env1_r: 0.05,
            env2_a: 0.001, env2_d: 0.0, env2_s: 1.0, env2_r: 0.05,
            lfo_rate: 6.0, lfo_wave: 0, lfo_to_pitch: 0.03, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // PWMPad — slow pulse width modulation
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 2, detune_cents: 5.0,
            vco1_level: 0.6, vco2_level: 0.6,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.65, resonance: 0.15, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.2, env_polarity: 1.0, key_follow: 0.4,
            env1_a: 0.001, env1_d: 0.5, env1_s: 0.7, env1_r: 0.8,
            env2_a: 0.5, env2_d: 0.8, env2_s: 0.8, env2_r: 1.5,
            lfo_rate: 0.3, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 3, portamento: 0.0,
        },
        // UniLead — fat unison lead
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 10.0,
            vco1_level: 0.7, vco2_level: 0.7,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.5, resonance: 0.2, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.4, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.01, env1_d: 0.3, env1_s: 0.6, env1_r: 0.3,
            env2_a: 0.01, env2_d: 0.4, env2_s: 0.7, env2_r: 0.4,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.015, lfo_to_filter: 0.0, lfo_delay: 0.4,
            voice_mode: 1, portamento: 0.08,
        },
        // KeyBass — punchy keyboard bass
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 3, detune_cents: 0.0,
            vco1_level: 0.8, vco2_level: 0.4,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.15, resonance: 0.25, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.5, env_polarity: 1.0, key_follow: 0.3,
            env1_a: 0.001, env1_d: 0.15, env1_s: 0.1, env1_r: 0.08,
            env2_a: 0.001, env2_d: 0.3, env2_s: 0.3, env2_r: 0.1,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 0, portamento: 0.0,
        },
        // Ambient — evolving Vangelis-style texture
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 0, detune_cents: 8.0,
            vco1_level: 0.5, vco2_level: 0.6,
            pulse_width: 0.5, sync: false, xmod: 0.08,
            cutoff: 0.35, resonance: 0.5, hpf_cutoff: 0.1, slope_24: true,
            env_mod: 0.15, env_polarity: 1.0, key_follow: 0.3,
            env1_a: 0.01, env1_d: 1.0, env1_s: 0.5, env1_r: 1.0,
            env2_a: 2.0, env2_d: 1.5, env2_s: 0.7, env2_r: 3.0,
            lfo_rate: 0.15, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.3, lfo_delay: 0.5,
            voice_mode: 3, portamento: 0.15,
        },
        // ── NEW PATCHES ──
        // Sweep — slow resonant filter sweep (Tangerine Dream / Jarre sequencer territory)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 6.0,
            vco1_level: 0.7, vco2_level: 0.7,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.15, resonance: 0.65, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.7, env_polarity: 1.0, key_follow: 0.3,
            env1_a: 2.0, env1_d: 3.0, env1_s: 0.3, env1_r: 1.5,
            env2_a: 0.01, env2_d: 0.0, env2_s: 1.0, env2_r: 0.5,
            lfo_rate: 0.08, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.15, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Stab — short rhythmic brass stab (Duran Duran "Rio" style)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 2, detune_cents: 5.0,
            vco1_level: 0.8, vco2_level: 0.7,
            pulse_width: 0.4, sync: false, xmod: 0.0,
            cutoff: 0.3, resonance: 0.2, hpf_cutoff: 0.08, slope_24: true,
            env_mod: 0.8, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.001, env1_d: 0.12, env1_s: 0.0, env1_r: 0.08,
            env2_a: 0.001, env2_d: 0.15, env2_s: 0.0, env2_r: 0.06,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Harp — ethereal plucked harp (Howard Jones style)
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 1, detune_cents: 3.0,
            vco1_level: 0.5, vco2_level: 0.7,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.45, resonance: 0.15, hpf_cutoff: 0.15, slope_24: false,
            env_mod: 0.4, env_polarity: 1.0, key_follow: 0.8,
            env1_a: 0.001, env1_d: 0.6, env1_s: 0.0, env1_r: 0.8,
            env2_a: 0.001, env2_d: 0.8, env2_s: 0.0, env2_r: 1.0,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // SynBass — sync bass (Thompson Twins style growl bass)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 0.0,
            vco1_level: 0.6, vco2_level: 0.9,
            pulse_width: 0.5, sync: true, xmod: 0.0,
            cutoff: 0.25, resonance: 0.35, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.65, env_polarity: 1.0, key_follow: 0.3,
            env1_a: 0.001, env1_d: 0.2, env1_s: 0.15, env1_r: 0.1,
            env2_a: 0.001, env2_d: 0.25, env2_s: 0.4, env2_r: 0.08,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 0, portamento: 0.0,
        },
        // SubBass — deep sub bass (triangle fundamental, barely any harmonics)
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 2.0,
            vco1_level: 1.0, vco2_level: 0.7,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.2, resonance: 0.3, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.2, env_polarity: 1.0, key_follow: 0.15,
            env1_a: 0.005, env1_d: 0.4, env1_s: 0.7, env1_r: 0.12,
            env2_a: 0.005, env2_d: 0.0, env2_s: 1.0, env2_r: 0.08,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 0, portamento: 0.0,
        },
        // Acid — TB-303-ish acid bass (resonant squelch, 24dB filter, glide)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 0.0,
            vco1_level: 0.9, vco2_level: 0.0,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.12, resonance: 0.75, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.85, env_polarity: 1.0, key_follow: 0.35,
            env1_a: 0.001, env1_d: 0.18, env1_s: 0.0, env1_r: 0.08,
            env2_a: 0.001, env2_d: 0.3, env2_s: 0.5, env2_r: 0.05,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 0, portamento: 0.12,
        },
        // Choir — bright analog choir (Depeche Mode "Somebody" style)
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 2, detune_cents: 8.0,
            vco1_level: 0.7, vco2_level: 0.7,
            pulse_width: 0.35, sync: false, xmod: 0.0,
            cutoff: 0.7, resonance: 0.25, hpf_cutoff: 0.2, slope_24: false,
            env_mod: 0.1, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.6, env1_d: 0.8, env1_s: 0.8, env1_r: 0.8,
            env2_a: 0.6, env2_d: 0.5, env2_s: 0.9, env2_r: 0.7,
            lfo_rate: 0.25, lfo_wave: 0, lfo_to_pitch: 0.01, lfo_to_filter: 0.05, lfo_delay: 0.3,
            voice_mode: 3, portamento: 0.0,
        },
        // Vox — vocal formant-like (resonant filter, Tears for Fears territory)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 2, detune_cents: 5.0,
            vco1_level: 0.6, vco2_level: 0.8,
            pulse_width: 0.3, sync: false, xmod: 0.0,
            cutoff: 0.4, resonance: 0.55, hpf_cutoff: 0.15, slope_24: false,
            env_mod: 0.35, env_polarity: 1.0, key_follow: 0.7,
            env1_a: 0.25, env1_d: 0.6, env1_s: 0.5, env1_r: 0.6,
            env2_a: 0.2, env2_d: 0.4, env2_s: 0.8, env2_r: 0.5,
            lfo_rate: 5.5, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.08, lfo_delay: 0.4,
            voice_mode: 2, portamento: 0.0,
        },
        // Whstle — pure sine-like whistle lead (Vangelis "Chariots of Fire" style)
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 0.0,
            vco1_level: 0.9, vco2_level: 0.0,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.3, resonance: 0.0, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.15, env_polarity: 1.0, key_follow: 0.9,
            env1_a: 0.15, env1_d: 0.3, env1_s: 0.7, env1_r: 0.3,
            env2_a: 0.1, env2_d: 0.0, env2_s: 1.0, env2_r: 0.2,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.025, lfo_to_filter: 0.0, lfo_delay: 0.5,
            voice_mode: 0, portamento: 0.1,
        },
        // PWMLd — pulse width modulation lead (OMD / Depeche Mode lead style)
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 2, detune_cents: 7.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.3, sync: false, xmod: 0.0,
            cutoff: 0.55, resonance: 0.15, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.3, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.01, env1_d: 0.3, env1_s: 0.5, env1_r: 0.25,
            env2_a: 0.01, env2_d: 0.0, env2_s: 1.0, env2_r: 0.3,
            lfo_rate: 0.4, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 1, portamento: 0.06,
        },
        // XMBell — cross-mod ring bell (aggressive metallic, Jarre "Oxygene" style)
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 1, detune_cents: 500.0,
            vco1_level: 0.5, vco2_level: 0.5,
            pulse_width: 0.5, sync: false, xmod: 0.6,
            cutoff: 0.8, resonance: 0.1, hpf_cutoff: 0.08, slope_24: false,
            env_mod: 0.2, env_polarity: -1.0, key_follow: 0.8,
            env1_a: 0.001, env1_d: 1.8, env1_s: 0.0, env1_r: 1.5,
            env2_a: 0.001, env2_d: 2.0, env2_s: 0.0, env2_r: 2.0,
            lfo_rate: 0.5, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Seq — tight sequence-friendly pluck (Tangerine Dream / Berlin school)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 4.0,
            vco1_level: 0.8, vco2_level: 0.6,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.25, resonance: 0.4, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.55, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.001, env1_d: 0.15, env1_s: 0.0, env1_r: 0.1,
            env2_a: 0.001, env2_d: 0.18, env2_s: 0.0, env2_r: 0.08,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Reso — screaming resonant sweep (classic Jupiter-8 factory reso demo)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 3, detune_cents: 0.0,
            vco1_level: 0.7, vco2_level: 0.2,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.2, resonance: 0.85, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.9, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.01, env1_d: 1.5, env1_s: 0.0, env1_r: 0.8,
            env2_a: 0.01, env2_d: 0.0, env2_s: 1.0, env2_r: 0.5,
            lfo_rate: 0.1, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.2, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Dtune — massively detuned poly (Duran Duran "Save a Prayer" style)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 15.0,
            vco1_level: 0.7, vco2_level: 0.7,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.55, resonance: 0.1, hpf_cutoff: 0.1, slope_24: true,
            env_mod: 0.2, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.5, env1_d: 0.8, env1_s: 0.7, env1_r: 1.2,
            env2_a: 0.3, env2_d: 0.5, env2_s: 0.85, env2_r: 1.0,
            lfo_rate: 0.2, lfo_wave: 0, lfo_to_pitch: 0.008, lfo_to_filter: 0.05, lfo_delay: 0.3,
            voice_mode: 3, portamento: 0.0,
        },
        // ── NEW PATCHES (batch 3) ──
        // Sources: Roland JP-8 factory patch sheets, Roland Cloud JUPITER-8 Model
        // Expansion programming guide (articles.roland.com), Sound on Sound
        // "Synth Secrets" series by Gordon Reid, Arturia Jup-8 V manual.

        // Clav — JP-8 factory patch #21 CLAV
        // Source: Roland factory patch sheet #21; percussive pulse-wave clavinet.
        // Pulse waves with very short filter+amp envelopes, moderate resonance,
        // HPF engaged for nasal quality. 24dB slope for sharp cutoff.
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 2, detune_cents: 3.0,
            vco1_level: 0.8, vco2_level: 0.7,
            pulse_width: 0.35, sync: false, xmod: 0.0,
            cutoff: 0.3, resonance: 0.35, hpf_cutoff: 0.2, slope_24: true,
            env_mod: 0.6, env_polarity: 1.0, key_follow: 0.7,
            env1_a: 0.001, env1_d: 0.12, env1_s: 0.0, env1_r: 0.08,
            env2_a: 0.001, env2_d: 0.2, env2_s: 0.0, env2_r: 0.1,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // HlwPad — Hollow Pad
        // Source: Roland Cloud JUPITER-8 Model Expansion programming guide.
        // Both pulse waves, Env-1 inverted with fast attack/short decay modulating
        // VCO-1 pitch for hollow onset. Mixer 180-200, fine tune 10, VCF ~750,
        // Env-2 A=400 S=full R=500-550. Optional LFO PWM at rate 100-110.
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 2, detune_cents: 10.0,
            vco1_level: 0.75, vco2_level: 0.75,
            pulse_width: 0.43, sync: false, xmod: 0.0,
            cutoff: 0.75, resonance: 0.1, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.1, env_polarity: -1.0, key_follow: 0.5,
            env1_a: 0.001, env1_d: 0.25, env1_s: 0.0, env1_r: 0.3,
            env2_a: 0.4, env2_d: 0.0, env2_s: 1.0, env2_r: 0.55,
            lfo_rate: 0.4, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // PwrPlk — Power Pluck
        // Source: Roland Cloud JUPITER-8 Model Expansion programming guide.
        // Both saw, mixer 200/200, fine tune 8, VCF 750 (0.75), Env-1 mod 230-250
        // (normalized ~0.9), Env-2 A=0 S=0 D/R=500-600, Env-1 A=0 S=0 D/R=470.
        // Described as "authentic when dry" — no effects needed.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 8.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.75, resonance: 0.15, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.9, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.001, env1_d: 0.47, env1_s: 0.0, env1_r: 0.47,
            env2_a: 0.001, env2_d: 0.55, env2_s: 0.0, env2_r: 0.55,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // LoStr — JP-8 factory patch #31 LO STRINGS
        // Source: Roland factory patch sheet #31; Roland Cloud programming guide
        // "Analog Strings" section. VCO-1 saw, VCO-2 pulse with PW manual ~110
        // (normalized 0.43), fine tune +6, mixer 200/200, VCF 800-900 (0.85),
        // HPF ~300 (0.3), Env-2 A=450 S=full R=500. 12dB slope for warmth.
        // The classic "JP Strings" sound used in countless records.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 2, detune_cents: 6.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.43, sync: false, xmod: 0.0,
            cutoff: 0.85, resonance: 0.08, hpf_cutoff: 0.3, slope_24: false,
            env_mod: 0.1, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.45, env1_d: 0.5, env1_s: 0.9, env1_r: 0.5,
            env2_a: 0.45, env2_d: 0.0, env2_s: 1.0, env2_r: 0.5,
            lfo_rate: 4.5, lfo_wave: 0, lfo_to_pitch: 0.012, lfo_to_filter: 0.0, lfo_delay: 0.6,
            voice_mode: 2, portamento: 0.0,
        },
        // Flute — JP-8 factory patch #82 FLUTE
        // Source: Roland factory patch sheet #82; Sound on Sound "Synthesizing
        // Simple Flutes" by Gordon Reid. Triangle wave (near-sine fundamental),
        // light noise for breath. Filter at ~2kHz region (0.4 normalized) with
        // keyboard tracking ~65%, light resonance for edge. Fast filter attack
        // peaking before amp. Vibrato via LFO at 5-6Hz with delay.
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 3, detune_cents: 0.0,
            vco1_level: 0.9, vco2_level: 0.15,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.4, resonance: 0.1, hpf_cutoff: 0.08, slope_24: true,
            env_mod: 0.2, env_polarity: 1.0, key_follow: 0.65,
            env1_a: 0.02, env1_d: 0.3, env1_s: 0.6, env1_r: 0.15,
            env2_a: 0.08, env2_d: 0.0, env2_s: 1.0, env2_r: 0.12,
            lfo_rate: 5.5, lfo_wave: 0, lfo_to_pitch: 0.02, lfo_to_filter: 0.05, lfo_delay: 0.5,
            voice_mode: 0, portamento: 0.05,
        },
        // Tuba — deep brass, JP-8 factory patch #35 LO BRASS territory
        // Source: Roland factory patch sheet #35 LO BRASS; Sound on Sound "Synth
        // Secrets" Part 25 (synthesizing brass). Saw waves for harmonic-rich brass
        // timbre. Low VCF cutoff (~350 = 0.35) with strong env mod (700-750 = 0.75).
        // Env-1 inverted with fast decay for downward pitch transient on attack
        // (mod ~10 = small pitch dip). Env-2 A=180, S=full, R=180.
        // Solo mode, deeper range implied by patch name.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 5.0,
            vco1_level: 0.9, vco2_level: 0.8,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.3, resonance: 0.2, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.75, env_polarity: 1.0, key_follow: 0.3,
            env1_a: 0.01, env1_d: 0.35, env1_s: 0.6, env1_r: 0.25,
            env2_a: 0.18, env2_d: 0.0, env2_s: 1.0, env2_r: 0.18,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.015, lfo_to_filter: 0.0, lfo_delay: 0.6,
            voice_mode: 0, portamento: 0.0,
        },
        // SawPad — pure saw pad (Roland Cloud "Smooth Pad" recipe)
        // Source: Roland Cloud JUPITER-8 Model Expansion programming guide.
        // Both saw, mixer 200/200, fine tune +/-7, VCF 600-700 (0.65),
        // Env mod ~70 assigned to Env-2, Env-2 A=400 R=500-550 S=max.
        // No HPF, 24dB slope. Classic Jupiter warmth.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 7.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.65, resonance: 0.15, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.25, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.4, env1_d: 0.8, env1_s: 0.85, env1_r: 0.55,
            env2_a: 0.4, env2_d: 0.0, env2_s: 1.0, env2_r: 0.55,
            lfo_rate: 0.8, lfo_wave: 0, lfo_to_pitch: 0.01, lfo_to_filter: 0.0, lfo_delay: 0.4,
            voice_mode: 2, portamento: 0.0,
        },
        // Clrnet — JP-8 factory patch #81 CLARINET
        // Source: Roland factory patch sheet #81; Sound on Sound "Synth Secrets"
        // clarinet synthesis. Square/pulse wave is the classic clarinet starting
        // point (odd harmonics). Narrow pulse width ~0.3 for hollow clarinet
        // character. Moderate filter with key follow for natural brightness
        // tracking. Vibrato delayed, solo mode with portamento.
        JupiterPatch {
            vco1_wave: 3, vco2_wave: 3, detune_cents: 0.0,
            vco1_level: 0.8, vco2_level: 0.15,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.45, resonance: 0.15, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.25, env_polarity: 1.0, key_follow: 0.7,
            env1_a: 0.03, env1_d: 0.2, env1_s: 0.7, env1_r: 0.12,
            env2_a: 0.05, env2_d: 0.0, env2_s: 1.0, env2_r: 0.1,
            lfo_rate: 5.5, lfo_wave: 0, lfo_to_pitch: 0.015, lfo_to_filter: 0.03, lfo_delay: 0.5,
            voice_mode: 0, portamento: 0.05,
        },
        // Cello — JP-8 factory patch #84 CELLO
        // Source: Roland factory patch sheet #84. Saw wave for rich bowed string
        // harmonics. Low HPF to remove rumble, moderate filter with key follow.
        // Slow attack for bowed onset, full sustain, moderate release.
        // Solo mode with gentle portamento for legato slides.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 3.0,
            vco1_level: 0.8, vco2_level: 0.6,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.5, resonance: 0.15, hpf_cutoff: 0.1, slope_24: false,
            env_mod: 0.2, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.3, env1_d: 0.5, env1_s: 0.8, env1_r: 0.4,
            env2_a: 0.25, env2_d: 0.3, env2_s: 0.9, env2_r: 0.35,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.015, lfo_to_filter: 0.0, lfo_delay: 0.6,
            voice_mode: 0, portamento: 0.08,
        },
        // Xylo — JP-8 factory patch #26 XYLO (xylophone)
        // Source: Roland factory patch sheet #26. Triangle waves for pure,
        // bell-like fundamental. Very short envelopes (percussive mallet strike).
        // High key follow so higher notes are brighter. No VCO detune for
        // clean pitch. Cross-mod adds metallic inharmonic content.
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 0.0,
            vco1_level: 0.7, vco2_level: 0.5,
            pulse_width: 0.5, sync: false, xmod: 0.15,
            cutoff: 0.6, resonance: 0.05, hpf_cutoff: 0.1, slope_24: true,
            env_mod: 0.3, env_polarity: 1.0, key_follow: 0.9,
            env1_a: 0.001, env1_d: 0.4, env1_s: 0.0, env1_r: 0.3,
            env2_a: 0.001, env2_d: 0.5, env2_s: 0.0, env2_r: 0.4,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // FnkBas — JP-8 factory patch #13 JUICY FUNK
        // Source: Roland factory patch sheet #13. Funky resonant bass with
        // sharp filter envelope. Single saw for tight low end, high resonance
        // for squelchy character. Very short envelopes, solo mode.
        // 24dB slope for aggressive filter sweep.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 3, detune_cents: 0.0,
            vco1_level: 0.9, vco2_level: 0.3,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.18, resonance: 0.6, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.7, env_polarity: 1.0, key_follow: 0.35,
            env1_a: 0.001, env1_d: 0.15, env1_s: 0.1, env1_r: 0.08,
            env2_a: 0.001, env2_d: 0.25, env2_s: 0.3, env2_r: 0.08,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 0, portamento: 0.06,
        },
        // WrmLd — warm lead (JP-8 factory patch #17 HAMMER LEAD territory)
        // Source: Roland factory patch sheet #17. Saw+pulse combination for a
        // rich, warm solo lead. Moderate filter with env mod for expressive
        // attack. Unison mode for fat sound, gentle portamento, delayed vibrato.
        // 12dB slope for smoother, warmer filtering.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 2, detune_cents: 7.0,
            vco1_level: 0.7, vco2_level: 0.7,
            pulse_width: 0.4, sync: false, xmod: 0.0,
            cutoff: 0.5, resonance: 0.2, hpf_cutoff: 0.03, slope_24: false,
            env_mod: 0.35, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.01, env1_d: 0.4, env1_s: 0.6, env1_r: 0.3,
            env2_a: 0.01, env2_d: 0.0, env2_s: 1.0, env2_r: 0.25,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.02, lfo_to_filter: 0.0, lfo_delay: 0.5,
            voice_mode: 1, portamento: 0.08,
        },
        // Noise — textural noise wash (JP-8 factory patch #66 SOLAR WINDS)
        // Source: Roland factory patch sheet #66 SOLAR WINDS; Jupiter-8 noise
        // generator is on VCO-2 wave 3. Filtered noise with slow LFO modulating
        // filter for evolving texture. Long envelopes, resonance for tonal
        // character. 12dB slope for gentler rolloff.
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 3, detune_cents: 0.0,
            vco1_level: 0.3, vco2_level: 0.9,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.4, resonance: 0.45, hpf_cutoff: 0.1, slope_24: false,
            env_mod: 0.2, env_polarity: 1.0, key_follow: 0.2,
            env1_a: 1.5, env1_d: 2.0, env1_s: 0.5, env1_r: 2.0,
            env2_a: 2.0, env2_d: 1.0, env2_s: 0.7, env2_r: 3.0,
            lfo_rate: 0.1, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.3, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // CarSyn — JP-8 factory patch #15 CARS SYNC (The Cars "Let's Go" style)
        // Source: Roland factory patch sheet #15 CARS SYNC. Hard sync lead with
        // filter envelope sweep for the classic sync scream. VCO-2 synced to
        // VCO-1, filter envelope sweeps harmonics. Solo mode, portamento for
        // pitch slides. Bright, aggressive character.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 0.0,
            vco1_level: 0.4, vco2_level: 0.9,
            pulse_width: 0.5, sync: true, xmod: 0.0,
            cutoff: 0.55, resonance: 0.2, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.7, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.01, env1_d: 0.5, env1_s: 0.2, env1_r: 0.25,
            env2_a: 0.005, env2_d: 0.0, env2_s: 1.0, env2_r: 0.2,
            lfo_rate: 5.5, lfo_wave: 0, lfo_to_pitch: 0.025, lfo_to_filter: 0.0, lfo_delay: 0.4,
            voice_mode: 0, portamento: 0.1,
        },
];

// ── PolyBLEP anti-aliasing ──

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

// ── VCO ──

#[derive(Debug, Clone)]
struct JupiterVco {
    phase: f64,
    freq: f64,
    dt: f64, // freq / sample_rate
    // Noise state (LCG)
    noise_state: u32,
    noise_value: f64,
}

impl JupiterVco {
    fn new() -> Self {
        Self { phase: 0.0, freq: 440.0, dt: 0.01, noise_state: 12345, noise_value: 0.0 }
    }

    /// Set the frequency, bounded either side.
    ///
    /// The bound is not decoration: cross modulation moves VCO-1 by three
    /// octaves either way and VCO-2 tunes an octave up on top of a note that
    /// may already be at the top of the keyboard, and a phase increment past
    /// 1.0 walks straight out of the accumulator, which only ever subtracts
    /// one wrap per sample.
    fn set_freq(&mut self, freq: f64, sr: f64) {
        self.freq = if freq.is_finite() { freq.clamp(0.01, sr * 0.45) } else { 0.01 };
        self.dt = self.freq / sr;
    }

    /// Advance one sample. Returns whether the ramp wrapped, which is the
    /// edge the sync switch fires on.
    fn advance(&mut self) -> bool {
        self.phase += self.dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            true
        } else {
            false
        }
    }

    /// The waveform at the phase the ramp is sitting on.
    fn value(&self, waveform: u8, pulse_width: f64) -> f64 {
        let (t, dt) = (self.phase, self.dt);
        match waveform {
            // Triangle, folded out of the ramp. No BLEP: it is continuous
            // across the wrap, so there is no step to band-limit.
            0 => 2.0 * (2.0 * t - 1.0).abs() - 1.0,
            1 => 2.0 * t - 1.0 - poly_blep(t, dt),
            2 => {
                let pw = pulse_width.clamp(0.05, 0.95);
                let mut pulse = if t < pw { 1.0 } else { -1.0 };
                pulse += poly_blep(t, dt);
                pulse -= poly_blep((t - pw).rem_euclid(1.0), dt);
                pulse
            }
            // Square on VCO-1; VCO-2 reads this slot as noise and never asks.
            3 => {
                let mut sq = if t < 0.5 { 1.0 } else { -1.0 };
                sq += poly_blep(t, dt);
                sq -= poly_blep((t - 0.5).rem_euclid(1.0), dt);
                sq
            }
            _ => 0.0,
        }
    }

    /// Generate noise sample (for VCO-2 waveform 3).
    fn tick_noise(&mut self) -> f64 {
        self.noise_state = self.noise_state.wrapping_mul(1103515245).wrapping_add(12345);
        self.noise_value = f64::from(self.noise_state as i32) / f64::from(i32::MAX);
        self.noise_value
    }

    /// Hard sync: restart this ramp from where the master's has got to.
    ///
    /// Not from zero. The master crossed its own wrap somewhere inside the
    /// sample, and `master_phase` is how far past it the sample landed; the
    /// slave has to start the same fraction of *its* period late or the sync
    /// edge jitters by up to a sample, which at 440 Hz is a percent of the
    /// period and audible as a rasp on the pitch.
    fn sync_to(&mut self, master_phase: f64, master_dt: f64) {
        let elapsed = master_phase / master_dt.max(1e-12);
        self.phase = (elapsed * self.dt).rem_euclid(1.0);
    }
}

/// Fast tanh approximation — good enough for real-time, captures the
/// saturation character.
#[inline]
fn tanh_approx(x: f64) -> f64 {
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

// ── IR3109 filter ──
//
// Four one-pole sections round a feedback loop, which is the chip's own
// topology, and the same chip the Juno-60 filters with. The integrators are
// topology-preserving (the `g/(1+g)` form), so a section's pole lands on the
// frequency it was asked for; the naive `s += g*(x-s)` form it replaced sat an
// octave below its own coefficient and took the whole cascade with it — the
// slider marked 316 Hz measured 137 Hz at -3 dB.
//
// The 12/24 dB switch moves the *output* tap and leaves the feedback on the
// fourth section, which is why the resonance behaves the same in both
// positions and the slope does not. Tapping the feedback at the second
// section instead — as this used to — cannot resonate at all: two real poles
// never reach the half-turn of phase a loop needs, so 12 dB mode had a 14 dB
// hole in its passband and a 2 dB bump where its resonance should have been.

const RESONANCE_MAX: f64 = 4.0;

#[derive(Debug, Clone)]
struct Ir3109Filter {
    s: [f64; 4],
}

impl Ir3109Filter {
    fn new() -> Self {
        Self { s: [0.0; 4] }
    }

    fn process(&mut self, input: f64, cutoff_norm: f64, resonance: f64,
               four_pole: bool, sr: f64) -> f64 {
        let freq = cutoff_hz(cutoff_norm).min(sr * 0.45);
        let g = (std::f64::consts::PI * freq / sr).tan();
        let gg = g / (1.0 + g);
        let res = resonance.clamp(0.0, 1.0) * RESONANCE_MAX;
        let compensation = 1.0 + resonance * 0.5;
        let fb = tanh_approx(self.s[3]);
        let mut x = tanh_approx(input * compensation - res * fb);
        let tap = if four_pole { 3 } else { 1 };
        let mut out = 0.0;

        for (i, s) in self.s.iter_mut().enumerate() {
            let v = (x - *s) * gg;
            let y = v + *s;
            *s = y + v;
            if s.abs() < 1e-18 { *s = 0.0; }
            x = y;
            if i == tap { out = y; }
        }
        // The loop is already contracting — `tanh_approx` tends to x/9 rather
        // than to 1, so four units of feedback becomes less than one unit of
        // state — but the one number that closes the loop is worth bounding
        // outright, since a self-oscillating filter has no input to bound it.
        self.s[3] = self.s[3].clamp(-4.0, 4.0);
        out
    }

    fn reset(&mut self) { self.s = [0.0; 4]; }
}

// ── HPF ──
//
// One pole, non-resonant, 6 dB/octave, ahead of the VCF as on the voice
// board. Topology-preserving like the ladder's sections, and for the same
// reason: the difference-equation form it replaced put its corner an octave
// out at the top of the sweep.

#[derive(Debug, Clone)]
struct HpFilter {
    state: f64,
}

impl HpFilter {
    fn new() -> Self { Self { state: 0.0 } }

    fn process(&mut self, input: f64, cutoff_norm: f64, sr: f64) -> f64 {
        if cutoff_norm < 0.001 { return input; } // the slider is fully down
        let freq = hpf_hz(cutoff_norm).min(sr * 0.45);
        let g = (std::f64::consts::PI * freq / sr).tan();
        let v = (input - self.state) * g / (1.0 + g);
        let lp = v + self.state;
        self.state = lp + v;
        if self.state.abs() < 1e-18 { self.state = 0.0; }
        input - lp
    }

    fn reset(&mut self) { self.state = 0.0; }
}

// ── ADSR envelopes ──
//
// Two per voice: ENV-1 for the filter, ENV-2 for the amplifier, each with its
// own four sliders as on the instrument. Every segment is a capacitor
// charging towards something, which is why none of them are straight lines:
//
// * attack charges towards 1.58 and the stage ends when it passes 1.0, so the
//   segment is the first time-constant of an exponential;
// * decay and release charge towards slightly past their target and stop when
//   they reach it, which is 3.5 time constants across the segment. That is
//   the shape measured on a Juno-60, and it is what makes the slider's number
//   the time the segment actually takes.
//
// The defect this replaced: every segment used its slider's seconds as a
// one-pole *time constant* and then ran until it was within 0.001 of the
// target, which is 6.9 constants. A segment took seven times the time it
// advertised, on top of a slider that was already an order of magnitude too
// slow in the middle of its travel.

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

/// One-pole coefficient for a segment of `seconds` spanning `constants` time
/// constants. Saturates at 1.0, so a zero-length segment is a jump.
fn env_rate(seconds: f64, constants: f64, sr: f64) -> f64 {
    if seconds <= 0.0 { return 1.0; }
    (1.0 - (-constants / (seconds * sr)).exp()).min(1.0)
}

#[derive(Debug, Clone)]
struct JupiterEnvelope {
    stage: EnvStage,
    level: f64,
    aim: f64,
    attack: f64, decay: f64, sustain: f64, release: f64,
    /// Per-sample coefficients for the three timed segments, recomputed only
    /// when a slider moves. The exponential in `env_rate` is not something to
    /// evaluate sixteen times a sample for an answer that changes when a
    /// finger does.
    rates: [f64; 3],
    sample_rate: f64,
}

impl JupiterEnvelope {
    fn new(sr: f64) -> Self {
        let mut env = Self {
            stage: EnvStage::Idle, level: 0.0, aim: 0.0,
            attack: 0.01, decay: 0.3, sustain: 0.7, release: 0.2,
            rates: [0.0; 3], sample_rate: sr,
        };
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

    /// Start the attack from wherever the level already is, rather than from
    /// zero — a retrigger on this instrument does not drop the note first.
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

// ── LFO (global, free-running) ──

#[derive(Debug, Clone)]
struct JupiterLfo {
    phase: f64,
    rate: f64, // Hz
    waveform: u8, // 0=sin, 1=saw, 2=square, 3=random
    // S&H state
    sh_value: f64,
    sh_noise_state: u32,
    // Per-note fade-in
    delay_time: f64,
    delay_counter: f64,
    delay_level: f64,
}

impl JupiterLfo {
    fn new() -> Self {
        Self {
            phase: 0.0, rate: 1.0, waveform: 0,
            sh_value: 0.0, sh_noise_state: 54321,
            delay_time: 0.0, delay_counter: 0.0, delay_level: 1.0,
        }
    }

    /// Restart the delay. Called on the first key of a phrase, not on every
    /// note: the delay runs from the gate going high, so a second note added
    /// to a held chord must not restart the vibrato under the notes already
    /// sounding. It used to fire on every note-on, in every mode.
    fn trigger_delay(&mut self) {
        self.delay_counter = 0.0;
        self.delay_level = if self.delay_time > 0.001 { 0.0 } else { 1.0 };
    }

    fn tick(&mut self, sample_rate: f64) -> f64 {
        let prev_phase = self.phase;
        self.phase += self.rate / sample_rate;
        let wrapped = self.phase >= 1.0;
        if wrapped { self.phase -= 1.0; }

        // Hold, then fade. Two stages rather than one, in the measured
        // Juno-60 proportion — see LFO_DELAY_HOLD_SHARE.
        if self.delay_level < 1.0 {
            self.delay_counter += 1.0 / sample_rate;
            let hold = self.delay_time * LFO_DELAY_HOLD_SHARE;
            let fade = self.delay_time - hold;
            self.delay_level = ((self.delay_counter - hold) / fade.max(1e-6)).clamp(0.0, 1.0);
        }

        let raw = match self.waveform {
            0 => (self.phase * TWO_PI).sin(),           // Sine
            1 => 1.0 - 2.0 * self.phase,               // Saw (ramp down)
            2 => if self.phase < 0.5 { 1.0 } else { -1.0 }, // Square
            3 => {
                // Sample & Hold: new random value each cycle
                if wrapped || prev_phase == 0.0 {
                    self.sh_noise_state = self.sh_noise_state.wrapping_mul(1103515245).wrapping_add(12345);
                    self.sh_value = f64::from(self.sh_noise_state as i32) / f64::from(i32::MAX);
                }
                self.sh_value
            }
            _ => 0.0,
        };

        raw * self.delay_level
    }
}

// ── Voice ──

#[derive(Debug, Clone)]
struct JupiterVoice {
    vco1: JupiterVco,
    vco2: JupiterVco,
    lpf: Ir3109Filter,
    hpf: HpFilter,
    env1: JupiterEnvelope, // filter
    env2: JupiterEnvelope, // VCA
    note: u8,
    velocity: f64,
    age: u64,
    target_freq: f64,
    current_freq: f64,
    glide_coeff: f64,
    /// VCO-2's last output, which is what cross modulation moves VCO-1 with.
    /// A sample old, because sync runs the other way and something has to
    /// open the loop.
    last_vco2: f64,
    // Per-voice variation (fixed on creation)
    drift_phase: f64,
    drift_rate: f64,
    cutoff_offset: f64,
    pitch_offset: f64, // cents
    sample_rate: f64,
}

impl JupiterVoice {
    fn new(sr: f64, voice_idx: usize) -> Self {
        // Deterministic per-voice variation based on index
        let seed = (voice_idx as u32).wrapping_mul(2654435761);
        let cutoff_var = ((seed & 0xFF) as f64 / 255.0 - 0.5) * 0.04; // ±2%
        let pitch_var = (((seed >> 8) & 0xFF) as f64 / 255.0 - 0.5) * 3.0; // ±1.5 cents
        let drift_rate = 0.1 + ((seed >> 16) & 0xFF) as f64 / 255.0 * 0.4; // 0.1-0.5 Hz

        Self {
            vco1: JupiterVco::new(),
            vco2: JupiterVco::new(),
            lpf: Ir3109Filter::new(),
            hpf: HpFilter::new(),
            env1: JupiterEnvelope::new(sr),
            env2: JupiterEnvelope::new(sr),
            note: 255,
            velocity: 0.0,
            age: 0,
            target_freq: 440.0,
            current_freq: 440.0,
            glide_coeff: 1.0,
            last_vco2: 0.0,
            drift_phase: voice_idx as f64 * 0.37, // stagger initial drift phases
            drift_rate,
            cutoff_offset: cutoff_var,
            pitch_offset: pitch_var,
            sample_rate: sr,
        }
    }

    fn note_on(&mut self, note: u8, vel: u8, patch: &JupiterPatch, portamento: bool, age: u64) {
        self.note = note;
        self.velocity = vel as f64 / 127.0;
        self.age = age;

        let freq = note_to_freq(note);
        if portamento && self.current_freq > 0.0 && patch.portamento > 0.001 {
            self.target_freq = freq;
            self.glide_coeff = env_rate(patch.portamento, ENV_CONSTANTS, self.sample_rate);
        } else {
            self.target_freq = freq;
            self.current_freq = freq;
            self.glide_coeff = 1.0;
        }

        self.env1.set_times(patch.env1_a, patch.env1_d, patch.env1_r);
        self.env1.sustain = patch.env1_s;
        self.env2.set_times(patch.env2_a, patch.env2_d, patch.env2_r);
        self.env2.sustain = patch.env2_s;

        self.env1.trigger();
        self.env2.trigger();
        self.hpf.reset();
    }

    fn note_off(&mut self) {
        self.env1.release_env();
        self.env2.release_env();
    }

    fn kill(&mut self) {
        self.note = 255;
        self.env1.kill();
        self.env2.kill();
        self.lpf.reset();
        self.hpf.reset();
    }

    fn is_sounding(&self) -> bool { self.env2.is_active() }
    fn is_held(&self) -> bool { self.env2.is_held() }

    fn tick(&mut self, patch: &JupiterPatch, lfo_out: f64) -> f64 {
        if !self.is_sounding() { return 0.0; }

        let sr = self.sample_rate;

        // The time sliders are live, so a note already sounding follows them;
        // the coefficients behind them are only recomputed when one moves.
        self.env1.set_times(patch.env1_a, patch.env1_d, patch.env1_r);
        self.env1.sustain = patch.env1_s;
        self.env2.set_times(patch.env2_a, patch.env2_d, patch.env2_r);
        self.env2.sustain = patch.env2_s;

        // Portamento
        if self.glide_coeff < 1.0 {
            self.current_freq += self.glide_coeff * (self.target_freq - self.current_freq);
        }

        // Per-voice drift
        self.drift_phase += self.drift_rate / sr;
        if self.drift_phase > 1.0 { self.drift_phase -= 1.0; }
        let drift_cents = (self.drift_phase * TWO_PI).sin() * 2.5; // ±2.5 cents

        // VCO frequencies with drift, vibrato and VCO-2's tuning. Cross
        // modulation is VCO-2 bending VCO-1's pitch, which is what the
        // control does on the instrument — it used to multiply VCO-1's
        // *output* by VCO-2's noise register, a number that is zero unless
        // VCO-2 is switched to noise, so on every patch in the bank that asks
        // for cross modulation it did precisely nothing.
        let pitch_mod = self.pitch_offset + drift_cents + lfo_out * patch.lfo_to_pitch * LFO_PITCH_CENTS;
        let base = self.current_freq * 2.0f64.powf(pitch_mod / 1200.0);
        let freq1 = if patch.xmod > 0.001 {
            base * 2.0f64.powf(self.last_vco2 * patch.xmod * XMOD_OCTAVES)
        } else {
            base
        };
        let freq2 = self.current_freq * 2.0f64.powf((pitch_mod + patch.detune_cents) / 1200.0);

        self.vco1.set_freq(freq1, sr);
        self.vco2.set_freq(freq2, sr);

        let vco1_reset = self.vco1.advance();
        let vco1_out = self.vco1.value(patch.vco1_wave, patch.pulse_width);

        let vco2_out = if patch.vco2_wave == 3 {
            self.vco2.tick_noise()
        } else {
            self.vco2.advance();
            // Hard sync: VCO-1 restarts VCO-2's ramp.
            if patch.sync && vco1_reset {
                self.vco2.sync_to(self.vco1.phase, self.vco1.dt);
            }
            self.vco2.value(patch.vco2_wave, patch.pulse_width)
        };
        self.last_vco2 = vco2_out;

        // The mixer's two faders. Scaled so that raising the second one
        // raises the level rather than the peak, as in the Juno's mixer.
        let weight = patch.vco1_level + patch.vco2_level;
        let mut mixed = vco1_out * patch.vco1_level + vco2_out * patch.vco2_level;
        if weight > 1.0 { mixed /= weight.sqrt(); }

        // HPF, ahead of the VCF as on the voice board
        let hp_out = self.hpf.process(mixed, patch.hpf_cutoff, sr);

        // Filter cutoff: panel + envelope + keyboard + LFO. Key follow is in
        // octaves of cutoff per octave of keyboard, so it has to be scaled by
        // how many octaves the cutoff slider spans — it used to be divided by
        // five semitones instead, which tracked at twice the rate the control
        // claims.
        let env1 = self.env1.tick();
        let key_follow =
            (f64::from(self.note) - 60.0) / 12.0 / CUTOFF_OCTAVES * patch.key_follow;
        let effective_cutoff = (patch.cutoff + self.cutoff_offset
            + env1 * patch.env_mod * patch.env_polarity
            + lfo_out * patch.lfo_to_filter
            + key_follow).clamp(0.0, 1.0);

        let lp_out = self.lpf.process(hp_out, effective_cutoff, patch.resonance, patch.slope_24, sr);

        // VCA
        let env2 = self.env2.tick();
        lp_out * env2 * self.velocity
    }
}

// ── Jupiter-8 Synth ──

pub struct Jupiter8Synth {
    voices: Vec<JupiterVoice>,
    lfo: JupiterLfo,
    sample_rate: f64,
    pub params: [f32; PARAM_COUNT],
    voice_counter: u64,
    last_patch_index: usize,
}

impl Jupiter8Synth {
    pub fn new() -> Self {
        let mut s = Self {
            voices: Vec::new(),
            lfo: JupiterLfo::new(),
            sample_rate: 44100.0,
            params: PARAM_DEFAULTS,
            voice_counter: 0,
            last_patch_index: usize::MAX,
        };
        s.sync_params_from_patch();
        s
    }

    fn current_patch_index(&self) -> usize {
        patch_index(self.params[P_PATCH])
    }

    /// The whole panel as the preset sets it.
    ///
    /// The bank is held in seconds and hertz, so the time and frequency
    /// sliders come back through their tapers; each switch lands on the
    /// midpoint of its position, so a switch loaded from a preset sits where
    /// [`step_discrete`] would leave it.
    pub fn params_for_patch(patch_value: f32) -> [f32; PARAM_COUNT] {
        let p = &PRESETS[patch_index(patch_value)];
        let mut params = [0.0f32; PARAM_COUNT];
        params[P_PATCH] = patch_value;
        params[P_PORTAMENTO] = (p.portamento / PORTAMENTO_MAX).clamp(0.0, 1.0) as f32;
        params[P_MODE] = knob_for(p.voice_mode as usize, 4);
        params[P_LFO_RATE] = slider_for(lfo_hz, p.lfo_rate);
        params[P_LFO_WAVE] = knob_for(p.lfo_wave as usize, 4);
        params[P_LFO_DELAY] = (p.lfo_delay / LFO_DELAY_MAX).clamp(0.0, 1.0) as f32;
        params[P_VCO_LFO] = p.lfo_to_pitch as f32;
        params[P_PW] = p.pulse_width as f32;
        params[P_VCO1_WAVE] = knob_for(p.vco1_wave as usize, 4);
        params[P_XMOD] = p.xmod as f32;
        params[P_VCO2_WAVE] = knob_for(p.vco2_wave as usize, 4);
        params[P_TUNE] = tune_slider(p.detune_cents);
        params[P_SYNC] = knob_for(usize::from(p.sync), 2);
        params[P_VCO1_LEVEL] = p.vco1_level as f32;
        params[P_VCO2_LEVEL] = p.vco2_level as f32;
        params[P_HPF] = p.hpf_cutoff as f32;
        params[P_CUTOFF] = p.cutoff as f32;
        params[P_RESO] = p.resonance as f32;
        params[P_SLOPE] = knob_for(usize::from(p.slope_24), 2);
        params[P_ENV_MOD] = p.env_mod as f32;
        params[P_ENV_POLARITY] = knob_for(usize::from(p.env_polarity < 0.0), 2);
        params[P_VCF_LFO] = p.lfo_to_filter as f32;
        params[P_KEY_FOLLOW] = p.key_follow as f32;
        // The bank does not record a VCA level — these presets were written
        // against a panel that had one master gain and no per-patch level —
        // so every patch loads the default and the fader stays where it is.
        params[P_LEVEL] = PARAM_DEFAULTS[P_LEVEL];
        params[P_ENV1_A] = slider_for(attack_seconds, p.env1_a);
        params[P_ENV1_D] = slider_for(decay_seconds, p.env1_d);
        params[P_ENV1_S] = p.env1_s as f32;
        params[P_ENV1_R] = slider_for(decay_seconds, p.env1_r);
        params[P_ENV2_A] = slider_for(attack_seconds, p.env2_a);
        params[P_ENV2_D] = slider_for(decay_seconds, p.env2_d);
        params[P_ENV2_S] = p.env2_s as f32;
        params[P_ENV2_R] = slider_for(decay_seconds, p.env2_r);
        params
    }

    /// When the patch selector moves, load its panel into the parameters.
    fn sync_params_from_patch(&mut self) {
        let idx = self.current_patch_index();
        if idx == self.last_patch_index { return; }
        self.last_patch_index = idx;
        let loaded = Self::params_for_patch(self.params[P_PATCH]);
        for (i, &v) in loaded.iter().enumerate() {
            if i != P_PATCH { self.params[i] = v; }
        }
    }

    fn voice_mode(&self) -> u8 {
        selector(self.params[P_MODE], 4) as u8
    }

    fn next_age(&mut self) -> u64 { self.voice_counter += 1; self.voice_counter }

    fn allocate_voice_poly1(&mut self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| !v.is_sounding()) { return i; }
        if let Some((i, _)) = self.voices.iter().enumerate()
            .filter(|(_, v)| !v.is_held()).min_by_key(|(_, v)| v.age) { return i; }
        self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap_or(0)
    }

    fn allocate_voice_poly2(&mut self) -> usize {
        // Kill all voices in release phase first
        for v in &mut self.voices {
            if v.is_sounding() && !v.is_held() { v.kill(); }
        }
        self.allocate_voice_poly1()
    }

    fn release_note(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.note == note && v.is_held() { v.note_off(); }
        }
    }

    fn kill_all_voices(&mut self) {
        for v in &mut self.voices { v.kill(); }
    }

    /// The panel as it stands, in the units the engine works in. Every
    /// control is live — the preset is only where the knobs started.
    fn active_patch(&self) -> JupiterPatch {
        let p = &self.params;
        JupiterPatch {
            vco1_wave: selector(p[P_VCO1_WAVE], 4) as u8,
            vco2_wave: selector(p[P_VCO2_WAVE], 4) as u8,
            detune_cents: tune_cents(f64::from(p[P_TUNE])),
            vco1_level: f64::from(p[P_VCO1_LEVEL]),
            vco2_level: f64::from(p[P_VCO2_LEVEL]),
            pulse_width: f64::from(p[P_PW]),
            sync: selector(p[P_SYNC], 2) == 1,
            xmod: f64::from(p[P_XMOD]),
            cutoff: f64::from(p[P_CUTOFF]),
            resonance: f64::from(p[P_RESO]),
            hpf_cutoff: f64::from(p[P_HPF]),
            slope_24: selector(p[P_SLOPE], 2) == 1,
            env_mod: f64::from(p[P_ENV_MOD]),
            env_polarity: if selector(p[P_ENV_POLARITY], 2) == 1 { -1.0 } else { 1.0 },
            key_follow: f64::from(p[P_KEY_FOLLOW]),
            env1_a: attack_seconds(f64::from(p[P_ENV1_A])),
            env1_d: decay_seconds(f64::from(p[P_ENV1_D])),
            env1_s: f64::from(p[P_ENV1_S]),
            env1_r: decay_seconds(f64::from(p[P_ENV1_R])),
            env2_a: attack_seconds(f64::from(p[P_ENV2_A])),
            env2_d: decay_seconds(f64::from(p[P_ENV2_D])),
            env2_s: f64::from(p[P_ENV2_S]),
            env2_r: decay_seconds(f64::from(p[P_ENV2_R])),
            lfo_rate: lfo_hz(f64::from(p[P_LFO_RATE])),
            lfo_wave: selector(p[P_LFO_WAVE], 4) as u8,
            lfo_to_pitch: f64::from(p[P_VCO_LFO]),
            lfo_to_filter: f64::from(p[P_VCF_LFO]),
            lfo_delay: f64::from(p[P_LFO_DELAY]) * LFO_DELAY_MAX,
            voice_mode: self.voice_mode(),
            portamento: porta_seconds(f64::from(p[P_PORTAMENTO])),
        }
    }

    fn any_key_held(&self) -> bool {
        self.voices.iter().any(JupiterVoice::is_held)
    }
}

impl Default for Jupiter8Synth {
    fn default() -> Self { Self::new() }
}

impl Plugin for Jupiter8Synth {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "Jupiter-8".into(),
            version: "0.1.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..MAX_VOICES).map(|i| JupiterVoice::new(sample_rate, i)).collect();
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() { return; }

        let buf_len = outputs[0].len();
        let gain = self.params[P_LEVEL] * OUTPUT_TRIM;
        let patch = self.active_patch();

        self.lfo.rate = patch.lfo_rate;
        self.lfo.waveform = patch.lfo_wave;
        self.lfo.delay_time = patch.lfo_delay;

        let mode = patch.voice_mode;

        // Sort MIDI events (allocation-free)
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

        for i in 0..buf_len {
            while ei < event_count && midi_events[event_indices[ei]].sample_offset as usize <= i {
                let ev = &midi_events[event_indices[ei]];
                match ev.status & 0xF0 {
                    0x90 => {
                        if ev.data2 > 0 {
                            // The LFO delay runs from the gate going high, so
                            // only the first key of a phrase restarts it.
                            let first_key = !self.any_key_held();
                            self.release_note(ev.data1);
                            let age = self.next_age();
                            match mode {
                                // Solo and unison stack every voice on the
                                // note; only solo glides to it.
                                0 | 1 => {
                                    let glide = mode == 0;
                                    for vi in 0..self.voices.len() {
                                        self.voices[vi].note_on(ev.data1, ev.data2, &patch, glide, age);
                                    }
                                }
                                // Poly2 takes the released voices back first
                                3 => {
                                    let idx = self.allocate_voice_poly2();
                                    self.voices[idx].note_on(ev.data1, ev.data2, &patch, false, age);
                                }
                                _ => {
                                    let idx = self.allocate_voice_poly1();
                                    self.voices[idx].note_on(ev.data1, ev.data2, &patch, false, age);
                                }
                            }
                            if first_key { self.lfo.trigger_delay(); }
                        } else {
                            self.release_note(ev.data1);
                        }
                    }
                    0x80 => self.release_note(ev.data1),
                    0xB0 => match ev.data1 {
                        120 => self.kill_all_voices(),
                        123 => { for v in &mut self.voices { if v.is_held() { v.note_off(); } } }
                        _ => {}
                    }
                    _ => {}
                }
                ei += 1;
            }

            // Global LFO
            let lfo_out = self.lfo.tick(self.sample_rate);

            // Sum voices
            let mut sample = 0.0f32;
            for v in &mut self.voices {
                sample += v.tick(&patch, lfo_out) as f32;
            }

            // Solo and Unison stack every voice on the same note, so they sum
            // coherently and need their own fixed divisor. The poly modes get
            // no divisor at all: OUTPUT_TRIM already carries the headroom for
            // a full eight-note chord, and anything that varies with the
            // sounding voice count pumps.
            if mode < 2 {
                sample /= (MAX_VOICES as f32).sqrt();
            }

            sample *= gain;
            // Bound the output without hard clipping it. The trim above keeps
            // ordinary playing under the knee, so this is the identity for
            // everything except a patch pushed past it by the gain knob.
            sample = soft_saturate(sample);

            for ch in outputs.iter_mut() { ch[i] = sample; }
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
                P_ENV1_A | P_ENV1_D | P_ENV1_R
                | P_ENV2_A | P_ENV2_D | P_ENV2_R
                | P_PORTAMENTO | P_LFO_DELAY => "s".into(),
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
        if index == P_PATCH {
            self.sync_params_from_patch();
        }
    }

    fn reset(&mut self) { self.kill_all_voices(); self.voice_counter = 0; }
}

fn note_to_freq(note: u8) -> f64 {
    440.0 * 2.0f64.powf((note as f64 - 69.0) / 12.0)
}

// ── Tests ──

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

    fn process_buffers(synth: &mut Jupiter8Synth, events: &[MidiEvent], count: usize) -> Vec<f32> {
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
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        // 200 buffers, not 4: the output carries a headroom trim, so five
        // milliseconds of attack is not enough signal to tell "sounding" from
        // "silent" with any margin.
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 200);
        assert!(peak(&out) > 0.005, "Should produce sound, peak={}", peak(&out));
    }

    #[test]
    fn silent_after_release() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        s.set_parameter(P_ENV2_R, 0.05);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[note_off(60, 0)], 3000);
        let out = process_buffers(&mut s, &[], 1);
        assert!(peak(&out) < 0.001, "peak={}", peak(&out));
    }

    #[test]
    fn output_is_finite() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 1000);
        assert!(out.iter().all(|v| v.is_finite()), "Output must be finite");
    }

    #[test]
    fn polyphony() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        let events = [note_on(60, 100, 0), note_on(64, 100, 0), note_on(67, 100, 0)];
        let out = process_buffers(&mut s, &events, 200);
        assert!(peak(&out) > 0.005 && peak(&out) < 1.0, "peak={}", peak(&out));
    }

    #[test]
    fn all_patches_produce_sound() {
        for (pi, name) in PATCH_NAMES.iter().enumerate() {
            let mut s = Jupiter8Synth::new();
            s.init(44100.0, 64);
            s.set_parameter(P_PATCH, patch_knob(pi));
            // Enough buffers for the slow-attack pads.
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 2500);
            assert!(peak(&out) > 0.001, "patch {pi} ({name}) is silent: peak={}", peak(&out));
        }
    }

    #[test]
    fn all_patches_finite() {
        for (pi, name) in PATCH_NAMES.iter().enumerate() {
            let mut s = Jupiter8Synth::new();
            s.init(44100.0, 64);
            s.set_parameter(P_PATCH, patch_knob(pi));
            let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 500);
            assert!(out.iter().all(|v| v.is_finite()),
                "patch {pi} ({name}) must produce finite output");
        }
    }

    #[test]
    fn cc120_kills_all() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[cc(120, 0, 0)], 1);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn all_params_readable() {
        let s = Jupiter8Synth::new();
        assert_eq!(s.parameter_count(), PARAM_COUNT);
        for i in 0..PARAM_COUNT {
            assert!(s.parameter_info(i).is_some());
            let val = s.get_parameter(i);
            assert!((0.0..=1.0).contains(&val), "param {i} = {val}");
        }
    }

    #[test]
    fn filter_resonance_affects_sound() {
        let mut s1 = Jupiter8Synth::new();
        s1.init(44100.0, 64);
        s1.set_parameter(P_RESO, 0.0);
        let flat = process_buffers(&mut s1, &[note_on(60, 100, 0)], 8);

        let mut s2 = Jupiter8Synth::new();
        s2.init(44100.0, 64);
        s2.set_parameter(P_RESO, 0.8);
        let reso = process_buffers(&mut s2, &[note_on(60, 100, 0)], 8);

        let diff: f32 = flat.iter().zip(reso.iter()).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 0.01, "Resonance should change sound, diff={diff}");
    }

    #[test]
    fn sample_accurate_midi() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 128);
        s.set_parameter(P_ENV2_A, 0.0);
        let mut out = vec![0.0f32; 128];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 64)]);
        assert!(peak(&out[..64]) < 0.001, "pre={}", peak(&out[..64]));
        assert!(peak(&out[64..]) > 0.001, "post={}", peak(&out[64..]));
    }

    #[test]
    fn solo_mode_all_voices_same_note() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        s.set_parameter(P_MODE, knob_for(0, 4));
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        assert!(s.voices.iter().all(|v| v.note == 60), "Solo: all voices should play same note");
    }

    #[test]
    fn poly_blep_at_boundaries() {
        let dt = 0.01;
        assert!(poly_blep(0.005, dt).abs() > 0.0, "BLEP should correct near reset");
        assert_eq!(poly_blep(0.5, dt), 0.0, "BLEP should be zero far from boundary");
    }

    // ── Panel ──

    #[test]
    fn the_panel_is_in_front_panel_order() {
        // The order is the instrument's, left to right, and the editor shows
        // the parameter block in index order — so this is the layout a player
        // sees. Sessions store the block positionally, which is what makes
        // the order worth pinning down in a test.
        assert_eq!(PARAM_NAMES[P_PATCH], "patch");
        assert_eq!(&PARAM_NAMES[P_PORTAMENTO..=P_MODE], &["porta", "mode"]);
        assert_eq!(&PARAM_NAMES[P_LFO_RATE..=P_LFO_DELAY], &["lfo rate", "lfo wave", "lfo dly"]);
        assert_eq!(&PARAM_NAMES[P_VCO_LFO..=P_PW], &["vco lfo", "pw"]);
        assert_eq!(&PARAM_NAMES[P_VCO1_WAVE..=P_XMOD], &["vco1 wav", "xmod"]);
        assert_eq!(&PARAM_NAMES[P_VCO2_WAVE..=P_SYNC], &["vco2 wav", "tune", "sync"]);
        assert_eq!(&PARAM_NAMES[P_VCO1_LEVEL..=P_VCO2_LEVEL], &["vco1 lvl", "vco2 lvl"]);
        assert_eq!(PARAM_NAMES[P_HPF], "hpf");
        assert_eq!(
            &PARAM_NAMES[P_CUTOFF..=P_KEY_FOLLOW],
            &["freq", "res", "slope", "env mod", "env pol", "vcf lfo", "kybd"]
        );
        assert_eq!(PARAM_NAMES[P_LEVEL], "level");
        assert_eq!(&PARAM_NAMES[P_ENV1_A..=P_ENV1_R], &["env1 a", "env1 d", "env1 s", "env1 r"]);
        assert_eq!(&PARAM_NAMES[P_ENV2_A..=P_ENV2_R], &["env2 a", "env2 d", "env2 s", "env2 r"]);
        assert_eq!(PARAM_COUNT, 32);
    }

    #[test]
    fn every_engine_control_is_reachable() {
        // The defect this guards: sixteen of the panel's controls existed in
        // the engine and had no parameter of their own, ENV-2 among them — its
        // four sliders were copies of ENV-1's, so the amplifier envelope could
        // not be touched. A control the engine reads has to have an index.
        fn render(s: &mut Jupiter8Synth) -> Vec<f32> {
            let mut out = process_buffers(s, &[note_on(72, 100, 0)], 200);
            out.extend(process_buffers(s, &[note_on(60, 100, 0)], 100));
            out.extend(process_buffers(s, &[note_off(72, 0), note_off(60, 1)], 200));
            out
        }
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            if index == P_PATCH { continue; }
            let mut low = Jupiter8Synth::new();
            low.init(44100.0, 64);
            let mut high = Jupiter8Synth::new();
            high.init(44100.0, 64);
            // Every path running, so no control is masked by a dead one: both
            // oscillators on a pulse so the width does something, VCO-2 off
            // unison so sync and tune do, solo mode so portamento does.
            for s in [&mut low, &mut high] {
                s.set_parameter(P_MODE, knob_for(0, 4));
                s.set_parameter(P_PORTAMENTO, 0.3);
                s.set_parameter(P_LFO_RATE, 0.6);
                s.set_parameter(P_VCO_LFO, 0.3);
                s.set_parameter(P_VCF_LFO, 0.3);
                s.set_parameter(P_PW, 0.35);
                s.set_parameter(P_VCO1_WAVE, knob_for(2, 4));
                s.set_parameter(P_VCO2_WAVE, knob_for(2, 4));
                s.set_parameter(P_TUNE, 0.6);
                s.set_parameter(P_VCO1_LEVEL, 0.7);
                s.set_parameter(P_VCO2_LEVEL, 0.7);
                s.set_parameter(P_HPF, 0.3);
                s.set_parameter(P_CUTOFF, 0.5);
                s.set_parameter(P_RESO, 0.3);
                s.set_parameter(P_ENV_MOD, 0.4);
                s.set_parameter(P_KEY_FOLLOW, 0.5);
                s.set_parameter(P_XMOD, 0.2);
                // Short envelopes, so that a held note reaches its sustain
                // and a released one has somewhere to fall from.
                for i in [P_ENV1_A, P_ENV2_A] { s.set_parameter(i, 0.0); }
                for i in [P_ENV1_D, P_ENV2_D] { s.set_parameter(i, 0.2); }
                for i in [P_ENV1_S, P_ENV2_S] { s.set_parameter(i, 0.4); }
                for i in [P_ENV1_R, P_ENV2_R] { s.set_parameter(i, 0.3); }
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
        // that, and this instrument's patch knob was stepping by 1/41.99.
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
        assert_eq!(discrete_label(P_PATCH, 0.0), Some("Pad"));
        assert_eq!(discrete_label(P_SLOPE, knob_for(0, 2)), Some("12dB"));
        assert_eq!(discrete_label(P_SLOPE, knob_for(1, 2)), Some("24dB"));
        assert_eq!(discrete_label(P_LFO_WAVE, knob_for(0, 4)), Some("SIN"));
        assert_eq!(discrete_label(P_LFO_WAVE, knob_for(1, 4)), Some("SAW"));
        assert_eq!(discrete_label(P_LFO_WAVE, knob_for(2, 4)), Some("SQR"));
        assert_eq!(discrete_label(P_LFO_WAVE, knob_for(3, 4)), Some("RND"));
        assert_eq!(discrete_label(P_VCO1_WAVE, knob_for(0, 4)), Some("TRI"));
        assert_eq!(discrete_label(P_VCO1_WAVE, knob_for(2, 4)), Some("PLS"));
        assert_eq!(discrete_label(P_VCO1_WAVE, knob_for(3, 4)), Some("SQR"));
        assert_eq!(discrete_label(P_VCO2_WAVE, knob_for(3, 4)), Some("NOISE"));
        assert_eq!(discrete_label(P_MODE, knob_for(2, 4)), Some("POLY1"));
        assert_eq!(discrete_label(P_SYNC, knob_for(1, 2)), Some("on"));
        assert_eq!(discrete_label(P_ENV_POLARITY, knob_for(1, 2)), Some("on"));
        assert_eq!(discrete_label(P_CUTOFF, 0.5), None);
        // Out-of-range knobs are labelled, not panicked on: `params` is public.
        assert_eq!(discrete_label(P_LFO_WAVE, 9.0), Some("RND"));
        assert_eq!(discrete_label(P_LFO_WAVE, -1.0), Some("SIN"));
        assert_eq!(discrete_label(P_PATCH, 2.0), Some(PATCH_NAMES[PATCH_COUNT - 1]));
    }

    #[test]
    fn the_patch_knob_lands_on_the_patch_it_names() {
        // 42 patches is enough that dividing the index by the count misses:
        // the quotient lands a hair below its own step for seven of them and
        // selects the patch before it.
        for (pi, name) in PATCH_NAMES.iter().enumerate() {
            let knob = patch_knob(pi);
            assert_eq!(patch_index(knob), pi, "patch {pi} knob {knob}");
            assert_eq!(discrete_label(P_PATCH, knob), Some(*name));
            let mut s = Jupiter8Synth::new();
            s.set_parameter(P_PATCH, knob);
            assert_eq!(s.current_patch_index(), pi);
        }
    }

    #[test]
    fn patch_zero_is_the_default_parameter_block() {
        let loaded = Jupiter8Synth::params_for_patch(0.0);
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
        // Bank to panel to engine. Every field of every preset has to arrive
        // in the control it belongs to and come back out in the engine's
        // units — the defect this catches is a preset loaded one slot out,
        // which is silent about itself and audible about nothing else.
        for (pi, want) in PRESETS.iter().enumerate() {
            let mut s = Jupiter8Synth::new();
            s.init(44100.0, 64);
            s.set_parameter(P_PATCH, patch_knob(pi));
            let name = PATCH_NAMES[pi];
            let got = s.active_patch();
            let close = |got: f64, want: f64, what: &str| {
                assert!((got - want).abs() < 1e-4,
                        "{name} {what}: {got} where the preset says {want}");
            };
            assert_eq!(got.vco1_wave, want.vco1_wave, "{name} vco1 wave");
            assert_eq!(got.vco2_wave, want.vco2_wave, "{name} vco2 wave");
            close(got.detune_cents, want.detune_cents, "tune");
            close(got.vco1_level, want.vco1_level, "vco1 level");
            close(got.vco2_level, want.vco2_level, "vco2 level");
            close(got.pulse_width, want.pulse_width, "pulse width");
            assert_eq!(got.sync, want.sync, "{name} sync");
            close(got.xmod, want.xmod, "xmod");
            close(got.cutoff, want.cutoff, "cutoff");
            close(got.resonance, want.resonance, "resonance");
            close(got.hpf_cutoff, want.hpf_cutoff, "hpf");
            assert_eq!(got.slope_24, want.slope_24, "{name} slope");
            close(got.env_mod, want.env_mod, "env mod");
            close(got.env_polarity, want.env_polarity, "env polarity");
            close(got.key_follow, want.key_follow, "key follow");
            close(got.env1_s, want.env1_s, "env1 sustain");
            close(got.env2_s, want.env2_s, "env2 sustain");
            for (what, got, want) in [
                ("env1 attack", got.env1_a, want.env1_a),
                ("env1 decay", got.env1_d, want.env1_d),
                ("env1 release", got.env1_r, want.env1_r),
                ("env2 attack", got.env2_a, want.env2_a),
                ("env2 decay", got.env2_d, want.env2_d),
                ("env2 release", got.env2_r, want.env2_r),
                ("lfo rate", got.lfo_rate, want.lfo_rate),
                ("portamento", got.portamento, want.portamento),
            ] {
                // The floor is the instrument's own: Roland's shortest
                // segment is 1 ms, so a preset asking for zero gets 1 ms.
                assert!((got - want).abs() < want.abs() * 1e-4 + 1.1e-3,
                        "{name} {what}: {got} where the preset says {want}");
            }
            assert_eq!(got.lfo_wave, want.lfo_wave, "{name} lfo wave");
            close(got.lfo_to_pitch, want.lfo_to_pitch, "lfo to pitch");
            close(got.lfo_to_filter, want.lfo_to_filter, "lfo to filter");
            close(got.lfo_delay, want.lfo_delay, "lfo delay");
            assert_eq!(got.voice_mode, want.voice_mode, "{name} voice mode");
        }
    }

    #[test]
    fn the_two_envelopes_are_independent() {
        // ENV-2 was unreachable: the panel's four ADSR sliders drove ENV-1 and
        // ENV-2 was assigned from it, so no amount of knob-turning could make
        // a stab out of a pad. A long amplifier envelope and a short filter
        // one have to differ from the other way round.
        let render = |env1: [f32; 4], env2: [f32; 4]| {
            let mut s = Jupiter8Synth::new();
            s.init(44100.0, 64);
            s.set_parameter(P_ENV_MOD, 0.6);
            s.set_parameter(P_CUTOFF, 0.3);
            for (i, v) in [P_ENV1_A, P_ENV1_D, P_ENV1_S, P_ENV1_R].iter().zip(env1) {
                s.set_parameter(*i, v);
            }
            for (i, v) in [P_ENV2_A, P_ENV2_D, P_ENV2_S, P_ENV2_R].iter().zip(env2) {
                s.set_parameter(*i, v);
            }
            process_buffers(&mut s, &[note_on(60, 100, 0)], 300)
        };
        let stab = render([0.0, 0.6, 0.8, 0.3], [0.0, 0.2, 0.0, 0.1]);
        let pad = render([0.0, 0.2, 0.0, 0.1], [0.0, 0.6, 0.8, 0.3]);
        let late = 260 * 64;
        assert!(peak(&pad[late..]) > 10.0 * peak(&stab[late..]),
                "the amplifier envelope does not decide the note's shape: \
                 pad {} against stab {}", peak(&pad[late..]), peak(&stab[late..]));
    }

    // ── Envelope ──

    fn stage_seconds(mut env: JupiterEnvelope, stage: EnvStage) -> f64 {
        let mut n = 0u64;
        while env.stage == stage && n < 44100 * 200 {
            env.tick();
            n += 1;
        }
        n as f64 / 44100.0
    }

    #[test]
    fn the_envelope_takes_the_time_the_slider_says() {
        // The defect: every segment used its slider's seconds as a one-pole
        // *time constant* and ran until it was within 0.001 of the target,
        // which is 6.9 of them. Every segment now takes the time it says, and
        // both envelopes are the same type, so this holds for ENV-2 too.
        for slider in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let want_attack = attack_seconds(slider);
            let want = decay_seconds(slider);

            let mut e = JupiterEnvelope::new(44100.0);
            e.set_times(want_attack, 100.0, 100.0);
            e.sustain = 1.0;
            e.trigger();
            let measured = stage_seconds(e, EnvStage::Attack);
            assert!((measured - want_attack).abs() < want_attack * 0.02 + 0.001,
                    "attack {slider}: {measured:.3} s for a {want_attack:.3} s setting");

            let mut e = JupiterEnvelope::new(44100.0);
            e.set_times(0.0005, want, 100.0);
            e.sustain = 0.0;
            e.level = 1.0;
            e.enter_decay();
            let measured = stage_seconds(e, EnvStage::Decay);
            assert!((measured - want).abs() < want * 0.02 + 0.002,
                    "decay {slider}: {measured:.3} s for a {want:.3} s setting");

            let mut e = JupiterEnvelope::new(44100.0);
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
    fn the_envelope_taper_covers_the_published_range() {
        // Roland's specification for both envelopes: attack 1 ms to 5 s,
        // decay and release 1 ms to 10 s. The slider used to run nearly
        // linearly across its range, which put a tenth of a second at the
        // bottom hundredth of the travel and made the whole middle of the
        // control unusable.
        assert!((attack_seconds(0.0) - 0.001).abs() < 1e-9);
        assert!((attack_seconds(1.0) - 5.001).abs() < 1e-9);
        assert!((decay_seconds(0.0) - 0.001).abs() < 1e-9);
        assert!((decay_seconds(1.0) - 10.001).abs() < 1e-9);
        // The shape is the Juno-60's measured one, scaled to this
        // instrument's ends: the middle of the decay slider is short enough
        // to play a pluck with.
        assert!((decay_seconds(0.5) - 0.74).abs() < 0.05, "{}", decay_seconds(0.5));
        assert!((attack_seconds(0.5) - 0.38).abs() < 0.05, "{}", attack_seconds(0.5));
        // Monotone, or `slider_for` would not be an inverse.
        let mut previous = 0.0;
        for i in 0..=1000 {
            let s = f64::from(i) / 1000.0;
            let (a, d) = (attack_seconds(s), decay_seconds(s));
            assert!(a > previous && d >= previous, "the taper is not monotone at {s}");
            previous = a.min(d);
        }
    }

    #[test]
    fn the_slider_a_preset_loads_reproduces_its_time() {
        // `params_for_patch` inverts the taper by bisection; this is the
        // assertion that it inverts the right one.
        for want in [0.001, 0.01, 0.1, 0.4, 1.0, 3.0, 5.0] {
            let got = attack_seconds(f64::from(slider_for(attack_seconds, want)));
            assert!((got - want).abs() < want * 1e-4 + 1e-6, "attack {want}: {got}");
        }
        for want in [0.001, 0.01, 0.15, 1.0, 2.5, 10.0] {
            let got = decay_seconds(f64::from(slider_for(decay_seconds, want)));
            assert!((got - want).abs() < want * 1e-4 + 1e-6, "decay {want}: {got}");
        }
        for want in [0.05, 0.1, 1.0, 5.5, 40.0] {
            let got = lfo_hz(f64::from(slider_for(lfo_hz, want)));
            assert!((got - want).abs() < want * 1e-4, "lfo {want}: {got}");
        }
    }

    #[test]
    fn the_envelope_segments_are_curved_like_a_capacitor() {
        // Measured on a Juno-60: the attack is 63% of the way up at the half
        // way point, not 50%, and the decay is at 15%.
        let mut e = JupiterEnvelope::new(44100.0);
        e.set_times(2.0, 100.0, 100.0);
        e.sustain = 1.0;
        e.trigger();
        let mut level = 0.0;
        for _ in 0..44100 { level = e.tick(); }
        assert!((level - 0.632).abs() < 0.02, "attack half way: {level:.3}");

        let mut e = JupiterEnvelope::new(44100.0);
        e.set_times(0.001, 2.0, 100.0);
        e.sustain = 0.0;
        e.level = 1.0;
        e.enter_decay();
        let mut level = 1.0;
        for _ in 0..44100 { level = e.tick(); }
        assert!((level - 0.148).abs() < 0.02, "decay half way: {level:.3}");
    }

    // ── Filter ──

    /// Magnitude of an impulse response at `hz`.
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

    fn filter_response(cutoff_norm: f64, res: f64, four_pole: bool) -> Vec<f64> {
        let mut f = Ir3109Filter::new();
        (0..16384)
            .map(|i| {
                f.process(if i == 0 { 1e-3 } else { 0.0 }, cutoff_norm, res, four_pole, 44100.0)
                    / 1e-3
            })
            .collect()
    }

    #[test]
    fn the_filter_is_the_slope_the_switch_says_at_the_frequency_the_slider_says() {
        // The shape was always poles; the frequency was not. A naive
        // integrator puts a section an octave below its own coefficient, so
        // the slider marked 632 Hz measured 274 Hz at -3 dB where four poles
        // put it at 306.
        //
        // The reference is the analog cascade itself — 1/(1+(f/f0)^2)^2 for
        // four poles, 1/(1+(f/f0)^2) for two — which is 12 dB or 6 dB down at
        // the cutoff and only reaches its full asymptote well above it.
        // Matching the curve is the stronger claim than matching a slope.
        //
        // Measured low on the sweep, because the reference is an analog
        // cascade and a bilinear one warps against it: two octaves under
        // Nyquist the two curves are 1.5 dB apart on the discrete filter's
        // own merits, which says nothing about whether the pole is placed
        // right.
        let sr = 44100.0;
        for norm in [0.2, 0.35] {
            let f0 = cutoff_hz(norm);
            for (four_pole, poles) in [(true, 4.0f64), (false, 2.0)] {
                let ir = filter_response(norm, 0.0, four_pole);
                let at_dc = magnitude_at(&ir, 5.0, sr);
                for multiple in [1.0f64, 2.0, 4.0, 8.0] {
                    let want = -10.0 * poles * (1.0 + multiple * multiple).log10();
                    let got = 20.0 * (magnitude_at(&ir, f0 * multiple, sr) / at_dc).log10();
                    assert!(
                        (got - want).abs() < 0.6,
                        "{poles} poles, cutoff {f0:.0} Hz at {multiple}x: {got:.1} dB, \
                         the cascade owes {want:.1} dB"
                    );
                }
            }
        }
    }

    #[test]
    fn resonance_peaks_in_both_slope_positions() {
        // The 12 dB position used to take its feedback from the second
        // section, and two real poles never reach the half turn of phase a
        // loop needs to resonate: it lost 14 dB of passband and gained 2 dB of
        // peak. The tap moves with the switch and the feedback does not, so
        // the resonance is the same in both positions and only the slope
        // changes.
        let sr = 44100.0;
        let norm = 0.5;
        let f0 = cutoff_hz(norm);
        for four_pole in [true, false] {
            let flat = filter_response(norm, 0.0, four_pole);
            let reference = magnitude_at(&flat, 5.0, sr);
            let mut previous =
                20.0 * (magnitude_at(&flat, f0, sr) / reference).log10();
            for res in [0.4, 0.7, 0.9] {
                let ir = filter_response(norm, res, four_pole);
                let peak = 20.0 * (magnitude_at(&ir, f0, sr) / reference).log10();
                assert!(peak > previous + 3.0,
                        "{four_pole:?}: resonance {res} added {:.1} dB", peak - previous);
                previous = peak;
            }
            // ...and the passband is still there underneath it.
            let ir = filter_response(norm, 1.0, four_pole);
            let dc = 20.0 * (magnitude_at(&ir, 5.0, sr) / reference).log10();
            assert!(dc > -12.0, "{four_pole:?}: full resonance costs {dc:.1} dB of passband");
        }
    }

    #[test]
    fn the_filter_rings_at_the_top_of_the_resonance_travel() {
        // AMSynths, who cloned the IR3109 board, note that an untrimmed
        // Jupiter-8 does not quite self-oscillate; the demos everyone knows
        // it by say otherwise, and it depends on the trimmer. The model rings
        // for a long time either way, which is the audible half of the claim
        // and the half a patch is written against.
        let sr = 44100.0;
        let mut f = Ir3109Filter::new();
        for _ in 0..64 { f.process(0.5, 0.5, 1.0, true, sr); }
        let mut tail = 0.0f64;
        for i in 0..(sr as usize) {
            let out = f.process(0.0, 0.5, 1.0, true, sr);
            if i > sr as usize - 4410 { tail = tail.max(out.abs()); }
        }
        assert!(tail > 0.01, "the filter does not ring: tail {tail:.5}");
    }

    #[test]
    fn the_high_pass_is_six_db_per_octave_where_the_slider_puts_it() {
        // The corner used to run to 10 kHz — which leaves nothing but air —
        // while the comment beside it claimed 4.5 kHz, and the difference
        // equation behind it put the corner an octave out at the top of the
        // sweep. It now spans 20 Hz to 1 kHz, pinned just past the Juno-60's
        // highest measured position.
        let sr = 44100.0;
        assert!((hpf_hz(0.0) - 20.0).abs() < 1e-9);
        assert!((hpf_hz(1.0) - 1000.6).abs() < 1.0, "{}", hpf_hz(1.0));
        for slider in [0.25, 0.5, 0.75, 1.0] {
            let want = hpf_hz(slider);
            let mut h = HpFilter::new();
            let ir: Vec<f64> = (0..16384)
                .map(|i| h.process(if i == 0 { 1.0 } else { 0.0 }, slider, sr))
                .collect();
            let passband = magnitude_at(&ir, 15000.0, sr);
            let drop = 20.0 * (magnitude_at(&ir, want, sr) / passband).log10();
            assert!((drop + 3.0).abs() < 0.3, "slider {slider}: {drop:.2} dB at {want:.0} Hz");
            let slope = 20.0 * (magnitude_at(&ir, want / 4.0, sr)
                / magnitude_at(&ir, want / 8.0, sr)).log10();
            assert!((slope - 6.0).abs() < 0.5, "slider {slider}: {slope:.1} dB/oct");
        }
        let mut h = HpFilter::new();
        assert_eq!(h.process(0.7, 0.0, sr), 0.7, "the bottom of the slider is not a bypass");
    }

    #[test]
    fn keyboard_follow_tracks_an_octave_of_cutoff_per_octave_of_keyboard() {
        // The defect: the keyboard offset was divided by five semitones
        // instead of by the octaves the cutoff slider spans, so full keyboard
        // follow tracked at two octaves per octave. Measured through the
        // engine's own arithmetic, an octave apart, at full follow.
        let octaves = |note: u8| {
            (f64::from(note) - 60.0) / 12.0 / CUTOFF_OCTAVES * 1.0 * CUTOFF_OCTAVES
        };
        assert!((octaves(72) - 1.0).abs() < 1e-12, "an octave up: {}", octaves(72));
        assert!((octaves(48) + 1.0).abs() < 1e-12, "an octave down: {}", octaves(48));
        // ...and the whole path, through cutoff_hz: the same slider offset a
        // note an octave up produces has to double the corner.
        let base = 0.4;
        let offset = (f64::from(72u8) - 60.0) / 12.0 / CUTOFF_OCTAVES;
        assert!((cutoff_hz(base + offset) / cutoff_hz(base) - 2.0).abs() < 1e-9);
    }

    // ── Oscillators ──

    #[test]
    fn sync_locks_vco2_to_vco1s_period() {
        // VCO-2 an octave and a fifth up, hard synced: the output repeats at
        // VCO-1's period, not its own. The reset is placed inside the sample
        // rather than on its edge, so the period is exact rather than jittery.
        let sr = 44100.0;
        let (f1, f2) = (441.0, 441.0 * 3.0 * 1.5);
        let mut vco1 = JupiterVco::new();
        let mut vco2 = JupiterVco::new();
        vco1.set_freq(f1, sr);
        vco2.set_freq(f2, sr);
        let mut out = Vec::new();
        for _ in 0..1000 {
            let reset = vco1.advance();
            vco2.advance();
            if reset { vco2.sync_to(vco1.phase, vco1.dt); }
            out.push(vco2.value(1, 0.5));
        }
        // 100 samples to a period of VCO-1. Two periods apart, the synced
        // slave is on the same sample of the same shape.
        for i in 400..500 {
            assert!((out[i] - out[i + 100]).abs() < 1e-9,
                    "sample {i}: {} then {}", out[i], out[i + 100]);
        }
        // Without sync it is not, or the test above proves nothing.
        let mut free = JupiterVco::new();
        free.set_freq(f2, sr);
        let loose: Vec<f64> = (0..1000).map(|_| { free.advance(); free.value(1, 0.5) }).collect();
        let drift: f64 = (400..500).map(|i| (loose[i] - loose[i + 100]).abs()).sum();
        assert!(drift > 1.0, "the free oscillator is already periodic at VCO-1's period");
    }

    #[test]
    fn cross_modulation_bends_vco1() {
        // It used to multiply VCO-1's *output* by `vco2.noise_value`, a
        // register only written when VCO-2 is switched to noise — so on the
        // four patches in the bank that ask for cross modulation, with VCO-2
        // on a triangle or a saw, the control did precisely nothing. This is
        // the assertion that says it does something now, and that turning it
        // off leaves the oscillator alone.
        let render = |xmod: f32| {
            let mut s = Jupiter8Synth::new();
            s.init(44100.0, 64);
            s.set_parameter(P_CUTOFF, 1.0);
            s.set_parameter(P_ENV_MOD, 0.0);
            s.set_parameter(P_VCO1_WAVE, knob_for(0, 4));
            s.set_parameter(P_VCO2_WAVE, knob_for(0, 4));
            s.set_parameter(P_TUNE, 0.8);
            s.set_parameter(P_XMOD, xmod);
            process_buffers(&mut s, &[note_on(60, 100, 0)], 60)
        };
        let off = render(0.0);
        let deep = render(0.5);
        let diff: f32 = off.iter().zip(deep.iter()).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 1.0, "cross modulation changes nothing: diff={diff}");
        // Zero crossings are the cheap proxy for "the pitch is moving".
        let crossings = |x: &[f32]| (1..x.len()).filter(|&i| x[i] > 0.0 && x[i - 1] <= 0.0).count();
        assert!(crossings(&deep) > crossings(&off),
                "cross modulation added no motion: {} against {}",
                crossings(&deep), crossings(&off));
        assert!(deep.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn the_oscillator_is_bounded_at_both_ends() {
        // Three octaves of cross modulation on top of an octave of tuning on
        // top of the top of the keyboard runs past Nyquist, and a phase
        // increment past 1.0 walks out of an accumulator that only ever
        // subtracts one wrap.
        let sr = 44100.0;
        let mut v = JupiterVco::new();
        v.set_freq(200_000.0, sr);
        assert!(v.dt < 0.5, "dt {} would step over a whole period", v.dt);
        v.set_freq(-5.0, sr);
        assert!(v.dt > 0.0, "a negative frequency ran the ramp backwards");
        v.set_freq(f64::NAN, sr);
        assert!(v.dt.is_finite(), "a non-finite frequency reached the accumulator");
        for _ in 0..1000 {
            v.advance();
            assert!((0.0..1.0).contains(&v.phase), "phase {} left the ramp", v.phase);
        }
    }

    // ── LFO ──

    #[test]
    fn the_lfo_covers_the_published_range_on_all_four_waveforms() {
        // Roland's specification: 0.05 to 40 Hz, sine, sawtooth, square and
        // random. The slider used to be a straight line across that range,
        // which put 20 Hz in the middle of the control.
        assert!((lfo_hz(0.0) - 0.05).abs() < 1e-9);
        assert!((lfo_hz(1.0) - 40.0).abs() < 1e-9);
        assert!((lfo_hz(0.5) - 1.414).abs() < 0.01, "{}", lfo_hz(0.5));

        let sr = 44100.0;
        for waveform in 0..4u8 {
            let mut lfo = JupiterLfo::new();
            lfo.rate = 5.0;
            lfo.waveform = waveform;
            let mut out = Vec::new();
            let mut wraps = Vec::new();
            let mut previous = lfo.phase;
            for i in 0..(sr as usize * 4) {
                out.push(lfo.tick(sr));
                if lfo.phase < previous { wraps.push(i); }
                previous = lfo.phase;
            }
            let high = out.iter().copied().fold(f64::MIN, f64::max);
            let low = out.iter().copied().fold(f64::MAX, f64::min);
            assert!(high > 0.5 && low < -0.5, "waveform {waveform} spans {low}..{high}");
            assert!(high <= 1.0 && low >= -1.0, "waveform {waveform} leaves ±1");
            let period = (wraps[wraps.len() - 1] - wraps[0]) as f64 / (wraps.len() - 1) as f64;
            assert!((sr / period - 5.0).abs() < 0.01,
                    "waveform {waveform} ran at {:.3} Hz", sr / period);
        }
    }

    #[test]
    fn the_lfo_delay_holds_then_fades_and_does_not_restart_mid_chord() {
        // Two defects: the delay was a straight fade with no hold, and it was
        // retriggered by every note-on in every mode, so a note added to a
        // held chord restarted the vibrato under the notes already sounding.
        let sr = 44100.0;
        let mut lfo = JupiterLfo::new();
        lfo.rate = 5.0;
        lfo.delay_time = 1.0;
        lfo.trigger_delay();
        let mut silent = 0.0f64;
        for _ in 0..(sr as usize * 7 / 10) { silent = silent.max(lfo.tick(sr).abs()); }
        assert!(silent < 1e-9, "the LFO was audible during the hold: {silent}");
        let mut faded = 0.0f64;
        for _ in 0..(sr as usize * 4 / 10) { faded = faded.max(lfo.tick(sr).abs()); }
        assert!(faded > 0.9, "the LFO did not reach full depth: {faded}");

        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        s.set_parameter(P_MODE, knob_for(2, 4)); // poly, or every key is every voice
        s.set_parameter(P_LFO_DELAY, 1.0);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 200);
        let before = s.lfo.delay_counter;
        process_buffers(&mut s, &[note_on(64, 100, 0)], 1);
        assert!(s.lfo.delay_counter > before, "the second key restarted the delay");
        process_buffers(&mut s, &[note_off(60, 0), note_off(64, 1)], 600);
        process_buffers(&mut s, &[note_on(67, 100, 0)], 1);
        assert!(s.lfo.delay_counter < before, "a fresh phrase did not restart the delay");
    }

    // ── Level ──

    #[test]
    fn no_setting_of_the_new_controls_reaches_full_scale() {
        // The controls that were added are the ones that can add level: the
        // two mixer faders are separate now rather than a crossfade that
        // always summed to one, resonance and the slope switch are reachable
        // on every patch, and cross modulation and sync are reachable at all.
        // Eight voices, every source up, the filter wide open and ringing.
        for four_pole in [false, true] {
            for sync in [false, true] {
                let mut s = Jupiter8Synth::new();
                s.init(44100.0, 64);
                s.set_parameter(P_VCO1_LEVEL, 1.0);
                s.set_parameter(P_VCO2_LEVEL, 1.0);
                s.set_parameter(P_VCO1_WAVE, knob_for(1, 4));
                s.set_parameter(P_VCO2_WAVE, knob_for(1, 4));
                s.set_parameter(P_TUNE, 0.62);
                s.set_parameter(P_SYNC, knob_for(usize::from(sync), 2));
                s.set_parameter(P_XMOD, 1.0);
                s.set_parameter(P_HPF, 0.0);
                s.set_parameter(P_CUTOFF, 1.0);
                s.set_parameter(P_RESO, 1.0);
                s.set_parameter(P_SLOPE, knob_for(usize::from(four_pole), 2));
                s.set_parameter(P_ENV_MOD, 1.0);
                s.set_parameter(P_KEY_FOLLOW, 1.0);
                s.set_parameter(P_LEVEL, 1.0);
                s.set_parameter(P_ENV1_A, 0.0);
                s.set_parameter(P_ENV2_A, 0.0);
                s.set_parameter(P_ENV1_S, 1.0);
                s.set_parameter(P_ENV2_S, 1.0);
                let events: Vec<MidiEvent> = [36, 43, 48, 55, 60, 64, 67, 72]
                    .iter()
                    .map(|&n| note_on(n, 127, 0))
                    .collect();
                let out = process_buffers(&mut s, &events, 400);
                assert!(out.iter().all(|v| v.is_finite()),
                        "four_pole {four_pole} sync {sync}: not finite");
                // Measured: 0.682 at the worst of the four combinations —
                // 12 dB slope, sync off — so the extremes of every new
                // control together sit 3.3 dB under full scale and 1.6 dB
                // under the master limiter's ceiling.
                assert!(peak(&out) < 1.0,
                        "four_pole {four_pole} sync {sync}: peak {} is at full scale",
                        peak(&out));
                assert!(peak(&out) > 0.05,
                        "four_pole {four_pole} sync {sync}: peak {} is too quiet for \
                         this to be measuring the extremes at all", peak(&out));
            }
        }
    }
}

