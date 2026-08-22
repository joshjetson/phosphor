//! Sequential Prophet-6: six-voice analog poly with poly mod, and all 500
//! factory programs.
//!
//! Dave Smith's 2015 return to the Prophet-5. Two discrete VCOs per voice with
//! a continuously variable waveshape, a triangle sub-oscillator, noise, two
//! resonant filters in series, two envelopes, an LFO, and the section that
//! makes it a Prophet rather than another analog poly: **poly mod**, which
//! routes the filter envelope and oscillator 2 — at audio rate — into
//! oscillator 1's frequency, waveshape and pulse width and into both filter
//! cutoffs.
//!
//! ## Sources
//!
//! * *Prophet-6 Operation Manual* 2.1 (Sequential, 2021) and 1.0 (Dave Smith
//!   Instruments, 2015). Every range, enumeration and behaviour in this file
//!   that says "the manual" is from one of those two, and where they disagree
//!   the disagreement is written down at the constant. The 1.0 manual matters
//!   because the factory bank is 2015 data: its effect lists are the ones the
//!   stored effect-type bytes index into.
//! * `P6_Programs_v1.01.syx`, Sequential's factory program set of 23 July
//!   2015. See [`ROM`] and `examples/p6_rom.rs`.
//! * Sound On Sound's Prophet-6 review (Gordon Reid, 2015) for the filter
//!   lineage — see [`Ssm2040`].
//!
//! ## Where this differs from the hardware, and why
//!
//! * **The arpeggiator and the 64-step sequencer are not here.** Both are
//!   stored in the factory dump and both are sequencing rather than sound,
//!   which is the DAW's job; the parameters that feed the *effects* — BPM,
//!   and the clock-sync divisions — are on the panel, because a synced delay
//!   is a sound.
//! * **Effect A and Effect B render chorus and the two delays only.** The
//!   type selectors carry the whole v1.0 list and every effect parameter is
//!   stored and round-trips, so a program that asks for a phaser or a reverb
//!   keeps its settings and renders through the rest of the chain dry. See
//!   the [`fx`] module for exactly which of the ten do something today.
//! * **Chord memory is captured by moving the unison switch to CHD while keys
//!   are held**, which is the hardware gesture (hold a chord, press unison)
//!   with the only control this rack has. The memorised chord itself is not
//!   in the decoded parameter block, so the thirteen factory programs that
//!   select CHD arrive with an empty memory and stack six voices on the note
//!   played, which is what the hardware does with chord memory cleared.
//! * **Aftertouch works end to end.** `phosphor-midi` already parsed channel
//!   pressure and `phosphor-core`'s `midi_to_plugin_event` already dropped
//!   it — it forwarded note, control-change and pitch-bend messages and
//!   nothing else — so adding this instrument meant adding one match arm
//!   there. Polyphonic key pressure is still dropped, which is correct for
//!   this instrument: "The Prophet-6 provides monophonic (or 'channel')
//!   aftertouch."
//!
//! ## Raw values and physical units
//!
//! The factory programs are raw instrument bytes — 0–60, 0–127, 0–164,
//! 0–254, 0–255 — and every conversion into hertz, seconds, semitones or
//! decibels is this file's, in the [`raw`] module, one function per law with
//! the manual's own words above it where the manual gives them and an
//! explicit note where it does not. The instrument publishes almost no
//! physical ranges: the manual gives the oscillator's span (16 Hz to 8 kHz
//! over nine octaves), the fine tune's (a quartertone each way), the pulse
//! width's shape (square at centre, narrow at both ends) and the maximum
//! delay time (1 second), and nothing at all for the filter cutoffs, the
//! envelope times or the LFO frequency. Those three are marked as judgment
//! at their own functions.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

use crate::level::soft_saturate;

const PI: f64 = std::f64::consts::PI;
const TAU: f64 = std::f64::consts::TAU;

/// Fixed headroom trim on the output, applied after the program volume.
///
/// Sized the way the other seven are — see `OUTPUT_TRIM` in dx7.rs for the
/// full reasoning and level.rs for the gain structure — on the median program
/// of the bank playing the same C major triad at velocity 100 that
/// `instruments_are_level_matched` uses, landing inside the 0.0187 to 0.0314
/// band the rest of the rack occupies.
///
/// Two numbers set it, and they pull in opposite directions. The bank's
/// median program on that triad has to land in the 0.0187 to 0.0314 band the
/// other seven occupy, and the loudest program in the bank — 285 *Genesis 2*,
/// a slow tremolo with both filters at maximum resonance, held as an
/// eight-note chord at velocity 127 for the eleven seconds its LFO takes to
/// come round — has to stay under the master limiter's ceiling. At 0.034 the
/// median measures 0.0252 and *Genesis 2* peaks at 0.57, which is under the
/// saturator's knee, so every one of the 500 programs renders as the trimmed
/// voice sum rather than as a bounded version of it.
///
/// Those two numbers only fit together because of [`VOICE_KNEE`]. Without the
/// rails on the voice's output stage a compensated resonance loop is 14 dB
/// hotter at maximum resonance than a ladder is, the twenty-nine programs
/// that sit there tower over the rest of the bank, and *Genesis 2* on its own
/// reached past the limiter's ceiling.
const OUTPUT_TRIM: f32 = 0.034;

// ── Parameter indices ──
//
// Front-panel order. The Prophet-6's panel runs, left to right: the program
// selectors and the performance switches, then OSCILLATOR 1, OSCILLATOR 2,
// SLOP, MIXER, the two filters with FILTER ENVELOPE between them, AMPLIFIER
// ENVELOPE, LOW FREQUENCY OSCILLATOR, POLY MOD, AFTERTOUCH, DISTORT, EFFECTS,
// and the MISC PARAMETERS strip. That is the order here.
//
// `program` is first because index 0 is where the editor looks for a preset
// selector, and `bank` is second because the two are one control between them.

pub const P_PROGRAM: usize = 0;
pub const P_BANK: usize = 1;
// Oscillator 1
pub const P_OSC1_FREQ: usize = 2;
pub const P_OSC1_SHAPE: usize = 3;
pub const P_OSC1_PW: usize = 4;
pub const P_SYNC: usize = 5;
// Oscillator 2
pub const P_OSC2_FREQ: usize = 6;
pub const P_OSC2_FINE: usize = 7;
pub const P_OSC2_SHAPE: usize = 8;
pub const P_OSC2_PW: usize = 9;
pub const P_OSC2_LOW: usize = 10;
pub const P_OSC2_KEY: usize = 11;
// Slop
pub const P_SLOP: usize = 12;
// Mixer
pub const P_OSC1_LEVEL: usize = 13;
pub const P_OSC2_LEVEL: usize = 14;
pub const P_SUB_LEVEL: usize = 15;
pub const P_NOISE_LEVEL: usize = 16;
// High-pass filter
pub const P_HP_CUTOFF: usize = 17;
pub const P_HP_RESO: usize = 18;
pub const P_HP_ENV: usize = 19;
pub const P_HP_VEL: usize = 20;
pub const P_HP_KEY: usize = 21;
// Low-pass filter
pub const P_LP_CUTOFF: usize = 22;
pub const P_LP_RESO: usize = 23;
pub const P_LP_ENV: usize = 24;
pub const P_LP_VEL: usize = 25;
pub const P_LP_KEY: usize = 26;
// Filter envelope
pub const P_F_ATTACK: usize = 27;
pub const P_F_DECAY: usize = 28;
pub const P_F_SUSTAIN: usize = 29;
pub const P_F_RELEASE: usize = 30;
// Amplifier envelope
pub const P_VCA_ENV: usize = 31;
pub const P_VCA_VEL: usize = 32;
pub const P_A_ATTACK: usize = 33;
pub const P_A_DECAY: usize = 34;
pub const P_A_SUSTAIN: usize = 35;
pub const P_A_RELEASE: usize = 36;
// Low frequency oscillator
pub const P_LFO_FREQ: usize = 37;
pub const P_LFO_SHAPE: usize = 38;
pub const P_LFO_AMOUNT: usize = 39;
pub const P_LFO_FREQ1: usize = 40;
pub const P_LFO_FREQ2: usize = 41;
pub const P_LFO_PW: usize = 42;
pub const P_LFO_AMP: usize = 43;
pub const P_LFO_LP: usize = 44;
pub const P_LFO_HP: usize = 45;
// Poly mod
pub const P_PM_FILTER_ENV: usize = 46;
pub const P_PM_OSC2: usize = 47;
pub const P_PM_FREQ1: usize = 48;
pub const P_PM_SHAPE1: usize = 49;
pub const P_PM_PW1: usize = 50;
pub const P_PM_LP: usize = 51;
pub const P_PM_HP: usize = 52;
// Aftertouch
pub const P_AT_AMOUNT: usize = 53;
pub const P_AT_FREQ1: usize = 54;
pub const P_AT_FREQ2: usize = 55;
pub const P_AT_LFO: usize = 56;
pub const P_AT_AMP: usize = 57;
pub const P_AT_LP: usize = 58;
pub const P_AT_HP: usize = 59;
// Distortion
pub const P_DISTORTION: usize = 60;
// Effects
pub const P_FX_ON: usize = 61;
pub const P_FXA_TYPE: usize = 62;
pub const P_FXA_MIX: usize = 63;
pub const P_FXA_P1: usize = 64;
pub const P_FXA_P2: usize = 65;
pub const P_FXA_SYNC: usize = 66;
pub const P_FXA_DIV: usize = 67;
pub const P_FXB_TYPE: usize = 68;
pub const P_FXB_MIX: usize = 69;
pub const P_FXB_P1: usize = 70;
pub const P_FXB_P2: usize = 71;
pub const P_FXB_SYNC: usize = 72;
pub const P_FXB_DIV: usize = 73;
pub const P_BPM: usize = 74;
// Misc parameters and the performance switches
pub const P_UNISON: usize = 75;
pub const P_UNISON_MODE: usize = 76;
pub const P_KEY_MODE: usize = 77;
pub const P_GLIDE: usize = 78;
pub const P_GLIDE_MODE: usize = 79;
pub const P_GLIDE_RATE: usize = 80;
pub const P_BEND_RANGE: usize = 81;
pub const P_PAN_SPREAD: usize = 82;
pub const P_VOLUME: usize = 83;

pub const PARAM_COUNT: usize = 84;

/// Panel names, eight columns wide because that is the editor's column.
pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "program", "bank",
    "1 freq", "1 shape", "1 width", "sync",
    "2 freq", "2 fine", "2 shape", "2 width", "2 low", "2 keybd",
    "slop",
    "osc 1", "osc 2", "sub", "noise",
    "hp freq", "hp res", "hp env", "hp vel", "hp keybd",
    "lp freq", "lp res", "lp env", "lp vel", "lp keybd",
    "flt atk", "flt dec", "flt sus", "flt rel",
    "amp env", "amp vel", "amp atk", "amp dec", "amp sus", "amp rel",
    "lfo rate", "lfo wave", "lfo amt", "lfo>1", "lfo>2", "lfo>pw", "lfo>amp",
    "lfo>lp", "lfo>hp",
    "pm f env", "pm osc2", "pm>freq", "pm>shape", "pm>pw", "pm>lp", "pm>hp",
    "at amt", "at>1", "at>2", "at>lfo", "at>amp", "at>lp", "at>hp",
    "distort",
    "fx on", "a type", "a mix", "a parm1", "a parm2", "a sync", "a div",
    "b type", "b mix", "b parm1", "b parm2", "b sync", "b div", "bpm",
    "unison", "voices", "key mode",
    "glide", "gld mode", "gld rate", "bend",
    "pan", "volume",
];

// ── Panel selectors ──

/// Effect A's type list, and the first six of Effect B's.
///
/// **This is the OS 1.0 list, and it has to be.** Later firmware inserted
/// `PH3`, `rin`, `FL1` and `FL2`, so the same byte means a different effect on
/// a modern Prophet-6 — but the factory bank in [`ROM`] is from July 2015 and
/// its type bytes span exactly 0–5 for A and 0–9 for B, which is this list and
/// only this list. The 2.1 manual's own NRPN appendix agrees (FX 1 = 0–5,
/// FX 2 = 0–9) while its body text lists the longer modern set, so the
/// appendix and the data agree against the body.
const FX_A_TYPES: [&str; 6] = ["off", "bbd", "ddl", "CHO", "PH1", "PH2"];

/// Effect B adds the four reverbs. "Reverb effects are only available as
/// Effect B, since it's the last stage in the serial effects chain."
const FX_B_TYPES: [&str; 10] =
    ["off", "bbd", "ddl", "CHO", "PH1", "PH2", "HAL", "rOO", "PLA", "SPr"];

/// The eleven clock-synced delay divisions, manual page 31.
const SYNC_DIVISIONS: [&str; 11] =
    ["1", "2d", "2", "4t", "4d", "4", "8d", "8", "8t", "16d", "16"];

/// LFO shapes, manual page 34. The first is a **triangle**, not a sine: "The
/// LFO on the Prophet-6 produces a variety of waveshapes, including triangle,
/// sawtooth, reverse sawtooth, square, and random."
const LFO_SHAPES: [&str; 5] = ["tri", "saw", "rev saw", "square", "random"];

/// Unison stacking. Six voices, then chord memory, which is the seventh
/// position the manual describes: "'CHD' will then appear as a choice if you
/// step through voice stacking options".
const UNISON_MODES: [&str; 7] = ["1 voice", "2 voice", "3 voice", "4 voice", "5 voice", "6 voice", "chord"];

/// Key assign, the six modes on manual page 52.
///
/// **The order is the bank's, not the manual's bullet list.** The manual
/// presents them LO, LOr, Hi/Hir, LAS/LAr; under that reading the 500 factory
/// programs would contain 144 set to *high-note* priority and not one set to
/// last-note, with the two retrigger variants unused. Under this order —
/// the three plain modes and then the three retrigger variants, which is how
/// the manual's own prose pairs them — the same bytes read as 329 low-note,
/// 144 last-note, 22 last-with-retrigger and 5 high-note, which is what a
/// factory bank looks like. Where the manual's presentation order and the
/// data disagree about an enumeration the manual never numbers, the data wins.
const KEY_MODES: [&str; 6] = ["low", "high", "last", "low re", "high re", "last re"];

/// Glide modes, manual page 49: fixed rate, fixed rate legato-only, fixed
/// time, fixed time legato-only.
const GLIDE_MODES: [&str; 4] = ["rate", "rate A", "time", "time A"];

/// Filter keyboard tracking: "off, half, full".
const KEY_AMOUNTS: [&str; 3] = ["off", "half", "full"];

const OFF_ON: [&str; 2] = ["off", "on"];

/// How many positions a selector has, or `None` for a knob.
fn discrete_steps(index: usize) -> Option<usize> {
    match index {
        P_PROGRAM => Some(PROGRAMS_PER_BANK),
        P_BANK => Some(BANK_COUNT),
        P_SYNC | P_OSC2_LOW | P_OSC2_KEY | P_HP_VEL | P_LP_VEL | P_VCA_VEL | P_LFO_FREQ1
        | P_LFO_FREQ2 | P_LFO_PW | P_LFO_AMP | P_LFO_LP | P_LFO_HP | P_PM_FREQ1 | P_PM_SHAPE1
        | P_PM_PW1 | P_PM_LP | P_PM_HP | P_AT_FREQ1 | P_AT_FREQ2 | P_AT_LFO | P_AT_AMP
        | P_AT_LP | P_AT_HP | P_FX_ON | P_FXA_SYNC | P_FXB_SYNC | P_UNISON | P_GLIDE => Some(2),
        P_HP_KEY | P_LP_KEY => Some(KEY_AMOUNTS.len()),
        P_GLIDE_MODE => Some(GLIDE_MODES.len()),
        P_LFO_SHAPE => Some(LFO_SHAPES.len()),
        P_KEY_MODE => Some(KEY_MODES.len()),
        P_FXA_TYPE => Some(FX_A_TYPES.len()),
        P_UNISON_MODE => Some(UNISON_MODES.len()),
        P_FXB_TYPE => Some(FX_B_TYPES.len()),
        P_FXA_DIV | P_FXB_DIV => Some(SYNC_DIVISIONS.len()),
        P_BEND_RANGE => Some(BEND_RANGE_MAX + 1),
        _ => None,
    }
}

/// The widest pitch bend the panel offers, in semitones.
///
/// The manual body says "P Wheel Range: 0...12 Semitones" and the bank's own
/// values run 1 to 12; the NRPN appendix says 0–24, which nothing in the
/// instrument or in the data supports. The body wins.
const BEND_RANGE_MAX: usize = 12;

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

/// Which parameter indices are selectors (rendered as labels, not bars).
#[must_use]
pub fn is_discrete(index: usize) -> bool {
    discrete_steps(index).is_some()
}

/// The knob position one step up or down from `value`. Knobs are unchanged.
///
/// Steps by *index* rather than by adding a fraction of the travel, for the
/// reason the other banks give: adding 1/n of the range n times does not
/// arrive at 1.0, and a step boundary missed by one ulp is a keypress that
/// visibly does nothing. With a hundred programs in a bank that error would
/// accumulate over a hundred presses.
#[must_use]
pub fn step_discrete(index: usize, value: f32, up: bool) -> f32 {
    let Some(count) = discrete_steps(index) else { return value };
    let current = selector(value, count);
    knob_for(
        if up { (current + 1).min(count - 1) } else { current.saturating_sub(1) },
        count,
    )
}

/// Label for a selector position.
///
/// Takes the whole parameter block rather than one value, as the DX7's does
/// and for the same reason: the two program selectors are one control between
/// them, and the name on the program knob depends on which bank the bank knob
/// is pointing at.
#[must_use]
pub fn discrete_label(params: &[f32], index: usize) -> Option<&'static str> {
    let value = params.get(index).copied().unwrap_or(0.0);
    let count = discrete_steps(index)?;
    let step = selector(value, count);
    Some(match index {
        P_PROGRAM => program_label(program_index(
            params.get(P_BANK).copied().unwrap_or(0.0),
            value,
        )),
        P_BANK => BANK_NAMES[step],
        P_HP_KEY | P_LP_KEY => KEY_AMOUNTS[step],
        P_GLIDE_MODE => GLIDE_MODES[step],
        P_LFO_SHAPE => LFO_SHAPES[step],
        P_KEY_MODE => KEY_MODES[step],
        P_FXA_TYPE => FX_A_TYPES[step],
        P_FXB_TYPE => FX_B_TYPES[step],
        P_UNISON_MODE => UNISON_MODES[step],
        P_FXA_DIV | P_FXB_DIV => SYNC_DIVISIONS[step],
        P_BEND_RANGE => BEND_LABELS[step],
        _ => OFF_ON[step],
    })
}

/// `"0"` through `"12"`, so that the bend-range switch reads as semitones.
const BEND_LABELS: [&str; BEND_RANGE_MAX + 1] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12",
];

/// A knob's value in seconds, for the seven that measure time.
#[must_use]
pub fn param_seconds(index: usize, value: f32) -> Option<f64> {
    match index {
        P_F_ATTACK | P_F_DECAY | P_F_RELEASE | P_A_ATTACK | P_A_DECAY | P_A_RELEASE => {
            Some(raw::env_seconds(f64::from(value) * 127.0))
        }
        P_GLIDE_RATE => Some(raw::glide_seconds(f64::from(value) * 127.0)),
        _ => None,
    }
}

// ── Raw values into physical units ──

/// Every conversion from a factory program's raw byte into the units the
/// engine works in.
///
/// The instrument stores 0–60, 0–127, 0–164, 0–254 and 0–255 and publishes
/// almost none of what those mean in hertz or seconds. Each function below
/// says which it is: the manual's own words, the NRPN range plus musical
/// judgment, or — for the three the manual is silent about — judgment with the
/// reasoning written out. The panel knob is the raw value divided by its
/// maximum, so every one of these takes the raw number back.
pub mod raw {
    use super::{PI, TAU};

    /// Oscillator frequency, raw 0–60.
    ///
    /// "Sets the base oscillator frequency over a 9-octave range from 16 Hz to
    /// 8 kHz (when used with the Transpose buttons). Adjustment is in
    /// semitones." Five of those nine octaves are the knob (0–60 semitones)
    /// and the other four are the transpose buttons, which are global and are
    /// not stored with a program.
    ///
    /// Unity is 24, not the middle of the travel: 223 of the 500 factory
    /// programs sit exactly there and the next four most common values are
    /// 12, 36, 48 and 0 — the octaves either side of it. So the knob runs
    /// 32' at the bottom through 8' at 24 to 1' at the top.
    #[must_use]
    pub fn osc_semitones(v: f64) -> f64 {
        v.clamp(0.0, 60.0) - 24.0
    }

    /// Oscillator 2 fine tune, raw 0–254, centre 127.
    ///
    /// "Fine tune control with a range of a quartertone up or down. The 12
    /// o'clock position is centered. Steps are in cents (50 cents = 1/2
    /// semitone)." A quartertone is 50 cents, so the travel is ±50.
    #[must_use]
    pub fn fine_cents(v: f64) -> f64 {
        bipolar(v, 254.0) * 50.0
    }

    /// Waveshape, raw 0–254, as a position on the morph.
    ///
    /// "Triangle, Sawtooth, Pulse — ... Waveshapes are continuously variable
    /// and smoothly transition from one shape to the next as you turn the
    /// shape knob." Three labelled points across the travel puts the sawtooth
    /// at the middle, which the bank agrees with: the programs named for
    /// square waves sit at 254 and the ones named for a soft low tone at 0,
    /// and the string programs sit at 127 to 140.
    #[must_use]
    pub fn shape(v: f64) -> f64 {
        (v / 254.0).clamp(0.0, 1.0)
    }

    /// Pulse width, raw 0–255, as a duty cycle.
    ///
    /// "Changes the width of the pulse wave from a square wave when the pulse
    /// width knob is at center position, to a very narrow pulse wave when the
    /// pulse width knob is full left or right." So the control is symmetric
    /// about the square, and both ends are thin — one thin positive and one
    /// thin negative, which sound the same. The bank reads correctly against
    /// this: *Carpenter Square* and *Busted Squares* sit within a few counts
    /// of 127 and *Harpsichord* sits at 30 and 37.
    ///
    /// [`DUTY_MIN`] is where the ends stop, since a duty of zero is silence.
    #[must_use]
    pub fn duty(v: f64) -> f64 {
        let t = (v / 255.0).clamp(0.0, 1.0);
        0.5 + (t - 0.5) * (1.0 - 2.0 * DUTY_MIN)
    }

    /// The thinnest pulse the pulse-width control reaches, as a duty cycle.
    ///
    /// The knob's ends, not modulation's: poly mod and the LFO can push the
    /// width past this, and the oscillator clamps there rather than here.
    pub const DUTY_MIN: f64 = 0.04;

    /// A mixer level or any other raw 0–127 fader, as an amplitude.
    #[must_use]
    pub fn level(v: f64) -> f64 {
        (v / 127.0).clamp(0.0, 1.0)
    }

    /// A raw 0–1 switch.
    #[must_use]
    pub fn on(v: f64) -> bool {
        v >= 0.5
    }

    /// A bipolar control, raw 0–`max` with `max/2` as zero, into −1…+1.
    #[must_use]
    pub fn bipolar(v: f64, max: f64) -> f64 {
        ((v - max * 0.5) / (max * 0.5)).clamp(-1.0, 1.0)
    }

    /// Filter cutoff, raw 0–164, as a MIDI note number.
    ///
    /// **Judgment, and here is the argument.** The manual publishes no cutoff
    /// range in hertz for either filter. What it does say is that with
    /// keyboard tracking at full "the filter-generated pitch [will] follow the
    /// keyboard in tune (i.e. in semitones)" and at half "in quarter tones" —
    /// so the filter's control voltage is calibrated in semitones, and the
    /// tracking amount is a plain 1 and 1/2 semitone per key. The cutoff
    /// parameter's own range is 164, which is a semitone count and not a
    /// fraction of anything; reading it as a MIDI note number is the reading
    /// that makes the knob and the tracking share one unit.
    ///
    /// It also puts the bank where a bank should be. The 500 programs' median
    /// cutoff is 58, which is 233 Hz, and their median low-pass envelope
    /// amount adds four octaves on top of it — a filter that opens to 3 or 4
    /// kHz under the envelope, which is what an analog polysynth sounds like.
    /// The top of the travel, 164, is 106 kHz: deliberately past the audible
    /// band so that the last stretch of the knob is genuinely "open", which is
    /// how the twelve programs that sit there (*Hi Hat* among them) use it.
    #[must_use]
    pub fn cutoff_note(v: f64) -> f64 {
        v.clamp(0.0, 164.0)
    }

    /// A note number as a frequency in hertz.
    #[must_use]
    pub fn note_hz(note: f64) -> f64 {
        440.0 * ((note - 69.0) / 12.0).exp2()
    }

    /// Filter resonance, raw 0–255, as a fraction of the travel.
    #[must_use]
    pub fn resonance(v: f64) -> f64 {
        (v / 255.0).clamp(0.0, 1.0)
    }

    /// How far a filter envelope amount at full can move a cutoff, in
    /// semitones.
    ///
    /// The envelope amount is raw 0–254 about a centre of 127, so ±127
    /// counts. Reading a count as a semitone of cutoff — the same unit the
    /// cutoff knob and the keyboard tracking use — makes the whole thing one
    /// scale, and gives an envelope at full 10.6 octaves of travel, which is
    /// the whole audible band and then some. *Filter Sweep*'s stored +86
    /// counts from a cutoff of 0 is then a sweep from 8 Hz to 1.2 kHz.
    pub const ENV_SEMITONES: f64 = 127.0;

    /// Envelope segment time, raw 0–127, in seconds.
    ///
    /// **Judgment.** Neither manual publishes an envelope time, in either
    /// direction, for either envelope; the NRPN table gives 0–127 and nothing
    /// else. Ten seconds at the top is the range the rest of this class of
    /// instrument uses and the range the rack's other envelopes already have.
    ///
    /// The exponent is what the *bank* settled, between the two laws that
    /// bracket it. A cubic law turns *Hi Hat*'s stored amplitude decay of 36
    /// into a 230 ms open hat; a fourth-power law turns it into 64 ms, which
    /// is a closed hat but leaves the bank's median filter decay at 0.35 s,
    /// short for an analog polysynth. Three and a half puts the hat at 121 ms
    /// and the median filter decay at 0.53 s, and leaves the pads where their
    /// names want them: *Thick Low Strings*' attack of 62 is 0.36 s and
    /// *Pas//Tense//Strings*' release of 79 is 1.1 s.
    #[must_use]
    pub fn env_seconds(v: f64) -> f64 {
        let t = (v / 127.0).clamp(0.0, 1.0);
        (ENV_MAX_S * t * t * t * t.sqrt()).max(ENV_MIN_S)
    }

    /// The shortest segment the envelope will produce. Half a millisecond is
    /// under a sample at every rate this runs at, so the bottom of the knob is
    /// an instant transition rather than a slow one.
    pub const ENV_MIN_S: f64 = 0.0005;
    /// The longest segment, at the top of the knob.
    pub const ENV_MAX_S: f64 = 10.0;

    /// LFO frequency, raw 0–255, in hertz.
    ///
    /// **Judgment.** The manual gives no numbers: "Though most often used for
    /// low-frequency modulation, the Prophet-6 LFO can actually function at
    /// speeds that extend into the audible range for extreme effects." So the
    /// top has to be audio rate and the bottom has to be slow enough for a
    /// sweep that takes half a minute. Geometric between them, because that is
    /// what a rate control is, which puts the middle of the knob at 3.9 Hz —
    /// a vibrato — and the bank's median at exactly that.
    #[must_use]
    pub fn lfo_hz(v: f64) -> f64 {
        let t = (v / 255.0).clamp(0.0, 1.0);
        LFO_MIN_HZ * (LFO_MAX_HZ / LFO_MIN_HZ).powf(t)
    }

    pub const LFO_MIN_HZ: f64 = 0.03;
    pub const LFO_MAX_HZ: f64 = 500.0;

    /// Slop, raw 0–127, as the widest detuning it will reach, in cents.
    ///
    /// "Slop amount is adjustable from subtle, barely perceptible amounts to
    /// wildly out of tune." Out of tune is a semitone; barely perceptible is a
    /// couple of cents, which is what the squared taper buys — a stored 16,
    /// the bank's most common non-zero setting, is 1.6 cents.
    #[must_use]
    pub fn slop_cents(v: f64) -> f64 {
        let t = (v / 127.0).clamp(0.0, 1.0);
        SLOP_MAX_CENTS * t * t
    }

    pub const SLOP_MAX_CENTS: f64 = 100.0;

    /// Glide, raw 0–127, in seconds — an octave's worth in the two fixed-rate
    /// modes, the whole transition in the two fixed-time modes.
    ///
    /// **Judgment**: the manual describes the four modes and never times any
    /// of them. Geometric from a millisecond to ten seconds, which is the same
    /// span the envelopes have and puts the bank's stock 70 at 0.16 s.
    #[must_use]
    pub fn glide_seconds(v: f64) -> f64 {
        let t = (v / 127.0).clamp(0.0, 1.0);
        GLIDE_MIN_S * (GLIDE_MAX_S / GLIDE_MIN_S).powf(t)
    }

    pub const GLIDE_MIN_S: f64 = 0.001;
    pub const GLIDE_MAX_S: f64 = 10.0;

    /// Tempo, raw 30–250, in beats per minute. The one parameter the NRPN
    /// table stores in its own units.
    #[must_use]
    pub fn bpm(v: f64) -> f64 {
        v.clamp(30.0, 250.0)
    }

    /// A one-pole coefficient for a corner at `hz`.
    #[must_use]
    pub fn one_pole(hz: f64, sr: f64) -> f64 {
        1.0 - (-TAU * hz / sr).exp()
    }

