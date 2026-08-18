//! The Phosphor synth: four oscillators, vector mixing, a ladder filter and a
//! virtual patch matrix.
//!
//! Every other instrument in the rack is a model of a particular machine,
//! which leaves this one free to be the opposite — three ideas taken from
//! three instruments and put in one signal path:
//!
//! * **the Minimoog's front half.** A four-pole transistor ladder with the
//!   bass loss that comes with its resonance, and a drive stage *before* it
//!   rather than after, because a Minimoog's growl is its mixer pushed past
//!   unity into the filter. One oscillator can also be given up to the
//!   modulation matrix, the way that instrument trades its third for an LFO.
//! * **the microKORG's matrix and its wavetables.** Assignable source →
//!   destination → amount, and a bank of digital single-cycle waveforms
//!   sitting alongside the analog shapes on the same oscillator.
//! * **the Prophet VS / Wavestation's vector mix.** Four oscillators balanced
//!   by a position on a square rather than by four faders — and that position
//!   is itself a modulation destination, so the balance can move.
//! * **the Wavestation's wave sequencing.** Each of the four oscillators can
//!   be handed a step list — waveform, length, crossfade, pitch, level — that
//!   it walks on its own clock, crossfading between neighbours. See below.
//!
//! ```text
//!   SEQ ─▶ OSC A ─┐
//!   SEQ ─▶ OSC B ─┤                                 ┌── AMP ──►
//!   SEQ ─▶ OSC C ─┼─ vector mix ─ DRIVE ─ LADDER ───┘
//!   SEQ ─▶ OSC D ─┘                 ▲        ▲
//!            ▲                      │        │
//!            │                ┌─────┴────────┴───────┐
//!            └────────────────┤      MOD MATRIX      │
//!                             │  src → dest → amount │
//!                             └──────────────────────┘
//! ```
//!
//! ## Wave sequencing
//!
//! A wave sequence is a list of steps. Each step names a waveform from the
//! bank, a length, a crossfade into the next step, a pitch offset and a level;
//! an oscillator pointed at a sequence walks the list and crossfades between
//! neighbours, so its timbre — and its pitch, and its rhythm — evolve on their
//! own. No envelope can imitate that, because an envelope shapes one waveform
//! where this replaces it.
//!
//! Per *oscillator* rather than per voice, which is the Wavestation's own
//! shape: four sequences of different lengths running against each other and
//! mixed on the vector square do not repeat on any period a listener can
//! count, where one sequence for the whole voice is a loop.
//!
//! The sequences live in [`SEQ_BANK`], a `'static` table, and a patch points
//! each oscillator at one by index — so a sequence is an object a patch
//! *references*, the way a Wavestation's is, rather than something a patch
//! contains. The panel selector is the reference, "off" is its first position,
//! and the oscillator's own controls stay live over the top of it: WAVE picks
//! whether the step's waveform is read at all, TABLE slides the whole sequence
//! through the bank, TUNE and LEVEL scale what the steps ask for.
//!
//! Timing is a clock in hertz — the SEQ RATE knob — and a step length in
//! *ticks* of it, rather than a step length in seconds. Two reasons. One knob
//! then speeds a whole pattern up without changing its rhythm, which is what a
//! player wants and what seconds-per-step cannot give; and a rate is one
//! number, so the matrix can push it (`seq hz`) where a table of durations
//! could not be pushed at all. It free-runs rather than syncing to the
//! transport because the plugin API carries no tempo — `Plugin::process` is
//! handed audio and MIDI and nothing else — so sync is not this instrument's
//! to implement.
//!
//! ## Melodic and keymapped patches
//!
//! A patch declares how it reads the keyboard, in `KeyMap`. A melodic patch
//! maps a note to a pitch, which is what a synthesizer normally does. A
//! **keymapped** patch maps a note to an entire voice recipe — its own
//! oscillator shapes and wavetable positions, tuning, vector balance, filter
//! and envelopes — which is what a Wavestation, an M1 or any other rompler
//! calls a drum kit.
//!
//! That is an engine capability rather than patch data, and it is here in
//! phase one for a reason: an engine that can only transpose one recipe cannot
//! be given drum patches later without being rebuilt. The point of having
//! drums here rather than only in the drum rack is that they come out of the
//! same four oscillators, the same ladder and the same envelopes as the pads,
//! so they sit in the same sonic world.
//!
//! ## The bank
//!
//! 229 patches, in `bank.rs` beside this file, and they are four sets in three
//! different situations: the eleven this instrument shipped with, the
//! microKORG's 128 factory programs under their **real names, slots,
//! categories and tempi with authored parameter values**, forty Minimoog
//! patches which are **authored outright because that instrument never had a
//! factory bank to transcribe**, and fifty Wavestation-idiom patches — wave
//! sequencing, vector movement and three drum kits — which are authored too.
//! That module's documentation is where the distinction is spelled out,
//! including what the sixteen vocoder programs and the eighteen arpeggio
//! programs are on an instrument with neither a vocoder nor an arpeggiator.
//!
//! One selector, not two: see the note above [`patch_bank`] for why this
//! instrument answers the "bank knob" question the opposite way to the DX7.
//!
//! ## The wavetables
//!
//! The sixteen digital waveforms in [`WAVE_NAMES`] are **generated here**, from
//! the harmonic tables in `WAVES`, in the spirit of the DW-8000's set rather
//! than copied from it. Nothing in this file is sampled from, transcribed from
//! or derived from Korg's ROM; what is borrowed is the idea — a small bank of
//! additive single-cycle shapes, mostly drawbar organ, reed, brass, vocal and
//! bell spectra, reachable from the same oscillator that makes a sawtooth.
//!
//! ## Real-time safety
//!
//! Nothing in `process` allocates, locks or panics. The voices are a fixed
//! array rather than a `Vec`, the MIDI sort runs in a fixed buffer, the key
//! map is a `'static` table read by a bounded scan at note-on, the wave
//! sequences are `'static` step lists that a voice indexes into rather than
//! scans — twice per step, not per sample — and the wavetable bank is built
//! once per process behind a `OnceLock` that the constructor resolves into a
//! `&'static`, so the audio thread never touches the cell at all.
//!
//! Every loop is bounded by a compile-time constant except the sample loop
//! itself. The one that could have grown is the sequence cursor: its increment
//! is clamped so that it crosses at most one step boundary per sample, which
//! makes advancing it an `if` rather than the "drain everything pending" loop
//! that a high enough clock rate would otherwise turn into a stall.
//!
//! That is checked rather than claimed. `the_audio_path_does_not_allocate`
//! counts allocations on its own thread across a buffer with three times as
//! many simultaneous keys as there are voices, on a sequenced patch, with
//! every sequence in the bank pointed at every oscillator in turn under a held
//! chord — and asserts the count is zero. The Odyssey had a `Vec` that could
//! reallocate inside the callback on the seventeenth simultaneous key, and
//! nothing caught it, because "no allocation in `process`" is a property of
//! the code rather than of its output.

use std::sync::OnceLock;

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

use crate::level::soft_saturate;

const TWO_PI: f64 = std::f64::consts::TAU;
const PI: f64 = std::f64::consts::PI;

/// Eight voices, the same as the Jupiter.
///
/// The instrument this replaced carried sixteen, on one oscillator pair and a
/// two-pole filter. Four oscillators and a four-pole ladder is roughly four
/// times that per voice, and eight is what the other polysynths in the rack
/// hold — an eight-note chord, which is what the headroom sweep plays, fits
/// exactly.
const MAX_VOICES: usize = 8;

/// Fixed headroom trim on the voice sum, applied after the gain knob.
///
/// Sized on ordinary playing, in step with the other five — see `OUTPUT_TRIM`
/// in dx7.rs, which carries the full reasoning. The trim lands this synth's
/// default patch at the same typical loudness as theirs: a triad at velocity
/// 100 measures 0.0286 RMS against their 0.0187 to 0.0314, and peaks at
/// 0.1397.
///
/// Sized on the *extremes* as well, which is unusual here and is because this
/// is the instrument whose predecessor failed exactly there. With sub, noise,
/// cutoff, resonance and sustain all at maximum that one reached 0.9470 on an
/// eight-note chord — past the master limiter's ceiling, burning 5 dB of
/// saturation against a 2 dB budget — and nothing measured it.
/// `the_panel_at_its_extremes_stays_under_the_ceiling` is what replaced that
/// silence: thirty-two deliberately hostile panels, each on five voicings at
/// two velocities, covering every oscillator shape at full level, every corner
/// of the vector, every matrix slot at full depth pointed at cutoff, resonance
/// and the vector at once, every wave sequence in the bank on all four
/// oscillators at two clock rates, the clock itself pushed to both ends of its
/// travel from every slot at once, and the filter self-oscillating with no
/// oscillator at all. The worst of the 320 renders measures 0.6982, which is
/// *under* the saturator's knee — so even the extremes are the trimmed voice
/// sum sample for sample, and the bounding stage never engages anywhere on the
/// panel.
///
/// Sequencing did not move that figure at all: it is the same case and the
/// same number as before there were sequences — every matrix slot at full
/// depth from velocity, on an eight-note chord — with the loudest sequenced
/// panel 0.0003 behind it. The reason is worth stating, because it is the
/// property that makes a step list safe to add: every waveform in the bank is
/// bounded by one, a step's level is bounded by one, and a crossfade between
/// two bounded values is bounded, so a step list cannot introduce level. The
/// vector mix is still bounded by the largest level knob, which is the
/// argument this trim already rested on.
///
/// Holding to the knee rather than to the 2 dB saturation budget the other
/// banks are allowed costs about 1.3 dB of level. It is worth it here: the
/// budget is a statement about how much of the saturator is inaudible, and
/// "the saturator is never in circuit" needs no such judgement.
///
/// The second factor is the GAIN knob, which sits at the top of its travel and
/// can therefore only cut. That is what makes the sweep above complete: a knob
/// with travel above its default is headroom nothing has measured.
const OUTPUT_TRIM: f32 = 0.116;

// ── Parameter indices ──
//
// Front-panel order, which here means signal order: the four oscillators, the
// vector that mixes them, the drive and filter they run into, the amplifier,
// then the modulation sources and the matrix that routes them. `patch` is
// first because that is where the editor looks for a preset selector.

pub const P_PATCH: usize = 0;

/// How many controls one oscillator has, and therefore the distance between
/// oscillator A's first knob and oscillator B's.
///
/// Public because an editor that wants to draw the four oscillators as four
/// rows needs the stride, and counting it from the index constants is the kind
/// of arithmetic that goes stale when a control is added — as one just was.
pub const P_OSC_STRIDE: usize = 6;

// OSC A
pub const P_A_WAVE: usize = 1;
pub const P_A_TABLE: usize = 2;
pub const P_A_TUNE: usize = 3;
pub const P_A_FINE: usize = 4;
pub const P_A_LEVEL: usize = 5;
pub const P_A_SEQ: usize = 6;
// OSC B
pub const P_B_WAVE: usize = 7;
pub const P_B_TABLE: usize = 8;
pub const P_B_TUNE: usize = 9;
pub const P_B_FINE: usize = 10;
pub const P_B_LEVEL: usize = 11;
pub const P_B_SEQ: usize = 12;
// OSC C
pub const P_C_WAVE: usize = 13;
pub const P_C_TABLE: usize = 14;
pub const P_C_TUNE: usize = 15;
pub const P_C_FINE: usize = 16;
pub const P_C_LEVEL: usize = 17;
pub const P_C_SEQ: usize = 18;
// OSC D
pub const P_D_WAVE: usize = 19;
pub const P_D_TABLE: usize = 20;
pub const P_D_TUNE: usize = 21;
pub const P_D_FINE: usize = 22;
pub const P_D_LEVEL: usize = 23;
pub const P_D_SEQ: usize = 24;
pub const P_D_MODE: usize = 25;

/// The wave sequence clock, shared by the four oscillators.
pub const P_SEQ_RATE: usize = 26;

// MIX
pub const P_VECTOR_X: usize = 27;
pub const P_VECTOR_Y: usize = 28;
pub const P_PULSE_WIDTH: usize = 29;

// FILTER
pub const P_DRIVE: usize = 30;
pub const P_CUTOFF: usize = 31;
pub const P_RESO: usize = 32;
pub const P_FILTER_ENV: usize = 33;
pub const P_KEY_FOLLOW: usize = 34;

// AMP
pub const P_VELOCITY: usize = 35;
pub const P_GAIN: usize = 36;

// LFOs
pub const P_LFO1_WAVE: usize = 37;
pub const P_LFO1_RATE: usize = 38;
pub const P_LFO2_WAVE: usize = 39;
pub const P_LFO2_RATE: usize = 40;

// ENVELOPES
pub const P_ATTACK1: usize = 41;
pub const P_DECAY1: usize = 42;
pub const P_SUSTAIN1: usize = 43;
pub const P_RELEASE1: usize = 44;
pub const P_ATTACK2: usize = 45;
pub const P_DECAY2: usize = 46;
pub const P_SUSTAIN2: usize = 47;
pub const P_RELEASE2: usize = 48;

/// The first of the matrix's slots. Slot `i` is three consecutive indices from
/// here: source, destination, amount.
pub const P_MOD_BASE: usize = 49;

/// How many source → destination → amount slots the matrix has.
///
/// Six. The microKORG's four is the reference and is famously just barely
/// enough, but four of the routes a microKORG patch spends a slot on are
/// hard-wired here — envelope 2 to cutoff, keyboard to cutoff, velocity to
/// amplitude, envelope 1 to amplitude — so six free slots go further than
/// four do there. Past six the panel starts to be mostly matrix: at three
/// parameters a slot, six is already 18 of the 67 controls.
pub const MOD_SLOTS: usize = 6;

pub const PARAM_COUNT: usize = P_MOD_BASE + MOD_SLOTS * 3;

/// The source knob of matrix slot `slot`, counting from zero.
#[must_use]
pub const fn p_mod_src(slot: usize) -> usize {
    P_MOD_BASE + slot * 3
}

/// The destination knob of matrix slot `slot`.
#[must_use]
pub const fn p_mod_dest(slot: usize) -> usize {
    P_MOD_BASE + slot * 3 + 1
}

/// The amount knob of matrix slot `slot`. Bipolar: the middle is no routing.
#[must_use]
pub const fn p_mod_amount(slot: usize) -> usize {
    P_MOD_BASE + slot * 3 + 2
}

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "patch",
    "a wave", "a table", "a tune", "a fine", "a level", "a seq",
    "b wave", "b table", "b tune", "b fine", "b level", "b seq",
    "c wave", "c table", "c tune", "c fine", "c level", "c seq",
    "d wave", "d table", "d tune", "d fine", "d level", "d seq", "d mode",
    "seq rate",
    "vector x", "vector y", "pw",
    "drive", "cutoff", "reso", "vcf env", "kybd",
    "vel", "gain",
    "lfo1", "lfo1 hz", "lfo2", "lfo2 hz",
    "attack 1", "decay 1", "sustain1", "release1",
    "attack 2", "decay 2", "sustain2", "release2",
    "m1 src", "m1 dest", "m1 amt",
    "m2 src", "m2 dest", "m2 amt",
    "m3 src", "m3 dest", "m3 amt",
    "m4 src", "m4 dest", "m4 amt",
    "m5 src", "m5 dest", "m5 amt",
    "m6 src", "m6 dest", "m6 amt",
];

/// Patch 0, INIT SAW, the panel the instrument loads with.
///
/// Derived from the first row of the bank rather than written out beside it,
/// so the default and the patch cannot drift apart.
pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = chart_params(0);

// ── Discrete controls ──
//
// Everything that picks a thing rather than sets a level: the patch selector,
// the four oscillator shape switches, the four wave sequence selectors, the
// oscillator D mode switch, the two LFO shape switches, the four coarse tune
// knobs and the matrix's twelve source and destination selectors. All of them
// are stored in the same 0..1 parameter block as the sliders, so a selector is
// a knob divided into `n` equal steps.

/// How many positions a selector has, or `None` for a slider.
fn discrete_steps(index: usize) -> Option<usize> {
    if index >= P_MOD_BASE {
        return match (index - P_MOD_BASE) % 3 {
            0 => Some(SOURCE_COUNT),
            1 => Some(DEST_COUNT),
            _ => None,
        };
    }
    match index {
        P_PATCH => Some(PATCH_COUNT),
        P_A_WAVE | P_B_WAVE | P_C_WAVE | P_D_WAVE => Some(SHAPE_COUNT),
        P_A_SEQ | P_B_SEQ | P_C_SEQ | P_D_SEQ => Some(SEQ_SLOTS),
        P_A_TUNE | P_B_TUNE | P_C_TUNE | P_D_TUNE => Some(TUNE_STEPS),
        P_D_MODE => Some(3),
        P_LFO1_WAVE | P_LFO2_WAVE => Some(LFO_SHAPE_COUNT),
        _ => None,
    }
}

/// One knob into one of `count` equal steps.
///
/// Total by construction: `params` is public, so the knob can arrive as
/// anything at all. The float-to-int cast saturates in both directions and
/// turns NaN into zero, so every input lands on a real position.
const fn selector(value: f32, count: usize) -> usize {
    let step = (value * (count as f32 - 0.01)) as usize;
    if step >= count {
        count - 1
    } else {
        step
    }
}

/// The knob position in the middle of step `index` of `count` — the one
/// position in the step that no amount of float rounding can push into a
/// neighbour. The inverse of [`selector`].
const fn knob_for(index: usize, count: usize) -> f32 {
    (index as f32 + 0.5) / count as f32
}

/// Which parameter indices are selectors (rendered as labels, not bars).
#[must_use]
pub fn is_discrete(index: usize) -> bool {
    discrete_steps(index).is_some()
}

/// The knob position one step up or down from `value`. Sliders are unchanged.
///
/// Steps by *index* rather than by adding a fraction of the travel. Adding
/// 1/49 of the range 49 times does not arrive at 1.0 — the error is a few ulps
/// either way, and a step boundary missed by one ulp is a keypress that
/// visibly does nothing. The DX7's bank knob stalled that way.
#[must_use]
pub fn step_discrete(index: usize, value: f32, up: bool) -> f32 {
    let Some(count) = discrete_steps(index) else { return value };
    let current = selector(value, count);
    knob_for(
        if up { (current + 1).min(count - 1) } else { current.saturating_sub(1) },
        count,
    )
}

/// Label for a selector position, or `None` for a slider.
#[must_use]
pub fn discrete_label(index: usize, value: f32) -> Option<&'static str> {
    let count = discrete_steps(index)?;
    let step = selector(value, count);
    if index >= P_MOD_BASE {
        return Some(match (index - P_MOD_BASE) % 3 {
            0 => SOURCE_LABELS[step],
            _ => DEST_LABELS[step],
        });
    }
    Some(match index {
        P_PATCH => PATCH_LABELS[step],
        P_A_WAVE | P_B_WAVE | P_C_WAVE | P_D_WAVE => SHAPE_LABELS[step],
        P_A_SEQ | P_B_SEQ | P_C_SEQ | P_D_SEQ => SEQ_LABELS[step],
        P_A_TUNE | P_B_TUNE | P_C_TUNE | P_D_TUNE => TUNE_LABELS[step],
        P_D_MODE => ["audio", "mod", "mod lo"][step],
        P_LFO1_WAVE | P_LFO2_WAVE => LFO_SHAPE_LABELS[step],
        _ => return None,
    })
}

/// A slider's value in seconds, for the seven that measure time. `None` for
/// the ones that read as a percentage.
///
/// SEQ RATE is a rate and reports the *period* it works out to — how long one
/// tick of the sequence clock lasts — because a step length is the number a
/// player matches against a tempo, and "125 ms" is that number where "62%" is
/// not. It is the one control here whose readout falls as the knob rises.
#[must_use]
pub fn param_seconds(index: usize, value: f32) -> Option<f64> {
    match index {
        P_ATTACK1 | P_ATTACK2 => Some(attack_seconds(f64::from(value))),
        P_DECAY1 | P_RELEASE1 | P_DECAY2 | P_RELEASE2 => Some(decay_seconds(f64::from(value))),
        P_SEQ_RATE => Some(1.0 / seq_hz(f64::from(value))),
        _ => None,
    }
}

// ── Panel tapers ──

/// A slider into 0..1 with an exponential feel: `curve` sets how much of the
/// range is spent in the top half of the travel.
fn taper(curve: f64, slider: f64) -> f64 {
    (curve * slider.clamp(0.0, 1.0)).exp_m1() / curve.exp_m1()
}

/// Attack: 1 ms at the bottom to 6 s at the top.
const ATTACK_MIN: f64 = 0.001;
const ATTACK_MAX: f64 = 6.0;
const ATTACK_CURVE: f64 = 5.0;

fn attack_seconds(slider: f64) -> f64 {
    ATTACK_MIN + taper(ATTACK_CURVE, slider) * ATTACK_MAX
}

/// Decay and release share one taper: 2 ms to 12 s.
const DECAY_MIN: f64 = 0.002;
const DECAY_MAX: f64 = 12.0;
const DECAY_CURVE: f64 = 5.0;

fn decay_seconds(slider: f64) -> f64 {
    DECAY_MIN + taper(DECAY_CURVE, slider) * DECAY_MAX
}

/// LFO rate, exponential end to end: 0.05 Hz to 30 Hz.
const LFO_MIN_HZ: f64 = 0.05;
const LFO_MAX_HZ: f64 = 30.0;

fn lfo_hz(slider: f64) -> f64 {
    LFO_MIN_HZ * (LFO_MAX_HZ / LFO_MIN_HZ).powf(slider.clamp(0.0, 1.0))
}

/// The wave sequence clock, exponential end to end: 0.25 Hz to 32 Hz, which is
/// a tick every four seconds at the bottom and every 31 ms at the top.
///
/// Seven octaves, chosen around what a step length is *for*: at 120 BPM a
/// sixteenth is 8 Hz, an eighth 4 Hz and a quarter 2 Hz, so the band a player
/// spends their time in sits in the middle of the travel with three octaves
/// either side. The top is past rhythm and into timbre, where the crossfades
/// start to be heard as a waveform of their own — which is a thing the machine
/// this borrows from is known for, so it is left reachable rather than trimmed
/// off.
const SEQ_MIN_HZ: f64 = 0.25;
const SEQ_MAX_HZ: f64 = 32.0;
/// How many octaves the rate slider covers, which is `log2(max/min)` written
/// out so that the bank can name a rate without a logarithm. Held to the two
/// constants above by `the_sequence_clock_covers_the_range_it_claims`.
const SEQ_OCTAVES: f32 = 7.0;

fn seq_hz(slider: f64) -> f64 {
    SEQ_MIN_HZ * (SEQ_MAX_HZ / SEQ_MIN_HZ).powf(slider.clamp(0.0, 1.0))
}

/// The knob position that asks for `SEQ_MIN_HZ * 2^octaves`.
///
/// How a patch names its clock. The taper is exponential end to end, so an
/// octave above the bottom of the travel is an exact fraction of it — which
/// keeps the conversion `const`, and [`PARAM_DEFAULTS`] is a `const` derived
/// from the first row of the bank.
const fn seq_rate_at(octaves: f32) -> f32 {
    octaves / SEQ_OCTAVES
}

/// The knob position for a rate in hertz, for a caller that has one — a test,
/// an editor, an import. The inverse of [`seq_hz`].
#[must_use]
pub fn seq_rate_knob(hz: f32) -> f32 {
    let ratio = f64::from(hz).max(f64::MIN_POSITIVE) / SEQ_MIN_HZ;
    (ratio.log2() / f64::from(SEQ_OCTAVES)).clamp(0.0, 1.0) as f32
}

/// Oscillator D in its low-frequency mode, from the coarse tune selector:
/// 0.1 Hz at the bottom of the knob to 25.6 Hz at the top, 1.6 Hz in the
/// middle. The Minimoog's third oscillator in LO range, on the same knob that
/// tunes it when it is making sound.
fn mod_lo_hz(semitones: f64) -> f64 {
    0.1 * 2.0f64.powf((semitones + f64::from(TUNE_RANGE)) / 6.0)
}

/// Cutoff slider to Hz: 16 Hz to 16 kHz, exponential. Three decades, the same
/// span the Odyssey's panel legend prints, chosen so the top of the travel is
/// still well under Nyquist at 44.1 kHz — a corner placed above it folds, and
/// a filter whose sweep folds *closes* at the top of its own travel.
const CUTOFF_MIN_HZ: f64 = 16.0;
const CUTOFF_DECADES: f64 = 3.0;
/// How many octaves the cutoff slider covers end to end, which is what turns a
/// keyboard-follow amount into a slider offset.
const CUTOFF_OCTAVES: f64 = CUTOFF_DECADES * std::f64::consts::LOG2_10;

fn cutoff_hz(slider: f64) -> f64 {
    CUTOFF_MIN_HZ * 10.0f64.powf(CUTOFF_DECADES * slider.clamp(0.0, 1.0))
}

/// Coarse tune, in semitones either way. 49 positions from -24 to +24.
const TUNE_RANGE: i32 = 24;
const TUNE_STEPS: usize = (TUNE_RANGE * 2 + 1) as usize;

const TUNE_LABELS: [&str; TUNE_STEPS] = [
    "-24", "-23", "-22", "-21", "-20", "-19", "-18", "-17", "-16", "-15", "-14", "-13",
    "-12", "-11", "-10", "-9", "-8", "-7", "-6", "-5", "-4", "-3", "-2", "-1",
    "0",
    "+1", "+2", "+3", "+4", "+5", "+6", "+7", "+8", "+9", "+10", "+11", "+12",
    "+13", "+14", "+15", "+16", "+17", "+18", "+19", "+20", "+21", "+22", "+23", "+24",
];

fn tune_semitones(value: f32) -> f64 {
    f64::from(selector(value, TUNE_STEPS) as i32 - TUNE_RANGE)
}

const fn tune_knob(semitones: i32) -> f32 {
    knob_for((semitones + TUNE_RANGE) as usize, TUNE_STEPS)
}

/// Fine tune: 50 cents either side of the coarse setting.
const FINE_CENTS: f64 = 50.0;

fn fine_cents(value: f32) -> f64 {
    (f64::from(value) - 0.5) * 2.0 * FINE_CENTS
}

const fn fine_knob(cents: f32) -> f32 {
    cents / (2.0 * FINE_CENTS as f32) + 0.5
}

/// The pulse width knob runs from square down to a 5% sliver, which is where
/// the pulse stops having a fundamental worth speaking of.
const PW_MIN: f64 = 0.05;

fn pulse_width(slider: f64) -> f64 {
    0.5 - slider.clamp(0.0, 1.0) * (0.5 - PW_MIN)
}

/// A bipolar knob: the middle is zero and the ends are ±1.
fn bipolar(value: f32) -> f64 {
    (f64::from(value) - 0.5) * 2.0
}

const fn bipolar_knob(amount: f32) -> f32 {
    amount * 0.5 + 0.5
}

/// How far a full-scale modulation moves the pitch: an octave either way.
const PITCH_MOD_SEMITONES: f64 = 12.0;

// ── The wavetable bank ──
//
// Sixteen single-cycle waveforms, generated from the harmonic tables below.
// Sixteen because that is the DW-8000's count, and because a bank that has to
// be swept through by a knob and by the matrix wants to be small enough that
// every position is a different sound rather than a different shade of one.
//
// The content is chosen for what those machines are actually used for: two
// drawbar organs, a hollow woodwind, a reed, a brass, two vowels, a bell, an
// electric piano, a clavinet and a digital wash, with the four analog shapes
// at the bottom of the bank so a sweep starts somewhere familiar. Nothing here
// is sampled, transcribed or otherwise taken from any instrument's ROM.

/// How many waveforms the bank holds.
pub const WAVE_COUNT: usize = 16;

/// What each waveform in the bank is, in bank order. The WAVE knob sweeps
/// through these and crossfades between neighbours, so the order is part of
/// the design: related timbres sit next to each other.
pub const WAVE_NAMES: [&str; WAVE_COUNT] = [
    "saw", "square", "triangle", "pulse 25", "pulse 10",
    "organ", "drawbar", "hollow", "reed", "brass",
    "vox ah", "vox oo", "bell", "e.piano", "clav", "digital",
];

/// Points per cycle. The sine that every partial is summed from is sampled at
/// exactly these points, so a partial's contribution is a table lookup rather
/// than a `sin` call: at sixteen waveforms by nine mip levels by 2048 points
/// the direct form would be seventeen million library calls at startup.
const TABLE_LEN: usize = 2048;

/// The highest harmonic the bank carries. At the bottom of a keyboard — MIDI
/// 36, 65 Hz — 256 harmonics reach 16.7 kHz, so nothing musically useful is
/// missing from the bottom octave and nothing above it needs this many.
const MAX_HARMONICS: usize = 256;

/// Band-limited copies, each with half the harmonics of the one below: 256,
/// 128, ... 1. A note picks the first level whose top harmonic is under
/// Nyquist, so the bank never aliases however high it is played.
const MIP_LEVELS: usize = 9;

