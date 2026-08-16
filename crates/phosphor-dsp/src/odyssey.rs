//! ARP Odyssey style duophonic two-VCO synthesizer.
//!
//! The whole front panel, in the order it appears on the instrument: the
//! controller group at the far left, VCO-1, VCO-2, the LFO and sample-and-hold
//! pair, the audio mixer with the filters and the amplifier, and the two
//! envelope generators at the right. Two polyBLEP VCOs with hard sync, an XOR
//! ring modulator, a white/pink noise generator, the sample-and-hold mixer
//! that doubles as a modulation source in its own right, the non-resonant
//! high-pass ahead of a low-pass that can be any of the instrument's three
//! filter revisions, and the ADSR and AR envelope generators.
//!
//! Where a number came from ARP it says so at the constant. The envelope and
//! oscillator ranges are ARP's published specification (ARP Odyssey Service
//! Manual, model 2800): ADSR attack 5 ms to 5 s, decay 10 ms to 8 s, sustain
//! 0 to 100 %, release 15 ms to 10 s; AR attack 5 ms to 5 s and release 10 ms
//! to 8 s; portamento up to 1.5 s per octave; VCF and HPF 16 Hz to 16 kHz.
//! The LFO's 0.2 to 20 Hz, the fine tuning's ±400 cents and the oscillators'
//! 20 Hz to 2 kHz are the panel legends of the reissue, whose panel is a
//! reproduction of the Rev 3 instrument's (ARP ODYSSEY Owner's Manual, Korg).
//!
//! ARP do not publish the *shape* of any slider, but the parts list does: the
//! five envelope time controls, the portamento, the sample-and-hold lag and
//! the two frequency-mod depths are 1 M audio-taper pots and the sustain is a
//! 100 K linear one (ARP Odyssey 2800 slider locations and values). So the
//! times bend and the sustain does not. The bend itself is the Juno-60's
//! measured one rescaled to this instrument's end points — the nearest
//! slider-by-slider capture of a comparable panel there is. See `juno.rs`,
//! whose comments carry the measurements.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

use crate::level::soft_saturate;

const TWO_PI: f64 = std::f64::consts::TAU;

/// Fixed headroom trim on the voice output, applied after the VCA level.
///
/// Sized on ordinary playing, in step with the other four — see `OUTPUT_TRIM`
/// in dx7.rs, which carries the full reasoning. The trim lands this synth's
/// median patch at the same loudness as theirs.
///
/// This synth is duophonic, so a chord does not stack the way a poly's does:
/// most of the bank peaks louder on a *single* note than on eight, which is
/// why the symptom here was one note clipping on its own and why
/// `tests/headroom.rs` sweeps this bank on both voicings.
///
/// It moved when the panel was rebuilt, and downwards: keyboard follow used
/// to track at over two octaves of cutoff per octave of keyboard and the two
/// four-pole filters sat two octaves under the frequency their slider named,
/// so correcting both let a good deal more of every patch through. Measured
/// across all 44 held for 9.3 s at velocity 127: the loudest is 1 Funk, at
/// 0.208 on a single note and 0.216 on an eight-note chord, and the next is
/// 35 Oboe at 0.138. Nothing in the bank reaches the saturator's knee, so
/// every patch on every voicing is the trimmed voice sample for sample. The
/// median of the bank on a triad at velocity 100 is 0 Bass at 0.0153 RMS,
/// which sits between the DX7's median and the Jupiter's.
/// `instruments_are_level_matched` is the assertion that notices if this
/// drifts against the other four.
const OUTPUT_TRIM: f32 = 0.100;

// ── Parameter indices ──
//
// Front-panel order, left to right, because that is the order a player reaches
// for them. `patch` is first because index 0 is where the editor looks for a
// preset selector; the noise switch, portamento and transpose follow because
// on the instrument they are the controls at the far left of the panel, beside
// the pitch pads.
//
// Forty-three of these are new. The engine modelled about half a Odyssey and
// the panel exposed sixteen controls of it: the pulse width, the noise fader,
// the high-pass, keyboard follow, the LFO's routing and depth, portamento, the
// two mixer faders, both oscillators' tuning past a fiftieth of a semitone,
// the sample-and-hold in every form — and, most of all, the AR envelope. The
// four ADSR sliders drove the ADSR, the ADSR drove the filter, and the
// amplifier was the AR, whose own two sliders did not exist and whose times
// were assigned from the ADSR's. So the control marked `decay` could not
// change how long a note lasted on twenty-three of the forty-four presets.

pub const P_PATCH: usize = 0;
// Controller
pub const P_NOISE_TYPE: usize = 1;
pub const P_PORTAMENTO: usize = 2;
pub const P_TRANSPOSE: usize = 3;
// VCO-1
pub const P_VCO1_FREQ: usize = 4;
pub const P_VCO1_FINE: usize = 5;
pub const P_VCO1_KYBD: usize = 6;
pub const P_VCO1_FM1: usize = 7;
pub const P_VCO1_FM1_SRC: usize = 8;
pub const P_VCO1_FM2: usize = 9;
pub const P_VCO1_FM2_SRC: usize = 10;
pub const P_VCO1_PW: usize = 11;
pub const P_VCO1_PWM: usize = 12;
pub const P_VCO1_PWM_SRC: usize = 13;
// VCO-2
pub const P_VCO2_FREQ: usize = 14;
pub const P_VCO2_FINE: usize = 15;
pub const P_SYNC: usize = 16;
pub const P_VCO2_FM1: usize = 17;
pub const P_VCO2_FM1_SRC: usize = 18;
pub const P_VCO2_FM2: usize = 19;
pub const P_VCO2_FM2_SRC: usize = 20;
pub const P_VCO2_PW: usize = 21;
pub const P_VCO2_PWM: usize = 22;
pub const P_VCO2_PWM_SRC: usize = 23;
// LFO and sample and hold
pub const P_LFO_RATE: usize = 24;
pub const P_SH_A: usize = 25;
pub const P_SH_A_SRC: usize = 26;
pub const P_SH_B: usize = 27;
pub const P_SH_B_SRC: usize = 28;
pub const P_SH_LAG: usize = 29;
pub const P_SH_TRIG: usize = 30;
// Audio mixer
pub const P_RING_LEVEL: usize = 31;
pub const P_RING_SRC: usize = 32;
pub const P_VCO1_LEVEL: usize = 33;
pub const P_VCO1_WAVE: usize = 34;
pub const P_VCO2_LEVEL: usize = 35;
pub const P_VCO2_WAVE: usize = 36;
// HPF and VCF
pub const P_HPF: usize = 37;
pub const P_CUTOFF: usize = 38;
pub const P_RESO: usize = 39;
pub const P_FILTER_TYPE: usize = 40;
pub const P_VCF_KYBD: usize = 41;
pub const P_VCF_KYBD_SRC: usize = 42;
pub const P_VCF_LFO: usize = 43;
pub const P_VCF_LFO_SRC: usize = 44;
pub const P_VCF_ENV: usize = 45;
pub const P_VCF_ENV_SRC: usize = 46;
// VCA
pub const P_VCA_GAIN: usize = 47;
pub const P_DRIVE: usize = 48;
pub const P_LEVEL: usize = 49;
// Envelope generators
pub const P_AR_A: usize = 50;
pub const P_AR_R: usize = 51;
pub const P_ATTACK: usize = 52;
pub const P_DECAY: usize = 53;
pub const P_SUSTAIN: usize = 54;
pub const P_RELEASE: usize = 55;
pub const P_VCA_ENV: usize = 56;
pub const P_ADSR_TRIG: usize = 57;
pub const P_AR_TRIG: usize = 58;
pub const PARAM_COUNT: usize = 59;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "patch",
    "noise", "porta", "transpos",
    "v1 freq", "v1 fine", "v1 kybd", "v1 fm1", "v1 fm1sr", "v1 fm2", "v1 fm2sr",
    "v1 pw", "v1 pwm", "v1 pwmsr",
    "v2 freq", "v2 fine", "sync", "v2 fm1", "v2 fm1sr", "v2 fm2", "v2 fm2sr",
    "v2 pw", "v2 pwm", "v2 pwmsr",
    "lfo rate", "sh in a", "sh a src", "sh in b", "sh b src", "sh lag", "sh trig",
    "mix ring", "ring src", "mix vco1", "vco1 wav", "mix vco2", "vco2 wav",
    "hpf", "freq", "res", "filter",
    "vcf kybd", "kybd src", "vcf lfo", "lfo src", "vcf env", "env src",
    "vca gain", "drive", "level",
    "ar a", "ar r", "attack", "decay", "sustain", "release",
    "vca env", "adsr trg", "ar trg",
];

/// Patch 0, "Bass", the preset the instrument loads with, as its panel.
/// `patch_zero_is_the_default_parameter_block` holds these and the first row
/// of [`BANK`] together, so neither can be edited without the other.
pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    0.0,       // patch: Bass
    0.25,      // noise: white
    0.0,       // portamento: off
    0.5,       // transpose: 0
    0.5,       // vco-1 freq: concert pitch
    0.5,       // vco-1 fine: 0 cents
    0.75,      // vco-1 keyboard: audio, tracking
    0.0,       // vco-1 fm1 depth
    0.25,      // vco-1 fm1 source: LFO sine
    0.0,       // vco-1 fm2 depth
    0.25,      // vco-1 fm2 source: S/H
    0.0,       // vco-1 pulse width: square
    0.0,       // vco-1 pwm depth
    0.25,      // vco-1 pwm source: LFO
    0.499_624, // vco-2 freq: -3 cents
    0.5,       // vco-2 fine: 0 cents
    0.25,      // sync: off
    0.0,       // vco-2 fm1 depth
    0.25,      // vco-2 fm1 source: LFO sine
    0.0,       // vco-2 fm2 depth
    0.25,      // vco-2 fm2 source: S/H
    0.0,       // vco-2 pulse width: square
    0.0,       // vco-2 pwm depth
    0.25,      // vco-2 pwm source: LFO
    0.349_485, // lfo rate: 1 Hz
    0.0,       // s/h input a level
    0.25,      // s/h input a source: VCO-1 saw
    1.0,       // s/h input b level
    0.25,      // s/h input b source: noise
    0.0,       // s/h lag
    0.25,      // s/h trigger: LFO
    0.0,       // mixer: noise/ring level
    0.25,      // mixer: noise
    0.8,       // mixer: vco-1 level
    0.25,      // vco-1 waveform: saw
    0.8,       // mixer: vco-2 level
    0.25,      // vco-2 waveform: saw
    0.0,       // hpf: 16 Hz, out of the way
    0.25,      // vcf freq
    0.3,       // vcf resonance
    0.166_667, // filter: 4023
    0.3,       // vcf keyboard follow amount
    0.25,      // vcf keyboard follow source: keyboard CV
    0.0,       // vcf lfo amount
    0.75,      // vcf lfo source: LFO
    0.6,       // vcf envelope amount
    0.25,      // vcf envelope source: ADSR
    0.0,       // vca gain: no drone
    0.25,      // drive: off
    0.75,      // vca level
    0.0,       // ar attack: 5 ms
    0.427_986, // ar release: 0.3 s
    0.0,       // adsr attack: 5 ms
    0.427_986, // adsr decay: 0.3 s
    0.2,       // adsr sustain
    0.304_306, // adsr release: 0.15 s
    0.25,      // vca envelope: AR
    0.25,      // adsr trigger: keyboard gate
    0.25,      // ar trigger: keyboard gate
];

// ── Patches ──

pub const PATCH_COUNT: usize = 44;

/// The preset names, in bank order.
pub const PATCH_NAMES: [&str; PATCH_COUNT] = derive_names();

/// The knob position that selects patch `index`, for a caller sweeping the
/// bank from outside — a level measurement, an export, a test.
///
/// The midpoint of the step, which is the one position in it that no amount
/// of float rounding can push into a neighbour, and the same position
/// [`step_discrete`] moves between. `index / (count - 0.01)`, the obvious
/// alternative, is not reliable at every bank size.
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
// Twenty-four of the panel's controls are switches rather than sliders — the
// two three-position ones are the transpose lever and the filter-revision
// selector — and the patch selector makes twenty-five indices that step. They
// are stored in the same 0..1 parameter block as everything else, so a switch
// is a knob divided into `n` equal steps.

/// How many positions a switch has, or `None` for a slider.
fn discrete_steps(index: usize) -> Option<usize> {
    match index {
        P_PATCH => Some(PATCH_COUNT),
        P_NOISE_TYPE
        | P_VCO1_KYBD
        | P_VCO1_FM1_SRC
        | P_VCO1_FM2_SRC
        | P_VCO1_PWM_SRC
        | P_SYNC
        | P_VCO2_FM1_SRC
        | P_VCO2_FM2_SRC
        | P_VCO2_PWM_SRC
        | P_SH_A_SRC
        | P_SH_B_SRC
        | P_SH_TRIG
        | P_RING_SRC
        | P_VCO1_WAVE
        | P_VCO2_WAVE
        | P_VCF_KYBD_SRC
        | P_VCF_LFO_SRC
        | P_VCF_ENV_SRC
        | P_DRIVE
        | P_VCA_ENV
        | P_ADSR_TRIG
        | P_AR_TRIG => Some(2),
        P_TRANSPOSE | P_FILTER_TYPE => Some(3),
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
/// 1/44 of the range 44 times does not arrive at 1.0 — the error is a few ulps
/// either way, and a step boundary missed by one ulp is a keypress that
/// visibly does nothing. The DX7's bank knob stalled that way, and this
/// instrument's patch knob was stepping by `1/(n - 0.01)` for the same reason.
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
        P_NOISE_TYPE => ["white", "pink"][step],
        P_TRANSPOSE => ["-2 oct", "0", "+2 oct"][step],
        P_VCO1_KYBD => ["LF", "audio"][step],
        P_VCO1_FM1_SRC => ["LFO sin", "LFO sqr"][step],
        P_VCO2_FM1_SRC => ["LFO sin", "S/H mix"][step],
        P_VCO1_FM2_SRC | P_VCO2_FM2_SRC => ["S/H", "ADSR"][step],
        P_VCO1_PWM_SRC | P_VCO2_PWM_SRC => ["LFO", "ADSR"][step],
        P_SYNC | P_DRIVE => ["off", "on"][step],
        P_SH_A_SRC => ["V1 saw", "V1 sqr"][step],
        P_SH_B_SRC => ["noise", "V2 sqr"][step],
        P_SH_TRIG => ["LFO", "KYBD"][step],
        P_RING_SRC => ["noise", "ring"][step],
        P_VCO1_WAVE | P_VCO2_WAVE => ["saw", "pulse"][step],
        P_FILTER_TYPE => ["4023", "4035", "4075"][step],
        P_VCF_KYBD_SRC => ["KYBD CV", "S/H mix"][step],
        P_VCF_LFO_SRC => ["S/H", "LFO"][step],
        // The two switches read in opposite orders on the panel: the
        // filter's is marked ADSR/AR and the amplifier's AR/ADSR.
        P_VCF_ENV_SRC => ["ADSR", "AR"][step],
        P_VCA_ENV => ["AR", "ADSR"][step],
        P_ADSR_TRIG | P_AR_TRIG => ["KYBD", "LFO rpt"][step],
        _ => return None,
    })
}

/// A slider's value in seconds, for the ones that measure time. `None` for
/// the ones that read as a percentage.
pub fn param_seconds(index: usize, value: f32) -> Option<f64> {
    let v = f64::from(value);
    match index {
        P_ATTACK | P_AR_A => Some(attack_seconds(v)),
        P_DECAY | P_AR_R => Some(decay_seconds(v)),
        P_RELEASE => Some(release_seconds(v)),
        P_PORTAMENTO => Some(porta_seconds(v)),
        P_SH_LAG => Some(lag_seconds(v)),
        _ => None,
    }
}

// ── Panel tapers ──
//
// The sliders are not linear in time or frequency. The end points are ARP's
// published specification; the curves between them are the shapes measured on
// a Juno-60, rescaled — see the note at the top of the file for why that
// instrument is the reference.

/// The shape of an audio-taper pot: zero at the bottom of the travel, one at
/// the top, and most of its useful range in the bottom fifth. `curve` is how
/// hard it bends.
fn taper(curve: f64, slider: f64) -> f64 {
    let s = slider.clamp(0.0, 1.0);
    (curve * s).exp_m1() / curve.exp_m1()
}

/// The decay and release taper: the same shape with an extra factor of the
/// slider, which is the one measured on a Juno-60's decay cap. It puts a
/// twentieth of the travel's time in the middle of the travel.
fn taper_squared(curve: f64, slider: f64) -> f64 {
    let s = slider.clamp(0.0, 1.0);
    taper(curve, s) * s
}

const ATTACK_CURVE: f64 = 5.0;
const DECAY_CURVE: f64 = 4.0;

/// Attack slider to seconds, for both envelopes. ARP's range is 5 ms to 5 s.
fn attack_seconds(slider: f64) -> f64 {
    0.005 + taper(ATTACK_CURVE, slider) * 5.0
}

/// The ADSR's decay and the AR's release share a taper because they share a
/// specification: 10 ms to 8 s.
///
/// The defect this replaced: the slider ran linearly onto 10 ms - 8 s, and the
/// number it produced was then used as a *time constant* by a one-pole that
/// ran for 6.9 of them, so the middle of the decay slider took 27 s to reach
/// -40 dB. Linearly, too, which put a tenth of a second in the bottom
/// hundredth of the travel and made the whole middle of the control unusable.
fn decay_seconds(slider: f64) -> f64 {
    0.010 + taper_squared(DECAY_CURVE, slider) * 8.0
}

/// The ADSR's release has its own range: 15 ms to 10 s.
fn release_seconds(slider: f64) -> f64 {
    0.015 + taper_squared(DECAY_CURVE, slider) * 10.0
}

/// LFO rate slider to Hz: the panel's 0.2 to 20 Hz, exponential across it. A
/// straight line would put 10 Hz in the middle of the slider, which is not a
/// vibrato; this puts 2 Hz there.
const LFO_MIN_HZ: f64 = 0.2;
const LFO_MAX_HZ: f64 = 20.0;

fn lfo_hz(slider: f64) -> f64 {
    LFO_MIN_HZ * (LFO_MAX_HZ / LFO_MIN_HZ).powf(slider.clamp(0.0, 1.0))
}

/// Portamento slider to seconds per octave. ARP quote a minimum speed of
/// about 1.5 seconds per octave, and the bottom of the travel is off.
const PORTAMENTO_MAX: f64 = 1.5;

fn porta_seconds(slider: f64) -> f64 {
    taper(DECAY_CURVE, slider) * PORTAMENTO_MAX
}

/// Sample-and-hold lag slider to the seconds a step takes to settle. ARP
/// publish no range; half a second at the top is a slew slow enough to turn
/// the staircase into a wander and short enough to still read as steps at the
/// bottom.
const LAG_MAX: f64 = 0.5;

fn lag_seconds(slider: f64) -> f64 {
    taper(DECAY_CURVE, slider) * LAG_MAX
}

/// The coarse frequency slider to cents, bipolar, with the middle of the
/// travel at concert pitch.
///
/// The panel legend is 20 Hz to 2 kHz, which is 6.64 octaves, and the slider
/// is linear in pitch across it. That is coarse — a keypress in the editor
/// moves four semitones — which is what the fine slider beside it is for, and
/// it is what lets VCO-2 reach the octaves and fifths the old ±50-cent detune
/// control could not: four presets in the bank carried an interval the panel
/// could not express and were silently collapsed onto +50 cents.
const TUNE_OCTAVES: f64 = 6.643_856_189_774_724; // log2(2000/20)
const TUNE_CENTS: f64 = 1200.0 * TUNE_OCTAVES;

fn tune_cents(slider: f64) -> f64 {
    (slider.clamp(0.0, 1.0) - 0.5) * TUNE_CENTS
}