    /// The TPT prewarped integrator gain for a corner at `hz`.
    #[must_use]
    pub fn tpt_g(hz: f64, sr: f64) -> f64 {
        (PI * hz.clamp(1.0, sr * 0.49) / sr).tan()
    }
}

// ── The factory programs ──

/// The five banks, as the front panel numbers them.
pub const BANK_NAMES: [&str; BANK_COUNT] = ["bank 1", "bank 2", "bank 3", "bank 4", "bank 5"];
pub const BANK_COUNT: usize = 5;
/// Programs in a bank. [`P_PROGRAM`] picks one of these; [`P_BANK`] picks the
/// bank, exactly as the instrument's bank and program buttons do.
pub const PROGRAMS_PER_BANK: usize = 100;
/// Every factory program, across all five banks.
pub const PROGRAM_COUNT: usize = BANK_COUNT * PROGRAMS_PER_BANK;

/// Bytes per program in [`ROM`]: the 107 parameter bytes and the 20-character
/// name that follows them.
const PACKED_PROGRAM: usize = 127;

/// Sequential's factory program set, as the instrument stores it.
///
/// `P6_Programs_v1.01.syx`, published 23 July 2015, unpacked out of the DSI
/// packed-MS-bit SysEx format and stripped to the part that is a program: the
/// 107 parameter bytes and the name. The 64-step sequencer, the byte after the
/// name and the 128 bytes of uninitialised firmware RAM that the dump routine
/// copies out with every program are all dropped.
///
/// Kept as the machine's own bytes and decoded at startup rather than
/// transcribed into source, for the reason the DX7's ROM gives: 63 KB of data
/// plus a documented byte map is exact and testable, where 500 struct literals
/// is neither. `examples/p6_rom.rs` is the whole provenance — run it against
/// the SysEx file and these bytes come out again.
///
/// **The byte map is not published.** Neither manual gives an offset table and
/// no open-source Prophet-6 editor exists; the map in [`raw_offset`] was
/// established from the bank itself, against the parameter inventory and
/// ranges in the manual's NRPN appendix. The three assignments most likely to
/// be wrong are called out there.
const ROM: &[u8; PROGRAM_COUNT * PACKED_PROGRAM] = include_bytes!("p6_programs.bin");

/// Where each panel control's raw byte lives in a program block, and the
/// maximum that byte can hold.
///
/// The offsets are empirical. What fixed them, briefly, for the ones that were
/// not simply the only field of their range:
///
/// * **The envelopes interleave.** Bytes 35–42 are filter, VCA, filter, VCA,
///   not two contiguous ADSRs. A VCA with decay 0 *and* sustain 0 is silent:
///   the pair (38, 40) is (0,0) in 0 of the 500 programs and the pair (37, 39)
///   in 40 of them, so 36/38/40/42 is the amplifier.
/// * **Which env amount belongs to the high-pass.** Of the 105 programs whose
///   byte 30 is off centre, 68% use the high-pass filter at all against a 39%
///   base rate; byte 29 shows no enrichment whatever. So 30 is the high-pass
///   amount and 29 the low-pass.
/// * **Which frequency byte is oscillator 1.** Oscillator 1 is the sync slave,
///   and sync is only interesting with the slave above the master: across the
///   122 sync programs byte 0 exceeds byte 1 in 81 cases against 21 the other
///   way, and across the 378 non-sync programs the same comparison is flat.
/// * **Sub octave against noise.** Programs with byte 10 high are 5.9× enriched
///   for noise names — *Hi Hat*, *Harsh Stone Drums*, *Noisy Horror* — and
///   programs with byte 9 high are enriched for bass names.
/// * **The effects bypass.** Byte 46 is 0 for all 40 of the bank's Prophet-5
///   ports and 1 for 80% of everything else, which under that base rate is a
///   probability of about 1e-28.
///
/// The softest are the destination bits: the LFO's six, aftertouch's six and
/// poly mod's five are each a contiguous run of 0/1 bytes, and where each run
/// *starts and ends* is certain — the bytes either side hold values well past
/// 1 — but the order within a run is not printed anywhere. Assuming the NRPN
/// table's order gets index 2 right in two runs independently (the pulse-width
/// destination is the most-used bit of both the LFO run and the poly-mod run,
/// and the NRPN table puts pulse width at index 2 of both) and gets three
/// other bits **wrong**, which the bank itself says:
///
/// * **Byte 69 is the LFO's amp destination, not its high-pass one.** The
///   manual gives a recipe for a gated VCA — "set the vca env amount to zero,
///   route the LFO square wave to amp with an initial amt setting of 100%" —
///   and the bank contains exactly three programs with a VCA envelope amount
///   of zero. All three sit at an LFO initial amount of 254 or 255, which is
///   the rest of that recipe, and all three have byte 69 set; no other bit of
///   the run is shared by all three. Under the NRPN order those three
///   programs have nothing routed to the amplifier at all and are silent.
/// * **Byte 68 is the LFO's high-pass destination, not its low-pass one.** A
///   high-pass destination is only meaningful on a program that uses the
///   high-pass filter, which 39% of the bank does. Of the 84 programs that set
///   byte 68, 61% do — four standard deviations up. No other bit of the run
///   passes 1.7. That puts the low-pass at byte 67, and the resulting usage
///   counts are the ones a factory bank should have: LFO to the low-pass on
///   151 programs and to the amplifier on 88, where the NRPN order has it the
///   other way round.
/// * **Byte 75 is aftertouch's high-pass destination, not its VCA one.** The
///   same test: 68% of the 50 programs that set byte 75 use the high-pass
///   filter, again four standard deviations up, and again nothing else in the
///   run passes 1.9. So bytes 74 and 75 are the other way round from the NRPN
///   table, and the rest of that run is left as it was.
///
/// Poly mod's five destination bits were tested the same way and nothing moved
/// them: the run already puts its two filter destinations last, and the
/// high-pass test on byte 83 reaches only 1.6 standard deviations on 52
/// programs, which is not enough to act on. They are the least certain five
/// assignments in this file.
fn raw_offset(index: usize) -> Option<(usize, f64)> {
    Some(match index {
        P_OSC1_FREQ => (0, 60.0),
        P_OSC2_FREQ => (1, 60.0),
        P_OSC2_FINE => (2, 254.0),
        P_OSC1_SHAPE => (3, 254.0),
        P_OSC2_SHAPE => (4, 254.0),
        P_OSC1_PW => (5, 255.0),
        P_OSC2_PW => (6, 255.0),
        P_OSC1_LEVEL => (7, 127.0),
        P_OSC2_LEVEL => (8, 127.0),
        P_SUB_LEVEL => (9, 127.0),
        P_NOISE_LEVEL => (10, 127.0),
        P_SYNC => (11, 1.0),
        P_OSC2_KEY => (12, 1.0),
        P_OSC2_LOW => (13, 1.0),
        P_GLIDE_RATE => (14, 127.0),
        P_GLIDE_MODE => (15, 3.0),
        P_GLIDE => (16, 1.0),
        P_BEND_RANGE => (17, BEND_RANGE_MAX as f64),
        P_SLOP => (18, 127.0),
        P_LP_CUTOFF => (19, 164.0),
        P_LP_RESO => (20, 255.0),
        P_LP_KEY => (21, 2.0),
        P_LP_VEL => (22, 1.0),
        P_HP_CUTOFF => (23, 164.0),
        P_HP_RESO => (24, 255.0),
        P_HP_KEY => (25, 2.0),
        P_HP_VEL => (26, 1.0),
        P_VOLUME => (27, 127.0),
        P_PAN_SPREAD => (28, 127.0),
        P_LP_ENV => (29, 254.0),
        P_HP_ENV => (30, 254.0),
        P_VCA_ENV => (31, 127.0),
        P_F_ATTACK => (35, 127.0),
        P_A_ATTACK => (36, 127.0),
        P_F_DECAY => (37, 127.0),
        P_A_DECAY => (38, 127.0),
        P_F_SUSTAIN => (39, 127.0),
        P_A_SUSTAIN => (40, 127.0),
        P_F_RELEASE => (41, 127.0),
        P_A_RELEASE => (42, 127.0),
        P_VCA_VEL => (43, 1.0),
        P_FXA_TYPE => (44, 5.0),
        P_FXB_TYPE => (45, 9.0),
        P_FX_ON => (46, 1.0),
        P_FXA_MIX => (48, 127.0),
        P_FXB_MIX => (49, 127.0),
        P_FXA_P1 => (50, 255.0),
        P_FXB_P1 => (51, 255.0),
        P_FXA_P2 => (52, 127.0),
        P_FXB_P2 => (53, 127.0),
        P_FXA_SYNC => (54, 1.0),
        P_FXB_SYNC => (55, 1.0),
        P_FXA_DIV => (56, 10.0),
        P_FXB_DIV => (57, 10.0),
        P_DISTORTION => (58, 127.0),
        P_LFO_FREQ => (59, 255.0),
        P_LFO_SHAPE => (62, 4.0),
        P_LFO_AMOUNT => (63, 255.0),
        P_LFO_FREQ1 => (64, 1.0),
        P_LFO_FREQ2 => (65, 1.0),
        P_LFO_PW => (66, 1.0),
        P_LFO_LP => (67, 1.0),
        P_LFO_HP => (68, 1.0),
        P_LFO_AMP => (69, 1.0),
        P_AT_AMOUNT => (70, 254.0),
        P_AT_FREQ1 => (71, 1.0),
        P_AT_FREQ2 => (72, 1.0),
        P_AT_LP => (73, 1.0),
        P_AT_AMP => (74, 1.0),
        P_AT_HP => (75, 1.0),
        P_AT_LFO => (76, 1.0),
        P_PM_FILTER_ENV => (77, 254.0),
        P_PM_OSC2 => (78, 254.0),
        P_PM_FREQ1 => (79, 1.0),
        P_PM_SHAPE1 => (80, 1.0),
        P_PM_PW1 => (81, 1.0),
        P_PM_LP => (82, 1.0),
        P_PM_HP => (83, 1.0),
        P_UNISON => (84, 1.0),
        P_UNISON_MODE => (85, 6.0),
        P_KEY_MODE => (86, 5.0),
        P_BPM => (87, 250.0),
        _ => return None,
    })
}

/// One factory program, decoded once and shared by every instance.
#[derive(Clone, Copy)]
struct Program {
    /// The panel, as knob positions. The program and bank selectors are
    /// filled in by the caller, since they are where the program came from.
    panel: [f32; PARAM_COUNT],
    /// The 20 characters the instrument stores, trimmed of trailing spaces.
    name: [u8; 20],
    name_len: u8,
    /// How much of the name fits the editor's twelve columns.
    label_len: u8,
}

impl Program {
    fn decode(block: &[u8]) -> Self {
        let mut panel = [0.0f32; PARAM_COUNT];
        for (index, slot) in panel.iter_mut().enumerate() {
            *slot = match raw_offset(index) {
                // A selector reads back as the middle of its step, so that
                // stepping away from a loaded program and back arrives at the
                // same byte. A knob reads back as its share of its range.
                Some((offset, max)) => {
                    let value = f64::from(block[offset]).min(max);
                    match discrete_steps(index) {
                        Some(count) => knob_for((value as usize).min(count - 1), count),
                        None => (value / max) as f32,
                    }
                }
                None => 0.0,
            };
        }
        let mut name = [b' '; 20];
        name.copy_from_slice(&block[107..127]);
        let name_len = name.iter().rposition(|c| *c != b' ').map_or(0, |i| i + 1);
        Self {
            panel,
            name,
            name_len: name_len as u8,
            label_len: name_len.min(LABEL_WIDTH) as u8,
        }
    }

    fn name(&'static self) -> &'static str {
        std::str::from_utf8(&self.name[..self.name_len as usize]).unwrap_or("?")
    }

    fn label(&'static self) -> &'static str {
        let end = self.name[..self.label_len as usize]
            .iter()
            .rposition(|c| *c != b' ')
            .map_or(0, |i| i + 1);
        std::str::from_utf8(&self.name[..end]).unwrap_or("?")
    }
}

/// How much of a name the editor's parameter column can show.
///
/// The FX panel is 24 columns and spends 12 of them on the indicator and the
/// parameter's own name, so a selector's label gets the other 12. Sequential's
/// names run to 20 characters — *OldestTrickInTheBook*, *Pulsing Filter
/// Sweep* — so the panel shows the first 12 and [`program_name`] keeps the
/// whole thing for anything with room to print it. Truncating at 12 collides
/// twice in 500, and both collisions are programs the factory bank itself
/// named the same thing.
const LABEL_WIDTH: usize = 12;

/// The 500 factory programs, decoded once for the whole process.
fn programs() -> &'static [Program; PROGRAM_COUNT] {
    static DECODED: std::sync::OnceLock<Box<[Program; PROGRAM_COUNT]>> =
        std::sync::OnceLock::new();
    DECODED.get_or_init(|| {
        let mut bank = Box::new(
            [Program { panel: [0.0; PARAM_COUNT], name: [b' '; 20], name_len: 0, label_len: 0 };
                PROGRAM_COUNT],
        );
        for (slot, block) in bank.iter_mut().zip(ROM.chunks_exact(PACKED_PROGRAM)) {
            *slot = Program::decode(block);
        }
        bank
    })
}

/// Which bank the bank knob is pointing at.
#[must_use]
pub fn bank_index(value: f32) -> usize {
    selector(value, BANK_COUNT)
}

/// Which program of the selected bank the program knob is pointing at.
#[must_use]
pub fn patch_index(value: f32) -> usize {
    selector(value, PROGRAMS_PER_BANK)
}

/// The absolute program number, 0–499, that the two knobs select together.
#[must_use]
pub fn program_index(bank: f32, program: f32) -> usize {
    bank_index(bank) * PROGRAMS_PER_BANK + patch_index(program)
}

/// The `(bank, program)` knob positions that select program number `index`.
#[must_use]
pub fn program_knobs(index: usize) -> (f32, f32) {
    let index = index.min(PROGRAM_COUNT - 1);
    (
        knob_for(index / PROGRAMS_PER_BANK, BANK_COUNT),
        knob_for(index % PROGRAMS_PER_BANK, PROGRAMS_PER_BANK),
    )
}

/// A factory program's name, all 20 characters of it, trailing spaces removed.
#[must_use]
pub fn program_name(index: usize) -> &'static str {
    programs()[index.min(PROGRAM_COUNT - 1)].name()
}

/// As much of the name as the editor's column fits. See [`LABEL_WIDTH`].
#[must_use]
pub fn program_label(index: usize) -> &'static str {
    programs()[index.min(PROGRAM_COUNT - 1)].label()
}

/// The whole panel for a factory program, for a caller that wants to load one
/// without an engine around it — the editor's program knob, a level
/// measurement, a test.
#[must_use]
pub fn params_for_program(bank: f32, program: f32) -> [f32; PARAM_COUNT] {
    let index = program_index(bank, program);
    let mut out = programs()[index].panel;
    out[P_BANK] = knob_for(index / PROGRAMS_PER_BANK, BANK_COUNT);
    out[P_PROGRAM] = knob_for(index % PROGRAMS_PER_BANK, PROGRAMS_PER_BANK);
    out
}

/// The panel the instrument loads with: bank 1, program 0, *Brassed Off*.
#[must_use]
pub fn param_defaults() -> [f32; PARAM_COUNT] {
    params_for_program(0.0, 0.0)
}

// ── The oscillator ──
//
// One knob per oscillator, morphing triangle → sawtooth → pulse, with the
// pulse width applying across the pulse half of the travel. ONE waveform at a
// time: the Prophet-5 mixed a sawtooth and a pulse together, and this
// instrument replaced that with a single continuous control, which is the
// biggest single difference between the two panels.
//
// Crossfading a triangle with a sawtooth is not a shape between them — it is a
// shape whose peak has dropped to 0.5 while its trough stayed at -1, so the
// morph has a level dip and a DC wander halfway along. What the three shapes
// *are* is one family, a **trapezoid** described by how long it spends rising,
// high, falling and low:
//
// ```text
//   triangle   rise 1/2, high 0,   fall 1/2, low 0
//   sawtooth   rise 1,   high 0,   fall 0,   low 0
//   pulse      rise 0,   high d,   fall 0,   low 1-d
// ```
//
// and every position on the knob is one member of it.
//
// The band-limiting is the corner-pair polyBLAMP scheme `phatty.rs` uses, and
// the argument for it is written out there: a trapezoid has four corners per
// cycle and no steps at all, an "instantaneous" edge is a trapezoid with an
// edge [`EDGE_MIN`] of a cycle wide, and the two corner corrections at the
// ends of a vanishing edge converge exactly on the polyBLEP of the step it
// becomes — so one code path covers the triangle's corners and the pulse's
// edges without a special case for either. It is written out again here rather
// than shared because the two instruments morph through different shapes in a
// different order, and because this one syncs the other way round.

/// The shortest rise or fall the morph will produce, as a fraction of a cycle.
const EDGE_MIN: f64 = 1.0e-6;

/// The trapezoid a shape knob and a pulse-width knob ask for together.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shape {
    rise: f64,
    high: f64,
    fall: f64,
    /// The raw trapezoid's mean, `high - low` — the ramps average to zero.
    mean: f64,
    /// What the DC-removed shape is divided by to bring its peak back to 1.
    ///
    /// A rectangle of duty `d` with its mean removed reaches `1 + |mean|` at
    /// one rail, which is 1.92 at the ends of the pulse-width travel. An
    /// analog pulse really does that and it is why a thin-pulse patch on a
    /// real Prophet is a peak-meter problem; normalising costs the thin end
    /// about 6 dB against the square and buys a hard bound of ±1 at every
    /// knob position, which in a rack where nothing may reach the master
    /// limiter on its own is the trade to make.
    scale: f64,
}

impl Shape {
    /// The morph. Two halves rather than the Little Phatty's three thirds:
    /// this panel's legend is triangle, sawtooth, pulse, so the sawtooth is
    /// the middle of the travel.
    fn at(shape: f64, duty: f64) -> Self {
        let w = shape.clamp(0.0, 1.0);
        let d = duty.clamp(raw::DUTY_MIN, 1.0 - raw::DUTY_MIN);
        let e = EDGE_MIN;
        let (rise, high, fall) = if w < 0.5 {
            // Triangle → sawtooth: the rise takes over the cycle while the
            // fall shrinks to an edge. Amplitude never moves.
            let a = w * 2.0;
            let rise = 0.5 + a * (0.5 - e);
            (rise, 0.0, 1.0 - rise)
        } else {
            // Sawtooth → pulse: the ramp shortens and a flat top and bottom
            // grow out of it in the ratio the pulse width asks for, so that
            // the shape arrives at exactly that duty cycle.
            let b = (w - 0.5) * 2.0;
            let span = 1.0 - 2.0 * e;
            ((1.0 - e) - b * span, b * d * span, e)
        };
        let low = 1.0 - rise - high - fall;
        let mean = high - low;
        Self { rise, high, fall, mean, scale: 1.0 / (1.0 + mean.abs()) }
    }

    /// The waveform at a phase, DC removed and peak-normalised.
    #[inline]
    fn value(&self, phase: f64) -> f64 {
        let a = self.rise;
        let b = a + self.high;
        let c = b + self.fall;
        let raw = if phase < a {
            2.0 * phase / a - 1.0
        } else if phase < b {
            1.0
        } else if phase < c {
            1.0 - 2.0 * (phase - b) / self.fall
        } else {
            -1.0
        };
        (raw - self.mean) * self.scale
    }

    /// The two edges, as (start phase, width, height).
    ///
    /// Edges rather than the four corners they add up to, because the two
    /// corners of an edge are a *pair*: their slope changes are equal and
    /// opposite, and at the thin end of the morph each is a million times the
    /// waveform's own scale. Fire one without the other and the output takes a
    /// spike of that size.
    #[inline]
    fn edges(&self) -> [(f64, f64, f64); 2] {
        [(0.0, self.rise, 2.0), (self.rise + self.high, self.fall, -2.0)]
    }
}

/// A stretch of phase covered inside one sample: where it starts, how far it
/// runs, and where in the sample it begins and ends. The last two are only
/// anything but 0 and 1 when a hard-sync reset splits the sample in two.
#[derive(Debug, Clone, Copy)]
struct Stretch {
    from: f64,
    span: f64,
    t0: f64,
    t1: f64,
}

impl Stretch {
    #[inline]
    fn place(&self, reached: f64) -> f64 {
        self.t0 + (reached / self.span) * (self.t1 - self.t0)
    }
}

/// The two samples a band-limiting correction is spread over.
#[derive(Debug, Clone, Copy, Default)]
struct Correction {
    before: f64,
    after: f64,
}

/// A morphing trapezoid oscillator with polyBLAMP corners and hard sync.
#[derive(Debug, Clone)]
struct Osc {
    phase: f64,
    /// The sample computed on the previous call, still open to corrections
    /// from events in this one — the one sample of latency a two-sided corner
    /// correction needs. Every oscillator in the voice is delayed identically,
    /// so sync timing and relative phase are unaffected.
    held: f64,
    /// Per edge, once its leading corner has been corrected: where its
    /// trailing corner is and exactly what correction it will make. Captured
    /// when the edge opens so that the pair cancels by construction however
    /// far poly mod moves the shape in between.
    closing: [Option<(f64, f64)>; 2],
}

impl Osc {
    fn new(phase: f64) -> Self {
        Self { phase, held: 0.0, closing: [None; 2] }
    }

    fn reset(&mut self, phase: f64) {
        self.phase = phase;
        self.held = 0.0;
        self.closing = [None; 2];
    }

    /// One sample. `dt` is the phase advance, `sync_at` the fraction of this
    /// step at which an external reset arrives.
    #[inline]
    fn tick(&mut self, dt: f64, shape: &Shape, sync_at: Option<f64>) -> f64 {
        let mut fix = Correction::default();
        let end;

        if let Some(u) = sync_at {
            let u = u.clamp(0.0, 1.0);
            let span = u * dt;
            self.walk(shape, Stretch { from: self.phase, span, t0: 0.0, t1: u }, dt, &mut fix);
            let at = wrap_phase(self.phase + span);
            let jump = shape.value(0.0) - shape.value(at);
            let rest = 1.0 - u;
            fix.before += 0.5 * jump * rest * rest;
            fix.after -= 0.5 * jump * u * u;
            self.closing = [None; 2];
            let span = rest * dt;
            self.walk(shape, Stretch { from: 0.0, span, t0: u, t1: 1.0 }, dt, &mut fix);
            end = span;
        } else {
            let stretch = Stretch { from: self.phase, span: dt, t0: 0.0, t1: 1.0 };
            self.walk(shape, stretch, dt, &mut fix);
            end = wrap_phase(self.phase + dt);
        }

        let out = self.held + fix.before;
        self.held = shape.value(end) + fix.after;
        self.phase = end;
        out
    }

    /// Where in this sample the phase crosses the top of its cycle, for an
    /// oscillator that is the sync master. Read before the tick, because the
    /// tick is what moves the phase past it.
    #[inline]
    fn wraps_at(&self, dt: f64) -> Option<f64> {
        if dt > 0.0 && self.phase + dt >= 1.0 {
            Some((1.0 - self.phase) / dt)
        } else {
            None
        }
    }

    #[inline]
    fn walk(&mut self, shape: &Shape, stretch: Stretch, dt: f64, fix: &mut Correction) {
        let Stretch { from: start, span, .. } = stretch;
        if span <= 0.0 {
            return;
        }
        let scale = dt * shape.scale;
        for (index, (at, width, height)) in shape.edges().into_iter().enumerate() {
            if width <= 0.0 {
                continue;
            }
            if let Some((phase, m)) = self.closing[index] {
                let reached = (phase - start).rem_euclid(1.0);
                if reached < span {
                    Self::corner(m, stretch.place(reached), fix);
                    self.closing[index] = None;
                }
            }
            let reached = (at - start).rem_euclid(1.0);
            if reached < span {
                if let Some((_, m)) = self.closing[index].take() {
                    Self::corner(m, stretch.place(reached), fix);
                }
                let m = height / width * scale;
                Self::corner(m, stretch.place(reached), fix);
                if width < span {
                    Self::corner(-m, stretch.place((reached + width).min(span)), fix);
                } else {
                    self.closing[index] = Some(((at + width).rem_euclid(1.0), -m));
                }
            }
        }
    }

    /// One corner: a slope change of `m` output units per sample, `t` of the
    /// way through the sample, spread over the two samples that straddle it.
    #[inline]
    fn corner(m: f64, t: f64, fix: &mut Correction) {
        let back = 1.0 - t;
        fix.before += m * back * back * back / 6.0;
        fix.after += m * t * t * t / 6.0;
    }
}

#[inline]
fn wrap_phase(p: f64) -> f64 {
    p - p.floor()
}

/// Rational tanh: one divide instead of a libm call, within 0.5% of the real
/// thing over the range these filters put through it.
#[inline]
fn tanh_approx(x: f64) -> f64 {
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

// ── The low-pass filter ──
//
// "The Low-Pass Filter is a 4-pole, 24 dB per-octave, resonant filter",
// described by Sequential as inspired by the original Prophet-5's — which
// means the **SSM2040** of the Rev 1 and Rev 2 instruments, not the CEM3320 of
// the Rev 3 and not a Moog transistor ladder. That distinction is the whole
// reason this filter is written out rather than borrowed from `phatty.rs` or
// `synth.rs`, both of which are ladders.
//
// Two differences, and they are what a listener hears:
//
// * **The passband survives resonance.** A ladder subtracts its feedback from
//   the signal at the input of the first transistor pair with nothing to make
//   it up, so its DC gain is `1/(1+k)` — at the top of the travel a ladder is
//   15 dB down at the bottom end and a resonant bass patch has no bass left.
//   That loss is the sound of a Moog and it is deliberate there. The 2040's
//   summing stage is compensated, and `resonance_keeps_its_bass_where_a_ladder
//   _loses_it` measures the two against each other and holds the gap open.
// * **It is cleaner.** A ladder's nonlinearity is four transistor pairs in the
//   forward path, one per stage, so every stage distorts. The 2040 is an OTA
//   design whose forward path stays linear over the levels an oscillator hands
//   it, and the only saturation here is in the feedback path, which is what
//   bounds the self-oscillation rather than what colours the sound.
//
// Self-oscillation is the manual's: "high levels of resonance can cause the
// filter to self oscillate and generate its own pitch", and the onset is
// smooth and early rather than a switch at the top of the knob.

/// How much feedback the loop can be given. Four correctly-placed poles reach
/// half a turn of phase with a quarter of the gain left, so 4.0 is exactly
/// marginal and the top of the travel has to sit past it to oscillate.
const LP_RES_MAX: f64 = 4.6;

/// How much of the resonance feedback the input stage makes up.
///
/// 1.0 would restore the passband exactly and 0.0 is a ladder. The 2040 sits
/// near the top of that range. Measured through the whole instrument against
/// the ladder in `phatty.rs`, on a low sawtooth with the filter wide open:
/// between no resonance and full resonance the ladder loses 15.5 dB under
/// 150 Hz and this filter *gains* 6.5, a gap of 22 dB. The linear analysis
/// says the compensated loop should lose 0.7 dB at the bottom end; the rest
/// is the saturation in the feedback path letting a little more signal past
/// as it starts to compress, which is what an OTA filter driven hard does.
/// See [`LP_FEEDBACK_HEADROOM`] for what it looked like before that was
/// bounded — 9 dB of bloom rather than 6.5, and broadband, because the loop
/// state was hitting a hard clamp inside a resonant circuit.
const LP_PASSBAND_COMP: f64 = 0.9;

/// Where on the resonance travel the loop stops losing and starts producing.
/// Earlier than a ladder's, which is the "smooth onset" half of the SSM
/// character.
const LP_SELF_OSC_KNEE: f64 = 0.82;

/// How much of its own oscillation a note starts the filter with, at the top
/// of the resonance travel. A filter with no state and no input stays silent
/// however negative its damping is, and the bank has programs whose only sound
/// source is the filter.
const LP_SELF_OSC_SEED: f64 = 0.04;

/// Where the voice's output stage runs out of headroom, and where it stops.
///
/// The forward path of this filter is linear — that is what makes it clean,
/// and it is the half of the SSM2040's character that a ladder's four
/// transistor pairs do not have. What linear all the way through does *not*
/// have is a bound, and a compensated resonance stage needs one: the peak
/// gain of a four-pole loop is unbounded as the feedback approaches marginal,
/// so the twenty-nine factory programs that sit at maximum resonance came out
/// 14 dB above the rest of the bank and one of them, 285 *Genesis 2*, reached
/// past the master limiter's ceiling on its own.
///
/// So the stage after the filter has rails, which is what the analog one has
/// and what the manual warns about in as many words: "High levels of
/// resonance can sometimes cause the Prophet-6 outputs to clip if its sound
/// generators are also set to high output in the Mixer." The knee is above
/// anything the mixer alone can produce — four oscillators at full is 2.2 —
/// so an ordinary program passes through it untouched, and only a resonant
/// bloom ever reaches it.
const VOICE_KNEE: f64 = 2.4;
const VOICE_RAIL: f64 = 4.5;

/// Soft, monotonic, bounded by [`VOICE_RAIL`], and the identity below
/// [`VOICE_KNEE`] — the same curve `level.rs` bounds the master output with,
/// at the scale a voice works in.
#[inline]
fn voice_limit(x: f64) -> f64 {
    let magnitude = x.abs();
    if magnitude <= VOICE_KNEE {
        return x;
    }
    let span = VOICE_RAIL - VOICE_KNEE;
    let u = (magnitude - VOICE_KNEE) / span;
    (VOICE_KNEE + span * u / (1.0 + u)).copysign(x)
}

/// How far the feedback path stays linear before it starts to compress.
///
/// The saturation in the loop is what bounds the self-oscillation, and it has
/// to be there. What it must *not* do is fold back into the passband: a
/// `tanh` applied straight to the loop state compresses the feedback while the
/// compensated input stage keeps its full gain, so a resonant bass patch comes
/// out 9 dB **louder** than the same patch with the resonance down. Scaling
/// into the curve and back out again keeps the loop linear over the levels an
/// oscillator hands it and leaves the compression for the self-oscillation,
/// which is the only thing that needs it.
const LP_FEEDBACK_HEADROOM: f64 = 2.5;

#[derive(Debug, Clone)]
struct LowPass {
    s: [f64; 4],
}

impl LowPass {
    fn new() -> Self {
        Self { s: [0.0; 4] }
    }

    #[inline]
    fn process(&mut self, input: f64, cutoff: f64, resonance: f64, sr: f64) -> f64 {
        let g = raw::tpt_g(cutoff, sr);
        let gg = g / (1.0 + g);
        let k = resonance.clamp(0.0, 1.0) * LP_RES_MAX;
        // The compensated summing stage. `tanh` sits on the feedback only:
        // it is what stops the self-oscillation running away, and keeping it
        // out of the forward path is what keeps the filter clean.
        let feedback = LP_FEEDBACK_HEADROOM * tanh_approx(self.s[3] / LP_FEEDBACK_HEADROOM);
        let mut x = input * (1.0 + LP_PASSBAND_COMP * k) - k * feedback;
        for s in &mut self.s {
            let v = (x - *s) * gg;
            let y = v + *s;
            *s = y + v;
            if s.abs() < 1.0e-18 {
                *s = 0.0;
            }
            x = y;
        }
        // The one number that closes the loop is worth bounding outright,
        // since a self-oscillating filter has no input to bound it. Well
        // clear of [`LP_FEEDBACK_HEADROOM`], so that the saturation in the
        // loop is what shapes the sound and this is only a numerical
        // backstop: a clamp that bites is a hard clipper inside a resonant
        // loop, which is broadband and sounds like it.
        self.s[3] = self.s[3].clamp(-16.0, 16.0);
        x
    }

    fn reset(&mut self) {
        self.s = [0.0; 4];
    }

    fn start(&mut self, resonance: f64) {
        let past = (resonance - LP_SELF_OSC_KNEE) / (1.0 - LP_SELF_OSC_KNEE);
        self.s = [past.clamp(0.0, 1.0) * LP_SELF_OSC_SEED; 4];
    }
}

// ── The high-pass filter ──
//
// "The High-Pass Filter is a 2-pole, 12 dB per octave, resonant filter." A
// resonant high-pass is the thing this instrument has that no other analog
// poly in the rack does — the Juno's is a passive shelf and the Jupiter's is
// non-resonant — and 305 of the 500 factory programs leave it at zero while
// the 195 that use it use it hard: 38 of them sit at maximum resonance.
//
// A topology-preserving state-variable filter, taken at its high-pass output.
// Two poles and a damping term, which is exactly what the hardware is.

/// The narrowest the high-pass resonance gets. A 2-pole resonant high-pass
/// lifts its corner by `Q`, so this is +18 dB at the corner and nothing
/// anywhere else — and unlike the low-pass it is not documented as
/// self-oscillating, so the travel stops short of that.
const HP_Q_MAX: f64 = 8.0;
const HP_Q_MIN: f64 = 0.5;

#[derive(Debug, Clone)]
struct HighPass {
    s1: f64,
    s2: f64,
}

impl HighPass {
    fn new() -> Self {
        Self { s1: 0.0, s2: 0.0 }
    }

    #[inline]
    fn process(&mut self, input: f64, cutoff: f64, resonance: f64, sr: f64) -> f64 {
        let g = raw::tpt_g(cutoff, sr);
        let q = HP_Q_MIN * (HP_Q_MAX / HP_Q_MIN).powf(resonance.clamp(0.0, 1.0));
        let damp = 1.0 / q;
        let hp = (input - (damp + g) * self.s1 - self.s2) / (1.0 + damp * g + g * g);
        let v1 = g * hp;
        let bp = v1 + self.s1;
        self.s1 = (bp + v1).clamp(-8.0, 8.0);
        let v2 = g * bp;
        let lp = v2 + self.s2;
        self.s2 = (lp + v2).clamp(-8.0, 8.0);
        hp
    }

    fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }
}