/// What one generated waveform is made of.
///
/// Described by harmonic content rather than by a sample list because that is
/// how a machine with a wavetable ROM had its waveforms designed, and because
/// a spectrum can be band-limited exactly where a sample list cannot.
#[derive(Debug, Clone, Copy)]
enum Spectrum {
    /// Every harmonic at 1/n.
    Saw,
    /// The odd harmonics at 1/n.
    Square,
    /// The odd harmonics at 1/n², alternating in sign.
    Triangle,
    /// A rectangle of the given duty cycle.
    Pulse(f64),
    /// A named list of (harmonic, amplitude): the drawbar, bell and struck
    /// shapes, which are a handful of partials and nothing between them.
    Partials(&'static [(usize, f64)]),
    /// n^-slope up to `top`, over every harmonic or only the odd ones.
    Tilt { slope: f64, top: usize, odd: bool },
    /// A 1/sqrt(n) spectrum shaped by gaussian bumps at (centre harmonic,
    /// width, weight). The bumps sit at fixed *harmonics* rather than fixed
    /// frequencies, because a single-cycle waveform transposes with the note —
    /// which is exactly what a wavetable machine's vowels do.
    Formants(&'static [(f64, f64, f64)]),
    /// Deterministic pseudo-random amplitudes and phases up to `top`: the
    /// inharmonic-sounding wash a digital bank is expected to have somewhere.
    Digital { seed: u32, top: usize },
}

const WAVES: [Spectrum; WAVE_COUNT] = [
    Spectrum::Saw,
    Spectrum::Square,
    Spectrum::Triangle,
    Spectrum::Pulse(0.25),
    Spectrum::Pulse(0.10),
    // 8' + 4' + 2', the three-drawbar organ.
    Spectrum::Partials(&[(1, 1.0), (2, 0.7), (4, 0.5)]),
    // A fuller drawbar registration, with the quint and the twenty-second.
    Spectrum::Partials(&[(1, 1.0), (2, 0.8), (3, 0.6), (4, 0.5), (6, 0.35), (8, 0.25)]),
    // Odd harmonics only, falling fast: a stopped pipe or a clarinet.
    Spectrum::Tilt { slope: 1.3, top: 32, odd: true },
    // Odd harmonics, barely falling: the buzz of a reed.
    Spectrum::Tilt { slope: 0.5, top: 16, odd: true },
    // Energy peaking around the fourth harmonic, which is what makes a brass
    // instrument read as brass.
    Spectrum::Formants(&[(4.0, 3.0, 1.0)]),
    Spectrum::Formants(&[(3.0, 1.5, 1.0), (7.0, 2.5, 0.6)]),
    Spectrum::Formants(&[(1.5, 1.2, 1.0), (4.0, 1.0, 0.15)]),
    // Sparse, widely spaced partials: a struck bar.
    Spectrum::Partials(&[(1, 1.0), (5, 0.7), (9, 0.5), (13, 0.35), (17, 0.25)]),
    // A fundamental, two low partials and a bell-like pair up at the twelfth.
    Spectrum::Partials(&[(1, 1.0), (2, 0.35), (3, 0.12), (12, 0.28), (14, 0.12)]),
    // Everything, hardly falling at all: a clavinet's rasp.
    Spectrum::Tilt { slope: 0.3, top: 24, odd: false },
    Spectrum::Digital { seed: 0x9E37_79B9, top: 48 },
];

impl Spectrum {
    /// Amplitude and phase — the latter in table steps — of harmonic `n`.
    fn harmonic(self, n: usize) -> (f64, usize) {
        let nf = n as f64;
        const HALF: usize = TABLE_LEN / 2;
        const QUARTER: usize = TABLE_LEN / 4;
        match self {
            Self::Saw => (1.0 / nf, 0),
            Self::Square => (if n % 2 == 1 { 1.0 / nf } else { 0.0 }, 0),
            Self::Triangle => (
                if n % 2 == 1 { 1.0 / (nf * nf) } else { 0.0 },
                if (n / 2) % 2 == 1 { HALF } else { 0 },
            ),
            // The Fourier series of a rectangle of duty d, on a cosine basis.
            // Harmonics at multiples of 1/d vanish, which is what gives a
            // narrow pulse its hollow middle.
            Self::Pulse(duty) => ((nf * PI * duty).sin() / nf, QUARTER),
            Self::Partials(list) => {
                let mut amplitude = 0.0;
                for (harmonic, value) in list {
                    if *harmonic == n {
                        amplitude = *value;
                    }
                }
                (amplitude, 0)
            }
            Self::Tilt { slope, top, odd } => {
                if n > top || (odd && n % 2 == 0) {
                    (0.0, 0)
                } else {
                    (nf.powf(-slope), 0)
                }
            }
            Self::Formants(bumps) => {
                let mut amplitude = 0.0;
                for (centre, width, weight) in bumps {
                    let x = (nf - centre) / width;
                    amplitude += weight * (-x * x).exp();
                }
                (amplitude / nf.sqrt(), 0)
            }
            Self::Digital { seed, top } => {
                if n > top {
                    return (0.0, 0);
                }
                // A hash of the harmonic number rather than a running
                // generator, so the spectrum is the same whatever order the
                // harmonics are asked for.
                let mut state = seed ^ (n as u32).wrapping_mul(2_654_435_761);
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let amplitude = 0.25 + 0.75 * f64::from(state >> 16) / 65_535.0;
                let phase = (state >> 5) as usize & (TABLE_LEN - 1);
                (amplitude / nf.powf(0.6), phase)
            }
        }
    }
}

/// The Lanczos sigma factor for harmonic `n` of a series truncated at `top`.
///
/// A truncated Fourier series overshoots at its discontinuities by about 9% of
/// the step, whatever the truncation — Gibbs' phenomenon — and the overshoot
/// is what a peak-normalised table would end up scaled by. Tapering the
/// harmonics with `sinc(n/(top+1))` removes it, at the cost of very slightly
/// softening the edge.
fn sigma(n: usize, top: usize) -> f64 {
    if top <= 1 {
        return 1.0;
    }
    let x = PI * n as f64 / (top as f64 + 1.0);
    x.sin() / x
}

/// The generated bank: `WAVE_COUNT` waveforms, each in `MIP_LEVELS`
/// band-limited copies of `TABLE_LEN` points.
struct WaveBank {
    samples: Vec<f32>,
}

/// Built once per process, on whatever thread first constructs an instrument,
/// and shared by every instance from there. 1.2 MB, which is not worth a copy
/// per track.
///
/// The audio thread never touches the cell: `PhosphorSynth::new` resolves it
/// once and keeps the `&'static`, so `process` reads a plain reference — no
/// lock, no atomic, and nothing that can be the first caller.
static BANK_CELL: OnceLock<WaveBank> = OnceLock::new();

fn wave_bank() -> &'static WaveBank {
    BANK_CELL.get_or_init(WaveBank::generate)
}

impl WaveBank {
    fn generate() -> Self {
        let mut sine = [0.0f64; TABLE_LEN];
        for (i, value) in sine.iter_mut().enumerate() {
            *value = (TWO_PI * i as f64 / TABLE_LEN as f64).sin();
        }

        let mut samples = vec![0.0f32; WAVE_COUNT * MIP_LEVELS * TABLE_LEN];
        for (wave, spectrum) in WAVES.iter().enumerate() {
            let mut amplitude = [0.0f64; MAX_HARMONICS + 1];
            let mut phase = [0usize; MAX_HARMONICS + 1];
            for n in 1..=MAX_HARMONICS {
                let (a, p) = spectrum.harmonic(n);
                amplitude[n] = a;
                phase[n] = p;
            }

            // Every level is scaled by the *full* band's peak, so that playing
            // a note high enough to drop a level does not step its loudness.
            // A level whose own peak would then pass 1 is taken down further:
            // every waveform in the bank is bounded by 1, which is what the
            // vector mix's headroom argument rests on.
            let mut full_band_peak = 1.0f64;
            for level in 0..MIP_LEVELS {
                let top = (MAX_HARMONICS >> level).max(1);
                let base = (wave * MIP_LEVELS + level) * TABLE_LEN;
                let mut tapered = [0.0f64; MAX_HARMONICS + 1];
                for (n, value) in tapered.iter_mut().enumerate().take(top + 1).skip(1) {
                    *value = amplitude[n] * sigma(n, top);
                }

                let mut peak = 0.0f64;
                for i in 0..TABLE_LEN {
                    let mut acc = 0.0;
                    for (n, weight) in tapered.iter().enumerate().take(top + 1).skip(1) {
                        if *weight == 0.0 {
                            continue;
                        }
                        acc += weight * sine[(n * i + phase[n]) & (TABLE_LEN - 1)];
                    }
                    samples[base + i] = acc as f32;
                    peak = peak.max(acc.abs());
                }
                if level == 0 {
                    full_band_peak = peak.max(1e-12);
                }
                let scale = (1.0 / full_band_peak).min(1.0 / peak.max(1e-12)) as f32;
                for value in &mut samples[base..base + TABLE_LEN] {
                    *value *= scale;
                }
            }
        }
        Self { samples }
    }

    /// One waveform at one band limit, linearly interpolated.
    #[inline]
    fn sample(&self, wave: usize, level: usize, phase: f64) -> f64 {
        let base = (wave * MIP_LEVELS + level) * TABLE_LEN;
        let x = phase * TABLE_LEN as f64;
        let index = x as usize & (TABLE_LEN - 1);
        let frac = x - x.floor();
        let a = f64::from(self.samples[base + index]);
        let b = f64::from(self.samples[base + ((index + 1) & (TABLE_LEN - 1))]);
        a + frac * (b - a)
    }

    /// A *position* in the bank rather than a waveform from it: the knob
    /// crossfades between neighbours, which is what makes it worth pointing
    /// the matrix at.
    #[inline]
    fn at(&self, position: f64, level: usize, phase: f64) -> f64 {
        let p = position.clamp(0.0, 1.0) * (WAVE_COUNT - 1) as f64;
        let index = (p as usize).min(WAVE_COUNT - 1);
        let frac = p - index as f64;
        let a = self.sample(index, level, phase);
        if frac <= 0.0 {
            return a;
        }
        let b = self.sample((index + 1).min(WAVE_COUNT - 1), level, phase);
        a + frac * (b - a)
    }
}

/// Which band-limited copy a note at `freq` should read.
///
/// Level `l` holds harmonics up to `MAX_HARMONICS >> l`, so the answer is how
/// many times that has to be halved before the top harmonic fits under
/// Nyquist.
#[inline]
fn mip_level(freq: f64, sr: f64) -> usize {
    let need = MAX_HARMONICS as f64 * 2.0 * freq.abs() / sr;
    if need <= 1.0 {
        return 0;
    }
    (need.log2().ceil() as usize).min(MIP_LEVELS - 1)
}

// ── Oscillator ──

/// What one oscillator is making.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Saw = 0,
    Pulse = 1,
    Triangle = 2,
    Sine = 3,
    Table = 4,
    Noise = 5,
}

const SHAPE_COUNT: usize = 6;
const SHAPE_LABELS: [&str; SHAPE_COUNT] = ["saw", "pulse", "tri", "sine", "table", "noise"];

impl Shape {
    /// Total: the switch is a public parameter, so anything can arrive here.
    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Pulse,
            2 => Self::Triangle,
            3 => Self::Sine,
            4 => Self::Table,
            5 => Self::Noise,
            _ => Self::Saw,
        }
    }
}

/// What oscillator D is for.
///
/// The Minimoog's trade: its third oscillator can be taken out of the mixer
/// and pointed at the modulation bus instead, and giving up a third of the
/// sound to get a modulation source is part of how that instrument is
/// programmed. Here the oscillator leaves the vector mix — it is not
/// redistributed among the other three, so the patch gets quieter, exactly as
/// pulling a fader down would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DMode {
    /// In the mix, like the other three.
    Audio = 0,
    /// Out of the mix and on the matrix, tracking the keyboard.
    Mod = 1,
    /// Out of the mix and on the matrix, free of the keyboard and running at
    /// the low frequency the coarse tune knob sets.
    ModLo = 2,
}

impl DMode {
    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Mod,
            2 => Self::ModLo,
            _ => Self::Audio,
        }
    }
}

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

/// One oscillator. Free-running: the phase is not reset by a note-on, so two
/// voices playing the same note never quite agree and a held chord does not
/// have the phase-coherent attack a reset gives.
#[derive(Debug, Clone)]
struct Osc {
    phase: f64,
    noise: u32,
}

impl Osc {
    fn new(seed: u32) -> Self {
        // Seeded per oscillator, so the four noise sources in a voice and the
        // eight voices' worth of them are independent. A single shared source
        // would sum coherently across voices — eight times the amplitude on an
        // eight-note chord rather than the square root of eight.
        Self { phase: 0.0, noise: seed | 1 }
    }

    /// One step of the phase accumulator: where in the cycle it now sits, and
    /// how much of a cycle one sample covers — the latter being what the
    /// band-limited step needs.
    #[inline]
    fn advance(&mut self, freq: f64, sr: f64) -> (f64, f64) {
        let dt = (freq / sr).clamp(0.0, 0.45);
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }
        (self.phase, dt)
    }

    /// Two positions in the wavetable bank, crossfaded — what a wave sequence
    /// asks for while it is between steps.
    ///
    /// One phase accumulator rather than two, so the two waveforms are read at
    /// the same point of the same cycle: a crossfade between them is then a
    /// waveform in its own right rather than two oscillators beating. The
    /// second lookup is skipped outright when the crossfade is not running,
    /// which is most of every step.
    #[inline]
    fn tick_blend(&mut self, freq: f64, sr: f64, a: f64, b: f64, mix: f64, bank: &WaveBank) -> f64 {
        let (t, _) = self.advance(freq, sr);
        let level = mip_level(freq, sr);
        let first = bank.at(a, level, t);
        if mix <= 0.0 {
            return first;
        }
        first + mix * (bank.at(b, level, t) - first)
    }

    #[inline]
    fn tick(
        &mut self,
        shape: Shape,
        freq: f64,
        sr: f64,
        pulse_width: f64,
        table_pos: f64,
        bank: &WaveBank,
    ) -> f64 {
        if shape == Shape::Noise {
            self.noise = self.noise.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            return f64::from(self.noise as i32) / f64::from(i32::MAX);
        }

        let (t, dt) = self.advance(freq, sr);

        match shape {
            Shape::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Shape::Pulse => {
                let pw = pulse_width.clamp(PW_MIN, 1.0 - PW_MIN);
                let mut value = if t < pw { 1.0 } else { -1.0 };
                value += poly_blep(t, dt);
                value -= poly_blep((t - pw).rem_euclid(1.0), dt);
                value
            }
            // Naive, and deliberately: a triangle's harmonics fall as 1/n², so
            // what folds back sits 30 dB or more under the fundamental even at
            // the top of the keyboard.
            Shape::Triangle => {
                if t < 0.5 {
                    4.0 * t - 1.0
                } else {
                    3.0 - 4.0 * t
                }
            }
            Shape::Sine => (TWO_PI * t).sin(),
            _ => bank.at(table_pos, mip_level(freq, sr), t),
        }
    }
}

// ── Wave sequencing ──
//
// A step list per oscillator, walked on a clock, crossfading between
// neighbours. The Wavestation's defining feature, and the reason it still
// sounds like nothing else: an envelope shapes one waveform, where a sequence
// replaces it.
//
// The shape of the thing, and why:
//
// * **a step names a waveform by index**, not a position between two. The
//   WAVE knob is already a continuous sweep through the bank and the matrix
//   can already move it; what a sequence wants instead is to *land* on a
//   waveform, so the step list reads as a list of sounds rather than of
//   fractions.
// * **a step's length is in ticks of a clock**, not in seconds. One knob then
//   moves a whole pattern without changing its rhythm.
// * **a step's crossfade is a fraction of its own length**, not a time. Speed
//   the clock up and the fades scale with the steps instead of swallowing
//   them, and the fraction is bounded to 0..1 by construction, so the
//   per-sample form needs no clamp.
// * **a step carries a pitch offset and a level.** Pitch is what makes a
//   sequence a riff rather than a texture; level at zero is a rest, which is
//   what makes it a rhythm. Both are crossfaded along with the waveform, so a
//   step with a long fade glides into the next and one with none snaps.
// * **the waveform column is the only one that needs a wavetable oscillator.**
//   The WAVE switch wins: pointing a sequence at a sawtooth gives a sawtooth
//   riff with rests, which is half of what `SEQ RIFF` is made of, and it is
//   why that switch is not dead on a sequenced oscillator. The cost is that a
//   sequence which varies *nothing but* its waveform — `morph 8` is the one in
//   this bank — is exactly inert on any shape but `table`, and nothing on the
//   panel says so. `a_waveform_only_sequence_needs_an_oscillator_that_reads_
//   waveforms` asserts it in both directions so that it is at least a stated
//   property rather than a surprise.
// * **the cursor is per voice and per oscillator**, and starts at note-on. A
//   free-running sequence would put every note at a different point of the
//   pattern, which is what a sequence shared by the whole instrument sounds
//   like and is not what this is for. The oscillator *phase* stays free
//   running, as it was.
//
// Everything the per-sample path touches is resolved when a step ends: the
// current step and the one it fades into, in engine units, along with the
// reciprocals that turn the crossfade into a multiply. What is left per sample
// is an add, a compare, two lerps and — only while a fade is actually running
// — a second wavetable lookup.

/// The most steps a sequence may carry.
///
/// Nothing in the engine needs a bound: the cursor indexes rather than scans,
/// so a longer list costs no more per sample. It is here for the editor that
/// will eventually build one of these, and it is asserted against the bank so
/// that the two cannot drift.
pub const MAX_STEPS: usize = 32;

/// How far the cursor may move in one sample, as a fraction of a step.
///
/// A half, so a step lasts at least two samples, the cursor crosses at most
/// one boundary per sample, and advancing it is an `if` rather than a loop —
/// bounded work per sample, with no "drain everything pending" pattern that a
/// high enough rate could turn into a stall. A step every two samples is
/// 22 kHz of sequence, which is well past the point where the clock has
/// stopped being a clock at all.
const MAX_STEP_ADVANCE: f64 = 0.5;

/// How far the matrix can push the clock, in octaves either way at full depth.
const SEQ_RATE_OCTAVES: f64 = 3.0;

/// One step of a wave sequence.
#[derive(Debug, Clone, Copy)]
struct Step {
    /// Which waveform in the bank, by index. Read only by an oscillator whose
    /// WAVE switch says `table` — on any other shape the step still sets the
    /// pitch, the level and the timing, so a sequence works on a sawtooth as
    /// a riff even though it has no waveform to give it.
    wave: u8,
    /// How long the step lasts, in ticks of the sequence clock. Zero reads as
    /// one: a step of no length would be a cursor that never leaves it.
    ticks: u8,
    /// How much of the step is spent crossfading into the next, as a fraction
    /// of the step's own length. Zero is a hard cut.
    fade: f32,
    /// Semitones from the note's own pitch.
    pitch: i8,
    /// What the step is worth in the mix, 0..1. Zero is a rest.
    level: f32,
}

/// One wave sequence: a name, a step list, and what happens at the end.
#[derive(Debug, Clone, Copy)]
struct SeqChart {
    /// Cut to the twelve columns the editor's selector row leaves for a label.
    name: &'static str,
    steps: &'static [Step],
    /// Whether the list repeats, or plays once and holds its last step.
    ///
    /// One-shot is not a lesser loop: three short steps that resolve into a
    /// fourth and stay there is an attack transient no envelope can make,
    /// because what changes across it is the waveform.
    looping: bool,
}

/// A step in the units the voice runs on.
#[derive(Debug, Clone, Copy)]
struct StepVoice {
    /// Position in the wavetable bank, 0..1.
    wave: f64,
    level: f64,
    /// Frequency multiplier from the step's pitch offset.
    ratio: f64,
}

/// What a cursor with no sequence on it asks for: the oscillator as the panel
/// has it, at its own pitch and its own level.
const STEP_UNITY: StepVoice = StepVoice { wave: 0.0, level: 1.0, ratio: 1.0 };

impl Step {
    fn resolve(self) -> StepVoice {
        StepVoice {
            wave: wave_position(self.wave),
            level: f64::from(self.level).clamp(0.0, 1.0),
            ratio: 2.0f64.powf(f64::from(self.pitch) / 12.0),
        }
    }
}

/// Where waveform `index` of the bank sits on the 0..1 position the TABLE knob
/// and the matrix both work in.
fn wave_position(index: u8) -> f64 {
    let top = (WAVE_COUNT - 1) as u8;
    f64::from(index.min(top)) / f64::from(top)
}

/// What one oscillator's sequence is asking for, this sample.
#[derive(Debug, Clone, Copy)]
struct SeqPoint {
    /// The step's waveform and the one it is fading into, as bank positions.
    wave: [f64; 2],
    /// How far between them, 0..1. Zero outside a crossfade, which is most of
    /// every step and is the case that costs one wavetable lookup instead of
    /// two.
    mix: f64,
    level: f64,
    ratio: f64,
}

const SEQ_IDLE: SeqPoint = SeqPoint { wave: [0.0; 2], mix: 0.0, level: 1.0, ratio: 1.0 };

/// One oscillator's place in one sequence.
#[derive(Debug, Clone, Copy)]
struct SeqCursor {
    /// The selector position this cursor was started from. Zero is off.
    ///
    /// Kept so that a selector moved while a note is sounding restarts that
    /// oscillator's sequence rather than being ignored until the next note —
    /// the panel's promise is that every control is live, and a knob that only
    /// takes effect on the next note-on is not.
    slot: usize,
    seq: Option<&'static SeqChart>,
    index: usize,
    /// How far through the current step, 0..1.
    phase: f64,
    /// How far `phase` moves per tick of the clock: 1/ticks.
    per_tick: f64,
    /// Where in the step the crossfade begins, and the reciprocal of what is
    /// left after it, so the fade is a subtract and a multiply rather than a
    /// divide.
    fade_from: f64,
    fade_scale: f64,
    now: StepVoice,
    next: StepVoice,
    /// A one-shot that has reached its last step. The clock stops rather than
    /// running on against a step it can never leave.
    held: bool,
}

impl SeqCursor {
    const OFF: Self = Self {
        slot: 0,
        seq: None,
        index: 0,
        phase: 0.0,
        per_tick: 1.0,
        fade_from: 1.0,
        fade_scale: 0.0,
        now: STEP_UNITY,
        next: STEP_UNITY,
        held: true,
    };

    /// Point a cursor at selector position `slot`, on step zero.
    ///
    /// The one place a step list is looked up. Bounded and allocation-free —
    /// an index into a `'static` table and a copy of two of its rows — and it
    /// runs at note-on, which is where [`KeyChart`] is resolved and for the
    /// same reason.
    fn start(slot: usize) -> Self {
        let Some(seq) = sequence_at(slot) else { return Self::OFF };
        if seq.steps.is_empty() {
            return Self::OFF;
        }
        let mut cursor = Self { slot, seq: Some(seq), held: false, ..Self::OFF };
        cursor.load();
        cursor
    }

    fn is_running(self) -> bool {
        self.seq.is_some()
    }

    /// Resolve the current step and the one it fades into.
    ///
    /// Off the per-sample path: it runs when a step ends, which at the top of
    /// the rate travel is every 1400 samples and at the bottom every few
    /// seconds. That is what buys the two `powf` calls in [`Step::resolve`].
    fn load(&mut self) {
        let Some(seq) = self.seq else { return };
        let steps = seq.steps;
        let step = steps[self.index.min(steps.len() - 1)];
        self.per_tick = 1.0 / f64::from(step.ticks.max(1));
        let fade = f64::from(step.fade).clamp(0.0, 1.0);
        self.fade_from = 1.0 - fade;
        self.fade_scale = if fade > 0.0 { 1.0 / fade } else { 0.0 };
        self.now = step.resolve();
        // What it fades into: the next step, the first again on a loop, or —
        // on the last step of a one-shot — itself, which is to say nothing.
        let following = match steps.get(self.index + 1) {
            Some(next) => *next,
            None if seq.looping => steps[0],
            None => step,
        };
        self.next = following.resolve();
    }

    /// One sample of the clock.
    ///
    /// Total and bounded: the increment is clamped, so at most one step
    /// boundary is crossed per sample and `phase` cannot leave 0..1; and a
    /// rate that arrives as an infinity or a NaN stalls the cursor rather than
    /// poisoning it, which matters because `params` is public and the rate
    /// knob can be set to anything at all.
    fn advance(&mut self, hz: f64, sr: f64) -> SeqPoint {
        if self.seq.is_none() {
            return SEQ_IDLE;
        }
        if !self.held {
            let step = hz * self.per_tick / sr;
            let step = if step.is_finite() { step.clamp(0.0, MAX_STEP_ADVANCE) } else { 0.0 };
            self.phase += step;
            if self.phase >= 1.0 {
                // The leftover is carried in the *old* step's units rather
                // than rescaled into the new one's. At the top of the rate
                // travel a step is 1400 samples, so the error a step of a
                // different length inherits is a fraction of one sample, and
                // it does not accumulate: the next boundary carries its own
                // leftover and nothing sums them.
                self.phase -= 1.0;
                self.next_step();
            }
        }
        let mix = if self.phase > self.fade_from {
            ((self.phase - self.fade_from) * self.fade_scale).min(1.0)
        } else {
            0.0
        };
        SeqPoint {
            wave: [self.now.wave, self.next.wave],
            mix,
            level: self.now.level + mix * (self.next.level - self.now.level),
            ratio: self.now.ratio + mix * (self.next.ratio - self.now.ratio),
        }
    }

    fn next_step(&mut self) {
        let Some(seq) = self.seq else { return };
        if self.index + 1 < seq.steps.len() {
            self.index += 1;
        } else if seq.looping {
            self.index = 0;
        } else {
            self.phase = 0.0;
            self.held = true;
            return;
        }
        self.load();
    }
}

/// Which sequence a selector position names. Position zero is off.
fn sequence_at(slot: usize) -> Option<&'static SeqChart> {
    slot.checked_sub(1).and_then(|index| SEQ_BANK.get(index))
}

/// How many sequences the bank holds.
pub const SEQ_COUNT: usize = 8;

/// How many positions an oscillator's sequence selector has: "off", then one
/// per sequence in the bank.
pub const SEQ_SLOTS: usize = SEQ_COUNT + 1;

/// The sequences.
///
/// Eight, and chosen so that no two of them do the same job: a gate, a morph,
/// a vowel drift, a riff, two one-shots, an odd-length shuffle and a stab.
/// Their lengths are 4, 8, 10, 4, 6 and 8 ticks, which is deliberate — four
/// oscillators pointed at four of these do not come back into step for the
/// length of their least common multiple, and that is the whole reason for
/// sequencing per oscillator rather than per voice.
const SEQ_BANK: [SeqChart; SEQ_COUNT] = [
    // A rhythm and nothing else: one waveform, gated on and off. It works on
    // any WAVE setting, which is what makes it the one to reach for on an
    // oscillator that is already the sound the patch wants.
    SeqChart {
        name: "gate 4",
        looping: true,
        steps: &[
            Step { wave: 5, ticks: 1, fade: 0.06, pitch: 0, level: 1.0 },
            Step { wave: 5, ticks: 1, fade: 0.06, pitch: 0, level: 0.0 },
            Step { wave: 5, ticks: 1, fade: 0.06, pitch: 0, level: 0.85 },
            Step { wave: 5, ticks: 1, fade: 0.06, pitch: 0, level: 0.0 },
        ],
    },
    // The other extreme: eight waveforms with the crossfade set to the whole
    // step, so the bank is walked continuously and there is no step to hear at
    // all — only a timbre that never stops moving.
    //
    // The one sequence in the bank that carries nothing but waveforms — every
    // step is at full level and at the note's own pitch — so it is the one
    // that does nothing at all on an oscillator whose WAVE switch is not on
    // `table`. That is the design working rather than failing, but it is the
    // trap in this bank and it is worth knowing which entry it is.
    SeqChart {
        name: "morph 8",
        looping: true,
        steps: &[
            Step { wave: 5, ticks: 1, fade: 1.0, pitch: 0, level: 1.0 },
            Step { wave: 6, ticks: 1, fade: 1.0, pitch: 0, level: 1.0 },
            Step { wave: 7, ticks: 1, fade: 1.0, pitch: 0, level: 1.0 },
            Step { wave: 8, ticks: 1, fade: 1.0, pitch: 0, level: 1.0 },
            Step { wave: 9, ticks: 1, fade: 1.0, pitch: 0, level: 1.0 },
            Step { wave: 10, ticks: 1, fade: 1.0, pitch: 0, level: 1.0 },
            Step { wave: 11, ticks: 1, fade: 1.0, pitch: 0, level: 1.0 },
            Step { wave: 12, ticks: 1, fade: 1.0, pitch: 0, level: 1.0 },
        ],
    },
    // Two vowels and two reeds on unequal steps, half-crossfaded: a voice that
    // keeps changing its mind. Ten ticks, so it walks against anything in four
    // or eight.
    SeqChart {
        name: "vox 4",
        looping: true,
        steps: &[
            Step { wave: 10, ticks: 3, fade: 0.6, pitch: 0, level: 1.0 },
            Step { wave: 11, ticks: 2, fade: 0.6, pitch: 0, level: 1.0 },
            Step { wave: 7, ticks: 3, fade: 0.6, pitch: 0, level: 0.95 },
            Step { wave: 8, ticks: 2, fade: 0.6, pitch: 0, level: 0.9 },
        ],
    },
    // A riff: the clavinet, four pitches, and just enough crossfade to keep
    // the step boundaries from clicking.
    SeqChart {
        name: "riff 5th",
        looping: true,
        steps: &[
            Step { wave: 14, ticks: 1, fade: 0.05, pitch: 0, level: 1.0 },
            Step { wave: 14, ticks: 1, fade: 0.05, pitch: 7, level: 0.9 },
            Step { wave: 12, ticks: 1, fade: 0.05, pitch: 12, level: 1.0 },
            Step { wave: 14, ticks: 1, fade: 0.05, pitch: 3, level: 0.85 },
        ],
    },
    // Played once: a bell and a clavinet an octave and a fifth up, resolving
    // into a drawbar organ that is then held for as long as the key is. The
    // attack of an instrument that does not exist.
    SeqChart {
        name: "attack",
        looping: false,
        steps: &[
            Step { wave: 12, ticks: 1, fade: 0.3, pitch: 12, level: 1.0 },
            Step { wave: 14, ticks: 1, fade: 0.3, pitch: 7, level: 0.95 },
            Step { wave: 6, ticks: 1, fade: 0.3, pitch: 0, level: 0.9 },
        ],
    },
    // The same idea over a wider run: four bells falling two octaves into a
    // held organ. One-shot, so it is an attack rather than a pattern.
    SeqChart {
        name: "bell run",
        looping: false,
        steps: &[
            Step { wave: 12, ticks: 1, fade: 0.1, pitch: 24, level: 0.9 },
            Step { wave: 12, ticks: 1, fade: 0.1, pitch: 19, level: 0.9 },
            Step { wave: 12, ticks: 1, fade: 0.1, pitch: 12, level: 0.95 },
            Step { wave: 13, ticks: 1, fade: 0.2, pitch: 7, level: 1.0 },
            Step { wave: 5, ticks: 4, fade: 0.2, pitch: 0, level: 1.0 },
        ],
    },
    // Three drawbar registrations on 3, 2 and 1 ticks: six ticks, which is the
    // odd length in the bank and the one that makes two sequences drift.
    SeqChart {
        name: "organ 3",
        looping: true,
        steps: &[
            Step { wave: 6, ticks: 3, fade: 0.25, pitch: 0, level: 1.0 },
            Step { wave: 5, ticks: 2, fade: 0.25, pitch: 0, level: 0.9 },
            Step { wave: 7, ticks: 1, fade: 0.25, pitch: 0, level: 0.8 },
        ],
    },
    // Brass stabs with the rests written in, an octave drop and a fourth up:
    // eight ticks of rhythm and pitch together, which is a sequence doing the
    // one thing an envelope and an LFO cannot do between them.
    SeqChart {
        name: "stab",
        looping: true,
        steps: &[
            Step { wave: 9, ticks: 1, fade: 0.08, pitch: 0, level: 1.0 },
            Step { wave: 9, ticks: 1, fade: 0.08, pitch: 0, level: 0.0 },
            Step { wave: 9, ticks: 2, fade: 0.08, pitch: -12, level: 0.9 },
            Step { wave: 15, ticks: 1, fade: 0.08, pitch: 0, level: 0.0 },
            Step { wave: 9, ticks: 1, fade: 0.08, pitch: 5, level: 0.95 },
            Step { wave: 9, ticks: 2, fade: 0.08, pitch: 0, level: 0.0 },
        ],
    },
];