fn tune_slider(cents: f64) -> f32 {
    (0.5 + cents / TUNE_CENTS).clamp(0.0, 1.0) as f32
}

/// The fine frequency slider to cents: the panel's ±400.
const FINE_CENTS: f64 = 800.0;

fn fine_cents(slider: f64) -> f64 {
    (slider.clamp(0.0, 1.0) - 0.5) * FINE_CENTS
}

fn fine_slider(cents: f64) -> f32 {
    (0.5 + cents / FINE_CENTS).clamp(0.0, 1.0) as f32
}

/// Pulse width slider to duty cycle. The panel runs from 50 % at the bottom of
/// the travel down to a narrow pulse at the top, which is the direction the
/// legend reads: "50 %…MIN".
const PW_MIN: f64 = 0.05;

fn pulse_width(slider: f64) -> f64 {
    0.5 - slider.clamp(0.0, 1.0) * (0.5 - PW_MIN)
}

fn pw_slider(duty: f64) -> f32 {
    ((0.5 - duty) / (0.5 - PW_MIN)).clamp(0.0, 1.0) as f32
}

/// How far the pulse width swings either side of its setting at full
/// modulation. Past about this the pulse collapses to nothing at the ends of
/// the sweep and the oscillator drops out.
const PW_SWING: f64 = 0.4;

/// Frequency-modulation depth slider to cents. Full travel is two octaves
/// either way, which is what a sweep, a siren or a sync tear needs; the taper
/// keeps a two-cent vibrato inside the bottom fiftieth of the travel.
const FM_MAX_CENTS: f64 = 2400.0;
const FM_CURVE: f64 = 5.0;

fn fm_cents(slider: f64) -> f64 {
    taper(FM_CURVE, slider) * FM_MAX_CENTS
}

/// Cutoff slider to Hz: the panel's 16 Hz to 16 kHz, exponential, and the same
/// sweep on all three filter revisions because the panel prints one legend and
/// one switch changes which board is in circuit.
///
/// The three used to have three different sweeps — 20 Hz to 35 kHz, to 20 kHz
/// and to 14 kHz. The first of those runs past Nyquist, and the state-variable
/// filter behind it turns round and *closes* over the top seventh of its
/// travel: the corner measured 21 kHz at 0.85 of the slider and 6 kHz at the
/// top of it.
const CUTOFF_MIN_HZ: f64 = 16.0;
const CUTOFF_DECADES: f64 = 3.0;
/// How many octaves the cutoff slider covers end to end, which is what turns
/// a keyboard-follow amount into a slider offset.
const CUTOFF_OCTAVES: f64 = CUTOFF_DECADES * std::f64::consts::LOG2_10;

fn cutoff_hz(slider: f64) -> f64 {
    CUTOFF_MIN_HZ * 10.0f64.powf(CUTOFF_DECADES * slider.clamp(0.0, 1.0))
}

/// The high-pass shares the low-pass's legend: 16 Hz to 16 kHz, 6 dB/octave
/// and non-resonant.
fn hpf_hz(slider: f64) -> f64 {
    cutoff_hz(slider)
}

/// The slider position whose taper gives `want`, by bisection.
///
/// Only [`OdysseySynth::params_for_patch`] needs it: the preset table is held
/// in seconds, hertz and cents, because that is what a patch *is*, and the
/// panel is held in slider positions, because that is what a panel is.
/// Twenty-four halvings put the answer within 6e-8 of the travel, which is
/// finer than an f32 knob can hold. Every taper it is used on is monotonic.
///
/// Real-time safe: a fixed count of halvings, no allocation, no lock. It runs
/// on the audio thread, because that is where a patch change arrives, but only
/// when the patch selector moves.
fn slider_for(taper: fn(f64) -> f64, want: f64) -> f32 {
    // The bottom of the travel exactly, rather than the last halving above
    // it: several of these tapers start at zero and a preset that asks for
    // none of something should get a slider that is off, not one sitting a
    // sixteen-millionth of the way up.
    if want <= taper(0.0) {
        return 0.0;
    }
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if taper(mid) < want { lo = mid; } else { hi = mid; }
    }
    (0.5 * (lo + hi)) as f32
}

// ── Internal preset data ──
//
// The panel in the units the engine works in: seconds, hertz and cents where
// the control measures one of those, and the slider's own 0..1 where it is a
// fader or a mod depth. `params_for_patch` runs a row back through the panel
// tapers to get the slider positions that produce it, and `preset_round_trip`
// holds the two directions together.

#[derive(Debug, Clone, Copy)]
struct OdysseyPatch {
    // Controller
    /// The noise generator's white/pink switch.
    noise_pink: bool,
    /// Seconds per octave of glide. ARP's control is a speed, not a time, so
    /// a wider interval takes proportionally longer.
    portamento: f64,
    /// The transpose lever: -2, 0 or +2 octaves.
    transpose: i8,
    // VCO-1
    vco1_tune: f64, // cents, coarse
    vco1_fine: f64, // cents
    /// The keyboard switch. False disconnects the oscillator from the
    /// keyboard CV and drops it into the 0.2-20 Hz range, which is how the
    /// instrument gets a second LFO.
    vco1_kybd: bool,
    vco1_fm1: f64, // cents of LFO frequency modulation
    /// The LFO's sine or its square.
    vco1_fm1_square: bool,
    vco1_fm2: f64, // cents from the sample and hold, or from the ADSR
    vco1_fm2_adsr: bool,
    vco1_pw: f64,  // duty cycle, 0.05..0.5
    vco1_pwm: f64, // 0..1 of PW_SWING
    vco1_pwm_adsr: bool,
    // VCO-2
    vco2_tune: f64,
    vco2_fine: f64,
    /// Hard sync: VCO-1 restarts VCO-2's ramp. Duophony is not available with
    /// it on, because VCO-2 no longer has a pitch of its own.
    sync: bool,
    vco2_fm1: f64,
    /// The LFO's sine, or the sample-and-hold *mixer* — the unsampled sum,
    /// which is what makes audio-rate modulation possible on this panel.
    vco2_fm1_shmix: bool,
    vco2_fm2: f64,
    vco2_fm2_adsr: bool,
    vco2_pw: f64,
    vco2_pwm: f64,
    vco2_pwm_adsr: bool,
    // LFO and sample and hold
    lfo_rate: f64, // Hz
    sh_a: f64,
    /// VCO-1's sawtooth or its square into the sample-and-hold mixer.
    sh_a_square: bool,
    sh_b: f64,
    /// The noise generator or VCO-2's square into the mixer.
    sh_b_vco2: bool,
    sh_lag: f64, // seconds for a step to settle
    /// The LFO's square, or the keyboard gate, clocks the hold.
    sh_kybd_trig: bool,
    // Audio mixer
    /// The third fader, shared by the noise generator and the ring modulator.
    ring_level: f64,
    ring_mod: bool,
    vco1_level: f64,
    vco1_pulse: bool,
    vco2_level: f64,
    vco2_pulse: bool,
    // Filters
    hpf: f64,
    cutoff: f64,
    resonance: f64,
    /// 0 = 4023 (Rev 1), 1 = 4035 (Rev 2), 2 = 4075 (Rev 3).
    filter_type: u8,
    kybd_amount: f64,
    /// The keyboard CV, or the sample-and-hold mixer, into the first mod slot.
    kybd_from_sh: bool,
    lfo_amount: f64,
    /// The LFO, or the held sample, into the second mod slot.
    lfo_from_lfo: bool,
    env_amount: f64,
    /// The ADSR or the AR into the third mod slot.
    env_from_ar: bool,
    // VCA
    /// The manual offset: signal passes at this level whether or not a key is
    /// down, which is how the instrument drones.
    vca_gain: f64,
    drive: bool,
    // Envelope generators
    ar_a: f64,
    ar_r: f64,
    adsr_a: f64,
    adsr_d: f64,
    adsr_s: f64,
    adsr_r: f64,
    /// Which envelope the amplifier follows.
    vca_adsr: bool,
    /// Retrigger on the LFO's square instead of on the key.
    adsr_lfo_trig: bool,
    ar_lfo_trig: bool,
}

/// The panel every preset starts from: both oscillators at concert pitch on
/// sawtooth, nothing modulating anything, the filter half open on the Rev 1
/// board, and the amplifier on the AR.
const BASE: OdysseyPatch = OdysseyPatch {
    noise_pink: false,
    portamento: 0.0,
    transpose: 0,
    vco1_tune: 0.0,
    vco1_fine: 0.0,
    vco1_kybd: true,
    vco1_fm1: 0.0,
    vco1_fm1_square: false,
    vco1_fm2: 0.0,
    vco1_fm2_adsr: false,
    vco1_pw: 0.5,
    vco1_pwm: 0.0,
    vco1_pwm_adsr: false,
    vco2_tune: 0.0,
    vco2_fine: 0.0,
    sync: false,
    vco2_fm1: 0.0,
    vco2_fm1_shmix: false,
    vco2_fm2: 0.0,
    vco2_fm2_adsr: false,
    vco2_pw: 0.5,
    vco2_pwm: 0.0,
    vco2_pwm_adsr: false,
    lfo_rate: 1.0,
    sh_a: 0.0,
    sh_a_square: false,
    sh_b: 1.0,
    sh_b_vco2: false,
    sh_lag: 0.0,
    sh_kybd_trig: false,
    ring_level: 0.0,
    ring_mod: false,
    vco1_level: 0.8,
    vco1_pulse: false,
    vco2_level: 0.8,
    vco2_pulse: false,
    hpf: 0.0,
    cutoff: 0.5,
    resonance: 0.0,
    filter_type: 0,
    kybd_amount: 0.0,
    kybd_from_sh: false,
    lfo_amount: 0.0,
    lfo_from_lfo: true,
    env_amount: 0.0,
    env_from_ar: false,
    vca_gain: 0.0,
    drive: false,
    ar_a: 0.005,
    ar_r: 0.3,
    adsr_a: 0.005,
    adsr_d: 0.3,
    adsr_s: 0.5,
    adsr_r: 0.15,
    vca_adsr: false,
    adsr_lfo_trig: false,
    ar_lfo_trig: false,
};

/// One row of the bank: a name and the panel that plays it.
#[derive(Debug, Clone, Copy)]
struct Program {
    name: &'static str,
    voice: OdysseyPatch,
}

const fn derive_names() -> [&'static str; PATCH_COUNT] {
    let mut names = [""; PATCH_COUNT];
    let mut i = 0;
    while i < PATCH_COUNT {
        names[i] = BANK[i].name;
        i += 1;
    }
    names
}

// ── The bank ──
//
// Forty-four presets, not a factory set: the Odyssey stores nothing, so there
// is no bank to transcribe. These are the sounds the instrument is known for,
// voiced from the sources named against each one and carried forward from the
// panel that came before, with three kinds of correction where the old panel
// could not say what the preset meant:
//
// * the four that asked for an interval — 3 Bells a fifth, 15 Duo and 29
//   Organ an octave, 37 Robot a minor third — and got +50 cents, because the
//   detune control the engine read spanned a semitone either way;
// * the two that asked for an LFO slower than the panel's own 0.2 Hz floor —
//   13 FltSwp at 0.12 Hz and 21 Wind at 0.15 — and got 0.2 Hz. They are
//   written at 0.2 Hz here rather than at a number the instrument cannot
//   reach;
// * the twenty-four whose sample-and-hold was welded to the LFO's own depth
//   sliders, so that asking for pitch or filter modulation got a sine *and* a
//   random staircase twice as deep on top of it. The hold has its own
//   routing now, and only the patch that is about it uses it;
// * every one of them, for the AR: its two sliders did not exist on the old
//   panel and its times were assigned from the ADSR's, so the twenty-three
//   presets that run their amplifier from the AR had their release taken from
//   a control marked `release` that also set the ADSR's. Thirty-four of the
//   forty-four rows carry AR times that differ from their ADSR's, and every
//   one of those thirty-four was dead data. They are honoured here.
//
// Sources:
//   [PB81]  ARP Odyssey Patch Book, ARP Instruments Inc., 1981
//   [OM76]  ARP Odyssey MkII Owner's Manual, 1976
//   [KPB]   Korg ARP Odyssey Patchbook, 2017
//   [SS]    Gordon Reid, "Synth Secrets", Sound on Sound, 1999-2004
//   [808]   Roland TR-808 circuit analysis (bridged-T tom/conga design)