// ── Envelopes ──
//
// Two four-stage envelopes, one on the filters and one on the amplifier. On
// the hardware they are **digitally generated** rather than analog, which is
// why there is no slop on them anywhere in this file: two notes played with
// the same velocity have the same envelope to the sample.
//
// The segments are exponential, which is the shape the manual's own ADSR
// figure draws and what a player expects: the attack charges towards 1.58 and
// ends when it passes 1.0, which is the first time constant of an exponential,
// and the decay and release charge a little past their target and stop when
// they reach it, which is 3.5 time constants across the segment and makes the
// knob's number the time the segment actually takes.
//
// The manual describes a hidden feature that reshapes the filter envelope's
// response through the poly mod filter-env knob. It is not modelled: it
// changes a curve rather than a value, nothing in the program data records it,
// and using the poly mod knob for two things at once would make the panel lie.

/// 1/(1-e^-1): the attack aims here so that it arrives at 1.0 after exactly
/// one time constant.
const ATTACK_AIM: f64 = 1.581_976_706_869_326;
/// Time constants spanned by a decay or release segment.
const ENV_CONSTANTS: f64 = 3.5;
/// How far past its target a decay or release aims so that it arrives after
/// `ENV_CONSTANTS` of them: e^-3.5 / (1 - e^-3.5).
const ENV_UNDERSHOOT: f64 = 0.031_144_869_855_006_6;

fn env_rate(seconds: f64, constants: f64, sr: f64) -> f64 {
    if seconds <= 0.0 {
        return 1.0;
    }
    (1.0 - (-constants / (seconds * sr)).exp()).min(1.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct EnvTimes {
    attack: f64,
    decay: f64,
    sustain: f64,
    release: f64,
}

#[derive(Debug, Clone)]
struct Envelope {
    stage: EnvStage,
    level: f64,
    aim: f64,
    times: EnvTimes,
    /// Per-sample coefficients for the three timed segments, recomputed only
    /// when a knob moves.
    rates: [f64; 3],
    sample_rate: f64,
}

impl Envelope {
    fn new(sr: f64) -> Self {
        let mut env = Self {
            stage: EnvStage::Idle,
            level: 0.0,
            aim: 0.0,
            times: EnvTimes { attack: 0.01, decay: 0.3, sustain: 0.7, release: 0.2 },
            rates: [0.0; 3],
            sample_rate: sr,
        };
        env.retime();
        env
    }

    fn retime(&mut self) {
        let sr = self.sample_rate;
        self.rates = [
            env_rate(self.times.attack, 1.0, sr),
            env_rate(self.times.decay, ENV_CONSTANTS, sr),
            env_rate(self.times.release, ENV_CONSTANTS, sr),
        ];
    }

    fn set_times(&mut self, times: EnvTimes) {
        if times.attack != self.times.attack
            || times.decay != self.times.decay
            || times.release != self.times.release
        {
            self.times = times;
            self.retime();
        } else {
            self.times.sustain = times.sustain;
        }
    }

    /// A new attack from wherever the envelope already is.
    fn trigger(&mut self) {
        self.stage = EnvStage::Attack;
        self.aim = ATTACK_AIM;
    }

    /// A new attack from zero — what the three retrigger key modes do.
    fn trigger_from_zero(&mut self) {
        self.level = 0.0;
        self.trigger();
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
        self.aim = self.times.sustain - ENV_UNDERSHOOT * (self.level - self.times.sustain);
    }

    fn enter_sustain(&mut self) {
        self.level = self.times.sustain;
        self.stage = if self.times.sustain <= 0.0 { EnvStage::Idle } else { EnvStage::Sustain };
    }

    #[inline]
    fn tick(&mut self) -> f64 {
        match self.stage {
            EnvStage::Idle => 0.0,
            EnvStage::Attack => {
                self.level += self.rates[0] * (self.aim - self.level);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.enter_decay();
                    if self.level <= self.times.sustain {
                        self.enter_sustain();
                    }
                }
                self.level
            }
            EnvStage::Decay => {
                self.level += self.rates[1] * (self.aim - self.level);
                if self.level <= self.times.sustain {
                    self.enter_sustain();
                }
                self.level
            }
            EnvStage::Sustain => {
                self.level = self.times.sustain;
                self.level
            }
            EnvStage::Release => {
                self.level += self.rates[2] * (self.aim - self.level);
                if self.level <= 0.0 {
                    self.kill();
                }
                self.level
            }
        }
    }
}

// ── Noise, slop and the LFO ──

/// A 32-bit avalanche mix — the finaliser from Murmur3, in the variant with
/// better statistics that appears in `splitmix`.
///
/// Every random number in this file is a mix of a counter rather than the
/// output of a linear congruential generator, and the reason is *streams*: an
/// LCG has one cycle, so two of them with different seeds produce the same
/// sequence at different offsets, and "these two voices are independent" is
/// then a matter of how far apart the two offsets happen to be. Mixing a
/// counter with a per-stream stride gives streams that are independent by
/// construction.
#[inline]
fn mix32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// White noise. One per voice, with its own stream: the Jupiter shipped with
/// every voice sharing a noise seed and six coherent copies of the same
/// sequence summed to 18 dB rather than 8, which is audible as a hard bright
/// edge on every noisy program. Nothing in this file shares randomness.
#[derive(Debug, Clone)]
struct Noise {
    state: u32,
    stride: u32,
}

impl Noise {
    fn new(seed: u32) -> Self {
        Self { state: mix32(seed), stride: seed | 1 }
    }

    #[inline]
    fn tick(&mut self) -> f64 {
        self.state = self.state.wrapping_add(self.stride);
        f64::from(mix32(self.state) >> 8) / f64::from(1u32 << 23) - 1.0
    }
}

/// One oscillator's share of the slop: a slow random walk in cents.
///
/// "Slop adds randomized detuning to the oscillators to emulate the tuning
/// instability of vintage analog oscillators." Two properties matter and both
/// are in the word *drift*: it has to move slowly enough that it reads as a
/// tuning rather than as a vibrato, and no two oscillators anywhere in the
/// instrument may ever move together. A one-pole filter on white noise gives
/// the first; a per-oscillator, per-voice seed gives the second.
///
/// It is also the unison detune. The manual says so — "To detune the
/// oscillators, use the slop knob" — so there is no separate unison spread
/// control on this instrument, and a six-voice stack with slop at zero is
/// twelve oscillators at exactly the same frequency, which is what the
/// hardware does too. What keeps that from sounding like one oscillator 15 dB
/// louder is that the voices free-run from different phases; see
/// [`Voice::new`].
#[derive(Debug, Clone)]
struct Slop {
    noise: Noise,
    target: f64,
    value: f64,
    phase: f64,
}

/// How fast the drift moves, in hertz. Slow enough to read as tuning.
const SLOP_HZ: f64 = 0.7;

/// How often the walk takes a new random target, in hertz.
///
/// Fixed in *time* rather than in samples, so that the sequence of targets is
/// the same at 44.1 kHz and at 96 kHz and the drift is a property of the
/// instrument rather than of the audio device. That is what lets
/// `the_pitch_is_the_same_at_every_sample_rate` hold to a tenth of a percent
/// with slop running.
const SLOP_UPDATE_HZ: f64 = 40.0;

/// What the one-pole's output has to be multiplied by to reach ±1.
///
/// A 0.7 Hz one-pole fed from a 40 Hz sample-and-hold averages about nine
/// independent values, so its rms is a third of its input's and the walk would
/// otherwise wander at a third of the depth the knob asks for.
const SLOP_GAIN: f64 = 4.0;

impl Slop {
    fn new(seed: u32) -> Self {
        Self { noise: Noise::new(seed), target: 0.0, value: 0.0, phase: 0.0 }
    }

    /// The drift, −1…+1, to be multiplied by the depth in cents.
    #[inline]
    fn tick(&mut self, step: f64, coefficient: f64) -> f64 {
        self.phase += step;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            self.target = self.noise.tick();
        }
        self.value += coefficient * (self.target - self.value);
        (self.value * SLOP_GAIN).clamp(-1.0, 1.0)
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.target = 0.0;
        self.phase = 0.0;
    }
}

/// The low-frequency oscillator: one per instrument, as on the hardware.
///
/// Five shapes, and their polarity is the manual's rather than the obvious
/// one: "Triangle and Random waves are bipolar... The square wave, sawtooth,
/// and reverse sawtooth generate only positive values. In the case of the
/// square wave this makes it possible to generate natural-sounding trills."
/// A trill goes up from the note and comes back, which a bipolar square could
/// not do.
///
/// The sixth, hidden shape is the manual's too: "choose random then turn
/// frequency all the way clockwise. This generates a white noise waveform."
#[derive(Debug, Clone)]
struct Lfo {
    phase: f64,
    sample_hold: f64,
    noise: Noise,
}

impl Lfo {
    fn new() -> Self {
        Self { phase: 0.0, sample_hold: 0.0, noise: Noise::new(0x1F35_1F35) }
    }

    #[inline]
    fn tick(&mut self, hz: f64, sr: f64, shape: usize, noise_mode: bool) -> f64 {
        let dt = (hz / sr).clamp(0.0, 0.45);
        self.phase += dt;
        let wrapped = self.phase >= 1.0;
        if wrapped {
            self.phase -= self.phase.floor();
        }
        let n = self.noise.tick();
        if wrapped {
            self.sample_hold = n;
        }
        let t = self.phase;
        match shape {
            // Sawtooth and reverse sawtooth, unipolar, band-limited: the LFO
            // reaches audio rate on this instrument and a naive edge up there
            // folds a spectrum back that would then be heard through whatever
            // it modulates, differently at every sample rate.
            1 => (t + 0.5 * poly_blep(t, dt)).clamp(0.0, 1.0),
            2 => (1.0 - t - 0.5 * poly_blep(t, dt)).clamp(0.0, 1.0),
            3 => {
                let mut v = if t < 0.5 { 1.0 } else { 0.0 };
                v += 0.5 * poly_blep(t, dt);
                v -= 0.5 * poly_blep((t - 0.5).rem_euclid(1.0), dt);
                v.clamp(0.0, 1.0)
            }
            4 => {
                if noise_mode {
                    n
                } else {
                    self.sample_hold
                }
            }
            // Triangle, naive: its harmonics fall as 1/n², so what folds back
            // sits far enough under the fundamental to be irrelevant.
            _ => {
                if t < 0.5 {
                    4.0 * t - 1.0
                } else {
                    3.0 - 4.0 * t
                }
            }
        }
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.sample_hold = 0.0;
    }
}

/// The band-limited step correction, `(height/2) · r(s)` with `r` the
/// two-point residual.
#[inline]
fn poly_blep(t: f64, dt: f64) -> f64 {
    if dt <= 0.0 {
        return 0.0;
    }
    if t < dt {
        let s = t / dt;
        2.0 * s - s * s - 1.0
    } else if t > 1.0 - dt {
        let s = (t - 1.0) / dt;
        s * s + 2.0 * s + 1.0
    } else {
        0.0
    }
}

// ── Distortion ──
//
// "The Prophet-6 provides stereo analog distortion. This can be used to add
// warmth, harmonic complexity, and an aggressive edge to sounds. The character
// of the distortion is affected by the harmonic content of a program." It is
// analog and it is after the voices, so it is on the summed stereo signal
// rather than inside a voice — 96 of the 500 programs use it and *ScumSlinger*
// and *Irradiated* sit at the top of the knob.
//
// The curve is the soft clipper `x / (1 + k|x|)`, which is the identity in f64
// at `k = 0` — so the bottom of the knob is the program as voiced, exactly —
// and is monotonic and bounded by `1/k`, so it folds nothing back. The makeup
// gain is `1 + k`, which makes the curve pass a full-scale input through
// unchanged at every setting: quiet material gets up to 26 dB louder, loud
// material gets squashed, and the output is bounded near 1 whatever the voices
// hand it. That bound is why a six-voice unison stack at maximum distortion is
// safe rather than merely trimmed.
//
// The asymmetry is in the denominator only, so both halves keep the same slope
// through zero. A kink at the zero crossing is crossover distortion, which is
// a much nastier sound than a clipper whose bias point is off centre; what
// differs here is where each half runs out of headroom, which is what leaves
// even harmonics and a DC offset behind. [`DcBlock`] takes the offset out.

/// How hard the knob's top drives the clipper.
const DIST_KNEE: f64 = 20.0;

/// The signal level the clipper's rails sit at, and therefore the level whose
/// loudness the knob leaves alone.
///
/// This constant is what stops the distortion being a limiter. With the rails
/// at 1.0 — a single voice's full-scale — the knob is *level preserving for a
/// single voice* and a compressor for anything else, and *ScumSlinger*, a
/// six-voice unison program with the distortion at the top of its travel,
/// measured 7 dB **quieter** with its own distortion than without it. Two is
/// where six voices sum: `√6` incoherent voices at 0.707 after the pan law.
/// At that level the curve passes the signal through at unity, quieter
/// material gets the drive as gain, and louder material clips against the
/// rails, which is what the circuit does.
const DIST_RAIL: f64 = 2.0;
/// How much shallower the negative half of the curve is than the positive.
const DIST_ASYMMETRY: f64 = 0.65;
/// Corner of the DC blocker that follows the distortion.
const DC_BLOCK_HZ: f64 = 12.0;

#[inline]
fn distort(x: f64, amount: f64) -> f64 {
    if amount <= 0.0 {
        return x;
    }
    let k = amount * DIST_KNEE;
    let bias = if x < 0.0 { DIST_ASYMMETRY } else { 1.0 };
    x * (1.0 + k) / (1.0 + k * bias * x.abs() / DIST_RAIL)
}

/// One-pole DC blocker. The coefficient comes from the sample rate, so the
/// corner is the same frequency at every rate rather than at 44100 only.
#[derive(Debug, Clone, Default)]
struct DcBlock {
    x1: f64,
    y1: f64,
}