/// The sequence selector's labels: "off", then the bank in order.
const SEQ_LABELS: [&str; SEQ_SLOTS] = derive_seq_labels();

const fn derive_seq_labels() -> [&'static str; SEQ_SLOTS] {
    let mut out = ["off"; SEQ_SLOTS];
    let mut i = 0;
    while i < SEQ_COUNT {
        out[i + 1] = SEQ_BANK[i].name;
        i += 1;
    }
    out
}

/// The knob position that points an oscillator at sequence `index`, or at
/// nothing when `index` is `None`.
///
/// The midpoint of the selector's step, which is the one position in it that
/// no amount of float rounding can push into a neighbour — the same rule
/// [`patch_knob`] follows.
#[must_use]
pub fn seq_knob(index: Option<usize>) -> f32 {
    let slot = index.map_or(0, |i| i.min(SEQ_COUNT - 1) + 1);
    knob_for(slot, SEQ_SLOTS)
}

/// Which sequence a selector knob names, or `None` for off.
#[must_use]
pub fn seq_index(value: f32) -> Option<usize> {
    selector(value, SEQ_SLOTS).checked_sub(1)
}

/// The name of sequence `index`.
#[must_use]
pub fn seq_name(index: usize) -> &'static str {
    SEQ_BANK[index.min(SEQ_COUNT - 1)].name
}

/// How many steps sequence `index` has.
#[must_use]
pub fn seq_step_count(index: usize) -> usize {
    SEQ_BANK[index.min(SEQ_COUNT - 1)].steps.len()
}

/// Whether sequence `index` repeats, or plays once and holds its last step.
#[must_use]
pub fn seq_loops(index: usize) -> bool {
    SEQ_BANK[index.min(SEQ_COUNT - 1)].looping
}

// ── Drive ──

/// The signal level the DRIVE knob holds still, in the units it sees them:
/// the vector mix, before the filter, before the envelope and the velocity.
///
/// The knob preserves the level of a signal *at* this amplitude exactly, lifts
/// what is quieter towards it and holds down what is louder — which is what a
/// compressor does, and what makes a synth sound driven rather than just loud.
///
/// Measured against this instrument rather than picked. The vector weights sum
/// to one, so the mix of four oscillators is bounded by the largest level knob
/// whatever the vector is doing — a panel with everything at the top measures
/// 0.93 to 1.00 into the drive stage, and the eight melodic patches in the
/// bank measure 0.30 (RESO DRONE, whose sources are two whispers of noise) to
/// 0.83 (INIT SAW and VECTOR SWEEP).
///
/// The reference sits below the middle of that band rather than in it, because
/// holding one voice's peak still is not the same as holding a chord still:
/// everything under that peak comes up towards it, and eight voices summed
/// carry the rise. What it costs is at the other end — a patch well above the
/// reference is held down rather than driven — and that is the direction that
/// helps, because a patch well above the reference is a loud one.
const DRIVE_REFERENCE: f64 = 0.5;

/// How hard the knob drives at the top of its travel. `1 + DRIVE_DEPTH *
/// DRIVE_REFERENCE` is the gain a signal far below the reference gets, which
/// at these two numbers is 7 — a little over 16 dB, which is about what a
/// Minimoog's mixer has above unity.
const DRIVE_DEPTH: f64 = 12.0;

/// The DRIVE knob's waveshaper: harmonics without level.
///
/// ```text
/// a = amount * DRIVE_DEPTH
/// y = x * (1 + a*r) / (1 + a*|x|),   r = DRIVE_REFERENCE
/// ```
///
/// The same curve the drum rack's DRIVE uses — `drive_stage_at` in
/// drum_rack/mod.rs — and for the same four reasons:
///
/// * **Identity at zero.** `a = 0` gives `y = x` exactly, in f64 and in f32,
///   so the bottom of the knob is the patch as voiced rather than a patch with
///   a waveshaper switched into it.
/// * **Level-preserving at the reference.** `|x| = r` gives `|y| = r` for
///   every `a`, so the knob is a tone control rather than a fader.
/// * **Monotonic and odd.** `dy/dx = (1 + a*r)/(1 + a|x|)^2 > 0`, so the curve
///   never folds back, and `y(-x) = -y(x)`, so it makes odd harmonics rather
///   than a DC offset the mixer would pass straight through.
/// * **Bounded.** `|y| < r + 1/a` for `a > 0`.
///
/// The knob this replaced had none of the last three. It multiplied by up to
/// eleven before its own denominator, so an eight-note chord went from 0.346
/// to 0.901 across its travel, +8.3 dB and past the master limiter's ceiling;
/// and past that denominator's knee it took more level off than the knob put
/// on, so on a loud patch turning DRIVE up from zero made it *quieter* — 0.947
/// at drive 0 and 0.888 at drive 0.1 — and then louder again.
#[inline]
fn drive_stage(x: f64, amount: f64) -> f64 {
    drive_stage_at(x, amount, DRIVE_REFERENCE)
}

#[inline]
fn drive_stage_at(x: f64, amount: f64, reference: f64) -> f64 {
    let a = amount * DRIVE_DEPTH;
    x * (1.0 + a * reference) / (1.0 + a * x.abs())
}

// ── The ladder filter ──
//
// Four one-pole sections round a feedback loop, which is a transistor ladder's
// own topology. The integrators are topology-preserving (the `g/(1+g)` form),
// so a section's pole lands on the frequency it was asked for; the naive
// `s += g*(x-s)` form sits an octave below its own coefficient and takes the
// whole cascade with it — that is what put the Juno's corner 2.3x low and the
// Jupiter's 1.95x before both were rewritten this way.
//
// There is no compensation on the input. That is the point: the resonance
// feedback is subtracted from the signal, so the passband drops away as the
// resonance comes up, and that loss is what a ladder sounds like. Putting it
// back is what makes a clone sound wrong.
//
// Measured, at 44.1 kHz, with `measure_filter`:
//
// * the corner lands where the slider says. Four analog poles are 12 dB down
//   at their own corner, and the -12 dB point measures 32.6 Hz where the
//   slider asks for 31.9, 63.9 for 63.7, 179.6 for 179.5, 506.7 for 506.0 and
//   2017 for 2014 — within 2.2% at the bottom of the sweep and 0.1% over the
//   rest of it. The -3 dB point sits at 0.44 of the corner, which is where
//   four cascaded poles put it;
// * the slope is 23.3 dB/octave measured between four and eight times the
//   corner, approaching the 24 dB/octave asymptote from below as the analog
//   cascade does. Above about 4 kHz the figure steepens, which is the
//   bilinear transform pulling the response to zero at Nyquist rather than a
//   fifth pole;
// * it oscillates. With no input at all the tail two seconds later measures
//   0.114 at resonance 0.90, 0.161 at 0.95 and 0.192 at the top of the
//   travel, and is exactly zero at 0.85 and below;
// * it loses its bass, monotonically: at a 1 kHz corner, a 30 Hz sine comes
//   out 6.5 dB down at a quarter of the resonance travel, 10.2 dB at half,
//   13.7 dB at 0.85 and 15.3 dB at the top.

/// How much feedback the loop can be given. Four correctly-placed poles reach
/// half a turn of phase with a quarter of the gain left, so 4.0 is exactly
/// marginal and the top of the travel has to sit past it to oscillate.
const LADDER_RES_MAX: f64 = 4.5;

/// Where on the resonance travel the loop stops losing and starts producing.
const SELF_OSC_KNEE: f64 = 0.9;

/// How much of its own oscillation a note starts the filter with, at the top
/// of the resonance travel.
///
/// A filter with no state and no input stays silent forever however negative
/// its damping is, because the loop multiplies zero by four and gets zero. A
/// patch whose sound source *is* the filter — and this bank has one — would
/// otherwise come out silent.
const SELF_OSC_SEED: f64 = 0.05;

#[derive(Debug, Clone)]
struct Ladder {
    s: [f64; 4],
}

impl Ladder {
    fn new() -> Self {
        Self { s: [0.0; 4] }
    }

    fn process(&mut self, input: f64, cutoff_norm: f64, resonance: f64, sr: f64) -> f64 {
        let g = (PI * cutoff_hz(cutoff_norm).min(sr * 0.49) / sr).tan();
        let gg = g / (1.0 + g);
        let res = resonance.clamp(0.0, 1.0) * LADDER_RES_MAX;
        let mut x = tanh_approx(input - res * tanh_approx(self.s[3]));

        for s in &mut self.s {
            let v = (x - *s) * gg;
            let y = v + *s;
            *s = y + v;
            if s.abs() < 1e-18 {
                *s = 0.0;
            }
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

// ── Envelopes ──
//
// Two per voice, both four-stage. Envelope 1 is wired to the amplifier and
// envelope 2 to the filter, and both are on the matrix as well, so the wiring
// is a starting point rather than a limit.
//
// Every segment is a capacitor charging towards something, which is why none
// of them are straight lines: attack charges towards 1.58 and ends when it
// passes 1.0, which is the first time constant of an exponential; decay and
// release charge a little past their target and stop when they reach it, which
// is 3.5 time constants across the segment and makes the slider's number the
// time the segment actually takes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
/// Time constants spanned by a decay or release segment.
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

#[derive(Debug, Clone, Copy)]
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
    /// when a slider moves. The exponential in `env_rate` is not something to
    /// evaluate sixteen times a sample for an answer that changes when a
    /// finger does.
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

    /// Follow the panel. A no-op unless a time slider moved, which is what
    /// keeps the exponentials off the per-sample path.
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

    fn is_held(&self) -> bool {
        matches!(self.stage, EnvStage::Attack | EnvStage::Decay | EnvStage::Sustain)
    }

    fn enter_decay(&mut self) {
        self.stage = EnvStage::Decay;
        self.aim = self.times.sustain - ENV_UNDERSHOOT * (self.level - self.times.sustain);
    }

    /// The decay has reached the sustain level.
    ///
    /// A sustain of zero means the segment ended in silence, so the note is
    /// *finished* rather than holding a voice at nothing until the key comes
    /// up. That matters for the percussive end of the bank and for keymapped
    /// patches especially: a drum has no sustain by definition, and eight
    /// voices held open by eight drum notes nobody has let go of is a kit that
    /// stops responding after eight hits.
    fn enter_sustain(&mut self) {
        self.level = self.times.sustain;
        self.stage = if self.times.sustain <= 0.0 {
            EnvStage::Idle
        } else {
            EnvStage::Sustain
        };
    }

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
            EnvStage::Sustain => self.times.sustain,
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

// ── LFOs ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LfoShape {
    Triangle = 0,
    Sine = 1,
    Saw = 2,
    Square = 3,
    SampleHold = 4,
}

const LFO_SHAPE_COUNT: usize = 5;
const LFO_SHAPE_LABELS: [&str; LFO_SHAPE_COUNT] = ["tri", "sine", "saw", "square", "s&h"];

impl LfoShape {
    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Sine,
            2 => Self::Saw,
            3 => Self::Square,
            4 => Self::SampleHold,
            _ => Self::Triangle,
        }
    }
}

/// One free-running LFO, shared by every voice.
///
/// Free-running rather than retriggered per note, which is what a Minimoog and
/// a Juno both do: a second note added to a held chord joins the vibrato
/// already in progress rather than starting its own.
#[derive(Debug, Clone)]
struct Lfo {
    phase: f64,
    held: f64,
    noise: u32,
}

impl Lfo {
    fn new(seed: u32) -> Self {
        Self { phase: 0.0, held: 0.0, noise: seed | 1 }
    }

    fn tick(&mut self, shape: LfoShape, rate: f64, sr: f64) -> f64 {
        self.phase += rate / sr;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            if shape == LfoShape::SampleHold {
                self.noise = self.noise.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                self.held = f64::from(self.noise as i32) / f64::from(i32::MAX);
            }
        }
        let t = self.phase;
        match shape {
            LfoShape::Triangle => {
                if t < 0.5 {
                    4.0 * t - 1.0
                } else {
                    3.0 - 4.0 * t
                }
            }
            LfoShape::Sine => (TWO_PI * t).sin(),
            LfoShape::Saw => 2.0 * t - 1.0,
            LfoShape::Square => {
                if t < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoShape::SampleHold => self.held,
        }
    }
}

// ── The modulation matrix ──

/// What a slot can listen to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Off = 0,
    Lfo1 = 1,
    Lfo2 = 2,
    Env1 = 3,
    Env2 = 4,
    Velocity = 5,
    KeyTrack = 6,
    Wheel = 7,
    VectorX = 8,
    VectorY = 9,
    OscD = 10,
}

const SOURCE_COUNT: usize = 11;
const SOURCE_LABELS: [&str; SOURCE_COUNT] = [
    "off", "lfo 1", "lfo 2", "env 1", "env 2", "vel", "kybd", "wheel", "vec x", "vec y", "osc d",
];

impl Source {
    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Lfo1,
            2 => Self::Lfo2,
            3 => Self::Env1,
            4 => Self::Env2,
            5 => Self::Velocity,
            6 => Self::KeyTrack,
            7 => Self::Wheel,
            8 => Self::VectorX,
            9 => Self::VectorY,
            10 => Self::OscD,
            _ => Self::Off,
        }
    }

    /// Whether the source swings either side of zero. The amplitude
    /// destination needs to know, because it folds its source to unipolar
    /// before applying it; nothing else does.
    fn is_bipolar(self) -> bool {
        matches!(self, Self::Lfo1 | Self::Lfo2 | Self::KeyTrack | Self::OscD)
    }
}

/// What a slot can move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dest {
    Off = 0,
    Pitch = 1,
    PulseWidth = 2,
    Wave = 3,
    Cutoff = 4,
    Resonance = 5,
    Amplitude = 6,
    VectorX = 7,
    VectorY = 8,
    /// The wave sequence clock, in octaves either way — so an envelope can
    /// start a pattern fast and let it settle, and an LFO can swing it.
    SeqRate = 9,
}

const DEST_COUNT: usize = 10;
const DEST_LABELS: [&str; DEST_COUNT] = [
    "off", "pitch", "pw", "wave", "cutoff", "reso", "amp", "vec x", "vec y", "seq hz",
];

impl Dest {
    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Pitch,
            2 => Self::PulseWidth,
            3 => Self::Wave,
            4 => Self::Cutoff,
            5 => Self::Resonance,
            6 => Self::Amplitude,
            7 => Self::VectorX,
            8 => Self::VectorY,
            9 => Self::SeqRate,
            _ => Self::Off,
        }
    }
}

/// One routing, resolved out of three knobs.
#[derive(Debug, Clone, Copy)]
struct Slot {
    source: Source,
    dest: Dest,
    amount: f64,
    bipolar: bool,
}

/// Everything a voice needs to know about the world outside itself, once per
/// sample.
#[derive(Debug, Clone, Copy)]
struct Modulators {
    lfo: [f64; 2],
    wheel: f64,
}

// ── The panel, in engine units ──

#[derive(Debug, Clone, Copy)]
struct OscSetting {
    shape: Shape,
    table: f64,
    /// Frequency multiplier from the coarse and fine tune knobs.
    ratio: f64,
    /// The low-frequency rate oscillator D runs at in its ModLo mode.
    lo_hz: f64,
    level: f64,
}

#[derive(Debug, Clone, Copy)]
struct Panel {
    osc: [OscSetting; 4],
    /// Which sequence each oscillator is pointed at: zero for none, otherwise
    /// a position in [`SEQ_BANK`] plus one. Always zero on a keymapped patch,
    /// where the oscillators are the recipe's rather than the panel's.
    seq: [usize; 4],
    /// The sequence clock, in hertz, before the matrix.
    seq_rate: f64,
    /// How far each TABLE knob has been moved from where this patch left it.
    ///
    /// What that knob means on a *sequenced* oscillator, because the step
    /// names the waveform and the knob can only shift it. Measured from the
    /// patch rather than from the middle of the travel, which is the same
    /// choice `cutoff_trim` makes below and for a stronger version of the same
    /// reason: a trim measured from a fixed position is only neutral on a
    /// patch that happens to sit there, so switching a sequence on would move
    /// the sound of every patch that does not.
    table_trim: [f64; 4],
    d_mode: DMode,
    vector: [f64; 2],
    pulse_width: f64,
    drive: f64,
    cutoff: f64,
    resonance: f64,
    filter_env: f64,
    key_follow: f64,
    velocity_depth: f64,
    gain: f64,
    lfo_shape: [LfoShape; 2],
    lfo_rate: [f64; 2],
    env: [EnvTimes; 2],
    /// The active routings, compacted to the front so the per-sample loop
    /// never walks a slot that is switched off.
    slots: [Slot; MOD_SLOTS],
    slot_count: usize,
    /// The patch's key map: empty on a melodic patch, one entry per mapped
    /// note on a keymapped one.
    keys: &'static [KeyChart],
    /// How far the CUTOFF knob has been moved from where this patch left it.
    /// The one panel control a keymapped patch still answers, because sweeping
    /// a whole kit is what a player does with it.
    cutoff_trim: f64,
}

/// The vector mix: bilinear weights on the unit square, oscillator A at the
/// bottom left and D at the top left, going round.
///
/// The four weights sum to exactly 1 at every position, which is the whole
/// headroom argument for this instrument: every oscillator is bounded by 1, so
/// the mix is bounded by the largest level knob however the vector moves and
/// whatever the matrix is doing to it.
#[inline]
fn vector_weights(x: f64, y: f64) -> [f64; 4] {
    let x = x.clamp(0.0, 1.0);
    let y = y.clamp(0.0, 1.0);
    [(1.0 - x) * (1.0 - y), x * (1.0 - y), x * y, (1.0 - x) * y]
}

// ── Voice ──

#[derive(Debug, Clone)]
struct Voice {
    osc: [Osc; 4],
    /// Where each oscillator stands in its wave sequence. Per voice, so two
    /// notes played a beat apart each hear the pattern from its beginning.
    seq: [SeqCursor; 4],
    filter: Ladder,
    env: [Envelope; 2],
    /// Oscillator D's last sample, which is what the matrix reads when D is a
    /// source. One sample late, because D's own pitch can be modulated by a
    /// slot that D itself feeds — a loop that has to be broken somewhere, and
    /// a hardware oscillator patched back into its own frequency input breaks
    /// it in exactly the same place.
    last_d: f64,
    note: u8,
    velocity: f64,
    age: u64,
    /// The recipe this note plays on a keymapped patch, resolved once at
    /// note-on. `None` on a melodic patch, where the panel is the voice.
    key: Option<KeyVoice>,
    sample_rate: f64,
}

impl Voice {
    fn new(sr: f64, index: usize) -> Self {
        Self {
            osc: std::array::from_fn(|i| {
                Osc::new(0x9E37_79B9 ^ ((index * 4 + i) as u32).wrapping_mul(2_246_822_519))
            }),
            seq: [SeqCursor::OFF; 4],
            filter: Ladder::new(),
            env: [Envelope::new(sr), Envelope::new(sr)],
            last_d: 0.0,
            note: 255,
            velocity: 0.0,
            age: 0,
            key: None,
            sample_rate: sr,
        }
    }

    fn note_on(&mut self, note: u8, velocity: u8, panel: &Panel, age: u64) {
        self.note = note;
        self.velocity = f64::from(velocity) / 127.0;
        self.age = age;
        // The one place the key map is read. A bounded scan of a `'static`
        // table and a copy of the result, both off the per-sample path.
        self.key = key_for_note(panel.keys, note).map(KeyVoice::resolve);
        // The sequences start where the note does, which is what makes a
        // one-shot an attack transient and a loop lock to the key rather than
        // to whenever the instrument was loaded.
        for (cursor, slot) in self.seq.iter_mut().zip(panel.seq) {
            *cursor = SeqCursor::start(slot);
        }
        let (times, resonance) = self
            .key
            .map_or((panel.env, panel.resonance), |key| (key.env, key.resonance));
        for (env, times) in self.env.iter_mut().zip(times) {
            env.set_times(times);
            env.trigger();
        }
        self.filter.start(resonance);
    }

    fn note_off(&mut self) {
        for env in &mut self.env {
            env.release_env();
        }
    }

    fn kill(&mut self) {
        self.note = 255;
        for env in &mut self.env {
            env.kill();
        }
        self.filter.reset();
        self.last_d = 0.0;
        self.key = None;
        self.seq = [SeqCursor::OFF; 4];
    }

    fn is_sounding(&self) -> bool {
        self.env[0].is_active()
    }

    fn is_held(&self) -> bool {
        self.env[0].is_held()
    }

    #[allow(clippy::too_many_lines)]
    fn tick(&mut self, panel: &Panel, m: &Modulators, bank: &WaveBank) -> f64 {
        if !self.is_sounding() {
            return 0.0;
        }
        let sr = self.sample_rate;
        // The recipe, on a keymapped patch. A copy rather than a borrow, so
        // the oscillators below can still be reached mutably.
        let key = self.key;

        // The time sliders are live on a melodic patch, so a note already
        // sounding follows them. On a keymapped one they are the recipe's and
        // were settled at note-on.
        if key.is_none() {
            for (env, times) in self.env.iter_mut().zip(panel.env) {
                env.set_times(times);
            }
        }
        let env1 = self.env[0].tick();
        let env2 = self.env[1].tick();
        // Envelope 2 is not wired to the amplifier, so it has to be told when
        // the voice is finished or it would hold a released note's filter open
        // for its own release time and no longer.
        if !self.env[0].is_active() {
            self.env[1].kill();
        }

        let key_track = (f64::from(self.note) - 60.0) / 36.0;

        // Everything from here reads the recipe on a keymapped patch and the
        // panel on a melodic one. The recipe's pitch is a frequency rather
        // than a note, because a drum's body is a frequency; keyboard follow
        // is skipped for the same reason, since the note is not a pitch.
        let oscillators = key.as_ref().map_or(&panel.osc, |k| &k.osc);
        let vector = key.map_or(panel.vector, |k| k.vector);
        let key_follow = if key.is_some() {
            0.0
        } else {
            (f64::from(self.note) - 60.0) / 12.0 / CUTOFF_OCTAVES * panel.key_follow
        };
        let (cutoff_base, resonance_base, filter_env) = key.map_or(
            (panel.cutoff, panel.resonance, panel.filter_env),
            |k| (k.cutoff + panel.cutoff_trim, k.resonance, k.filter_env),
        );

        // The matrix. Every routing lands in one accumulator per destination,
        // so two slots pointed at the same place add rather than fight.
        let mut bus = [0.0f64; DEST_COUNT];
        for slot in &panel.slots[..panel.slot_count] {
            let value = match slot.source {
                Source::Lfo1 => m.lfo[0],
                Source::Lfo2 => m.lfo[1],
                Source::Env1 => env1,
                Source::Env2 => env2,
                Source::Velocity => self.velocity,
                Source::KeyTrack => key_track,
                Source::Wheel => m.wheel,
                // The vector position as a source is its resting position,
                // not the modulated one — a slot pointed from the vector at
                // the vector would otherwise be reading its own output.
                Source::VectorX => vector[0],
                Source::VectorY => vector[1],
                Source::OscD => self.last_d,
                Source::Off => continue,
            };
            if slot.dest == Dest::Amplitude {
                // The amplifier is the one destination that cannot be allowed
                // to add: a matrix that could double the level would cost
                // every patch 6 dB of headroom whether it used the slot or
                // not. So this accumulates *attenuation* — the source is
                // folded to 0..1, inverted when the amount is negative, and
                // what is left of unity is taken away. A full-depth tremolo
                // still reaches silence and unity, and nothing here can pass
                // unity.
                let uni = if slot.bipolar { value.mul_add(0.5, 0.5) } else { value };
                let uni = if slot.amount >= 0.0 { uni } else { 1.0 - uni };
                bus[Dest::Amplitude as usize] += slot.amount.abs() * (1.0 - uni);
            } else {
                bus[slot.dest as usize] += value * slot.amount;
            }
        }

        let pitch_ratio = if bus[Dest::Pitch as usize] == 0.0 {
            1.0
        } else {
            2.0f64.powf(bus[Dest::Pitch as usize] * PITCH_MOD_SEMITONES / 12.0)
        };
        let width = pulse_width_from(panel.pulse_width + bus[Dest::PulseWidth as usize]);
        let table_shift = bus[Dest::Wave as usize];
        let base_freq = key.map_or_else(|| note_to_freq(self.note), |k| k.base_hz) * pitch_ratio;

        // The sequence clock, and one sample of each of the four cursors.
        //
        // The selector is compared against the cursor rather than read at
        // note-on and forgotten, so moving it under a held note restarts that
        // oscillator's sequence. That restart is a bounded table read on the
        // one sample the knob moved, which is the same work note-on does.
        let seq_rate = if bus[Dest::SeqRate as usize] == 0.0 {
            panel.seq_rate
        } else {
            panel.seq_rate * 2.0f64.powf(bus[Dest::SeqRate as usize] * SEQ_RATE_OCTAVES)
        };
        let mut seq = [SEQ_IDLE; 4];
        let mut sequenced = [false; 4];
        for (i, cursor) in self.seq.iter_mut().enumerate() {
            if cursor.slot != panel.seq[i] {
                *cursor = SeqCursor::start(panel.seq[i]);
            }
            sequenced[i] = cursor.is_running();
            seq[i] = cursor.advance(seq_rate, sr);
        }

        // Oscillator D first, so that its contribution to the mix and its
        // value on the matrix are the same sample.
        let d = &oscillators[3];
        let d_base = if panel.d_mode == DMode::ModLo {
            d.lo_hz * pitch_ratio
        } else {
            base_freq * d.ratio
        };
        let d_freq = d_base * seq[3].ratio;
        let d_out = if sequenced[3] && d.shape == Shape::Table {
            let shift = panel.table_trim[3] + table_shift;
            self.osc[3].tick_blend(
                d_freq,
                sr,
                seq[3].wave[0] + shift,
                seq[3].wave[1] + shift,
                seq[3].mix,
                bank,
            )
        } else {
            self.osc[3].tick(d.shape, d_freq, sr, width, d.table + table_shift, bank)
        };
        self.last_d = d_out;

        let vector_x = vector[0] + bus[Dest::VectorX as usize];
        let vector_y = vector[1] + bus[Dest::VectorY as usize];
        let weights = vector_weights(vector_x, vector_y);

        // The mix. A sequenced oscillator takes its waveform, its pitch and
        // its level from the step it is on; its own knobs stay live over the
        // top — TUNE and LEVEL multiply what the step asks for, and TABLE
        // shifts the whole sequence through the bank, from wherever the patch
        // left that knob. The step's *waveform* is the one thing a sequence
        // can only give to an oscillator that is reading the bank, so on any
        // other shape the WAVE switch still wins and what is left of the
        // sequence is a riff and a rhythm.
        let mut mix = 0.0;
        for i in 0..3 {
            let o = &oscillators[i];
            let freq = base_freq * o.ratio * seq[i].ratio;
            let value = if sequenced[i] && o.shape == Shape::Table {
                let shift = panel.table_trim[i] + table_shift;
                self.osc[i].tick_blend(
                    freq,
                    sr,
                    seq[i].wave[0] + shift,
                    seq[i].wave[1] + shift,
                    seq[i].mix,
                    bank,
                )
            } else {
                self.osc[i].tick(o.shape, freq, sr, width, o.table + table_shift, bank)
            };
            mix += weights[i] * o.level * seq[i].level * value;
        }
        if panel.d_mode == DMode::Audio {
            mix += weights[3] * d.level * seq[3].level * d_out;
        }

        let driven = drive_stage(mix, panel.drive);

        let cutoff = (cutoff_base + env2 * filter_env + key_follow + bus[Dest::Cutoff as usize])
            .clamp(0.0, 1.0);
        let resonance = (resonance_base + bus[Dest::Resonance as usize]).clamp(0.0, 1.0);
        let filtered = self.filter.process(driven, cutoff, resonance, sr);

        let velocity_gain = 1.0 - panel.velocity_depth * (1.0 - self.velocity);
        let amp_mod = (1.0 - bus[Dest::Amplitude as usize]).clamp(0.0, 1.0);
        let recipe_level = key.map_or(1.0, |k| k.level);
        filtered * env1 * velocity_gain * amp_mod * recipe_level
    }
}

fn pulse_width_from(knob: f64) -> f64 {
    pulse_width(knob.clamp(0.0, 1.0))
}

fn note_to_freq(note: u8) -> f64 {
    440.0 * 2.0f64.powf((f64::from(note) - 69.0) / 12.0)
}

// ── Patches ──
//
// The mechanism is here and the data is in `bank`: a chart row per patch, a
// selector that steps by index, and one conversion from chart to panel that
// the default parameter block is derived from. Adding a patch is a row of
// data in the other file.
//
// The bank is a module of its own because it is 229 rows long and this file
// is the engine. Nothing about it is public except the counts and the names —
// `BANK` itself stays inside the instrument, because a patch is a set of knob
// positions and [`PhosphorSynth::params_for_patch`] is how a caller gets one.

pub use bank::{BANK_COUNT, BANK_NAMES, PATCH_COUNT};

use bank::{BANK, BANK_FIRST};

mod bank;

/// One row of the bank: where every control sits on that patch.
#[derive(Debug, Clone, Copy)]
struct Chart {
    name: &'static str,
    /// Cut to the twelve columns the editor's selector row leaves for a label.
    label: &'static str,
    /// Where the patch lives, in the addressing of whichever instrument it
    /// came from: `A.11` to `B.88` on the microKORG, `M.01` and `W.01` on the
    /// two authored sets, `P.01` on the instrument's own.
    slot: &'static str,
    /// What the keyboard does on this patch.
    keys: KeyMap,
    osc: [OscChart; 4],
    /// Which wave sequence each oscillator is pointed at: `None` for none,
    /// otherwise a position in [`SEQ_BANK`].
    seq: [Option<usize>; 4],
    /// The sequence clock, as [`seq_rate_at`] names it.
    seq_rate: f32,
    /// 0 = audio, 1 = mod, 2 = mod lo.
    d_mode: u8,
    /// The vector position: x, y.
    vector: [f32; 2],
    pulse_width: f32,
    drive: f32,
    /// Cutoff, resonance, envelope 2 depth (bipolar), keyboard follow.
    filter: [f32; 4],
    velocity: f32,
    gain: f32,
    /// Shape and rate, for each of the two LFOs.
    lfo: [(u8, f32); 2],
    /// Attack, decay, sustain, release, for each of the two envelopes.
    env: [[f32; 4]; 2],
    /// Source, destination, amount (bipolar), for each matrix slot.
    matrix: [(u8, u8, f32); MOD_SLOTS],
}