const BANK: [Program; PATCH_COUNT] = [
    // Bass — classic Odyssey bass
    Program { name: "Bass", voice: OdysseyPatch {
            vco2_tune: -3.0, cutoff: 0.25, resonance: 0.3, env_amount: 0.6, kybd_amount: 0.3,
            lfo_rate: 1.0, adsr_a: 0.005, adsr_d: 0.3, adsr_s: 0.2, adsr_r: 0.15,
            ar_a: 0.005, ar_r: 0.3,
        ..BASE } },
    // Funk — Chameleon-style funky bass
    Program { name: "Funk", voice: OdysseyPatch {
            vco2_tune: -5.0, vco1_level: 1.0, vco2_level: 1.0, cutoff: 0.25, resonance: 0.8,
            env_amount: 0.75, kybd_amount: 0.4, lfo_rate: 1.0, adsr_a: 0.005, adsr_d: 0.25,
            adsr_s: 0.1, adsr_r: 0.2, ar_a: 0.005, ar_r: 0.25,
        ..BASE } },
    // SyncLd — aggressive sync sweep lead
    Program { name: "SyncLd", voice: OdysseyPatch {
            vco1_level: 0.0, vco2_level: 1.0, sync: true, cutoff: 0.65, resonance: 0.2,
            env_amount: 0.7, kybd_amount: 0.5, lfo_rate: 5.0, adsr_a: 0.005, adsr_d: 0.4,
            adsr_s: 0.5, adsr_r: 0.25, ar_a: 0.005, ar_r: 0.3, vca_adsr: true,
        ..BASE } },
    // Bells — ring mod metallic bells
    Program { name: "Bells", voice: OdysseyPatch {
            vco1_pulse: true, vco2_pulse: true, vco2_tune: 700.0, vco1_level: 0.0,
            vco2_level: 0.0, ring_mod: true, ring_level: 1.0, cutoff: 0.5, resonance: 0.15,
            env_amount: 0.5, kybd_amount: 0.6, lfo_rate: 1.0, adsr_a: 0.005, adsr_d: 0.5,
            adsr_s: 0.0, adsr_r: 0.4, ar_a: 0.005, ar_r: 0.5, vca_adsr: true,
        ..BASE } },
    // Pad — strings/pad
    Program { name: "Pad", voice: OdysseyPatch {
            vco2_tune: 6.0, vco1_level: 0.7, vco2_level: 0.7, vco1_pw: 0.3, vco2_pw: 0.3,
            cutoff: 0.45, resonance: 0.1, hpf: 0.05, env_amount: 0.2, kybd_amount: 0.6,
            vco1_fm1: 1.5, vco2_fm1: 1.5, vco1_pwm: 0.3, vco2_pwm: 0.3, lfo_rate: 4.0,
            adsr_a: 0.4, adsr_d: 0.3, adsr_s: 0.8, adsr_r: 0.5, ar_a: 0.4, ar_r: 0.5,
            vca_adsr: true,
        ..BASE } },
    // S&H — sample and hold random pattern, and the one preset in the bank
    // that is *about* the hold: the noise generator into the mixer, the LFO's
    // square clocking it at 6 Hz, a little lag to round the steps off, and
    // the held voltage on both oscillators' second frequency-mod input and on
    // the filter. It used to be a sine vibrato and a sine filter sweep, with
    // the staircase arriving as a side effect of the depth slider.
    Program { name: "S&H", voice: OdysseyPatch {
            vco2_pulse: true, vco1_level: 0.6, vco2_level: 0.6, cutoff: 0.4, resonance: 0.3,
            env_amount: 0.3, kybd_amount: 0.4,
            vco1_fm2: 300.0, vco2_fm2: 300.0,
            sh_lag: 0.02, lfo_amount: 0.3, lfo_from_lfo: false,
            lfo_rate: 6.0, adsr_a: 0.005, adsr_d: 0.2, adsr_s: 0.6,
            adsr_r: 0.2, ar_a: 0.005, ar_r: 0.2,
        ..BASE } },
    // Zap — sci-fi laser effect
    Program { name: "Zap", voice: OdysseyPatch {
            vco1_level: 0.0, vco2_level: 1.0, sync: true, cutoff: 0.8, resonance: 0.4,
            env_amount: 1.0, kybd_amount: 0.3, lfo_rate: 1.0, adsr_a: 0.005, adsr_d: 0.6,
            adsr_s: 0.0, adsr_r: 0.4, ar_a: 0.005, ar_r: 0.6, vca_adsr: true,
        ..BASE } },
    // HwkFunk — Alan Hawkshaw funky sequence style
    Program { name: "HwkFunk", voice: OdysseyPatch {
            vco1_pulse: true, vco1_level: 0.7, vco2_level: 0.5, vco1_pw: 0.35, vco2_pw: 0.35,
            filter_type: 1, cutoff: 0.25, resonance: 0.3, hpf: 0.05, env_amount: 0.55,
            kybd_amount: 0.6, lfo_rate: 1.0, adsr_a: 0.001, adsr_d: 0.2, adsr_s: 0.1,
            adsr_r: 0.08, ar_a: 0.001, ar_r: 0.15,
        ..BASE } },
    // Atmos — Brian Bennett atmospheric pad
    Program { name: "Atmos", voice: OdysseyPatch {
            vco2_tune: 6.0, vco1_level: 0.6, vco2_level: 0.6, ring_level: 0.05, cutoff: 0.4,
            resonance: 0.35, hpf: 0.08, env_amount: 0.2, kybd_amount: 0.3, lfo_amount: 0.25,
            lfo_rate: 0.2, portamento: 0.15, adsr_a: 1.5, adsr_d: 1.0, adsr_s: 0.7,
            adsr_r: 2.5, ar_a: 1.8, ar_r: 3.0, vca_adsr: true,
        ..BASE } },
    // Cars — Gary Numan nasal lead
    Program { name: "Cars", voice: OdysseyPatch {
            vco1_pulse: true, vco2_pulse: true, vco2_tune: 5.0, vco2_level: 0.6,
            vco1_pw: 0.4, vco2_pw: 0.4, filter_type: 2, cutoff: 0.45, resonance: 0.25,
            hpf: 0.05, env_amount: 0.3, kybd_amount: 0.5, lfo_rate: 1.0, portamento: 0.075,
            adsr_a: 0.01, adsr_d: 0.4, adsr_s: 0.5, adsr_r: 0.2, ar_a: 0.01, ar_r: 0.25,
        ..BASE } },
    // SciFi — wobble effect
    Program { name: "SciFi", voice: OdysseyPatch {
            vco2_tune: 12.0, vco1_level: 0.7, vco2_level: 0.5, ring_mod: true,
            ring_level: 0.15, filter_type: 1, cutoff: 0.5, resonance: 0.6, env_amount: 0.3,
            kybd_amount: 0.4, lfo_amount: 0.4, vco1_fm1: 8.0, vco2_fm1: 8.0, lfo_rate: 4.0,
            adsr_a: 0.01, adsr_d: 0.5, adsr_s: 0.6, adsr_r: 0.5, ar_a: 0.01, ar_r: 0.4,
            vca_adsr: true,
        ..BASE } },
    // Pluck — percussive pluck/clavinet
    Program { name: "Pluck", voice: OdysseyPatch {
            vco1_pulse: true, vco1_level: 0.6, vco2_level: 0.7, vco1_pw: 0.3, vco2_pw: 0.3,
            ring_level: 0.02, filter_type: 2, cutoff: 0.1, resonance: 0.2, hpf: 0.05,
            env_amount: 0.7, kybd_amount: 0.8, lfo_rate: 1.0, adsr_a: 0.001, adsr_d: 0.12,
            adsr_s: 0.0, adsr_r: 0.08, ar_a: 0.001, ar_r: 0.1,
        ..BASE } },
    // ThkLead — fat Zawinul-style lead
    Program { name: "ThkLead", voice: OdysseyPatch {
            vco2_tune: 8.0, filter_type: 1, cutoff: 0.4, resonance: 0.15, env_amount: 0.35,
            kybd_amount: 0.5, vco1_fm1: 2.0, vco2_fm1: 2.0, lfo_rate: 5.5, portamento: 0.15,
            adsr_a: 0.01, adsr_d: 0.3, adsr_s: 0.65, adsr_r: 0.25, ar_a: 0.01, ar_r: 0.3,
        ..BASE } },
    // FltSwp — Vince Clarke filter sweep pad
    Program { name: "FltSwp", voice: OdysseyPatch {
            vco2_pulse: true, vco2_tune: 4.0, vco1_level: 0.6, vco2_level: 0.6, cutoff: 0.3,
            resonance: 0.45, hpf: 0.05, env_amount: 0.1, kybd_amount: 0.3, lfo_amount: 0.45,
            vco1_pwm: 0.3, vco2_pwm: 0.3, lfo_rate: 0.2, portamento: 0.12, adsr_a: 0.8,
            adsr_d: 0.5, adsr_s: 0.8, adsr_r: 1.5, ar_a: 1.0, ar_r: 2.0, vca_adsr: true,
        ..BASE } },
    // NoiseHt — percussive noise burst
    Program { name: "NoiseHt", voice: OdysseyPatch {
            vco1_level: 0.3, vco2_level: 0.0, ring_level: 0.8, filter_type: 2, cutoff: 0.8,
            resonance: 0.1, hpf: 0.15, env_amount: 0.6, lfo_rate: 1.0, adsr_a: 0.001,
            adsr_d: 0.08, adsr_s: 0.0, adsr_r: 0.05, ar_a: 0.001, ar_r: 0.06,
        ..BASE } },
    // Duo — duophonic split: low voice bass + high voice lead (George Duke style)
    // Two saws detuned an octave apart, moderate filter for body
    Program { name: "Duo", voice: OdysseyPatch {
            vco2_tune: 1200.0, vco1_level: 0.9, vco2_level: 0.7, filter_type: 1,
            cutoff: 0.35, resonance: 0.2, env_amount: 0.4, kybd_amount: 0.5, vco1_fm1: 1.5,
            vco2_fm1: 1.5, lfo_rate: 5.0, portamento: 0.09, adsr_a: 0.005, adsr_d: 0.3,
            adsr_s: 0.5, adsr_r: 0.2, ar_a: 0.005, ar_r: 0.25,
        ..BASE } },
    // SnarDrm — snare drum synthesis (noise burst + pitched VCO body)
    Program { name: "SnarDrm", voice: OdysseyPatch {
            vco1_level: 0.5, vco2_level: 0.0, ring_level: 0.7, filter_type: 2, cutoff: 0.55,
            resonance: 0.15, hpf: 0.1, env_amount: 0.8, lfo_rate: 1.0, adsr_a: 0.001,
            adsr_d: 0.12, adsr_s: 0.0, adsr_r: 0.08, ar_a: 0.001, ar_r: 0.1,
        ..BASE } },
    // Kick — deep thud via self-oscillating filter as tone source
    Program { name: "Kick", voice: OdysseyPatch {
            vco1_level: 0.3, vco2_level: 0.0, ring_level: 0.1, filter_type: 1, cutoff: 0.08,
            resonance: 0.95, env_amount: 0.4, lfo_rate: 1.0, adsr_a: 0.001, adsr_d: 0.15,
            adsr_s: 0.0, adsr_r: 0.1, ar_a: 0.001, ar_r: 0.12,
        ..BASE } },
    // Rezz — high-resonance sweep, Larry Fast / Synergy style
    Program { name: "Rezz", voice: OdysseyPatch {
            vco2_tune: 7.0, vco1_level: 0.7, vco2_level: 0.7, filter_type: 1, cutoff: 0.2,
            resonance: 0.8, env_amount: 0.85, kybd_amount: 0.3, lfo_amount: 0.15,
            lfo_rate: 0.3, adsr_a: 0.01, adsr_d: 0.8, adsr_s: 0.1, adsr_r: 0.5, ar_a: 0.01,
            ar_r: 0.6, vca_adsr: true,
        ..BASE } },
    // Squelch — acid-style squelchy bass (TB-303 territory on Odyssey)
    Program { name: "Squelch", voice: OdysseyPatch {
            vco1_level: 0.9, vco2_level: 0.0, filter_type: 1, cutoff: 0.12, resonance: 0.75,
            env_amount: 0.9, kybd_amount: 0.4, lfo_rate: 1.0, portamento: 0.06,
            adsr_a: 0.001, adsr_d: 0.15, adsr_s: 0.0, adsr_r: 0.08, ar_a: 0.001, ar_r: 0.12,
        ..BASE } },
    // Growl — aggressive detuned sync lead (Edgar Winter style)
    Program { name: "Growl", voice: OdysseyPatch {
            vco2_tune: 15.0, vco1_level: 0.5, vco2_level: 1.0, sync: true, filter_type: 2,
            cutoff: 0.3, resonance: 0.35, env_amount: 0.6, kybd_amount: 0.4, lfo_amount: 0.2,
            vco1_fm1: 4.0, vco2_fm1: 4.0, lfo_rate: 6.0, portamento: 0.075, adsr_a: 0.01,
            adsr_d: 0.35, adsr_s: 0.6, adsr_r: 0.2, ar_a: 0.01, ar_r: 0.25,
        ..BASE } },
    // Wind — breathy texture (noise through resonant filter with slow sweep)
    Program { name: "Wind", voice: OdysseyPatch {
            vco1_level: 0.1, vco2_level: 0.0, ring_level: 0.9, cutoff: 0.3, resonance: 0.5,
            hpf: 0.1, env_amount: 0.15, kybd_amount: 0.2, lfo_amount: 0.35, lfo_rate: 0.2,
            adsr_a: 0.8, adsr_d: 0.5, adsr_s: 0.6, adsr_r: 1.5, ar_a: 1.0, ar_r: 2.0,
            vca_adsr: true,
        ..BASE } },
    // WahBass — auto-wah bass (Stevie Wonder / Herbie Hancock)
    // High env_mod with short decay creates wah-like filter sweep on each note
    Program { name: "WahBass", voice: OdysseyPatch {
            vco2_pulse: true, vco2_tune: -5.0, vco1_level: 0.9, vco2_level: 0.6,
            vco1_pw: 0.35, vco2_pw: 0.35, filter_type: 1, cutoff: 0.15, resonance: 0.55,
            env_amount: 0.8, kybd_amount: 0.5, lfo_rate: 1.0, adsr_a: 0.005, adsr_d: 0.2,
            adsr_s: 0.15, adsr_r: 0.12, ar_a: 0.005, ar_r: 0.2,
        ..BASE } },
    // Stab — short rhythmic stab (new wave / Ultravox style)
    Program { name: "Stab", voice: OdysseyPatch {
            vco1_pulse: true, vco2_pulse: true, vco2_tune: 3.0, vco1_pw: 0.45, vco2_pw: 0.45,
            filter_type: 2, cutoff: 0.5, resonance: 0.2, hpf: 0.05, env_amount: 0.5,
            kybd_amount: 0.5, lfo_rate: 1.0, adsr_a: 0.001, adsr_d: 0.1, adsr_s: 0.0,
            adsr_r: 0.06, ar_a: 0.001, ar_r: 0.08,
        ..BASE } },
    // Buzz — harsh PWM texture (industrial / Throbbing Gristle territory)
    Program { name: "Buzz", voice: OdysseyPatch {
            vco1_pulse: true, vco2_pulse: true, vco2_tune: 10.0, vco1_level: 0.7,
            vco2_level: 0.7, vco1_pw: 0.15, vco2_pw: 0.15, filter_type: 2, cutoff: 0.55,
            resonance: 0.3, env_amount: 0.25, kybd_amount: 0.4, vco1_pwm: 0.6, vco2_pwm: 0.6,
            lfo_rate: 3.5, adsr_a: 0.01, adsr_d: 0.3, adsr_s: 0.7, adsr_r: 0.3, ar_a: 0.01,
            ar_r: 0.3, vca_adsr: true,
        ..BASE } },
    // Flute — gentle, breathy flute (prog rock, Genesis-era)
    Program { name: "Flute", voice: OdysseyPatch {
            vco1_level: 0.6, vco2_level: 0.0, ring_level: 0.08, cutoff: 0.35, resonance: 0.1,
            hpf: 0.12, env_amount: 0.15, kybd_amount: 0.8, vco1_fm1: 2.0, vco2_fm1: 2.0,
            lfo_rate: 5.5, portamento: 0.045, adsr_a: 0.08, adsr_d: 0.2, adsr_s: 0.7,
            adsr_r: 0.15, ar_a: 0.08, ar_r: 0.15, vca_adsr: true,
        ..BASE } },
    // Trem — tremolo lead (Tangerine Dream / Klaus Schulze style)
    Program { name: "Trem", voice: OdysseyPatch {
            vco2_pulse: true, vco2_tune: 5.0, vco1_level: 0.7, vco2_level: 0.5, vco1_pw: 0.4,
            vco2_pw: 0.4, cutoff: 0.45, resonance: 0.2, env_amount: 0.3, kybd_amount: 0.5,
            lfo_amount: 0.5, lfo_rate: 7.0, adsr_a: 0.01, adsr_d: 0.3, adsr_s: 0.6,
            adsr_r: 0.3, ar_a: 0.01, ar_r: 0.3, vca_adsr: true,
        ..BASE } },
    // Siren — rising pitch emergency siren effect
    Program { name: "Siren", voice: OdysseyPatch {
            vco2_level: 0.0, cutoff: 0.6, resonance: 0.15, env_amount: 0.2, kybd_amount: 0.5,
            vco1_fm1: 50.0, vco2_fm1: 50.0, lfo_rate: 1.5, portamento: 0.45, adsr_a: 0.01,
            adsr_d: 0.3, adsr_s: 0.8, adsr_r: 0.5, ar_a: 0.01, ar_r: 0.5, vca_adsr: true,
        ..BASE } },
    // Brass — punchy brass section (Herbie Hancock "Headhunters" era)
    Program { name: "Brass", voice: OdysseyPatch {
            vco2_tune: 6.0, filter_type: 1, cutoff: 0.2, resonance: 0.15, env_amount: 0.7,
            kybd_amount: 0.5, lfo_rate: 1.0, adsr_a: 0.02, adsr_d: 0.15, adsr_s: 0.55,
            adsr_r: 0.15, ar_a: 0.02, ar_r: 0.15,
        ..BASE } },
    // Organ — cheesy combo organ (drawbar-esque pulse waves)
    Program { name: "Organ", voice: OdysseyPatch {
            vco1_pulse: true, vco2_pulse: true, vco2_tune: 1200.0, vco1_level: 0.7,
            vco2_level: 0.5, cutoff: 0.6, resonance: 0.05, hpf: 0.05, env_amount: 0.05,
            kybd_amount: 0.6, vco1_fm1: 1.0, vco2_fm1: 1.0, lfo_rate: 6.5, adsr_a: 0.005,
            adsr_d: 0.1, adsr_s: 0.9, adsr_r: 0.05, ar_a: 0.005, ar_r: 0.05,
        ..BASE } },
    // ── New patches ──────────────────────────────────────────────
    //
    // Sources:
    //   [PB81]  ARP Odyssey Patch Book, ARP Instruments Inc., 1981
    //   [OM76]  ARP Odyssey MkII Owner's Manual, 1976
    //   [KPB]   Korg ARP Odyssey Patchbook, 2017 (100 modern patches)
    //   [SS]    Gordon Reid, "Synth Secrets", Sound on Sound, 1999-2004
    //   [808]   Roland TR-808 circuit analysis (bridged-T tom/conga design)
    //   [SOS]   Sound on Sound practical synthesis articles
    // Conga — analog conga drum
    // Source: [PB81] "Bigger Bass Drum & Tom Tom Solo" adapted for conga range;
    // [808] bridged-T conga circuit: sine-like oscillator, fast pitch drop,
    // ~400ms decay, no noise (congas are cleaner than toms), bandpass-like
    // filtering via resonance. Played in upper register for conga pitch.
    Program { name: "Conga", voice: OdysseyPatch {
            vco1_level: 1.0, vco2_level: 0.0, filter_type: 2, cutoff: 0.35, resonance: 0.55,
            hpf: 0.1, env_amount: 0.65, kybd_amount: 0.8, lfo_rate: 1.0, adsr_a: 0.001,
            adsr_d: 0.4, adsr_s: 0.0, adsr_r: 0.3, ar_a: 0.001, ar_r: 0.35,
        ..BASE } },
    // Tom — analog tom drum
    // Source: [808] TR-808 tom circuit: same bridged-T oscillator as conga but
    // with added pink noise for body; [PB81] "Bigger Bass Drum & Tom Tom Solo"
    // patch uses single VCO, fast ADSR decay, moderate noise, resonant filter.
    // Longer decay than conga, noise adds the characteristic 808 tom "thud".
    Program { name: "Tom", voice: OdysseyPatch {
            vco1_level: 1.0, vco2_level: 0.0, ring_level: 0.25, filter_type: 2, cutoff: 0.3,
            resonance: 0.45, hpf: 0.05, env_amount: 0.7, kybd_amount: 0.7, lfo_rate: 1.0,
            adsr_a: 0.001, adsr_d: 0.5, adsr_s: 0.0, adsr_r: 0.4, ar_a: 0.001, ar_r: 0.45,
        ..BASE } },
    // Clap — synthetic hand clap
    // Source: [808] TR-808 clap circuit: filtered noise burst with fast
    // repeating envelope to simulate multiple hands; [KPB] modern percussion
    // patches use noise through bandpass (resonant LPF + HPF). Short burst
    // with ~200ms decay, mid-band filtered noise, slight reverb-like tail.
    Program { name: "Clap", voice: OdysseyPatch {
            vco1_pulse: true, vco2_pulse: true, vco1_level: 0.0, vco2_level: 0.0,
            ring_level: 1.0, cutoff: 0.45, resonance: 0.35, hpf: 0.2, env_amount: 0.3,
            lfo_rate: 1.0, adsr_a: 0.001, adsr_d: 0.2, adsr_s: 0.0, adsr_r: 0.15,
            ar_a: 0.001, ar_r: 0.2,
        ..BASE } },
    // PWMBas — pulse-width modulation bass
    // Source: [KPB] Korg 2017 patchbook "PWM Bass" category; [OM76] owner's
    // manual demonstrates PWM using LFO→pulse width for animated bass.
    // Two pulse VCOs slightly detuned, LFO modulates pulse width for
    // the characteristic hollow-to-thin cycling movement. 4035 Moog filter
    // for deep, round low end typical of Odyssey PWM bass patches.
    Program { name: "PWMBas", voice: OdysseyPatch {
            vco1_pulse: true, vco2_pulse: true, vco2_tune: -5.0, vco1_pw: 0.35,
            vco2_pw: 0.35, filter_type: 1, cutoff: 0.22, resonance: 0.4, env_amount: 0.55,
            kybd_amount: 0.3, vco1_pwm: 0.6, vco2_pwm: 0.6, lfo_rate: 2.5, adsr_a: 0.005,
            adsr_d: 0.3, adsr_s: 0.3, adsr_r: 0.15, ar_a: 0.005, ar_r: 0.2,
        ..BASE } },
    // Violin — solo violin
    // Source: [PB81] "Solo Violin" patch; [SS] Gordon Reid "Synthesizing
    // Bowed Strings" (SoS): sawtooth wave, body resonances at 300-700Hz,
    // gentle HF roll-off (~9dB/oct above mid), delayed vibrato at ~5-6Hz,
    // slow attack ~80-120ms to simulate bow grab. [SOS] "Practical Bowed-
    // String Synthesis": Korg 700 violin used sawtooth 4', modest vibrato,
    // tiny portamento. Single saw VCO, low-pass filtering to tame
    // brightness, moderate resonance for body, slow attack for bow.
    Program { name: "Violin", voice: OdysseyPatch {
            vco1_level: 1.0, vco2_level: 0.0, cutoff: 0.4, resonance: 0.2, hpf: 0.08,
            env_amount: 0.15, kybd_amount: 0.6, vco1_fm1: 2.5, vco2_fm1: 2.5, lfo_rate: 5.5,
            portamento: 0.075, adsr_a: 0.1, adsr_d: 0.2, adsr_s: 0.8, adsr_r: 0.2,
            ar_a: 0.08, ar_r: 0.2, vca_adsr: true,
        ..BASE } },
    // Oboe — nasal reed tone
    // Source: [SS] Gordon Reid "Synth Secrets" series on woodwinds: oboe
    // timbre comes from narrow pulse width (~10-15%), creating the
    // characteristic "nasal" or "pinched" quality. [OM76] Odyssey manual
    // demonstrates pulse width for reed instruments. [PB81] "Clarinet &
    // English Horn Duo" uses narrow pulse. Narrow PW, moderate filter
    // with some resonance for the nasal peak, slow-ish attack for breath.
    Program { name: "Oboe", voice: OdysseyPatch {
            vco1_pulse: true, vco2_pulse: true, vco2_tune: 2.0, vco1_level: 0.9,
            vco2_level: 0.3, vco1_pw: 0.15, vco2_pw: 0.15, ring_level: 0.05, cutoff: 0.38,
            resonance: 0.3, hpf: 0.05, env_amount: 0.2, kybd_amount: 0.6, vco1_fm1: 2.0,
            vco2_fm1: 2.0, lfo_rate: 5.0, adsr_a: 0.06, adsr_d: 0.15, adsr_s: 0.75,
            adsr_r: 0.12, ar_a: 0.05, ar_r: 0.12, vca_adsr: true,
        ..BASE } },
    // Alarm — emergency siren (two-tone)
    // Source: [PB81] "Italian Siren" patch: uses LFO to modulate pitch of
    // single VCO at ~2Hz rate for the characteristic European two-tone
    // emergency siren. Square wave for harsh, cutting tone that carries.
    // High filter cutoff, no resonance, fast gate for continuous tone.
    Program { name: "Alarm", voice: OdysseyPatch {
            vco1_pulse: true, vco2_pulse: true, vco1_level: 1.0, vco2_level: 0.0,
            cutoff: 0.7, resonance: 0.05, env_amount: 0.0, kybd_amount: 0.3, vco1_fm1: 35.0,
            vco2_fm1: 35.0, lfo_rate: 2.0, adsr_a: 0.005, adsr_d: 0.1, adsr_s: 1.0,
            adsr_r: 0.05, ar_a: 0.005, ar_r: 0.05,
        ..BASE } },
    // Robot — metallic robotic voice
    // Source: [PB81] ring mod patches; ring mod between two VCOs at
    // non-harmonic intervals creates inharmonic, metallic, "robotic"
    // timbres. Detuned by ~minor 3rd (300 cents) for dissonant clang.
    // Pulse waves through ring mod with resonant filter for vowel-like
    // formant. Classic Odyssey technique documented in owner's manual
    // ring modulator section.
    Program { name: "Robot", voice: OdysseyPatch {
            vco1_pulse: true, vco2_pulse: true, vco2_tune: 300.0, vco1_level: 0.0,
            vco2_level: 0.3, vco1_pw: 0.4, vco2_pw: 0.4, ring_mod: true, ring_level: 0.9,
            cutoff: 0.35, resonance: 0.5, env_amount: 0.3, kybd_amount: 0.4,
            lfo_amount: 0.15, vco1_pwm: 0.2, vco2_pwm: 0.2, lfo_rate: 3.0, adsr_a: 0.01,
            adsr_d: 0.2, adsr_s: 0.6, adsr_r: 0.15, ar_a: 0.01, ar_r: 0.15, vca_adsr: true,
        ..BASE } },
    // Whstlr — "Beginning Whistler"
    // Source: [PB81] "Beginning Whistler" patch: self-oscillating filter
    // (high resonance) creates a pure sine-like whistle tone. No VCO
    // audio — sound comes entirely from the resonant filter peak. ADSR
    // controls filter to create pitch contour. Key follow at maximum
    // so filter tracks keyboard. Slow LFO vibrato for human quality.
    Program { name: "Whstlr", voice: OdysseyPatch {
            vco1_level: 0.0, vco2_level: 0.0, ring_level: 0.08, cutoff: 0.5, resonance: 0.95,
            env_amount: 0.1, kybd_amount: 1.0, lfo_amount: 0.03, lfo_rate: 5.0,
            portamento: 0.12, adsr_a: 0.08, adsr_d: 0.1, adsr_s: 0.9, adsr_r: 0.15,
            ar_a: 0.06, ar_r: 0.15, vca_adsr: true,
        ..BASE } },
    // Choir — soprano choir pad
    // Source: [PB81] "Choir Soprano" patch: two detuned sawtooth waves
    // for chorus effect, pulse width modulation to simulate vowel
    // movement, slow attack for breath onset, HPF to remove muddiness.
    // [SS] choir synthesis uses detuned saws + PWM for formant animation.
    Program { name: "Choir", voice: OdysseyPatch {
            vco2_pulse: true, vco2_tune: 7.0, vco1_level: 0.7, vco2_level: 0.6, vco1_pw: 0.4,
            vco2_pw: 0.4, ring_level: 0.05, cutoff: 0.42, resonance: 0.15, hpf: 0.08,
            env_amount: 0.15, kybd_amount: 0.5, lfo_amount: 0.08, vco1_fm1: 1.0,
            vco2_fm1: 1.0, vco1_pwm: 0.35, vco2_pwm: 0.35, lfo_rate: 3.5, adsr_a: 0.3,
            adsr_d: 0.3, adsr_s: 0.75, adsr_r: 0.4, ar_a: 0.25, ar_r: 0.4, vca_adsr: true,
        ..BASE } },
    // Sitar — "High Voltage Sitar"
    // Source: [PB81] "High Voltage Sitar" patch: uses oscillator sync
    // with resonant filter to create the buzzy, twangy sitar-like
    // harmonics. Sync creates the metallic overtone series; high
    // resonance adds the nasal "bridge buzz" quality. Fast attack,
    // medium decay for plucked string character. VCO2 slightly above
    // VCO1 for the sync sweep.
    Program { name: "Sitar", voice: OdysseyPatch {
            vco2_tune: 50.0, vco1_level: 0.0, vco2_level: 1.0, sync: true, cutoff: 0.45,
            resonance: 0.6, env_amount: 0.5, kybd_amount: 0.7, vco1_fm1: 1.5, vco2_fm1: 1.5,
            lfo_rate: 5.5, adsr_a: 0.003, adsr_d: 0.6, adsr_s: 0.15, adsr_r: 0.3,
            ar_a: 0.003, ar_r: 0.4,
        ..BASE } },
    // TrmTub — trombone/tuba brass
    // Source: [PB81] "Trombone/Tuba" patch; [SS] Gordon Reid brass
    // synthesis: sawtooth waves, ~50ms attack for lip settling, filter
    // opens with loudness (env_mod), resonance proportional to amplitude.
    // Two detuned saws for section thickness. 4035 Moog ladder filter
    // for the fat brass quality. Key follow moderate — lower notes
    // should be darker. 5Hz vibrato per Reid's recommendation.
    Program { name: "TrmTub", voice: OdysseyPatch {
            vco2_tune: 5.0, vco1_level: 0.9, vco2_level: 0.7, filter_type: 1, cutoff: 0.18,
            resonance: 0.2, env_amount: 0.65, kybd_amount: 0.4, vco1_fm1: 1.5, vco2_fm1: 1.5,
            lfo_rate: 5.0, portamento: 0.03, adsr_a: 0.05, adsr_d: 0.2, adsr_s: 0.6,
            adsr_r: 0.15, ar_a: 0.04, ar_r: 0.15, vca_adsr: true,
        ..BASE } },
    // Mrmbas — marimba with echo
    // Source: [PB81] "Marimba w/Echo" patch: uses resonant filter to
    // create the tuned wooden bar resonance, very fast attack and
    // medium decay for the struck bar character. Key follow high so
    // filter tracks pitch. HPF removes low-frequency mud. Single VCO
    // pulse wave — the hollow quality mimics wooden resonator. LFO
    // modulates filter subtly for the "echo" tremolo effect.
    Program { name: "Mrmbas", voice: OdysseyPatch {
            vco1_pulse: true, vco2_pulse: true, vco1_level: 0.9, vco2_level: 0.0,
            vco1_pw: 0.45, vco2_pw: 0.45, cutoff: 0.5, resonance: 0.35, hpf: 0.05,
            env_amount: 0.4, kybd_amount: 0.8, lfo_amount: 0.12, lfo_rate: 8.0,
            adsr_a: 0.001, adsr_d: 0.45, adsr_s: 0.0, adsr_r: 0.3, ar_a: 0.001, ar_r: 0.35,
            vca_adsr: true,
        ..BASE } },
    // Thremn — theremin
    // Source: [PB81] "Theremin" patch; classic Odyssey theremin technique:
    // single sawtooth VCO through open filter, maximum portamento for the
    // characteristic sliding pitch, sine-like vibrato at ~6Hz for the
    // wavering quality. The Odyssey was widely used for theremin effects.
    // [OM76] portamento section demonstrates this technique. High key
    // follow, wide-open filter, ADSR controls amplitude.
    Program { name: "Thremn", voice: OdysseyPatch {
            vco1_level: 1.0, vco2_level: 0.0, cutoff: 0.55, resonance: 0.1, env_amount: 0.1,
            kybd_amount: 0.7, vco1_fm1: 4.0, vco2_fm1: 4.0, lfo_rate: 6.0, portamento: 0.6,
            adsr_a: 0.08, adsr_d: 0.1, adsr_s: 0.9, adsr_r: 0.2, ar_a: 0.06, ar_r: 0.2,
            vca_adsr: true,
        ..BASE } },
];