impl DcBlock {
    #[inline]
    fn tick(&mut self, x: f64, a: f64) -> f64 {
        let y = x - self.x1 + a * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

// ── Effects ──
//
// Two slots in series, A then B, with a true-bypass switch over both: "The
// on/off switch enables and disables both Effect A and Effect B, using a true
// bypass, ensuring a pure analog signal path." Effect A carries no reverbs
// because it is not the last stage in the chain, which is why its type list is
// six long and Effect B's is ten.
//
// **Three of the ten are rendered: `bbd`, `ddl` and `CHO`.** They are the ones
// that are cheap, the ones that carry most of the factory bank's character,
// and — for the two delays — the ones the clock-sync machinery exists for. The
// two phasers and the four reverbs are stored, selectable, and pass their
// input through unchanged; a program that asks for a hall reverb renders dry
// and keeps every one of its settings, so connecting a reverb later is a
// change to this module and to nothing else. The alternative, dropping the
// unrendered types from the selector, would silently renumber 2015 program
// data and is not on the table.
//
// The effect indices are [`FX_A_TYPES`] / [`FX_B_TYPES`], which are the OS 1.0
// lists; see the note there for why that matters.
mod fx {
    /// Effect type indices, shared by both slots for the first six.
    pub const OFF: usize = 0;
    pub const BBD: usize = 1;
    pub const DDL: usize = 2;
    pub const CHORUS: usize = 3;
}

/// The eleven clock-synced delay divisions, in beats.
///
/// The manual's own table gives a *value* column and a *delay time* column,
/// and two of its eleven rows are wrong: it prints "4t → 1 beat", the same as
/// the plain quarter on the row below, and "8t → 1/2 of 1 beat", the same as
/// the plain eighth above it. A quarter triplet is two thirds of a beat and an
/// eighth triplet is one third, which is what is used here; the other nine
/// rows are the manual's as printed.
const SYNC_BEATS: [f64; 11] = [
    4.0,
    3.0,
    2.0,
    2.0 / 3.0,
    1.5,
    1.0,
    0.75,
    0.5,
    1.0 / 3.0,
    0.375,
    0.25,
];

/// "Maximum delay time is 1 second."
const DELAY_MAX_S: f64 = 1.0;
/// The shortest the delay-time knob reaches.
const DELAY_MIN_S: f64 = 0.002;
/// The most feedback the knob allows. Short of 1 so that a delay cannot run
/// away, which on an instrument whose output must stay under the limiter on
/// its own is a requirement rather than a taste.
const DELAY_FEEDBACK_MAX: f64 = 0.92;
/// Where a bucket-brigade line loses its treble, per repeat. The BBD's whole
/// character: "characterized by relatively short delay times and a warmer
/// character than digital delays due to their loss of treble and clarity".
const BBD_LOSS_HZ: f64 = 2_600.0;
/// How far a bucket-brigade clock wanders, as a fraction of the delay time.
const BBD_WOW: f64 = 0.0025;

/// Chorus sweep centre and the widest it moves, in seconds.
const CHORUS_CENTRE_S: f64 = 0.0072;
const CHORUS_SWEEP_S: f64 = 0.0048;
const CHORUS_MIN_HZ: f64 = 0.05;
const CHORUS_MAX_HZ: f64 = 8.0;
/// How long a chorus line has to be. Centre plus sweep plus a sample of slack.
const CHORUS_MAX_S: f64 = 0.02;

/// What one effect slot is set to, read once a block.
#[derive(Debug, Clone, Copy)]
struct FxSetting {
    kind: usize,
    mix: f64,
    /// Delay time in seconds, or chorus rate in hertz — whichever the type
    /// wants from parameter 1.
    p1: f64,
    /// Feedback, or chorus depth.
    p2: f64,
}

/// One effect slot: a delay line long enough for the maximum delay time, and a
/// stereo chorus line. Both are allocated in `init` and neither is ever
/// resized, so nothing here allocates on the audio thread.
#[derive(Debug, Clone)]
struct FxSlot {
    delay: Vec<f32>,
    write: usize,
    loop_lp: f64,
    wow: f64,
    wow_noise: Noise,
    chorus: Vec<f32>,
    chorus_write: usize,
    chorus_phase: f64,
}

impl FxSlot {
    fn new(seed: u32) -> Self {
        Self {
            delay: Vec::new(),
            write: 0,
            loop_lp: 0.0,
            wow: 0.0,
            wow_noise: Noise::new(seed),
            chorus: Vec::new(),
            chorus_write: 0,
            chorus_phase: 0.0,
        }
    }

    fn init(&mut self, sr: f64) {
        let delay_len = (sr * DELAY_MAX_S) as usize + 2;
        self.delay.clear();
        self.delay.resize(delay_len, 0.0);
        let chorus_len = (sr * CHORUS_MAX_S) as usize + 2;
        self.chorus.clear();
        self.chorus.resize(chorus_len * 2, 0.0);
        self.reset();
    }

    fn reset(&mut self) {
        self.delay.fill(0.0);
        self.chorus.fill(0.0);
        self.write = 0;
        self.chorus_write = 0;
        self.chorus_phase = 0.0;
        self.loop_lp = 0.0;
        self.wow = 0.0;
    }

    /// Reads `back` samples behind the write head, linearly interpolated.
    #[inline]
    fn tap(buffer: &[f32], write: usize, back: f64, stride: usize, offset: usize) -> f64 {
        let frames = buffer.len() / stride;
        if frames == 0 {
            return 0.0;
        }
        let back = back.clamp(1.0, frames as f64 - 2.0);
        let whole = back as usize;
        let frac = back - whole as f64;
        let i0 = (write + frames - whole) % frames;
        let i1 = (i0 + frames - 1) % frames;
        let a = f64::from(buffer[i0 * stride + offset]);
        let b = f64::from(buffer[i1 * stride + offset]);
        a + (b - a) * frac
    }

    #[inline]
    fn process(&mut self, left: f64, right: f64, set: &FxSetting, sr: f64) -> (f64, f64) {
        if set.kind == fx::OFF {
            return (left, right);
        }
        match set.kind {
            fx::BBD | fx::DDL => {
                if self.delay.is_empty() {
                    return (left, right);
                }
                let frames = self.delay.len();
                let mono = (left + right) * 0.5;
                let mut back = set.p1 * sr;
                if set.kind == fx::BBD {
                    // A bucket-brigade line is clocked by an analog oscillator
                    // and the clock wanders, which is most of why a BBD does
                    // not sound like a digital delay set to the same time.
                    self.wow += 0.0004 * (self.wow_noise.tick() - self.wow);
                    back *= 1.0 + self.wow * BBD_WOW * 400.0;
                }
                let delayed = Self::tap(&self.delay, self.write, back, 1, 0);
                let fed = if set.kind == fx::BBD {
                    let a = raw::one_pole(BBD_LOSS_HZ, sr);
                    self.loop_lp += a * (delayed - self.loop_lp);
                    tanh_approx(self.loop_lp)
                } else {
                    delayed
                };
                self.delay[self.write] = (mono + fed * set.p2) as f32;
                self.write = (self.write + 1) % frames;
                // The wet signal is mono, as it is on the instrument: one
                // delay line, added to both sides. The dry stays stereo.
                let wet = delayed;
                (
                    left * (1.0 - set.mix) + wet * set.mix,
                    right * (1.0 - set.mix) + wet * set.mix,
                )
            }
            fx::CHORUS => {
                if self.chorus.is_empty() {
                    return (left, right);
                }
                let frames = self.chorus.len() / 2;
                self.chorus_phase += set.p1 / sr;
                if self.chorus_phase >= 1.0 {
                    self.chorus_phase -= self.chorus_phase.floor();
                }
                // Two sweeps a quarter cycle apart, which is what makes a
                // chorus wide rather than merely detuned.
                let sweep_l = (TAU * self.chorus_phase).sin();
                let sweep_r = (TAU * self.chorus_phase + PI * 0.5).sin();
                let depth = set.p2 * CHORUS_SWEEP_S;
                let back_l = (CHORUS_CENTRE_S + depth * sweep_l) * sr;
                let back_r = (CHORUS_CENTRE_S + depth * sweep_r) * sr;
                let wet_l = Self::tap(&self.chorus, self.chorus_write, back_l, 2, 0);
                let wet_r = Self::tap(&self.chorus, self.chorus_write, back_r, 2, 1);
                self.chorus[self.chorus_write * 2] = left as f32;
                self.chorus[self.chorus_write * 2 + 1] = right as f32;
                self.chorus_write = (self.chorus_write + 1) % frames;
                (
                    left * (1.0 - set.mix) + wet_l * set.mix,
                    right * (1.0 - set.mix) + wet_r * set.mix,
                )
            }
            // The two phasers and the four reverbs. Stored, selectable, and
            // not rendered — see the module comment.
            _ => (left, right),
        }
    }
}

// ── Modulation depths ──
//
// The instrument publishes none of these: the LFO's amount knob, poly mod's
// two amount knobs and the aftertouch amount knob are all "0 to full" with no
// unit anywhere in either manual. What follows is judgment, and the reasoning
// is the same in each case — the depth has to be enough for the sound the
// manual names the destination for, and no more.

/// LFO to either oscillator's frequency, at full. "Use a triangle wave as a
/// source to create vibrato. Use a square wave to create trills." A trill is
/// an interval, so the top of the knob has to reach one; an octave leaves room
/// for the whole of one.
const LFO_PITCH_SEMITONES: f64 = 12.0;
/// LFO to either filter, at full. "Use a triangle wave LFO to create an
/// auto-wah effect", which wants several octaves.
const LFO_FILTER_SEMITONES: f64 = 60.0;
/// LFO to pulse width, at full: the whole of one side of the travel, so that
/// a bipolar triangle sweeps the width from one end to the other.
const LFO_PW: f64 = 0.5;

/// Poly mod to oscillator 1's frequency, at full.
///
/// "Choose osc 2 as a modulation source to produce FM effects with their
/// characteristic complex harmonics and metallic timbre." FM at a modulation
/// index worth having needs the deviation to be several times the carrier, so
/// this is the widest depth in the instrument — four octaves each way.
const PM_PITCH_SEMITONES: f64 = 48.0;
/// Poly mod to either filter cutoff, at full. Seven octaves, because the
/// destination this exists for is audio-rate filter modulation and a filter
/// FM that only moves the cutoff by a fifth is inaudible as FM.
const PM_FILTER_SEMITONES: f64 = 84.0;

/// Aftertouch to either oscillator's frequency, at full.
const AT_PITCH_SEMITONES: f64 = 12.0;
/// Aftertouch to either filter cutoff, at full.
const AT_FILTER_SEMITONES: f64 = 48.0;

/// How far oscillator 2 drops when its low-frequency switch is on.
///
/// **Judgment**: the manual says only that the switch "turns Oscillator 2 into
/// a low-frequency oscillator... The frequency, fine, shape, and pulse width
/// controls still apply". Seven octaves puts the knob's unity position at
/// 2 Hz with the keyboard at middle C and the whole of its five-octave travel
/// between 0.5 and 16 Hz, which is the band an LFO wants.
const OSC2_LOW_DROP: f64 = 7.0;

/// The note oscillator 2 plays from when its keyboard switch is off: "the
/// Oscillator 2 ignores the keyboard and note data received via MIDI and plays
/// at its base frequency setting."
const OSC2_NO_KEY_NOTE: f64 = 60.0;

/// Where the filters' cutoff stops, whatever the knob and the modulation ask
/// for.
///
/// A VCF core has a rail, and the top of the cutoff parameter is deliberately
/// past the audible band — [`raw::cutoff_note`] reads 164 as 106 kHz — so
/// something has to stop it. Stopping it at the *sample rate* is not that
/// something: a resonant peak parked at 0.45 of the sample rate is at 19.8 kHz
/// at 44.1 kHz and at 43 kHz at 96 kHz, so a program with the filter wide open
/// and the resonance up would be a different sound on a different audio
/// device. 18 kHz is the rail, and Nyquist is only the second clamp.
const CUTOFF_MAX_HZ: f64 = 18_000.0;

/// The drift every oscillator has even with slop at zero, in cents.
///
/// "The oscillators on the Prophet-6 are extremely stable" — but they are
/// still analog, and two of them are never at exactly one frequency. Small
/// enough to be inaudible as detuning (a beat every twenty seconds at A3) and
/// large enough that a six-voice unison stack with slop at zero is six voices
/// rather than one voice 15 dB louder.
const SLOP_FLOOR_CENTS: f64 = 1.5;

// ── The panel, read once a block ──

/// Every control, converted out of knob positions into the units the engine
/// works in. Built once per `process` call rather than per sample: none of it
/// can change inside a block, and a tangent and a dozen exponentials per
/// sample for numbers that move when a finger does is not a trade worth
/// making.
#[derive(Debug, Clone, Copy)]
struct Panel {
    osc1_semitones: f64,
    osc1_shape: f64,
    osc1_duty: f64,
    sync: bool,
    osc2_semitones: f64,
    osc2_shape: f64,
    osc2_duty: f64,
    osc2_low: bool,
    osc2_key: bool,
    slop_cents: f64,

    osc1_level: f64,
    osc2_level: f64,
    sub_level: f64,
    noise_level: f64,

    hp_note: f64,
    hp_res: f64,
    hp_env: f64,
    hp_vel: bool,
    hp_key: f64,
    lp_note: f64,
    lp_res: f64,
    lp_env: f64,
    lp_vel: bool,
    lp_key: f64,

    f_times: EnvTimes,
    a_times: EnvTimes,
    vca_amount: f64,
    vca_vel: bool,

    lfo_hz: f64,
    lfo_shape: usize,
    lfo_noise: bool,
    lfo_initial: f64,
    lfo_dest: [bool; 6],

    pm_filter_env: f64,
    pm_osc2: f64,
    pm_dest: [bool; 5],

    at_amount: f64,
    at_dest: [bool; 6],

    distortion: f64,
    volume: f64,
    pan_spread: f64,

    unison: bool,
    unison_voices: usize,
    chord_mode: bool,
    key_mode: usize,
    glide_on: bool,
    glide_mode: usize,
    glide_seconds: f64,
    bend_range: f64,

    fx_on: bool,
    fx_a: FxSetting,
    fx_b: FxSetting,
}

/// A continuous control's raw instrument value.
fn knob(params: &[f32; PARAM_COUNT], index: usize) -> f64 {
    let max = raw_offset(index).map_or(1.0, |(_, max)| max);
    f64::from(params[index].clamp(0.0, 1.0)) * max
}

/// A selector's position.
fn step_of(params: &[f32; PARAM_COUNT], index: usize) -> usize {
    discrete_steps(index).map_or(0, |count| selector(params[index], count))
}

/// A two-position switch.
fn flag(params: &[f32; PARAM_COUNT], index: usize) -> bool {
    step_of(params, index) == 1
}

impl Panel {
    fn read(params: &[f32; PARAM_COUNT], sr: f64) -> Self {
        let bpm = raw::bpm(knob(params, P_BPM));
        let slot = |kind_index: usize, mix_index: usize, p1: usize, p2: usize,
                    sync_index: usize, div_index: usize| {
            let kind = step_of(params, kind_index);
            let p1_raw = knob(params, p1);
            let p2_raw = knob(params, p2);
            let (p1, p2) = if kind == fx::CHORUS {
                (
                    CHORUS_MIN_HZ
                        * (CHORUS_MAX_HZ / CHORUS_MIN_HZ).powf((p1_raw / 255.0).clamp(0.0, 1.0)),
                    raw::level(p2_raw),
                )
            } else {
                let seconds = if flag(params, sync_index) {
                    // "The combination of longer synced delay times with slower
                    // tempos can result in delay times that would be greater
                    // than 1 second. When that happens, the delay time is
                    // divided by 2 until it no longer exceeds the 1 second
                    // limit."
                    let mut s = SYNC_BEATS[step_of(params, div_index)] * 60.0 / bpm;
                    while s > DELAY_MAX_S {
                        s *= 0.5;
                    }
                    s
                } else {
                    DELAY_MIN_S
                        * (DELAY_MAX_S / DELAY_MIN_S).powf((p1_raw / 255.0).clamp(0.0, 1.0))
                };
                (seconds, raw::level(p2_raw) * DELAY_FEEDBACK_MAX)
            };
            FxSetting { kind, mix: raw::level(knob(params, mix_index)), p1, p2 }
        };

        let f_times = EnvTimes {
            attack: raw::env_seconds(knob(params, P_F_ATTACK)),
            decay: raw::env_seconds(knob(params, P_F_DECAY)),
            sustain: raw::level(knob(params, P_F_SUSTAIN)),
            release: raw::env_seconds(knob(params, P_F_RELEASE)),
        };
        let a_times = EnvTimes {
            attack: raw::env_seconds(knob(params, P_A_ATTACK)),
            decay: raw::env_seconds(knob(params, P_A_DECAY)),
            sustain: raw::level(knob(params, P_A_SUSTAIN)),
            release: raw::env_seconds(knob(params, P_A_RELEASE)),
        };

        let lfo_shape = step_of(params, P_LFO_SHAPE);
        let lfo_raw = knob(params, P_LFO_FREQ);
        let unison_mode = step_of(params, P_UNISON_MODE);

        Self {
            osc1_semitones: raw::osc_semitones(knob(params, P_OSC1_FREQ)),
            osc1_shape: raw::shape(knob(params, P_OSC1_SHAPE)),
            osc1_duty: raw::duty(knob(params, P_OSC1_PW)),
            sync: flag(params, P_SYNC),
            osc2_semitones: raw::osc_semitones(knob(params, P_OSC2_FREQ))
                + raw::fine_cents(knob(params, P_OSC2_FINE)) / 100.0,
            osc2_shape: raw::shape(knob(params, P_OSC2_SHAPE)),
            osc2_duty: raw::duty(knob(params, P_OSC2_PW)),
            osc2_low: flag(params, P_OSC2_LOW),
            osc2_key: flag(params, P_OSC2_KEY),
            slop_cents: raw::slop_cents(knob(params, P_SLOP)) + SLOP_FLOOR_CENTS,

            osc1_level: raw::level(knob(params, P_OSC1_LEVEL)),
            osc2_level: raw::level(knob(params, P_OSC2_LEVEL)),
            sub_level: raw::level(knob(params, P_SUB_LEVEL)),
            noise_level: raw::level(knob(params, P_NOISE_LEVEL)),

            hp_note: raw::cutoff_note(knob(params, P_HP_CUTOFF)),
            hp_res: raw::resonance(knob(params, P_HP_RESO)),
            hp_env: raw::bipolar(knob(params, P_HP_ENV), 254.0) * raw::ENV_SEMITONES,
            hp_vel: flag(params, P_HP_VEL),
            hp_key: step_of(params, P_HP_KEY) as f64 * 0.5,
            lp_note: raw::cutoff_note(knob(params, P_LP_CUTOFF)),
            lp_res: raw::resonance(knob(params, P_LP_RESO)),
            lp_env: raw::bipolar(knob(params, P_LP_ENV), 254.0) * raw::ENV_SEMITONES,
            lp_vel: flag(params, P_LP_VEL),
            lp_key: step_of(params, P_LP_KEY) as f64 * 0.5,

            f_times,
            a_times,
            vca_amount: raw::level(knob(params, P_VCA_ENV)),
            vca_vel: flag(params, P_VCA_VEL),

            lfo_hz: raw::lfo_hz(lfo_raw),
            lfo_shape,
            // "choose random then turn frequency all the way clockwise. This
            // generates a white noise waveform."
            lfo_noise: lfo_shape == 4 && lfo_raw >= 254.0,
            lfo_initial: (knob(params, P_LFO_AMOUNT) / 255.0).clamp(0.0, 1.0),
            lfo_dest: [
                flag(params, P_LFO_FREQ1),
                flag(params, P_LFO_FREQ2),
                flag(params, P_LFO_PW),
                flag(params, P_LFO_AMP),
                flag(params, P_LFO_LP),
                flag(params, P_LFO_HP),
            ],

            pm_filter_env: raw::bipolar(knob(params, P_PM_FILTER_ENV), 254.0),
            pm_osc2: raw::bipolar(knob(params, P_PM_OSC2), 254.0),
            pm_dest: [
                flag(params, P_PM_FREQ1),
                flag(params, P_PM_SHAPE1),
                flag(params, P_PM_PW1),
                flag(params, P_PM_LP),
                flag(params, P_PM_HP),
            ],

            at_amount: raw::bipolar(knob(params, P_AT_AMOUNT), 254.0),
            at_dest: [
                flag(params, P_AT_FREQ1),
                flag(params, P_AT_FREQ2),
                flag(params, P_AT_LFO),
                flag(params, P_AT_AMP),
                flag(params, P_AT_LP),
                flag(params, P_AT_HP),
            ],

            distortion: raw::level(knob(params, P_DISTORTION)),
            volume: raw::level(knob(params, P_VOLUME)),
            pan_spread: raw::level(knob(params, P_PAN_SPREAD)),

            unison: flag(params, P_UNISON),
            unison_voices: if unison_mode >= 6 { VOICES } else { unison_mode + 1 },
            chord_mode: unison_mode >= 6,
            key_mode: step_of(params, P_KEY_MODE),
            glide_on: flag(params, P_GLIDE),
            glide_mode: step_of(params, P_GLIDE_MODE),
            glide_seconds: raw::glide_seconds(knob(params, P_GLIDE_RATE)),
            bend_range: step_of(params, P_BEND_RANGE) as f64,

            fx_on: flag(params, P_FX_ON),
            fx_a: slot(P_FXA_TYPE, P_FXA_MIX, P_FXA_P1, P_FXA_P2, P_FXA_SYNC, P_FXA_DIV),
            fx_b: slot(P_FXB_TYPE, P_FXB_MIX, P_FXB_P1, P_FXB_P2, P_FXB_SYNC, P_FXB_DIV),
        }
        .with_rate(sr)
    }

    /// The one thing that depends on the sample rate rather than on a knob.
    fn with_rate(mut self, sr: f64) -> Self {
        let ceiling = sr * 0.45;
        self.lfo_hz = self.lfo_hz.min(ceiling);
        self
    }
}

/// What every voice shares within one sample: the instrument's single LFO,
/// the wheels, and the coefficients that come from the sample rate.
#[derive(Debug, Clone, Copy)]
struct Shared {
    lfo: f64,
    lfo_depth: f64,
    pressure: f64,
    bend: f64,
    sr: f64,
    slop_step: f64,
    slop_coefficient: f64,
    cutoff_ceiling_hz: f64,
}

// ── The voice ──
//
// Six of them, which is the instrument. Each carries two oscillators, the sub,
// its own noise generator, its own slop walks, both filters and both
// envelopes — everything except the LFO, which is one per instrument on the
// hardware and one per instrument here.
//
// The oscillators **free-run**: nothing resets a phase on note-on, and each
// voice starts life at its own offset. That is what an analog polysynth does,
// and it is what makes a six-voice unison stack six voices rather than one
// voice 15 dB louder. It is also why every random seed in this file is
// per-voice and per-oscillator — the Jupiter shipped once with a shared noise
// seed and its six voices summed coherently to 18 dB instead of 8.

pub const VOICES: usize = 6;

/// Where each voice's oscillators start, in cycles.
///
/// **Not** a low-discrepancy sequence, and that is the whole point. Six phases
/// spread evenly round the circle — golden ratio, van der Corput, anything
/// with good equidistribution — is exactly the configuration in which six
/// oscillators at the same frequency *cancel*: the sum of six unit vectors at
/// evenly spaced angles is zero, and it stays near zero for every low harmonic
/// too. Measured, a six-voice unison stack on those phases is 1.2 times one
/// voice instead of the 2.4 that six uncorrelated sources give, so the
/// instrument's headline sound came out thinner than a single note.
///
/// So the phases are a hash of the oscillator's index, and [`PHASE_SEED`] was
/// chosen by measuring the resulting stack: all three oscillators land within
/// 3% of √6, which is what six genuinely uncorrelated sources sum to.
/// `a_unison_stack_is_louder_than_one_voice` holds it there.
fn start_phase(voice: usize, osc: usize) -> f64 {
    let n = ((voice * 3 + osc + 1) as u32).wrapping_mul(0x9E37_79B9).wrapping_add(PHASE_SEED);
    f64::from(mix32(n)) / f64::from(u32::MAX)
}

/// See [`start_phase`]. Chosen by measurement, not by taste.
const PHASE_SEED: u32 = 0x03F2_EC8A;

/// Each voice's place in the stereo field, before the pan spread knob scales
/// it. Alternating rather than left-to-right, so that three sounding voices
/// out of six are still spread rather than all on one side.
const VOICE_PAN: [f64; VOICES] = [-1.0, 0.6, -0.2, 1.0, -0.6, 0.2];

struct Voice {
    index: usize,
    osc1: Osc,
    osc2: Osc,
    sub: Osc,
    slop1: Slop,
    slop2: Slop,
    noise: Noise,
    lpf: LowPass,
    hpf: HighPass,
    filter_env: Envelope,
    amp_env: Envelope,
    note: u8,
    velocity: u8,
    glide_note: f64,
    target_note: f64,
    /// Semitones per sample the glide moves at, which the fixed-time modes
    /// recompute at every new note and the fixed-rate modes do not.
    glide_rate: f64,
    /// False until this voice has ever been given a note, so that its first
    /// note does not glide up from note zero.
    pitched: bool,
    gate: bool,
    /// When this voice was last started, for stealing.
    age: u64,
}

impl Voice {
    fn new(index: usize, sr: f64) -> Self {
        // Every seed distinct, and distinct from every other voice's. The
        // multiplier is the 64-bit golden-ratio constant truncated to 32 bits,
        // which is what SplitMix uses to decorrelate consecutive seeds.
        let seed = |slot: u32| (index as u32 * 4 + slot + 1).wrapping_mul(0x9E37_79B9) | 1;
        Self {
            index,
            osc1: Osc::new(start_phase(index, 0)),
            osc2: Osc::new(start_phase(index, 1)),
            sub: Osc::new(start_phase(index, 2)),
            slop1: Slop::new(seed(0)),
            slop2: Slop::new(seed(1)),
            noise: Noise::new(seed(2)),
            lpf: LowPass::new(),
            hpf: HighPass::new(),
            filter_env: Envelope::new(sr),
            amp_env: Envelope::new(sr),
            note: 60,
            velocity: 100,
            glide_note: 60.0,
            target_note: 60.0,
            glide_rate: 0.0,
            pitched: false,
            gate: false,
            age: 0,
        }
    }

    fn reset(&mut self) {
        self.osc1.reset(start_phase(self.index, 0));
        self.osc2.reset(start_phase(self.index, 1));
        self.sub.reset(start_phase(self.index, 2));
        self.slop1.reset();
        self.slop2.reset();
        self.lpf.reset();
        self.hpf.reset();
        self.filter_env.kill();
        self.amp_env.kill();
        self.gate = false;
        self.pitched = false;
    }

    fn is_free(&self) -> bool {
        !self.gate && !self.amp_env.is_active()
    }

    /// Point the voice at a note. `glide` is whether it should slide there
    /// from where it is rather than jump.
    fn retune(&mut self, note: u8, panel: &Panel, sr: f64, glide: bool) {
        self.note = note;
        self.target_note = f64::from(note);
        if !self.pitched || !glide {
            self.glide_note = self.target_note;
            self.pitched = true;
        }
        // Fixed rate: an octave takes the knob's time whatever the interval.
        // Fixed time: the whole interval takes it.
        let seconds = panel.glide_seconds.max(1.0e-6);
        self.glide_rate = if panel.glide_mode >= 2 {
            (self.target_note - self.glide_note).abs() / seconds / sr
        } else {
            12.0 / seconds / sr
        };
    }

    fn start(&mut self, velocity: u8, panel: &Panel, age: u64, from_zero: bool) {
        self.velocity = velocity;
        self.gate = true;
        self.age = age;
        if from_zero {
            self.filter_env.trigger_from_zero();
            self.amp_env.trigger_from_zero();
        } else {
            self.filter_env.trigger();
            self.amp_env.trigger();
        }
        self.lpf.start(panel.lp_res);
    }

    fn release(&mut self) {
        self.gate = false;
        self.filter_env.release_env();
        self.amp_env.release_env();
    }

    #[inline]
    fn tick(&mut self, p: &Panel, s: &Shared) -> (f64, f64) {
        if !self.gate && !self.amp_env.is_active() {
            return (0.0, 0.0);
        }
        self.filter_env.set_times(p.f_times);
        self.amp_env.set_times(p.a_times);

        let filter_env = self.filter_env.tick();
        let amp_env = self.amp_env.tick();

        // Glide, at whatever rate the mode chose.
        if p.glide_on {
            let remaining = self.target_note - self.glide_note;
            if remaining.abs() <= self.glide_rate {
                self.glide_note = self.target_note;
            } else {
                self.glide_note += self.glide_rate.copysign(remaining);
            }
        } else {
            self.glide_note = self.target_note;
        }

        let velocity = f64::from(self.velocity) / 127.0;
        let lfo = s.lfo * s.lfo_depth;
        let pressure = s.pressure * p.at_amount;

        // ── Oscillator 2, which is computed first because it is a modulation
        // source for oscillator 1 and, when sync is on, its master ──

        let slop2 = self.slop2.tick(s.slop_step, s.slop_coefficient) * p.slop_cents / 100.0;
        let key2 = if p.osc2_key { self.glide_note } else { OSC2_NO_KEY_NOTE };
        let mut note2 = key2 + p.osc2_semitones + slop2 + s.bend;
        if p.lfo_dest[1] {
            note2 += lfo * LFO_PITCH_SEMITONES;
        }
        if p.at_dest[1] {
            note2 += pressure * AT_PITCH_SEMITONES;
        }
        let hz2 = raw::note_hz(note2)
            * if p.osc2_low { (-OSC2_LOW_DROP).exp2() } else { 1.0 };
        let dt2 = (hz2 / s.sr).clamp(0.0, 0.45);

        let mut duty2 = p.osc2_duty;
        if p.lfo_dest[2] {
            duty2 += lfo * LFO_PW;
        }
        let shape2 = Shape::at(p.osc2_shape, duty2);
        let sync_at = if p.sync { self.osc2.wraps_at(dt2) } else { None };
        let out2 = self.osc2.tick(dt2, &shape2, None);

        // ── Poly mod ──
        //
        // "Poly Mod modulation sources: filter envelope, oscillator 2
        // frequency." Both bipolar, both able to reach five destinations, and
        // both read here at the sample rate so that oscillator 2 at audio rate
        // into the low-pass really is filter FM.

        let poly = filter_env * p.pm_filter_env + out2 * p.pm_osc2;

        // ── Oscillator 1 ──

        let slop1 = self.slop1.tick(s.slop_step, s.slop_coefficient) * p.slop_cents / 100.0;
        let mut note1 = self.glide_note + p.osc1_semitones + slop1 + s.bend;
        if p.lfo_dest[0] {
            note1 += lfo * LFO_PITCH_SEMITONES;
        }
        if p.at_dest[0] {
            note1 += pressure * AT_PITCH_SEMITONES;
        }
        if p.pm_dest[0] {
            note1 += poly * PM_PITCH_SEMITONES;
        }
        let hz1 = raw::note_hz(note1);
        let dt1 = (hz1 / s.sr).clamp(0.0, 0.45);

        let mut shape1_at = p.osc1_shape;
        if p.pm_dest[1] {
            shape1_at += poly;
        }
        let mut duty1 = p.osc1_duty;
        if p.lfo_dest[2] {
            duty1 += lfo * LFO_PW;
        }
        if p.pm_dest[2] {
            duty1 += poly * LFO_PW;
        }
        let shape1 = Shape::at(shape1_at, duty1);
        let out1 = self.osc1.tick(dt1, &shape1, sync_at);

        // The sub is a divide-by-two off oscillator 1: "a triangle wave
        // oscillator pitched one octave below Oscillator 1", so it inherits
        // oscillator 1's tuning, its slop and its modulation and halves it.
        let sub = self.sub.tick(dt1 * 0.5, &SUB_SHAPE, None);
        let noise = self.noise.tick();

        let mix = out1 * p.osc1_level
            + out2 * p.osc2_level
            + sub * p.sub_level
            + noise * p.noise_level;

        // ── Filters, high-pass first and then low-pass ──
        //
        // "If used at the same time, the two filters act as a band-pass
        // filter." One envelope drives both, with its own bipolar amount and
        // its own velocity switch at each: "Velocity: on, off — When enabled,
        // allows key velocity to influence filter frequency."

        let keyed = self.glide_note - 60.0;
        let mut hp_note = p.hp_note
            + p.hp_env * filter_env * if p.hp_vel { velocity } else { 1.0 }
            + p.hp_key * keyed;
        let mut lp_note = p.lp_note
            + p.lp_env * filter_env * if p.lp_vel { velocity } else { 1.0 }
            + p.lp_key * keyed;
        if p.lfo_dest[4] {
            lp_note += lfo * LFO_FILTER_SEMITONES;
        }
        if p.lfo_dest[5] {
            hp_note += lfo * LFO_FILTER_SEMITONES;
        }
        if p.at_dest[4] {
            lp_note += pressure * AT_FILTER_SEMITONES;
        }
        if p.at_dest[5] {
            hp_note += pressure * AT_FILTER_SEMITONES;
        }
        if p.pm_dest[3] {
            lp_note += poly * PM_FILTER_SEMITONES;
        }
        if p.pm_dest[4] {
            hp_note += poly * PM_FILTER_SEMITONES;
        }

        let mut signal = mix;
        if p.hp_note > 0.0 || p.hp_env != 0.0 {
            let hz = raw::note_hz(hp_note).clamp(5.0, s.cutoff_ceiling_hz);
            signal = self.hpf.process(signal, hz, p.hp_res, s.sr);
        }
        let lp_hz = raw::note_hz(lp_note).clamp(5.0, s.cutoff_ceiling_hz);
        signal = voice_limit(self.lpf.process(signal, lp_hz, p.lp_res, s.sr));

        // ── Amplifier ──
        //
        // The envelope, the LFO and aftertouch are *summed* into one control
        // voltage rather than multiplied, which is what makes both of the
        // manual's own examples work: the gated-VCA trick needs env amount at
        // zero and a unipolar square to open the amplifier by itself, and
        // "if the Amplifier Envelope's env amount is set to full, positive
        // amounts of amp aftertouch will have no effect since the VCA is
        // already at its maximum output level" needs the sum to clamp.

        let mut gain = amp_env * p.vca_amount * if p.vca_vel { velocity } else { 1.0 };
        if p.lfo_dest[3] {
            gain += lfo;
        }
        if p.at_dest[3] {
            gain += pressure;
        }
        let out = signal * gain.clamp(0.0, 1.0) * p.volume;

        // Equal power across the spread, so that widening the field does not
        // change how loud the instrument is.
        let pan = VOICE_PAN[self.index] * p.pan_spread;
        let angle = (pan + 1.0) * (PI / 4.0);
        (out * angle.cos(), out * angle.sin())
    }
}

/// The sub-oscillator's shape: a triangle, fixed. "Because a triangle wave has
/// few harmonics and is mainly characterized by its fundamental frequency,
/// adding a sub octave to sounds such as bass are a great way to increase
/// their low-register presence." Not a square, which is what most sub
/// oscillators are and what makes this one softer.
const SUB_SHAPE: Shape = Shape {
    rise: 0.5,
    high: 0.0,
    fall: 0.5,
    mean: 0.0,
    scale: 1.0,
};

// ── The instrument ──

/// How many keys the keyboard remembers at once. Only unison needs the stack —
/// "Key Assign settings are only relevant to Unison mode. They do not affect
/// polyphonic playback" — but the stack is kept in both so that switching the
/// unison switch mid-phrase picks up the keys that are already down.
const MAX_HELD: usize = 16;

/// The maximum number of MIDI events sorted in place per block.
const MAX_EVENTS: usize = 256;

/// "Hold down a chord on the keyboard (6 notes maximum)."
const MAX_CHORD: usize = VOICES;

pub struct Prophet6 {
    params: [f32; PARAM_COUNT],
    sample_rate: f64,
    voices: [Voice; VOICES],
    lfo: Lfo,
    fx_a: FxSlot,
    fx_b: FxSlot,
    dc_left: DcBlock,
    dc_right: DcBlock,

    held: [u8; MAX_HELD],
    held_velocity: [u8; MAX_HELD],
    held_len: usize,
    /// The chord memory, as intervals from the note that was lowest when it
    /// was captured. Empty until the unison switch is moved to CHD with keys
    /// down; the factory programs that select CHD arrive with it empty.
    chord: [i16; MAX_CHORD],
    chord_len: usize,
    /// Which note the unison stack is currently sounding, if any.
    unison_note: Option<u8>,
    /// Where the unison-mode switch was, so that a move *to* CHD can be told
    /// from a program load that happens to select it.
    unison_mode_was: usize,
    /// Increments on every note-on, so that "oldest" has a meaning.
    clock: u64,

    /// CC 1. Rests at zero, as the wheel does on the instrument: "If you set
    /// this parameter to zero but still select a modulation destination,
    /// modulation is only applied when you use the Mod Wheel."
    mod_wheel: f64,
    /// Channel pressure, 0…1.
    pressure: f64,
    /// The pitch wheel, −1…+1, scaled by the bend range.
    bend: f64,
}

impl Prophet6 {
    #[must_use]
    pub fn new() -> Self {
        let sr = 44_100.0;
        let mut synth = Self {
            params: param_defaults(),
            sample_rate: sr,
            voices: std::array::from_fn(|i| Voice::new(i, sr)),
            lfo: Lfo::new(),
            fx_a: FxSlot::new(0x5A5A_1234),
            fx_b: FxSlot::new(0x3C3C_9876),
            dc_left: DcBlock::default(),
            dc_right: DcBlock::default(),
            held: [0; MAX_HELD],
            held_velocity: [0; MAX_HELD],
            held_len: 0,
            chord: [0; MAX_CHORD],
            chord_len: 0,
            unison_note: None,
            unison_mode_was: 0,
            clock: 0,
            mod_wheel: 0.0,
            pressure: 0.0,
            bend: 0.0,
        };
        synth.unison_mode_was = step_of(&synth.params, P_UNISON_MODE);
        synth.fx_a.init(sr);
        synth.fx_b.init(sr);
        synth
    }

    /// Which factory program the two selectors are pointing at, 0–499.
    #[must_use]
    pub fn current_program(&self) -> usize {
        program_index(self.params[P_BANK], self.params[P_PROGRAM])
    }

    fn sync_params_from_program(&mut self) {
        self.params = params_for_program(self.params[P_BANK], self.params[P_PROGRAM]);
        self.unison_mode_was = step_of(&self.params, P_UNISON_MODE);
    }

    /// The chord memory, as intervals from its lowest note. Empty when
    /// nothing has been captured.
    #[must_use]
    pub fn chord_memory(&self) -> &[i16] {
        &self.chord[..self.chord_len]
    }

    // ── The keyboard ──

    fn forget(&mut self, note: u8) {
        let Some(at) = self.held[..self.held_len].iter().position(|n| *n == note) else {
            return;
        };
        for i in at + 1..self.held_len {
            self.held[i - 1] = self.held[i];
            self.held_velocity[i - 1] = self.held_velocity[i];
        }
        self.held_len -= 1;
    }

    fn remember(&mut self, note: u8, velocity: u8) {
        self.forget(note);
        if self.held_len == MAX_HELD {
            for i in 1..MAX_HELD {
                self.held[i - 1] = self.held[i];
                self.held_velocity[i - 1] = self.held_velocity[i];
            }
            self.held_len -= 1;
        }
        self.held[self.held_len] = note;
        self.held_velocity[self.held_len] = velocity;
        self.held_len += 1;
    }

    /// Which held key wins under the selected key-assign mode. The three
    /// retrigger modes pick the same key as the three plain ones; what they
    /// change is whether the envelopes restart.
    fn winner(&self, key_mode: usize) -> Option<usize> {
        if self.held_len == 0 {
            return None;
        }
        let keys = &self.held[..self.held_len];
        Some(match key_mode % 3 {
            0 => keys.iter().enumerate().min_by_key(|(_, n)| **n).map_or(0, |(i, _)| i),
            1 => keys.iter().enumerate().max_by_key(|(_, n)| **n).map_or(0, |(i, _)| i),
            _ => self.held_len - 1,
        })
    }

    /// Capture the keys that are down as the chord memory. The hardware
    /// gesture is "hold down a chord on the keyboard, press the unison
    /// switch"; the only switch here is the voice-stacking selector, so
    /// stepping it onto CHD is what captures.
    fn capture_chord(&mut self) {
        let mut notes: [u8; MAX_HELD] = self.held;
        let len = self.held_len.min(MAX_CHORD);
        notes[..self.held_len].sort_unstable();
        self.chord_len = len;
        let base = i16::from(notes[0]);
        for (slot, held) in self.chord[..len].iter_mut().zip(&notes[..len]) {
            *slot = i16::from(*held) - base;
        }
    }

    /// Give a voice a note, gliding if the mode and the moment say so.
    fn place(&mut self, voice: usize, note: u8, velocity: u8, panel: &Panel, retrigger: bool) {
        let sr = self.sample_rate;
        let clock = self.clock;
        let v = &mut self.voices[voice];
        // "Fixed Rate A" and "Fixed Time A" only glide when playing legato:
        // "glide only occurs when a note is held until the next note is
        // played." The other two glide from wherever the voice last was.
        let legato = v.gate || v.amp_env.is_active();
        let glide = panel.glide_on && if panel.glide_mode % 2 == 1 { legato } else { v.pitched };
        v.retune(note, panel, sr, glide);
        if retrigger || !legato {
            v.start(velocity, panel, clock, panel.key_mode >= 3 && legato);
        } else {
            v.velocity = velocity;
            v.gate = true;
            v.age = clock;
        }
    }

    /// Point the unison stack at whichever key now wins.
    fn retarget_unison(&mut self, panel: &Panel, retrigger: bool) {
        let Some(at) = self.winner(panel.key_mode) else {
            for voice in &mut self.voices {
                voice.release();
            }
            self.unison_note = None;
            return;
        };
        let note = self.held[at];
        let velocity = self.held_velocity[at];
        let fresh = self.unison_note.is_none();
        self.unison_note = Some(note);

        // Chord memory transposes the stored voicing so that its lowest note
        // is the key played — "If low-note priority is chosen, the note that
        // you play corresponds to the lowest note of the chord voicing."
        let mut notes = [note; MAX_CHORD];
        let mut count = 1usize;
        if panel.chord_mode && self.chord_len > 0 {
            count = self.chord_len;
            let anchor = if panel.key_mode % 3 == 1 { self.chord[self.chord_len - 1] } else { 0 };
            for (slot, interval) in notes[..count].iter_mut().zip(&self.chord[..count]) {
                *slot = (i16::from(note) + interval - anchor).clamp(0, 127) as u8;
            }
        }

        let stack = panel.unison_voices.min(VOICES);
        for slot in 0..stack {
            self.place(slot, notes[slot % count], velocity, panel, retrigger || fresh);
        }
        for slot in stack..VOICES {
            if self.voices[slot].gate {
                self.voices[slot].release();
            }
        }
    }

    /// A free voice if there is one, otherwise the one whose note is oldest.
    /// A released voice that is still ringing is taken before a held one.
    fn allocate(&self) -> usize {
        if let Some(free) = self.voices.iter().position(Voice::is_free) {
            return free;
        }
        let released = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.gate)
            .min_by_key(|(_, v)| v.age);
        if let Some((index, _)) = released {
            return index;
        }
        self.voices
            .iter()
            .enumerate()
            .min_by_key(|(_, v)| v.age)
            .map_or(0, |(index, _)| index)
    }

    fn note_on(&mut self, note: u8, velocity: u8, panel: &Panel) {
        self.remember(note, velocity);
        self.clock += 1;
        if panel.unison {
            let retrigger = panel.key_mode >= 3;
            self.retarget_unison(panel, retrigger);
        } else {
            let slot = self.allocate();
            self.place(slot, note, velocity, panel, true);
        }
    }

    fn note_off(&mut self, note: u8, panel: &Panel) {
        self.forget(note);
        if panel.unison {
            self.retarget_unison(panel, false);
        } else {
            for voice in &mut self.voices {
                if voice.gate && voice.note == note {
                    voice.release();
                }
            }
        }
    }

    fn all_notes_off(&mut self) {
        self.held_len = 0;
        self.unison_note = None;
        for voice in &mut self.voices {
            voice.release();
        }
    }

    fn kill_all(&mut self) {
        self.held_len = 0;
        self.unison_note = None;
        for voice in &mut self.voices {
            voice.reset();
        }
        self.fx_a.reset();
        self.fx_b.reset();
        self.dc_left.reset();
        self.dc_right.reset();
    }
}

impl Default for Prophet6 {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for Prophet6 {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "Prophet-6".into(),
            version: "0.1.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        for (index, voice) in self.voices.iter_mut().enumerate() {
            *voice = Voice::new(index, sample_rate);
        }
        self.lfo.reset();
        self.fx_a.init(sample_rate);
        self.fx_b.init(sample_rate);
        self.dc_left.reset();
        self.dc_right.reset();
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() {
            return;
        }
        let buf_len = outputs[0].len();
        let sr = self.sample_rate;
        let panel = Panel::read(&self.params, sr);