#[derive(Debug, Clone, Copy)]
struct OscChart {
    /// 0 saw, 1 pulse, 2 triangle, 3 sine, 4 wavetable, 5 noise.
    shape: u8,
    /// Position in the wavetable bank, 0..1.
    table: f32,
    semitones: i32,
    cents: f32,
    level: f32,
}

/// A slot that is not routed anywhere.
const NO_ROUTE: (u8, u8, f32) = (0, 0, 0.0);

// ── Keymapped patches ──
//
// A patch where the note number picks a *sound* rather than a pitch, which is
// what a Wavestation, an M1 or any other rompler calls a drum kit. It is an
// engine capability rather than patch data: a melodic patch maps a note to a
// frequency, and a keymapped one maps it to an entire voice recipe — its own
// oscillator shapes and wavetable positions, its own tuning, its own vector
// balance, its own filter and envelopes and level.
//
// What stays on the panel when a patch is keymapped is everything global or
// downstream: DRIVE, GAIN, the velocity depth, the pulse width, the two LFOs
// and the whole matrix. So a kit is still one instrument — driven, swept and
// modulated as a unit — which is the point of having drums here rather than
// only in the drum rack: they are made of the same oscillators, ladder and
// envelopes as the pads and sit in the same sonic world.
//
// The CUTOFF knob is the one exception, and it is live: what it does on a
// keymapped patch is *offset* every recipe's cutoff by however far it has been
// moved from where the patch left it. That is the control a player reaches for
// on a whole kit, and leaving it inert would be the wrong kind of literal.

/// How a patch reads the keyboard.
///
/// One field rather than a flag beside a table, so a patch cannot claim to be
/// keymapped and carry nothing, or carry a map and be played melodically.
#[derive(Debug, Clone, Copy)]
enum KeyMap {
    /// The note sets the pitch and the panel is the voice.
    Melodic,
    /// The note selects a whole voice recipe from this table, which is sorted
    /// by note and may be as sparse as it likes.
    Keymapped(&'static [KeyChart]),
}

impl KeyMap {
    /// The table, or an empty one for a melodic patch.
    const fn table(self) -> &'static [KeyChart] {
        match self {
            Self::Melodic => &[],
            Self::Keymapped(keys) => keys,
        }
    }
}

/// One note of a keymapped patch: the whole voice recipe that note plays.
///
/// The oscillators are tuned in semitones and cents from `hz` rather than from
/// the keyboard, because a drum's body is a frequency and not a note.
#[derive(Debug, Clone, Copy)]
struct KeyChart {
    /// The MIDI note this recipe is played from.
    note: u8,
    /// What it is. Not printed anywhere yet; it is here so that the tests and
    /// a future editor can name a zone rather than count it.
    name: &'static str,
    /// Where the body of the sound sits, in hertz.
    hz: f32,
    osc: [OscChart; 4],
    /// The vector position this recipe mixes its four oscillators at.
    vector: [f32; 2],
    /// Cutoff, resonance, envelope 2 depth (bipolar).
    filter: [f32; 3],
    /// Attack, decay, sustain, release, for the amplitude envelope and then
    /// the modulation envelope.
    env: [[f32; 4]; 2],
    level: f32,
}

/// Which recipe a note plays.
///
/// **The decision this is:** a note the patch does not map plays the nearest
/// note it does, rather than nothing.
///
/// Silence was the alternative and it is the more literal answer — a key with
/// no sound assigned makes no sound. It was rejected for the reason the drum
/// rack rejected it in `voice_606`: a part written against a General MIDI kit
/// reaches for notes that no particular kit has, and under silence loading a
/// kit deletes half of a finished part with no indication of why. Folding
/// plays it on the closest thing the patch has, which is what a player with
/// that kit in front of them would do anyway.
///
/// Ties — a note exactly between two entries — go to the lower one, because
/// the scan runs in note order and only takes a strictly closer entry.
///
/// Bounded and allocation-free: a linear scan of a table that is `'static`,
/// run once at note-on.
fn key_for_note(keys: &'static [KeyChart], note: u8) -> Option<&'static KeyChart> {
    let mut best: Option<&'static KeyChart> = None;
    let mut best_distance = u16::MAX;
    for entry in keys {
        let distance = u16::from(entry.note.abs_diff(note));
        if distance < best_distance {
            best_distance = distance;
            best = Some(entry);
        }
    }
    best
}

/// A recipe in the units the engine runs on, resolved once at note-on.
#[derive(Debug, Clone, Copy)]
struct KeyVoice {
    osc: [OscSetting; 4],
    base_hz: f64,
    vector: [f64; 2],
    cutoff: f64,
    resonance: f64,
    filter_env: f64,
    env: [EnvTimes; 2],
    level: f64,
}

impl KeyVoice {
    fn resolve(chart: &KeyChart) -> Self {
        Self {
            osc: std::array::from_fn(|i| osc_setting(&chart.osc[i])),
            base_hz: f64::from(chart.hz),
            vector: [f64::from(chart.vector[0]), f64::from(chart.vector[1])],
            cutoff: f64::from(chart.filter[0]),
            resonance: f64::from(chart.filter[1]),
            filter_env: bipolar(chart.filter[2]),
            env: [env_times(chart.env[0]), env_times(chart.env[1])],
            level: f64::from(chart.level),
        }
    }
}

/// One oscillator chart row in engine units. Shared by the panel and by the
/// keymap, so a recipe's oscillator and a panel oscillator cannot end up
/// meaning different things.
fn osc_setting(chart: &OscChart) -> OscSetting {
    let semitones = f64::from(chart.semitones);
    let cents = f64::from(chart.cents);
    OscSetting {
        shape: Shape::from_index(chart.shape as usize),
        table: f64::from(chart.table),
        ratio: 2.0f64.powf(semitones / 12.0 + cents / 1200.0),
        lo_hz: mod_lo_hz(semitones) * 2.0f64.powf(cents / 1200.0),
        level: f64::from(chart.level),
    }
}

/// Four sliders in panel units as four times in seconds.
fn env_times(chart: [f32; 4]) -> EnvTimes {
    EnvTimes {
        attack: attack_seconds(f64::from(chart[0])),
        decay: decay_seconds(f64::from(chart[1])),
        sustain: f64::from(chart[2]),
        release: decay_seconds(f64::from(chart[3])),
    }
}

/// The patch names in full: the factory names on the microKORG set, and the
/// authored ones on the other three.
pub const PATCH_NAMES: [&str; PATCH_COUNT] = derive_names();

/// The names cut to the twelve columns the editor's selector row leaves.
///
/// Slot code, a space, and as much of the name as fits — the same shape the
/// Juno's and the Jupiter's labels take, and for the same reason: at 229
/// patches a name on its own does not say where in the bank you are.
pub const PATCH_LABELS: [&str; PATCH_COUNT] = derive_labels();

/// Where each patch sits, in the addressing of the instrument it came from.
///
/// `A.11` to `B.88` are the microKORG's own slots, read off the factory list.
/// `P.01`, `M.01` and `W.01` number the three sets that have no hardware
/// addressing to borrow.
pub const PATCH_SLOTS: [&str; PATCH_COUNT] = derive_slots();

const fn derive_names() -> [&'static str; PATCH_COUNT] {
    let mut out = [""; PATCH_COUNT];
    let mut i = 0;
    while i < PATCH_COUNT {
        out[i] = BANK[i].name;
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

const fn derive_slots() -> [&'static str; PATCH_COUNT] {
    let mut out = [""; PATCH_COUNT];
    let mut i = 0;
    while i < PATCH_COUNT {
        out[i] = BANK[i].slot;
        i += 1;
    }
    out
}

// ── Moving around a bank of 229 ──
//
// **The decision this is:** one selector, not two.
//
// The DX7 in this project has a bank knob beside its voice knob, and 229
// patches on one control is the same problem its 256 had — so the question
// was asked again here and answered the other way, because the two
// instruments differ in the thing that matters. A DX7 voice is 145 numbers in
// ROM that the panel never shows; its two selectors between them *name* a
// voice and change nothing else. This instrument's patch selector **is** the
// panel: choosing a patch overwrites the other 66 parameters. A second
// selector that also overwrote them would mean two controls owning one piece
// of state, and every consumer — the session's selector table, a preset load,
// the editor's "the patch changed, resend everything" path — would have to
// agree about which of them won. That is exactly the kind of
// silent-and-plausible failure the session format's position table exists to
// prevent.
//
// The sets are also not the same size — 11, 128, 40 and 50 — so the DX7's
// rectangular `bank * 32 + voice` arithmetic does not apply. It would need a
// patch selector whose *number of positions* depended on another knob, which
// `discrete_steps` cannot express: it takes an index, not the block. That is
// why the DX7 needs its own `discrete_label(&params, index)` signature and
// why the editor special-cases it.
//
// So the patch knob keeps one position per patch, `step_discrete` keeps
// moving one patch per keypress as it is required to, and what a player needs
// instead of a second knob is published here as data: which set a patch is
// in, where each set begins, and one function that jumps between them. An
// editor can bind that to a key without a parameter existing for it.

/// Which set patch `index` belongs to — a position in [`BANK_NAMES`].
///
/// Total: an index past the end reads as the last set, because callers get
/// their index from a knob and a knob can arrive as anything.
#[must_use]
pub fn patch_bank(index: usize) -> usize {
    let mut set = 0;
    while set + 1 < BANK_COUNT && index >= BANK_FIRST[set + 1] {
        set += 1;
    }
    set
}

/// The half-open range of patches in set `bank`. An out-of-range set reads as
/// the last one.
#[must_use]
pub fn bank_bounds(bank: usize) -> (usize, usize) {
    let bank = bank.min(BANK_COUNT - 1);
    (BANK_FIRST[bank], BANK_FIRST[bank + 1])
}

/// The knob position a coarse move lands on, for an editor that wants one
/// beside the patch knob's fine one.
///
/// Up is the first patch of the next set. Down is the first patch of *this*
/// set, and only then the first of the set before it — so from the middle of
/// the microKORG's 128 the first press takes you to A.11 rather than past it
/// into the eleven, which is what a player reaching for a coarse control
/// means by it. At either end the move stops rather than wrapping.
#[must_use]
pub fn bank_step(value: f32, up: bool) -> f32 {
    let index = patch_index(value);
    let set = patch_bank(index);
    let target = if up {
        if set + 1 < BANK_COUNT { BANK_FIRST[set + 1] } else { PATCH_COUNT - 1 }
    } else if index > BANK_FIRST[set] {
        BANK_FIRST[set]
    } else {
        BANK_FIRST[set.saturating_sub(1)]
    };
    patch_knob(target)
}

/// The knob position that selects patch `index`, for a caller sweeping the
/// bank from outside — a level measurement, an export, a test.
///
/// The midpoint of the step, which is the one position in it that no amount of
/// float rounding can push into a neighbour, and the same position
/// [`step_discrete`] moves between.
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

/// Whether a patch reads the keyboard as a set of sounds rather than as a set
/// of pitches — a drum kit, in other words.
///
/// Public because an editor showing a kit's zones, or a piano roll labelling
/// its rows, needs to know before it draws anything.
#[must_use]
pub fn is_keymapped(patch: usize) -> bool {
    !BANK[patch.min(PATCH_COUNT - 1)].keys.table().is_empty()
}

/// How many notes a keymapped patch maps. Zero for a melodic one.
#[must_use]
pub fn key_zone_count(patch: usize) -> usize {
    BANK[patch.min(PATCH_COUNT - 1)].keys.table().len()
}

/// The note and the name of one of a keymapped patch's zones, in note order.
///
/// Two calls rather than a slice of tuples so that nothing has to be built to
/// answer them: the bank owns the table and this reads it in place.
#[must_use]
pub fn key_zone(patch: usize, zone: usize) -> Option<(u8, &'static str)> {
    BANK[patch.min(PATCH_COUNT - 1)]
        .keys
        .table()
        .get(zone)
        .map(|entry| (entry.note, entry.name))
}

/// One chart row as a parameter block.
///
/// `const` so that [`PARAM_DEFAULTS`] can be the first row of the bank rather
/// than a hand-copied duplicate of it — a duplicate is a thing to forget when
/// the patch is revoiced, and the Juno's default and its patch 0 have to be
/// held together by a test for exactly that reason.
const fn chart_params(index: usize) -> [f32; PARAM_COUNT] {
    let c = &BANK[index];
    let mut p = [0.0f32; PARAM_COUNT];
    p[P_PATCH] = knob_for(index, PATCH_COUNT);

    // A `while` rather than a `for`, because iterators are not const.
    let mut i = 0;
    while i < 4 {
        let o = &c.osc[i];
        let base = P_A_WAVE + i * P_OSC_STRIDE;
        p[base] = knob_for(o.shape as usize, SHAPE_COUNT);
        p[base + 1] = o.table;
        p[base + 2] = tune_knob(o.semitones);
        p[base + 3] = fine_knob(o.cents);
        p[base + 4] = o.level;
        p[base + 5] = match c.seq[i] {
            Some(index) => knob_for(index + 1, SEQ_SLOTS),
            None => knob_for(0, SEQ_SLOTS),
        };
        i += 1;
    }
    p[P_D_MODE] = knob_for(c.d_mode as usize, 3);
    p[P_SEQ_RATE] = c.seq_rate;

    p[P_VECTOR_X] = c.vector[0];
    p[P_VECTOR_Y] = c.vector[1];
    p[P_PULSE_WIDTH] = c.pulse_width;

    p[P_DRIVE] = c.drive;
    p[P_CUTOFF] = c.filter[0];
    p[P_RESO] = c.filter[1];
    p[P_FILTER_ENV] = c.filter[2];
    p[P_KEY_FOLLOW] = c.filter[3];

    p[P_VELOCITY] = c.velocity;
    p[P_GAIN] = c.gain;

    p[P_LFO1_WAVE] = knob_for(c.lfo[0].0 as usize, LFO_SHAPE_COUNT);
    p[P_LFO1_RATE] = c.lfo[0].1;
    p[P_LFO2_WAVE] = knob_for(c.lfo[1].0 as usize, LFO_SHAPE_COUNT);
    p[P_LFO2_RATE] = c.lfo[1].1;

    p[P_ATTACK1] = c.env[0][0];
    p[P_DECAY1] = c.env[0][1];
    p[P_SUSTAIN1] = c.env[0][2];
    p[P_RELEASE1] = c.env[0][3];
    p[P_ATTACK2] = c.env[1][0];
    p[P_DECAY2] = c.env[1][1];
    p[P_SUSTAIN2] = c.env[1][2];
    p[P_RELEASE2] = c.env[1][3];

    let mut slot = 0;
    while slot < MOD_SLOTS {
        let (source, dest, amount) = c.matrix[slot];
        let base = P_MOD_BASE + slot * 3;
        p[base] = knob_for(source as usize, SOURCE_COUNT);
        p[base + 1] = knob_for(dest as usize, DEST_COUNT);
        p[base + 2] = bipolar_knob(amount);
        slot += 1;
    }
    p
}

// ── PhosphorSynth ──

pub struct PhosphorSynth {
    voices: [Voice; MAX_VOICES],
    lfo: [Lfo; 2],
    bank: &'static WaveBank,
    sample_rate: f64,
    /// The mod wheel, CC 1. Held here rather than per voice because the wheel
    /// is one physical control and a note added to a held chord should join it
    /// where it stands.
    wheel: f64,
    pub params: [f32; PARAM_COUNT],
    voice_counter: u64,
    last_patch_index: usize,
}

impl PhosphorSynth {
    #[must_use]
    pub fn new() -> Self {
        Self {
            voices: std::array::from_fn(|i| Voice::new(44_100.0, i)),
            lfo: [Lfo::new(0x1234_5678), Lfo::new(0x8765_4321)],
            bank: wave_bank(),
            sample_rate: 44_100.0,
            wheel: 0.0,
            params: PARAM_DEFAULTS,
            voice_counter: 0,
            last_patch_index: 0,
        }
    }

    /// The whole panel as the chart for this patch sets it.
    #[must_use]
    pub fn params_for_patch(patch_value: f32) -> [f32; PARAM_COUNT] {
        let mut params = chart_params(patch_index(patch_value));
        params[P_PATCH] = patch_value;
        params
    }

    fn sync_params_from_patch(&mut self) {
        let index = patch_index(self.params[P_PATCH]);
        if index == self.last_patch_index {
            return;
        }
        self.last_patch_index = index;
        let loaded = Self::params_for_patch(self.params[P_PATCH]);
        for (i, value) in loaded.iter().enumerate() {
            if i != P_PATCH {
                self.params[i] = *value;
            }
        }
    }

    fn next_age(&mut self) -> u64 {
        self.voice_counter += 1;
        self.voice_counter
    }

    fn allocate_voice(&mut self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| !v.is_sounding()) {
            return i;
        }
        if let Some((i, _)) = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_held())
            .min_by_key(|(_, v)| v.age)
        {
            return i;
        }
        self.voices
            .iter()
            .enumerate()
            .min_by_key(|(_, v)| v.age)
            .map_or(0, |(i, _)| i)
    }

    fn release_note(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.note == note && v.is_held() {
                v.note_off();
            }
        }
    }

    fn kill_all(&mut self) {
        for v in &mut self.voices {
            v.kill();
        }
    }

    /// The panel as it stands, in the units the engine works in. Every control
    /// is live — the preset is only where the knobs started.
    fn panel(&self) -> Panel {
        let p = &self.params;
        let chart = &BANK[patch_index(p[P_PATCH])];
        let osc = std::array::from_fn(|i| {
            let base = P_A_WAVE + i * P_OSC_STRIDE;
            let semitones = tune_semitones(p[base + 2]);
            let cents = fine_cents(p[base + 3]);
            OscSetting {
                shape: Shape::from_index(selector(p[base], SHAPE_COUNT)),
                table: f64::from(p[base + 1]),
                ratio: 2.0f64.powf(semitones / 12.0 + cents / 1200.0),
                lo_hz: mod_lo_hz(semitones) * 2.0f64.powf(cents / 1200.0),
                level: f64::from(p[base + 4]),
            }
        });

        let mut slots = [Slot { source: Source::Off, dest: Dest::Off, amount: 0.0, bipolar: false };
            MOD_SLOTS];
        let mut slot_count = 0;
        for slot in 0..MOD_SLOTS {
            let base = P_MOD_BASE + slot * 3;
            let source = Source::from_index(selector(p[base], SOURCE_COUNT));
            let dest = Dest::from_index(selector(p[base + 1], DEST_COUNT));
            let amount = bipolar(p[base + 2]);
            if source == Source::Off || dest == Dest::Off || amount == 0.0 {
                continue;
            }
            slots[slot_count] = Slot { source, dest, amount, bipolar: source.is_bipolar() };
            slot_count += 1;
        }

        // The sequence selectors, and the one case where they are not read:
        // on a keymapped patch the oscillators belong to the recipe the note
        // picked, and a sequence is an oscillator control. The four selectors
        // are inert on a kit for the same reason the four WAVE switches are.
        let keymapped = !chart.keys.table().is_empty();
        let seq = std::array::from_fn(|i| {
            if keymapped {
                0
            } else {
                selector(p[P_A_WAVE + i * P_OSC_STRIDE + 5], SEQ_SLOTS)
            }
        });

        Panel {
            osc,
            seq,
            seq_rate: seq_hz(f64::from(p[P_SEQ_RATE])),
            table_trim: std::array::from_fn(|i| {
                f64::from(p[P_A_WAVE + i * P_OSC_STRIDE + 1]) - f64::from(chart.osc[i].table)
            }),
            d_mode: DMode::from_index(selector(p[P_D_MODE], 3)),
            vector: [f64::from(p[P_VECTOR_X]), f64::from(p[P_VECTOR_Y])],
            pulse_width: f64::from(p[P_PULSE_WIDTH]),
            drive: f64::from(p[P_DRIVE]),
            cutoff: f64::from(p[P_CUTOFF]),
            resonance: f64::from(p[P_RESO]),
            filter_env: bipolar(p[P_FILTER_ENV]),
            key_follow: f64::from(p[P_KEY_FOLLOW]),
            velocity_depth: f64::from(p[P_VELOCITY]),
            gain: f64::from(p[P_GAIN]),
            lfo_shape: [
                LfoShape::from_index(selector(p[P_LFO1_WAVE], LFO_SHAPE_COUNT)),
                LfoShape::from_index(selector(p[P_LFO2_WAVE], LFO_SHAPE_COUNT)),
            ],
            lfo_rate: [lfo_hz(f64::from(p[P_LFO1_RATE])), lfo_hz(f64::from(p[P_LFO2_RATE]))],
            env: [
                env_times([p[P_ATTACK1], p[P_DECAY1], p[P_SUSTAIN1], p[P_RELEASE1]]),
                env_times([p[P_ATTACK2], p[P_DECAY2], p[P_SUSTAIN2], p[P_RELEASE2]]),
            ],
            slots,
            slot_count,
            keys: chart.keys.table(),
            cutoff_trim: f64::from(p[P_CUTOFF]) - f64::from(chart.filter[0]),
        }
    }
}

impl Default for PhosphorSynth {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for PhosphorSynth {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "Phosphor Synth".into(),
            version: "0.3.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = std::array::from_fn(|i| Voice::new(sample_rate, i));
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() {
            return;
        }

        let buf_len = outputs[0].len();
        let panel = self.panel();
        let gain = (panel.gain as f32) * OUTPUT_TRIM;
        let sr = self.sample_rate;
        let bank = self.bank;