// ── PolyBLEP anti-aliasing ──

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

/// A Padé approximant of `tanh`, the same one the other two analog
/// instruments saturate with. It tends to x/9 rather than to 1, which is
/// enough of a limiter for a feedback loop and cheaper than the real thing.
#[inline]
fn tanh_approx(x: f64) -> f64 {
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// The gentler saturation the Norton op-amps in the Rev 3 filter contribute.
#[inline]
fn soft_clip(x: f64) -> f64 {
    x / (1.0 + x.abs())
}

// ── VCO ──

#[derive(Debug, Clone)]
struct OdysseyVco {
    phase: f64,
    dt: f64,
}

impl OdysseyVco {
    fn new() -> Self {
        Self { phase: 0.0, dt: 0.01 }
    }

    fn set_freq(&mut self, freq: f64, sr: f64) {
        self.dt = freq.clamp(0.01, sr * 0.45) / sr;
    }

    /// One sample. Returns the sawtooth, the pulse, and whether the ramp
    /// restarted this sample — which is what hard sync needs.
    fn tick(&mut self, pulse_width: f64) -> (f64, f64, bool) {
        let dt = self.dt;
        self.phase += dt;
        let reset = self.phase >= 1.0;
        if reset {
            self.phase -= 1.0;
        }
        let t = self.phase;

        let saw = 2.0 * t - 1.0 - poly_blep(t, dt);

        let pw = pulse_width.clamp(0.02, 0.98);
        let mut pulse = if t < pw { 1.0 } else { -1.0 };
        pulse += poly_blep(t, dt);
        pulse -= poly_blep((t - pw).rem_euclid(1.0), dt);

        (saw, pulse, reset)
    }

    fn reset_phase(&mut self) {
        self.phase = 0.0;
    }
}

// ── Noise generator ──
//
// White or pink, as the switch at the far left of the panel says. The pink
// filter is Paul Kellett's three-pole economy version, trimmed so that the
// two colours leave the generator at the same RMS — the switch changes the
// colour and not the level, which is what a fader in front of it needs.

const PINK_TRIM: f64 = 0.334;

#[derive(Debug, Clone)]
struct NoiseGen {
    state: u32,
    pink: [f64; 3],
}

impl NoiseGen {
    fn new() -> Self {
        Self { state: 12345, pink: [0.0; 3] }
    }

    fn white(&mut self) -> f64 {
        self.state = self.state.wrapping_mul(1_103_515_245).wrapping_add(12345);
        f64::from(self.state as i32) / f64::from(i32::MAX)
    }

    fn pink(&mut self) -> f64 {
        let w = self.white();
        self.pink[0] = 0.997_65 * self.pink[0] + w * 0.099_046_0;
        self.pink[1] = 0.963_00 * self.pink[1] + w * 0.296_516_4;
        self.pink[2] = 0.570_00 * self.pink[2] + w * 1.052_691_3;
        (self.pink[0] + self.pink[1] + self.pink[2] + w * 0.184_8) * PINK_TRIM
    }
}

// ── Filters ──
//
// One low-pass per revision, all three on the panel's single 16 Hz - 16 kHz
// legend because the reissue prints one legend and one switch decides which
// board is in circuit. What differs between them is the slope, the
// nonlinearity and what the resonance costs the passband — which is what the
// three are actually known for.
//
// All three integrate in the topology-preserving (`g/(1+g)`) form, so a
// section's pole lands on the frequency it was asked for. The four-pole pair
// used the naive `s += g*(x-s)` form, which sits an octave below its own
// coefficient and takes the whole cascade with it: the slider marked 632 Hz
// measured its -3 dB point at 141 Hz where four correctly-placed poles put it
// at 275.

/// Where on the resonance travel the loop stops losing and starts producing.
/// The panel legend reads "MIN…SELF OSC", so the top of the slider has to
/// oscillate rather than merely ring.
const SELF_OSC_KNEE: f64 = 0.9;

/// ARP 4023 — the Rev 1 board. Two poles, 12 dB/octave, and no bass lost at
/// resonance: this is the state-variable topology, whose low-pass output has
/// unity gain at DC whatever the damping is doing. The brightest of the three
/// and the one the instrument is remembered for.
#[derive(Debug, Clone)]
struct Filter4023 {
    ic1: f64,
    ic2: f64,
}

/// How hard the filter's own amplitude damps the loop once the resonance has
/// taken the damping negative. The oscillation settles where the two cancel,
/// which is what stops a filter past its knee from running away and is
/// cheaper than putting a saturator inside the implicit solve.
const OSC_LIMIT: f64 = 0.25;

impl Filter4023 {
    fn new() -> Self {
        Self { ic1: 0.0, ic2: 0.0 }
    }

    fn process(&mut self, input: f64, cutoff_norm: f64, resonance: f64, sr: f64) -> f64 {
        let g = (std::f64::consts::PI * cutoff_hz(cutoff_norm).min(sr * 0.49) / sr).tan();
        let k = 2.0 * (1.0 - resonance.clamp(0.0, 1.0) / SELF_OSC_KNEE)
            + OSC_LIMIT * self.ic1 * self.ic1;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let v3 = input - self.ic2;
        let v1 = a1 * (self.ic1 + g * v3);
        let v2 = self.ic2 + g * v1;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        if self.ic1.abs() < 1e-18 { self.ic1 = 0.0; }
        if self.ic2.abs() < 1e-18 { self.ic2 = 0.0; }
        v2
    }

    fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    /// Start a note with the filter already ringing, if the resonance is far
    /// enough up its travel for the loop to keep it going. A filter with no
    /// state and no input stays silent forever however negative its damping
    /// is, and two presets in the bank have no oscillator at all.
    fn start(&mut self, resonance: f64) {
        let past = (resonance - SELF_OSC_KNEE) / (1.0 - SELF_OSC_KNEE);
        self.ic1 = past.clamp(0.0, 1.0) * SELF_OSC_SEED;
    }
}

/// How much state a note starts the filter with when the resonance is at the
/// top of its travel.
const SELF_OSC_SEED: f64 = 0.05;

/// How much feedback the two four-pole boards can be given. Four
/// correctly-placed poles reach a half turn of phase with a quarter of the
/// gain left, so 4.0 is exactly marginal and the top of the travel has to sit
/// past it for the panel's "SELF OSC" to mean anything.
const LADDER_RES_MAX: f64 = 4.5;

/// ARP 4035 — the Rev 2 board, a four-pole transistor ladder. 24 dB/octave,
/// saturating in the loop, and no compensation on the input, so the bass
/// drops away as the resonance comes up. That loss is the ladder's signature
/// and the reason this revision is the warmest of the three.
#[derive(Debug, Clone)]
struct Filter4035 {
    s: [f64; 4],
}

impl Filter4035 {
    fn new() -> Self {
        Self { s: [0.0; 4] }
    }

    fn process(&mut self, input: f64, cutoff_norm: f64, resonance: f64, sr: f64) -> f64 {
        let g = (std::f64::consts::PI * cutoff_hz(cutoff_norm).min(sr * 0.49) / sr).tan();
        let gg = g / (1.0 + g);
        let res = resonance.clamp(0.0, 1.0) * LADDER_RES_MAX;
        let mut x = tanh_approx(input - res * tanh_approx(self.s[3]));

        for s in &mut self.s {
            let v = (x - *s) * gg;
            let y = v + *s;
            *s = y + v;
            if s.abs() < 1e-18 { *s = 0.0; }
            x = y;
        }
        // The one number that closes the loop is worth bounding outright,
        // since a self-oscillating filter has no input to bound it.
        self.s[3] = self.s[3].clamp(-4.0, 4.0);
        x
    }

    fn reset(&mut self) {
        self.s = [0.0; 4];
    }

    fn start(&mut self, resonance: f64) {
        let past = (resonance - SELF_OSC_KNEE) / (1.0 - SELF_OSC_KNEE);
        self.s = [past.clamp(0.0, 1.0) * SELF_OSC_SEED; 4];
    }
}

/// How much of what the resonance takes out of the passband the Rev 3
/// board's input summer puts back.
const NORTON_COMPENSATION: f64 = 1.7;

/// ARP 4075 — the Rev 3 board, four Norton op-amp integrators round a
/// feedback path rather than a ladder. Also 24 dB/octave, with the gentler
/// clipping of a Norton input stage, which is why this revision is described
/// as the most controlled of the three.
///
/// The ladder's bass loss at resonance is documented — it is what a
/// transistor ladder does. Nothing to hand says how much of it this board
/// keeps, so its input is compensated part of the way and it lands between
/// the other two: the Rev 1 keeps its bass outright, the Rev 2 loses it, and
/// the Rev 3 sits in the middle, which is the order players put them in.
#[derive(Debug, Clone)]
struct Filter4075 {
    s: [f64; 4],
}

impl Filter4075 {
    fn new() -> Self {
        Self { s: [0.0; 4] }
    }

    fn process(&mut self, input: f64, cutoff_norm: f64, resonance: f64, sr: f64) -> f64 {
        let g = (std::f64::consts::PI * cutoff_hz(cutoff_norm).min(sr * 0.49) / sr).tan();
        let gg = g / (1.0 + g);
        let r = resonance.clamp(0.0, 1.0);
        let res = r * LADDER_RES_MAX;
        let compensation = 1.0 + r * NORTON_COMPENSATION;
        let mut x = soft_clip(input * compensation - res * soft_clip(self.s[3]));

        for s in &mut self.s {
            let v = (x - *s) * gg;
            let y = v + *s;
            *s = y + v;
            if s.abs() < 1e-18 { *s = 0.0; }
            x = y;
        }
        self.s[3] = self.s[3].clamp(-4.0, 4.0);
        x
    }

    fn reset(&mut self) {
        self.s = [0.0; 4];
    }

    fn start(&mut self, resonance: f64) {
        let past = (resonance - SELF_OSC_KNEE) / (1.0 - SELF_OSC_KNEE);
        self.s = [past.clamp(0.0, 1.0) * SELF_OSC_SEED; 4];
    }
}

/// The high-pass ahead of the low-pass, as on the voice board: one pole,
/// non-resonant, 6 dB/octave, topology-preserving like the low-pass sections
/// and for the same reason — the difference-equation form it replaced put its
/// corner an octave and a bit out at the top of the sweep, 6.8 kHz where the
/// slider said 16.
#[derive(Debug, Clone)]
struct HpFilter {
    state: f64,
}

impl HpFilter {
    fn new() -> Self {
        Self { state: 0.0 }
    }

    fn process(&mut self, input: f64, cutoff_norm: f64, sr: f64) -> f64 {
        let g = (std::f64::consts::PI * hpf_hz(cutoff_norm).min(sr * 0.49) / sr).tan();
        let v = (input - self.state) * g / (1.0 + g);
        let lp = v + self.state;
        self.state = lp + v;
        if self.state.abs() < 1e-18 { self.state = 0.0; }
        input - lp
    }

    fn reset(&mut self) {
        self.state = 0.0;
    }
}

// ── Envelope generators ──
//
// One type, used twice: the ADSR, and the AR, which ARP describe as the same
// generator with its sustain fixed at maximum. So the AR rises to peak, holds
// there for as long as the key is down and falls when it is released — which
// is the defect at the centre of this rewrite. The AR here went to its release
// segment the instant the attack finished, so every note on the twenty-three
// presets whose amplifier follows it died at the release rate whatever the
// player did, and the ADSR's decay could not change the length of a note.
//
// Every segment is a capacitor charging towards something, which is why none
// of them are straight lines:
//
// * attack charges towards 1.58 and the stage ends when it passes 1.0, so the
//   segment is the first time constant of an exponential;
// * decay and release charge towards slightly past their target and stop when
//   they reach it, which is 3.5 time constants across the segment. That is
//   the shape measured on a Juno-60, and it is what makes the slider's number
//   the time the segment actually takes.
//
// The defect that replaced: every segment used its slider's seconds as a
// one-pole *time constant* and then ran until it was within 0.001 of the
// target, which is 6.9 of them. A segment took seven times the time it
// advertised.

#[derive(Debug, Clone, Copy, PartialEq)]
enum EnvStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

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
    if seconds <= 0.0 {
        return 1.0;
    }
    (1.0 - (-constants / (seconds * sr)).exp()).min(1.0)
}

#[derive(Debug, Clone)]
struct OdysseyEnvelope {
    stage: EnvStage,
    level: f64,
    aim: f64,
    attack: f64,
    decay: f64,
    sustain: f64,
    release: f64,
    /// Per-sample coefficients for the three timed segments, recomputed only
    /// when a slider moves. The exponential in `env_rate` is not something to
    /// evaluate six times a sample for an answer that changes when a finger
    /// does.
    rates: [f64; 3],
    sample_rate: f64,
}