        let shared_base = Shared {
            lfo: 0.0,
            lfo_depth: 0.0,
            pressure: 0.0,
            bend: 0.0,
            sr,
            slop_step: SLOP_UPDATE_HZ / sr,
            slop_coefficient: raw::one_pole(SLOP_HZ, sr),
            cutoff_ceiling_hz: (sr * 0.45).min(CUTOFF_MAX_HZ),
        };
        let dc = (-TAU * DC_BLOCK_HZ / sr).exp();

        // MIDI events, sorted in place without allocating.
        let mut order = [0usize; MAX_EVENTS];
        let event_count = midi_events.len().min(MAX_EVENTS);
        for (i, slot) in order[..event_count].iter_mut().enumerate() {
            *slot = i;
        }
        for i in 1..event_count {
            let mut j = i;
            while j > 0
                && midi_events[order[j]].sample_offset < midi_events[order[j - 1]].sample_offset
            {
                order.swap(j, j - 1);
                j -= 1;
            }
        }
        let mut next_event = 0;

        let stereo = outputs.len() >= 2;

        for i in 0..buf_len {
            while next_event < event_count
                && midi_events[order[next_event]].sample_offset as usize <= i
            {
                let event = &midi_events[order[next_event]];
                match event.status & 0xF0 {
                    0x90 => {
                        if event.data2 > 0 {
                            self.note_on(event.data1, event.data2, &panel);
                        } else {
                            self.note_off(event.data1, &panel);
                        }
                    }
                    0x80 => self.note_off(event.data1, &panel),
                    // Channel pressure. "The Prophet-6 provides monophonic (or
                    // 'channel') aftertouch, which means that applying
                    // pressure to any key within a chord will apply modulation
                    // to all notes currently held."
                    0xD0 => self.pressure = f64::from(event.data1) / 127.0,
                    0xE0 => {
                        let raw = i32::from(event.data2) * 128 + i32::from(event.data1);
                        self.bend = f64::from(raw - 8_192) / 8_192.0;
                    }
                    0xB0 => match event.data1 {
                        1 => self.mod_wheel = f64::from(event.data2) / 127.0,
                        120 => self.kill_all(),
                        123 => self.all_notes_off(),
                        _ => {}
                    },
                    _ => {}
                }
                next_event += 1;
            }

            let lfo_value = self.lfo.tick(panel.lfo_hz, sr, panel.lfo_shape, panel.lfo_noise);
            let wheel = panel.lfo_initial + self.mod_wheel * (1.0 - panel.lfo_initial);
            let at_lfo = if panel.at_dest[2] { self.pressure * panel.at_amount } else { 0.0 };
            let shared = Shared {
                lfo: lfo_value,
                lfo_depth: (wheel + at_lfo).clamp(-1.0, 1.0),
                pressure: self.pressure,
                bend: self.bend * panel.bend_range,
                ..shared_base
            };

            let mut left = 0.0;
            let mut right = 0.0;
            for voice in &mut self.voices {
                let (l, r) = voice.tick(&panel, &shared);
                left += l;
                right += r;
            }

            if panel.distortion > 0.0 {
                left = self.dc_left.tick(distort(left, panel.distortion), dc);
                right = self.dc_right.tick(distort(right, panel.distortion), dc);
            }

            if panel.fx_on {
                let (l, r) = self.fx_a.process(left, right, &panel.fx_a, sr);
                let (l, r) = self.fx_b.process(l, r, &panel.fx_b, sr);
                left = l;
                right = r;
            }

            outputs[0][i] = soft_saturate(left as f32 * OUTPUT_TRIM);
            if stereo {
                outputs[1][i] = soft_saturate(right as f32 * OUTPUT_TRIM);
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
        let defaults = param_defaults();
        Some(ParameterInfo {
            name: PARAM_NAMES[index].into(),
            min: 0.0,
            max: 1.0,
            default: defaults[index],
            unit: match index {
                P_F_ATTACK | P_F_DECAY | P_F_RELEASE | P_A_ATTACK | P_A_DECAY | P_A_RELEASE
                | P_GLIDE_RATE => "s".into(),
                P_LFO_FREQ | P_HP_CUTOFF | P_LP_CUTOFF => "Hz".into(),
                P_OSC1_FREQ | P_OSC2_FREQ => "semi".into(),
                P_BPM => "bpm".into(),
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
        match index {
            P_PROGRAM | P_BANK => self.sync_params_from_program(),
            P_UNISON_MODE => {
                let now = step_of(&self.params, P_UNISON_MODE);
                if now >= 6 && self.unison_mode_was < 6 && self.held_len > 0 {
                    self.capture_chord();
                }
                self.unison_mode_was = now;
            }
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.kill_all();
        self.lfo.reset();
        self.mod_wheel = 0.0;
        self.pressure = 0.0;
        self.bend = 0.0;
        self.clock = 0;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const SR: f64 = 44_100.0;
    const BLOCK: usize = 256;

    pub(crate) fn note_on(note: u8, velocity: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x90, data1: note, data2: velocity }
    }
    pub(crate) fn note_off(note: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x80, data1: note, data2: 0 }
    }
    fn cc(number: u8, value: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0xB0, data1: number, data2: value }
    }
    fn aftertouch(amount: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0xD0, data1: amount, data2: 0 }
    }

    /// A synth at factory program `index`, initialised and silent.
    pub(crate) fn fresh(index: usize) -> Prophet6 {
        let mut s = Prophet6::new();
        s.init(SR, BLOCK);
        let (bank, program) = program_knobs(index);
        s.set_parameter(P_BANK, bank);
        s.set_parameter(P_PROGRAM, program);
        s.reset();
        s
    }

    /// A synth with the panel set by hand rather than by a program.
    fn built(setup: &[(usize, f32)]) -> Prophet6 {
        let mut s = Prophet6::new();
        s.init(SR, BLOCK);
        for (index, value) in setup {
            s.set_parameter(*index, *value);
        }
        s.reset();
        s
    }

    pub(crate) fn render(synth: &mut Prophet6, events: &[MidiEvent], blocks: usize) -> Vec<f32> {
        render_at(synth, events, blocks, SR)
    }

    pub(crate) fn render_at(synth: &mut Prophet6, events: &[MidiEvent], blocks: usize, sr: f64) -> Vec<f32> {
        let mut left = vec![0.0f32; BLOCK];
        let mut right = vec![0.0f32; BLOCK];
        let mut out = Vec::with_capacity(blocks * BLOCK);
        let _ = sr;
        for block in 0..blocks {
            left.fill(0.0);
            right.fill(0.0);
            let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
            synth.process(&[], &mut outs, if block == 0 { events } else { &[] });
            out.extend_from_slice(&left);
        }
        out
    }

    /// A held chord, rendered from a synth already pointed at a program.
    pub(crate) fn render_program(
        synth: &mut Prophet6,
        notes: &[u8],
        velocity: u8,
        blocks: usize,
    ) -> Vec<f32> {
        let events: Vec<MidiEvent> = notes
            .iter()
            .map(|&n| MidiEvent { sample_offset: 0, status: 0x90, data1: n, data2: velocity })
            .collect();
        render(synth, &events, blocks)
    }

    pub(crate) fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    pub(crate) fn rms(x: &[f32]) -> f64 {
        if x.is_empty() {
            return 0.0;
        }
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64).sqrt()
    }

    /// RMS over 4096-sample windows, which is what every level claim in this
    /// module is made against — long enough to average a 10 Hz cycle, short
    /// enough to see an envelope move.
    fn window_rms(x: &[f32]) -> Vec<f64> {
        x.chunks(4096).filter(|c| c.len() == 4096).map(rms).collect()
    }

    /// The centre of gravity of the spectrum, in hertz: the rms of the first
    /// difference over the rms of the signal is the spectral centroid in
    /// radians a sample.
    fn brightness(x: &[f32], sr: f64) -> f64 {
        let (mut total, mut slope, mut last) = (0.0f64, 0.0f64, 0.0f64);
        for v in x {
            let s = f64::from(*v);
            total += s * s;
            slope += (s - last) * (s - last);
            last = s;
        }
        (slope / total.max(1.0e-30)).sqrt() * sr / TAU
    }

    /// Energy below `hz`, as an rms, through a two-pole one-pole cascade.
    pub(crate) fn low_band(x: &[f32], hz: f64, sr: f64) -> f64 {
        let a = raw::one_pole(hz, sr);
        let (mut s1, mut s2, mut sum) = (0.0f64, 0.0f64, 0.0f64);
        for v in x {
            s1 += a * (f64::from(*v) - s1);
            s2 += a * (s1 - s2);
            sum += s2 * s2;
        }
        (sum / x.len().max(1) as f64).sqrt()
    }

    // ── The instrument ──

    #[test]
    fn silence_with_no_input() {
        let mut s = fresh(0);
        let out = render(&mut s, &[], 4);
        assert_eq!(peak(&out), 0.0, "an idle instrument is not silent");
    }

    #[test]
    fn a_note_before_init_is_silence_rather_than_a_panic() {
        let mut s = Prophet6::new();
        let mut left = [0.0f32; 64];
        let mut outs: [&mut [f32]; 1] = [&mut left];
        s.process(&[], &mut outs, &[note_on(60, 100, 0)]);
        assert!(left.iter().all(|v| v.is_finite()));
        // And with no outputs at all.
        s.process(&[], &mut [], &[note_on(60, 100, 0)]);
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = fresh(0);
        let out = render(&mut s, &[note_on(60, 100, 0)], 40);
        assert!(peak(&out) > 0.001, "the default program made no sound: {}", peak(&out));
    }

    #[test]
    fn silent_after_release() {
        let mut s = fresh(0);
        render(&mut s, &[note_on(60, 100, 0)], 20);
        let tail = render(&mut s, &[note_off(60, 0)], 900);
        let last = &tail[tail.len() - 4096..];
        assert!(rms(last) < 1.0e-6, "still ringing 5 s after release: {}", rms(last));
    }

    #[test]
    fn output_is_finite_across_the_keyboard() {
        for program in [0usize, 31, 191, 419, 486] {
            for note in [0u8, 24, 48, 60, 84, 108, 127] {
                let mut s = fresh(program);
                let out = render(&mut s, &[note_on(note, 127, 0)], 30);
                assert!(
                    out.iter().all(|v| v.is_finite()),
                    "program {program} note {note} produced a non-finite sample"
                );
                assert!(peak(&out) < 1.0, "program {program} note {note} reached full scale");
            }
        }
    }

    #[test]
    fn cc120_kills_and_cc123_releases() {
        let mut s = fresh(13); // Thick Low Strings: a long release.
        render(&mut s, &[note_on(60, 110, 0)], 30);
        let released = render(&mut s, &[cc(123, 0, 0)], 4);
        assert!(rms(&released) > 0.0, "all-notes-off silenced the release tail instantly");

        let mut s = fresh(13);
        render(&mut s, &[note_on(60, 110, 0)], 30);
        let killed = render(&mut s, &[cc(120, 0, 0)], 4);
        assert_eq!(peak(&killed[BLOCK..]), 0.0, "all-sound-off left something ringing");
    }

    #[test]
    fn sample_accurate_midi() {
        let mut s = fresh(0);
        let mut left = vec![0.0f32; BLOCK];
        let mut outs: [&mut [f32]; 1] = [&mut left];
        s.process(&[], &mut outs, &[note_on(60, 100, 200)]);
        assert_eq!(
            peak(&left[..190]),
            0.0,
            "a note at offset 200 sounded before the offset"
        );
    }

    #[test]
    fn all_params_readable() {
        let s = fresh(0);
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            let info = s.parameter_info(index).expect("every index has info");
            assert_eq!(&info.name, name);
            assert!(s.get_parameter(index).is_finite());
        }
        assert!(s.parameter_info(PARAM_COUNT).is_none());
        assert_eq!(s.parameter_count(), PARAM_COUNT);
    }

    #[test]
    fn junk_parameters_do_not_escape_the_panel() {
        let mut s = fresh(0);
        for index in 0..PARAM_COUNT {
            for junk in [-5.0f32, 5.0, f32::NAN, f32::INFINITY] {
                s.set_parameter(index, junk);
            }
        }
        let out = render(&mut s, &[note_on(60, 100, 0)], 20);
        assert!(out.iter().all(|v| v.is_finite()), "junk on the panel escaped as a sample");
    }

    // ── Rate independence ──
    //
    // Everything below is measured at four sample rates and asserted to agree,
    // because the defect this guards against is the one that only shows up on
    // someone else's audio device: a coefficient with 44100 baked into it
    // sounds right on the machine it was voiced on and wrong everywhere else.

    const RATES: [f64; 4] = [22_050.0, 44_100.0, 48_000.0, 96_000.0];

    /// Every control at its neutral position: no modulation anywhere, one
    /// oscillator at unity, the filters out of the way and the amplifier open
    /// for as long as the key is down.
    ///
    /// Built explicitly rather than by starting from a factory program and
    /// overriding what a test cares about. The first version did the latter
    /// and inherited *Brassed Off*'s poly mod to pulse width, which quietly
    /// put a second harmonic on every waveform the oscillator tests measured.
    pub(crate) fn neutral() -> Vec<(usize, f32)> {
        let off = |index: usize| (index, 0.0f32);
        let mut panel = vec![
            (P_OSC1_FREQ, 24.0 / 60.0),
            (P_OSC1_SHAPE, 0.0),
            (P_OSC1_PW, 0.5),
            off(P_SYNC),
            (P_OSC2_FREQ, 24.0 / 60.0),
            (P_OSC2_FINE, 0.5),
            (P_OSC2_SHAPE, 0.0),
            (P_OSC2_PW, 0.5),
            off(P_OSC2_LOW),
            (P_OSC2_KEY, 1.0),
            off(P_SLOP),
            (P_OSC1_LEVEL, 1.0),
            off(P_OSC2_LEVEL),
            off(P_SUB_LEVEL),
            off(P_NOISE_LEVEL),
            off(P_HP_CUTOFF),
            off(P_HP_RESO),
            (P_HP_ENV, 0.5),
            off(P_HP_VEL),
            off(P_HP_KEY),
            (P_LP_CUTOFF, 120.0 / 164.0),
            off(P_LP_RESO),
            (P_LP_ENV, 0.5),
            off(P_LP_VEL),
            off(P_LP_KEY),
            off(P_F_ATTACK),
            (P_F_DECAY, 1.0),
            (P_F_SUSTAIN, 1.0),
            off(P_F_RELEASE),
            (P_VCA_ENV, 1.0),
            off(P_VCA_VEL),
            off(P_A_ATTACK),
            (P_A_DECAY, 1.0),
            (P_A_SUSTAIN, 1.0),
            off(P_A_RELEASE),
            (P_LFO_FREQ, 0.5),
            (P_LFO_SHAPE, 0.0),
            off(P_LFO_AMOUNT),
            (P_PM_FILTER_ENV, 0.5),
            (P_PM_OSC2, 0.5),
            (P_AT_AMOUNT, 0.5),
            off(P_DISTORTION),
            off(P_FX_ON),
            off(P_UNISON),
            (P_UNISON_MODE, 0.0),
            (P_KEY_MODE, 0.0),
            off(P_GLIDE),
            (P_GLIDE_MODE, 0.0),
            off(P_GLIDE_RATE),
            (P_BEND_RANGE, knob_for(2, BEND_RANGE_MAX + 1)),
            off(P_PAN_SPREAD),
            (P_VOLUME, 1.0),
        ];
        // Every modulation destination off, all seventeen of them.
        for index in [
            P_LFO_FREQ1, P_LFO_FREQ2, P_LFO_PW, P_LFO_AMP, P_LFO_LP, P_LFO_HP, P_PM_FREQ1,
            P_PM_SHAPE1, P_PM_PW1, P_PM_LP, P_PM_HP, P_AT_FREQ1, P_AT_FREQ2, P_AT_LFO,
            P_AT_AMP, P_AT_LP, P_AT_HP,
        ] {
            panel.push(off(index));
        }
        panel
    }

    /// The neutral panel with a triangle on oscillator 1: zero crossings on
    /// this are twice the fundamental.
    pub(crate) fn plain_tone() -> Vec<(usize, f32)> {
        neutral()
    }

    pub(crate) fn at_rate(setup: &[(usize, f32)], sr: f64) -> Prophet6 {
        let mut s = Prophet6::new();
        s.init(sr, BLOCK);
        for (index, value) in setup {
            s.set_parameter(*index, *value);
        }
        s.reset();
        s
    }

    /// Zero crossings a second, with hysteresis at a tenth of the peak so
    /// that a wobble around zero counts once rather than three times.
    pub(crate) fn crossings_per_second(x: &[f32], sr: f64) -> f64 {
        let gate = peak(x) * 0.1;
        let mut crossings = 0u32;
        let mut sign = 0i32;
        for v in x {
            if *v > gate && sign <= 0 {
                if sign != 0 {
                    crossings += 1;
                }
                sign = 1;
            } else if *v < -gate && sign >= 0 {
                if sign != 0 {
                    crossings += 1;
                }
                sign = -1;
            }
        }
        f64::from(crossings) / (x.len() as f64 / sr)
    }

    /// The repetition rate of a waveform, by autocorrelation: the best
    /// correlating lag, then the first submultiple that correlates nearly as
    /// well so that a shape whose second half resembles its first is not read
    /// an octave down, then a parabola through the peak.
    fn fundamental_hz(x: &[f32], sr: f64, low: f64, high: f64) -> f64 {
        let mean = f64::from(x.iter().sum::<f32>()) / x.len() as f64;
        let centred: Vec<f64> = x.iter().map(|v| f64::from(*v) - mean).collect();
        let min_lag = ((sr / high) as usize).max(1);
        let max_lag = ((sr / low) as usize).min(centred.len() / 2);
        let mut scores = Vec::with_capacity(max_lag - min_lag + 1);
        for lag in min_lag..=max_lag {
            let (mut num, mut a2, mut b2) = (0.0, 0.0, 0.0);
            for i in 0..centred.len() - lag {
                num += centred[i] * centred[i + lag];
                a2 += centred[i] * centred[i];
                b2 += centred[i + lag] * centred[i + lag];
            }
            scores.push(num / (a2 * b2).sqrt().max(1.0e-30));
        }
        let mut at = scores
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map_or(0, |(i, _)| i);
        let best = scores[at];
        for i in 1..at {
            if scores[i] >= scores[i - 1] && scores[i] > scores[i + 1] && scores[i] > best * 0.8 {
                at = i;
                break;
            }
        }
        let refine = if at > 0 && at + 1 < scores.len() {
            let (a, b, c) = (scores[at - 1], scores[at], scores[at + 1]);
            let denominator = 2.0 * (2.0 * b - a - c);
            if denominator.abs() > 1.0e-12 {
                (c - a) / denominator
            } else {
                0.0
            }
        } else {
            0.0
        };
        sr / ((min_lag + at) as f64 + refine)
    }

    /// The magnitude of one frequency in a signal, by a single DFT bin. Slow
    /// and exact enough: a handful of harmonics per test.
    fn harmonic(x: &[f32], hz: f64, sr: f64) -> f64 {
        let w = TAU * hz / sr;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, v) in x.iter().enumerate() {
            let phase = w * n as f64;
            re += f64::from(*v) * phase.cos();
            im += f64::from(*v) * phase.sin();
        }
        (re * re + im * im).sqrt() / x.len() as f64
    }

    /// The first `count` harmonics of `hz`, each as a share of the first.
    fn harmonics(x: &[f32], hz: f64, sr: f64, count: usize) -> Vec<f64> {
        let first = harmonic(x, hz, sr).max(1.0e-30);
        (1..=count).map(|n| harmonic(x, hz * n as f64, sr) / first).collect()
    }

    #[test]
    fn the_pitch_is_the_same_at_every_sample_rate() {
        for note in [36u8, 48, 60, 72] {
            let reference = {
                let mut s = at_rate(&plain_tone(), 44_100.0);
                let out = render_at(&mut s, &[note_on(note, 100, 0)], 60, 44_100.0);
                crossings_per_second(&out, 44_100.0)
            };
            for rate in RATES {
                let mut s = at_rate(&plain_tone(), rate);
                let blocks = (60.0 * rate / 44_100.0) as usize;
                let out = render_at(&mut s, &[note_on(note, 100, 0)], blocks, rate);
                let measured = crossings_per_second(&out, rate);
                let error = (measured - reference).abs() / reference;
                assert!(
                    error < 0.02,
                    "note {note} at {rate} Hz sounds {measured:.1} crossings a second \
                     against {reference:.1} at 44100 — {:.1}% out",
                    error * 100.0
                );
            }
        }
    }

    #[test]
    fn the_lfo_runs_at_the_rate_the_knob_says_at_every_sample_rate() {
        // The manual's own gated-VCA recipe, which is also the cleanest way
        // to count LFO cycles: "set the vca env amount to zero, route the LFO
        // square wave to amp with an initial amt setting of 100%". The output
        // is then the oscillator switched on and off at the LFO rate.
        let mut setup = plain_tone();
        setup.extend([
            (P_LFO_SHAPE, knob_for(3, LFO_SHAPES.len())),
            (P_LFO_AMOUNT, 1.0),
            (P_LFO_AMP, 1.0),
            (P_LFO_FREQ, 100.0 / 255.0),
            (P_VCA_ENV, 0.0),
        ]);
        let expected = raw::lfo_hz(100.0);
        for rate in RATES {
            let mut s = at_rate(&setup, rate);
            let blocks = (900.0 * rate / 44_100.0) as usize;
            let out = render_at(&mut s, &[note_on(60, 100, 0)], blocks, rate);
            assert!(peak(&out) > 0.0, "the gated VCA never opened at {rate} Hz");
            // Count the gate openings from the short-window envelope.
            let window = (rate / 1_000.0) as usize;
            let envelope: Vec<f64> = out
                .chunks(window)
                .map(|c| c.iter().map(|v| f64::from(v.abs())).fold(0.0f64, f64::max))
                .collect();
            let top = envelope.iter().copied().fold(0.0f64, f64::max);
            let mut opens: Vec<usize> = Vec::new();
            let mut open = false;
            for (i, level) in envelope.iter().enumerate() {
                if *level > top * 0.5 && !open {
                    opens.push(i);
                    open = true;
                } else if *level < top * 0.1 {
                    open = false;
                }
            }
            assert!(opens.len() >= 4, "the gate opened {} times at {rate} Hz", opens.len());
            // Between the first and the last opening, so that a partial cycle
            // at either end of the render cannot be counted as a whole one.
            let span = (opens[opens.len() - 1] - opens[0]) as f64 * window as f64 / rate;
            let measured = (opens.len() - 1) as f64 / span;
            assert!(
                (measured - expected).abs() / expected < 0.1,
                "the LFO gated at {measured:.2} Hz at {rate} Hz, expected {expected:.2}"
            );
        }
    }

    #[test]
    fn the_envelope_takes_its_time_at_every_sample_rate() {
        // A decay from full to silence with no sustain under it. The segment
        // is 3.5 time constants aimed a little past zero, so the time to
        // silence is the time the knob names — which is the property worth
        // pinning, because it is what makes the panel's seconds honest.
        let mut setup = plain_tone();
        setup.extend([(P_A_ATTACK, 0.0), (P_A_DECAY, 90.0 / 127.0), (P_A_SUSTAIN, 0.0)]);
        let expected = raw::env_seconds(90.0);
        for rate in RATES {
            let mut s = at_rate(&setup, rate);
            let blocks = (800.0 * rate / 44_100.0) as usize;
            let out = render_at(&mut s, &[note_on(60, 100, 0)], blocks, rate);
            let window = (rate / 200.0) as usize;
            let envelope: Vec<f64> = out.chunks(window).map(rms).collect();
            let top = envelope.iter().copied().fold(0.0f64, f64::max);
            let last = envelope
                .iter()
                .rposition(|v| *v > top * 0.001)
                .unwrap_or(0) as f64
                * window as f64
                / rate;
            assert!(
                (last - expected).abs() / expected < 0.06,
                "the decay reached silence after {last:.3} s at {rate} Hz, and the knob \
                 says {expected:.3}"
            );
        }
    }

    // ── The oscillator ──

    /// The first three harmonics of a held A2, for a shape and a width.
    fn shape_harmonics(shape: f32, width: f32) -> Vec<f64> {
        let mut setup = plain_tone();
        setup.extend([(P_OSC1_SHAPE, shape), (P_OSC1_PW, width), (P_LP_CUTOFF, 1.0)]);
        let mut s = built(&setup);
        // Skip the attack, so the DFT sees a steady waveform.
        render(&mut s, &[note_on(45, 100, 0)], 10);
        let out = render(&mut s, &[], 60);
        harmonics(&out, raw::note_hz(45.0), SR, 3)
    }