        // MIDI event sorting, allocation-free: a fixed index buffer and an
        // insertion sort, which is what a handful of events per buffer wants.
        let mut event_indices: [usize; 256] = [0; 256];
        let event_count = midi_events.len().min(256);
        for (i, slot) in event_indices.iter_mut().enumerate().take(event_count) {
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
                            self.release_note(ev.data1);
                            let age = self.next_age();
                            let index = self.allocate_voice();
                            self.voices[index].note_on(ev.data1, ev.data2, &panel, age);
                        } else {
                            self.release_note(ev.data1);
                        }
                    }
                    0x80 => self.release_note(ev.data1),
                    0xB0 => match ev.data1 {
                        1 => self.wheel = f64::from(ev.data2) / 127.0,
                        120 => self.kill_all(),
                        123 => {
                            for v in &mut self.voices {
                                if v.is_held() {
                                    v.note_off();
                                }
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
                ei += 1;
            }

            let modulators = Modulators {
                lfo: [
                    self.lfo[0].tick(panel.lfo_shape[0], panel.lfo_rate[0], sr),
                    self.lfo[1].tick(panel.lfo_shape[1], panel.lfo_rate[1], sr),
                ],
                wheel: self.wheel,
            };

            let mut sample = 0.0f32;
            for v in &mut self.voices {
                sample += v.tick(&panel, &modulators, bank) as f32;
            }
            // Bound the output without hard clipping it. The trim above keeps
            // even the panel's extremes under the knee, so this is the
            // identity for everything the instrument can currently produce.
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
                P_ATTACK1 | P_DECAY1 | P_RELEASE1 | P_ATTACK2 | P_DECAY2 | P_RELEASE2
                | P_SEQ_RATE => "s".into(),
                P_LFO1_RATE | P_LFO2_RATE => "Hz".into(),
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
        self.kill_all();
        self.voice_counter = 0;
        self.wheel = 0.0;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // ── An allocation counter for the audio path ──
    //
    // The defect this exists for happened on the Odyssey: a `Vec` of held
    // notes that could reallocate inside the callback on the seventeenth
    // simultaneous key. Nothing caught it, because "no allocation in
    // `process`" is a property of the code rather than of its output, and
    // every test here only reads the output.
    //
    // So the test binary counts allocations. Per thread rather than globally,
    // because cargo runs tests in parallel and a global count would see every
    // other test's work; the thread-local is declared with `const` so that
    // reading it cannot itself allocate, and `try_with` is used so that an
    // allocation during thread teardown cannot panic inside the allocator.

    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    }

    struct Counting;

    fn note_allocation() {
        let _ = ALLOCATIONS.try_with(|c| c.set(c.get() + 1));
    }

    // SAFETY: every method forwards to the system allocator with the same
    // pointer and layout it was given, so the allocator's contract is the
    // system allocator's contract. The counter is a thread-local `Cell` of a
    // plain integer, which allocates nothing and cannot re-enter.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            note_allocation();
            System.alloc(layout)
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            System.dealloc(ptr, layout);
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            note_allocation();
            System.alloc_zeroed(layout)
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            note_allocation();
            System.realloc(ptr, layout, new_size)
        }
    }

    #[global_allocator]
    static COUNTING: Counting = Counting;

    /// How many times the allocator was reached on this thread while `body`
    /// ran. `pub(crate)` because the counting allocator is installed once for
    /// the whole test binary, so every instrument that wants to assert its
    /// audio path is allocation-free shares this one.
    pub(crate) fn allocations_during(body: impl FnOnce()) -> u64 {
        let before = ALLOCATIONS.with(Cell::get);
        body();
        ALLOCATIONS.with(Cell::get) - before
    }

    fn note_on(note: u8, vel: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x90, data1: note, data2: vel }
    }
    fn note_off(note: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x80, data1: note, data2: 0 }
    }
    fn cc(number: u8, value: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0xB0, data1: number, data2: value }
    }

    fn process_buffers(synth: &mut PhosphorSynth, events: &[MidiEvent], count: usize) -> Vec<f32> {
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

    fn rms(samples: &[f32]) -> f32 {
        let sum: f64 = samples.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
        (sum / samples.len() as f64).sqrt() as f32
    }

    // ── Real-time safety ──

    #[test]
    fn the_audio_path_does_not_allocate() {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 256);
        let mut out = vec![0.0f32; 256];

        // On a sequenced patch, so that the step tables, the cursors and the
        // restart a moved selector causes are all inside the count. A step
        // list is a `'static` slice and a cursor is a plain struct in the
        // voice, so none of it should reach the allocator — but that is a
        // property of the code rather than of its output, which is the whole
        // reason this test counts rather than listens.
        let riff = PATCH_NAMES.iter().position(|n| *n == "SEQ RIFF").unwrap();
        s.set_parameter(P_PATCH, patch_knob(riff));

        // Warm everything the first call would touch: the wavetable bank,
        // which is built once per process behind a `OnceLock`.
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 0)]);

        // More simultaneous keys than there are voices, so the allocation
        // would land where the Odyssey's did — in the note bookkeeping under
        // a chord too big for the instrument.
        let events: Vec<MidiEvent> =
            (36u8..60).map(|n| note_on(n, 127, u32::from(n) % 8)).collect();
        let releases: Vec<MidiEvent> = (36u8..60).map(|n| note_off(n, 0)).collect();

        let allocations = allocations_during(|| {
            let mut outs: [&mut [f32]; 1] = [&mut out];
            s.process(&[], &mut outs, &events);
            for _ in 0..8 {
                s.process(&[], &mut outs, &[]);
            }
            // Every sequence in the bank pointed at every oscillator under a
            // held chord, which is the restart path at its busiest.
            for seq in 0..SEQ_COUNT {
                for i in 0..4 {
                    s.params[P_A_SEQ + i * P_OSC_STRIDE] = seq_knob(Some(seq));
                }
                s.process(&[], &mut outs, &[]);
            }
            s.process(&[], &mut outs, &releases);
            for _ in 0..8 {
                s.process(&[], &mut outs, &[]);
            }
            s.process(&[], &mut outs, &[cc(1, 64, 0), cc(120, 0, 32)]);
        });

        assert_eq!(allocations, 0, "the audio path allocated {allocations} times");
    }

    /// The counter has to actually count, or the assertion above is vacuous.
    #[test]
    fn the_allocation_counter_sees_an_allocation() {
        let allocations = allocations_during(|| {
            let v: Vec<u8> = Vec::with_capacity(4096);
            std::hint::black_box(&v);
        });
        assert!(allocations >= 1, "the counter saw nothing");
    }

    // ── The instrument ──

    #[test]
    fn silence_with_no_input() {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 64);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 200);
        assert!(peak(&out) > 0.005, "peak={}", peak(&out));
    }

    #[test]
    fn silent_after_release() {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[note_off(60, 0)], 500);
        let out = process_buffers(&mut s, &[], 1);
        assert!(peak(&out) < 0.001, "peak={}", peak(&out));
    }

    #[test]
    fn output_is_finite() {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 1000);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn polyphony() {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 64);
        let events = [note_on(60, 100, 0), note_on(64, 100, 0), note_on(67, 100, 0)];
        let out = process_buffers(&mut s, &events, 200);
        assert!(peak(&out) > 0.005 && peak(&out) < 1.0, "peak={}", peak(&out));
    }

    #[test]
    fn sample_accurate_midi() {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 128);
        let mut out = vec![0.0f32; 128];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 64)]);
        // Before the offset no voice is sounding at all, so the sum is exactly
        // zero — a stronger claim than "quiet", and one that does not move
        // when the output trim does.
        assert!(out[..64].iter().all(|&v| v == 0.0));
        assert!(peak(&out[64..]) > 0.0001);
    }

    #[test]
    fn cc120_kills_all() {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[cc(120, 0, 0)], 1);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn retrigger_doesnt_leak() {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 64);
        for _ in 0..100 {
            process_buffers(&mut s, &[note_on(60, 100, 0)], 1);
        }
        process_buffers(&mut s, &[note_off(60, 0)], 800);
        let out = process_buffers(&mut s, &[], 1);
        assert!(peak(&out) < 0.001, "peak={}", peak(&out));
    }

    #[test]
    fn more_keys_than_voices_steals_rather_than_panics() {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 64);
        let events: Vec<MidiEvent> = (40u8..70).map(|n| note_on(n, 110, 0)).collect();
        let out = process_buffers(&mut s, &events, 100);
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(peak(&out) < 1.0, "peak={}", peak(&out));
    }

    // ── The panel ──

    #[test]
    fn the_panel_is_the_shape_the_editor_expects() {
        assert_eq!(PARAM_NAMES.len(), PARAM_COUNT);
        assert_eq!(PARAM_DEFAULTS.len(), PARAM_COUNT);
        for (i, name) in PARAM_NAMES.iter().enumerate() {
            assert!(name.len() <= 8, "{i} {name:?} is wider than the parameter column");
        }
        // The patch selector is at index 0 because that is where the editor
        // looks for one.
        assert_eq!(P_PATCH, 0);
        assert!(is_discrete(P_PATCH));
        // Every label the panel can print fits the twelve columns the FX panel
        // leaves after the parameter name.
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            let Some(count) = discrete_steps(index) else { continue };
            for step in 0..count {
                let label = discrete_label(index, knob_for(step, count)).unwrap();
                assert!(
                    label.chars().count() <= 12,
                    "{name} position {step} label {label:?} does not fit"
                );
            }
        }
    }

    #[test]
    fn all_params_readable() {
        let s = PhosphorSynth::new();
        assert_eq!(s.parameter_count(), PARAM_COUNT);
        for i in 0..PARAM_COUNT {
            assert!(s.parameter_info(i).is_some());
            let value = s.get_parameter(i);
            assert!((0.0..=1.0).contains(&value), "param {i} = {value}");
        }
        assert!(s.parameter_info(PARAM_COUNT).is_none());
    }

    /// Every selector steps by index and lands on the position it names, from
    /// either end, without stalling on a boundary a fraction missed by an ulp.
    #[test]
    fn every_selector_steps_by_index() {
        for index in 0..PARAM_COUNT {
            let Some(count) = discrete_steps(index) else {
                assert_eq!(step_discrete(index, 0.42, true), 0.42, "slider {index} moved");
                assert_eq!(step_discrete(index, 0.42, false), 0.42, "slider {index} moved");
                continue;
            };
            let mut knob = 0.0f32;
            for step in 0..count {
                assert_eq!(selector(knob, count), step, "param {index} stalled at {step}");
                knob = step_discrete(index, knob, true);
            }
            // ...and it stops at the top rather than wrapping round.
            assert_eq!(selector(knob, count), count - 1);
            assert_eq!(step_discrete(index, knob, true), knob);
            for step in (0..count).rev() {
                assert_eq!(selector(knob, count), step, "param {index} stalled at {step}");
                knob = step_discrete(index, knob, false);
            }
            assert_eq!(selector(knob, count), 0);
            assert_eq!(step_discrete(index, knob, false), knob);
        }
    }

    #[test]
    fn discrete_labels_read_as_the_panel_does() {
        assert_eq!(discrete_label(P_PATCH, 0.0), Some("P01 INIT SAW"));
        assert_eq!(discrete_label(P_A_WAVE, knob_for(0, SHAPE_COUNT)), Some("saw"));
        assert_eq!(discrete_label(P_A_WAVE, knob_for(4, SHAPE_COUNT)), Some("table"));
        assert_eq!(discrete_label(P_D_MODE, knob_for(2, 3)), Some("mod lo"));
        assert_eq!(discrete_label(P_A_TUNE, tune_knob(0)), Some("0"));
        assert_eq!(discrete_label(P_A_TUNE, tune_knob(-12)), Some("-12"));
        assert_eq!(discrete_label(P_A_TUNE, tune_knob(24)), Some("+24"));
        assert_eq!(discrete_label(P_LFO1_WAVE, knob_for(4, LFO_SHAPE_COUNT)), Some("s&h"));
        assert_eq!(discrete_label(p_mod_src(0), knob_for(1, SOURCE_COUNT)), Some("lfo 1"));
        assert_eq!(discrete_label(p_mod_dest(0), knob_for(4, DEST_COUNT)), Some("cutoff"));
        // The amount knob is a slider, not a selector.
        assert_eq!(discrete_label(p_mod_amount(0), 0.5), None);
        assert!(!is_discrete(p_mod_amount(0)));
        assert_eq!(discrete_label(P_CUTOFF, 0.5), None);
        // Out of range in either direction still lands on a real position.
        assert_eq!(discrete_label(P_A_WAVE, 9.0), Some("noise"));
        assert_eq!(discrete_label(P_A_WAVE, -1.0), Some("saw"));
    }

    #[test]
    fn the_time_sliders_report_their_own_seconds() {
        assert!((param_seconds(P_ATTACK1, 0.0).unwrap() - ATTACK_MIN).abs() < 1e-9);
        assert!((param_seconds(P_ATTACK1, 1.0).unwrap() - (ATTACK_MIN + ATTACK_MAX)).abs() < 1e-6);
        assert!((param_seconds(P_RELEASE2, 0.0).unwrap() - DECAY_MIN).abs() < 1e-9);
        assert!((param_seconds(P_DECAY1, 1.0).unwrap() - (DECAY_MIN + DECAY_MAX)).abs() < 1e-6);
        assert_eq!(param_seconds(P_CUTOFF, 0.5), None);
        assert_eq!(param_seconds(P_LFO1_RATE, 0.5), None);
        // Monotone, or the number under the bar disagrees with the sound.
        let mut previous = 0.0;
        for step in 0..=100 {
            let value = step as f32 / 100.0;
            let seconds = param_seconds(P_ATTACK1, value).unwrap();
            assert!(seconds > previous, "the attack taper is not monotone at {value}");
            previous = seconds;
        }
    }

    // ── The patch bank ──

    #[test]
    fn the_patch_knob_lands_on_the_patch_it_names() {
        for (index, label) in PATCH_LABELS.iter().enumerate() {
            let knob = patch_knob(index);
            assert_eq!(patch_index(knob), index, "patch {index} knob {knob}");
            assert_eq!(discrete_label(P_PATCH, knob), Some(*label));
            let mut s = PhosphorSynth::new();
            s.set_parameter(P_PATCH, knob);
            assert_eq!(patch_index(s.params[P_PATCH]), index);
        }
        // Past the end is the last patch, not a panic.
        assert_eq!(patch_index(2.0), PATCH_COUNT - 1);
        assert_eq!(patch_index(-1.0), 0);
        assert_eq!(patch_knob(PATCH_COUNT + 10), patch_knob(PATCH_COUNT - 1));
    }

    #[test]
    fn patch_zero_is_the_default_parameter_block() {
        assert_eq!(PARAM_DEFAULTS, PhosphorSynth::params_for_patch(patch_knob(0)));
        // The selector defaults to the *midpoint* of its first step rather
        // than to zero, which is where `step_discrete` leaves it and where the
        // session format's position walk expects to find it.
        assert_eq!(PARAM_DEFAULTS[P_PATCH], patch_knob(0));
        assert_eq!(patch_index(PARAM_DEFAULTS[P_PATCH]), 0);
        assert_eq!(PhosphorSynth::new().params, PARAM_DEFAULTS);
        // GAIN is at the top of its travel, so the knob can only cut and the
        // headroom sweep at that setting is the whole of it.
        assert_eq!(PARAM_DEFAULTS[P_GAIN], 1.0);
        // DRIVE is at the bottom, where the stage is bit-for-bit the identity.
        assert_eq!(PARAM_DEFAULTS[P_DRIVE], 0.0);
    }

    #[test]
    fn selecting_a_patch_loads_its_whole_panel() {
        let mut s = PhosphorSynth::new();
        s.set_parameter(P_PATCH, patch_knob(1));
        assert_eq!(s.params, PhosphorSynth::params_for_patch(patch_knob(1)));
        // ...and moving another control afterwards does not reload it.
        s.set_parameter(P_CUTOFF, 0.123);
        assert!((s.params[P_CUTOFF] - 0.123).abs() < 1e-9);
        assert_eq!(s.params, {
            let mut want = PhosphorSynth::params_for_patch(patch_knob(1));
            want[P_CUTOFF] = 0.123;
            want
        });
    }

    // ── The bank sweep ──
    //
    // Two guarantees over all 229 patches, measured rather than argued:
    // **every patch speaks**, and **no patch reaches full scale**. They are
    // one loop because they are the same renders.
    //
    // The first exists because two Juno patches once rendered exact silence —
    // their sound source was the filter self-oscillating and nothing seeded
    // it — and a silent patch in a bank this size is a thing nobody finds by
    // playing. Four patches here are that same shape (`RESO DRONE`, `SINE
    // DRONE`, `FILTER SINE`, and `WIND` at the top of its resonance) so the
    // floor is not hypothetical.
    //
    // The second is the same statement `the_panel_at_its_extremes_stays_under
    // _the_ceiling` makes about the panel, made about the bank: single note
    // and eight-note chord, velocities 100 and 127.
    //
    // Split into one test per set so that they run in parallel — the sweep is
    // most of this file's wall time, and cargo gives a test a thread rather
    // than a loop iteration.

    /// The master limiter's ceiling, -1 dBFS. The same value as
    /// `LIMITER_CEILING` in the mixer; repeated because this file cannot
    /// reach it.
    const CEILING: f32 = 0.891;

    /// The two voicings the guarantee is stated over: one note, and the
    /// eight-note chord the panel sweep found its worst case on.
    const BANK_VOICINGS: [&[u8]; 2] = [&[60], &[36, 43, 48, 55, 60, 64, 67, 72]];

    /// How long patch `index` needs to reach its loudest, in buffers.
    ///
    /// Derived from the patch rather than fixed. An attack-decay-sustain
    /// envelope is at its loudest at the *end of its attack*, so what has to
    /// be covered is the slower of the two attacks — the amplifier's and the
    /// filter's — and not the decay after it. The bank holds both a
    /// one-millisecond pluck and a pad that takes two and a half seconds to
    /// arrive, and one length for both is either wrong about the pad or six
    /// times too long for everything else.
    ///
    /// The floor is 0.37 s, which is the length the whole project's headroom
    /// sweep in `tests/headroom.rs` uses and is well past every attack
    /// transient in the instrument.
    fn buffers_for(index: usize) -> usize {
        let p = PhosphorSynth::params_for_patch(patch_knob(index));
        let attack = attack_seconds(f64::from(p[P_ATTACK1]))
            .max(attack_seconds(f64::from(p[P_ATTACK2])));
        // 4 s is a bound on the work rather than a claim; nothing in the bank
        // asks for more than three.
        let buffers = ((attack + 0.3).min(4.0) * 44_100.0 / 64.0) as usize;
        buffers.max(256)
    }

    /// One patch, one voicing, one velocity. `hurried` speeds every clock the
    /// patch has — see [`sweep_set`].
    fn render_patch(
        index: usize,
        notes: &[u8],
        velocity: u8,
        buffers: usize,
        hurried: bool,
    ) -> Vec<f32> {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 64);
        s.set_parameter(P_PATCH, patch_knob(index));
        if hurried {
            s.set_parameter(P_LFO1_RATE, 0.85);
            s.set_parameter(P_LFO2_RATE, 0.85);
            s.set_parameter(P_SEQ_RATE, seq_rate_knob(16.0));
        }
        let events: Vec<MidiEvent> = notes.iter().map(|n| note_on(*n, velocity, 0)).collect();
        process_buffers(&mut s, &events, buffers)
    }

    /// Every patch of one set: both voicings at both velocities, and a fifth
    /// render with the clocks sped up. Returns the loudest it found.
    ///
    /// **The fifth render is what makes the other four enough.** An LFO at
    /// 0.05 Hz takes twenty seconds to visit both ends of its travel, and a
    /// wave sequence at a quarter of a hertz takes half a minute to walk its
    /// steps — so a patch whose loudest moment is an LFO opening the filter
    /// or a sequence reaching its loudest step would not be caught by any
    /// render short enough to run 229 times. Rendering for thirty seconds is
    /// not the answer; moving the clock is. The rates are parameters, so the
    /// last case is the patch as voiced with both LFOs at 12 Hz and the
    /// sequence clock at 16 — every excursion the patch's routings can
    /// produce, visited several times inside a third of a second.
    fn sweep_set(set: usize) -> (f32, String) {
        let (first, end) = bank_bounds(set);
        let mut worst = (0.0f32, String::new());
        for index in first..end {
            let name = PATCH_NAMES[index];
            let slot = PATCH_SLOTS[index];
            let buffers = buffers_for(index);
            let mut cases: Vec<(&[u8], u8, bool)> = Vec::new();
            for notes in BANK_VOICINGS {
                for velocity in [100u8, 127] {
                    cases.push((notes, velocity, false));
                }
            }
            cases.push((BANK_VOICINGS[1], 127, true));

            for (notes, velocity, hurried) in cases {
                let out = render_patch(index, notes, velocity, buffers, hurried);
                let measured = peak(&out);
                let how = if hurried { ", clocks hurried" } else { "" };
                assert!(
                    out.iter().all(|v| v.is_finite()),
                    "{slot} {name} produced a non-finite sample"
                );
                assert!(
                    out.iter().all(|v| v.abs() < 1.0),
                    "{slot} {name} on {} notes @{velocity}{how} reached full scale",
                    notes.len()
                );
                assert!(
                    measured <= CEILING,
                    "{slot} {name} on {} notes @{velocity}{how} peaks at {measured:.4}, \
                     past the ceiling",
                    notes.len()
                );
                // Audible on the quietest of the renders, which is the one
                // that would hide a patch that only speaks when it is played
                // hard or in a chord.
                if notes.len() == 1 && velocity == 100 {
                    assert!(
                        measured > 0.01,
                        "{slot} {name} is silent: peak {measured:.5} over {:.2} s",
                        buffers as f64 * 64.0 / 44_100.0
                    );
                }
                if measured > worst.0 {
                    worst = (measured, format!("{slot} {name} @{velocity}{how}"));
                }
            }
        }
        worst
    }

    /// Nothing in a set may be louder than the panel's own worst case, which
    /// `the_panel_at_its_extremes_stays_under_the_ceiling` measures at 0.6982
    /// — every level at the top, the filter open and resonant, the drive at
    /// the top and every matrix slot at full depth. A patch above that would
    /// mean the bank had found level the panel cannot reach, which cannot
    /// happen honestly.
    fn assert_set_is_bounded(set: usize, worst: &(f32, String), measured: f32) {
        assert!(
            worst.0 < crate::level::SATURATION_KNEE,
            "{}: the loudest patch ({}) reaches {:.4}, past the saturator's knee",
            BANK_NAMES[set],
            worst.1,
            worst.0
        );
        assert!(
            worst.0 > 0.15,
            "{}: nothing in the set reaches 0.15 ({}, {:.4})",
            BANK_NAMES[set],
            worst.1,
            worst.0
        );
        // ...and the figure each test's comment quotes, pinned so that the
        // two cannot drift. Half a decibel either way, which is wider than
        // any float difference and narrower than a revoicing.
        assert!(
            (measured * 0.945..measured * 1.06).contains(&worst.0),
            "{}: the loudest patch ({}) measures {:.4}, where the comment says {measured:.4}",
            BANK_NAMES[set],
            worst.1,
            worst.0
        );
    }

    /// The instrument's own eleven, and the loudest patch in the whole
    /// instrument with it: `P.11 SYNTH KIT` at 0.5357, which is eight
    /// *different drums* struck at once at velocity 127 — the one voicing in
    /// the sweep that a keymapped patch reads differently from a melodic one,
    /// and the reason it is louder than any chord.
    #[test]
    fn the_phosphor_set_is_audible_and_has_headroom() {
        assert_set_is_bounded(0, &sweep_set(0), 0.5357);
    }

    /// 128 patches, and the loudest of them is `A.77 Noisy Hit` at 0.4705:
    /// two noise sources and a saw through the drive at 0.6, with a
    /// three-hundred-millisecond envelope and no sustain at all.
    #[test]
    fn the_microkorg_set_is_audible_and_has_headroom() {
        assert_set_is_bounded(1, &sweep_set(1), 0.4705);
    }

    /// The loudest of the forty is `M.33 WIND` at 0.5045 — three noise
    /// sources through a resonant corner, which is the one patch in the set
    /// with no pitched oscillator to bound it.
    #[test]
    fn the_minimoog_set_is_audible_and_has_headroom() {
        assert_set_is_bounded(2, &sweep_set(2), 0.5045);
    }

    /// The loudest of the fifty is `W.49 WAVE KIT` at 0.4203, which is again
    /// a kit struck eight ways at once rather than a chord.
    #[test]
    fn the_wavestation_set_is_audible_and_has_headroom() {
        assert_set_is_bounded(3, &sweep_set(3), 0.4203);
    }



    /// The patches that stand in for a ring modulator and a cross modulator
    /// really are modulated.
    ///
    /// This instrument has neither, and what it has instead is oscillator D
    /// out of the mixer and onto the matrix — pointed at the amplitude it is
    /// amplitude modulation, which puts sum and difference sidebands either
    /// side of a carrier that stays in the middle; pointed at pitch and at an
    /// audio rate it is cross modulation. Both claims are made in the bank's
    /// comments, and both are cheap to check: render the patch as voiced and
    /// again with that one matrix amount at zero, and compare the shape of
    /// the spectrum.
    ///
    /// The weakest of the twelve is `Ring Chord` at 0.069, and it is weakest
    /// for a reason worth knowing: it is a triad, so the modulator lands on
    /// three carriers at once and each set of sidebands is a third of the
    /// level of a single note's. The strongest are the two that put D on
    /// pitch rather than on the amplitude.
    #[test]
    fn the_ring_and_cross_modulated_patches_really_are_modulated() {
        for (name, floor) in [
            ("Acid Ring Bass", 0.10),
            ("Techstep Ring Bass", 0.08),
            ("Unison Ring Lead", 0.20),
            ("Ring Chord", 0.05),
            ("Short Ring Perc.", 0.05),
            ("RingSync Bass", 0.08),
            ("X-Mod Perc.", 0.15),
            ("X-Mod Bass", 0.30),
            ("Domin8or", 0.20),
            ("OSC3 GROWL", 0.30),
            ("OSC3 VIBRATO", 0.30),
            ("Modulation Lead", 0.30),
        ] {
            let index = patch_named(name);
            let voiced = render_patch(index, &[60], 100, 400, false);

            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            s.set_parameter(P_PATCH, patch_knob(index));
            let mut found = false;
            for slot in 0..MOD_SLOTS {
                let source = Source::from_index(selector(s.params[p_mod_src(slot)], SOURCE_COUNT));
                if source == Source::OscD {
                    s.set_parameter(p_mod_amount(slot), bipolar_knob(0.0));
                    found = true;
                }
            }
            assert!(found, "{name} has no oscillator D routing at all");
            // ...and D has to be out of the mixer, or it is a fourth
            // oscillator rather than a modulator.
            assert_ne!(
                selector(s.params[P_D_MODE], 3),
                DMode::Audio as usize,
                "{name} leaves oscillator D in the mix"
            );

            let flat = process_buffers(&mut s, &[note_on(60, 100, 0)], 400);
            let distance: f64 = spectrum_shape(&voiced[2048..6144])
                .iter()
                .zip(spectrum_shape(&flat[2048..6144]).iter())
                .map(|(p, q)| (p - q).abs())
                .sum();
            assert!(
                distance > floor,
                "{name} sounds the same with its oscillator D routing switched off: \
                 {distance:.4} apart, where {floor} is the floor"
            );
        }
    }


    /// Every patch that carries a wave sequence is audibly changed by it.
    ///
    /// The failure this exists for is real and was found by it: five of the
    /// percussion programs — `Bleeps Perc.` among them — had a step list and
    /// an amplitude envelope a tenth of a second long, so the note was over
    /// before the second step arrived and the sequence did *nothing*. On the
    /// hardware an arpeggiator retriggers the envelope per step; here the
    /// step list plays under one held note, so a percussive arpeggio has to
    /// take its rhythm from the sequence's own rests and let the amplifier
    /// hold. That is what those patches do now, and this is what would catch
    /// it if one of them were revoiced back.
    ///
    /// Measured as the mean absolute difference between the patch as voiced
    /// and the same patch with every sequence selector at "off", against the
    /// patch's own level — so it is blind to how loud the patch is and asks
    /// only whether the step list is doing anything. The smallest in the bank
    /// is `W.42 STRING WAVE` at 0.17, where the sequence is deliberately on
    /// one oscillator of four and the other three are a string section
    /// holding still.
    #[test]
    fn every_sequenced_patch_is_audibly_sequenced() {
        let mut sequenced = 0;
        for index in 0..PATCH_COUNT {
            let p = PhosphorSynth::params_for_patch(patch_knob(index));
            let carries = (0..4).any(|i| seq_index(p[P_A_SEQ + i * P_OSC_STRIDE]).is_some());
            if !carries {
                continue;
            }
            sequenced += 1;

            let on = render_patch(index, &[60], 100, 700, false);
            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            s.set_parameter(P_PATCH, patch_knob(index));
            for i in 0..4 {
                s.set_parameter(P_A_SEQ + i * P_OSC_STRIDE, seq_knob(None));
            }
            let off = process_buffers(&mut s, &[note_on(60, 100, 0)], 700);

            let difference: f32 =
                on.iter().zip(off.iter()).map(|(a, b)| (a - b).abs()).sum::<f32>()
                    / on.len() as f32;
            let level = rms(&on);
            assert!(level > 0.001, "{} is too quiet to measure", PATCH_NAMES[index]);
            assert!(
                difference / level > 0.10,
                "{} {} sounds the same with its sequences switched off: {:.3} of its own \
                 level, so the step list is inaudible",
                PATCH_SLOTS[index],
                PATCH_NAMES[index],
                difference / level
            );
        }
        assert_eq!(sequenced, 54, "the bank should carry 54 sequenced patches");
    }


    #[test]
    fn patch_names_fit_the_editors_label_column() {
        for (index, label) in PATCH_LABELS.iter().enumerate() {
            assert!(
                label.chars().count() <= 12,
                "patch {index} label {label:?} needs {} of the 12 columns the panel leaves",
                label.chars().count()
            );
            assert!(!label.is_empty(), "patch {index} has no label");
            assert!(!PATCH_NAMES[index].is_empty(), "patch {index} has no name");
            // A label is the slot and then as much of the name as fits, so
            // that a player stepping through 229 patches can see where in the
            // bank they are. Both halves have to be there: a label that is
            // only its slot number would fit and say nothing.
            let slot = PATCH_SLOTS[index];
            let short: String = slot.chars().filter(|c| *c != '.').collect();
            assert!(
                label.starts_with(&format!("{short} ")),
                "patch {index} {label:?} does not lead with its slot {slot:?}"
            );
            assert!(
                label.chars().count() > short.chars().count() + 1,
                "patch {index} {label:?} is a slot with no name after it"
            );
        }
        for (index, slot) in PATCH_SLOTS.iter().enumerate() {
            assert_eq!(slot.chars().count(), 4, "slot {index} {slot:?} is not four columns");
            assert!(
                PATCH_SLOTS.iter().filter(|s| *s == slot).count() == 1,
                "slot {slot:?} is used twice"
            );
        }
    }

    /// Every chart row lands inside the panel's travel.
    ///
    /// `chart_params` writes a chart's numbers straight into the parameter
    /// block, and the block is what the engine reads — so a row that asks for
    /// a level of 1.4, a tuning of +30 semitones or 60 cents of detune would
    /// arrive as a knob outside its own range. Some of those clamp harmlessly
    /// and some do not: the headroom argument for this instrument is that the
    /// vector mix is bounded by the largest *level knob*, and a level knob
    /// above one would quietly undo it.
    ///
    /// This is also the check that catches the class of mistake a bank this
    /// size invites, and did: nineteen oscillators across the four sets were
    /// written with a semitone offset in the wavetable column — `+12` where
    /// `semi = 12` was meant — which the engine ignored on an analog shape,
    /// so those oscillators played at the wrong octave and nothing said so.
    #[test]
    fn every_chart_row_is_inside_the_panels_travel() {
        for index in 0..PATCH_COUNT {
            let params = PhosphorSynth::params_for_patch(patch_knob(index));
            for (i, value) in params.iter().enumerate() {
                assert!(
                    (0.0..=1.0).contains(value),
                    "{} {}: {} is {value}, outside the knob's travel",
                    PATCH_SLOTS[index],
                    PATCH_NAMES[index],
                    PARAM_NAMES[i]
                );
            }
            // The four oscillator levels are the ones the headroom argument
            // rests on, so they are named rather than left to the sweep above.
            for osc in 0..4 {
                let level = params[P_A_LEVEL + osc * P_OSC_STRIDE];
                assert!(
                    level <= 1.0,
                    "{} oscillator {osc} is at {level}",
                    PATCH_NAMES[index]
                );
            }
        }
    }

    /// The four sets, and the coarse move that stands in for a second knob.
    #[test]
    fn the_bank_divides_into_the_sets_it_names() {
        assert_eq!(BANK_NAMES.len(), BANK_COUNT);
        assert_eq!(BANK_FIRST[0], 0);
        assert_eq!(BANK_FIRST[BANK_COUNT], PATCH_COUNT);
        for (set, name) in BANK_NAMES.iter().enumerate() {
            let (first, end) = bank_bounds(set);
            assert!(first < end, "{name} is empty");
            assert!(name.chars().count() <= 12, "set name {name:?} does not fit the panel");
            for index in first..end {
                assert_eq!(patch_bank(index), set, "patch {index} is in the wrong set");
            }
        }
        // The slot letter and the set agree, which is the property that makes
        // a label readable without the set being printed beside it.
        for (index, slot) in PATCH_SLOTS.iter().enumerate() {
            let letter = slot.chars().next().unwrap();
            let want = match letter {
                'P' => 0,
                'A' | 'B' => 1,
                'M' => 2,
                _ => 3,
            };
            assert_eq!(patch_bank(index), want, "{slot} is not in {}", BANK_NAMES[want]);
        }
        // Out of range answers rather than panics, in both directions.
        assert_eq!(patch_bank(PATCH_COUNT + 99), BANK_COUNT - 1);
        assert_eq!(bank_bounds(BANK_COUNT + 5), bank_bounds(BANK_COUNT - 1));

        // The coarse move: up lands on the first patch of the next set, down
        // on the first of this one and then on the first of the previous, so
        // the two are inverses wherever the knob is standing.
        let mut knob = patch_knob(0);
        for (set, first) in BANK_FIRST.iter().enumerate().take(BANK_COUNT).skip(1) {
            knob = bank_step(knob, true);
            assert_eq!(patch_index(knob), *first, "up did not reach set {set}");
        }
        assert_eq!(patch_index(bank_step(knob, true)), PATCH_COUNT - 1, "up ran off the end");
        for (set, first) in BANK_FIRST.iter().enumerate().take(BANK_COUNT - 1).rev() {
            knob = bank_step(knob, false);
            assert_eq!(patch_index(knob), *first, "down did not reach set {set}");
        }
        assert_eq!(patch_index(bank_step(knob, false)), 0, "down ran off the start");
        // From the middle of a set, down goes to the top of that set first.
        let middle = patch_knob(BANK_FIRST[1] + 40);
        assert_eq!(patch_index(bank_step(middle, false)), BANK_FIRST[1]);
        assert_eq!(patch_index(bank_step(middle, true)), BANK_FIRST[2]);
    }

    /// The microKORG set is the factory list, checked against the factory
    /// list.
    ///
    /// The fixture is Korg's own Voice Name List as data — slot, MIDI number,
    /// row, name, category, single or layer, tempo and arpeggiator state —
    /// included verbatim rather than transcribed into Rust, so that a bank
    /// which agrees with itself cannot pass by agreeing with a typo. The same
    /// arrangement the DX7's ROM fixture uses, and for the same reason.
    ///
    /// What it holds this file to is names, slots and *order*, which is all
    /// of the microKORG set that is factory data. The parameter values are
    /// authored and no fixture can check them.
    #[test]
    fn the_microkorg_set_is_the_factory_list() {
        const LIST: &str = include_str!("../tests/data/microkorg_voices.json");
        let voices: serde_json::Value = serde_json::from_str(LIST).unwrap();
        let voices = voices.as_array().unwrap();
        assert_eq!(voices.len(), 128, "the factory list is 128 programs");

        let (first, end) = bank_bounds(1);
        assert_eq!(end - first, voices.len(), "the set is not the size of the list");

        for (offset, voice) in voices.iter().enumerate() {
            let index = first + offset;
            let slot = voice["slot"].as_str().unwrap();
            let name = voice["name"].as_str().unwrap();
            assert_eq!(PATCH_SLOTS[index], slot, "patch {index} is in the wrong slot");
            assert_eq!(PATCH_NAMES[index], name, "{slot} is not the factory name");
            // The MIDI program number is the position in the set, which is
            // what makes a program change land on the program it names.
            assert_eq!(voice["midi"].as_u64().unwrap() as usize, offset);
        }
    }

    /// No two patches in the bank are the same sound.
    ///
    /// The failure this exists for is the one a bank of 229 invites: a row
    /// copied to save typing and then not revoiced, which nobody finds by
    /// playing because nobody plays 229 patches side by side. Measured on the
    /// *shape* of the spectrum — `spectrum_shape` is normalised, so it is
    /// blind to level and to pitch — over the tenth of a second after the
    /// note starts, which is where a patch's identity is and where the short
    /// ones still have something to measure.
    ///
    /// The closest pair in the bank is `A.52 MG Bass 1` and `M.01 FAT BASS`
    /// at 0.053, and they are supposed to be close: MG is Korg's shorthand
    /// for Moog and both patches are the same instrument's bass sound, voiced
    /// from the same three sawtooths. That pair is what sets the threshold —
    /// everything else is further apart than the two patches that are trying
    /// to be the same thing.
    #[test]
    fn no_two_patches_in_the_bank_are_the_same_sound() {
        let prints: Vec<[f64; SPECTRUM_BINS]> = (0..PATCH_COUNT)
            .map(|index| spectrum_shape(&render_patch(index, &[60], 100, 700, false)[512..4608]))
            .collect();
        let mut closest = (f64::MAX, 0usize, 0usize);
        for (i, a) in prints.iter().enumerate() {
            for (j, b) in prints.iter().enumerate().skip(i + 1) {
                let distance: f64 = a.iter().zip(b.iter()).map(|(p, q)| (p - q).abs()).sum();
                if distance < closest.0 {
                    closest = (distance, i, j);
                }
            }
        }
        assert!(
            closest.0 > 0.03,
            "{} {} and {} {} are the same sound: {:.4} apart",
            PATCH_SLOTS[closest.1],
            PATCH_NAMES[closest.1],
            PATCH_SLOTS[closest.2],
            PATCH_NAMES[closest.2],
            closest.0
        );
    }

    /// The microKORG set is voiced to its categories, not just to its names.
    ///
    /// The factory list puts every program in a category, and a category is a
    /// claim about the sound: a Bass is dark and an S.E. is not. Measured in
    /// zero crossings a second, which separates them by an order of
    /// magnitude, and asserted on medians rather than on any one patch —
    /// `Sub Bass` and `Killa Beez` are both in this set and no per-patch rule
    /// covers them both.
    ///
    /// The nine medians as they stand: Bass 960, Synth 1352, Vocoder 1374,
    /// Strings/Pad 1478, Synth Lead 2141, Hit 2464, KBD 2686, Arpeggio 3196,
    /// S.E. 6135 crossings a second.
    #[test]
    fn the_microkorg_set_is_voiced_to_its_categories() {
        const LIST: &str = include_str!("../tests/data/microkorg_voices.json");
        let voices: serde_json::Value = serde_json::from_str(LIST).unwrap();
        let (first, _) = bank_bounds(1);

        let mut by_category: std::collections::BTreeMap<&str, Vec<f64>> =
            std::collections::BTreeMap::new();
        for (offset, voice) in voices.as_array().unwrap().iter().enumerate() {
            let out = render_patch(first + offset, &[60], 100, 700, false);
            by_category
                .entry(voice["category"].as_str().unwrap())
                .or_default()
                .push(brightness(&out));
        }

        let median = |category: &str| -> f64 {
            let mut v = by_category[category].clone();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() / 2]
        };
        let bass = median("Bass");
        let lead = median("Synth Lead");
        let effect = median("S.E.");
        assert!(bass < lead, "the basses are not darker than the leads: {bass:.0} vs {lead:.0}");
        assert!(
            effect > bass * 3.0,
            "the effects are not brighter than the basses: {effect:.0} vs {bass:.0}"
        );
        for category in by_category.keys() {
            assert!(
                bass <= median(category),
                "{category} is darker than the basses: {:.0} against {bass:.0}",
                median(category)
            );
        }
    }

    /// The two substitutions the bank makes are structural, so they can be
    /// asserted rather than only described.
    ///
    /// **The vocoder programs** are voiced as the carrier alone, and the
    /// carrier is the wavetable oscillator — so every one of the sixteen has
    /// to be reading the wave bank somewhere. **The arpeggio programs** are
    /// voiced as the sound the arpeggiator would be playing, so each of the
    /// eighteen has to carry either a wave sequence or a stepped modulation
    /// at the tempo the factory list gives it. B.21 S&H Signal is the one
    /// that takes the second route: its part is a sample-and-hold on pitch at
    /// a sixteenth of 138, which is what that name asks for and what no step
    /// list in the bank can do.
    #[test]
    fn the_vocoder_and_arpeggio_programs_are_voiced_as_they_claim() {
        const LIST: &str = include_str!("../tests/data/microkorg_voices.json");
        let voices: serde_json::Value = serde_json::from_str(LIST).unwrap();
        let (first, _) = bank_bounds(1);

        let mut vocoders = 0;
        let mut arpeggios = 0;
        let mut sequenced = 0;
        for (offset, voice) in voices.as_array().unwrap().iter().enumerate() {
            let index = first + offset;
            let slot = PATCH_SLOTS[index];
            let name = PATCH_NAMES[index];
            let p = PhosphorSynth::params_for_patch(patch_knob(index));
            let shapes: Vec<Shape> = (0..4)
                .map(|i| {
                    Shape::from_index(selector(p[P_A_WAVE + i * P_OSC_STRIDE], SHAPE_COUNT))
                })
                .collect();
            let sequences: Vec<Option<usize>> =
                (0..4).map(|i| seq_index(p[P_A_SEQ + i * P_OSC_STRIDE])).collect();

            match voice["category"].as_str().unwrap() {
                "Vocoder" => {
                    vocoders += 1;
                    assert!(
                        shapes.contains(&Shape::Table),
                        "{slot} {name} is a vocoder program with no wavetable carrier"
                    );
                }
                "Arpeggio" => {
                    arpeggios += 1;
                    let carries = sequences.iter().any(Option::is_some);
                    if carries {
                        sequenced += 1;
                    }
                    let stepped = (0..MOD_SLOTS).any(|slot| {
                        let source = Source::from_index(selector(p[p_mod_src(slot)], SOURCE_COUNT));
                        let dest = Dest::from_index(selector(p[p_mod_dest(slot)], DEST_COUNT));
                        let amount = bipolar(p[p_mod_amount(slot)]).abs();
                        matches!(source, Source::Lfo1 | Source::Lfo2)
                            && dest == Dest::Pitch
                            && amount > 0.1
                    });
                    assert!(
                        carries || stepped,
                        "{slot} {name} is an arpeggio program with neither a sequence nor \
                         a stepped modulation"
                    );
                }
                _ => {}
            }
        }
        assert_eq!(vocoders, 16, "the factory list has sixteen vocoder programs");
        assert_eq!(arpeggios, 18, "the factory list has eighteen arpeggio programs");
        assert_eq!(sequenced, 17, "seventeen of the eighteen carry a step list");
    }

    // ── Keymapped patches ──

    /// Which patch a name selects, for the tests that measure a particular
    /// one. By name rather than by position, because the bank grew around
    /// them: SYNTH KIT was the last patch when it was the only kit and is now
    /// the eleventh of 229.
    fn patch_named(name: &str) -> usize {
        PATCH_NAMES
            .iter()
            .position(|n| *n == name)
            .unwrap_or_else(|| panic!("no patch called {name}"))
    }

    /// The starter kit, which most of the keymap tests measure.
    fn kit_index() -> usize {
        patch_named("SYNTH KIT")
    }

    fn kit() -> PhosphorSynth {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 64);
        s.set_parameter(P_PATCH, patch_knob(kit_index()));
        s
    }

    /// 1.16 s of one note, which reaches past the longest recipe in the kit.
    fn strike(patch: usize, note: u8, velocity: u8) -> Vec<f32> {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 64);
        s.set_parameter(P_PATCH, patch_knob(patch));
        process_buffers(&mut s, &[note_on(note, velocity, 0)], 800)
    }

    /// How long the sound lasts, in samples: up to the last one above a
    /// hundredth of its own peak.
    fn sounding_samples(samples: &[f32]) -> usize {
        let floor = peak(samples) * 0.01;
        samples.iter().rposition(|v| v.abs() > floor).map_or(0, |i| i + 1)
    }

    /// Zero crossings per second over the sounding part.
    ///
    /// A cheap brightness measure, and the right one here: it separates a
    /// 55 Hz sine from a band of noise by two orders of magnitude, where a
    /// spectral centroid would need a transform per render and this file has
    /// forty of them.
    fn brightness(samples: &[f32]) -> f64 {
        let n = sounding_samples(samples).max(2);
        let window = &samples[..n];
        let crossings = window.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        crossings as f64 * 44_100.0 / n as f64
    }

    /// The point of a keymapped patch: three notes, three *sounds*, not one
    /// sound at three pitches.
    ///
    /// Asserted against a melodic patch on the same three notes, because
    /// "different" on its own is what a transposition also gives. What
    /// separates them is the *pattern* of the difference: on a melodic patch
    /// brightness follows the pitch and every note lasts the same time; on the
    /// kit the kick is fifty times darker than the hat and lasts eight times
    /// longer, which no transposition of one recipe can produce.
    #[test]
    fn a_keymapped_patch_plays_a_different_sound_on_every_note() {
        let kick = strike(kit_index(), 36, 110);
        let snare = strike(kit_index(), 38, 110);
        let hat = strike(kit_index(), 42, 110);

        for (name, out) in [("kick", &kick), ("snare", &snare), ("hat", &hat)] {
            assert!(peak(out) > 0.01, "the {name} is silent: peak {}", peak(out));
            assert!(out.iter().all(|v| v.is_finite()));
        }

        let (kick_hz, snare_hz, hat_hz) =
            (brightness(&kick), brightness(&snare), brightness(&hat));
        assert!(kick_hz < 1_500.0, "the kick is not a low sound: {kick_hz:.0} crossings/s");
        assert!(hat_hz > 6_000.0, "the hat is not a bright sound: {hat_hz:.0} crossings/s");
        assert!(
            snare_hz > kick_hz * 3.0 && snare_hz < hat_hz * 0.8,
            "the snare does not sit between the kick and the hat: \
             {kick_hz:.0}, {snare_hz:.0}, {hat_hz:.0} crossings/s"
        );

        let (kick_len, hat_len) = (sounding_samples(&kick), sounding_samples(&hat));
        assert!(
            kick_len > hat_len * 3,
            "the kick and the hat are the same length: {kick_len} against {hat_len} samples"
        );

        // The contrast. On a melodic patch those same three notes are one
        // recipe transposed: brightness tracks the pitch to within a factor of
        // two and every note lasts the same time.
        let melodic: Vec<Vec<f32>> = [36u8, 38, 42].iter().map(|n| strike(0, *n, 110)).collect();
        let ratio = brightness(&melodic[2]) / brightness(&melodic[0]);
        let pitch_ratio = 2.0f64.powf(6.0 / 12.0);
        assert!(
            (ratio / pitch_ratio).abs() > 0.5 && (ratio / pitch_ratio) < 2.0,
            "the melodic patch is not transposing: brightness moved {ratio:.2}x \
             where the pitch moved {pitch_ratio:.2}x"
        );
        let lengths: Vec<usize> = melodic.iter().map(|v| sounding_samples(v)).collect();
        let spread = lengths.iter().max().unwrap() - lengths.iter().min().unwrap();
        assert!(
            spread * 8 < *lengths.iter().max().unwrap(),
            "the melodic patch's notes are different lengths: {lengths:?}"
        );
        // ...and the kit's are not the same recipe at all: the hat is not the
        // kick moved six semitones.
        let kit_ratio = brightness(&hat) / brightness(&kick);
        assert!(
            kit_ratio > 5.0,
            "the kit's hat is only {kit_ratio:.2}x brighter than its kick, which is \
             what a transposition would give"
        );
    }

    #[test]
    fn every_zone_of_the_kit_sounds_and_they_are_all_different() {
        assert!(is_keymapped(kit_index()), "the kit is not keymapped");
        assert!(!is_keymapped(0), "the default patch is keymapped");
        assert_eq!(key_zone_count(kit_index()), 8);
        assert_eq!(key_zone_count(0), 0);
        assert_eq!(key_zone(kit_index(), 0), Some((36, "kick")));
        assert_eq!(key_zone(kit_index(), 8), None);
        // Out of range in either direction still answers rather than panics.
        assert_eq!(key_zone_count(PATCH_COUNT + 5), key_zone_count(PATCH_COUNT - 1));

        let mut renders = Vec::new();
        for zone in 0..key_zone_count(kit_index()) {
            let (note, name) = key_zone(kit_index(), zone).unwrap();
            let out = strike(kit_index(), note, 110);
            assert!(peak(&out) > 0.01, "{name} on note {note} is silent");
            assert!(peak(&out) < 0.891, "{name} peaks at {}", peak(&out));
            renders.push((name, out));
        }
        for (i, (name_a, a)) in renders.iter().enumerate() {
            for (name_b, b) in renders.iter().skip(i + 1) {
                let difference: f32 =
                    a.iter().zip(b.iter()).map(|(p, q)| (p - q).abs()).sum::<f32>()
                        / a.len() as f32;
                assert!(difference > 1e-4, "{name_a} and {name_b} sound the same");
            }
        }
    }

    /// Which patches read the keyboard as a set of sounds.
    fn kits() -> Vec<usize> {
        (0..PATCH_COUNT).filter(|i| is_keymapped(*i)).collect()
    }

    /// Every note of every kit, and the argument for having four of them.
    ///
    /// The bank carries the starter kit and three more — analog, wavetable
    /// and hand percussion — and the claim being made is that they are three
    /// obviously different sounds out of one engine rather than one kit
    /// re-tuned three times. So this measures both: within a kit no two notes
    /// may be the same sample for sample, and across kits the same note has
    /// to differ in *brightness*, which is the measure a transposition cannot
    /// move much and a change of waveform moves a lot.
    #[test]
    fn every_kit_speaks_on_every_note_and_no_two_kits_are_alike() {
        let kits = kits();
        assert_eq!(kits.len(), 4, "the bank should carry four kits");
        for &kit in &kits {
            let zones = key_zone_count(kit);
            assert!(zones >= 8, "{} has only {zones} zones", PATCH_NAMES[kit]);
            let mut renders = Vec::new();
            for zone in 0..zones {
                let (note, name) = key_zone(kit, zone).unwrap();
                let out = strike(kit, note, 110);
                let kit_name = PATCH_NAMES[kit];
                assert!(
                    peak(&out) > 0.01,
                    "{kit_name}: {name} on note {note} is silent ({:.5})",
                    peak(&out)
                );
                assert!(peak(&out) <= CEILING, "{kit_name}: {name} peaks at {}", peak(&out));
                assert!(out.iter().all(|v| v.is_finite()));
                renders.push((name, out));
            }
            for (i, (name_a, a)) in renders.iter().enumerate() {
                for (name_b, b) in renders.iter().skip(i + 1) {
                    let difference: f32 =
                        a.iter().zip(b.iter()).map(|(p, q)| (p - q).abs()).sum::<f32>()
                            / a.len() as f32;
                    assert!(
                        difference > 1e-4,
                        "{}: {name_a} and {name_b} sound the same",
                        PATCH_NAMES[kit]
                    );
                }
            }
        }

        // Across kits, on the notes they share: 36 is a kick on three of them
        // and folds to the lowest thing the percussion kit has, 38 is a
        // snare, 42 is a hi-hat.
        //
        // Measured by spectrum rather than by zero crossings, and the hi-hats
        // are why: three different hats made three different ways all cross
        // zero about 25,000 times a second, because that measure saturates on
        // anything mostly noise. `spectrum_shape` is normalised, so it is
        // blind to level and to pitch and sees only the shape — which is the
        // thing that is supposed to differ.
        for note in [36u8, 38, 42] {
            let shapes: Vec<(usize, [f64; SPECTRUM_BINS])> = kits
                .iter()
                .map(|&k| (k, spectrum_shape(&strike(k, note, 110)[..4096])))
                .collect();
            for (i, (kit_a, a)) in shapes.iter().enumerate() {
                for (kit_b, b) in shapes.iter().skip(i + 1) {
                    let distance: f64 =
                        a.iter().zip(b.iter()).map(|(p, q)| (p - q).abs()).sum();
                    assert!(
                        distance > 0.15,
                        "note {note} has the same spectrum on {} and {}: {distance:.3} apart, \
                         where two kits should be a quarter or more",
                        PATCH_NAMES[*kit_a],
                        PATCH_NAMES[*kit_b]
                    );
                }
            }
        }
    }

    /// A note the kit does not map plays the nearest one it does — not
    /// silence, and not that note transposed.
    #[test]
    fn an_unmapped_note_folds_onto_the_nearest_zone() {
        // Well above the top of the map, and well below the bottom.
        let kit = kit_index();
        assert_eq!(strike(kit, 100, 110), strike(kit, 49, 110), "the top did not fold");
        assert_eq!(strike(kit, 0, 110), strike(kit, 36, 110), "the bottom did not fold");
        // A tie goes to the lower entry, because the scan runs in note order
        // and only takes a strictly closer one. 37 is one from 36 and one
        // from 38.
        assert_eq!(strike(kit, 37, 110), strike(kit, 36, 110), "a tie went the wrong way");
        // ...and it really is the recipe rather than a transposition of it:
        // the fold sounds identical, where a transposed note would not.
        assert_ne!(strike(kit, 100, 110), strike(kit, 36, 110));
    }

    /// Velocity still works per note on a keymapped patch.
    #[test]
    fn velocity_still_works_on_every_zone() {
        for zone in 0..key_zone_count(kit_index()) {
            let (note, name) = key_zone(kit_index(), zone).unwrap();
            let soft = peak(&strike(kit_index(), note, 40));
            let hard = peak(&strike(kit_index(), note, 127));
            assert!(hard > soft * 1.2, "{name} is not velocity sensitive: {soft} vs {hard}");
        }
    }

    /// The CUTOFF knob is the one panel control a kit still answers, and it
    /// answers as an offset from where the patch left it.
    #[test]
    fn the_cutoff_knob_sweeps_a_whole_kit() {
        let render = |cutoff: f32| {
            let mut s = kit();
            s.set_parameter(P_CUTOFF, cutoff);
            let energy: f32 = process_buffers(&mut s, &[note_on(38, 110, 0)], 400)
                .iter()
                .map(|v| v * v)
                .sum();
            energy
        };
        let closed = render(0.2);
        let open = render(1.0);
        assert!(open > closed * 1.5, "the knob did nothing: {closed} against {open}");
        // At the patch's own setting the trim is zero, so the render is the
        // patch as voiced.
        let mut a = kit();
        let mut b = kit();
        b.set_parameter(P_CUTOFF, 0.60);
        assert_eq!(
            process_buffers(&mut a, &[note_on(38, 110, 0)], 100),
            process_buffers(&mut b, &[note_on(38, 110, 0)], 100)
        );
    }

    /// A drum has no sustain, and a voice that never ends is a kit that stops
    /// answering after eight hits. The envelope finishes at the end of its
    /// decay when the sustain is zero, whether or not the key has come up.
    #[test]
    fn a_percussive_note_frees_its_voice_without_a_note_off() {
        let mut s = kit();
        // Sixteen hits, none of them released, on a synth with eight voices.
        for _ in 0..16 {
            let out = process_buffers(&mut s, &[note_on(36, 110, 0)], 400);
            assert!(peak(&out) > 0.01, "a hit went missing");
        }
        // Nothing is left holding on.
        let out = process_buffers(&mut s, &[], 200);
        assert!(peak(&out) < 1e-4, "a drum note is still sounding: {}", peak(&out));
    }

    /// A whole kit struck at once, at the top of the velocity range, is the
    /// worst thing a keymapped patch can be asked for.
    #[test]
    fn the_whole_kit_at_once_has_headroom() {
        let mut s = kit();
        let events: Vec<MidiEvent> = (0..key_zone_count(kit_index()))
            .map(|zone| note_on(key_zone(kit_index(), zone).unwrap().0, 127, 0))
            .collect();
        let out = process_buffers(&mut s, &events, 400);
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(out.iter().all(|v| v.abs() < 1.0), "the kit reached full scale");
        assert!(peak(&out) <= 0.891, "the whole kit peaks at {:.4}", peak(&out));
        assert!(peak(&out) > 0.05, "the whole kit is inaudible: {:.4}", peak(&out));
    }

    // ── The wavetable bank ──

    #[test]
    fn every_generated_waveform_is_bounded_and_centred() {
        let bank = wave_bank();
        assert_eq!(WAVE_NAMES.len(), WAVE_COUNT);
        assert_eq!(WAVES.len(), WAVE_COUNT);
        for (wave, wave_name) in WAVE_NAMES.iter().enumerate() {
            for level in 0..MIP_LEVELS {
                let base = (wave * MIP_LEVELS + level) * TABLE_LEN;
                let slice = &bank.samples[base..base + TABLE_LEN];
                let peak = slice.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
                // Bounded by one at every level: the vector mix's headroom
                // argument is that its weights sum to one and every source is
                // inside the unit interval, which is only true if this is.
                assert!(peak <= 1.0 + 1e-6, "{wave_name} level {level} peaks at {peak}");
                // No DC: an offset would be passed straight through the filter
                // and the amplifier and would show up as a click on every
                // note-off.
                let mean: f64 = slice.iter().map(|v| f64::from(*v)).sum::<f64>()
                    / TABLE_LEN as f64;
                assert!(mean.abs() < 1e-3, "{wave_name} level {level} has {mean} of DC");
            }
            // The full-band copy actually reaches the top of its range, or the
            // waveform is quieter than every other one in the bank.
            let base = wave * MIP_LEVELS * TABLE_LEN;
            let peak = bank.samples[base..base + TABLE_LEN]
                .iter()
                .map(|v| v.abs())
                .fold(0.0f32, f32::max);
            assert!((peak - 1.0).abs() < 1e-5, "{wave_name} peaks at {peak}");
        }
    }

    #[test]
    fn no_two_waveforms_in_the_bank_are_the_same_sound() {
        let bank = wave_bank();
        for (a, name_a) in WAVE_NAMES.iter().enumerate() {
            for (b, name_b) in WAVE_NAMES.iter().enumerate().skip(a + 1) {
                let base_a = a * MIP_LEVELS * TABLE_LEN;
                let base_b = b * MIP_LEVELS * TABLE_LEN;
                // Compared as a spectrum rather than sample by sample, because
                // two waveforms differing only in phase are the same sound.
                let mut difference = 0.0f64;
                for harmonic in 1..=32usize {
                    let magnitude = |base: usize| {
                        let (mut re, mut im) = (0.0f64, 0.0f64);
                        for (i, v) in bank.samples[base..base + TABLE_LEN].iter().enumerate() {
                            let w = TWO_PI * (harmonic * i % TABLE_LEN) as f64 / TABLE_LEN as f64;
                            re += f64::from(*v) * w.cos();
                            im += f64::from(*v) * w.sin();
                        }
                        (re * re + im * im).sqrt() / TABLE_LEN as f64
                    };
                    difference += (magnitude(base_a) - magnitude(base_b)).abs();
                }
                assert!(
                    difference > 0.01,
                    "{name_a} and {name_b} have the same spectrum ({difference:.5})"
                );
            }
        }
    }

    #[test]
    fn the_band_limiting_picks_a_level_that_cannot_alias() {
        let sr = 44_100.0;
        for note in 0u8..128 {
            let freq = note_to_freq(note);
            let level = mip_level(freq, sr);
            let top = (MAX_HARMONICS >> level).max(1);
            assert!(
                top as f64 * freq <= sr * 0.5 || level == MIP_LEVELS - 1,
                "note {note} at {freq:.1} Hz reads level {level}, whose {top}th harmonic is at {}",
                top as f64 * freq
            );
        }
        // The bottom of the keyboard gets the whole band, and the level only
        // ever rises with pitch.
        assert_eq!(mip_level(note_to_freq(21), sr), 0);
        let mut previous = 0;
        for note in 0u8..128 {
            let level = mip_level(note_to_freq(note), sr);
            assert!(level >= previous, "the level fell between {} and {note}", note - 1);
            previous = level;
        }
    }

    /// The knob is a position in the bank, not a switch: halfway between two
    /// waveforms is half of each.
    #[test]
    fn the_wave_knob_crossfades_between_neighbours() {
        let bank = wave_bank();
        let step = 1.0 / (WAVE_COUNT - 1) as f64;
        for phase in [0.0, 0.13, 0.37, 0.61, 0.88] {
            let a = bank.at(0.0, 0, phase);
            let b = bank.at(step, 0, phase);
            let middle = bank.at(step * 0.5, 0, phase);
            assert!(
                (middle - (a + b) * 0.5).abs() < 1e-6,
                "at phase {phase} the midpoint is {middle}, not {}",
                (a + b) * 0.5
            );
        }
        // The ends are exactly the first and last waveform.
        for phase in [0.0, 0.25, 0.5] {
            assert!((bank.at(0.0, 0, phase) - bank.sample(0, 0, phase)).abs() < 1e-12);
            assert!(
                (bank.at(1.0, 0, phase) - bank.sample(WAVE_COUNT - 1, 0, phase)).abs() < 1e-12
            );
        }
    }

    // ── Wave sequencing ──
    //
    // The claim to be proved is "the timbre evolves on its own", and the
    // measurement has to be able to tell that apart from "the note got louder"
    // and "the note changed pitch", because a sequence does both of those too.
    //
    // So what is measured is the *shape* of the spectrum: magnitudes at 24
    // log-spaced frequencies, normalised to sum to one, and the L1 distance
    // between the two most distant windows of a render. Zero is one unchanging
    // timbre; two is two windows with no frequency in common. Normalising is
    // what makes it blind to level, and using bins rather than harmonics of
    // the played note is what makes it blind to pitch.
    //
    // A direct transform rather than an FFT because this crate has neither and
    // the measurement is a handful of windows rather than a spectrogram: 24
    // bins over a 4096-point window is 200k multiply-adds, which is nothing
    // against the render that produced it.

    const SPECTRUM_BINS: usize = 24;

    fn bin_hz(bin: usize) -> f64 {
        200.0 * (10_000.0f64 / 200.0).powf(bin as f64 / (SPECTRUM_BINS - 1) as f64)
    }

    fn magnitude_of(window: &[f32], hz: f64, sr: f64) -> f64 {
        let w = TWO_PI * hz / sr;
        let (mut re, mut im) = (0.0, 0.0);
        for (n, v) in window.iter().enumerate() {
            let p = w * n as f64;
            re += f64::from(*v) * p.cos();
            im -= f64::from(*v) * p.sin();
        }
        (re * re + im * im).sqrt() / window.len() as f64
    }

    /// The window's spectrum, normalised to sum to one.
    fn spectrum_shape(window: &[f32]) -> [f64; SPECTRUM_BINS] {
        let mut out = [0.0; SPECTRUM_BINS];
        let mut total = 0.0;
        for (bin, slot) in out.iter_mut().enumerate() {
            *slot = magnitude_of(window, bin_hz(bin), 44_100.0);
            total += *slot;
        }
        if total > 0.0 {
            for slot in &mut out {
                *slot /= total;
            }
        }
        out
    }

    /// How far apart the two most different windows of a render are.
    fn timbre_travel(samples: &[f32], window: usize) -> f64 {
        let shapes: Vec<[f64; SPECTRUM_BINS]> =
            samples.chunks_exact(window).map(spectrum_shape).collect();
        let mut worst = 0.0f64;
        for (i, a) in shapes.iter().enumerate() {
            for b in shapes.iter().skip(i + 1) {
                worst = worst.max(a.iter().zip(b.iter()).map(|(p, q)| (p - q).abs()).sum::<f64>());
            }
        }
        worst
    }

    /// How deeply the amplitude envelope swings at `hz`, as a fraction of its
    /// own mean — which is what a rhythm is, measured.
    ///
    /// Hann-windowed, and it has to be: the envelope is sampled every 64
    /// samples and no interesting rate fits a whole number of times into a
    /// render, so an unwindowed transform reads the note's own attack as
    /// several tenths of modulation at every frequency. That is what the first
    /// version of this measured, and it put a patch's rhythm at 0.05 whether
    /// its sequences were running or not.
    fn envelope_modulation(samples: &[f32], hz: f64) -> f64 {
        const HOP: usize = 64;
        let env: Vec<f32> = samples.chunks_exact(HOP).map(peak).collect();
        if env.len() < 8 {
            return 0.0;
        }
        let rate = 44_100.0 / HOP as f64;
        let mean = f64::from(env.iter().sum::<f32>()) / env.len() as f64;
        if mean <= 0.0 {
            return 0.0;
        }
        let w = TWO_PI * hz / rate;
        let (mut re, mut im, mut norm) = (0.0, 0.0, 0.0);
        for (n, v) in env.iter().enumerate() {
            let hann = 0.5 - 0.5 * (TWO_PI * n as f64 / env.len() as f64).cos();
            let p = w * n as f64;
            let x = (f64::from(*v) - mean) * hann;
            re += x * p.cos();
            im -= x * p.sin();
            norm += hann;
        }
        2.0 * (re * re + im * im).sqrt() / norm / mean
    }

    /// One wavetable oscillator alone at the A corner, filter open, envelope
    /// holding — a panel on which nothing moves except what the sequence does.
    ///
    /// TABLE is left where the patch has it, which is what makes the sequence
    /// play as written: the knob is an offset from the patch's own value.
    fn bare_sequence_panel(seq: Option<usize>, hz: f32) -> PhosphorSynth {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 64);
        s.set_parameter(P_VECTOR_X, 0.0);
        s.set_parameter(P_VECTOR_Y, 0.0);
        s.set_parameter(P_A_WAVE, knob_for(Shape::Table as usize, SHAPE_COUNT));
        s.set_parameter(P_A_SEQ, seq_knob(seq));
        s.set_parameter(P_SEQ_RATE, seq_rate_knob(hz));
        s.set_parameter(P_CUTOFF, 1.0);
        s.set_parameter(P_FILTER_ENV, 0.5);
        s.set_parameter(P_ATTACK1, 0.0);
        s.set_parameter(P_DECAY1, 0.0);
        s.set_parameter(P_SUSTAIN1, 1.0);
        s
    }

    /// The headline claim, and the one thing no envelope can imitate: with a
    /// sequence on, the timbre keeps moving, and with the same panel and no
    /// sequence it does not.
    ///
    /// Measured on one oscillator with nothing else on the panel moving, so
    /// what is left is the step list and only the step list. Every sequence in
    /// the bank travels between 0.98 and 1.74 where the same oscillator with
    /// its selector at "off" travels 0.089 — an order of magnitude, on a
    /// measurement that is blind to level and to pitch.
    #[test]
    fn a_wave_sequence_moves_the_timbre_and_an_unsequenced_oscillator_does_not() {
        let still =
            process_buffers(&mut bare_sequence_panel(None, 4.0), &[note_on(60, 100, 0)], 700);
        let unsequenced = timbre_travel(&still[..44_100], 4096);
        assert!(
            unsequenced < 0.2,
            "the panel moves on its own: an unsequenced oscillator travels {unsequenced:.3}"
        );

        for seq in 0..SEQ_COUNT {
            let out = process_buffers(
                &mut bare_sequence_panel(Some(seq), 4.0),
                &[note_on(60, 100, 0)],
                700,
            );
            let travel = timbre_travel(&out[..44_100], 4096);
            assert!(peak(&out) > 0.01, "{} is silent", seq_name(seq));
            assert!(out.iter().all(|v| v.is_finite()), "{} is not finite", seq_name(seq));
            assert!(
                travel > 0.6,
                "{} holds one timbre: it travels {travel:.3} where a still panel travels \
                 {unsequenced:.3}",
                seq_name(seq)
            );
            assert!(
                travel > unsequenced * 4.0,
                "{} moves no more than the panel it is on: {travel:.3} against {unsequenced:.3}",
                seq_name(seq)
            );
        }
    }

    /// A sequence that plays once stops on its last step and holds it; one
    /// that loops does not.
    ///
    /// Measured over the last two seconds of a four-second note, which is past
    /// the end of both one-shots in the bank. They travel 0.202 and 0.314
    /// there, against 1.002 to 1.557 for the six that loop.
    #[test]
    fn a_one_shot_sequence_stops_where_a_loop_carries_on() {
        for seq in 0..SEQ_COUNT {
            let out = process_buffers(
                &mut bare_sequence_panel(Some(seq), 4.0),
                &[note_on(60, 100, 0)],
                2800,
            );
            let late = timbre_travel(&out[out.len() - 88_200..], 4096);
            if seq_loops(seq) {
                assert!(
                    late > 0.7,
                    "{} loops but has stopped moving: {late:.3}",
                    seq_name(seq)
                );
            } else {
                assert!(
                    late < 0.45,
                    "{} plays once but is still moving two seconds after it ended: {late:.3}",
                    seq_name(seq)
                );
                assert!(peak(&out[out.len() - 88_200..]) > 0.01, "{} went silent", seq_name(seq));
            }
        }
    }

    /// The cursor itself, away from the audio it produces: a one-shot ends on
    /// its last step and stays there, a loop comes back round to its first.
    #[test]
    fn the_cursor_holds_a_one_shot_and_wraps_a_loop() {
        let run = |slot: usize, samples: usize| {
            let mut cursor = SeqCursor::start(slot);
            let mut visited = Vec::new();
            for _ in 0..samples {
                cursor.advance(40.0, 44_100.0);
                if visited.last() != Some(&cursor.index) {
                    visited.push(cursor.index);
                }
            }
            (cursor, visited)
        };

        for index in 0..SEQ_COUNT {
            let steps = seq_step_count(index);
            // Long enough for several passes at 40 Hz: the longest sequence in
            // the bank is 16 ticks, which is 0.4 s.
            let (cursor, visited) = run(index + 1, 44_100 * 3);
            assert!(cursor.is_running(), "{} did not start", seq_name(index));
            if seq_loops(index) {
                assert!(!cursor.held, "{} stopped, but it loops", seq_name(index));
                assert!(
                    visited.len() > steps,
                    "{} visited {} of its {steps} steps and did not come round",
                    seq_name(index),
                    visited.len()
                );
                assert!(visited.contains(&0) && visited.contains(&(steps - 1)));
            } else {
                assert!(cursor.held, "{} loops, but it should play once", seq_name(index));
                assert_eq!(
                    cursor.index,
                    steps - 1,
                    "{} stopped somewhere other than its last step",
                    seq_name(index)
                );
                assert_eq!(
                    visited.len(),
                    steps,
                    "{} did not visit every step once",
                    seq_name(index)
                );
                // Held really means held: the point it hands out stops moving.
                let mut settled = cursor;
                let first = settled.advance(40.0, 44_100.0);
                for _ in 0..10_000 {
                    let point = settled.advance(40.0, 44_100.0);
                    assert_eq!(point.wave, first.wave);
                    assert_eq!(point.mix, first.mix);
                    assert_eq!(point.level, first.level);
                    assert_eq!(point.ratio, first.ratio);
                }
            }
        }
    }

    /// The clock runs at the rate the knob names, and the readout under the
    /// bar is the tick length that rate works out to.
    #[test]
    fn the_sequence_clock_runs_at_the_rate_the_knob_names() {
        // The taper's ends, and the octave count the bank names rates with.
        assert!((seq_hz(0.0) - SEQ_MIN_HZ).abs() < 1e-12);
        assert!((seq_hz(1.0) - SEQ_MAX_HZ).abs() < 1e-9);
        assert!(
            (SEQ_MIN_HZ * 2.0f64.powf(f64::from(SEQ_OCTAVES)) - SEQ_MAX_HZ).abs() < 1e-9,
            "SEQ_OCTAVES does not span the taper"
        );
        for (octaves, hz) in [(0.0, 0.25), (3.0, 2.0), (5.0, 8.0), (7.0, 32.0)] {
            let knob = seq_rate_at(octaves);
            assert!(
                (seq_hz(f64::from(knob)) - hz).abs() < 1e-6,
                "seq_rate_at({octaves}) is {} Hz, not {hz}",
                seq_hz(f64::from(knob))
            );
            assert!((f64::from(seq_rate_knob(hz as f32)) - f64::from(knob)).abs() < 1e-6);
            // The panel prints the tick length rather than the rate.
            let seconds = param_seconds(P_SEQ_RATE, knob).unwrap();
            assert!((seconds - 1.0 / hz).abs() < 1e-6, "the panel reads {seconds} s at {hz} Hz");
        }

        // ...and the cursor actually advances at it. `gate 4` is four one-tick
        // steps, so at 8 Hz it crosses eight boundaries a second.
        for hz in [2.0f64, 8.0, 30.0] {
            let mut cursor = SeqCursor::start(1);
            let mut boundaries = 0;
            let mut previous = cursor.index;
            for _ in 0..44_100 {
                cursor.advance(hz, 44_100.0);
                if cursor.index != previous {
                    boundaries += 1;
                    previous = cursor.index;
                }
            }
            let measured = f64::from(boundaries);
            assert!(
                (measured - hz).abs() <= 1.0,
                "at {hz} Hz the cursor crossed {measured} step boundaries in a second"
            );
        }
    }

    /// The cursor is total. `params` is public, so the rate can arrive as
    /// anything at all, and a sequence is the one thing on this panel with a
    /// state machine in it.
    #[test]
    fn the_sequence_cursor_is_bounded_and_total() {
        for hz in [0.0f64, -1.0, 1e9, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            for slot in 0..=SEQ_COUNT {
                let mut cursor = SeqCursor::start(slot);
                let mut previous = cursor.index;
                for _ in 0..20_000 {
                    let point = cursor.advance(hz, 44_100.0);
                    assert!(
                        (0.0..1.0).contains(&cursor.phase),
                        "rate {hz} put the cursor at phase {}",
                        cursor.phase
                    );
                    assert!((0.0..=1.0).contains(&point.mix), "rate {hz} gave mix {}", point.mix);
                    assert!((0.0..=1.0).contains(&point.level));
                    assert!(point.ratio.is_finite() && point.ratio > 0.0);
                    assert!(point.wave.iter().all(|w| (0.0..=1.0).contains(w)));
                    // At most one step boundary per sample, which is what
                    // makes the work per sample bounded.
                    if let Some(seq) = cursor.seq {
                        let moved = cursor.index != previous;
                        let stepped_by_one = (previous + 1) % seq.steps.len() == cursor.index;
                        assert!(!moved || stepped_by_one, "rate {hz} skipped a step");
                    }
                    previous = cursor.index;
                }
            }
        }
        // A selector position past the end of the bank is off, not a panic.
        assert!(!SeqCursor::start(SEQ_COUNT + 5).is_running());
        assert!(!SeqCursor::start(0).is_running());
        assert_eq!(SeqCursor::start(0).advance(4.0, 44_100.0).level, 1.0);
    }

    /// The matrix reaches the clock: a slot pointed at `seq hz` makes the
    /// pattern run faster, and the rhythm moves with it.
    ///
    /// `gate 4` at 4 Hz gates the amplitude twice a second — its two loud
    /// steps — so the envelope swings at 2 Hz. Velocity at full and a slot at
    /// +0.5 multiplies the clock by 2^1.5, which puts the same swing at 5.7 Hz.
    #[test]
    fn the_matrix_can_push_the_sequence_clock() {
        let render = |amount: Option<f32>| {
            let mut s = bare_sequence_panel(Some(0), 4.0);
            if let Some(amount) = amount {
                s.set_parameter(p_mod_src(0), knob_for(Source::Velocity as usize, SOURCE_COUNT));
                s.set_parameter(p_mod_dest(0), knob_for(Dest::SeqRate as usize, DEST_COUNT));
                s.set_parameter(p_mod_amount(0), bipolar_knob(amount));
            }
            process_buffers(&mut s, &[note_on(60, 127, 0)], 1400)
        };
        let plain = render(None);
        let pushed = render(Some(0.5));

        let slow = 2.0;
        let fast = slow * 2.0f64.powf(0.5 * SEQ_RATE_OCTAVES);
        assert!(
            envelope_modulation(&plain, slow) > 0.2,
            "the gate is not audible at rest: {:.4}",
            envelope_modulation(&plain, slow)
        );
        assert!(
            envelope_modulation(&pushed, fast) > envelope_modulation(&plain, fast) * 4.0,
            "pushing the clock did not move the rhythm to {fast:.1} Hz: {:.4} against {:.4}",
            envelope_modulation(&pushed, fast),
            envelope_modulation(&plain, fast)
        );
        assert!(
            envelope_modulation(&pushed, slow) < envelope_modulation(&plain, slow) * 0.5,
            "the rhythm is still at {slow} Hz after the clock was pushed: {:.4} against {:.4}",
            envelope_modulation(&pushed, slow),
            envelope_modulation(&plain, slow)
        );
    }

    /// A sequence on an oscillator that is not reading the wavetable bank is
    /// still a rhythm and a riff — the step's waveform is the one column that
    /// needs a table to land on.
    #[test]
    fn a_sequence_on_a_shape_with_no_table_is_still_a_rhythm() {
        let render = |shape: Shape, seq: Option<usize>| {
            let mut s = bare_sequence_panel(seq, 4.0);
            s.set_parameter(P_A_WAVE, knob_for(shape as usize, SHAPE_COUNT));
            process_buffers(&mut s, &[note_on(60, 110, 0)], 700)
        };
        for shape in [Shape::Saw, Shape::Pulse, Shape::Triangle, Shape::Sine, Shape::Noise] {
            let gated = render(shape, Some(0));
            let plain = render(shape, None);
            assert!(
                envelope_modulation(&gated, 2.0) > envelope_modulation(&plain, 2.0) * 4.0,
                "{shape:?} does not answer the gate: {:.4} against {:.4}",
                envelope_modulation(&gated, 2.0),
                envelope_modulation(&plain, 2.0)
            );
            // ...and the WAVE switch still wins: the step's waveform column
            // did not quietly turn the oscillator into a wavetable.
            let table = render(Shape::Table, Some(0));
            let difference: f32 = gated
                .iter()
                .zip(table.iter())
                .map(|(a, b)| (a - b).abs())
                .sum::<f32>()
                / gated.len() as f32;
            assert!(difference > 1e-4, "{shape:?} sounds like the wavetable oscillator");
        }
    }

    /// Moving a sequence selector under a held note takes effect on that note.
    ///
    /// The step list is resolved at note-on the way the key map is, but the
    /// *reference* is checked every sample, so the panel keeps its promise
    /// that every control is live.
    #[test]
    fn moving_a_sequence_selector_under_a_held_note_takes_effect() {
        let mut held = bare_sequence_panel(Some(0), 4.0);
        let mut switched = bare_sequence_panel(Some(0), 4.0);
        let before_a = process_buffers(&mut held, &[note_on(60, 110, 0)], 100);
        let before_b = process_buffers(&mut switched, &[note_on(60, 110, 0)], 100);
        assert_eq!(before_a, before_b, "the two panels did not start the same");

        switched.set_parameter(P_A_SEQ, seq_knob(Some(3)));
        let after_a = process_buffers(&mut held, &[], 200);
        let after_b = process_buffers(&mut switched, &[], 200);
        let difference: f32 = after_a
            .iter()
            .zip(after_b.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / after_a.len() as f32;
        assert!(difference > 1e-4, "the selector did nothing under a held note ({difference:e})");

        // ...and switching it off leaves the oscillator as the panel has it.
        let mut turned_off = bare_sequence_panel(Some(0), 4.0);
        process_buffers(&mut turned_off, &[note_on(60, 110, 0)], 100);
        turned_off.set_parameter(P_A_SEQ, seq_knob(None));
        let out = process_buffers(&mut turned_off, &[], 400);
        assert!(
            timbre_travel(&out, 4096) < 0.2,
            "the oscillator is still sequenced after the selector was turned off: {:.3}",
            timbre_travel(&out, 4096)
        );
    }

    /// The two sequenced patches are actually made of their sequences: turning
    /// the four selectors off changes the render by a substantial fraction of
    /// its own level, and the rhythm at the patch's own clock collapses.
    #[test]
    fn the_sequenced_patches_are_mostly_their_sequences() {
        // SEQ PAD's clock is 2 Hz and its level steps land on it; SEQ RIFF's
        // is 8 Hz and `gate 4` puts two loud steps in every four, so its
        // rhythm is at half the clock.
        for (name, clock) in [("SEQ PAD", 2.0), ("SEQ RIFF", 4.0)] {
            let index = PATCH_NAMES.iter().position(|n| *n == name).unwrap();
            let render = |off: bool| {
                let mut s = PhosphorSynth::new();
                s.init(44_100.0, 64);
                s.set_parameter(P_PATCH, patch_knob(index));
                if off {
                    for i in 0..4 {
                        s.set_parameter(P_A_SEQ + i * P_OSC_STRIDE, seq_knob(None));
                    }
                }
                process_buffers(&mut s, &[note_on(60, 100, 0)], 3400)
            };
            let on = render(false);
            let off = render(true);

            let difference = on
                .iter()
                .zip(off.iter())
                .map(|(a, b)| f64::from((a - b).abs()))
                .sum::<f64>()
                / on.len() as f64;
            let level = f64::from(rms(&on));
            assert!(
                difference > level * 0.25,
                "{name} barely uses its sequences: turning them off moved {difference:.5} \
                 against {level:.5} RMS"
            );

            // A second in, so the attack is not what is being transformed.
            let body = &on[44_100..];
            let quiet = &off[44_100..];
            assert!(
                envelope_modulation(body, clock) > 0.04,
                "{name} has no rhythm at its own {clock} Hz clock: {:.4}",
                envelope_modulation(body, clock)
            );
            assert!(
                envelope_modulation(body, clock) > envelope_modulation(quiet, clock) * 3.0,
                "{name}'s rhythm is not the sequences': {:.4} against {:.4} with them off",
                envelope_modulation(body, clock),
                envelope_modulation(quiet, clock)
            );
        }
    }

    /// The bank, as the editor and the engine both have to read it.
    #[test]
    fn the_sequence_bank_is_the_shape_the_editor_expects() {
        assert_eq!(SEQ_BANK.len(), SEQ_COUNT);
        assert_eq!(SEQ_SLOTS, SEQ_COUNT + 1);
        assert_eq!(SEQ_LABELS[0], "off");
        for (index, seq) in SEQ_BANK.iter().enumerate() {
            assert!(!seq.name.is_empty(), "sequence {index} has no name");
            assert!(
                seq.name.chars().count() <= 12,
                "sequence {index} label {:?} does not fit the panel",
                seq.name
            );
            assert!(!seq.steps.is_empty(), "{} has no steps", seq.name);
            assert!(seq.steps.len() <= MAX_STEPS, "{} has more than MAX_STEPS", seq.name);
            for (n, step) in seq.steps.iter().enumerate() {
                assert!(step.ticks >= 1, "{} step {n} lasts no time", seq.name);
                assert!(
                    (0.0..=1.0).contains(&step.fade),
                    "{} step {n} crossfades {} of itself",
                    seq.name,
                    step.fade
                );
                assert!((0.0..=1.0).contains(&step.level), "{} step {n} is out of range", seq.name);
                assert!(
                    (step.wave as usize) < WAVE_COUNT,
                    "{} step {n} names waveform {} of {WAVE_COUNT}",
                    seq.name,
                    step.wave
                );
                assert!(
                    step.pitch.abs() <= 24,
                    "{} step {n} transposes past two octaves",
                    seq.name
                );
            }
            // The accessors an editor reads it through.
            assert_eq!(seq_name(index), seq.name);
            assert_eq!(seq_step_count(index), seq.steps.len());
            assert_eq!(seq_loops(index), seq.looping);
            assert_eq!(SEQ_LABELS[index + 1], seq.name);
            // ...and the selector lands where it says it does.
            assert_eq!(seq_index(seq_knob(Some(index))), Some(index));
            assert_eq!(discrete_label(P_A_SEQ, seq_knob(Some(index))), Some(seq.name));
        }
        assert_eq!(seq_index(seq_knob(None)), None);
        assert_eq!(discrete_label(P_D_SEQ, seq_knob(None)), Some("off"));
        // Out of range in either direction still answers.
        assert_eq!(seq_knob(Some(SEQ_COUNT + 9)), seq_knob(Some(SEQ_COUNT - 1)));
        assert_eq!(seq_name(SEQ_COUNT + 9), seq_name(SEQ_COUNT - 1));
        assert_eq!(seq_index(9.0), Some(SEQ_COUNT - 1));
        assert_eq!(seq_index(-1.0), None);
        // Both endings are represented, or one of them is untested by every
        // test above that walks the bank.
        assert!(SEQ_BANK.iter().any(|s| s.looping));
        assert!(SEQ_BANK.iter().any(|s| !s.looping));
        // And a waveform is named by index rather than swept to: the two ends
        // of the bank are the first and last waveform exactly.
        assert!((wave_position(0) - 0.0).abs() < 1e-12);
        assert!((wave_position((WAVE_COUNT - 1) as u8) - 1.0).abs() < 1e-12);
        assert!((wave_position(200) - 1.0).abs() < 1e-12);
    }

    /// Every sequence in the bank changes the output, reached the way a player
    /// reaches it: the panel the instrument loads with, one oscillator turned
    /// to `table`, one selector moved, one note.
    ///
    /// The guard the matrix has had all along in
    /// `every_source_reaches_every_destination`, and the one the bank did not.
    /// What it would have caught: the TABLE knob is an offset the sequence is
    /// shifted through the bank by, and it was first written as an offset from
    /// the *middle of the travel* rather than from where the patch left the
    /// knob. Seven of the eleven patches leave it at zero, so on all of them a
    /// sequence was dragged half a bank downwards and its low steps clamped
    /// onto waveform 0 — which on the default panel made `organ 3` six times
    /// weaker than it should be and `morph 8` bit-identical to no sequence for
    /// its first two steps.
    #[test]
    fn every_sequence_in_the_bank_changes_the_output() {
        let render = |seq: Option<usize>| {
            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            s.set_parameter(P_A_WAVE, knob_for(Shape::Table as usize, SHAPE_COUNT));
            s.set_parameter(P_A_LEVEL, 1.0);
            s.set_parameter(P_A_SEQ, seq_knob(seq));
            // Four seconds, which is past a full cycle of the longest
            // sequence in the bank at the clock this panel loads with. A
            // shorter render is the other way a sequence looks inert: at
            // 4 Hz `vox 4` holds its first step for 750 ms, so a third of a
            // second of audio cannot tell it from no sequence at all.
            process_buffers(&mut s, &[note_on(60, 110, 0)], 2800)
        };
        let unsequenced = render(None);
        for index in 0..SEQ_COUNT {
            let out = render(Some(index));
            let differing = out.iter().zip(unsequenced.iter()).filter(|(a, b)| a != b).count();
            let travelled: f64 = out
                .iter()
                .zip(unsequenced.iter())
                .map(|(a, b)| f64::from((a - b).abs()))
                .sum();
            assert!(
                differing > out.len() / 4,
                "{} changed only {differing} of {} samples against no sequence at all",
                seq_name(index),
                out.len()
            );
            assert!(
                travelled > 100.0,
                "{} barely changes the sound: {travelled:.1} summed against no sequence",
                seq_name(index)
            );
        }
    }

    /// Which columns of a sequence an oscillator can actually read depends on
    /// its WAVE switch, and a sequence that varies nothing but the waveform is
    /// therefore *exactly* inert on a shape that has no waveform to read.
    ///
    /// That is the deliberate half of the design — it is what lets `gate 4`
    /// put a rhythm on a sawtooth bass — and it is stated here as an assertion
    /// rather than left as a surprise, because the surprise is a good one: an
    /// oscillator switched to `noise` with `morph 8` on it renders bit for bit
    /// the same as one with no sequence at all, and nothing on the panel says
    /// why.
    #[test]
    fn a_waveform_only_sequence_needs_an_oscillator_that_reads_waveforms() {
        /// Whether every step asks for the same level and the same pitch, so
        /// that the waveform column is the only one carrying anything.
        fn waveform_only(seq: &SeqChart) -> bool {
            let first = seq.steps[0];
            seq.steps
                .iter()
                .all(|s| s.level == first.level && s.pitch == first.pitch)
        }

        // Four seconds, so that every sequence has passed several step
        // boundaries: a level or a pitch can only show up when the cursor
        // moves, where a waveform shows up on the first sample.
        let render = |shape: Shape, seq: Option<usize>| {
            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            s.set_parameter(P_A_WAVE, knob_for(shape as usize, SHAPE_COUNT));
            s.set_parameter(P_A_LEVEL, 1.0);
            s.set_parameter(P_A_SEQ, seq_knob(seq));
            process_buffers(&mut s, &[note_on(60, 110, 0)], 2800)
        };

        // The bank has one of each, or half of this test is vacuous.
        assert!(SEQ_BANK.iter().any(waveform_only), "no sequence in the bank is waveform-only");
        assert!(SEQ_BANK.iter().any(|s| !waveform_only(s)));

        for shape in [Shape::Saw, Shape::Pulse, Shape::Triangle, Shape::Sine, Shape::Noise] {
            let plain = render(shape, None);
            for (index, seq) in SEQ_BANK.iter().enumerate() {
                let out = render(shape, Some(index));
                if waveform_only(seq) {
                    // Bit-identical, not merely close: a step's level and
                    // pitch that never change multiply the oscillator by
                    // exactly one, and one is exact.
                    assert_eq!(
                        out, plain,
                        "{} varies only its waveform, so on {shape:?} it should be inert",
                        seq.name
                    );
                } else {
                    // Measured over the first 1.5 s rather than the whole
                    // render, because two of the bank's sequences are
                    // one-shots that resolve to unity and hold there — past
                    // the end of `bell run` a sawtooth really is the plain
                    // sawtooth again, and that is the sequence working rather
                    // than failing.
                    let head = 66_150.min(out.len());
                    let differing = out[..head]
                        .iter()
                        .zip(plain[..head].iter())
                        .filter(|(a, b)| a != b)
                        .count();
                    assert!(
                        differing > head / 4,
                        "{} carries a level or a pitch, so it should still work on \
                         {shape:?}: only {differing} of {head} samples moved",
                        seq.name
                    );
                }
            }
            // ...and on the wavetable oscillator every one of them works,
            // which is what makes the inertness a property of the shape rather
            // than of the sequence.
            let table = render(Shape::Table, None);
            for index in 0..SEQ_COUNT {
                assert_ne!(render(Shape::Table, Some(index)), table, "{}", seq_name(index));
            }
        }
    }

    /// A sequenced oscillator's own knobs stay live, which is the whole reason
    /// the sequence is a *reference* on the panel rather than a mode the
    /// oscillator goes into. TABLE becomes a bipolar shift of the sequence
    /// through the bank with its middle as the neutral position; TUNE and
    /// LEVEL still multiply what the steps ask for.
    #[test]
    fn a_sequenced_oscillators_own_knobs_stay_live() {
        let render = |set: &dyn Fn(&mut PhosphorSynth)| {
            let mut s = bare_sequence_panel(Some(0), 4.0);
            set(&mut s);
            process_buffers(&mut s, &[note_on(60, 110, 0)], 100)
        };
        let mean_difference = |a: &[f32], b: &[f32]| -> f64 {
            a.iter().zip(b.iter()).map(|(p, q)| f64::from((p - q).abs())).sum::<f64>()
                / a.len() as f64
        };

        let neutral = render(&|_| {});
        for (name, knob, index) in [
            ("table", P_A_TABLE, 0.30f32),
            ("tune", P_A_TUNE, tune_knob(7)),
            ("level", P_A_LEVEL, 0.4),
        ]
        {
            let moved = render(&|s| s.set_parameter(knob, index));
            assert!(
                mean_difference(&neutral, &moved) > 1e-5,
                "the {name} knob is dead on a sequenced oscillator"
            );
        }
        // ...and TABLE where the patch left it is the sequence as written.
        // `gate 4` is four steps of one waveform — the sixth in the bank — and
        // this window is inside its first step, so a sequenced oscillator with
        // that knob untouched has to be the plain oscillator pointed at that
        // waveform. Not bit-identical, only because the position that names
        // waveform 5 is a third, which f32 cannot hold and the step table does
        // not have to: the two differ by 3e-8 of a bank position.
        let plain = {
            let mut s = bare_sequence_panel(None, 4.0);
            s.set_parameter(P_A_TABLE, 5.0 / (WAVE_COUNT - 1) as f32);
            process_buffers(&mut s, &[note_on(60, 110, 0)], 100)
        };
        let difference = mean_difference(&neutral, &plain);
        assert!(
            difference < 1e-5,
            "TABLE at the middle of its travel is not the sequence as written: {difference:e}"
        );
        // The comparison only means something if that knob position matters.
        let elsewhere = {
            let mut s = bare_sequence_panel(None, 4.0);
            s.set_parameter(P_A_TABLE, 8.0 / (WAVE_COUNT - 1) as f32);
            process_buffers(&mut s, &[note_on(60, 110, 0)], 100)
        };
        assert!(mean_difference(&neutral, &elsewhere) > 1e-3);
    }

    /// A keymapped patch is a set of recipes, and a recipe owns its
    /// oscillators — so the four sequence selectors are inert on one, exactly
    /// as the four WAVE switches already are.
    #[test]
    fn a_keymapped_patch_ignores_the_sequence_selectors() {
        let render = |seq: Option<usize>| {
            let mut s = kit();
            for i in 0..4 {
                s.set_parameter(P_A_SEQ + i * P_OSC_STRIDE, seq_knob(seq));
            }
            process_buffers(&mut s, &[note_on(38, 110, 0)], 400)
        };
        assert_eq!(render(None), render(Some(1)), "a kit answered the sequence selector");
        // The same selector on a melodic patch is not inert, or the assertion
        // above would hold for the wrong reason.
        let melodic = |seq: Option<usize>| {
            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            s.set_parameter(P_A_WAVE, knob_for(Shape::Table as usize, SHAPE_COUNT));
            s.set_parameter(P_A_SEQ, seq_knob(seq));
            process_buffers(&mut s, &[note_on(60, 110, 0)], 400)
        };
        assert_ne!(melodic(None), melodic(Some(1)));
    }

    // ── The vector mix ──

    #[test]
    fn the_vector_weights_always_sum_to_one() {
        for x in 0..=20 {
            for y in 0..=20 {
                let w = vector_weights(f64::from(x) / 20.0, f64::from(y) / 20.0);
                let sum: f64 = w.iter().sum();
                assert!((sum - 1.0).abs() < 1e-12, "at ({x},{y}) the weights sum to {sum}");
                assert!(w.iter().all(|v| *v >= 0.0), "a weight went negative at ({x},{y})");
            }
        }
        // A knob that arrived as nonsense is still a position on the square.
        for (x, y) in [(-1.0, 0.5), (2.0, 0.5), (0.5, -3.0), (0.5, 9.0)] {
            let sum: f64 = vector_weights(x, y).iter().sum();
            assert!((sum - 1.0).abs() < 1e-12, "({x},{y}) summed to {sum}");
        }
        // The corners are one oscillator each, and the centre is a quarter of
        // each — which is what makes a vector position a mix rather than a
        // fader bank.
        assert_eq!(vector_weights(0.0, 0.0), [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(vector_weights(1.0, 0.0), [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(vector_weights(1.0, 1.0), [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(vector_weights(0.0, 1.0), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(vector_weights(0.5, 0.5), [0.25, 0.25, 0.25, 0.25]);
    }

    /// Moving the vector to a corner has to actually silence the other three,
    /// or the mix is not a vector mix.
    #[test]
    fn the_vector_position_chooses_which_oscillator_is_heard() {
        let render = |x: f32, y: f32| {
            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            // Four different shapes, so the corners cannot be confused.
            for (i, shape) in [0usize, 1, 2, 3].into_iter().enumerate() {
                s.set_parameter(P_A_WAVE + i * P_OSC_STRIDE, knob_for(shape, SHAPE_COUNT));
            }
            s.set_parameter(P_VECTOR_X, x);
            s.set_parameter(P_VECTOR_Y, y);
            process_buffers(&mut s, &[note_on(60, 100, 0)], 100)
        };
        let corners = [render(0.0, 0.0), render(1.0, 0.0), render(1.0, 1.0), render(0.0, 1.0)];
        for (i, a) in corners.iter().enumerate() {
            assert!(rms(a) > 0.001, "corner {i} is silent");
            for (j, b) in corners.iter().enumerate().skip(i + 1) {
                let difference: f32 =
                    a.iter().zip(b.iter()).map(|(p, q)| (p - q).abs()).sum::<f32>()
                        / a.len() as f32;
                assert!(difference > 0.001, "corners {i} and {j} sound the same");
            }
        }
    }

    // ── DRIVE ──

    /// The property that makes the knob safe to leave alone: at the bottom of
    /// its travel it is not in the signal path at all, bit for bit. The panel
    /// the instrument loads with has it there, and every render of that panel
    /// has to stay what it was.
    #[test]
    fn the_drive_stage_is_the_identity_at_zero() {
        let mut x = -8.0f64;
        while x <= 8.0 {
            assert_eq!(drive_stage(x, 0.0).to_bits(), x.to_bits(), "drive 0 altered {x}");
            x += 0.001;
        }
        for x in [0.0f64, -0.0, f64::MIN_POSITIVE, -f64::MIN_POSITIVE, 1e-300, -1e-300] {
            assert_eq!(drive_stage(x, 0.0).to_bits(), x.to_bits(), "drive 0 altered {x:e}");
        }
        assert_eq!(drive_stage(0.0, 1.0), 0.0);
    }

    /// The property that makes it a tone control rather than a fader: a signal
    /// at the reference comes out at the reference, whatever the knob says.
    #[test]
    fn the_drive_stage_holds_the_reference_level() {
        for amount in [0.0f64, 0.1, 0.25, 0.5, 0.75, 1.0] {
            let out = drive_stage(DRIVE_REFERENCE, amount);
            assert!(
                (out - DRIVE_REFERENCE).abs() < 1e-12,
                "amount {amount} moved the reference {DRIVE_REFERENCE} to {out}"
            );
            // ...and it is odd, so it makes harmonics rather than a DC offset
            // the mixer would pass straight through.
            assert!((drive_stage(-DRIVE_REFERENCE, amount) + DRIVE_REFERENCE).abs() < 1e-12);
        }
    }

    #[test]
    fn the_drive_stage_is_monotonic_and_bounded() {
        for amount in [0.01f64, 0.1, 0.25, 0.5, 0.75, 1.0] {
            let bound = DRIVE_REFERENCE + 1.0 / (amount * DRIVE_DEPTH);
            let mut previous = f64::NEG_INFINITY;
            let mut x = -16.0f64;
            while x <= 16.0 {
                let y = drive_stage(x, amount);
                assert!(y >= previous, "amount {amount} folded back at {x}: {y} < {previous}");
                assert!(y.abs() <= bound, "amount {amount} at {x} reached {y}, past {bound}");
                previous = y;
                x += 0.001;
            }
            assert!(drive_stage(1e9, amount) <= bound);
        }
    }

    /// The knob has to be monotonic *in the knob* as well as in the signal.
    /// The stage this replaced was not: on a signal past its denominator's
    /// knee, turning DRIVE up from zero made the patch quieter and then louder
    /// again.
    #[test]
    fn turning_the_drive_knob_up_never_steps_the_level_down() {
        for x in [0.1f64, 0.5, 1.0, 4.0] {
            let mut previous = drive_stage(x, 0.0);
            let mut amount = 0.0f64;
            while amount <= 1.0 {
                amount += 0.005;
                let y = drive_stage(x, amount);
                let moving_up = x <= DRIVE_REFERENCE;
                assert!(
                    if moving_up { y >= previous } else { y <= previous },
                    "the knob reversed on {x} at amount {amount}: {y} against {previous}"
                );
                previous = y;
            }
        }
    }

    /// What the knob is for, measured on the instrument rather than on the
    /// curve: peak does not rise and loudness does.
    ///
    /// The two bounds are on different measurements, and deliberately. Peak
    /// may not rise, because peak is what the ceiling is about; RMS may not
    /// fall, because a compressor takes peaks down while bringing loudness up,
    /// and bounding peak in both directions would assert that the knob does
    /// not compress, which is the one thing it is for.
    ///
    /// Measured over 1.16 s rather than the 0.29 s the rest of this file
    /// renders, because the default patch's four detuned oscillators beat
    /// against each other over seconds: a short window catches some voices
    /// near coherence and some near cancellation, and reads the difference
    /// between them as the knob's doing. At 0.29 s the eight-note chord
    /// appears to *lose* 0.11 dB of RMS at the first notch of the knob; over a
    /// full beat period it gains 0.51 dB there and 2.33 dB by the top.
    #[test]
    fn driving_the_instrument_adds_loudness_without_adding_peak() {
        for notes in [&[60u8][..], &[36, 43, 48, 55, 60, 64, 67, 72][..]] {
            let mut undriven: Option<(f32, f32)> = None;
            for drive in [0.0f32, 0.1, 0.25, 0.5, 0.75, 1.0] {
                let mut s = PhosphorSynth::new();
                s.init(44_100.0, 64);
                s.set_parameter(P_DRIVE, drive);
                let events: Vec<MidiEvent> = notes.iter().map(|n| note_on(*n, 127, 0)).collect();
                let out = process_buffers(&mut s, &events, 800);
                let Some((first_peak, first_rms)) = undriven else {
                    undriven = Some((peak(&out), rms(&out)));
                    continue;
                };
                let moved = 20.0 * (peak(&out) / first_peak).log10();
                assert!(
                    moved <= 0.0,
                    "{} notes, drive {drive} added {moved:+.2} dB of peak",
                    notes.len()
                );
                assert!(
                    rms(&out) > first_rms,
                    "{} notes, drive {drive} took loudness away: {first_rms:.5} -> {:.5}",
                    notes.len(),
                    rms(&out)
                );
            }
        }
    }

    // ── The ladder filter ──

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

    fn ladder_response(cutoff_norm: f64, res: f64) -> Vec<f64> {
        let mut f = Ladder::new();
        (0..16_384)
            .map(|i| f.process(if i == 0 { 1e-3 } else { 0.0 }, cutoff_norm, res, 44_100.0) / 1e-3)
            .collect()
    }

    /// Four poles, on the frequency the slider names.
    ///
    /// The reference is the analog cascade itself — `1/(1+(f/f0)^2)^2` — which
    /// is 12 dB down at the cutoff and only reaches its full 24 dB/octave
    /// asymptote well above it. Matching the curve is the stronger claim than
    /// matching a slope.
    ///
    /// The two four-pole filters that came before this one both integrated
    /// naively, which puts a section an octave below its own coefficient and
    /// the cascade 2.3x and 1.95x low; this one uses the topology-preserving
    /// form those two now use.
    #[test]
    fn the_ladder_is_four_poles_at_the_frequency_the_slider_names() {
        let sr = 44_100.0;
        assert!((cutoff_hz(0.0) - CUTOFF_MIN_HZ).abs() < 1e-9);
        assert!((cutoff_hz(1.0) - 16_000.0).abs() < 1e-6);
        for norm in [0.2, 0.35] {
            let f0 = cutoff_hz(norm);
            let ir = ladder_response(norm, 0.0);
            let at_dc = magnitude_at(&ir, 5.0, sr);
            for multiple in [1.0f64, 2.0, 4.0, 8.0] {
                let want = -40.0 * (1.0 + multiple * multiple).log10();
                let got = 20.0 * (magnitude_at(&ir, f0 * multiple, sr) / at_dc).log10();
                assert!(
                    (got - want).abs() < 0.8,
                    "cutoff {f0:.0} Hz at {multiple}x: {got:.1} dB, the cascade owes {want:.1} dB"
                );
            }
        }
    }

    /// The sweep only ever opens, which is not free: a corner placed above
    /// Nyquist folds back down, and a filter whose sweep folds *closes* at the
    /// top of its own travel.
    #[test]
    fn the_cutoff_sweep_only_ever_opens() {
        let sr = 44_100.0;
        let mut previous = 0.0;
        for step in 0..=20 {
            let norm = f64::from(step) / 20.0;
            let ir = ladder_response(norm, 0.0);
            let at_dc = magnitude_at(&ir, 5.0, sr);
            let through = magnitude_at(&ir, 8_000.0, sr) / at_dc;
            assert!(
                through >= previous - 1e-9,
                "the filter closes between {} and {norm} of the slider",
                norm - 0.05
            );
            previous = through;
        }
    }

    #[test]
    fn the_ladder_oscillates_at_the_top_of_the_resonance_travel() {
        let sr = 44_100.0;
        let mut f = Ladder::new();
        for _ in 0..64 {
            f.process(0.5, 0.5, 1.0, sr);
        }
        let mut tail = 0.0f64;
        for i in 0..(sr as usize * 3) {
            let y = f.process(0.0, 0.5, 1.0, sr);
            assert!(y.is_finite(), "the filter diverged");
            if i > sr as usize * 2 {
                tail = tail.max(y.abs());
            }
        }
        assert!(tail > 0.02, "the filter does not oscillate: tail {tail:.6}");
        assert!(tail < 4.0, "the filter runs away: tail {tail:.6}");

        // ...and below the knee it dies away, or every patch with the
        // resonance up would drone.
        let mut quiet = Ladder::new();
        for _ in 0..64 {
            quiet.process(0.5, 0.5, 0.6, sr);
        }
        let mut tail = 0.0f64;
        for i in 0..(sr as usize) {
            let y = quiet.process(0.0, 0.5, 0.6, sr);
            if i > sr as usize / 2 {
                tail = tail.max(y.abs());
            }
        }
        assert!(tail < 1e-4, "the filter oscillates well below the knee: {tail:.6}");
    }

    /// The ladder's signature, and the thing a clone gets wrong: the resonance
    /// feedback comes out of the passband, so the bass goes with it. Nothing
    /// here compensates for that.
    #[test]
    fn the_ladder_loses_its_bass_as_the_resonance_comes_up() {
        let sr = 44_100.0;
        // Driven with a sine two and a half octaves under the corner and
        // measured in the steady state, because the impulse response of a
        // filter this resonant is still ringing at the end of any window short
        // enough to transform.
        let bass = |res: f64| {
            let mut f = Ladder::new();
            let (mut re, mut im) = (0.0, 0.0);
            for i in 0..(sr as usize) {
                let w = TWO_PI * 30.0 * i as f64 / sr;
                let y = f.process(0.05 * w.sin(), 0.6, res, sr);
                if i > sr as usize / 2 {
                    re += y * w.cos();
                    im += y * w.sin();
                }
            }
            10.0 * (re * re + im * im).log10()
        };
        let loss = bass(0.85) - bass(0.0);
        assert!(loss < -8.0, "the ladder keeps its bass at resonance: {loss:.1} dB");
        // ...and it is monotone in the resonance, so the loss is the knob's
        // rather than an artefact of one setting.
        let mut previous = f64::INFINITY;
        for step in 0..=8 {
            let level = bass(f64::from(step) / 10.0);
            assert!(level <= previous + 0.05, "the bass came back at resonance {}", step);
            previous = level;
        }
    }

    #[test]
    fn keyboard_follow_tracks_an_octave_of_cutoff_per_octave_of_keyboard() {
        let offset = |note: u8| (f64::from(note) - 60.0) / 12.0 / CUTOFF_OCTAVES;
        assert!((cutoff_hz(0.4 + offset(72)) / cutoff_hz(0.4) - 2.0).abs() < 1e-9);
        assert!((cutoff_hz(0.4 + offset(48)) / cutoff_hz(0.4) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn filter_cutoff_affects_sound() {
        let render = |cutoff: f32| {
            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            s.set_parameter(P_FILTER_ENV, 0.5);
            s.set_parameter(P_CUTOFF, cutoff);
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 100);
            out.iter().map(|v| v * v).sum::<f32>()
        };
        let bright = render(1.0);
        let dark = render(0.1);
        assert!(bright > dark * 1.5, "bright={bright} dark={dark}");
    }

    // ── The modulation matrix ──

    /// Every source reaches every destination, and every routing changes the
    /// sound. A matrix with a slot that quietly does nothing is worse than one
    /// with fewer slots.
    ///
    /// Played at G4 rather than middle C, because keyboard tracking is zero at
    /// middle C by definition and this test would otherwise pass `kybd` on the
    /// strength of a note that cannot show it.
    #[test]
    fn every_source_reaches_every_destination() {
        let baseline = {
            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            // A panel every destination can be heard through: a wavetable
            // oscillator for `wave`, a pulse for `pw`, room above and below on
            // the cutoff and the vector.
            setup_matrix_panel(&mut s);
            process_buffers(&mut s, &[note_on(67, 90, 0), cc(1, 100, 0)], 150)
        };

        for (source, source_name) in SOURCE_LABELS.iter().enumerate().skip(1) {
            for (dest, dest_name) in DEST_LABELS.iter().enumerate().skip(1) {
                let mut s = PhosphorSynth::new();
                s.init(44_100.0, 64);
                setup_matrix_panel(&mut s);
                s.set_parameter(p_mod_src(0), knob_for(source, SOURCE_COUNT));
                s.set_parameter(p_mod_dest(0), knob_for(dest, DEST_COUNT));
                s.set_parameter(p_mod_amount(0), bipolar_knob(0.6));
                let out = process_buffers(&mut s, &[note_on(67, 90, 0), cc(1, 100, 0)], 150);
                let difference: f32 = baseline
                    .iter()
                    .zip(out.iter())
                    .map(|(a, b)| (a - b).abs())
                    .sum::<f32>()
                    / out.len() as f32;
                assert!(
                    difference > 1e-5,
                    "{source_name} → {dest_name} changed nothing ({difference:e})"
                );
                assert!(out.iter().all(|v| v.is_finite()));
                assert!(
                    peak(&out) < 0.891,
                    "{source_name} → {dest_name} reached {}",
                    peak(&out)
                );
            }
        }
    }

    /// A panel on which every destination is audible: oscillator D on the
    /// matrix, wavetables and a pulse in the mix, the vector off-centre, the
    /// cutoff with room either way, and a wave sequence running on oscillator
    /// A so that the clock is something a slot can push.
    ///
    /// `morph 8` rather than one of the stepped sequences, because the window
    /// this test renders is 0.22 s and a stepped pattern at any sane clock
    /// would not reach its second step inside it — a continuous morph moves on
    /// every sample, so a rate change shows up immediately.
    fn setup_matrix_panel(s: &mut PhosphorSynth) {
        s.set_parameter(P_A_WAVE, knob_for(Shape::Table as usize, SHAPE_COUNT));
        s.set_parameter(P_A_TABLE, 0.4);
        s.set_parameter(P_A_SEQ, seq_knob(Some(1)));
        s.set_parameter(P_SEQ_RATE, seq_rate_at(6.0));
        s.set_parameter(P_B_WAVE, knob_for(Shape::Pulse as usize, SHAPE_COUNT));
        s.set_parameter(P_C_WAVE, knob_for(Shape::Table as usize, SHAPE_COUNT));
        s.set_parameter(P_C_TABLE, 0.7);
        s.set_parameter(P_D_WAVE, knob_for(Shape::Saw as usize, SHAPE_COUNT));
        s.set_parameter(P_D_MODE, knob_for(DMode::Mod as usize, 3));
        s.set_parameter(P_VECTOR_X, 0.45);
        s.set_parameter(P_VECTOR_Y, 0.55);
        s.set_parameter(P_PULSE_WIDTH, 0.3);
        s.set_parameter(P_CUTOFF, 0.5);
        s.set_parameter(P_RESO, 0.35);
        s.set_parameter(P_FILTER_ENV, 0.5);
        s.set_parameter(P_SUSTAIN1, 0.9);
        s.set_parameter(P_SUSTAIN2, 0.7);
    }

    /// The amplitude destination is the one that cannot be allowed to add, and
    /// this is the assertion that it does not: whatever is routed there, at
    /// whatever depth, the render is no louder than the same patch with the
    /// slot switched off.
    #[test]
    fn nothing_routed_to_the_amplifier_can_make_a_patch_louder() {
        let unrouted = {
            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            process_buffers(&mut s, &[note_on(60, 127, 0)], 200)
        };
        let ceiling = peak(&unrouted);
        for (source, source_name) in SOURCE_LABELS.iter().enumerate().skip(1) {
            for amount in [-1.0f32, -0.5, 0.5, 1.0] {
                let mut s = PhosphorSynth::new();
                s.init(44_100.0, 64);
                s.set_parameter(p_mod_src(0), knob_for(source, SOURCE_COUNT));
                s.set_parameter(p_mod_dest(0), knob_for(Dest::Amplitude as usize, DEST_COUNT));
                s.set_parameter(p_mod_amount(0), bipolar_knob(amount));
                let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 200);
                assert!(
                    peak(&out) <= ceiling + 1e-6,
                    "{source_name} at {amount} took the patch from {ceiling:.4} to {:.4}",
                    peak(&out)
                );
            }
        }
    }

    /// A full-depth tremolo has to actually reach silence *and* unity, or the
    /// amplitude destination is only a trim.
    ///
    /// Measured on four sine oscillators at the same pitch rather than on the
    /// default patch, because the default's four detuned saws beat against
    /// each other: their peak within any one short window depends on where
    /// that window falls in the beat, and this test needs a signal whose peak
    /// is the same in every window so that the tremolo's own envelope is the
    /// only thing being measured.
    #[test]
    fn a_full_depth_tremolo_reaches_silence_and_unity() {
        fn steady_sine_patch() -> PhosphorSynth {
            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            for i in 0..4 {
                let base = P_A_WAVE + i * P_OSC_STRIDE;
                s.set_parameter(base, knob_for(Shape::Sine as usize, SHAPE_COUNT));
                s.set_parameter(base + 2, tune_knob(0));
                s.set_parameter(base + 3, fine_knob(0.0));
            }
            s.set_parameter(P_SUSTAIN1, 1.0);
            s.set_parameter(P_ATTACK1, 0.0);
            s.set_parameter(P_DECAY1, 0.0);
            s
        }

        let mut s = steady_sine_patch();
        s.set_parameter(P_LFO1_RATE, 0.55);
        s.set_parameter(p_mod_src(0), knob_for(Source::Lfo1 as usize, SOURCE_COUNT));
        s.set_parameter(p_mod_dest(0), knob_for(Dest::Amplitude as usize, DEST_COUNT));
        s.set_parameter(p_mod_amount(0), bipolar_knob(1.0));
        let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 400);

        // Windows of 64 samples, because the LFO passes through its trough
        // rather than sitting there: over a longer window the tremolo has
        // already come back up and the "trough" is the window's own edge.
        let mut window_peaks: Vec<f32> = out.chunks(64).map(peak).collect();
        window_peaks.sort_by(f32::total_cmp);
        assert!(window_peaks[0] < 1e-3, "the trough is {}", window_peaks[0]);

        let mut plain = steady_sine_patch();
        let reference = peak(&process_buffers(&mut plain, &[note_on(60, 127, 0)], 400));
        let crest = window_peaks[window_peaks.len() - 1];
        assert!(
            crest > reference * 0.9,
            "the peak of the tremolo is {crest}, well under the patch's own {reference}"
        );
    }

    /// The Minimoog trade: oscillator D leaves the mixer when it goes on the
    /// matrix, and what it modulates is audible.
    #[test]
    fn oscillator_d_can_be_given_up_to_the_matrix() {
        let render = |mode: usize, route: bool| {
            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            // Only D in the mix, so its leaving is unmistakable.
            s.set_parameter(P_VECTOR_X, 0.0);
            s.set_parameter(P_VECTOR_Y, 1.0);
            s.set_parameter(P_D_MODE, knob_for(mode, 3));
            if route {
                s.set_parameter(p_mod_src(0), knob_for(Source::OscD as usize, SOURCE_COUNT));
                s.set_parameter(p_mod_dest(0), knob_for(Dest::Cutoff as usize, DEST_COUNT));
                s.set_parameter(p_mod_amount(0), bipolar_knob(0.5));
            }
            process_buffers(&mut s, &[note_on(60, 110, 0)], 200)
        };
        // In the mix at the D corner, the voice sounds.
        assert!(rms(&render(DMode::Audio as usize, false)) > 0.005);
        // On the matrix, that corner is empty.
        assert!(rms(&render(DMode::Mod as usize, false)) < 1e-6);
        // ...but it is still driving the filter, from a patch that has its
        // sound elsewhere.
        let modulated = {
            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            s.set_parameter(P_D_MODE, knob_for(DMode::ModLo as usize, 3));
            s.set_parameter(p_mod_src(0), knob_for(Source::OscD as usize, SOURCE_COUNT));
            s.set_parameter(p_mod_dest(0), knob_for(Dest::Cutoff as usize, DEST_COUNT));
            s.set_parameter(p_mod_amount(0), bipolar_knob(0.6));
            process_buffers(&mut s, &[note_on(60, 110, 0)], 400)
        };
        let plain = {
            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            s.set_parameter(P_D_MODE, knob_for(DMode::ModLo as usize, 3));
            process_buffers(&mut s, &[note_on(60, 110, 0)], 400)
        };
        let difference: f32 = modulated
            .iter()
            .zip(plain.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / plain.len() as f32;
        assert!(difference > 1e-4, "oscillator D on the matrix did nothing ({difference:e})");
        // The low-frequency mode is where the panel says it is.
        assert!((mod_lo_hz(0.0) - 1.6).abs() < 1e-9);
        assert!((mod_lo_hz(-24.0) - 0.1).abs() < 1e-12);
        assert!((mod_lo_hz(24.0) - 25.6).abs() < 1e-9);
    }

    // ── Headroom ──

    /// The eight panels the extremes sweep, each one a way of asking the
    /// instrument for as much level as it will give.
    ///
    /// This is the measurement the instrument this replaced did not have. Its
    /// panel at the extremes — sub, noise, cutoff, resonance and sustain all
    /// at maximum — reached 0.9470 on an eight-note chord at velocity 127,
    /// past the master limiter's ceiling and burning 5 dB of saturation
    /// against a 2 dB budget, and nothing measured it.
    fn hostile_panels() -> Vec<(String, PhosphorSynth)> {
        let mut cases = Vec::new();

        for (shape_index, shape_name) in SHAPE_LABELS.iter().enumerate() {
            // Every oscillator on one shape, every level at the top, the
            // filter wide open and holding, the drive at the top.
            let mut s = PhosphorSynth::new();
            for i in 0..4 {
                let base = P_A_WAVE + i * P_OSC_STRIDE;
                s.set_parameter(base, knob_for(shape_index, SHAPE_COUNT));
                s.set_parameter(base + 1, 1.0);
                s.set_parameter(base + 4, 1.0);
            }
            s.set_parameter(P_CUTOFF, 1.0);
            s.set_parameter(P_RESO, 1.0);
            s.set_parameter(P_FILTER_ENV, 1.0);
            s.set_parameter(P_DRIVE, 1.0);
            s.set_parameter(P_SUSTAIN1, 1.0);
            s.set_parameter(P_SUSTAIN2, 1.0);
            s.set_parameter(P_ATTACK1, 0.0);
            s.set_parameter(P_DECAY1, 0.0);
            s.set_parameter(P_VELOCITY, 0.0);
            cases.push(((*shape_name).to_string(), s));
        }

        // Every corner of the vector, so no single oscillator is hidden by the
        // mix, on the loudest shape.
        for (name, x, y) in [
            ("corner A", 0.0, 0.0),
            ("corner B", 1.0, 0.0),
            ("corner C", 1.0, 1.0),
            ("corner D", 0.0, 1.0),
        ] {
            let mut s = PhosphorSynth::new();
            for i in 0..4 {
                let base = P_A_WAVE + i * P_OSC_STRIDE;
                s.set_parameter(base, knob_for(Shape::Pulse as usize, SHAPE_COUNT));
                s.set_parameter(base + 4, 1.0);
            }
            s.set_parameter(P_VECTOR_X, x);
            s.set_parameter(P_VECTOR_Y, y);
            s.set_parameter(P_PULSE_WIDTH, 0.0);
            s.set_parameter(P_CUTOFF, 1.0);
            s.set_parameter(P_RESO, 1.0);
            s.set_parameter(P_DRIVE, 1.0);
            s.set_parameter(P_SUSTAIN1, 1.0);
            s.set_parameter(P_ATTACK1, 0.0);
            s.set_parameter(P_DECAY1, 0.0);
            s.set_parameter(P_VELOCITY, 0.0);
            cases.push((name.to_string(), s));
        }

        // Every slot in the matrix pointed at cutoff, resonance and the
        // vector at once, at full depth, from the sources that never rest.
        for (name, source) in [
            ("all slots lfo", Source::Lfo1 as usize),
            ("all slots env", Source::Env2 as usize),
            ("all slots vel", Source::Velocity as usize),
        ] {
            let mut s = PhosphorSynth::new();
            for i in 0..4 {
                s.set_parameter(P_A_WAVE + i * P_OSC_STRIDE + 4, 1.0);
            }
            s.set_parameter(P_CUTOFF, 0.8);
            s.set_parameter(P_RESO, 0.9);
            s.set_parameter(P_DRIVE, 1.0);
            s.set_parameter(P_SUSTAIN1, 1.0);
            s.set_parameter(P_ATTACK1, 0.0);
            s.set_parameter(P_DECAY1, 0.0);
            s.set_parameter(P_VELOCITY, 0.0);
            s.set_parameter(P_LFO1_RATE, 0.6);
            for slot in 0..MOD_SLOTS {
                let dest = [Dest::Cutoff, Dest::Resonance, Dest::VectorX, Dest::VectorY][slot % 4];
                s.set_parameter(p_mod_src(slot), knob_for(source, SOURCE_COUNT));
                s.set_parameter(p_mod_dest(slot), knob_for(dest as usize, DEST_COUNT));
                s.set_parameter(p_mod_amount(slot), 1.0);
            }
            cases.push((name.to_string(), s));
        }

        // Every sequence in the bank on all four oscillators at once, with
        // every level at the top, the filter open and holding and the drive
        // at the top — the same panel as the shape sweep above, with the step
        // lists added.
        //
        // A sequence cannot introduce level: every waveform in the bank is
        // bounded by one, a step's level is bounded by one, and a crossfade
        // between two values bounded by one is bounded by one, so the vector
        // mix is still bounded by the largest level knob. What a sequence
        // *can* do is put a louder waveform under the corner the vector is
        // resting on, and transpose it down where the filter passes more of
        // it — which is why this is measured rather than argued.
        for (index, name) in [
            (0usize, "seq gate 4"),
            (1, "seq morph 8"),
            (2, "seq vox 4"),
            (3, "seq riff 5th"),
            (4, "seq attack"),
            (5, "seq bell run"),
            (6, "seq organ 3"),
            (7, "seq stab"),
        ] {
            for (rate, rate_name) in [(0.5f32, "slow"), (24.0, "fast")] {
                let mut s = PhosphorSynth::new();
                for i in 0..4 {
                    let base = P_A_WAVE + i * P_OSC_STRIDE;
                    s.set_parameter(base, knob_for(Shape::Table as usize, SHAPE_COUNT));
                    s.set_parameter(base + 4, 1.0);
                    s.set_parameter(base + 5, seq_knob(Some(index)));
                }
                s.set_parameter(P_SEQ_RATE, seq_rate_knob(rate));
                s.set_parameter(P_CUTOFF, 1.0);
                s.set_parameter(P_RESO, 1.0);
                s.set_parameter(P_FILTER_ENV, 1.0);
                s.set_parameter(P_DRIVE, 1.0);
                s.set_parameter(P_SUSTAIN1, 1.0);
                s.set_parameter(P_SUSTAIN2, 1.0);
                s.set_parameter(P_ATTACK1, 0.0);
                s.set_parameter(P_DECAY1, 0.0);
                s.set_parameter(P_VELOCITY, 0.0);
                cases.push((format!("{name} {rate_name}"), s));
            }
        }

        // The clock itself pushed by the matrix, from every slot at once and
        // in both directions, on the sequence with the deepest rests: a rest
        // that is being crossed faster than the crossfade can follow is the
        // shape of edge a step list can make that nothing else on the panel
        // can.
        for (name, amount) in [("seq clock up", 1.0f32), ("seq clock down", 0.0)] {
            let mut s = PhosphorSynth::new();
            for i in 0..4 {
                let base = P_A_WAVE + i * P_OSC_STRIDE;
                s.set_parameter(base, knob_for(Shape::Table as usize, SHAPE_COUNT));
                s.set_parameter(base + 4, 1.0);
                s.set_parameter(base + 5, seq_knob(Some(7)));
            }
            s.set_parameter(P_SEQ_RATE, seq_rate_knob(16.0));
            s.set_parameter(P_CUTOFF, 0.9);
            s.set_parameter(P_RESO, 0.95);
            s.set_parameter(P_DRIVE, 1.0);
            s.set_parameter(P_SUSTAIN1, 1.0);
            s.set_parameter(P_ATTACK1, 0.0);
            s.set_parameter(P_DECAY1, 0.0);
            s.set_parameter(P_VELOCITY, 0.0);
            s.set_parameter(P_LFO1_RATE, 0.6);
            for slot in 0..MOD_SLOTS {
                let source = [Source::Lfo1, Source::Env2, Source::Velocity][slot % 3];
                s.set_parameter(p_mod_src(slot), knob_for(source as usize, SOURCE_COUNT));
                s.set_parameter(p_mod_dest(slot), knob_for(Dest::SeqRate as usize, DEST_COUNT));
                s.set_parameter(p_mod_amount(slot), amount);
            }
            cases.push((name.to_string(), s));
        }

        // The filter as the source: no oscillator at all, resonance at the top.
        let mut self_osc = PhosphorSynth::new();
        for i in 0..4 {
            self_osc.set_parameter(P_A_WAVE + i * P_OSC_STRIDE + 4, 0.0);
        }
        self_osc.set_parameter(P_RESO, 1.0);
        self_osc.set_parameter(P_CUTOFF, 0.5);
        self_osc.set_parameter(P_SUSTAIN1, 1.0);
        self_osc.set_parameter(P_ATTACK1, 0.0);
        self_osc.set_parameter(P_DECAY1, 0.0);
        self_osc.set_parameter(P_VELOCITY, 0.0);
        cases.push(("self-oscillating".to_string(), self_osc));

        cases
    }

    const VOICINGS: [&[u8]; 5] = [
        &[60],
        &[60, 64, 67],
        &[60, 64, 67, 71],
        &[36, 48, 60, 64, 67],
        &[36, 43, 48, 55, 60, 64, 67, 72],
    ];

    #[test]
    fn the_panel_at_its_extremes_stays_under_the_ceiling() {
        /// The master limiter's ceiling, -1 dBFS. The same value as
        /// `LIMITER_CEILING` in the mixer and `TARGET_PEAK` in the headroom
        /// test; repeated because this file cannot reach either.
        const CEILING: f32 = 0.891;

        let mut worst = (0.0f32, String::new());
        for (name, mut s) in hostile_panels() {
            for notes in VOICINGS {
                for velocity in [100u8, 127] {
                    s.init(44_100.0, 64);
                    s.reset();
                    let events: Vec<MidiEvent> =
                        notes.iter().map(|n| note_on(*n, velocity, 0)).collect();
                    let out = process_buffers(&mut s, &events, 400);
                    let measured = peak(&out);
                    assert!(
                        out.iter().all(|v| v.is_finite()),
                        "{name} {notes:?} @{velocity} produced a non-finite sample"
                    );
                    assert!(
                        out.iter().all(|v| v.abs() < 1.0),
                        "{name} {notes:?} @{velocity} reached full scale"
                    );
                    assert!(
                        measured <= CEILING,
                        "{name} {notes:?} @{velocity} peaks at {measured:.4}, past the ceiling"
                    );
                    if measured > worst.0 {
                        worst = (measured, format!("{name} {notes:?} @{velocity}"));
                    }
                }
            }
        }
        // The panel should not be so quiet that the ceiling is meaningless.
        assert!(
            worst.0 > 0.4,
            "nothing on the whole panel reaches 0.4 ({}, {:.4}) — the trim is too deep",
            worst.1,
            worst.0
        );
        // ...and the worst of them is still under the saturator's knee, so
        // even the extremes are the trimmed voice sum sample for sample.
        assert!(
            worst.0 < crate::level::SATURATION_KNEE,
            "the worst panel ({}) reaches {:.4}, past the saturator's knee",
            worst.1,
            worst.0
        );
        // The number `OUTPUT_TRIM`'s comment quotes, pinned so that the two
        // cannot drift: the worst of the 320 renders is every matrix slot at
        // full depth from velocity, on an eight-note chord — the same case as
        // before this instrument could sequence, because a step list cannot
        // introduce level. The loudest sequenced panel is `riff 5th` at
        // 0.6979, which is 0.0003 behind it.
        assert!(
            (0.68..0.71).contains(&worst.0),
            "the worst panel ({}) measures {:.4}, where OUTPUT_TRIM says 0.6982",
            worst.1,
            worst.0
        );
    }

    /// The GAIN knob is a multiplier before the bounding stage, so peak is
    /// monotone in it and the top of its travel — where it defaults — is the
    /// only setting the sweep above has to cover.
    #[test]
    fn the_gain_knob_can_only_cut() {
        let mut louder = f32::INFINITY;
        for setting in [1.0f32, 0.75, 0.5, 0.25, 0.0] {
            let mut s = PhosphorSynth::new();
            s.init(44_100.0, 64);
            s.set_parameter(P_GAIN, setting);
            let events: Vec<MidiEvent> =
                VOICINGS[4].iter().map(|n| note_on(*n, 127, 0)).collect();
            let out = process_buffers(&mut s, &events, 200);
            assert!(
                peak(&out) < louder,
                "the knob at {setting} is not quieter than the setting above it \
                 ({:.4} against {louder:.4})",
                peak(&out)
            );
            louder = peak(&out);
        }
    }

    /// The other half of the guarantee: loud enough to use. A regression
    /// guard rather than a derived quantity — the five keyboards measure
    /// 0.0187 to 0.0314 RMS on this input, and this floor sits about 4 dB
    /// under the quietest of them.
    #[test]
    fn ordinary_playing_is_at_a_usable_level() {
        let mut s = PhosphorSynth::new();
        s.init(44_100.0, 64);
        let events: Vec<MidiEvent> = [60u8, 64, 67].iter().map(|n| note_on(*n, 100, 0)).collect();
        let out = process_buffers(&mut s, &events, 200);
        assert!(rms(&out) >= 0.0115, "a triad sits at {:.5} RMS", rms(&out));
        assert!(peak(&out) <= 0.5, "a triad peaks at {:.4}", peak(&out));
    }
}