impl OdysseyEnvelope {
    fn new(sr: f64) -> Self {
        let mut env = Self {
            stage: EnvStage::Idle,
            level: 0.0,
            aim: 0.0,
            attack: 0.005,
            decay: 0.3,
            sustain: 0.5,
            release: 0.15,
            rates: [0.0; 3],
            sample_rate: sr,
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

    /// Follow the time sliders. A no-op unless one of them moved, which is
    /// what keeps the exponentials off the per-sample path.
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

    fn kill(&mut self) {
        self.stage = EnvStage::Idle;
        self.level = 0.0;
    }

    fn is_active(&self) -> bool {
        self.stage != EnvStage::Idle
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

// ── LFO ──
//
// One per instrument, free-running, sine and square out of the same phase, as
// on the panel: the FM switches choose which of the two a destination gets.
// It used to be a voice field that only advanced while a note was sounding,
// so every note started with the LFO at the same phase and a tremolo was
// welded to the note-on.

#[derive(Debug, Clone)]
struct OdysseyLfo {
    phase: f64,
    rate: f64,
}

impl OdysseyLfo {
    fn new() -> Self {
        Self { phase: 0.0, rate: 1.0 }
    }

    /// Returns the sine and the square, which come out of the same phase:
    /// the FM and PWM switches choose which of the two a destination gets.
    fn tick(&mut self, sr: f64) -> (f64, f64) {
        self.phase += self.rate / sr;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let sine = (self.phase * TWO_PI).sin();
        let square = if self.phase < 0.5 { 1.0 } else { -1.0 };
        (sine, square)
    }
}

// ── Sample and hold ──
//
// The mixer's sum is a modulation source in its own right — the panel routes
// "S/H MIXER OR PEDAL" to VCO-2's frequency and to the filter without going
// through the hold at all, which is how the instrument does audio-rate
// modulation — and the hold samples that same sum on the clock's edge. The
// lag slider slews the staircase.
//
// It used to sample the noise generator and nothing else, on the LFO and
// nothing else, through a fixed half-sample lag, and its output was scaled by
// the LFO's *own* depth sliders. So there was no way to ask for vibrato
// without also getting a random staircase twice as deep, and twenty-four of
// the forty-four presets were getting one.

#[derive(Debug, Clone)]
struct SampleAndHold {
    held: f64,
    output: f64,
    prev_trigger: bool,
}

impl SampleAndHold {
    fn new() -> Self {
        Self { held: 0.0, output: 0.0, prev_trigger: false }
    }

    fn process(&mut self, input: f64, trigger: bool, lag: f64, sr: f64) -> f64 {
        if trigger && !self.prev_trigger {
            self.held = input;
        }
        self.prev_trigger = trigger;
        let coeff = env_rate(lag, ENV_CONSTANTS, sr);
        self.output += (self.held - self.output) * coeff;
        if self.output.abs() < 1e-18 {
            self.output = 0.0;
        }
        self.output
    }

    fn reset(&mut self) {
        self.held = 0.0;
        self.output = 0.0;
        self.prev_trigger = false;
    }
}

// ── Held notes ──
//
// A fixed slab rather than a `Vec`, because a note-on arrives on the audio
// thread and the list it used to push onto was allocated for sixteen: the
// seventeenth key held at once reallocated inside the callback.

const MAX_HELD: usize = 16;

#[derive(Debug, Clone, Copy)]
struct HeldNotes {
    notes: [u8; MAX_HELD],
    len: usize,
}

impl HeldNotes {
    const fn new() -> Self {
        Self { notes: [0; MAX_HELD], len: 0 }
    }

    fn push(&mut self, note: u8) {
        if self.notes[..self.len].contains(&note) {
            return;
        }
        if self.len == MAX_HELD {
            self.notes.copy_within(1.., 0);
            self.len -= 1;
        }
        self.notes[self.len] = note;
        self.len += 1;
    }

    fn remove(&mut self, note: u8) {
        let mut out = 0;
        for i in 0..self.len {
            if self.notes[i] != note {
                self.notes[out] = self.notes[i];
                out += 1;
            }
        }
        self.len = out;
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The lowest and the highest key down, which is the duophonic split:
    /// VCO-1 takes the bottom of the chord and VCO-2 the top.
    fn split(&self) -> Option<(u8, u8)> {
        let held = &self.notes[..self.len];
        Some((*held.iter().min()?, *held.iter().max()?))
    }
}

// ── Voice ──
//
// One, because the instrument has one: one filter, one amplifier, one pair of
// envelopes. Two keys held split the oscillators, they do not make a second
// voice.

/// How far the pitch pads bend, in semitones. ARP's pads bend in proportion
/// to finger pressure and their range is a trimmer on the board; a MIDI
/// keyboard's wheel gets the two semitones that is the convention for one.
const BEND_SEMITONES: f64 = 2.0;
/// How much vibrato the centre pad — the modulation wheel — adds at full.
const PPC_VIBRATO_CENTS: f64 = 50.0;

/// The Odyssey's oscillators drift against each other by about a thirtieth of
/// a semitone at worst, which is what keeps two saws in unison from phasing
/// like one.
const DRIFT_CENTS: f64 = 1.5;

/// The bottom of the low-frequency range the keyboard switch drops an
/// oscillator into: the panel's 0.2 Hz to 20 Hz, the same span in octaves as
/// the audio range above it.
const LF_BASE_HZ: f64 = 0.2;

/// How hard the VCA's drive switch pushes, and the trim that keeps a quiet
/// signal about where it was without it.
const DRIVE_GAIN: f64 = 3.0;
const DRIVE_TRIM: f64 = 0.45;

#[derive(Debug)]
struct OdysseyVoice {
    vco1: OdysseyVco,
    vco2: OdysseyVco,
    noise: NoiseGen,
    hpf: HpFilter,
    filter_4023: Filter4023,
    filter_4035: Filter4035,
    filter_4075: Filter4075,
    adsr: OdysseyEnvelope,
    ar: OdysseyEnvelope,
    sh: SampleAndHold,
    held: HeldNotes,
    /// Pitch in octaves above 1 Hz, which is where a glide of so many seconds
    /// per octave is a straight line.
    vco1_octaves: f64,
    vco2_octaves: f64,
    vco1_target: f64,
    vco2_target: f64,
    /// The sample-and-hold mixer's two inputs, a sample old. The mixer feeds
    /// the oscillators' frequency and the oscillators feed the mixer, so
    /// something has to open the loop; on the instrument it is the hold.
    last_sh_a: f64,
    last_sh_b: f64,
    velocity: f64,
    gate: bool,
    /// Whether a note has ever been played. Portamento glides from the
    /// previous pitch, and the first note of a session has none.
    started: bool,
    /// Whether the LFO's square was high last sample, which is the clock the
    /// envelope repeat switches gate off.
    prev_lfo_high: bool,
    bend: f64,
    mod_wheel: f64,
    sample_rate: f64,
    drift_phase1: f64,
    drift_phase2: f64,
}

impl OdysseyVoice {
    fn new(sr: f64) -> Self {
        Self {
            vco1: OdysseyVco::new(),
            vco2: OdysseyVco::new(),
            noise: NoiseGen::new(),
            hpf: HpFilter::new(),
            filter_4023: Filter4023::new(),
            filter_4035: Filter4035::new(),
            filter_4075: Filter4075::new(),
            adsr: OdysseyEnvelope::new(sr),
            ar: OdysseyEnvelope::new(sr),
            sh: SampleAndHold::new(),
            held: HeldNotes::new(),
            vco1_octaves: 0.0,
            vco2_octaves: 0.0,
            vco1_target: 0.0,
            vco2_target: 0.0,
            last_sh_a: 0.0,
            last_sh_b: 0.0,
            velocity: 1.0,
            gate: false,
            started: false,
            prev_lfo_high: false,
            bend: 0.0,
            mod_wheel: 0.0,
            sample_rate: sr,
            drift_phase1: 0.0,
            drift_phase2: 0.37,
        }
    }

    fn note_on(&mut self, note: u8, vel: u8, patch: &OdysseyPatch) {
        self.velocity = f64::from(vel) / 127.0;
        self.held.push(note);
        let first_ever = !self.started;
        self.started = true;
        self.update_frequencies(patch, first_ever);

        if !self.gate {
            self.gate = true;
            self.apply_times(patch);
            self.adsr.trigger();
            self.ar.trigger();
            self.start_filter(patch);
        }
        // Legato inside a held chord does not retrigger: the Odyssey's gate
        // stays high while any key is down.
    }

    fn note_off(&mut self, note: u8, patch: &OdysseyPatch) {
        self.held.remove(note);
        if self.held.is_empty() {
            self.gate = false;
            self.adsr.release_env();
            self.ar.release_env();
        } else {
            self.update_frequencies(patch, false);
        }
    }

    /// Where the two oscillators are headed. `jump` is the first key of a
    /// phrase, which has nothing to glide from.
    fn update_frequencies(&mut self, patch: &OdysseyPatch, jump: bool) {
        let Some((low, high)) = self.held.split() else { return };
        // Sync takes VCO-2's pitch away from it, so the split with it on is
        // not a split: both oscillators follow the lowest key.
        let high = if patch.sync { low } else { high };
        self.vco1_target = note_octaves(low);
        self.vco2_target = note_octaves(high);
        if jump || patch.portamento <= 0.0 {
            self.vco1_octaves = self.vco1_target;
            self.vco2_octaves = self.vco2_target;
        }
    }

    fn apply_times(&mut self, patch: &OdysseyPatch) {
        self.adsr.set_times(patch.adsr_a, patch.adsr_d, patch.adsr_r);
        self.adsr.sustain = patch.adsr_s;
        // ARP's AR is the same generator with its sustain pinned at maximum,
        // so its decay never runs and its hold is the top of the envelope.
        self.ar.set_times(patch.ar_a, 0.0, patch.ar_r);
        self.ar.sustain = 1.0;
    }

    fn start_filter(&mut self, patch: &OdysseyPatch) {
        match patch.filter_type {
            0 => self.filter_4023.start(patch.resonance),
            1 => self.filter_4035.start(patch.resonance),
            _ => self.filter_4075.start(patch.resonance),
        }
    }

    fn kill(&mut self) {
        self.held.clear();
        self.gate = false;
        self.adsr.kill();
        self.ar.kill();
        self.hpf.reset();
        self.sh.reset();
        self.filter_4023.reset();
        self.filter_4035.reset();
        self.filter_4075.reset();
    }

    fn is_sounding(&self, patch: &OdysseyPatch) -> bool {
        // The VCA gain slider passes signal whether or not a key is down,
        // which is how the instrument drones. A voice with it up never stops.
        self.ar.is_active() || self.adsr.is_active() || patch.vca_gain > 0.0
    }

    #[allow(clippy::too_many_lines)]
    fn tick(&mut self, patch: &OdysseyPatch, lfo_sin: f64, lfo_sq: f64) -> f64 {
        // The repeat switches take the envelope's *gate* off the key and put
        // it on the LFO's square — "the pulse wave of the LFO is sent to the
        // EG, and the EG repeats the envelope cyclically", which is what
        // makes an Odyssey sequence-like without a sequencer. It only repeats
        // while a key is down.
        let lfo_high = lfo_sq > 0.0;
        let rising = lfo_high && !self.prev_lfo_high;
        let falling = !lfo_high && self.prev_lfo_high;
        self.prev_lfo_high = lfo_high;
        if self.gate {
            if patch.adsr_lfo_trig {
                if rising { self.adsr.trigger(); }
                if falling { self.adsr.release_env(); }
            }
            if patch.ar_lfo_trig {
                if rising { self.ar.trigger(); }
                if falling { self.ar.release_env(); }
            }
        }

        if !self.is_sounding(patch) {
            return 0.0;
        }

        let sr = self.sample_rate;
        self.apply_times(patch);

        // Portamento is a speed, not a time: ARP's control is in seconds per
        // octave, so glide runs at a constant rate in pitch and a wider
        // interval takes proportionally longer.
        if patch.portamento > 0.0 {
            let step = 1.0 / (patch.portamento * sr);
            for (now, target) in [
                (&mut self.vco1_octaves, self.vco1_target),
                (&mut self.vco2_octaves, self.vco2_target),
            ] {
                *now += (target - *now).clamp(-step, step);
            }
        }

        let adsr = self.adsr.tick();
        let ar = self.ar.tick();

        let noise = if patch.noise_pink { self.noise.pink() } else { self.noise.white() };

        // The sample-and-hold mixer, and the hold that runs off it.
        let sh_mix = self.last_sh_a * patch.sh_a + self.last_sh_b * patch.sh_b;
        let trigger = if patch.sh_kybd_trig { self.gate } else { lfo_high };
        let sh_out = self.sh.process(sh_mix, trigger, patch.sh_lag, sr);

        // Per-oscillator drift
        self.drift_phase1 += 0.23 / sr;
        self.drift_phase2 += 0.31 / sr;
        if self.drift_phase1 > 1.0 { self.drift_phase1 -= 1.0; }
        if self.drift_phase2 > 1.0 { self.drift_phase2 -= 1.0; }
        let drift1 = (self.drift_phase1 * TWO_PI).sin() * DRIFT_CENTS;
        let drift2 = (self.drift_phase2 * TWO_PI).sin() * DRIFT_CENTS;

        // The pitch pads: the two outer ones bend, the middle one adds
        // vibrato in proportion to how hard it is pressed.
        let pads = self.bend * BEND_SEMITONES * 100.0;
        let vibrato = lfo_sin * self.mod_wheel * PPC_VIBRATO_CENTS;
        let transpose = f64::from(patch.transpose) * 1200.0;

        let fm1_1 = (if patch.vco1_fm1_square { lfo_sq } else { lfo_sin }) * patch.vco1_fm1;
        let fm2_1 = (if patch.vco1_fm2_adsr { adsr } else { sh_out }) * patch.vco1_fm2;
        let fm1_2 = (if patch.vco2_fm1_shmix { sh_mix } else { lfo_sin }) * patch.vco2_fm1;
        let fm2_2 = (if patch.vco2_fm2_adsr { adsr } else { sh_out }) * patch.vco2_fm2;

        let cents1 = patch.vco1_tune + patch.vco1_fine + fm1_1 + fm2_1 + drift1 + vibrato + pads;
        let cents2 = patch.vco2_tune + patch.vco2_fine + fm1_2 + fm2_2 + drift2 + vibrato + pads;

        // The keyboard switch: off, and the oscillator leaves the keyboard
        // behind for the 0.2-20 Hz range, which is the instrument's second
        // LFO. Its coarse slider still sets where in that range it sits.
        let freq1 = if patch.vco1_kybd {
            exp2(self.vco1_octaves + (cents1 + transpose) / 1200.0)
        } else {
            LF_BASE_HZ * exp2((cents1 + TUNE_CENTS * 0.5) / 1200.0)
        };
        let freq2 = exp2(self.vco2_octaves + (cents2 + transpose) / 1200.0);
        self.vco1.set_freq(freq1, sr);
        self.vco2.set_freq(freq2, sr);

        let pwm1 = if patch.vco1_pwm_adsr { adsr } else { lfo_sin };
        let pwm2 = if patch.vco2_pwm_adsr { adsr } else { lfo_sin };
        let pw1 = patch.vco1_pw + pwm1 * patch.vco1_pwm * PW_SWING;
        let pw2 = patch.vco2_pw + pwm2 * patch.vco2_pwm * PW_SWING;

        let (saw1, pulse1, reset1) = self.vco1.tick(pw1);
        if patch.sync && reset1 {
            self.vco2.reset_phase();
        }
        let (saw2, pulse2, _) = self.vco2.tick(pw2);

        // The audio mixer's three faders. The third one is shared by the
        // noise generator and the ring modulator, as on the panel, and the
        // ring modulator is an XOR of the two pulse outputs whichever
        // waveform the mixer switches are showing.
        let vco1_out = if patch.vco1_pulse { pulse1 } else { saw1 };
        let vco2_out = if patch.vco2_pulse { pulse2 } else { saw2 };
        let third = if patch.ring_mod { -pulse1 * pulse2 } else { noise };
        let mixed = vco1_out * patch.vco1_level
            + vco2_out * patch.vco2_level
            + third * patch.ring_level;

        self.last_sh_a = if patch.sh_a_square { pulse1 } else { saw1 };
        self.last_sh_b = if patch.sh_b_vco2 { pulse2 } else { noise };

        let hp_out = self.hpf.process(mixed, patch.hpf, sr);

        // The filter's three modulation slots, each a fader and a switch, as
        // on the panel. Keyboard follow is in octaves of cutoff per octave of
        // keyboard, so it is scaled by how many octaves the cutoff slider
        // spans — it used to be divided by five semitones instead, which
        // tracked at over twice the rate the control claims.
        let key = self.held.split().map_or(60.0, |(low, _)| f64::from(low));
        let slot1 = if patch.kybd_from_sh {
            sh_mix
        } else {
            (key - 60.0) / 12.0 / CUTOFF_OCTAVES
        };
        let slot2 = if patch.lfo_from_lfo { lfo_sin } else { sh_out };
        let slot3 = if patch.env_from_ar { ar } else { adsr };
        let effective_cutoff = (patch.cutoff
            + slot1 * patch.kybd_amount
            + slot2 * patch.lfo_amount
            + slot3 * patch.env_amount)
            .clamp(0.0, 1.0);

        let lp_out = match patch.filter_type {
            0 => self.filter_4023.process(hp_out, effective_cutoff, patch.resonance, sr),
            1 => self.filter_4035.process(hp_out, effective_cutoff, patch.resonance, sr),
            _ => self.filter_4075.process(hp_out, effective_cutoff, patch.resonance, sr),
        };

        // The VCA: its gain slider is a constant offset that passes signal
        // with no key down, and the envelope the switch chose rides on top.
        let vca_env = if patch.vca_adsr { adsr } else { ar };
        let out = lp_out * (patch.vca_gain + vca_env * self.velocity);
        if patch.drive {
            tanh_approx(out * DRIVE_GAIN) * DRIVE_TRIM
        } else {
            out
        }
    }
}

#[inline]
fn exp2(octaves: f64) -> f64 {
    2.0f64.powf(octaves)
}

/// A note as octaves above 1 Hz, which is the space a glide of so many
/// seconds per octave is a straight line in.
fn note_octaves(note: u8) -> f64 {
    (440.0f64).log2() + (f64::from(note) - 69.0) / 12.0
}

// ── Odyssey synth ──

pub struct OdysseySynth {
    voice: Option<OdysseyVoice>,
    lfo: OdysseyLfo,
    sample_rate: f64,
    pub params: [f32; PARAM_COUNT],
    last_patch_index: usize,
}

impl OdysseySynth {
    #[must_use]
    pub fn new() -> Self {
        let mut s = Self {
            voice: None,
            lfo: OdysseyLfo::new(),
            sample_rate: 44100.0,
            params: PARAM_DEFAULTS,
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
    /// The bank is held in seconds, hertz and cents, so those sliders come
    /// back through their tapers; each switch lands on the midpoint of its
    /// position, so a switch loaded from a preset sits where [`step_discrete`]
    /// would leave it.
    #[must_use]
    pub fn params_for_patch(patch_value: f32) -> [f32; PARAM_COUNT] {
        let p = &BANK[patch_index(patch_value)].voice;
        let mut params = [0.0f32; PARAM_COUNT];
        let two = |on: bool| knob_for(usize::from(on), 2);
        params[P_PATCH] = patch_value;
        params[P_NOISE_TYPE] = two(p.noise_pink);
        params[P_PORTAMENTO] = slider_for(porta_seconds, p.portamento);
        params[P_TRANSPOSE] = knob_for((p.transpose / 2 + 1).clamp(0, 2) as usize, 3);
        params[P_VCO1_FREQ] = tune_slider(p.vco1_tune);
        params[P_VCO1_FINE] = fine_slider(p.vco1_fine);
        params[P_VCO1_KYBD] = two(p.vco1_kybd);
        params[P_VCO1_FM1] = slider_for(fm_cents, p.vco1_fm1);
        params[P_VCO1_FM1_SRC] = two(p.vco1_fm1_square);
        params[P_VCO1_FM2] = slider_for(fm_cents, p.vco1_fm2);
        params[P_VCO1_FM2_SRC] = two(p.vco1_fm2_adsr);
        params[P_VCO1_PW] = pw_slider(p.vco1_pw);
        params[P_VCO1_PWM] = p.vco1_pwm as f32;
        params[P_VCO1_PWM_SRC] = two(p.vco1_pwm_adsr);
        params[P_VCO2_FREQ] = tune_slider(p.vco2_tune);
        params[P_VCO2_FINE] = fine_slider(p.vco2_fine);
        params[P_SYNC] = two(p.sync);
        params[P_VCO2_FM1] = slider_for(fm_cents, p.vco2_fm1);
        params[P_VCO2_FM1_SRC] = two(p.vco2_fm1_shmix);
        params[P_VCO2_FM2] = slider_for(fm_cents, p.vco2_fm2);
        params[P_VCO2_FM2_SRC] = two(p.vco2_fm2_adsr);
        params[P_VCO2_PW] = pw_slider(p.vco2_pw);
        params[P_VCO2_PWM] = p.vco2_pwm as f32;
        params[P_VCO2_PWM_SRC] = two(p.vco2_pwm_adsr);
        params[P_LFO_RATE] = slider_for(lfo_hz, p.lfo_rate);
        params[P_SH_A] = p.sh_a as f32;
        params[P_SH_A_SRC] = two(p.sh_a_square);
        params[P_SH_B] = p.sh_b as f32;
        params[P_SH_B_SRC] = two(p.sh_b_vco2);
        params[P_SH_LAG] = slider_for(lag_seconds, p.sh_lag);
        params[P_SH_TRIG] = two(p.sh_kybd_trig);
        params[P_RING_LEVEL] = p.ring_level as f32;
        params[P_RING_SRC] = two(p.ring_mod);
        params[P_VCO1_LEVEL] = p.vco1_level as f32;
        params[P_VCO1_WAVE] = two(p.vco1_pulse);
        params[P_VCO2_LEVEL] = p.vco2_level as f32;
        params[P_VCO2_WAVE] = two(p.vco2_pulse);
        params[P_HPF] = p.hpf as f32;
        params[P_CUTOFF] = p.cutoff as f32;
        params[P_RESO] = p.resonance as f32;
        params[P_FILTER_TYPE] = knob_for(p.filter_type as usize, 3);
        params[P_VCF_KYBD] = p.kybd_amount as f32;
        params[P_VCF_KYBD_SRC] = two(p.kybd_from_sh);
        params[P_VCF_LFO] = p.lfo_amount as f32;
        params[P_VCF_LFO_SRC] = two(p.lfo_from_lfo);
        params[P_VCF_ENV] = p.env_amount as f32;
        params[P_VCF_ENV_SRC] = two(p.env_from_ar);
        params[P_VCA_GAIN] = p.vca_gain as f32;
        params[P_DRIVE] = two(p.drive);
        // The bank records no VCA level: these presets were written against a
        // panel with one master gain and no per-patch level, so every patch
        // loads the default and the fader stays where the player left it.
        params[P_LEVEL] = PARAM_DEFAULTS[P_LEVEL];
        params[P_AR_A] = slider_for(attack_seconds, p.ar_a);
        params[P_AR_R] = slider_for(decay_seconds, p.ar_r);
        params[P_ATTACK] = slider_for(attack_seconds, p.adsr_a);
        params[P_DECAY] = slider_for(decay_seconds, p.adsr_d);
        params[P_SUSTAIN] = p.adsr_s as f32;
        params[P_RELEASE] = slider_for(release_seconds, p.adsr_r);
        params[P_VCA_ENV] = two(p.vca_adsr);
        params[P_ADSR_TRIG] = two(p.adsr_lfo_trig);
        params[P_AR_TRIG] = two(p.ar_lfo_trig);
        params
    }

    /// When the patch selector moves, load its panel into the parameters.
    fn sync_params_from_patch(&mut self) {
        let idx = self.current_patch_index();
        if idx == self.last_patch_index {
            return;
        }
        self.last_patch_index = idx;
        let loaded = Self::params_for_patch(self.params[P_PATCH]);
        for (i, &v) in loaded.iter().enumerate() {
            if i != P_PATCH {
                self.params[i] = v;
            }
        }
    }

    /// The panel as it stands, in the units the engine works in. Every
    /// control is live — the preset is only where the sliders started.
    fn active_patch(&self) -> OdysseyPatch {
        let p = &self.params;
        let on = |i: usize| selector(p[i], 2) == 1;
        let f = |i: usize| f64::from(p[i]);
        OdysseyPatch {
            noise_pink: on(P_NOISE_TYPE),
            portamento: porta_seconds(f(P_PORTAMENTO)),
            transpose: (selector(p[P_TRANSPOSE], 3) as i8 - 1) * 2,
            vco1_tune: tune_cents(f(P_VCO1_FREQ)),
            vco1_fine: fine_cents(f(P_VCO1_FINE)),
            vco1_kybd: on(P_VCO1_KYBD),
            vco1_fm1: fm_cents(f(P_VCO1_FM1)),
            vco1_fm1_square: on(P_VCO1_FM1_SRC),
            vco1_fm2: fm_cents(f(P_VCO1_FM2)),
            vco1_fm2_adsr: on(P_VCO1_FM2_SRC),
            vco1_pw: pulse_width(f(P_VCO1_PW)),
            vco1_pwm: f(P_VCO1_PWM),
            vco1_pwm_adsr: on(P_VCO1_PWM_SRC),
            vco2_tune: tune_cents(f(P_VCO2_FREQ)),
            vco2_fine: fine_cents(f(P_VCO2_FINE)),
            sync: on(P_SYNC),
            vco2_fm1: fm_cents(f(P_VCO2_FM1)),
            vco2_fm1_shmix: on(P_VCO2_FM1_SRC),
            vco2_fm2: fm_cents(f(P_VCO2_FM2)),
            vco2_fm2_adsr: on(P_VCO2_FM2_SRC),
            vco2_pw: pulse_width(f(P_VCO2_PW)),
            vco2_pwm: f(P_VCO2_PWM),
            vco2_pwm_adsr: on(P_VCO2_PWM_SRC),
            lfo_rate: lfo_hz(f(P_LFO_RATE)),
            sh_a: f(P_SH_A),
            sh_a_square: on(P_SH_A_SRC),
            sh_b: f(P_SH_B),
            sh_b_vco2: on(P_SH_B_SRC),
            sh_lag: lag_seconds(f(P_SH_LAG)),
            sh_kybd_trig: on(P_SH_TRIG),
            ring_level: f(P_RING_LEVEL),
            ring_mod: on(P_RING_SRC),
            vco1_level: f(P_VCO1_LEVEL),
            vco1_pulse: on(P_VCO1_WAVE),
            vco2_level: f(P_VCO2_LEVEL),
            vco2_pulse: on(P_VCO2_WAVE),
            hpf: f(P_HPF),
            cutoff: f(P_CUTOFF),
            resonance: f(P_RESO),
            filter_type: selector(p[P_FILTER_TYPE], 3) as u8,
            kybd_amount: f(P_VCF_KYBD),
            kybd_from_sh: on(P_VCF_KYBD_SRC),
            lfo_amount: f(P_VCF_LFO),
            lfo_from_lfo: on(P_VCF_LFO_SRC),
            env_amount: f(P_VCF_ENV),
            env_from_ar: on(P_VCF_ENV_SRC),
            vca_gain: f(P_VCA_GAIN),
            drive: on(P_DRIVE),
            ar_a: attack_seconds(f(P_AR_A)),
            ar_r: decay_seconds(f(P_AR_R)),
            adsr_a: attack_seconds(f(P_ATTACK)),
            adsr_d: decay_seconds(f(P_DECAY)),
            adsr_s: f(P_SUSTAIN),
            adsr_r: release_seconds(f(P_RELEASE)),
            vca_adsr: on(P_VCA_ENV),
            adsr_lfo_trig: on(P_ADSR_TRIG),
            ar_lfo_trig: on(P_AR_TRIG),
        }
    }
}

impl Default for OdysseySynth {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for OdysseySynth {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "Odyssey".into(),
            version: "0.1.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voice = Some(OdysseyVoice::new(sample_rate));
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() {
            return;
        }
        let patch = self.active_patch();
        let gain = self.params[P_LEVEL] * OUTPUT_TRIM;
        self.lfo.rate = patch.lfo_rate;
        let buf_len = outputs[0].len();
        // Disjoint borrows: the LFO and the voice are both stepped in the
        // sample loop and neither can hold the whole synth.
        let Self { voice, lfo, sample_rate, .. } = self;
        let Some(voice) = voice.as_mut() else { return };
        let sample_rate = *sample_rate;

        // Sort MIDI events (allocation-free)
        let mut event_indices: [usize; 256] = [0; 256];
        let event_count = midi_events.len().min(256);
        for (i, slot) in event_indices[..event_count].iter_mut().enumerate() {
            *slot = i;
        }
        for i in 1..event_count {
            let mut j = i;
            while j > 0
                && midi_events[event_indices[j]].sample_offset
                    < midi_events[event_indices[j - 1]].sample_offset
            {
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
                            voice.note_on(ev.data1, ev.data2, &patch);
                        } else {
                            voice.note_off(ev.data1, &patch);
                        }
                    }
                    0x80 => voice.note_off(ev.data1, &patch),
                    // The middle pitch pad, which applies vibrato in
                    // proportion to how hard it is pressed.
                    0xB0 => match ev.data1 {
                        1 => voice.mod_wheel = f64::from(ev.data2) / 127.0,
                        120 | 123 => voice.kill(),
                        _ => {}
                    },
                    // The outer pitch pads.
                    0xE0 => {
                        let raw = i32::from(ev.data1) | (i32::from(ev.data2) << 7);
                        voice.bend = f64::from(raw - 8192) / 8192.0;
                    }
                    _ => {}
                }
                ei += 1;
            }

            let (lfo_sin, lfo_sq) = lfo.tick(sample_rate);
            let sample = voice.tick(&patch, lfo_sin, lfo_sq) as f32;
            // Bound the output without hard clipping it. The trim above keeps
            // ordinary playing under the knee, so this is the identity for
            // everything except a patch pushed past it by the level fader.
            let sample = soft_saturate(sample * gain);

            for ch in outputs.iter_mut() {
                ch[i] = sample;
            }
        }
    }

    fn parameter_count(&self) -> usize {
        PARAM_COUNT
    }

    fn parameter_info(&self, index: usize) -> Option<ParameterInfo> {
        if index >= PARAM_COUNT {
            return None;
        }
        Some(ParameterInfo {
            name: PARAM_NAMES[index].into(),
            min: 0.0,
            max: 1.0,
            default: PARAM_DEFAULTS[index],
            unit: match index {
                P_ATTACK | P_DECAY | P_RELEASE | P_AR_A | P_AR_R | P_PORTAMENTO | P_SH_LAG => {
                    "s".into()
                }
                P_LFO_RATE => "Hz".into(),
                _ => String::new(),
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

    fn reset(&mut self) {
        if let Some(v) = self.voice.as_mut() {
            v.kill();
        }
    }
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
    fn cc(number: u8, value: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0xB0, data1: number, data2: value }
    }
    fn bend(value: u16, offset: u32) -> MidiEvent {
        MidiEvent {
            sample_offset: offset,
            status: 0xE0,
            data1: (value & 0x7F) as u8,
            data2: (value >> 7) as u8,
        }
    }

    fn process_buffers(synth: &mut OdysseySynth, events: &[MidiEvent], count: usize) -> Vec<f32> {
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

    fn synth_at(patch: usize) -> OdysseySynth {
        let mut s = OdysseySynth::new();
        s.init(44100.0, 64);
        s.set_parameter(P_PATCH, patch_knob(patch));
        s
    }

    // ── Basics ──

    #[test]
    fn silence_with_no_input() {
        let mut s = synth_at(0);
        let out = process_buffers(&mut s, &[], 4);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = synth_at(0);
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 8);
        assert!(peak(&out) > 0.001, "no sound: {}", peak(&out));
    }

    #[test]
    fn silent_after_release() {
        let mut s = synth_at(0);
        s.set_parameter(P_AR_R, 0.0);
        s.set_parameter(P_RELEASE, 0.0);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 8);
        process_buffers(&mut s, &[note_off(60, 0)], 200);
        let out = process_buffers(&mut s, &[], 4);
        assert!(peak(&out) < 0.001, "still sounding: {}", peak(&out));
    }

    #[test]
    fn output_is_finite() {
        let mut s = synth_at(0);
        let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 1000);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn duophonic_split() {
        // VCO-1 takes the lowest key and VCO-2 the highest, so two keys are
        // two pitches out of one voice rather than two voices.
        let mut s = synth_at(0);
        process_buffers(&mut s, &[note_on(48, 100, 0), note_on(72, 100, 0)], 4);
        let voice = s.voice.as_ref().unwrap();
        assert_eq!(voice.held.len, 2);
        assert_eq!(voice.held.split(), Some((48, 72)));
        let low = exp2(voice.vco1_octaves);
        let high = exp2(voice.vco2_octaves);
        assert!((high / low - 4.0).abs() < 1e-9, "two octaves apart: {low} and {high}");
    }

    #[test]
    fn held_notes_do_not_grow_past_their_slab() {
        // The list used to be a `Vec` reserved for sixteen, pushed to from
        // the audio callback: the seventeenth key held at once reallocated
        // inside `process`.
        let mut held = HeldNotes::new();
        for note in 0..64u8 {
            held.push(note);
        }
        assert_eq!(held.len, MAX_HELD);
        assert_eq!(held.split(), Some((48, 63)), "the oldest keys should have been dropped");
        held.push(20);
        assert_eq!(held.len, MAX_HELD);
        assert_eq!(held.split(), Some((20, 63)));
        held.remove(63);
        assert_eq!(held.len, MAX_HELD - 1);
        assert_eq!(held.split(), Some((20, 62)));
    }

    #[test]
    fn all_patches_produce_sound() {
        for (index, name) in PATCH_NAMES.iter().enumerate() {
            let mut s = synth_at(index);
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 2000);
            assert!(peak(&out) > 0.001, "{name} is silent: {}", peak(&out));
        }
    }

    #[test]
    fn all_patches_finite() {
        for (index, name) in PATCH_NAMES.iter().enumerate() {
            let mut s = synth_at(index);
            let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 500);
            assert!(out.iter().all(|v| v.is_finite()), "{name} is not finite");
        }
    }

    #[test]
    fn cc120_kills() {
        let mut s = synth_at(0);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 4);
        process_buffers(&mut s, &[cc(120, 0, 0)], 1);
        let out = process_buffers(&mut s, &[], 2);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn all_params_readable() {
        let s = OdysseySynth::new();
        assert_eq!(s.parameter_count(), PARAM_COUNT);
        for i in 0..PARAM_COUNT {
            assert!(s.parameter_info(i).is_some());
            let v = s.get_parameter(i);
            assert!((0.0..=1.0).contains(&v), "param {i} = {v}");
        }
        assert!(s.parameter_info(PARAM_COUNT).is_none());
    }

    #[test]
    fn sample_accurate_midi() {
        let mut s = synth_at(0);
        s.set_parameter(P_AR_A, 0.0);
        let mut out = vec![0.0f32; 128];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 64)]);
        assert!(peak(&out[..64]) < 0.001, "sound before the note: {}", peak(&out[..64]));
        assert!(peak(&out[64..]) > 0.001, "no sound after it: {}", peak(&out[64..]));
    }

    // ── Panel ──

    #[test]
    fn the_panel_is_in_front_panel_order() {
        // The order is the instrument's, left to right, and the editor shows
        // the parameter block in index order — so this is the layout a player
        // sees. Sessions store the block positionally, which is what makes
        // the order worth pinning down in a test.
        assert_eq!(PARAM_NAMES[P_PATCH], "patch");
        assert_eq!(&PARAM_NAMES[P_NOISE_TYPE..=P_TRANSPOSE], &["noise", "porta", "transpos"]);
        assert_eq!(
            &PARAM_NAMES[P_VCO1_FREQ..=P_VCO1_PWM_SRC],
            &["v1 freq", "v1 fine", "v1 kybd", "v1 fm1", "v1 fm1sr", "v1 fm2", "v1 fm2sr",
              "v1 pw", "v1 pwm", "v1 pwmsr"]
        );
        assert_eq!(
            &PARAM_NAMES[P_VCO2_FREQ..=P_VCO2_PWM_SRC],
            &["v2 freq", "v2 fine", "sync", "v2 fm1", "v2 fm1sr", "v2 fm2", "v2 fm2sr",
              "v2 pw", "v2 pwm", "v2 pwmsr"]
        );
        assert_eq!(
            &PARAM_NAMES[P_LFO_RATE..=P_SH_TRIG],
            &["lfo rate", "sh in a", "sh a src", "sh in b", "sh b src", "sh lag", "sh trig"]
        );
        assert_eq!(
            &PARAM_NAMES[P_RING_LEVEL..=P_VCO2_WAVE],
            &["mix ring", "ring src", "mix vco1", "vco1 wav", "mix vco2", "vco2 wav"]
        );
        assert_eq!(
            &PARAM_NAMES[P_HPF..=P_VCF_ENV_SRC],
            &["hpf", "freq", "res", "filter", "vcf kybd", "kybd src", "vcf lfo", "lfo src",
              "vcf env", "env src"]
        );
        assert_eq!(&PARAM_NAMES[P_VCA_GAIN..=P_LEVEL], &["vca gain", "drive", "level"]);
        assert_eq!(
            &PARAM_NAMES[P_AR_A..=P_AR_TRIG],
            &["ar a", "ar r", "attack", "decay", "sustain", "release", "vca env", "adsr trg",
              "ar trg"]
        );
        assert_eq!(PARAM_COUNT, 59);
        // The editor lays the name out in eight columns before the bar.
        for name in PARAM_NAMES {
            assert!(name.len() <= 8, "{name:?} is wider than the editor's name column");
        }
    }

    #[test]
    fn every_engine_control_is_reachable() {
        // The defect this guards: the engine modelled about half an Odyssey
        // and the panel exposed sixteen controls of it, so the noise fader,
        // the pulse width, the high-pass, keyboard follow, portamento, the
        // whole sample-and-hold section and both AR sliders could not be
        // touched. A control the engine reads has to have an index.
        //
        // Every source running and every destination routed, so that no
        // control is masked by a dead path.
        fn primed() -> OdysseySynth {
            let mut s = OdysseySynth::new();
            s.init(44100.0, 64);
            for (index, value) in [
                (P_VCO1_LEVEL, 0.6), (P_VCO2_LEVEL, 0.6), (P_RING_LEVEL, 0.3),
                (P_VCO1_WAVE, 0.75), (P_VCO2_WAVE, 0.75),
                (P_VCO1_PW, 0.5), (P_VCO2_PW, 0.5),
                (P_VCO1_PWM, 0.4), (P_VCO2_PWM, 0.4),
                (P_VCO1_FM1, 0.3), (P_VCO2_FM1, 0.3),
                (P_VCO1_FM2, 0.3), (P_VCO2_FM2, 0.3),
                (P_SH_A, 0.5), (P_SH_B, 0.5), (P_SH_LAG, 0.2),
                (P_LFO_RATE, 0.92),
                (P_HPF, 0.3), (P_CUTOFF, 0.5), (P_RESO, 0.4),
                (P_VCF_KYBD, 0.5), (P_VCF_LFO, 0.3), (P_VCF_ENV, 0.4),
                (P_LEVEL, 0.7),
                (P_AR_A, 0.05), (P_AR_R, 0.3),
                (P_ATTACK, 0.05), (P_DECAY, 0.3), (P_SUSTAIN, 0.4), (P_RELEASE, 0.3),
            ] {
                s.set_parameter(index, value);
            }
            s
        }
        fn render(s: &mut OdysseySynth) -> Vec<f32> {
            // Two keys, an octave apart and not at middle C, so that the
            // duophonic split, keyboard follow and portamento all have
            // something to do; then released, so the release sliders do too.
            let mut out = process_buffers(s, &[note_on(55, 100, 0)], 60);
            out.extend(process_buffers(s, &[note_on(72, 100, 0)], 120));
            out.extend(process_buffers(s, &[note_off(72, 0), note_off(55, 0)], 120));
            out
        }
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            if index == P_PATCH {
                continue;
            }
            let mut low = primed();
            let mut high = primed();
            low.set_parameter(index, 0.0);
            high.set_parameter(index, 1.0);
            let a = render(&mut low);
            let b = render(&mut high);
            let diff: f32 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum();
            assert!(diff > 1e-3, "parameter {index} ({name}) changes nothing: diff={diff}");
        }
    }