    #[test]
    fn the_shape_knob_morphs_triangle_through_sawtooth_to_pulse() {
        // "Triangle, Sawtooth, Pulse — ... Waveshapes are continuously
        // variable and smoothly transition from one shape to the next."
        // Each of the three has a spectrum nothing else has: a triangle is odd
        // harmonics falling as 1/n², a sawtooth is every harmonic falling as
        // 1/n, and a square is odd harmonics falling as 1/n. So the second
        // harmonic tells a sawtooth from the other two and the third tells a
        // triangle from a square.
        let triangle = shape_harmonics(0.0, 0.5);
        assert!(triangle[1] < 0.05, "the triangle end has a second harmonic: {triangle:?}");
        assert!(
            (0.07..0.16).contains(&triangle[2]),
            "the triangle end's third harmonic is not 1/9: {triangle:?}"
        );

        let sawtooth = shape_harmonics(0.5, 0.5);
        assert!(
            (0.40..0.60).contains(&sawtooth[1]),
            "the middle of the travel is not a sawtooth: {sawtooth:?}"
        );
        assert!(
            (0.25..0.42).contains(&sawtooth[2]),
            "the middle of the travel is not a sawtooth: {sawtooth:?}"
        );

        let square = shape_harmonics(1.0, 0.5);
        assert!(square[1] < 0.08, "the pulse end at centre width is not a square: {square:?}");
        assert!(
            (0.25..0.42).contains(&square[2]),
            "the pulse end at centre width is not a square: {square:?}"
        );

        // And the in-between positions really are in between rather than a
        // crossfade of the two ends, which would show as a second harmonic
        // that jumps rather than travels.
        let quarter = shape_harmonics(0.25, 0.5);
        assert!(
            quarter[1] > triangle[1] + 0.05 && quarter[1] < sawtooth[1] - 0.05,
            "a quarter of the way along, the second harmonic is not between the \
             triangle's and the sawtooth's: {quarter:?}"
        );
        let three_quarters = shape_harmonics(0.75, 0.5);
        assert!(
            three_quarters[1] < sawtooth[1] - 0.05 && three_quarters[1] > square[1] + 0.02,
            "three quarters of the way along, the second harmonic is not between the \
             sawtooth's and the square's: {three_quarters:?}"
        );
    }

    #[test]
    fn pulse_width_is_square_at_centre_and_narrow_at_both_ends() {
        // "Changes the width of the pulse wave from a square wave when the
        // pulse width knob is at center position, to a very narrow pulse wave
        // when the pulse width knob is full left or right." A square has no
        // even harmonics; a narrow pulse has all of them; and the two ends of
        // the travel are the same duty cycle one way up and the other, so
        // they have the same spectrum.
        let centre = shape_harmonics(1.0, 0.5);
        let left = shape_harmonics(1.0, 0.0);
        let right = shape_harmonics(1.0, 1.0);
        assert!(centre[1] < 0.08, "the centre of the width travel is not a square: {centre:?}");
        assert!(left[1] > 0.7, "the left end of the width travel is not narrow: {left:?}");
        assert!(right[1] > 0.7, "the right end of the width travel is not narrow: {right:?}");
        for (a, b) in left.iter().zip(&right) {
            assert!(
                (a - b).abs() < 0.05,
                "the two ends of the width travel are not the same duty cycle: \
                 {left:?} and {right:?}"
            );
        }
    }

    #[test]
    fn the_sub_is_an_octave_below_oscillator_1() {
        let mut setup = plain_tone();
        setup.extend([(P_OSC1_LEVEL, 0.0), (P_SUB_LEVEL, 1.0)]);
        let mut s = built(&setup);
        let sub = fundamental_hz(&render(&mut s, &[note_on(57, 100, 0)], 80), SR, 40.0, 2_000.0);
        let mut s = built(&plain_tone());
        let osc = fundamental_hz(&render(&mut s, &[note_on(57, 100, 0)], 80), SR, 40.0, 2_000.0);
        assert!(
            (sub * 2.0 - osc).abs() / osc < 0.03,
            "the sub sounds at {sub:.1} Hz against the oscillator's {osc:.1}, \
             which is not an octave"
        );
    }

    #[test]
    fn sync_locks_oscillator_1_to_oscillator_2() {
        // "Sync forces Oscillator 1 (the slave) to restart its cycle every
        // time Oscillator 2 (the master) starts a cycle." So with sync on,
        // moving oscillator 1 up an octave leaves the pitch where oscillator 2
        // put it and changes the timbre instead — which is the whole sound.
        let play = |sync: f32, osc1_freq: f32| {
            let mut setup = plain_tone();
            setup.extend([
                (P_SYNC, sync),
                (P_OSC1_FREQ, osc1_freq),
                (P_OSC2_FREQ, 24.0 / 60.0),
                (P_OSC1_SHAPE, 0.5),
                (P_OSC2_LEVEL, 0.0),
                (P_LP_CUTOFF, 1.0),
            ]);
            let mut s = built(&setup);
            render(&mut s, &[note_on(45, 100, 0)], 10);
            let out = render(&mut s, &[], 60);
            (fundamental_hz(&out, SR, 60.0, 2_000.0), brightness(&out, SR))
        };
        let expected = raw::note_hz(45.0);

        let (free_low, _) = play(0.0, 24.0 / 60.0);
        let (free_high, _) = play(0.0, 31.0 / 60.0);
        assert!(
            (free_low - expected).abs() / expected < 0.02,
            "unsynced at unison the pitch is {free_low:.1} Hz, expected {expected:.1}"
        );
        // Seven semitones rather than twelve: at an octave the slave's own
        // cycle ends exactly where the master resets it, so a synced octave
        // and a free one are the same waveform and the test would prove
        // nothing.
        let fifth = expected * (7.0f64 / 12.0).exp2();
        assert!(
            (free_high - fifth).abs() / fifth < 0.04,
            "unsynced a fifth up the pitch is {free_high:.1} Hz, expected {fifth:.1}"
        );

        let (synced_low, dull) = play(1.0, 24.0 / 60.0);
        let (synced_high, _) = play(1.0, 31.0 / 60.0);
        assert!(
            (synced_low - expected).abs() / expected < 0.02
                && (synced_high - expected).abs() / expected < 0.02,
            "with sync on the pitch followed oscillator 1: {synced_low:.1} Hz then \
             {synced_high:.1} Hz, and oscillator 2 says {expected:.1}"
        );
        // Seventeen semitones up, still not a whole-number ratio, so the
        // pitch is still the master's and the formant has moved two octaves.
        let (synced_top, bright) = play(1.0, 41.0 / 60.0);
        assert!(
            (synced_top - expected).abs() / expected < 0.02,
            "with sync on and the slave seventeen semitones up the pitch is \
             {synced_top:.1} Hz, and oscillator 2 says {expected:.1}"
        );
        assert!(
            bright > dull * 1.3,
            "sweeping the synced oscillator did not change the timbre: {dull:.0} then {bright:.0}"
        );
    }

    #[test]
    fn oscillator_2_low_frequency_drops_it_into_the_lfo_band() {
        // With the low-frequency switch on, oscillator 2 is a modulator
        // rather than a voice: poly mod to oscillator 1's frequency should
        // wobble the pitch slowly rather than produce sidebands.
        let mut setup = plain_tone();
        setup.extend([
            (P_OSC2_LOW, 1.0),
            (P_OSC2_KEY, 0.0),
            (P_OSC2_FREQ, 36.0 / 60.0),
            (P_OSC2_SHAPE, 0.0),
            (P_PM_OSC2, 1.0),
            (P_PM_FILTER_ENV, 0.5),
            (P_PM_FREQ1, 1.0),
        ]);
        let mut s = built(&setup);
        let out = render(&mut s, &[note_on(60, 100, 0)], 400);
        // The pitch has to move, and it moves at a few hertz, so it is
        // measured across the whole render rather than at two points that
        // could land on the same part of a cycle.
        let rates: Vec<f64> = out
            .chunks(4_096)
            .filter(|c| c.len() == 4_096)
            .map(|c| crossings_per_second(c, SR))
            .collect();
        let high = rates.iter().copied().fold(0.0f64, f64::max);
        let low = rates.iter().copied().fold(f64::MAX, f64::min);
        assert!(
            high / low > 1.1,
            "oscillator 2 in low-frequency mode did not modulate anything: {low:.0} to {high:.0}"
        );
    }

    // ── The filters ──

    /// The steady-state gain of a filter at `hz`, by driving a sine through it
    /// and comparing rms in to rms out.
    ///
    /// Settle and measure are both fixed at 0.2 s rather than at a number of
    /// cycles. A number of cycles is the obvious choice and it is wrong: deep
    /// in a filter's stopband the measurement window is short in *time*, so
    /// the settling transient — which is set by the cutoff, not by the probe
    /// frequency — is most of what gets averaged, and a four-pole reads as
    /// fourteen decibels an octave.
    fn filter_gain(mut step: impl FnMut(f64) -> f64, hz: f64) -> f64 {
        let settle = (SR * 0.2) as usize;
        let window = (SR * 0.2) as usize;
        let mut input = 0.0;
        let mut output = 0.0;
        for n in 0..settle + window {
            let x = (TAU * hz * n as f64 / SR).sin();
            let y = step(x);
            if n >= settle {
                input += x * x;
                output += y * y;
            }
        }
        (output / input.max(1.0e-30)).sqrt()
    }

    pub(crate) fn db(x: f64) -> f64 {
        20.0 * x.max(1.0e-30).log10()
    }

    #[test]
    fn the_low_pass_is_four_poles_and_the_high_pass_two() {
        // "The Low-Pass Filter is a 4-pole, 24 dB per-octave, resonant filter.
        // The High-Pass Filter is a 2-pole, 12 dB per octave, resonant
        // filter." Measured an octave apart, well into the stopband where the
        // asymptote holds.
        // Measured two and three octaves above the corner, and low enough in
        // the spectrum that the trapezoidal integrator's own zero at Nyquist
        // has not started to steepen the slope: at 1.6 kHz it is worth two
        // tenths of a decibel.
        let cutoff = 200.0;
        let mut lp = LowPass::new();
        let a = db(filter_gain(|x| lp.process(x, cutoff, 0.0, SR), 800.0));
        let mut lp = LowPass::new();
        let b = db(filter_gain(|x| lp.process(x, cutoff, 0.0, SR), 1_600.0));
        assert!(
            (-26.0..-22.0).contains(&(b - a)),
            "the low-pass rolls off at {:.1} dB an octave, not 24",
            a - b
        );

        let corner = 500.0;
        let mut hp = HighPass::new();
        let a = db(filter_gain(|x| hp.process(x, corner, 0.0, SR), 125.0));
        let mut hp = HighPass::new();
        let b = db(filter_gain(|x| hp.process(x, corner, 0.0, SR), 62.5));
        assert!(
            (-14.0..-10.0).contains(&(b - a)),
            "the high-pass rolls off at {:.1} dB an octave, not 12",
            a - b
        );
    }

    #[test]
    fn the_low_pass_resonates_and_then_oscillates() {
        // "High levels of resonance can cause the filter to self oscillate
        // and generate its own pitch."
        let cutoff = 1_000.0;
        let peak_at = |resonance: f64| {
            let mut lp = LowPass::new();
            db(filter_gain(|x| lp.process(x, cutoff, resonance, SR), cutoff))
        };
        let flat = peak_at(0.0);
        let mid = peak_at(0.6);
        assert!(mid > flat + 6.0, "resonance at 0.6 lifts the corner by only {:.1} dB", mid - flat);

        // With no input at all, the top of the travel produces a tone at the
        // cutoff and the bottom produces nothing.
        let ring = |resonance: f64| {
            let mut lp = LowPass::new();
            lp.start(resonance);
            let mut out = vec![0.0f32; 20_000];
            for v in &mut out {
                *v = lp.process(0.0, cutoff, resonance, SR) as f32;
            }
            let tail = &out[10_000..];
            (rms(tail), fundamental_hz(tail, SR, 300.0, 4_000.0))
        };
        let (quiet, _) = ring(0.5);
        let (loud, hz) = ring(1.0);
        assert!(quiet < 1.0e-6, "the filter oscillates halfway up the resonance travel");
        assert!(loud > 0.01, "the filter does not self oscillate at the top of the travel");
        assert!(
            (hz - cutoff).abs() / cutoff < 0.06,
            "the self-oscillation sits at {hz:.0} Hz rather than at the cutoff"
        );
    }

    /// The difference between an SSM2040-lineage filter and a Moog ladder,
    /// measured end to end against the ladder in the rack.
    ///
    /// A transistor ladder subtracts its resonance feedback from the signal at
    /// the input of the first stage with nothing to make it up, so its
    /// passband gain is `1/(1+k)` and a resonant bass patch loses its bass.
    /// The SSM2040's summing stage is compensated. This is the single
    /// audible difference between the two designs and the reason the Prophet's
    /// filter is written out rather than borrowed from `phatty.rs`, so it is
    /// held open by measurement rather than by comment.
    /// How much bass each of the two filters in the rack loses between no
    /// resonance and full resonance, in dB: `(prophet, ladder)`.
    ///
    /// A low sawtooth with the filter wide open, so the whole waveform is in
    /// the passband and the only thing resonance can do to it is take the
    /// bottom out. Measured under 150 Hz, which is well below the corner and
    /// well below where either filter self-oscillates.
    pub(crate) fn passband_loss() -> (f64, f64) {
        use crate::phatty::{self, LittlePhatty};

        const LOW_NOTE: u8 = 33; // A1, 55 Hz.
        const BAND_HZ: f64 = 150.0;

        let prophet = |resonance: f32| {
            let mut setup = neutral();
            setup.extend([(P_OSC1_SHAPE, 0.5), (P_LP_CUTOFF, 1.0), (P_LP_RESO, resonance)]);
            let mut s = built(&setup);
            let out = render(&mut s, &[note_on(LOW_NOTE, 100, 0)], 80);
            low_band(&out[out.len() / 4..], BAND_HZ, SR)
        };

        let moog = |resonance: f32| {
            let mut s = LittlePhatty::new();
            s.init(SR, BLOCK);
            for (index, value) in [
                (phatty::P_O1_WAVE, 1.0 / 3.0),
                (phatty::P_O1_LEVEL, 1.0),
                (phatty::P_O2_LEVEL, 0.0),
                (phatty::P_O1_OCT, 0.375),
                (phatty::P_CUTOFF, 1.0),
                (phatty::P_RESO, resonance),
                (phatty::P_EG_AMT, 0.0),
                (phatty::P_KB_AMT, 0.0),
                (phatty::P_OVERLOAD, 0.0),
                (phatty::P_POLES, 0.875),
                (phatty::P_VEL_SENS, 0.5),
                (phatty::P_MOD_AMT, 0.0),
                (phatty::P_GLIDE, 0.0),
                (phatty::P_V_ATTACK, 0.0),
                (phatty::P_V_DECAY, 1.0),
                (phatty::P_V_SUSTAIN, 1.0),
                (phatty::P_VOLUME, 1.0),
            ] {
                s.set_parameter(index, value);
            }
            s.reset();
            let mut left = vec![0.0f32; BLOCK];
            let mut out = Vec::new();
            for block in 0..80 {
                left.fill(0.0);
                let mut outs: [&mut [f32]; 1] = [&mut left];
                let events = [note_on(LOW_NOTE, 100, 0)];
                s.process(&[], &mut outs, if block == 0 { &events } else { &[] });
                out.extend_from_slice(&left);
            }
            low_band(&out[out.len() / 4..], BAND_HZ, SR)
        };

        (db(prophet(0.0) / prophet(1.0)), db(moog(0.0) / moog(1.0)))
    }

    /// The difference between an SSM2040-lineage filter and a Moog ladder,
    /// measured end to end against the ladder in the rack.
    ///
    /// A transistor ladder subtracts its resonance feedback from the signal at
    /// the input of the first stage with nothing to make it up, so its
    /// passband gain is `1/(1+k)` and a resonant bass patch loses its bass.
    /// The SSM2040's summing stage is compensated. This is the single audible
    /// difference between the two designs and the reason the Prophet's filter
    /// is written out rather than borrowed from `phatty.rs`, so it is held
    /// open by measurement rather than by comment.
    #[test]
    fn resonance_keeps_its_bass_where_a_ladder_loses_it() {
        let (ssm_loss, ladder_loss) = passband_loss();
        assert!(
            ladder_loss > 8.0,
            "the ladder lost only {ladder_loss:.1} dB of bass at full resonance, so this \
             comparison is not measuring what it claims to"
        );
        assert!(
            ssm_loss < 3.0,
            "the Prophet's filter lost {ssm_loss:.1} dB of bass at full resonance, which is \
             ladder behaviour rather than SSM2040 behaviour"
        );
        assert!(
            ladder_loss - ssm_loss > 6.0,
            "the two filters are {:.1} dB apart on passband loss at full resonance \
             (ladder {ladder_loss:.1} dB, Prophet {ssm_loss:.1} dB), which is close enough \
             that they would sound like the same filter",
            ladder_loss - ssm_loss
        );
    }

    #[test]
    fn the_filter_envelope_is_bipolar_and_reaches_both_filters() {
        // "This control is bipolar. Positive settings produce standard
        // behavior... Negative settings invert the envelope."
        let brightness_of = |index: usize, amount: f32| {
            let mut setup = neutral();
            setup.extend([
                (P_OSC1_SHAPE, 0.5),
                (P_LP_CUTOFF, 60.0 / 164.0),
                (P_HP_CUTOFF, 60.0 / 164.0),
                (P_F_ATTACK, 0.0),
                (P_F_DECAY, 1.0),
                (P_F_SUSTAIN, 1.0),
                (index, amount),
            ]);
            let mut s = built(&setup);
            render(&mut s, &[note_on(45, 100, 0)], 10);
            let out = render(&mut s, &[], 40);
            (brightness(&out, SR), rms(&out))
        };
        let (flat_lp, _) = brightness_of(P_LP_ENV, 0.5);
        let (up_lp, _) = brightness_of(P_LP_ENV, 1.0);
        let (down_lp, _) = brightness_of(P_LP_ENV, 0.0);
        assert!(up_lp > flat_lp * 1.5, "a positive low-pass envelope did not open the filter");
        assert!(down_lp < flat_lp, "a negative low-pass envelope did not close the filter");

        // The high-pass amount takes the bottom out rather than adding top,
        // so it shows as level rather than as brightness.
        let (_, flat_hp) = brightness_of(P_HP_ENV, 0.5);
        let (_, up_hp) = brightness_of(P_HP_ENV, 1.0);
        assert!(
            up_hp < flat_hp * 0.7,
            "a positive high-pass envelope did not raise the high-pass cutoff"
        );
    }

    #[test]
    fn keyboard_tracking_is_off_half_and_full() {
        // "Setting keyboard to full when the filter is self oscillating will
        // cause the filter-generated pitch to follow the keyboard in tune
        // (i.e. in semitones). Setting the keyboard to half will cause the
        // filter-generated pitch to follow the keyboard pitch in quarter
        // tones."
        let pitch = |tracking: usize, note: u8| {
            let mut setup = neutral();
            setup.extend([
                (P_OSC1_LEVEL, 0.0),
                (P_LP_CUTOFF, 60.0 / 164.0),
                (P_LP_RESO, 1.0),
                (P_LP_KEY, knob_for(tracking, 3)),
            ]);
            let mut s = built(&setup);
            render(&mut s, &[note_on(note, 100, 0)], 20);
            let out = render(&mut s, &[], 40);
            fundamental_hz(&out, SR, 60.0, 4_000.0)
        };
        for (tracking, octaves) in [(0usize, 0.0f64), (1, 0.5), (2, 1.0)] {
            let low = pitch(tracking, 48);
            let high = pitch(tracking, 60);
            let measured = (high / low).log2();
            assert!(
                (measured - octaves).abs() < 0.08,
                "with keyboard tracking at position {tracking}, an octave on the keyboard \
                 moved the self-oscillation by {measured:.2} octaves rather than {octaves}"
            );
        }
    }

    // ── Poly mod ──

    #[test]
    fn poly_mod_reaches_every_destination() {
        // Five destinations, each on its own, each measured by the thing it
        // is supposed to change. The source is the filter envelope, so the
        // change is a sweep rather than a sideband and can be seen by
        // comparing the start of the note with the end of it.
        fn sweep(destination: usize, amount: f32) -> Vec<f32> {
            let mut setup = neutral();
            setup.extend([
                // In the pulse half of the morph, because pulse width is one
                // of the destinations and it does nothing on a triangle or a
                // sawtooth: "When Oscillator 1 is set to pulse wave, choosing
                // this as a destination modulates its pulse width."
                (P_OSC1_SHAPE, 0.85),
                (P_OSC1_PW, 0.5),
                (P_LP_CUTOFF, 100.0 / 164.0),
                (P_HP_CUTOFF, 40.0 / 164.0),
                (P_F_ATTACK, 0.0),
                (P_F_DECAY, 80.0 / 127.0),
                (P_F_SUSTAIN, 0.0),
                (P_PM_FILTER_ENV, amount),
                (destination, 1.0),
            ]);
            let mut s = built(&setup);
            render(&mut s, &[note_on(45, 100, 0)], 120)
        }
        for destination in [P_PM_FREQ1, P_PM_SHAPE1, P_PM_PW1, P_PM_LP, P_PM_HP] {
            let flat = sweep(destination, 0.5);
            let modulated = sweep(destination, 1.0);
            let same = flat
                .iter()
                .zip(&modulated)
                .all(|(a, b)| (a - b).abs() < 1.0e-6);
            assert!(!same, "poly mod destination {} did nothing", PARAM_NAMES[destination]);
        }
    }

    #[test]
    fn oscillator_2_into_the_low_pass_is_filter_modulation_at_audio_rate() {
        // "Use Poly Mod to create complex harmonic effects ranging from FM
        // (frequency modulation) to audio-rate filter modulation and beyond."
        // Oscillator 2 at audio rate into the low-pass puts sidebands around
        // everything the filter passes, which shows as harmonics that are not
        // multiples of the note.
        let play = |amount: f32| {
            let mut setup = neutral();
            setup.extend([
                (P_OSC1_SHAPE, 0.0),
                (P_OSC2_SHAPE, 0.0),
                (P_OSC2_LEVEL, 0.0),
                (P_OSC2_KEY, 0.0),
                (P_OSC2_FREQ, 45.0 / 60.0),
                (P_LP_CUTOFF, 70.0 / 164.0),
                (P_LP_RESO, 0.7),
                (P_PM_OSC2, amount),
                (P_PM_LP, 1.0),
            ]);
            let mut s = built(&setup);
            render(&mut s, &[note_on(45, 100, 0)], 10);
            render(&mut s, &[], 60)
        };
        let clean = play(0.5);
        let modulated = play(1.0);
        // The modulator's own frequency, which is not a harmonic of the note.
        let modulator = raw::note_hz(OSC2_NO_KEY_NOTE + raw::osc_semitones(45.0));
        let note = raw::note_hz(45.0);
        let side = |x: &[f32]| harmonic(x, note + modulator, SR) / harmonic(x, note, SR).max(1e-30);
        let before = side(&clean);
        let after = side(&modulated);
        assert!(
            after > before * 4.0 && after > 0.01,
            "audio-rate filter modulation produced no sideband: {before:.4} then {after:.4}"
        );
    }

    // ── The LFO ──

    #[test]
    fn the_lfo_shapes_have_the_polarity_the_manual_gives() {
        // "Triangle and Random waves are bipolar... The square wave,
        // sawtooth, and reverse sawtooth generate only positive values."
        // Measured on the LFO itself rather than through the instrument,
        // because it is a property of the waveform.
        for (shape, shape_name) in LFO_SHAPES.iter().enumerate() {
            let mut lfo = Lfo::new();
            let mut low = 0.0f64;
            let mut high = 0.0f64;
            for _ in 0..40_000 {
                let v = lfo.tick(7.0, SR, shape, false);
                low = low.min(v);
                high = high.max(v);
            }
            let bipolar = matches!(shape, 0 | 4);
            if bipolar {
                assert!(
                    low < -0.8 && high > 0.8,
                    "{shape_name} should be bipolar and runs {low:.2} to {high:.2}"
                );
            } else {
                assert!(
                    low >= -0.02 && high > 0.8,
                    "{shape_name} should be positive only and runs {low:.2} to {high:.2}"
                );
            }
        }
    }

    #[test]
    fn the_hidden_sixth_lfo_shape_is_noise() {
        // "The Prophet-6 has a sixth 'hidden' LFO waveshape that you can use
        // as a modulation source — noise. To access this, choose random then
        // turn frequency all the way clockwise."
        let params = {
            let mut s = built(&neutral());
            s.set_parameter(P_LFO_SHAPE, knob_for(4, LFO_SHAPES.len()));
            s.set_parameter(P_LFO_FREQ, 1.0);
            s.params
        };
        assert!(Panel::read(&params, SR).lfo_noise, "random at the top of the knob is not noise");
        let mut params = params;
        params[P_LFO_FREQ] = 0.9;
        assert!(!Panel::read(&params, SR).lfo_noise, "random below the top of the knob is noise");
    }

    #[test]
    fn the_gated_vca_recipe_from_the_manual_works() {
        // "To recreate the 'gated VCA' effect used on certain classic rock
        // anthems, choose an organ sound, then set the vca env amount to
        // zero, route the LFO square wave to amp with an initial amt setting
        // of 100% and hold a few chords." Three of the 500 factory programs
        // are built this way, and they are what fixed the LFO destination
        // block's byte order — see `raw_offset`.
        let mut setup = neutral();
        setup.extend([
            (P_OSC1_SHAPE, 0.5),
            (P_VCA_ENV, 0.0),
            (P_LFO_SHAPE, knob_for(3, LFO_SHAPES.len())),
            (P_LFO_AMOUNT, 1.0),
            (P_LFO_FREQ, 120.0 / 255.0),
        ]);
        let mut closed = built(&setup);
        let silent = render(&mut closed, &[note_on(60, 100, 0)], 100);
        assert_eq!(peak(&silent), 0.0, "the VCA sounded with its envelope amount at zero");

        setup.push((P_LFO_AMP, 1.0));
        let mut gated = built(&setup);
        let out = render(&mut gated, &[note_on(60, 100, 0)], 100);
        let windows = window_rms(&out);
        let loudest = windows.iter().copied().fold(0.0f64, f64::max);
        let quietest = windows.iter().copied().fold(f64::MAX, f64::min);
        assert!(loudest > 0.001, "routing the LFO to amp did not open the VCA");
        assert!(
            quietest < loudest * 0.2,
            "the gate never closed: {quietest:.5} against {loudest:.5}"
        );
    }

    #[test]
    fn the_lfo_reaches_every_destination() {
        fn play(destination: usize, depth: f32) -> Vec<f32> {
            let mut setup = neutral();
            setup.extend([
                (P_OSC1_SHAPE, 0.75),
                (P_OSC2_LEVEL, 0.5),
                (P_LP_CUTOFF, 80.0 / 164.0),
                (P_HP_CUTOFF, 40.0 / 164.0),
                (P_LFO_FREQ, 120.0 / 255.0),
                (P_LFO_AMOUNT, depth),
                (destination, 1.0),
            ]);
            let mut s = built(&setup);
            render(&mut s, &[note_on(45, 100, 0)], 120)
        }
        for destination in [P_LFO_FREQ1, P_LFO_FREQ2, P_LFO_PW, P_LFO_AMP, P_LFO_LP, P_LFO_HP] {
            let flat = play(destination, 0.0);
            let modulated = play(destination, 1.0);
            let same = flat.iter().zip(&modulated).all(|(a, b)| (a - b).abs() < 1.0e-6);
            assert!(!same, "LFO destination {} did nothing", PARAM_NAMES[destination]);
        }
    }

    #[test]
    fn the_mod_wheel_adds_to_the_initial_amount_and_does_not_replace_it() {
        // "If you set this parameter to zero but still select a modulation
        // destination, modulation is only applied when you use the Mod
        // Wheel." So the wheel rests at zero and adds; a program that stores
        // an initial amount does not need it.
        let mut setup = neutral();
        setup.extend([
            (P_OSC1_SHAPE, 0.5),
            (P_LFO_FREQ1, 1.0),
            (P_LFO_FREQ, 150.0 / 255.0),
            (P_LFO_AMOUNT, 0.0),
        ]);
        let mut s = built(&setup);
        let still = render(&mut s, &[note_on(60, 100, 0)], 60);
        let moved = render(&mut s, &[cc(1, 127, 0)], 60);
        let steady = fundamental_hz(&still[still.len() / 2..], SR, 100.0, 1_000.0);
        let spread: Vec<f64> = moved
            .chunks(2_048)
            .map(|c| fundamental_hz(c, SR, 100.0, 1_000.0))
            .collect();
        let widest = spread.iter().copied().fold(0.0f64, f64::max);
        let narrowest = spread.iter().copied().fold(f64::MAX, f64::min);
        assert!(
            widest / narrowest > 1.05,
            "the mod wheel did not add vibrato: {narrowest:.1} to {widest:.1} Hz"
        );
        assert!(steady > 0.0);
    }

    // ── Aftertouch ──

    #[test]
    fn channel_pressure_reaches_every_destination() {
        // The Prophet-6 provides channel aftertouch, and the six destinations
        // its panel offers are each checked here. That the message reaches a
        // plugin at all is `phosphor-core`'s half, and is held down by
        // `channel_pressure_reaches_the_plugin_and_key_pressure_does_not`.
        fn play(destination: usize, pressure: u8) -> Vec<f32> {
            let mut setup = neutral();
            setup.extend([
                (P_OSC1_SHAPE, 0.75),
                (P_OSC2_LEVEL, 0.5),
                (P_LP_CUTOFF, 80.0 / 164.0),
                (P_HP_CUTOFF, 40.0 / 164.0),
                (P_VCA_ENV, 0.5),
                (P_LFO_FREQ1, 1.0),
                (P_LFO_FREQ, 150.0 / 255.0),
                (P_AT_AMOUNT, 1.0),
                (destination, 1.0),
            ]);
            let mut s = built(&setup);
            render(&mut s, &[note_on(45, 100, 0)], 20);
            render(&mut s, &[aftertouch(pressure, 0)], 60)
        }
        for destination in [P_AT_FREQ1, P_AT_FREQ2, P_AT_LFO, P_AT_AMP, P_AT_LP, P_AT_HP] {
            let released = play(destination, 0);
            let pressed = play(destination, 127);
            let same = released.iter().zip(&pressed).all(|(a, b)| (a - b).abs() < 1.0e-6);
            assert!(!same, "aftertouch destination {} did nothing", PARAM_NAMES[destination]);
        }
    }