    #[test]
    fn switches_step_one_position_per_press() {
        // A float-fraction stepper walks a switch a fraction of a position at
        // a time and stalls on the boundary; the DX7's bank knob did exactly
        // that, and this instrument's patch knob and its three-position
        // filter switch were stepping by 1/(n - 0.01) and by 0.34.
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
        assert_eq!(discrete_label(P_PATCH, 0.0), Some("Bass"));
        assert_eq!(discrete_label(P_NOISE_TYPE, knob_for(1, 2)), Some("pink"));
        assert_eq!(discrete_label(P_TRANSPOSE, knob_for(0, 3)), Some("-2 oct"));
        assert_eq!(discrete_label(P_TRANSPOSE, knob_for(2, 3)), Some("+2 oct"));
        assert_eq!(discrete_label(P_FILTER_TYPE, knob_for(0, 3)), Some("4023"));
        assert_eq!(discrete_label(P_FILTER_TYPE, knob_for(1, 3)), Some("4035"));
        assert_eq!(discrete_label(P_FILTER_TYPE, knob_for(2, 3)), Some("4075"));
        assert_eq!(discrete_label(P_VCO1_KYBD, knob_for(0, 2)), Some("LF"));
        assert_eq!(discrete_label(P_VCO1_FM1_SRC, knob_for(1, 2)), Some("LFO sqr"));
        assert_eq!(discrete_label(P_VCO2_FM1_SRC, knob_for(1, 2)), Some("S/H mix"));
        assert_eq!(discrete_label(P_SH_TRIG, knob_for(1, 2)), Some("KYBD"));
        assert_eq!(discrete_label(P_RING_SRC, knob_for(1, 2)), Some("ring"));
        // The two envelope switches read in opposite orders on the panel:
        // the filter's is marked ADSR/AR and the amplifier's AR/ADSR.
        assert_eq!(discrete_label(P_VCF_ENV_SRC, knob_for(0, 2)), Some("ADSR"));
        assert_eq!(discrete_label(P_VCF_ENV_SRC, knob_for(1, 2)), Some("AR"));
        assert_eq!(discrete_label(P_VCA_ENV, knob_for(0, 2)), Some("AR"));
        assert_eq!(discrete_label(P_VCA_ENV, knob_for(1, 2)), Some("ADSR"));
        assert_eq!(discrete_label(P_AR_TRIG, knob_for(1, 2)), Some("LFO rpt"));
        assert_eq!(discrete_label(P_CUTOFF, 0.5), None);
        // Out-of-range knobs are labelled, not panicked on: `params` is public.
        assert_eq!(discrete_label(P_FILTER_TYPE, 9.0), Some("4075"));
        assert_eq!(discrete_label(P_PATCH, -1.0), Some("Bass"));
    }

    #[test]
    fn the_patch_knob_lands_on_the_patch_it_names() {
        for (index, name) in PATCH_NAMES.iter().enumerate() {
            let knob = patch_knob(index);
            assert_eq!(patch_index(knob), index, "patch {index} knob {knob}");
            assert_eq!(discrete_label(P_PATCH, knob), Some(*name));
            let s = synth_at(index);
            assert_eq!(s.current_patch_index(), index);
        }
        for (i, name) in PATCH_NAMES.iter().enumerate() {
            assert!(
                !PATCH_NAMES[i + 1..].contains(name),
                "two patches are named {name}"
            );
        }
    }

    #[test]
    fn patch_zero_is_the_default_parameter_block() {
        let loaded = OdysseySynth::params_for_patch(0.0);
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
        for (index, program) in BANK.iter().enumerate() {
            let want = &program.voice;
            let got = synth_at(index).active_patch();
            let name = program.name;
            let close = |got: f64, want: f64, what: &str| {
                assert!(
                    (got - want).abs() < want.abs() * 1e-4 + 1e-4,
                    "{name} {what}: {got} where the preset says {want}"
                );
            };
            assert_eq!(got.noise_pink, want.noise_pink, "{name} noise type");
            close(got.portamento, want.portamento, "portamento");
            assert_eq!(got.transpose, want.transpose, "{name} transpose");
            for (what, got, want) in [
                ("vco1 tune", got.vco1_tune, want.vco1_tune),
                ("vco1 fine", got.vco1_fine, want.vco1_fine),
                ("vco2 tune", got.vco2_tune, want.vco2_tune),
                ("vco2 fine", got.vco2_fine, want.vco2_fine),
            ] {
                // The coarse slider carries 7972 cents in a single f32, so
                // one ulp of the knob is a thousandth of a cent at the ends
                // of the travel.
                assert!((got - want).abs() < 1e-2,
                        "{name} {what}: {got} where the preset says {want}");
            }
            assert_eq!(got.vco1_kybd, want.vco1_kybd, "{name} vco1 keyboard");
            close(got.vco1_fm1, want.vco1_fm1, "vco1 fm1");
            assert_eq!(got.vco1_fm1_square, want.vco1_fm1_square, "{name} vco1 fm1 source");
            close(got.vco1_fm2, want.vco1_fm2, "vco1 fm2");
            assert_eq!(got.vco1_fm2_adsr, want.vco1_fm2_adsr, "{name} vco1 fm2 source");
            close(got.vco1_pw, want.vco1_pw, "vco1 pulse width");
            close(got.vco1_pwm, want.vco1_pwm, "vco1 pwm");
            assert_eq!(got.vco1_pwm_adsr, want.vco1_pwm_adsr, "{name} vco1 pwm source");
            assert_eq!(got.sync, want.sync, "{name} sync");
            close(got.vco2_fm1, want.vco2_fm1, "vco2 fm1");
            assert_eq!(got.vco2_fm1_shmix, want.vco2_fm1_shmix, "{name} vco2 fm1 source");
            close(got.vco2_fm2, want.vco2_fm2, "vco2 fm2");
            assert_eq!(got.vco2_fm2_adsr, want.vco2_fm2_adsr, "{name} vco2 fm2 source");
            close(got.vco2_pw, want.vco2_pw, "vco2 pulse width");
            close(got.vco2_pwm, want.vco2_pwm, "vco2 pwm");
            assert_eq!(got.vco2_pwm_adsr, want.vco2_pwm_adsr, "{name} vco2 pwm source");
            close(got.lfo_rate, want.lfo_rate, "lfo rate");
            close(got.sh_a, want.sh_a, "s/h input a");
            assert_eq!(got.sh_a_square, want.sh_a_square, "{name} s/h input a source");
            close(got.sh_b, want.sh_b, "s/h input b");
            assert_eq!(got.sh_b_vco2, want.sh_b_vco2, "{name} s/h input b source");
            close(got.sh_lag, want.sh_lag, "s/h lag");
            assert_eq!(got.sh_kybd_trig, want.sh_kybd_trig, "{name} s/h trigger");
            close(got.ring_level, want.ring_level, "noise/ring level");
            assert_eq!(got.ring_mod, want.ring_mod, "{name} noise/ring switch");
            close(got.vco1_level, want.vco1_level, "vco1 level");
            assert_eq!(got.vco1_pulse, want.vco1_pulse, "{name} vco1 waveform");
            close(got.vco2_level, want.vco2_level, "vco2 level");
            assert_eq!(got.vco2_pulse, want.vco2_pulse, "{name} vco2 waveform");
            close(got.hpf, want.hpf, "hpf");
            close(got.cutoff, want.cutoff, "cutoff");
            close(got.resonance, want.resonance, "resonance");
            assert_eq!(got.filter_type, want.filter_type, "{name} filter type");
            close(got.kybd_amount, want.kybd_amount, "vcf keyboard amount");
            assert_eq!(got.kybd_from_sh, want.kybd_from_sh, "{name} vcf keyboard source");
            close(got.lfo_amount, want.lfo_amount, "vcf lfo amount");
            assert_eq!(got.lfo_from_lfo, want.lfo_from_lfo, "{name} vcf lfo source");
            close(got.env_amount, want.env_amount, "vcf envelope amount");
            assert_eq!(got.env_from_ar, want.env_from_ar, "{name} vcf envelope source");
            close(got.vca_gain, want.vca_gain, "vca gain");
            assert_eq!(got.drive, want.drive, "{name} drive");
            for (what, got, want) in [
                ("ar attack", got.ar_a, want.ar_a),
                ("ar release", got.ar_r, want.ar_r),
                ("adsr attack", got.adsr_a, want.adsr_a),
                ("adsr decay", got.adsr_d, want.adsr_d),
                ("adsr release", got.adsr_r, want.adsr_r),
            ] {
                // The floor is the instrument's own: ARP's shortest segment
                // is 5 ms, so a preset asking for 1 ms gets 5.
                assert!((got - want).abs() < want.abs() * 1e-4 + 5e-3,
                        "{name} {what}: {got} where the preset says {want}");
            }
            close(got.adsr_s, want.adsr_s, "adsr sustain");
            assert_eq!(got.vca_adsr, want.vca_adsr, "{name} vca envelope");
            assert_eq!(got.adsr_lfo_trig, want.adsr_lfo_trig, "{name} adsr trigger");
            assert_eq!(got.ar_lfo_trig, want.ar_lfo_trig, "{name} ar trigger");
        }
    }

    // ── Envelopes ──

    fn stage_seconds(mut env: OdysseyEnvelope, stage: EnvStage) -> f64 {
        let mut n = 0u64;
        while env.stage == stage && n < 44100 * 200 {
            env.tick();
            n += 1;
        }
        n as f64 / 44100.0
    }