    #[test]
    fn the_pitch_wheel_bends_by_the_range_it_names() {
        for semitones in [2usize, 7, 12] {
            let mut setup = neutral();
            setup.extend([
                (P_OSC1_SHAPE, 0.5),
                (P_BEND_RANGE, knob_for(semitones, BEND_RANGE_MAX + 1)),
            ]);
            let mut s = built(&setup);
            let flat = render(&mut s, &[note_on(45, 100, 0)], 40);
            let bent = render(
                &mut s,
                &[MidiEvent { sample_offset: 0, status: 0xE0, data1: 127, data2: 127 }],
                40,
            );
            let a = fundamental_hz(&flat[flat.len() / 2..], SR, 60.0, 2_000.0);
            let b = fundamental_hz(&bent[bent.len() / 2..], SR, 60.0, 2_000.0);
            let measured = 12.0 * (b / a).log2();
            assert!(
                (measured - semitones as f64).abs() < 0.15,
                "a full bend with the range at {semitones} moved the pitch {measured:.2} \
                 semitones"
            );
        }
    }

    // ── Voice management ──

    fn sounding(s: &Prophet6) -> Vec<u8> {
        let mut notes: Vec<u8> = s.voices.iter().filter(|v| v.gate).map(|v| v.note).collect();
        notes.sort_unstable();
        notes
    }

    #[test]
    fn six_voices_and_the_seventh_note_steals_the_oldest() {
        let mut s = built(&neutral());
        let notes: [u8; 6] = [48, 52, 55, 59, 62, 65];
        let events: Vec<MidiEvent> =
            notes.iter().map(|n| note_on(*n, 100, 0)).collect();
        render(&mut s, &events, 4);
        assert_eq!(sounding(&s), notes.to_vec(), "six notes did not fill six voices");

        render(&mut s, &[note_on(69, 100, 0)], 4);
        let after = sounding(&s);
        assert_eq!(after.len(), VOICES, "a seventh note grew the instrument");
        assert!(after.contains(&69), "the seventh note did not sound");
        assert!(!after.contains(&48), "the seventh note stole something other than the oldest");
    }

    #[test]
    fn a_released_voice_is_taken_before_a_held_one() {
        let mut s = built(&neutral());
        // Six held, then one released — the released one is the newest, so an
        // allocator that only looked at age would steal a key that is down.
        let notes: [u8; 6] = [48, 52, 55, 59, 62, 65];
        render(&mut s, &notes.iter().map(|n| note_on(*n, 100, 0)).collect::<Vec<_>>(), 4);
        render(&mut s, &[note_off(65, 0)], 2);
        render(&mut s, &[note_on(72, 100, 0)], 2);
        let after = sounding(&s);
        assert!(after.contains(&72), "the new note did not sound");
        for held in [48u8, 52, 55, 59, 62] {
            assert!(after.contains(&held), "note {held} was stolen while its key was down");
        }
    }

    #[test]
    fn unison_stacks_the_number_of_voices_it_names() {
        for voices in 1..=VOICES {
            let mut setup = neutral();
            setup.extend([
                (P_UNISON, 1.0),
                (P_UNISON_MODE, knob_for(voices - 1, UNISON_MODES.len())),
            ]);
            let mut s = built(&setup);
            render(&mut s, &[note_on(48, 100, 0)], 4);
            let gated = s.voices.iter().filter(|v| v.gate).count();
            assert_eq!(gated, voices, "the {voices}-voice stack sounded {gated} voices");
        }
    }

    #[test]
    fn a_unison_stack_is_louder_than_one_voice() {
        // Free-running phases, so the stack sums incoherently — which is what
        // makes it thick rather than merely loud, and is why nothing here
        // resets an oscillator's phase on note-on. Six uncorrelated sources
        // sum to √6, and the starting phases are chosen so that they do; see
        // [`start_phase`] for what happens when they are spread evenly
        // instead.
        let level = |voices: usize| {
            let mut setup = neutral();
            setup.extend([
                (P_OSC1_SHAPE, 0.5),
                (P_UNISON, 1.0),
                (P_UNISON_MODE, knob_for(voices - 1, UNISON_MODES.len())),
            ]);
            let mut s = built(&setup);
            rms(&render(&mut s, &[note_on(45, 100, 0)], 60))
        };
        let one = level(1);
        let six = level(6);
        let ratio = six / one;
        assert!(
            (1.8..3.4).contains(&ratio),
            "six voices are {ratio:.2} times one, and six uncorrelated sources are 2.45"
        );
    }

    #[test]
    fn chord_memory_round_trips() {
        // The hardware gesture is "hold down a chord on the keyboard, press
        // the unison switch"; the rack's equivalent is stepping the voice
        // selector onto CHD with keys down.
        let mut setup = neutral();
        setup.push((P_UNISON, 1.0));
        let mut s = built(&setup);
        render(&mut s, &[note_on(60, 100, 0), note_on(64, 100, 1), note_on(67, 100, 2)], 4);
        assert!(s.chord_memory().is_empty(), "a chord was memorised before it was asked for");

        s.set_parameter(P_UNISON_MODE, knob_for(6, UNISON_MODES.len()));
        assert_eq!(s.chord_memory(), &[0, 4, 7], "a major triad did not round-trip");

        // A single note now plays the whole voicing, transposed.
        render(&mut s, &[note_off(60, 0), note_off(64, 1), note_off(67, 2)], 4);
        render(&mut s, &[note_on(55, 100, 0)], 4);
        // Chord memory stacks all six voices over the stored voicing, so a
        // three-note chord gets two voices a note.
        let notes = sounding(&s);
        assert_eq!(notes.len(), VOICES, "chord memory did not use the whole instrument");
        let mut distinct = notes.clone();
        distinct.dedup();
        assert_eq!(distinct, vec![55, 59, 62], "the memorised chord did not transpose");

        // And holding a single note while stepping onto CHD clears it back to
        // a plain stack, which is the manual's own way of clearing it.
        s.set_parameter(P_UNISON_MODE, knob_for(0, UNISON_MODES.len()));
        render(&mut s, &[note_off(55, 0), note_on(48, 100, 1)], 4);
        s.set_parameter(P_UNISON_MODE, knob_for(6, UNISON_MODES.len()));
        assert_eq!(s.chord_memory(), &[0], "a single held note did not clear the chord memory");
    }

    #[test]
    fn key_priority_applies_in_unison_and_not_in_poly() {
        // "Key Assign settings are only relevant to Unison mode. They do not
        // affect polyphonic playback."
        let play = |unison: f32, key_mode: usize| {
            let mut setup = neutral();
            setup.extend([
                (P_UNISON, unison),
                (P_UNISON_MODE, knob_for(2, UNISON_MODES.len())),
                (P_KEY_MODE, knob_for(key_mode, KEY_MODES.len())),
            ]);
            let mut s = built(&setup);
            render(&mut s, &[note_on(60, 100, 0), note_on(72, 100, 8)], 4);
            sounding(&s)
        };
        assert_eq!(play(1.0, 0), vec![60, 60, 60], "low-note priority did not pick the low note");
        assert_eq!(play(1.0, 1), vec![72, 72, 72], "high-note priority did not pick the high note");
        assert_eq!(play(1.0, 2), vec![72, 72, 72], "last-note priority did not pick the last note");
        assert_eq!(play(0.0, 0), vec![60, 72], "key priority changed polyphonic playback");
        assert_eq!(play(0.0, 1), vec![60, 72], "key priority changed polyphonic playback");
    }

    #[test]
    fn letting_go_of_the_winner_hands_the_stack_to_the_key_still_down() {
        let mut setup = neutral();
        setup.extend([
            (P_UNISON, 1.0),
            (P_UNISON_MODE, knob_for(2, UNISON_MODES.len())),
            (P_KEY_MODE, knob_for(0, KEY_MODES.len())),
        ]);
        let mut s = built(&setup);
        render(&mut s, &[note_on(48, 100, 0), note_on(60, 100, 8)], 4);
        assert_eq!(sounding(&s), vec![48, 48, 48]);
        render(&mut s, &[note_off(48, 0)], 4);
        assert_eq!(sounding(&s), vec![60, 60, 60], "the stack did not return to the held key");
    }

    // ── Slop ──

    #[test]
    fn no_two_oscillators_share_a_random_stream() {
        // The Jupiter shipped once with every voice sharing a noise seed, and
        // six coherent copies of the same sequence summed to 18 dB instead of
        // 8. Nothing in this instrument may share randomness: every stream it
        // owns — two slop walks and one noise generator per voice — is
        // correlated against every other over a hundred thousand draws, which
        // is enough that a correlation of 0.01 is already significant.
        //
        // Correlated against each other *raw*, not through the one-pole the
        // slop walk runs them through: a 0.7 Hz filter leaves about one
        // independent value a second, so correlating a second of filtered
        // walk is correlating one number against one number and says nothing.
        const DRAWS: usize = 100_000;
        let mut streams: Vec<Vec<f64>> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        for index in 0..VOICES {
            let mut voice = Voice::new(index, SR);
            for (slot, name) in ["slop 1", "slop 2", "noise"].iter().enumerate() {
                let mut draws = Vec::with_capacity(DRAWS);
                for _ in 0..DRAWS {
                    draws.push(match slot {
                        0 => voice.slop1.noise.tick(),
                        1 => voice.slop2.noise.tick(),
                        _ => voice.noise.tick(),
                    });
                }
                streams.push(draws);
                names.push(format!("voice {index} {name}"));
            }
        }
        for a in 0..streams.len() {
            for b in a + 1..streams.len() {
                let n = DRAWS as f64;
                let ma = streams[a].iter().sum::<f64>() / n;
                let mb = streams[b].iter().sum::<f64>() / n;
                let (mut num, mut va, mut vb) = (0.0, 0.0, 0.0);
                for (x, y) in streams[a].iter().zip(&streams[b]) {
                    num += (x - ma) * (y - mb);
                    va += (x - ma) * (x - ma);
                    vb += (y - mb) * (y - mb);
                }
                let r = num / (va * vb).sqrt().max(1.0e-30);
                assert!(
                    r.abs() < 0.02,
                    "{} and {} correlate at {r:.4} — they share a stream",
                    names[a], names[b]
                );
            }
        }
    }

    #[test]
    fn slop_detunes_and_the_knob_says_how_far() {
        // "Slop amount is adjustable from subtle, barely perceptible amounts
        // to wildly out of tune."
        let spread = |amount: f32| {
            let mut setup = neutral();
            setup.extend([(P_OSC1_SHAPE, 0.0), (P_SLOP, amount)]);
            let mut s = built(&setup);
            let out = render(&mut s, &[note_on(57, 100, 0)], 400);
            let pitches: Vec<f64> = out
                .chunks(16_384)
                .filter(|c| c.len() == 16_384)
                .map(|c| fundamental_hz(c, SR, 150.0, 800.0))
                .collect();
            let high = pitches.iter().copied().fold(0.0f64, f64::max);
            let low = pitches.iter().copied().fold(f64::MAX, f64::min);
            1_200.0 * (high / low).log2()
        };
        let none = spread(0.0);
        let lots = spread(1.0);
        assert!(none < 8.0, "with slop at zero the pitch still wandered {none:.1} cents");
        assert!(lots > 30.0, "with slop at full the pitch wandered only {lots:.1} cents");
    }

    // ── Distortion ──

    #[test]
    fn the_distortion_knob_is_the_identity_at_zero() {
        for level in [-2.0f64, -0.5, 0.0, 0.25, 1.0, 6.0] {
            assert_eq!(distort(level, 0.0), level, "the distortion curve moved {level} at zero");
        }
    }

    #[test]
    fn distortion_adds_harmonics_and_bounds_the_output() {
        // A triangle, because its spectrum is the easiest to hear a shaper
        // in: no even harmonics at all and a third at a ninth of the
        // fundamental. "The character of the distortion is affected by the
        // harmonic content of a program."
        let play = |amount: f32| {
            let mut setup = neutral();
            setup.extend([(P_OSC1_SHAPE, 0.0), (P_DISTORTION, amount)]);
            let mut s = built(&setup);
            render(&mut s, &[note_on(45, 100, 0)], 10);
            let out = render(&mut s, &[], 60);
            (harmonics(&out, raw::note_hz(45.0), SR, 3), rms(&out))
        };
        let (clean, quiet) = play(0.0);
        let (dirty, loud) = play(1.0);
        assert!(clean[1] < 0.05, "the undistorted triangle already has even harmonics: {clean:?}");
        assert!(
            dirty[1] > 0.03 && dirty[1] > clean[1] * 3.0,
            "the distortion is symmetric — no even harmonics appeared: {clean:?} then {dirty:?}"
        );
        assert!(
            dirty[2] > clean[2] * 1.4,
            "the distortion knob added no odd harmonics: {clean:?} then {dirty:?}"
        );
        // One voice sits well under the clipper's rails, so the knob is gain
        // as well as grit there. A six-voice stack already sits at the rails
        // and the knob costs it a decibel or two of level instead, which is
        // what a clipper does — see `DIST_RAIL`.
        assert!(loud > quiet, "the distortion knob made a single voice quieter");

        // Whatever the voices hand it, the curve is bounded — which is what
        // makes a six-voice unison stack through it safe rather than merely
        // trimmed.
        for input in [0.5f64, 1.0, 6.0, 60.0, 600.0] {
            let out = distort(input, 1.0);
            assert!(
                out < DIST_RAIL * 1.1,
                "the distortion passed {input} through as {out}, past its own rails"
            );
        }
    }

    // ── Effects ──

    #[test]
    fn the_delay_repeats_at_the_time_it_names() {
        for (kind, name) in [(fx::DDL, "ddl"), (fx::BBD, "bbd")] {
            let mut setup = neutral();
            setup.extend([
                (P_OSC1_SHAPE, 0.5),
                (P_A_DECAY, 20.0 / 127.0),
                (P_A_SUSTAIN, 0.0),
                (P_FX_ON, 1.0),
                (P_FXA_TYPE, knob_for(kind, FX_A_TYPES.len())),
                (P_FXA_MIX, 1.0),
                (P_FXA_P2, 0.5),
                (P_FXB_TYPE, knob_for(fx::OFF, FX_B_TYPES.len())),
            ]);
            // A quarter of a second, geometrically, out of the knob's range.
            let target = 0.25f64;
            let knob = (target / DELAY_MIN_S).log10() / (DELAY_MAX_S / DELAY_MIN_S).log10();
            setup.push((P_FXA_P1, (knob * 255.0 / 255.0) as f32));
            let mut s = built(&setup);
            let out = render(&mut s, &[note_on(45, 100, 0)], 200);
            let window = 512;
            let envelope: Vec<f64> = out.chunks(window).map(rms).collect();
            let top = envelope.iter().copied().fold(0.0f64, f64::max);
            let mut hits: Vec<usize> = Vec::new();
            let mut above = false;
            for (i, level) in envelope.iter().enumerate() {
                if *level > top * 0.15 && !above {
                    hits.push(i);
                    above = true;
                } else if *level < top * 0.05 {
                    above = false;
                }
            }
            assert!(hits.len() >= 3, "{name} produced {} repeats", hits.len());
            let gap = (hits[hits.len() - 1] - hits[0]) as f64 * window as f64
                / SR
                / (hits.len() - 1) as f64;
            assert!(
                (gap - target).abs() / target < 0.1,
                "{name} repeated every {gap:.3} s, and the knob asks for {target:.3}"
            );
        }
    }

    #[test]
    fn clock_sync_divides_the_tempo() {
        // "When a delay effect is chosen, this enables syncing of the timed
        // delay repeats to the Arpeggiator, Sequencer, or MIDI clock."
        let mut params = built(&neutral()).params;
        params[P_FXA_TYPE] = knob_for(fx::DDL, FX_A_TYPES.len());
        params[P_FXA_SYNC] = 1.0;
        params[P_BPM] = 120.0 / 250.0;
        for (division, beats) in [(5usize, 1.0f64), (7, 0.5), (10, 0.25)] {
            params[P_FXA_DIV] = knob_for(division, SYNC_DIVISIONS.len());
            let panel = Panel::read(&params, SR);
            let expected = beats * 60.0 / 120.0;
            assert!(
                (panel.fx_a.p1 - expected).abs() / expected < 1.0e-4,
                "{} at 120 bpm is {:.4} s, expected {expected:.4}",
                SYNC_DIVISIONS[division],
                panel.fx_a.p1
            );
        }
        // "Maximum delay time is 1 second... the delay time is divided by 2
        // until it no longer exceeds the 1 second limit."
        params[P_BPM] = 60.0 / 250.0;
        params[P_FXA_DIV] = knob_for(0, SYNC_DIVISIONS.len()); // four beats
        let panel = Panel::read(&params, SR);
        assert!(
            panel.fx_a.p1 <= DELAY_MAX_S && panel.fx_a.p1 > DELAY_MAX_S * 0.5,
            "four beats at 60 bpm came out as {:.3} s",
            panel.fx_a.p1
        );
    }

    #[test]
    fn the_chorus_widens_and_the_unrendered_effects_pass_through() {
        let render_stereo = |kind: usize| {
            let mut setup = neutral();
            setup.extend([
                (P_OSC1_SHAPE, 0.5),
                (P_FX_ON, 1.0),
                (P_FXA_TYPE, knob_for(kind, FX_A_TYPES.len())),
                (P_FXA_MIX, 1.0),
                (P_FXA_P1, 0.3),
                (P_FXA_P2, 1.0),
                (P_FXB_TYPE, knob_for(fx::OFF, FX_B_TYPES.len())),
            ]);
            let mut s = built(&setup);
            let mut left = vec![0.0f32; BLOCK];
            let mut right = vec![0.0f32; BLOCK];
            let (mut l, mut r) = (Vec::new(), Vec::new());
            for block in 0..120 {
                left.fill(0.0);
                right.fill(0.0);
                let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
                let events = [note_on(45, 100, 0)];
                s.process(&[], &mut outs, if block == 0 { &events } else { &[] });
                l.extend_from_slice(&left);
                r.extend_from_slice(&right);
            }
            (l, r)
        };
        let (l, r) = render_stereo(fx::CHORUS);
        let difference = rms(&l.iter().zip(&r).map(|(a, b)| a - b).collect::<Vec<f32>>());
        assert!(
            difference > rms(&l) * 0.1,
            "the chorus produced the same signal on both sides"
        );

        // The two phasers and the four reverbs are stored and selectable and
        // render dry — see the `fx` module. A program that asks for one of
        // them keeps its settings and sounds like the voice.
        let (dry_l, _) = render_stereo(fx::OFF);
        for kind in [4usize, 5] {
            let (l, _) = render_stereo(kind);
            assert_eq!(
                l, dry_l,
                "effect {} is rendering something, which the module docs say it is not",
                FX_A_TYPES[kind]
            );
        }
    }

    // ── The panel ──