    #[test]
    fn the_ar_holds_at_peak_while_the_key_is_down() {
        // This is the defect at the centre of the rewrite. ARP describe the
        // AR as the ADSR with its sustain fixed at maximum, so a note held on
        // it holds. This one went to its release segment the instant the
        // attack finished, so it fell away at the release rate with the key
        // still down — and since twenty-three of the forty-four presets run
        // their amplifier from the AR, the slider marked `decay` could not
        // change how long a note lasted on any of them.
        let mut s = synth_at(0);
        assert_eq!(discrete_label(P_VCA_ENV, s.get_parameter(P_VCA_ENV)), Some("AR"));
        s.set_parameter(P_AR_A, 0.0);
        s.set_parameter(P_AR_R, 0.4);
        // The amplifier alone: the filter envelope closing would dim the note
        // whatever the AR did.
        s.set_parameter(P_VCF_ENV, 0.0);
        s.set_parameter(P_CUTOFF, 0.8);
        s.set_parameter(P_VCO2_LEVEL, 0.0);
        // Ten seconds of held key, against an AR release of well under one.
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 44100 * 10 / 64);
        let early = peak(&out[..44100]);
        let late = peak(&out[out.len() - 44100..]);
        assert!(late > 0.9 * early, "the note died with the key down: {early} to {late}");
        // ...and it does stop when the key comes up.
        let tail = process_buffers(&mut s, &[note_off(60, 0)], 44100 * 6 / 64);
        assert!(peak(&tail[tail.len() - 4410..]) < 0.001 * early, "the release never finished");
    }

    #[test]
    fn the_decay_slider_sets_how_long_a_note_lasts() {
        // Measured the way the defect was found: the amplifier on the ADSR,
        // the sustain at nothing, the key held, and the time from the peak to
        // -40 dB. It used to read the same at every position of the slider.
        let seconds_to_minus_40 = |decay: f32| {
            let mut s = synth_at(0);
            s.set_parameter(P_VCA_ENV, knob_for(1, 2)); // ADSR
            s.set_parameter(P_VCF_ENV, 0.0); // amplitude only, no filter sweep
            s.set_parameter(P_CUTOFF, 0.8);
            s.set_parameter(P_ATTACK, 0.0);
            s.set_parameter(P_SUSTAIN, 0.0);
            s.set_parameter(P_DECAY, decay);
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 44100 * 20 / 64);
            let window = 441; // 10 ms
            let envelope: Vec<f32> =
                out.chunks(window).map(peak).collect();
            let top = envelope.iter().copied().fold(0.0f32, f32::max);
            let at = envelope.iter().position(|&v| v >= top).unwrap_or(0);
            let hit = envelope.iter().skip(at).position(|&v| v < top * 0.01);
            hit.map_or(f64::INFINITY, |i| i as f64 * window as f64 / 44100.0)
        };
        let mut previous = 0.0;
        for slider in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let measured = seconds_to_minus_40(slider);
            let want = decay_seconds(f64::from(slider));
            assert!(measured > previous, "decay {slider}: {measured} s is not longer than {previous} s");
            previous = measured;
            // -40 dB of a segment that spans 3.5 time constants and stops at
            // its target arrives close to the end of it.
            assert!(
                (measured - want).abs() < want * 0.25 + 0.02,
                "decay {slider}: {measured:.3} s for a {want:.3} s setting"
            );
        }
    }

    #[test]
    fn the_envelope_takes_the_time_the_slider_says() {
        // The defect: every segment used its slider's seconds as a one-pole
        // *time constant* and ran until it was within 0.001 of the target,
        // which is 6.9 of them. Both envelopes are the same type, so this
        // holds for the AR as well.
        for slider in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let want_attack = attack_seconds(slider);
            let want_decay = decay_seconds(slider);
            let want_release = release_seconds(slider);

            let mut e = OdysseyEnvelope::new(44100.0);
            e.set_times(want_attack, 100.0, 100.0);
            e.sustain = 1.0;
            e.trigger();
            let measured = stage_seconds(e, EnvStage::Attack);
            assert!((measured - want_attack).abs() < want_attack * 0.02 + 0.001,
                    "attack {slider}: {measured:.3} s for a {want_attack:.3} s setting");

            let mut e = OdysseyEnvelope::new(44100.0);
            e.set_times(0.0005, want_decay, 100.0);
            e.sustain = 0.0;
            e.level = 1.0;
            e.enter_decay();
            let measured = stage_seconds(e, EnvStage::Decay);
            assert!((measured - want_decay).abs() < want_decay * 0.02 + 0.002,
                    "decay {slider}: {measured:.3} s for a {want_decay:.3} s setting");

            let mut e = OdysseyEnvelope::new(44100.0);
            e.set_times(0.0005, 100.0, want_release);
            e.level = 1.0;
            e.stage = EnvStage::Sustain;
            e.release_env();
            let measured = stage_seconds(e, EnvStage::Release);
            assert!((measured - want_release).abs() < want_release * 0.02 + 0.002,
                    "release {slider}: {measured:.3} s for a {want_release:.3} s setting");
        }
    }

    #[test]
    fn the_envelope_taper_covers_the_published_range() {
        // ARP's specification: ADSR attack 5 ms to 5 s, decay 10 ms to 8 s,
        // release 15 ms to 10 s; the AR's release shares the decay's range.
        assert!((attack_seconds(0.0) - 0.005).abs() < 1e-9);
        assert!((attack_seconds(1.0) - 5.005).abs() < 1e-9);
        assert!((decay_seconds(0.0) - 0.010).abs() < 1e-9);
        assert!((decay_seconds(1.0) - 8.010).abs() < 1e-9);
        assert!((release_seconds(0.0) - 0.015).abs() < 1e-9);
        assert!((release_seconds(1.0) - 10.015).abs() < 1e-9);
        // Audio-taper pots, so the middle of the travel is a twentieth of the
        // way up in time and not half of it.
        assert!((decay_seconds(0.5) - 0.487).abs() < 0.02, "{}", decay_seconds(0.5));
        assert!((attack_seconds(0.5) - 0.384).abs() < 0.02, "{}", attack_seconds(0.5));
        // Monotone, or `slider_for` would not be an inverse.
        let (mut a0, mut d0, mut r0) = (0.0, 0.0, 0.0);
        for i in 0..=1000 {
            let s = f64::from(i) / 1000.0;
            let (a, d, r) = (attack_seconds(s), decay_seconds(s), release_seconds(s));
            assert!(a > a0 && d >= d0 && r >= r0, "a taper is not monotone at {s}");
            a0 = a;
            d0 = d;
            r0 = r;
        }
    }

    #[test]
    fn the_slider_a_preset_loads_reproduces_its_time() {
        for want in [0.005, 0.01, 0.1, 0.4, 1.0, 3.0, 5.0] {
            let got = attack_seconds(f64::from(slider_for(attack_seconds, want)));
            assert!((got - want).abs() < want * 1e-4 + 1e-6, "attack {want}: {got}");
        }
        for want in [0.01, 0.05, 0.15, 1.0, 2.5, 8.0] {
            let got = decay_seconds(f64::from(slider_for(decay_seconds, want)));
            assert!((got - want).abs() < want * 1e-4 + 1e-6, "decay {want}: {got}");
        }
        for want in [0.015, 0.1, 1.0, 5.0, 10.0] {
            let got = release_seconds(f64::from(slider_for(release_seconds, want)));
            assert!((got - want).abs() < want * 1e-4 + 1e-6, "release {want}: {got}");
        }
        for want in [0.2, 1.0, 5.5, 20.0] {
            let got = lfo_hz(f64::from(slider_for(lfo_hz, want)));
            assert!((got - want).abs() < want * 1e-4, "lfo {want}: {got}");
        }
        for want in [0.0, 1.5, 10.0, 100.0, 700.0, 2400.0] {
            let got = fm_cents(f64::from(slider_for(fm_cents, want)));
            assert!((got - want).abs() < want * 1e-4 + 1e-6, "fm depth {want}: {got}");
        }
        // The tapers that start at zero come back exactly off, not a
        // sixteen-millionth of the way up.
        assert_eq!(slider_for(fm_cents, 0.0), 0.0);
        assert_eq!(slider_for(porta_seconds, 0.0), 0.0);
        assert_eq!(slider_for(lag_seconds, 0.0), 0.0);
    }

    #[test]
    fn the_envelope_segments_are_curved_like_a_capacitor() {
        // Measured on a Juno-60: the attack is 63% of the way up at the half
        // way point, not 50%, and the decay is at 15%.
        let mut e = OdysseyEnvelope::new(44100.0);
        e.set_times(2.0, 100.0, 100.0);
        e.sustain = 1.0;
        e.trigger();
        let mut level = 0.0;
        for _ in 0..44100 {
            level = e.tick();
        }
        assert!((level - 0.632).abs() < 0.02, "attack half way: {level:.3}");

        let mut e = OdysseyEnvelope::new(44100.0);
        e.set_times(0.001, 2.0, 100.0);
        e.sustain = 0.0;
        e.level = 1.0;
        e.enter_decay();
        let mut level = 1.0;
        for _ in 0..44100 {
            level = e.tick();
        }
        assert!((level - 0.148).abs() < 0.02, "decay half way: {level:.3}");
    }

    #[test]
    fn the_envelopes_are_independent() {
        // The AR's two sliders did not exist, and its times were assigned
        // from the ADSR's every buffer. The amplifier's release has to decide
        // how long a note rings after the key comes up whatever the filter
        // envelope is doing, and the other way round.
        let render = |ar_release: f32, adsr: [f32; 4]| {
            let mut s = synth_at(0);
            s.set_parameter(P_CUTOFF, 0.8);
            s.set_parameter(P_VCF_ENV, 0.2);
            s.set_parameter(P_AR_A, 0.0);
            s.set_parameter(P_AR_R, ar_release);
            for (i, v) in [P_ATTACK, P_DECAY, P_SUSTAIN, P_RELEASE].iter().zip(adsr) {
                s.set_parameter(*i, v);
            }
            process_buffers(&mut s, &[note_on(60, 100, 0)], 100);
            process_buffers(&mut s, &[note_off(60, 0)], 400)
        };
        // A long filter release under a short amplifier one, and the reverse.
        let stab = render(0.1, [0.0, 0.6, 0.8, 0.8]);
        let pad = render(0.7, [0.0, 0.2, 0.0, 0.0]);
        let late = 350 * 64;
        assert!(
            peak(&pad[late..]) > 10.0 * peak(&stab[late..]),
            "the AR does not decide how long the note rings: pad {} against stab {}",
            peak(&pad[late..]), peak(&stab[late..])
        );
    }

    // ── Filters ──

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

    fn filter_response(kind: u8, cutoff_norm: f64, res: f64) -> Vec<f64> {
        let mut a = Filter4023::new();
        let mut b = Filter4035::new();
        let mut c = Filter4075::new();
        (0..16384)
            .map(|i| {
                let x = if i == 0 { 1e-3 } else { 0.0 };
                (match kind {
                    0 => a.process(x, cutoff_norm, res, 44100.0),
                    1 => b.process(x, cutoff_norm, res, 44100.0),
                    _ => c.process(x, cutoff_norm, res, 44100.0),
                }) / 1e-3
            })
            .collect()
    }

    #[test]
    fn each_filter_is_the_slope_and_the_frequency_the_panel_says() {
        // The shape was always poles; the frequency was not. The two
        // four-pole boards integrated naively, which puts a section an octave
        // below its own coefficient: the slider marked 632 Hz measured its
        // -3 dB point at 141 Hz where four correctly-placed poles put it at
        // 275, a factor of 4.6 against the 2.3 the cascade owes on its own.
        //
        // The reference is the analog cascade itself — 1/(1+(f/f0)^2)^2 for
        // four poles and 1/(1+(f/f0)^2) for two — which is 12 dB or 6 dB down
        // at the cutoff and only reaches its full asymptote well above it.
        // Matching the curve is the stronger claim than matching a slope.
        //
        // Measured low on the sweep, because the reference is an analog
        // cascade and a bilinear one warps against it.
        let sr = 44100.0;
        for norm in [0.2, 0.35] {
            let f0 = cutoff_hz(norm);
            for (kind, poles) in [(0u8, 2.0f64), (1, 4.0), (2, 4.0)] {
                let ir = filter_response(kind, norm, 0.0);
                let at_dc = magnitude_at(&ir, 5.0, sr);
                for multiple in [1.0f64, 2.0, 4.0, 8.0] {
                    let want = -10.0 * poles * (1.0 + multiple * multiple).log10();
                    let got = 20.0 * (magnitude_at(&ir, f0 * multiple, sr) / at_dc).log10();
                    assert!(
                        (got - want).abs() < 0.8,
                        "filter {kind}, cutoff {f0:.0} Hz at {multiple}x: {got:.1} dB, \
                         the cascade owes {want:.1} dB"
                    );
                }
            }
        }
    }

    #[test]
    fn the_cutoff_sweep_only_ever_opens() {
        // The Rev 1 filter's sweep ran to 35 kHz through a `sin` that folds
        // above Nyquist, so the top seventh of its travel *closed* the
        // filter: the corner measured over 21 kHz at 0.85 of the slider and
        // 6 kHz at the top of it. All three now share the panel's one legend.
        let sr = 44100.0;
        assert!((cutoff_hz(0.0) - 16.0).abs() < 1e-9);
        assert!((cutoff_hz(1.0) - 16000.0).abs() < 1e-6);
        for kind in 0..3 {
            let mut previous = 0.0;
            for step in 0..=20 {
                let norm = f64::from(step) / 20.0;
                let ir = filter_response(kind, norm, 0.0);
                let at_dc = magnitude_at(&ir, 5.0, sr);
                // How much of a fixed high band gets through: monotone in the
                // slider if the corner only ever rises.
                let through = magnitude_at(&ir, 8000.0, sr) / at_dc;
                assert!(
                    through >= previous - 1e-9,
                    "filter {kind} closes between {} and {norm} of the slider",
                    norm - 0.05
                );
                previous = through;
            }
        }
    }

    #[test]
    fn every_filter_oscillates_at_the_top_of_the_resonance_travel() {
        // The panel legend reads "MIN…SELF OSC" and two presets in the bank
        // have no oscillator in the mixer at all, so the filter is the only
        // thing that can make their sound. None of the three oscillated: at
        // full resonance all of them were silent a second after the input
        // stopped.
        let sr = 44100.0;
        for kind in 0..3 {
            let mut a = Filter4023::new();
            let mut b = Filter4035::new();
            let mut c = Filter4075::new();
            let mut run = |x: f64| match kind {
                0 => a.process(x, 0.5, 1.0, sr),
                1 => b.process(x, 0.5, 1.0, sr),
                _ => c.process(x, 0.5, 1.0, sr),
            };
            for _ in 0..64 {
                run(0.5);
            }
            let mut tail = 0.0f64;
            for i in 0..(sr as usize * 3) {
                let y = run(0.0);
                if i > sr as usize * 2 {
                    tail = tail.max(y.abs());
                }
                assert!(y.is_finite(), "filter {kind} diverged");
            }
            assert!(tail > 0.02, "filter {kind} does not oscillate: tail {tail:.6}");
            assert!(tail < 4.0, "filter {kind} runs away: tail {tail:.6}");
        }
        // ...and below the knee it dies away, or every patch with the
        // resonance up would drone.
        let mut f = Filter4023::new();
        for _ in 0..64 {
            f.process(0.5, 0.5, 0.6, sr);
        }
        let mut tail = 0.0f64;
        for i in 0..(sr as usize) {
            let y = f.process(0.0, 0.5, 0.6, sr);
            if i > sr as usize / 2 {
                tail = tail.max(y.abs());
            }
        }
        assert!(tail < 1e-4, "the filter oscillates well below the knee: {tail:.6}");
    }

    #[test]
    fn the_ladder_loses_bass_at_resonance_and_the_other_two_do_not() {
        // What the three revisions are actually known for. The 4035 is a
        // transistor ladder, whose resonance feedback comes out of the
        // passband; the 4023 is a state-variable filter, whose low-pass
        // output has unity gain at DC whatever the damping does; and the
        // 4075's Norton integrators sit round a compensated input.
        //
        // Driven with a sine two and a half octaves under the corner and
        // measured in the steady state, because an impulse response of a
        // filter this resonant is still ringing at the end of any window
        // short enough to transform.
        let sr = 44100.0;
        let bass = |kind: u8, res: f64| {
            let mut a = Filter4023::new();
            let mut b = Filter4035::new();
            let mut c = Filter4075::new();
            let (mut re, mut im) = (0.0, 0.0);
            for i in 0..(sr as usize) {
                let w = TWO_PI * 30.0 * i as f64 / sr;
                let x = 0.05 * w.sin();
                let y = match kind {
                    0 => a.process(x, 0.6, res, sr),
                    1 => b.process(x, 0.6, res, sr),
                    _ => c.process(x, 0.6, res, sr),
                };
                // A single bin at the drive frequency, so that a filter still
                // ringing at its own corner is not counted as bass.
                if i > sr as usize / 2 {
                    re += y * w.cos();
                    im += y * w.sin();
                }
            }
            10.0 * (re * re + im * im).log10()
        };
        let ladder = bass(1, 0.85) - bass(1, 0.0);
        let svf = bass(0, 0.85) - bass(0, 0.0);
        let norton = bass(2, 0.85) - bass(2, 0.0);
        assert!(ladder < -8.0, "the ladder keeps its bass at resonance: {ladder:.1} dB");
        assert!(svf > -1.0, "the state-variable filter loses {svf:.1} dB of bass");
        assert!(
            (-9.0..-3.0).contains(&norton),
            "the Norton board should sit between the other two: {norton:.1} dB \
             against {svf:.1} and {ladder:.1}"
        );
    }

    #[test]
    fn the_high_pass_is_six_db_per_octave_where_the_slider_puts_it() {
        // The corner used to be placed by a difference equation that put it
        // 2.3 times low at the top of the sweep: 6.8 kHz where the slider
        // said 16.
        let sr = 44100.0;
        assert!((hpf_hz(0.0) - 16.0).abs() < 1e-9);
        assert!((hpf_hz(1.0) - 16000.0).abs() < 1e-6);
        for slider in [0.1, 0.3, 0.5, 0.7] {
            let want = hpf_hz(slider);
            let mut h = HpFilter::new();
            let ir: Vec<f64> = (0..16384)
                .map(|i| h.process(if i == 0 { 1.0 } else { 0.0 }, slider, sr))
                .collect();
            let passband = magnitude_at(&ir, 20000.0, sr);
            let drop = 20.0 * (magnitude_at(&ir, want, sr) / passband).log10();
            assert!((drop + 3.0).abs() < 0.4, "slider {slider}: {drop:.2} dB at {want:.0} Hz");
            let slope = 20.0 * (magnitude_at(&ir, want / 4.0, sr)
                / magnitude_at(&ir, want / 8.0, sr)).log10();
            assert!((slope - 6.0).abs() < 0.5, "slider {slider}: {slope:.1} dB/oct");
        }
    }

    #[test]
    fn keyboard_follow_tracks_an_octave_of_cutoff_per_octave_of_keyboard() {
        // The defect: the keyboard offset was divided by five semitones and
        // then added to a slider that spanned eleven octaves, so full follow
        // tracked at over two octaves per octave — and by a different amount
        // on each of the three filters, because each had its own sweep.
        let offset = |note: u8| (f64::from(note) - 60.0) / 12.0 / CUTOFF_OCTAVES;
        assert!((cutoff_hz(0.4 + offset(72)) / cutoff_hz(0.4) - 2.0).abs() < 1e-9);
        assert!((cutoff_hz(0.4 + offset(48)) / cutoff_hz(0.4) - 0.5).abs() < 1e-9);
    }

    // ── Oscillators and modulation ──

    #[test]
    fn sync_locks_vco2_to_vco1s_period() {
        let sr = 44100.0;
        let mut v1 = OdysseyVco::new();
        let mut v2 = OdysseyVco::new();
        v1.set_freq(200.0, sr);
        v2.set_freq(517.0, sr);
        let mut resets = 0;
        for _ in 0..(sr as usize) {
            let (_, _, reset1) = v1.tick(0.5);
            if reset1 {
                v2.reset_phase();
                resets += 1;
            }
            v2.tick(0.5);
        }
        assert!((resets as f64 - 200.0).abs() < 2.0, "master ran at {resets} Hz");
        // With sync on, the slave's own tuning changes the timbre and not the
        // pitch, which is what makes the sweep.
        let mut s = synth_at(2); // SyncLd
        s.set_parameter(P_SYNC, knob_for(1, 2));
        let a = process_buffers(&mut s, &[note_on(60, 100, 0)], 40);
        let mut s = synth_at(2);
        s.set_parameter(P_SYNC, knob_for(1, 2));
        s.set_parameter(P_VCO2_FREQ, 0.6);
        let b = process_buffers(&mut s, &[note_on(60, 100, 0)], 40);
        let diff: f32 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff > 0.01, "the slave's tuning does nothing under sync: {diff}");
    }

    #[test]
    fn ring_modulation_is_the_exclusive_or_of_the_two_pulses() {
        // ARP's ring modulator is a logic gate on the two square waves, not
        // an analog multiplier, which is what makes it harsh.
        let mut s = synth_at(0);
        s.set_parameter(P_VCO1_LEVEL, 0.0);
        s.set_parameter(P_VCO2_LEVEL, 0.0);
        s.set_parameter(P_RING_LEVEL, 1.0);
        s.set_parameter(P_RING_SRC, knob_for(1, 2));
        s.set_parameter(P_CUTOFF, 1.0);
        s.set_parameter(P_RESO, 0.0);
        s.set_parameter(P_VCF_ENV, 0.0);
        let unison = process_buffers(&mut s, &[note_on(60, 100, 0)], 60);
        // Two identical squares exclusive-ored together is a constant, which
        // the amplifier's own envelope then shapes; detune one and the
        // difference tone appears.
        let mut s = synth_at(0);
        for (i, v) in [
            (P_VCO1_LEVEL, 0.0), (P_VCO2_LEVEL, 0.0), (P_RING_LEVEL, 1.0),
            (P_RING_SRC, knob_for(1, 2)), (P_CUTOFF, 1.0), (P_RESO, 0.0), (P_VCF_ENV, 0.0),
            (P_VCO2_FREQ, tune_slider(700.0)),
        ] {
            s.set_parameter(i, v);
        }
        let fifth = process_buffers(&mut s, &[note_on(60, 100, 0)], 60);
        let diff: f32 = unison.iter().zip(fifth.iter()).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff > 1.0, "the ring modulator does not hear the interval: {diff}");
        // The gate runs off the pulse outputs whichever waveform the mixer is
        // showing, so switching the mixer to sawtooth changes nothing.
        let mut s = synth_at(0);
        for (i, v) in [
            (P_VCO1_LEVEL, 0.0), (P_VCO2_LEVEL, 0.0), (P_RING_LEVEL, 1.0),
            (P_RING_SRC, knob_for(1, 2)), (P_CUTOFF, 1.0), (P_RESO, 0.0), (P_VCF_ENV, 0.0),
            (P_VCO1_WAVE, knob_for(1, 2)), (P_VCO2_WAVE, knob_for(1, 2)),
        ] {
            s.set_parameter(i, v);
        }
        let pulsed = process_buffers(&mut s, &[note_on(60, 100, 0)], 60);
        assert_eq!(unison, pulsed, "the mixer's waveform switch reached the ring modulator");
    }

    #[test]
    fn the_sample_and_hold_is_not_the_lfo() {
        // The defect: the hold's output was scaled by the LFO's *own* depth
        // sliders and added on top of it, so twenty-four of the forty-four
        // presets got a random staircase they never asked for. Vibrato alone
        // has to be a sine.
        let mut s = synth_at(0);
        s.set_parameter(P_VCO1_FM1, 0.5); // a wide LFO vibrato
        s.set_parameter(P_VCO2_FM1, 0.5);
        s.set_parameter(P_LFO_RATE, 0.8);
        let with_lfo = process_buffers(&mut s, &[note_on(60, 100, 0)], 200);

        let mut s = synth_at(0);
        s.set_parameter(P_VCO1_FM1, 0.5);
        s.set_parameter(P_VCO2_FM1, 0.5);
        s.set_parameter(P_LFO_RATE, 0.8);
        s.set_parameter(P_SH_A, 1.0); // the whole hold section turned up
        s.set_parameter(P_SH_B, 1.0);
        let with_hold = process_buffers(&mut s, &[note_on(60, 100, 0)], 200);
        assert_eq!(with_lfo, with_hold, "the hold reaches the pitch with nothing routed to it");

        // ...and it does reach it when the FM switch says so.
        let mut s = synth_at(0);
        for (i, v) in [
            (P_VCO1_FM2, 0.5), (P_VCO2_FM2, 0.5),
            (P_VCO1_FM2_SRC, knob_for(0, 2)), (P_VCO2_FM2_SRC, knob_for(0, 2)),
            (P_LFO_RATE, 0.8), (P_SH_B, 1.0),
        ] {
            s.set_parameter(i, v);
        }
        let held = process_buffers(&mut s, &[note_on(60, 100, 0)], 200);
        let diff: f32 = with_lfo.iter().zip(held.iter()).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff > 1.0, "the hold does not reach the pitch when it is routed: {diff}");
    }

    #[test]
    fn portamento_is_a_speed_and_not_a_time() {
        // ARP's control is in seconds per octave, so a wider interval takes
        // proportionally longer to arrive.
        let travel = |from: u8, to: u8| {
            let mut s = synth_at(0);
            s.set_parameter(P_PORTAMENTO, slider_for(porta_seconds, 0.5));
            process_buffers(&mut s, &[note_on(from, 100, 0)], 4);
            process_buffers(&mut s, &[note_off(from, 0), note_on(to, 100, 0)], 1);
            let target = note_octaves(to);
            let mut samples = 0usize;
            for _ in 0..1200 {
                process_buffers(&mut s, &[], 1);
                samples += 64;
                if (s.voice.as_ref().unwrap().vco1_octaves - target).abs() < 1e-6 {
                    break;
                }
            }
            samples as f64 / 44100.0
        };
        let one = travel(60, 72);
        let two = travel(60, 84);
        assert!((one - 0.5).abs() < 0.02, "an octave took {one:.3} s at 0.5 s/oct");
        assert!((two - 1.0).abs() < 0.02, "two octaves took {two:.3} s at 0.5 s/oct");
    }

    #[test]
    fn the_lfo_covers_the_published_range() {
        assert!((lfo_hz(0.0) - 0.2).abs() < 1e-9);
        assert!((lfo_hz(1.0) - 20.0).abs() < 1e-9);
        for want in [0.2, 1.0, 6.0, 20.0] {
            let mut lfo = OdysseyLfo::new();
            lfo.rate = want;
            let mut edges = 0;
            let mut previous = 1.0;
            for _ in 0..44100 {
                let (_, square) = lfo.tick(44100.0);
                if square > 0.0 && previous < 0.0 {
                    edges += 1;
                }
                previous = square;
            }
            assert!((f64::from(edges) - want).abs() <= 1.0, "{want} Hz measured {edges}");
        }
        // Free-running: it does not restart with a note, so a tremolo is not
        // welded to the key.
        let mut s = synth_at(0);
        s.set_parameter(P_LFO_RATE, 0.5);
        process_buffers(&mut s, &[], 100);
        let phase = s.lfo.phase;
        assert!(phase > 0.0, "the LFO did not run with no key down");
    }

    #[test]
    fn the_pitch_pads_bend_and_the_middle_one_adds_vibrato() {
        // The pads had no MIDI at all: pitch bend and the modulation wheel
        // were dropped on the floor.
        let mut s = synth_at(0);
        s.set_parameter(P_CUTOFF, 0.9);
        let flat = process_buffers(&mut s, &[note_on(60, 100, 0), bend(0, 0)], 60);
        let mut s = synth_at(0);
        s.set_parameter(P_CUTOFF, 0.9);
        let centre = process_buffers(&mut s, &[note_on(60, 100, 0), bend(8192, 0)], 60);
        let diff: f32 = flat.iter().zip(centre.iter()).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff > 1.0, "the pitch pads do nothing: {diff}");

        let mut s = synth_at(0);
        s.set_parameter(P_CUTOFF, 0.9);
        s.set_parameter(P_LFO_RATE, 0.6);
        let wheeled = process_buffers(&mut s, &[note_on(60, 100, 0), cc(1, 127, 0)], 60);
        let mut s = synth_at(0);
        s.set_parameter(P_CUTOFF, 0.9);
        s.set_parameter(P_LFO_RATE, 0.6);
        let dry = process_buffers(&mut s, &[note_on(60, 100, 0)], 60);
        let diff: f32 = dry.iter().zip(wheeled.iter()).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff > 1.0, "the middle pad does not add vibrato: {diff}");
    }

    #[test]
    fn the_noise_switch_changes_the_colour_and_not_the_level() {
        let mut n = NoiseGen::new();
        let mut white = 0.0f64;
        let mut pink = 0.0f64;
        for _ in 0..200_000 {
            let w = n.white();
            white += w * w;
        }
        let mut n = NoiseGen::new();
        for _ in 0..200_000 {
            let p = n.pink();
            pink += p * p;
        }
        let ratio = (pink / white).sqrt();
        assert!((ratio - 1.0).abs() < 0.15, "pink is {ratio:.3} times white's RMS");
    }


    #[test]
    fn the_vca_gain_slider_drones_with_no_key_down() {
        // The manual: "the volume at which the audio signal always passes
        // through the VCA". Raised, the instrument sounds without a key.
        let mut s = synth_at(0);
        s.set_parameter(P_VCA_GAIN, 0.5);
        let out = process_buffers(&mut s, &[], 40);
        assert!(peak(&out) > 0.001, "the gain slider does not drone: {}", peak(&out));
        let mut s = synth_at(0);
        let out = process_buffers(&mut s, &[], 40);
        assert!(peak(&out) == 0.0, "the instrument sounds with the gain slider down");
    }
}