    #[test]
    fn the_panel_is_in_front_panel_order() {
        // Every index used exactly once, no gaps, and the sections in the
        // order the instrument's panel puts them.
        let indices = [
            P_PROGRAM, P_BANK,
            P_OSC1_FREQ, P_OSC1_SHAPE, P_OSC1_PW, P_SYNC,
            P_OSC2_FREQ, P_OSC2_FINE, P_OSC2_SHAPE, P_OSC2_PW, P_OSC2_LOW, P_OSC2_KEY,
            P_SLOP,
            P_OSC1_LEVEL, P_OSC2_LEVEL, P_SUB_LEVEL, P_NOISE_LEVEL,
            P_HP_CUTOFF, P_HP_RESO, P_HP_ENV, P_HP_VEL, P_HP_KEY,
            P_LP_CUTOFF, P_LP_RESO, P_LP_ENV, P_LP_VEL, P_LP_KEY,
            P_F_ATTACK, P_F_DECAY, P_F_SUSTAIN, P_F_RELEASE,
            P_VCA_ENV, P_VCA_VEL, P_A_ATTACK, P_A_DECAY, P_A_SUSTAIN, P_A_RELEASE,
            P_LFO_FREQ, P_LFO_SHAPE, P_LFO_AMOUNT, P_LFO_FREQ1, P_LFO_FREQ2, P_LFO_PW,
            P_LFO_AMP, P_LFO_LP, P_LFO_HP,
            P_PM_FILTER_ENV, P_PM_OSC2, P_PM_FREQ1, P_PM_SHAPE1, P_PM_PW1, P_PM_LP, P_PM_HP,
            P_AT_AMOUNT, P_AT_FREQ1, P_AT_FREQ2, P_AT_LFO, P_AT_AMP, P_AT_LP, P_AT_HP,
            P_DISTORTION,
            P_FX_ON, P_FXA_TYPE, P_FXA_MIX, P_FXA_P1, P_FXA_P2, P_FXA_SYNC, P_FXA_DIV,
            P_FXB_TYPE, P_FXB_MIX, P_FXB_P1, P_FXB_P2, P_FXB_SYNC, P_FXB_DIV, P_BPM,
            P_UNISON, P_UNISON_MODE, P_KEY_MODE,
            P_GLIDE, P_GLIDE_MODE, P_GLIDE_RATE, P_BEND_RANGE,
            P_PAN_SPREAD, P_VOLUME,
        ];
        assert_eq!(indices.len(), PARAM_COUNT, "the panel list and PARAM_COUNT disagree");
        for (position, index) in indices.iter().enumerate() {
            assert_eq!(*index, position, "parameter {position} is out of panel order");
        }
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            assert!(!name.is_empty(), "parameter {index} has no name");
            assert!(name.chars().count() <= 8, "parameter name {name:?} overflows its column");
        }
        // Every engine control has a byte in the program block. The two
        // selectors do not, because they are where the program came from.
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            let expected = index != P_PROGRAM && index != P_BANK;
            assert_eq!(
                raw_offset(index).is_some(),
                expected,
                "{name} is {} a stored parameter",
                if expected { "not" } else { "unexpectedly" }
            );
        }
        // And no two controls read the same byte.
        let mut offsets: Vec<usize> = (0..PARAM_COUNT).filter_map(|i| raw_offset(i).map(|(o, _)| o)).collect();
        offsets.sort_unstable();
        let count = offsets.len();
        offsets.dedup();
        assert_eq!(offsets.len(), count, "two panel controls read the same program byte");
    }

    #[test]
    fn switches_step_one_position_per_press() {
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            let Some(count) = discrete_steps(index) else {
                assert_eq!(
                    step_discrete(index, 0.4, true),
                    0.4,
                    "{name} is a knob and moved when stepped"
                );
                continue;
            };
            let mut knob = 0.0;
            for position in 0..count {
                assert_eq!(
                    selector(knob, count),
                    position,
                    "{name} skipped a position stepping up"
                );
                knob = step_discrete(index, knob, true);
            }
            assert_eq!(selector(knob, count), count - 1, "{name} ran off the top");
            for position in (0..count).rev() {
                assert_eq!(
                    selector(knob, count),
                    position,
                    "{name} skipped a position stepping down"
                );
                knob = step_discrete(index, knob, false);
            }
            assert_eq!(selector(knob, count), 0, "{name} ran off the bottom");
        }
    }

    #[test]
    fn switch_labels_read_as_the_panel_does() {
        let mut params = [0.0f32; PARAM_COUNT];
        for index in 0..PARAM_COUNT {
            let Some(count) = discrete_steps(index) else {
                params[index] = 0.5;
                assert!(
                    discrete_label(&params, index).is_none(),
                    "{} is a knob with a label",
                    PARAM_NAMES[index]
                );
                continue;
            };
            for position in 0..count {
                params[index] = knob_for(position, count);
                let label = discrete_label(&params, index)
                    .unwrap_or_else(|| panic!("{} has no label", PARAM_NAMES[index]));
                assert!(!label.is_empty(), "{} position {position} has a blank label", PARAM_NAMES[index]);
                // Twelve columns is what the editor's parameter row leaves.
                assert!(
                    label.chars().count() <= 12,
                    "{} position {position} reads {label:?}, which does not fit",
                    PARAM_NAMES[index]
                );
            }
        }
        // The named enumerations, spot-checked against the manual's own words.
        params[P_LFO_SHAPE] = knob_for(0, LFO_SHAPES.len());
        assert_eq!(discrete_label(&params, P_LFO_SHAPE), Some("tri"));
        params[P_FXB_TYPE] = knob_for(9, FX_B_TYPES.len());
        assert_eq!(discrete_label(&params, P_FXB_TYPE), Some("SPr"));
        params[P_LP_KEY] = knob_for(1, 3);
        assert_eq!(discrete_label(&params, P_LP_KEY), Some("half"));
        params[P_UNISON_MODE] = knob_for(6, UNISON_MODES.len());
        assert_eq!(discrete_label(&params, P_UNISON_MODE), Some("chord"));
        params[P_GLIDE_MODE] = knob_for(3, GLIDE_MODES.len());
        assert_eq!(discrete_label(&params, P_GLIDE_MODE), Some("time A"));
    }

    #[test]
    fn the_program_knobs_land_on_the_program_they_name() {
        for index in 0..PROGRAM_COUNT {
            let (bank, program) = program_knobs(index);
            assert_eq!(program_index(bank, program), index, "program {index} is not reachable");
        }
        // Junk on either knob still lands on a real program.
        for junk in [-1.0f32, 2.0, f32::NAN] {
            assert!(program_index(junk, 0.5) < PROGRAM_COUNT);
            assert!(program_index(0.5, junk) < PROGRAM_COUNT);
        }
        // And walking the two selectors reaches all five hundred.
        let mut seen = vec![false; PROGRAM_COUNT];
        let mut bank = 0.0f32;
        for _ in 0..BANK_COUNT {
            let mut program = 0.0f32;
            for _ in 0..PROGRAMS_PER_BANK {
                seen[program_index(bank, program)] = true;
                program = step_discrete(P_PROGRAM, program, true);
            }
            bank = step_discrete(P_BANK, bank, true);
        }
        assert!(seen.iter().all(|v| *v), "the two selectors do not reach every program");
    }

    #[test]
    fn the_program_knob_loads_the_whole_panel() {
        for index in [0usize, 31, 191, 419, 499] {
            let mut s = fresh(index);
            let expected = params_for_program(s.params[P_BANK], s.params[P_PROGRAM]);
            assert_eq!(s.params, expected, "program {index} did not load its own panel");
            assert_eq!(s.current_program(), index);
            // Moving the bank knob reloads too, which is the thing that makes
            // two selectors one control.
            s.set_parameter(P_OSC1_LEVEL, 0.123);
            s.set_parameter(P_BANK, s.params[P_BANK]);
            assert_eq!(s.params, expected, "moving the bank knob did not reload the panel");
        }
    }

    #[test]
    fn every_engine_control_is_reachable() {
        // Every control moved between two positions on a panel where
        // everything is engaged — both oscillators up, both filters in
        // circuit, the pulse half of the morph, unison on, glide on, the mod
        // and pitch wheels moved, aftertouch applied, both effects on with
        // clock sync — so that nothing is inert for want of context.
        fn play(s: &mut Prophet6) -> Vec<f32> {
            let events = [
                note_on(45, 90, 0),
                cc(1, 100, 40),
                MidiEvent { sample_offset: 60, status: 0xE0, data1: 0, data2: 100 },
                aftertouch(90, 80),
            ];
            // Fourteen semitones, not twelve: the fixed-rate glide modes take
            // the knob's time for an octave and the fixed-time modes take it
            // for the whole interval, so a leap of exactly an octave makes
            // the two indistinguishable.
            let first = render(s, &events, 40);
            let second = render(s, &[note_on(59, 110, 0)], 40);
            let mut out = first;
            out.extend_from_slice(&second);
            out.extend_from_slice(&render(s, &[note_off(59, 0), note_off(45, 1)], 30));
            out
        }
        let rich = {
            let mut panel = neutral();
            panel.extend([
                (P_OSC1_SHAPE, 0.8),
                (P_OSC2_SHAPE, 0.8),
                (P_OSC2_LEVEL, 0.7),
                (P_SUB_LEVEL, 0.5),
                (P_NOISE_LEVEL, 0.3),
                (P_LP_CUTOFF, 90.0 / 164.0),
                (P_LP_RESO, 0.4),
                (P_LP_ENV, 0.8),
                (P_HP_CUTOFF, 40.0 / 164.0),
                (P_HP_RESO, 0.4),
                (P_HP_ENV, 0.6),
                (P_F_ATTACK, 0.2),
                (P_F_DECAY, 0.5),
                (P_F_SUSTAIN, 0.5),
                (P_F_RELEASE, 0.4),
                (P_A_ATTACK, 0.15),
                (P_A_DECAY, 0.5),
                (P_A_SUSTAIN, 0.6),
                (P_A_RELEASE, 0.4),
                (P_LFO_FREQ, 0.6),
                (P_LFO_AMOUNT, 0.5),
                (P_LFO_FREQ1, 1.0),
                (P_LFO_FREQ2, 1.0),
                (P_LFO_PW, 1.0),
                (P_LFO_AMP, 1.0),
                (P_LFO_LP, 1.0),
                (P_LFO_HP, 1.0),
                (P_PM_FILTER_ENV, 0.8),
                (P_PM_OSC2, 0.7),
                (P_PM_FREQ1, 1.0),
                (P_PM_SHAPE1, 1.0),
                (P_PM_PW1, 1.0),
                (P_PM_LP, 1.0),
                (P_PM_HP, 1.0),
                (P_AT_AMOUNT, 0.8),
                (P_AT_FREQ1, 1.0),
                (P_AT_FREQ2, 1.0),
                (P_AT_LFO, 1.0),
                (P_AT_AMP, 1.0),
                (P_AT_LP, 1.0),
                (P_AT_HP, 1.0),
                (P_DISTORTION, 0.3),
                (P_FX_ON, 1.0),
                (P_FXA_TYPE, knob_for(fx::DDL, FX_A_TYPES.len())),
                (P_FXA_MIX, 0.5),
                (P_FXA_P1, 0.4),
                (P_FXA_P2, 0.4),
                (P_FXA_SYNC, 1.0),
                (P_FXA_DIV, knob_for(7, SYNC_DIVISIONS.len())),
                // Effect A synced and Effect B free, so that the division
                // selectors are live on one slot and the delay-time knobs on
                // the other: a synced delay takes its time from the division
                // and ignores parameter 1, which is the instrument's own
                // behaviour and not something a test can arrange around.
                (P_FXB_TYPE, knob_for(fx::DDL, FX_B_TYPES.len())),
                (P_FXB_MIX, 0.5),
                (P_FXB_P1, 0.4),
                (P_FXB_P2, 0.6),
                (P_FXB_SYNC, 0.0),
                (P_FXB_DIV, knob_for(5, SYNC_DIVISIONS.len())),
                (P_BPM, 100.0 / 250.0),
                (P_UNISON, 1.0),
                (P_UNISON_MODE, knob_for(2, UNISON_MODES.len())),
                // Last-note priority, so that the leap in `play` actually
                // moves the stack: under the low-note priority the panel
                // otherwise defaults to, a note above the one already held
                // does nothing at all and glide has nothing to slide.
                (P_KEY_MODE, knob_for(2, KEY_MODES.len())),
                (P_GLIDE, 1.0),
                (P_GLIDE_RATE, 0.4),
                (P_LP_VEL, 1.0),
                (P_HP_VEL, 1.0),
                (P_VCA_VEL, 1.0),
                (P_SLOP, 0.3),
                (P_PAN_SPREAD, 0.5),
                (P_VOLUME, 0.8),
            ]);
            panel
        };

        for (index, name) in PARAM_NAMES.iter().enumerate() {
            if index == P_PROGRAM || index == P_BANK {
                continue;
            }
            // The two effect-type selectors are stepped between two effects
            // that *render* rather than between two ends of the travel: both
            // ends of Effect A's list are silent stages — position 0 is off
            // and position 5 is a phaser, which is stored and not rendered —
            // so a sweep between them would be comparing a wire with a wire.
            let (down, up) = match index {
                P_FXA_TYPE => (knob_for(fx::BBD, FX_A_TYPES.len()), knob_for(fx::CHORUS, FX_A_TYPES.len())),
                P_FXB_TYPE => (knob_for(fx::DDL, FX_B_TYPES.len()), knob_for(fx::CHORUS, FX_B_TYPES.len())),
                _ => (0.15, 0.85),
            };
            // ...and the two delay-time knobs are probed with their own
            // slot's clock sync switched the other way, for the same reason.
            let mut panel = rich.clone();
            match index {
                P_FXA_P1 => panel.push((P_FXA_SYNC, 0.0)),
                P_FXB_DIV => panel.push((P_FXB_SYNC, 1.0)),
                _ => {}
            }
            let mut low = built(&panel);
            low.set_parameter(index, down);
            let mut high = built(&panel);
            high.set_parameter(index, up);
            let a = play(&mut low);
            let b = play(&mut high);
            let same = a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1.0e-7);
            assert!(!same, "{name} does nothing to the sound");
        }
    }

    // ── The factory bank ──

    #[test]
    fn the_rom_is_the_shape_the_decoder_expects() {
        assert_eq!(ROM.len(), PROGRAM_COUNT * PACKED_PROGRAM);
        // Every one of the 87 decoded parameters, in every one of the 500
        // programs, inside the range the manual's NRPN appendix gives it.
        // This is the check that a byte map slipping by one would fail.
        for index in 0..PROGRAM_COUNT {
            let block = &ROM[index * PACKED_PROGRAM..(index + 1) * PACKED_PROGRAM];
            for (param, name) in PARAM_NAMES.iter().enumerate() {
                let Some((offset, max)) = raw_offset(param) else { continue };
                assert!(
                    f64::from(block[offset]) <= max,
                    "program {index} has {name} = {} at byte {offset}, past its documented {max}",
                    block[offset]
                );
            }
            for character in &block[107..127] {
                assert!(
                    (0x20..=0x7E).contains(character),
                    "program {index} has a name character outside printable ASCII"
                );
            }
        }
    }

    #[test]
    fn program_names_come_from_the_rom() {
        // Spot checks against Sequential's own factory-preset list.
        assert_eq!(program_name(0), "Brassed Off");
        assert_eq!(program_name(31), "Hi Hat");
        assert_eq!(program_name(34), "Jupiter Bass");
        assert_eq!(program_name(191), "In Unison");
        assert_eq!(program_name(419), "Filter Sweep");
        assert_eq!(program_name(487), "T8 Piano");
        assert_eq!(program_name(PROGRAM_COUNT), program_name(PROGRAM_COUNT - 1));
        // The bank contains a run of forty Prophet-5 ports, named for the
        // instrument this one descends from.
        let ports = (400..460).filter(|i| program_name(*i).starts_with("P5 ")).count();
        assert_eq!(ports, 40, "the forty Prophet-5 ports are not where they were");

        for index in 0..PROGRAM_COUNT {
            let name = program_name(index);
            assert!(!name.is_empty(), "program {index} has no name");
            assert!(name.chars().count() <= 20, "program {index} name {name:?} is too long");
            assert!(!name.ends_with(' '), "program {index} name {name:?} keeps its padding");
            let label = program_label(index);
            assert!(label.chars().count() <= LABEL_WIDTH, "program {index} label {label:?} is too long");
            assert!(name.starts_with(label), "program {index} label {label:?} is not its name");
        }
    }

    #[test]
    fn no_factory_program_is_silent() {
        // Three of the 500 have a VCA envelope amount of zero and are only
        // audible because the LFO is routed to the amplifier — which is what
        // fixed the LFO destination byte order. If that assignment regresses,
        // this is the test that says so.
        for index in 0..PROGRAM_COUNT {
            let mut s = fresh(index);
            let out = render(&mut s, &[note_on(60, 110, 0)], 120);
            assert!(
                out.iter().all(|v| v.is_finite()),
                "program {index} {} produced a non-finite sample",
                program_name(index)
            );
            if peak(&out) > 1.0e-4 {
                continue;
            }
            // A program whose filter envelope takes six seconds to open is
            // not silent, it is slow — the bank has one, 443 P5 43 — so a
            // program that says nothing in the first 0.7 s is given the
            // fifteen it would take the slowest attack and release in the
            // instrument.
            let mut s = fresh(index);
            let slow = render(&mut s, &[note_on(60, 110, 0)], 2_600);
            assert!(
                peak(&slow) > 1.0e-4,
                "program {index} {} is silent (peak {:.2e} over fifteen seconds)",
                program_name(index),
                peak(&slow)
            );
        }
    }

    #[test]
    fn the_bank_covers_the_instrument() {
        // A factory bank that never touches half the panel would mean a byte
        // map that had lost half the panel. Every selector's every position
        // that the bank is known to use, and every switch both ways.
        let mut used: Vec<Vec<bool>> = (0..PARAM_COUNT)
            .map(|index| vec![false; discrete_steps(index).unwrap_or(0)])
            .collect();
        let mut spread: Vec<(f32, f32)> = vec![(f32::MAX, 0.0); PARAM_COUNT];
        for index in 0..PROGRAM_COUNT {
            let panel = params_for_program(program_knobs(index).0, program_knobs(index).1);
            for param in 0..PARAM_COUNT {
                if let Some(count) = discrete_steps(param) {
                    used[param][selector(panel[param], count)] = true;
                }
                let (low, high) = &mut spread[param];
                *low = low.min(panel[param]);
                *high = high.max(panel[param]);
            }
        }
        // Both positions of every switch.
        for param in [
            P_SYNC, P_OSC2_LOW, P_OSC2_KEY, P_LP_VEL, P_HP_VEL, P_VCA_VEL, P_FX_ON, P_UNISON,
            P_GLIDE, P_FXA_SYNC, P_FXB_SYNC,
        ] {
            assert!(used[param][0] && used[param][1], "the bank never moves {}", PARAM_NAMES[param]);
        }
        // Every effect type, every LFO shape, every unison stacking, every
        // glide mode, and both filter tracking amounts.
        for (param, count) in [
            (P_FXA_TYPE, FX_A_TYPES.len()),
            (P_FXB_TYPE, FX_B_TYPES.len()),
            (P_LFO_SHAPE, LFO_SHAPES.len()),
            (P_UNISON_MODE, UNISON_MODES.len()),
            (P_GLIDE_MODE, GLIDE_MODES.len()),
        ] {
            for position in 0..count {
                assert!(
                    used[param][position],
                    "the bank never selects {} position {position}",
                    PARAM_NAMES[param]
                );
            }
        }
        // And every continuous control is actually varied across the bank.
        for param in 0..PARAM_COUNT {
            if discrete_steps(param).is_some() {
                continue;
            }
            let (low, high) = spread[param];
            assert!(
                high - low > 0.2,
                "the bank only ever puts {} between {low:.2} and {high:.2}",
                PARAM_NAMES[param]
            );
        }
    }

    /// Every control at its maximum at once, which is not a program but is
    /// reachable by hand, on the loudest chord the keyboard can play.
    ///
    /// The bank sweep in `tests/headroom.rs` covers the 500 programs; this
    /// covers the panel, which is the other half — a player who turns
    /// everything up must not be able to make the master limiter act on this
    /// track alone.
    #[test]
    fn the_worst_panel_a_hand_can_reach_stays_under_the_ceiling() {
        /// The master limiter's ceiling, −1 dBFS. The same value
        /// `tests/headroom.rs` uses, repeated for the same reason.
        const TARGET_PEAK: f32 = 0.891;

        let mut s = Prophet6::new();
        s.init(SR, BLOCK);
        // The program selectors first, since moving them reloads the panel,
        // and then everything else to the top of its travel.
        s.set_parameter(P_PROGRAM, 0.0);
        s.set_parameter(P_BANK, 0.0);
        for index in 2..PARAM_COUNT {
            s.set_parameter(index, 1.0);
        }
        s.reset();

        let notes: [u8; 8] = [36, 43, 48, 55, 60, 64, 67, 72];
        let events: Vec<MidiEvent> =
            notes.iter().map(|&n| note_on(n, 127, 0)).collect();
        // Long enough for the delay at the top of its time knob to fill, the
        // LFO at the top of its rate knob to be irrelevant, and the filter
        // envelope at the top of its attack to arrive.
        let mut out = render(&mut s, &events, 400);
        out.extend_from_slice(&render(&mut s, &[aftertouch(127, 0), cc(1, 127, 1)], 800));

        assert!(out.iter().all(|v| v.is_finite()), "the worst panel produced a non-finite sample");
        let top = peak(&out);
        assert!(
            top <= TARGET_PEAK,
            "every control at maximum peaks at {top:.4}, past the −1 dBFS ceiling"
        );
        // And the panel has to be able to reach a useful level, or the
        // assertion above is met by an instrument that is simply too quiet.
        assert!(
            top > 0.1,
            "every control at maximum only reaches {top:.4}, so the trim is too deep"
        );
    }

    // ── Real-time safety ──

    #[test]
    fn the_audio_path_does_not_allocate() {
        // "No allocation in `process`" is a property of the code rather than
        // of its output, so it is counted rather than listened to. The
        // counting allocator lives in synth.rs and is installed for the whole
        // test binary; this is the Prophet-6's half of it.
        use crate::synth::tests::allocations_during;

        let mut s = Prophet6::new();
        s.init(SR, 256);
        let mut out = vec![0.0f32; 256];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 0)]);

        // More simultaneous keys than the keyboard remembers, so the note
        // stack has to push the oldest off the bottom rather than grow.
        let chord: Vec<MidiEvent> = (36u8..72)
            .map(|n| MidiEvent {
                sample_offset: u32::from(n) % 8,
                status: 0x90,
                data1: n,
                data2: 110,
            })
            .collect();
        let releases: Vec<MidiEvent> = (36u8..72).map(|n| note_off(n, 0)).collect();

        let allocations = allocations_during(|| {
            let mut outs: [&mut [f32]; 1] = [&mut out];
            s.process(&[], &mut outs, &chord);
            for _ in 0..8 {
                s.process(&[], &mut outs, &[]);
            }
            s.process(&[], &mut outs, &[cc(1, 64, 0), aftertouch(80, 4)]);
            s.process(&[], &mut outs, &releases);
            // Every program in the bank, loaded while the instrument is
            // sounding, which is what a preset sweep does.
            for index in 0..PROGRAM_COUNT {
                let (bank, program) = program_knobs(index);
                s.set_parameter(P_BANK, bank);
                s.set_parameter(P_PROGRAM, program);
                s.process(&[], &mut outs, &[note_on(60, 110, 0)]);
            }
            // Unison, chord memory and the two panic controls.
            s.set_parameter(P_UNISON, 1.0);
            s.process(&[], &mut outs, &[note_on(48, 100, 0), note_on(55, 100, 8)]);
            s.set_parameter(P_UNISON_MODE, knob_for(6, UNISON_MODES.len()));
            s.process(&[], &mut outs, &[note_on(60, 100, 0)]);
            s.process(&[], &mut outs, &[cc(123, 0, 0)]);
            s.process(&[], &mut outs, &[cc(120, 0, 0)]);
        });
        assert_eq!(allocations, 0, "the audio path allocated {allocations} times");
    }

    // ── The programs, by measurement ──

    /// What a program does with a held note and a release: level, where its
    /// energy sits, how it moves, and what is left at the end.
    struct Character {
        level: f64,
        brightness: f64,
        /// The share of energy under 300 Hz.
        bass: f64,
        /// How much of the note is left in its last quarter.
        tail: f64,
        /// The loudest 4096-sample window over the quietest, which separates
        /// a program that moves from one that sits still.
        movement: f64,
        /// The brightest window over the dullest, which is what a filter
        /// sweep looks like.
        sweep: f64,
    }

    fn character(index: usize) -> Character {
        let mut s = fresh(index);
        let mut out = render(&mut s, &[note_on(48, 100, 0)], 150);
        out.extend_from_slice(&render(&mut s, &[note_off(48, 0)], 60));
        let windows = window_rms(&out);
        let loud = windows.iter().copied().fold(0.0f64, f64::max);
        let quiet = windows.iter().copied().fold(f64::MAX, f64::min);
        let brights: Vec<f64> = out
            .chunks(4_096)
            .filter(|c| c.len() == 4_096 && rms(c) > loud * 0.2)
            .map(|c| brightness(c, SR))
            .collect();
        let quarter = out.len() / 4;
        Character {
            level: rms(&out),
            brightness: brightness(&out, SR),
            bass: low_band(&out, 300.0, SR) / rms(&out).max(1.0e-30),
            tail: rms(&out[out.len() - quarter..]) / rms(&out[..quarter]).max(1.0e-30),
            movement: loud / quiet.max(1.0e-12),
            sweep: brights.iter().copied().fold(0.0f64, f64::max)
                / brights.iter().copied().fold(f64::MAX, f64::min).max(1.0e-12),
        }
    }

    /// The five programs the decode was checked against, checked again by
    /// rendering them rather than by reading their bytes.
    #[test]
    fn the_anchor_programs_sound_like_their_names() {
        let hat = character(31);
        assert_eq!(program_name(31), "Hi Hat");
        assert!(
            hat.brightness > 3_000.0,
            "031 Hi Hat is not noisy: its energy sits at {:.0} Hz",
            hat.brightness
        );
        assert!(hat.bass < 0.15, "031 Hi Hat has {:.2} of its energy under 300 Hz", hat.bass);
        assert!(hat.tail < 0.02, "031 Hi Hat is still sounding at the end: {:.3}", hat.tail);

        let sweep = character(419);
        assert_eq!(program_name(419), "Filter Sweep");
        assert!(
            sweep.sweep > 1.5,
            "419 Filter Sweep does not sweep: its brightest window is only {:.2} times \
             its dullest",
            sweep.sweep
        );
        assert!(
            sweep.brightness < 1_500.0,
            "419 Filter Sweep is not a filtered sound: {:.0} Hz",
            sweep.brightness
        );

        // 191 In Unison stacks the whole instrument on one note, so it is
        // loud for what it is: one note at the level of a program's chord.
        let unison = character(191);
        assert_eq!(program_name(191), "In Unison");
        let panel = params_for_program(program_knobs(191).0, program_knobs(191).1);
        assert!(flag(&panel, P_UNISON), "191 In Unison does not have unison on");
        let bank_median = {
            let mut levels: Vec<f64> = (0..PROGRAM_COUNT)
                .step_by(17)
                .map(|i| character(i).level)
                .collect();
            levels.sort_by(|a, b| a.partial_cmp(b).unwrap());
            levels[levels.len() / 2]
        };
        assert!(
            unison.level > bank_median * 1.5,
            "191 In Unison is {:.5} against a bank median of {bank_median:.5}",
            unison.level
        );

        // 011 ScumSlinger is the dirtiest program in the bank: distortion at
        // 127, the maximum anything in the 500 uses. Measured on crest
        // factor, which is what a clipper changes and changes unmistakably —
        // brightness says nothing here because this program is a six-voice
        // hard-sync lead with its filter open, and is already at eight
        // kilohertz before the distortion touches it.
        assert_eq!(program_name(11), "ScumSlinger");
        let panel = params_for_program(program_knobs(11).0, program_knobs(11).1);
        assert!(
            (knob(&panel, P_DISTORTION) - 127.0).abs() < 0.5,
            "011 ScumSlinger no longer has its distortion at maximum"
        );
        let crest = |distortion: Option<f32>| {
            let mut s = fresh(11);
            if let Some(amount) = distortion {
                s.set_parameter(P_DISTORTION, amount);
            }
            let out = render(&mut s, &[note_on(48, 100, 0)], 150);
            f64::from(peak(&out)) / rms(&out).max(1.0e-30)
        };
        let clean = crest(Some(0.0));
        let dirty = crest(None);
        assert!(
            dirty < clean * 0.7,
            "011 ScumSlinger's distortion does not clip: crest factor {clean:.2} with the \
             knob down against {dirty:.2} as stored"
        );

        // 000 Brassed Off is the program the instrument loads with: a brass
        // poly, so it has a body under it, it sustains while the key is down
        // and it is not a bright lead.
        let brass = character(0);
        assert_eq!(program_name(0), "Brassed Off");
        assert!(
            brass.bass > 0.3,
            "000 Brassed Off has only {:.2} of its energy under 300 Hz",
            brass.bass
        );
        // Measured at C3, where the fundamental is 131 Hz: a squarish pulse
        // through a four-pole sitting near 900 Hz puts the centre of gravity
        // at about one and a half times the fundamental, which is what a
        // filtered brass patch is.
        assert!(
            (150.0..1_200.0).contains(&brass.brightness),
            "000 Brassed Off's energy sits at {:.0} Hz, which is not a brass sound",
            brass.brightness
        );
        assert!(
            brass.sweep > 1.25,
            "000 Brassed Off has no attack on it: its brightest window is {:.2} times its \
             dullest",
            brass.sweep
        );
        assert!(
            brass.tail > 0.3,
            "000 Brassed Off does not sustain while the key is down: {:.2}",
            brass.tail
        );
    }

    /// A spread of programs from every one of the five banks, rendered and
    /// measured rather than eyeballed: every one has to speak, stay finite,
    /// land in a usable level range and sit somewhere sensible in the
    /// spectrum.
    #[test]
    fn programs_from_every_bank_render_plausibly() {
        // Twenty-five, five from each bank, spread across each hundred.
        let picks: Vec<usize> = (0..BANK_COUNT)
            .flat_map(|bank| (0..5).map(move |n| bank * PROGRAMS_PER_BANK + n * 23 + 4))
            .collect();
        let mut report = String::new();
        for index in picks {
            let c = character(index);
            report.push_str(&format!(
                "\n  {index:>3} {:<21} rms {:.5} bright {:>6.0} bass {:.2} tail {:.2}",
                program_name(index), c.level, c.brightness, c.bass, c.tail
            ));
            assert!(
                c.level > 2.0e-4,
                "program {index} {} is inaudible at {:.6} rms{report}",
                program_name(index), c.level
            );
            assert!(
                c.level < 0.35,
                "program {index} {} is at {:.4} rms, which is far above the bank{report}",
                program_name(index), c.level
            );
            assert!(
                (20.0..14_000.0).contains(&c.brightness),
                "program {index} {} has its energy at {:.0} Hz{report}",
                program_name(index), c.brightness
            );
        }
    }

    /// Sequential's 500 are 500 different sounds, near enough.
    ///
    /// Not "all 125,250 pairs differ", which is not something a factory bank
    /// promises: this one contains forty Prophet-5 ports built from one
    /// template, three pairs that share a *name*, and families of programs
    /// that are one knob apart on purpose. What is asserted is that the bank
    /// is not degenerate — that the decode has not collapsed it — so at most
    /// one program in twenty may sit inside the tolerance of another, and the
    /// closest pairs are printed when it fails.
    #[test]
    fn the_bank_is_five_hundred_sounds_rather_than_one() {
        fn fingerprint(index: usize) -> [f64; 6] {
            let c = character(index);
            [c.level, c.brightness, c.bass, c.tail, c.movement.min(50.0), c.sweep.min(20.0)]
        }
        /// What counts as a difference, per feature.
        const TOLERANCE: [f64; 6] = [0.10, 0.10, 0.08, 0.12, 0.20, 0.15];
        fn apart(a: &[f64; 6], b: &[f64; 6]) -> f64 {
            let mut worst = 0.0f64;
            for i in 0..6 {
                let d = (a[i] - b[i]).abs() / a[i].abs().max(b[i].abs()).max(1.0e-12);
                worst = worst.max(d / TOLERANCE[i]);
            }
            worst
        }

        let prints: Vec<[f64; 6]> = (0..PROGRAM_COUNT).map(fingerprint).collect();
        let mut twins: Vec<(f64, usize, usize)> = Vec::new();
        let mut has_twin = vec![false; PROGRAM_COUNT];
        for a in 0..PROGRAM_COUNT {
            for b in a + 1..PROGRAM_COUNT {
                let distance = apart(&prints[a], &prints[b]);
                if distance <= 1.0 {
                    twins.push((distance, a, b));
                    has_twin[a] = true;
                    has_twin[b] = true;
                }
            }
        }
        let count = has_twin.iter().filter(|v| **v).count();
        if count > PROGRAM_COUNT / 20 {
            twins.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
            let mut report = String::new();
            for (distance, a, b) in twins.iter().take(12) {
                report.push_str(&format!(
                    "\n  {distance:.3}  {a:>3} {:<21} / {b:>3} {}",
                    program_name(*a), program_name(*b)
                ));
            }
            panic!(
                "{count} of the {PROGRAM_COUNT} programs render as a near-copy of another, \
                 which is more than a factory bank's own families account for:{report}"
            );
        }
    }
}

#[cfg(test)]
mod measure {
    //! Printouts rather than assertions: the numbers that set `OUTPUT_TRIM`,
    //! size the bank's spread and answer "how much does this instrument
    //! cost". Ignored by default; run one with
    //! `cargo test -p phosphor-dsp --lib -- --ignored --nocapture report_`.

    use super::tests::*;
    use super::*;

    #[test]
    #[ignore]
    fn report_levels() {
        let chord: [u8; 8] = [36, 43, 48, 55, 60, 64, 67, 72];
        let mut triad: Vec<(usize, f64)> = Vec::new();
        let mut worst = (0.0f32, 0usize);
        for index in 0..PROGRAM_COUNT {
            let mut s = fresh(index);
            let held = render_program(&mut s, &[60, 64, 67], 100, 200);
            triad.push((index, rms(&held)));
            let mut s = fresh(index);
            let loud = render_program(&mut s, &chord, 127, 400);
            let top = peak(&loud);
            if top > worst.0 {
                worst = (top, index);
            }
        }
        triad.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        println!("triad @100, rms over 1.16 s:");
        for (label, at) in [("min", 0), ("p25", 125), ("median", 250), ("p75", 375), ("max", 499)] {
            println!(
                "  {label:>6} {:.6}  {:>3} {}",
                triad[at].1,
                triad[at].0,
                program_name(triad[at].0)
            );
        }
        println!(
            "loudest eight-note chord @127: {:.4} on {} {}",
            worst.0,
            worst.1,
            program_name(worst.1)
        );
        println!(
            "  before the saturator: {:.4}",
            crate::level::saturation_input(worst.0)
        );
    }

    #[test]
    #[ignore]
    fn report_filter_against_the_ladder() {
        let (ssm, ladder) = passband_loss();
        println!("bass under 150 Hz lost between no resonance and full resonance:");
        println!("  Prophet-6 SSM2040-lineage low-pass: {ssm:.2} dB");
        println!("  Little Phatty transistor ladder:    {ladder:.2} dB");
        println!("  the gap:                            {:.2} dB", ladder - ssm);
    }

    const SR_M: f64 = 44_100.0;

    /// The five programs the decode was checked against, and three more, with
    /// the numbers `the_anchor_programs_sound_like_their_names` asserts on.
    #[test]
    #[ignore]
    fn report_anchors() {
        println!(
            "{:>4} {:<20} {:>8} {:>9} {:>8} {:>6} {:>6} {:>7}",
            "idx", "name", "peak", "rms", "bright", "bass", "tail", "crest"
        );
        for index in [0usize, 9, 11, 13, 31, 191, 419, 460] {
            let mut s = fresh(index);
            let mut out = render_program(&mut s, &[48], 100, 150);
            out.extend_from_slice(&render(&mut s, &[note_off(48, 0)], 60));
            let quarter = out.len() / 4;
            let (mut total, mut slope, mut last) = (0.0f64, 0.0f64, 0.0f64);
            for v in &out {
                let x = f64::from(*v);
                total += x * x;
                slope += (x - last) * (x - last);
                last = x;
            }
            let level = rms(&out);
            println!(
                "{index:>4} {:<20} {:>8.4} {:>9.5} {:>8.0} {:>6.2} {:>6.2} {:>7.2}",
                program_name(index),
                peak(&out),
                level,
                (slope / total.max(1.0e-30)).sqrt() * SR_M / TAU,
                low_band(&out, 300.0, SR_M) / level.max(1.0e-30),
                rms(&out[out.len() - quarter..]) / rms(&out[..quarter]).max(1.0e-30),
                f64::from(peak(&out)) / level.max(1.0e-30),
            );
        }
    }

    /// Five programs from each of the five banks, rendered and measured — the
    /// printout behind `programs_from_every_bank_render_plausibly`.
    #[test]
    #[ignore]
    fn report_bank_spread() {
        println!("{:>4} {:<22} {:>9} {:>8} {:>6} {:>6}", "idx", "name", "rms", "bright", "bass", "tail");
        for bank in 0..BANK_COUNT {
            for n in 0..5 {
                let index = bank * PROGRAMS_PER_BANK + n * 23 + 4;
                let mut s = fresh(index);
                let mut out = render_program(&mut s, &[48], 100, 150);
                out.extend_from_slice(&render(&mut s, &[note_off(48, 0)], 60));
                let quarter = out.len() / 4;
                let (mut total, mut slope, mut last) = (0.0f64, 0.0f64, 0.0f64);
                for v in &out {
                    let x = f64::from(*v);
                    total += x * x;
                    slope += (x - last) * (x - last);
                    last = x;
                }
                let bright = (slope / total.max(1.0e-30)).sqrt() * SR_M / TAU;
                println!(
                    "{index:>4} {:<22} {:>9.5} {bright:>8.0} {:>6.2} {:>6.2}",
                    program_name(index),
                    rms(&out),
                    low_band(&out, 300.0, SR_M) / rms(&out).max(1.0e-30),
                    rms(&out[out.len() - quarter..]) / rms(&out[..quarter]).max(1.0e-30),
                );
            }
        }
    }

    /// The pitch of a held note at every sample rate — the printout behind
    /// `the_pitch_is_the_same_at_every_sample_rate`.
    #[test]
    #[ignore]
    fn report_rate_independence() {
        for note in [36u8, 48, 60, 72] {
            let mut line = format!("note {note:>3}:");
            let mut reference = 0.0;
            for rate in [44_100.0f64, 22_050.0, 48_000.0, 96_000.0] {
                let mut s = at_rate(&plain_tone(), rate);
                let blocks = (60.0 * rate / 44_100.0) as usize;
                let out = render_at(&mut s, &[note_on(note, 100, 0)], blocks, rate);
                let measured = crossings_per_second(&out, rate);
                if reference == 0.0 {
                    reference = measured;
                }
                line.push_str(&format!(
                    "  {rate:>7.0} Hz {:>8.2} ({:+.2}%)",
                    measured / 2.0,
                    100.0 * (measured / reference - 1.0)
                ));
            }
            println!("{line}");
        }
    }

    #[test]
    #[ignore]
    fn report_cost() {
        // Wall-clock cost of a six-voice chord, against the audio it produces.
        let mut s = Prophet6::new();
        s.init(SR_M, 512);
        let mut left = vec![0.0f32; 512];
        let mut right = vec![0.0f32; 512];
        let events: Vec<MidiEvent> = [48u8, 52, 55, 59, 62, 67]
            .iter()
            .map(|&n| MidiEvent { sample_offset: 0, status: 0x90, data1: n, data2: 100 })
            .collect();
        let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
        s.process(&[], &mut outs, &events);
        let blocks = 2_000;
        let start = std::time::Instant::now();
        for _ in 0..blocks {
            let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
            s.process(&[], &mut outs, &[]);
        }
        let elapsed = start.elapsed().as_secs_f64();
        let audio = f64::from(blocks) * 512.0 / SR_M;
        println!(
            "six voices: {:.1} µs a 512-sample block, {:.2}% of one core at 44.1 kHz",
            elapsed / f64::from(blocks) * 1.0e6,
            100.0 * elapsed / audio
        );
    }
}
