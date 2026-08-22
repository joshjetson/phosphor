//! Moog Little Phatty Stage II: a monophonic ladder synth with morphing
//! oscillators.
//!
//! The last instrument Bob Moog had direct input on, and the rack's first true
//! Moog monosynth. Two VCOs, a transistor ladder with a switchable slope, two
//! ADSRs, one modulation bus with a spare destination, glide, and mono voice
//! management with the three keyboard priorities and the three gate modes.
//!
//! The headline is the **wave control**. Each oscillator's waveform is
//! continuously variable from triangle through sawtooth through square down to
//! a skinny pulse — one knob, morphing, not a selector. The manual is explicit
//! that it is a morph ("The waveform is morphed gradually from one to another
//! as the value control is rotated") and that it is voltage controlled, which
//! is why WAVE is one of the four modulation destinations. Everything about
//! the oscillator here is built around making the in-between positions real
//! waveforms rather than crossfades of two others — see [`Trapezoid`].
//!
//! ## Sources
//!
//! * *Little Phatty Stage II User's Manual*, Moog Music 2009 (77 pages; text
//!   and illustrations by Greg Kist, Cyril Lance and Amos Gaynes). Every
//!   range, every selector's position list and every default in this file that
//!   says "the manual" came from it, page number given at the constant.
//! * Sound On Sound, *Little Phatty by Bob Moog* (Gordon Reid) and *Moog Slim
//!   Phatty* — the Slim is the same engine in a box. The maximum cutoff being
//!   audibly lower than a vintage Moog's is theirs, and it is why
//!   [`CUTOFF_MAX_HZ`] is 16 kHz rather than the 20 the Sub Phatty moved to.
//!
//! ## Where this deliberately differs from the hardware
//!
//! Three places, all of them because this is a rack module rather than a
//! keyboard, and each is marked at the control it affects:
//!
//! * **Ten of the panel controls live in the LP's Advanced Preset menus**
//!   rather than on its front panel — filter poles, velocity sensitivity, the
//!   gate mode, the two alternate modulation sources, the second modulation
//!   destination, the release enable, the keyboard priority and the pitch
//!   wheel's two ranges. They are all stored per preset on the hardware, so
//!   they are panel controls here, put in the section they belong to. The
//!   menu's other items are global settings, MIDI plumbing or the arpeggiator,
//!   which is sequencing rather than sound and is the DAW's job.
//! * **VOLUME is stored with the patch.** The manual says plainly that it is
//!   not ("The VOLUME control setting is not stored with the preset"), because
//!   on the instrument it is a master knob under your left hand. A bank of a
//!   hundred patches with no per-patch level is a bank that jumps 10 dB
//!   between neighbours, so it is stored here.
//! * **The modulation wheel starts at full.** On the instrument the mod bus
//!   output is scaled by the wheel, so a patch with AMOUNT up does nothing
//!   until you push the wheel. There is no wheel here, so CC 1 defaults to
//!   127 and AMOUNT is the control. CC 1 still scales it for a player who
//!   sends one.
//!
//! ## Preset bank
//!
//! The hundred patches in [`BANK`] are **original patches written for this
//! model**, not Moog's factory bank. The Stage II's factory *names* are
//! printed on the last page of the manual, but no published source gives the
//! parameter values behind them and the machine's SysEx dumps are not in
//! circulation, so a bank claiming to be the factory set would be a hundred
//! guesses wearing someone else's labels. These are ours, named in our own
//! voice, and voiced to cover the same ground the real bank does: it is mostly
//! basses.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

use crate::level::soft_saturate;

const PI: f64 = std::f64::consts::PI;

/// Fixed headroom trim on the output, applied after the VOLUME knob.
///
/// Sized on ordinary playing, in step with the other six — see `OUTPUT_TRIM`
/// in dx7.rs, which carries the full reasoning, and the module docs in
/// level.rs. This one is a monosynth, so "ordinary playing" is a single note
/// rather than a triad, and the trim is what puts one Little Phatty note at
/// the same loudness as a Juno's three.
///
/// Set by measurement rather than by the usual peak target, and the reason is
/// the instrument: a monosynth's crest factor is nothing like a polysynth's. A
/// Juno's default triad is three sawtooths with a string attack and peaks 20 dB
/// over its own rms; this instrument's is one filtered oscillator and peaks 6 dB
/// over. Sizing this trim so that ordinary playing *peaks* near −12 dBFS, which
/// is how the other six were set, would put the LP 14 dB above every one of
/// them on the level-match test. So it is sized on rms instead: the bank's
/// median patch on the same C major triad the workspace's
/// `instruments_are_level_matched` uses lands at 0.0306, inside the 0.0187 to
/// 0.0314 band the other six occupy. The peaks fall where they fall, which is
/// −16 dBFS for the loudest patch in the bank.
///
/// The worst *panel* — both oscillators at full, 16', overload and resonance at
/// the top, the filter open, which is not a patch but is reachable by hand — is
/// what `the_worst_panel_a_hand_can_reach_stays_under_the_ceiling` holds down.
const OUTPUT_TRIM: f32 = 0.235;

// ── Parameter indices ──
//
// Front-panel order, left to right, which on this instrument is: the user
// interface strip on the far left (glide, octave transpose, fine tune), then
// MODULATION, OSCILLATORS, FILTER, ENVELOPE GENERATORS, and OUTPUT. The
// Advanced Preset parameters sit with the section they belong to rather than
// in a menu of their own, because on a panel that is where you would reach for
// them.
//
// `patch` is first because index 0 is where the editor looks for a preset
// selector.

pub const P_PATCH: usize = 0;
// User interface strip
pub const P_GLIDE: usize = 1;
pub const P_OCTAVE: usize = 2;
pub const P_FINE: usize = 3;
pub const P_BEND_UP: usize = 4;
pub const P_BEND_DOWN: usize = 5;
// Modulation
pub const P_LFO_RATE: usize = 6;
pub const P_MOD_AMT: usize = 7;
pub const P_MOD_SRC: usize = 8;
pub const P_MOD_DEST: usize = 9;
pub const P_MOD_DEST2: usize = 10;
pub const P_SRC5: usize = 11;
pub const P_SRC6: usize = 12;
// Oscillators
pub const P_O1_OCT: usize = 13;
pub const P_O1_WAVE: usize = 14;
pub const P_O1_LEVEL: usize = 15;
pub const P_GLIDE_RATE: usize = 16;
pub const P_SYNC: usize = 17;
pub const P_O2_OCT: usize = 18;
pub const P_O2_FREQ: usize = 19;
pub const P_O2_WAVE: usize = 20;
pub const P_O2_LEVEL: usize = 21;
// Filter
pub const P_CUTOFF: usize = 22;
pub const P_RESO: usize = 23;
pub const P_KB_AMT: usize = 24;
pub const P_EG_AMT: usize = 25;
pub const P_OVERLOAD: usize = 26;
pub const P_POLES: usize = 27;
pub const P_VEL_SENS: usize = 28;
// Envelope generators
pub const P_F_ATTACK: usize = 29;
pub const P_F_DECAY: usize = 30;
pub const P_F_SUSTAIN: usize = 31;
pub const P_F_RELEASE: usize = 32;
pub const P_V_ATTACK: usize = 33;
pub const P_V_DECAY: usize = 34;
pub const P_V_SUSTAIN: usize = 35;
pub const P_V_RELEASE: usize = 36;
pub const P_EGR_REL: usize = 37;
pub const P_GATE: usize = 38;
// Keyboard
pub const P_PRIORITY: usize = 39;
// Output
pub const P_VOLUME: usize = 40;

pub const PARAM_COUNT: usize = 41;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "patch",
    "glide", "octave", "fine", "bend up", "bend dn",
    "lfo rate", "mod amt", "source", "dest", "dest 2", "src 5", "src 6",
    "1 octave", "1 wave", "1 level", "glide rt", "1-2 sync",
    "2 octave", "2 freq", "2 wave", "2 level",
    "cutoff", "res", "kb amt", "eg amt", "overload", "poles", "vel sens",
    "flt atk", "flt dec", "flt sus", "flt rel",
    "vol atk", "vol dec", "vol sus", "vol rel",
    "eg rel", "gate",
    "priority",
    "volume",
];

// ── Panel selectors ──
//
// Every selector's positions are the manual's own, in the manual's own order.
// Where the manual gives two orders — the Modulation section's prose lists the
// six sources starting from the sawtooth, the Overview on page 7 and the MIDI
// CC table on page 55 both start from the triangle — the two that agree win,
// and the CC table is the machine-readable one.

/// Modulation sources: manual page 7 and the CC 68 value table on page 55.
///
/// Positions 5 and 6 are each two sources with a menu switch between them,
/// which is what [`P_SRC5`] and [`P_SRC6`] are. The brief this was built from
/// had sample-and-hold and the filter envelope as separate positions and no
/// oscillator-2 source at all; the manual has S&H as the alternate reading of
/// position 5 and osc 2 / noise as position 6.
const MOD_SOURCES: [&str; 6] = ["tri", "square", "saw", "ramp", "filt eg", "osc 2"];

/// Modulation destinations: manual page 7 and CC 69.
const MOD_DESTS: [&str; 4] = ["pitch", "filter", "wave", "osc 2"];

/// The secondary destination, which adds OFF to the same four. CC 106.
const MOD_DESTS2: [&str; 5] = ["off", "pitch", "filter", "wave", "osc 2"];

/// Velocity sensitivity reads as a signed number on the instrument's display,
/// -8 through +8, so it reads as one here.
const VEL_LABELS: [&str; 17] = [
    "-8", "-7", "-6", "-5", "-4", "-3", "-2", "-1", "0",
    "+1", "+2", "+3", "+4", "+5", "+6", "+7", "+8",
];

/// How many positions a selector has, or `None` for a knob.
fn discrete_steps(index: usize) -> Option<usize> {
    match index {
        P_PATCH => Some(PATCH_COUNT),
        P_GLIDE | P_SYNC | P_SRC5 | P_SRC6 | P_EGR_REL => Some(2),
        P_GATE | P_PRIORITY => Some(3),
        P_BEND_UP | P_BEND_DOWN => Some(BEND_UP.len()),
        P_MOD_DEST | P_O1_OCT | P_O2_OCT | P_POLES => Some(4),
        P_OCTAVE | P_MOD_DEST2 => Some(5),
        P_MOD_SRC => Some(6),
        P_VEL_SENS => Some(VEL_LABELS.len()),
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

/// Which parameter indices are selectors (rendered as labels, not bars).
#[must_use]
pub fn is_discrete(index: usize) -> bool {
    discrete_steps(index).is_some()
}

/// The knob position one step up or down from `value`. Knobs are unchanged.
///
/// Steps by *index* rather than by adding a fraction of the travel, for the
/// reason given at the Juno's own `step_discrete`: adding 1/n of the range n
/// times does not arrive at 1.0, and a step boundary missed by one ulp is a
/// keypress that visibly does nothing.
#[must_use]
pub fn step_discrete(index: usize, value: f32, up: bool) -> f32 {
    let Some(count) = discrete_steps(index) else { return value };
    let current = selector(value, count);
    knob_for(
        if up { (current + 1).min(count - 1) } else { current.saturating_sub(1) },
        count,
    )
}

/// Label for a selector position, or `None` for a knob.
#[must_use]
pub fn discrete_label(index: usize, value: f32) -> Option<&'static str> {
    let count = discrete_steps(index)?;
    let step = selector(value, count);
    Some(match index {
        P_PATCH => PATCH_NAMES[step],
        P_GLIDE | P_SYNC | P_EGR_REL => ["off", "on"][step],
        P_OCTAVE => ["-2", "-1", "0", "+1", "+2"][step],
        P_MOD_SRC => MOD_SOURCES[step],
        P_MOD_DEST => MOD_DESTS[step],
        P_MOD_DEST2 => MOD_DESTS2[step],
        P_SRC5 => ["filt eg", "s&h"][step],
        P_SRC6 => ["osc 2", "noise"][step],
        P_O1_OCT | P_O2_OCT => ["16'", "8'", "4'", "2'"][step],
        P_POLES => ["6dB", "12dB", "18dB", "24dB"][step],
        P_VEL_SENS => VEL_LABELS[step],
        P_BEND_UP => ["0", "+2", "+3", "+4", "+5", "+7", "+12"][step],
        P_BEND_DOWN => ["0", "-2", "-3", "-4", "-5", "-7", "-12"][step],
        P_GATE => ["leg on", "leg off", "reset"][step],
        P_PRIORITY => ["low", "high", "last"][step],
        _ => return None,
    })
}

/// A knob's value in seconds, for the eight that measure time. `None` for the
/// ones that read as a percentage.
#[must_use]
pub fn param_seconds(index: usize, value: f32) -> Option<f64> {
    match index {
        P_F_ATTACK | P_F_DECAY | P_F_RELEASE | P_V_ATTACK | P_V_DECAY | P_V_RELEASE => {
            Some(env_seconds(f64::from(value)))
        }
        _ => None,
    }
}

// ── Panel tapers ──
//
// Every range here is the manual's, and every taper between the two ends is
// geometric unless the constant says otherwise. Geometric because these are
// all frequency or time controls, where equal turns of the knob should be
// equal ratios, and because the manual publishes both ends of each of them and
// nothing in between.

/// Attack, decay and release, both envelopes: "from 1 msec to 10 seconds"
/// (manual page 16, and the specification appendix on page 72).
const ENV_MIN_S: f64 = 0.001;
const ENV_MAX_S: f64 = 10.0;

fn env_seconds(knob: f64) -> f64 {
    ENV_MIN_S * (ENV_MAX_S / ENV_MIN_S).powf(knob.clamp(0.0, 1.0))
}

/// "The cutoff frequency is adjustable from about 20 Hz to 16 KHz" (page 14,
/// and the specification appendix).
///
/// The top end is the character. Sound On Sound, reviewing the same engine in
/// the Slim Phatty: "the maximum cutoff frequency of the Phatty's filter is
/// audibly lower than those of the vintage synths." The Sub Phatty raised it
/// to 20 kHz precisely because this one is dark. It stays at 16 here.
const CUTOFF_MIN_HZ: f64 = 20.0;
const CUTOFF_MAX_HZ: f64 = 16_000.0;

/// How many octaves the cutoff knob covers end to end: `log2(16000/20)`.
///
/// The taper is written in terms of this rather than of the two ends, because
/// everything else that moves the cutoff — keyboard tracking, the envelope,
/// velocity, the mod bus — is an offset in octaves, and there should be one
/// number that says how wide an octave is on this knob.
const CUTOFF_OCTAVES: f64 = 9.643_856_189_774_724;

fn cutoff_hz(knob: f64) -> f64 {
    CUTOFF_MIN_HZ * (CUTOFF_OCTAVES * knob.clamp(0.0, 1.0)).exp2()
}

/// "The frequency is adjustable from 0.2 Hz to 500 Hz. Since the LFO rate
/// extends well into the audio range, this allows the LFO to be used for
/// clangorous (FM-like) modulations" (manual page 17).
///
/// The specification appendix on page 72 says 50 Hz for the same control. Two
/// against one: the body text and Sound On Sound's review both say 500, and 50
/// Hz is not "well into the audio range". The appendix has the typo.
const LFO_MIN_HZ: f64 = 0.2;
const LFO_MAX_HZ: f64 = 500.0;

fn lfo_hz(knob: f64) -> f64 {
    LFO_MIN_HZ * (LFO_MAX_HZ / LFO_MIN_HZ).powf(knob.clamp(0.0, 1.0))
}

/// Glide, in semitones per second.
///
/// Constant *rate*, not constant time, which the manual settles by measuring
/// it: "about 5 seconds to go from the lowest C to the highest C on the
/// keyboard" (page 12). The LP's keyboard is 37 keys, C to C, so that is 36
/// semitones in 5 seconds — 7.2 semitones per second at the top of the knob,
/// and a figure that only means anything if the time depends on the interval.
///
/// The bottom of the knob is "virtually instantaneous": 4000 semitones per
/// second crosses the whole keyboard in 9 ms.
const GLIDE_FAST: f64 = 4_000.0;
const GLIDE_SLOW: f64 = 7.2;

fn glide_semitones_per_second(knob: f64) -> f64 {
    GLIDE_FAST * (GLIDE_SLOW / GLIDE_FAST).powf(knob.clamp(0.0, 1.0))
}

/// "The pitch of Oscillator 2 can be adjusted up or down 7 semitones (a
/// fifth)" (page 12); the specification appendix agrees. The knob is centred,
/// so 0.5 is unison.
const OSC2_RANGE_SEMITONES: f64 = 7.0;

/// "The FINE TUNE control is used to tune the Little Phatty's oscillators ±3
/// semitones" (page 21).
const FINE_RANGE_SEMITONES: f64 = 3.0;

/// The pitch wheel's two ranges, which the PITCH BEND advanced preset menu
/// sets independently: "Values: UP: 0, +2, +3, +4, +5, +7, +12 / DN: 0, -2,
/// -3, -4, -5, -7, -12" (page 36). Both default to two semitones, which is
/// where the calibration preset leaves them.
const BEND_UP: [f64; 7] = [0.0, 2.0, 3.0, 4.0, 5.0, 7.0, 12.0];
const BEND_DOWN: [f64; 7] = [0.0, -2.0, -3.0, -4.0, -5.0, -7.0, -12.0];

/// The four octave switch positions, as frequency multipliers. 16' is an
/// octave below 8', which is where A440 sits on note 69.
const OCTAVE_FEET: [f64; 4] = [0.5, 1.0, 2.0, 4.0];

/// How far the filter envelope can move the cutoff at EGR AMNT hard over,
/// in octaves. The control is bipolar — "a positive amount will cause the
/// Filter EG to raise the cutoff frequency, while a negative amount will cause
/// the Filter EG to lower the cutoff" (page 13) — so this is each way.
const EG_AMOUNT_OCTAVES: f64 = 6.0;

/// How far velocity can move the cutoff at FILT SENS ±8, in octaves.
///
/// The manual gives the control's range (-8 to +8) and its direction but no
/// depth, so this is chosen: four octaves is enough that a patch voiced for it
/// goes from closed to open across a playable velocity range, and it is the
/// only thing velocity does on this instrument. Amplitude does not respond to
/// velocity at all, on the hardware or here, and that is most of why an LP
/// feels different under the fingers.
const VELOCITY_OCTAVES: f64 = 4.0;

/// The mod bus depths at AMOUNT hard over.
///
/// Pitch and osc 2 get the same seven semitones the OSC 2 FREQ knob covers, so
/// a mod bus pointed at pitch sweeps the same fifth by hand or by LFO. The
/// amount knob is squared on the way in, which is what puts the manual's own
/// worked example — "Set the AMOUNT to 50%... These settings will produce a
/// vibrato effect" — at 1.75 semitones rather than 3.5.
const MOD_PITCH_SEMITONES: f64 = 7.0;
/// Six octaves of filter sweep at the top of the amount knob, which is most of
/// the cutoff range.
const MOD_FILTER_OCTAVES: f64 = 6.0;

// ── The oscillator ──
//
// One knob per oscillator, morphing triangle → sawtooth → square → skinny
// pulse. The obvious implementation — generate two waveforms and crossfade —
// is the wrong one, and audibly so: a half-and-half mix of a triangle and a
// sawtooth is not a shape between them, it is a shape whose peak has dropped
// to 0.5 while its trough stayed at -1, so the morph has a level dip and a DC
// wander in the middle of its travel.
//
// What all four shapes *are* is one family: a **trapezoid**, described by how
// long it spends rising, high, falling and low. Every position on the knob is
// one member of that family, and moving through them is what the panel legend
// describes:
//
// ```text
//   triangle   rise 1/2, high 0,   fall 1/2, low 0
//   sawtooth   rise 1,   high 0,   fall 0,   low 0
//   square     rise 0,   high 1/2, fall 0,   low 1/2
//   pulse      rise 0,   high d,   fall 0,   low 1-d
// ```
//
// It is also what the circuit does. The saw-to-square half of the travel is a
// gain into a clipper — the ramp steepens and flattens against the rails —
// which is exactly "rise shortens, high and low grow", and the square-to-pulse
// half is the clipper's threshold moving off centre, which is exactly "high
// shrinks, low grows". So the parameterisation is the mechanism rather than an
// approximation of it.
//
// ## Band-limiting
//
// A trapezoid has four corners per cycle and no steps at all, so the whole
// waveform is corrected with polyBLAMP — the slope-discontinuity counterpart
// of the polyBLEP the phosphor synth's oscillators use — and there is no
// separate case for the sawtooth's flyback or the square's edges. Those are
// trapezoids too, with an edge [`EDGE_MIN`] of a cycle long instead of zero,
// and the two BLAMP corrections at the ends of a vanishing edge converge
// *exactly* on the BLEP of the step it becomes. That is worth stating as
// algebra, since it is what makes the single code path legitimate: with a
// corner correction of `m/2 · B(s)` and `B(s) = -(s-1)³/3` after the corner,
// two corners `δ` apart with slope changes `±m` contribute
// `-(m/2)·u²·δ` to the sample after them, and `m·δ` is the height of the step
// the pair collapses into.
//
// One sample of latency buys the correction: a corner between two samples has
// to move both of them, and the earlier of the two is only still available if
// the oscillator is holding it. Both oscillators are delayed identically, so
// sync timing and their relative phase are unaffected.

/// The shortest rise or fall the morph will produce, as a fraction of a cycle.
///
/// This is what a "vertical" edge is here. Small enough to be instantaneous by
/// any measure — 50 ns at 20 Hz — and large enough that the slope change at
/// its corners, `2/EDGE_MIN`, stays far away from the point where the two
/// corner corrections cancelling each other would cost real precision. At the
/// worst case the pair cancels from about 1.5e5 down to 1, which leaves eleven
/// significant digits of the f64.
const EDGE_MIN: f64 = 1.0e-6;

/// Duty cycle of the skinniest pulse the knob reaches.
///
/// The manual notes that *modulation* can narrow the pulse until it is silent;
/// the panel does not go that far, and neither does this. The wave control is
/// clamped to its own travel, so the thinnest sound in the instrument is this.
const DUTY_MIN: f64 = 0.05;

/// The trapezoid a wave-knob position asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shape {
    rise: f64,
    high: f64,
    fall: f64,
    low: f64,
    /// The raw trapezoid's mean, `high - low`, which is zero everywhere except
    /// the pulse quarter of the travel.
    mean: f64,
    /// What the DC-removed shape is divided by to bring its peak back to 1.
    ///
    /// A rectangle of duty `d` that has had its mean taken out reaches
    /// `1 - mean` at the top, which is 1.9 at a 5% duty — an analog pulse
    /// really does do that, and it is why a narrow pulse patch on a real Moog
    /// is a peak-meter problem. Normalising instead costs the thin end of the
    /// travel about 6 dB of level relative to the square, which the LEVEL and
    /// VOLUME knobs give back, and buys a hard bound of ±1 on every waveform
    /// at every knob position. In a rack where nothing may reach the master
    /// limiter on its own, that trade goes this way.
    scale: f64,
}

impl Shape {
    /// The four labelled positions on the panel legend are evenly spaced round
    /// the knob, so the travel is three equal thirds.
    fn at(wave: f64) -> Self {
        let w = wave.clamp(0.0, 1.0);
        let e = EDGE_MIN;
        let (rise, high, fall) = if w < 1.0 / 3.0 {
            // Triangle → sawtooth: the rise takes over the cycle while the
            // fall shrinks to an edge. Amplitude never moves.
            let a = w * 3.0;
            let rise = 0.5 + a * (0.5 - e);
            (rise, 0.0, 1.0 - rise)
        } else if w < 2.0 / 3.0 {
            // Sawtooth → square: the ramp steepens and flat top and bottom
            // grow out of it, which is what raising the gain into a clipper
            // does to a ramp.
            let b = w * 3.0 - 1.0;
            let rise = (1.0 - e) + b * (e - (1.0 - e));
            let flat = (1.0 - rise - e) * 0.5;
            (rise, flat, e)
        } else {
            // Square → pulse: the clipper's threshold moves off centre.
            let c = (w * 3.0 - 2.0).min(1.0);
            let square = (1.0 - 2.0 * e) * 0.5;
            (e, square + c * (DUTY_MIN - square), e)
        };
        let low = 1.0 - rise - high - fall;
        let mean = high - low;
        Self { rise, high, fall, low, mean, scale: 1.0 / (1.0 - mean) }
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
    /// spike of that size. Pairing them here is what makes it impossible.
    #[inline]
    fn edges(&self) -> [(f64, f64, f64); 2] {
        [(0.0, self.rise, 2.0), (self.rise + self.high, self.fall, -2.0)]
    }
}

/// A stretch of phase covered inside one sample: where it starts, how far it
/// runs, and where in the sample it begins and ends.
///
/// The last two are only ever anything other than 0 and 1 when a hard-sync
/// reset splits the sample in two, and they exist so that a corner crossed
/// either side of the reset lands at the right sub-sample position anyway.
#[derive(Debug, Clone, Copy)]
struct Stretch {
    from: f64,
    span: f64,
    t0: f64,
    t1: f64,
}

impl Stretch {
    /// Where in the sample a corner `reached` into this stretch sits.
    #[inline]
    fn place(&self, reached: f64) -> f64 {
        self.t0 + (reached / self.span) * (self.t1 - self.t0)
    }
}

/// The two samples a band-limiting correction is spread over: the one about
/// to be emitted, and the one being computed.
#[derive(Debug, Clone, Copy, Default)]
struct Correction {
    before: f64,
    after: f64,
}

/// A morphing trapezoid oscillator with polyBLAMP corners and hard sync.
#[derive(Debug, Clone)]
struct Trapezoid {
    phase: f64,
    /// The sample computed on the previous call, still open to corrections
    /// from events in this one. The one sample of latency the two-sided
    /// correction needs.
    held: f64,
    /// Per edge, once its leading corner has been corrected: where its
    /// trailing corner is and exactly what correction that corner will make.
    ///
    /// Both halves of this matter, and both were measured going wrong.
    ///
    /// *That* it is remembered is what stops half a pair being applied at all.
    /// The two cases are the oscillator's first sample and every hard-sync
    /// reset: in both the phase arrives at zero without having travelled
    /// there, so the trailing corner of an edge that ends at the top of the
    /// cycle sits right under it, and firing it alone puts a spike of
    /// `2·dt/EDGE_MIN/6` — sixteen hundred, on a sawtooth at 220 Hz — into the
    /// output.
    ///
    /// *What* is remembered is what stops the pair failing to cancel. The
    /// slope change is `2·dt/width`, and `width` moves with the wave knob,
    /// which the mod bus can move every sample. Recomputing it at the trailing
    /// corner from a shape that has since changed leaves the difference
    /// behind, and near the thin end of the morph that difference is a factor
    /// of a million: four patches with the mod bus on WAVE peaked at 0.93
    /// where the rest of the bank sits under 0.25. Capturing the correction
    /// when the edge opens makes the pair cancel by construction, whatever the
    /// knob does in between.
    closing: [Option<(f64, f64)>; 2],
}

impl Trapezoid {
    fn new() -> Self {
        Self { phase: 0.0, held: 0.0, closing: [None; 2] }
    }

    fn reset(&mut self) {
        self.phase = 0.0;
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
            // Everything up to the reset, at its own share of the sample.
            let span = u * dt;
            self.walk(shape, Stretch { from: self.phase, span, t0: 0.0, t1: u }, dt, &mut fix);
            let at = wrap_phase(self.phase + span);
            // The reset itself: a step from wherever the waveform had got to
            // back to the start of its cycle.
            let jump = shape.value(0.0) - shape.value(at);
            let rest = 1.0 - u;
            fix.before += 0.5 * jump * rest * rest;
            fix.after -= 0.5 * jump * u * u;
            // An edge the reset interrupted has no trailing corner: the step
            // above is what happened instead of the rest of it.
            self.closing = [None; 2];
            // ...and everything after it, from phase zero.
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

    /// Adds the band-limited corner corrections for every corner the phase
    /// crosses while it moves `span` forward from `start`. `t0`..`t1` is where
    /// in the sample that span sits, so that a corner's correction lands at
    /// the right sub-sample position even when a sync reset splits the step.
    ///
    /// An edge whose trailing corner falls past the end of this span leaves
    /// [`Trapezoid::open`] set, and the next call picks it up. That is the
    /// ordinary case for a triangle, whose edges are half a cycle long, and it
    /// happens once in a few thousand cycles for a sawtooth's flyback — the
    /// leftover on the sample before is bounded by `(width/dt)²/3`, which for
    /// an edge that short is nothing.
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

            // The trailing corner of an edge that began in an earlier step,
            // with the correction it was given then.
            if let Some((phase, m)) = self.closing[index] {
                let reached = (phase - start).rem_euclid(1.0);
                if reached < span {
                    Self::corner(m, stretch.place(reached), fix);
                    self.closing[index] = None;
                }
            }

            let reached = (at - start).rem_euclid(1.0);
            if reached < span {
                // An edge cut short by the knob moving under it is closed here
                // rather than left open, so that the books always balance.
                if let Some((_, m)) = self.closing[index].take() {
                    Self::corner(m, stretch.place(reached), fix);
                }
                let m = height / width * scale;
                Self::corner(m, stretch.place(reached), fix);
                if width < span {
                    // Narrower than the step, so the pair belongs to this step
                    // whatever happens next, and the trailing corner is clamped
                    // to the end of the step rather than deferred to a shape
                    // that may have moved. That clamp costs at most a sample of
                    // timing and bounds the pair's residual at 1.0; deferring it
                    // costs nothing when the shape is still and everything when
                    // it is not, because `m` here is `2·dt/width` and `width` at
                    // the thin end of the morph is a millionth of a cycle.
                    Self::corner(-m, stretch.place((reached + width).min(span)), fix);
                } else {
                    // Wide enough that the pair really does straddle steps —
                    // a triangle's edges are half a cycle long. `m` is at most
                    // `2·scale` here, so an edge left open by a sync reset or
                    // by the knob costs a third of a unit at worst.
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

// ── The ladder ──
//
// Four one-pole sections round a feedback loop, which is a transistor ladder's
// own topology, with topology-preserving integrators so that a section's pole
// lands on the frequency it was asked for. That much is the same design the
// phosphor synth's `Ladder` uses and for the same reasons; it is written out
// again here rather than shared because this one has the pole switch, and
// because the two instruments should be free to move independently — the house
// synth's filter is voiced against its own four-oscillator front end.
//
// What is *not* copied is the character: this filter's cutoff knob stops at
// 16 kHz where the house synth's runs to 20, which is the whole "a Phatty is
// darker than a vintage Moog" observation, and it is in [`cutoff_hz`].
//
// **The pole switch taps the ladder; it does not shorten it.** The resonance
// feedback still comes from the fourth section at every setting, and only the
// output moves. That is what the hardware has to be doing — FILTER POLES is a
// preset parameter on CC 109, so it is a switch on the output buffer, and
// moving the feedback tap as well would need more switches than a ladder has
// — and it is also what players describe: a 2-pole Phatty still resonates and
// still self-oscillates, it just leaks more top end past the corner. Feeding
// back from the tap instead would kill resonance outright at one and two
// poles, since one pole cannot reach half a turn of phase at any frequency
// and two only reach it at infinity.
//
// There is no gain compensation on the input, deliberately: the resonance
// feedback is subtracted from the signal, so the passband loses its bass as
// resonance comes up. That loss is what a ladder sounds like.

/// How much feedback the loop can be given. Four correctly-placed poles reach
/// half a turn of phase with a quarter of the gain left, so 4.0 is exactly
/// marginal and the top of the travel has to sit past it to oscillate.
const LADDER_RES_MAX: f64 = 4.5;

/// Where on the resonance travel the loop stops losing and starts producing.
const SELF_OSC_KNEE: f64 = 0.9;

/// How much of its own oscillation a note starts the filter with, at the top
/// of the resonance travel. A filter with no state and no input stays silent
/// however negative its damping is, and this bank has patches whose sound
/// source is the filter.
const SELF_OSC_SEED: f64 = 0.05;

/// Rational tanh: one divide instead of a libm call, and within 0.5% of the
/// real thing over the range the ladder puts through it.
#[inline]
fn tanh_approx(x: f64) -> f64 {
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

#[derive(Debug, Clone)]
struct Ladder {
    s: [f64; 4],
}

impl Ladder {
    fn new() -> Self {
        Self { s: [0.0; 4] }
    }

    fn process(&mut self, input: f64, cutoff: f64, resonance: f64, poles: usize, sr: f64) -> f64 {
        let g = (PI * cutoff.clamp(5.0, sr * 0.49) / sr).tan();
        let gg = g / (1.0 + g);
        let res = resonance.clamp(0.0, 1.0) * LADDER_RES_MAX;
        let mut x = tanh_approx(input - res * tanh_approx(self.s[3]));
        let mut tap = x;

        for (i, s) in self.s.iter_mut().enumerate() {
            let v = (x - *s) * gg;
            let y = v + *s;
            *s = y + v;
            if s.abs() < 1.0e-18 {
                *s = 0.0;
            }
            x = y;
            if i + 1 == poles {
                tap = x;
            }
        }
        // The one number that closes the loop is worth bounding outright,
        // since a self-oscillating filter has no input to bound it.
        self.s[3] = self.s[3].clamp(-4.0, 4.0);
        tap
    }

    fn reset(&mut self) {
        self.s = [0.0; 4];
    }

    fn start(&mut self, resonance: f64) {
        let past = (resonance - SELF_OSC_KNEE) / (1.0 - SELF_OSC_KNEE);
        self.s = [past.clamp(0.0, 1.0) * SELF_OSC_SEED; 4];
    }
}

// ── Overload ──
//
// "The OVERLOAD parameter allows you to set the amount of signal clipping from
// none to soft to hard clipping as the amount is increased" (page 14), and the
// specification appendix adds that it is "variable pre and post distortion,
// adds +6dB signal boost at full level". The manual's own tech note says the
// circuit clips asymmetrically.
//
// So: a pre-filter stage with an offset into a soft clipper, a gentler
// post-filter stage without one, and 6 dB of makeup across the knob's travel.
// The offset is what makes it asymmetric — a real clipper is asymmetric
// because its bias point is not centred, which is a very different thing from
// bending the two halves of the curve by different amounts. Bending the halves
// separately puts a slope kink at the zero crossing and sounds like crossover
// distortion; offsetting the whole signal into a smooth curve produces even
// harmonics with no kink anywhere, and a DC offset that [`DcBlock`] takes back
// out before the filter can pass it to the amplifier.

/// Where the pre-filter stage's knee sits, as the reciprocal of a signal
/// level: `1 / OVERLOAD_KNEE` is the input at which the shaper's gain has
/// fallen to half its small-signal value.
///
/// It is 0.5 for a reason that is a *constraint* rather than a taste: the
/// mixer can hand this stage 2.0, both oscillators at full, and the knob must
/// not make anything quieter at any setting. With a makeup of `1 + a` over a
/// shaper of `1/(1 + a·C·|x|)`, the level at a fixed input is increasing in
/// the knob exactly while `|x| < 1/C`, so `C = 0.5` buys monotonicity right up
/// to the loudest thing the panel can produce.
///
/// This is not a hypothetical. The first version of this stage used the
/// house synth's level-preserving-at-a-reference curve, which is increasing
/// only while `|x| < r + 1/D`; at `r = 0.4` and `D = 14` that is 0.47, so
/// every patch with a mix above half scale got *quieter* halfway up the knob.
/// `Sync Scream` measured 0.0688 rms at overload 0 and 0.0587 at 0.5, and the
/// workspace's own `no_keyboard_drive_setting_exceeds_the_target` caught it.
const OVERLOAD_KNEE: f64 = 0.5;

/// The post-filter stage's knee, gentler than the pre-filter one because the
/// two multiply: the level at the output is increasing in the knob while
/// `OVERLOAD_KNEE·|x| + OVERLOAD_KNEE_POST·|y|` stays under 1, and the filter
/// output `y` is the same order as the mix `x`.
const OVERLOAD_KNEE_POST: f64 = 0.05;

/// How much shallower the negative half of the pre-filter curve is than the
/// positive.
///
/// This is the asymmetry the manual's tech note describes, and it is put in
/// the *denominator* only. Both halves keep the same slope through zero, so
/// there is no kink there — a kink at the zero crossing is crossover
/// distortion, which is a different and much nastier sound than a clipper
/// whose bias point is off centre. What differs is where each half runs out of
/// headroom, which is what leaves even harmonics and a DC offset behind.
const OVERLOAD_ASYMMETRY: f64 = 0.6;

/// Corner frequency of the DC blocker that follows the pre-filter stage.
const DC_BLOCK_HZ: f64 = 8.0;

/// The soft clipper both overload stages are built from.
///
/// ```text
/// y = x / (1 + k*|x|)
/// ```
///
/// Identity at `k = 0` in f32 and f64 both, so the bottom of the knob is the
/// patch as voiced; monotonic and bounded by `1/k`, so it folds nothing back
/// and cannot run away; and its gain falls smoothly from 1 at the origin,
/// which is a soft clip becoming a hard one exactly as the knob describes.
#[inline]
fn overload_curve(x: f64, knee: f64) -> f64 {
    x / (1.0 + knee * x.abs())
}

/// Pre-filter, with the 6 dB the specification appendix documents: "Overload:
/// Variable pre and post distortion, adds +6dB signal boost at full level."
///
/// The boost is exactly `1 + amount` for any signal well under the knee, so
/// the top of the knob is +6.02 dB; a signal at the knee gets less, which is
/// what a clipper does and why "about" is the manual's own word.
#[inline]
fn overload_pre(x: f64, amount: f64) -> f64 {
    let knee = amount * OVERLOAD_KNEE * if x < 0.0 { OVERLOAD_ASYMMETRY } else { 1.0 };
    overload_curve(x, knee) * (1.0 + amount)
}

/// Post-filter: the same clipper, much gentler, and no makeup — the boost is
/// already in the stage before the filter, which is where it belongs.
#[inline]
fn overload_post(x: f64, amount: f64) -> f64 {
    overload_curve(x, amount * OVERLOAD_KNEE_POST)
}

/// One-pole DC blocker. The coefficient comes from the sample rate, so the
/// corner is 8 Hz at every rate rather than 8 Hz at 44100.
#[derive(Debug, Clone)]
struct DcBlock {
    x1: f64,
    y1: f64,
}

impl DcBlock {
    fn new() -> Self {
        Self { x1: 0.0, y1: 0.0 }
    }

    #[inline]
    fn tick(&mut self, x: f64, coefficient: f64) -> f64 {
        let y = x - self.x1 + coefficient * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

// ── Envelopes ──
//
// Two identical ADSRs, one on the filter and one on the amplifier, both 1 ms
// to 10 s on their three time controls. Every segment is a capacitor charging
// towards something, which is why none of them are straight lines: the attack
// charges towards 1.58 and ends when it passes 1.0, which is the first time
// constant of an exponential, and the decay and release charge a little past
// their target and stop when they reach it, which is 3.5 time constants across
// the segment and makes the knob's number the time the segment actually takes.
//
// The three **gate modes** are the manual's, from the GATE menu on page 35:
//
// * `LEG ON` — "the envelopes aren't retriggered until the key is fully
//   released". Legato playing changes the pitch and nothing else. This is the
//   instrument's default and it is most of how an LP feels.
// * `LEG OFF` — "will retrigger the envelope on a new note from the current
//   EGR level". A new attack, starting from wherever the envelope already was,
//   which is why it reaches the top sooner than the ATTACK knob says.
// * `EGR RESET` — "will force the envelope generators to start from 0 volts
//   each time a note is triggered". The hard, percussive one.
//
// Returning to a note that was already held — letting go of the newer key —
// never retriggers in any mode. That is the held-note return, and it is a
// property of the keyboard rather than of the gate.

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
    /// when a knob moves. Three exponentials per sample for an answer that
    /// changes when a finger does is not a trade worth making.
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

    /// A new attack from zero — the EGR RESET gate mode.
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
        // A sustain of zero means the segment ended in silence, so the note is
        // finished rather than holding the amplifier open at nothing.
        self.stage = if self.times.sustain <= 0.0 { EnvStage::Idle } else { EnvStage::Sustain };
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

// ── LFO, noise and sample-and-hold ──

/// The four LFO waveforms, in the panel's order. Moog's convention: the
/// sawtooth falls and the ramp rises, which is why both are on the selector.
#[derive(Debug, Clone)]
struct Lfo {
    phase: f64,
}

impl Lfo {
    fn new() -> Self {
        Self { phase: 0.0 }
    }

    /// One sample. Returns the waveform and whether the cycle just restarted,
    /// which is the sample-and-hold's clock.
    #[inline]
    fn tick(&mut self, hz: f64, sr: f64, wave: usize) -> (f64, bool) {
        let dt = (hz / sr).clamp(0.0, 0.45);
        self.phase += dt;
        let wrapped = self.phase >= 1.0;
        if wrapped {
            self.phase -= self.phase.floor();
        }
        let t = self.phase;
        // The square, sawtooth and ramp are band-limited even here. The LFO
        // runs to 500 Hz on this instrument — "clangorous (FM-like)
        // modulations" is the manual's own description of the top of the knob
        // — and a naive edge up there folds a spectrum back that would then be
        // heard through whatever it was modulating, differently at every
        // sample rate.
        let value = match wave {
            1 => {
                let mut v = if t < 0.5 { 1.0 } else { -1.0 };
                v += poly_blep(t, dt);
                v -= poly_blep((t - 0.5).rem_euclid(1.0), dt);
                v
            }
            2 => 1.0 - 2.0 * t + poly_blep(t, dt),
            3 => 2.0 * t - 1.0 - poly_blep(t, dt),
            // Triangle, naive: its harmonics fall as 1/n², so what folds back
            // sits far enough under the fundamental to be irrelevant.
            _ => {
                if t < 0.5 {
                    4.0 * t - 1.0
                } else {
                    3.0 - 4.0 * t
                }
            }
        };
        (value, wrapped)
    }

    fn reset(&mut self) {
        self.phase = 0.0;
    }
}

/// The band-limited step correction, in the same form the phosphor synth's
/// oscillators use: `(height/2) · r(s)`, with `r` the two-point residual.
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

/// White noise. Its own generator rather than a shared one: it feeds the
/// sample-and-hold as well as the noise modulation source, and both want the
/// same stream so that switching between them is a switch rather than a
/// different sound.
#[derive(Debug, Clone)]
struct Noise {
    state: u32,
}

impl Noise {
    fn new() -> Self {
        Self { state: 0x2545_f491 }
    }

    #[inline]
    fn tick(&mut self) -> f64 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(self.state >> 8) / f64::from(1u32 << 23) - 1.0
    }
}

// ── The panel, read once a block ──

/// Every control, converted out of knob positions into the units the engine
/// works in. Built once per `process` call rather than per sample: none of it
/// can change inside a block, and a tangent and six exponentials per sample
/// for numbers that move when a finger does is not a trade worth making.
#[derive(Debug, Clone, Copy)]
struct Panel {
    glide_on: bool,
    octave: f64,
    fine: f64,
    bend_up: f64,
    bend_down: f64,
    lfo_hz: f64,
    mod_amount: f64,
    mod_src: usize,
    mod_dest: usize,
    mod_dest2: usize,
    src5: usize,
    src6: usize,
    o1_oct: usize,
    o1_wave: f64,
    o1_level: f64,
    glide_rate: f64,
    sync: bool,
    o2_oct: usize,
    o2_detune: f64,
    o2_wave: f64,
    o2_level: f64,
    cutoff: f64,
    resonance: f64,
    kb_amount: f64,
    eg_amount: f64,
    overload: f64,
    poles: usize,
    vel_sens: f64,
    f_times: EnvTimes,
    v_times: EnvTimes,
    gate_mode: usize,
    priority: usize,
    volume: f64,
}

impl Panel {
    fn read(params: &[f32; PARAM_COUNT]) -> Self {
        let knob = |i: usize| f64::from(params[i]).clamp(0.0, 1.0);
        let step = |i: usize| {
            let count = discrete_steps(i).unwrap_or(1);
            selector(params[i], count)
        };

        // EGR RELEASE OFF "disables the Release segment of both the filter and
        // volume envelopes" (page 35). The shortest release the panel has
        // rather than a jump to zero, because a jump to zero is a click.
        let release_on = step(P_EGR_REL) == 1;
        let release = |i: usize| if release_on { env_seconds(knob(i)) } else { ENV_MIN_S };

        Self {
            glide_on: step(P_GLIDE) == 1,
            octave: (step(P_OCTAVE) as f64 - 2.0) * 12.0,
            fine: (knob(P_FINE) * 2.0 - 1.0) * FINE_RANGE_SEMITONES,
            bend_up: BEND_UP[step(P_BEND_UP)],
            bend_down: BEND_DOWN[step(P_BEND_DOWN)],
            lfo_hz: lfo_hz(knob(P_LFO_RATE)),
            mod_amount: knob(P_MOD_AMT),
            mod_src: step(P_MOD_SRC),
            mod_dest: step(P_MOD_DEST),
            mod_dest2: step(P_MOD_DEST2),
            src5: step(P_SRC5),
            src6: step(P_SRC6),
            o1_oct: step(P_O1_OCT),
            o1_wave: knob(P_O1_WAVE),
            o1_level: knob(P_O1_LEVEL),
            glide_rate: glide_semitones_per_second(knob(P_GLIDE_RATE)),
            sync: step(P_SYNC) == 1,
            o2_oct: step(P_O2_OCT),
            o2_detune: (knob(P_O2_FREQ) * 2.0 - 1.0) * OSC2_RANGE_SEMITONES,
            o2_wave: knob(P_O2_WAVE),
            o2_level: knob(P_O2_LEVEL),
            cutoff: cutoff_hz(knob(P_CUTOFF)),
            resonance: knob(P_RESO),
            kb_amount: knob(P_KB_AMT),
            eg_amount: knob(P_EG_AMT) * 2.0 - 1.0,
            overload: knob(P_OVERLOAD),
            poles: step(P_POLES) + 1,
            vel_sens: (step(P_VEL_SENS) as f64 - 8.0) / 8.0,
            f_times: EnvTimes {
                attack: env_seconds(knob(P_F_ATTACK)),
                decay: env_seconds(knob(P_F_DECAY)),
                sustain: knob(P_F_SUSTAIN),
                release: release(P_F_RELEASE),
            },
            v_times: EnvTimes {
                attack: env_seconds(knob(P_V_ATTACK)),
                decay: env_seconds(knob(P_V_DECAY)),
                sustain: knob(P_V_SUSTAIN),
                release: release(P_V_RELEASE),
            },
            gate_mode: step(P_GATE),
            priority: step(P_PRIORITY),
            volume: knob(P_VOLUME),
        }
    }
}

// ── The instrument ──

/// How many keys the keyboard will keep track of at once.
///
/// A fixed array rather than a `Vec`, because this is read and written on the
/// audio thread and a `Vec` that reallocates on the seventeenth simultaneous
/// key is a defect this project has already shipped once. Sixteen is well past
/// what two hands hold down, and the seventeenth key pushes the oldest off the
/// bottom rather than being dropped, so last-note priority stays correct.
const MAX_HELD: usize = 16;

/// The maximum number of MIDI events sorted in place per block.
const MAX_EVENTS: usize = 256;

pub struct LittlePhatty {
    params: [f32; PARAM_COUNT],
    sample_rate: f64,

    held: [u8; MAX_HELD],
    held_velocity: [u8; MAX_HELD],
    held_len: usize,
    current_note: u8,
    current_velocity: u8,

    /// Where the pitch CV is now, and where it is heading, in note numbers.
    glide_note: f64,
    target_note: f64,
    /// False until the first note, so that the first note does not glide up
    /// from note zero.
    pitched: bool,

    osc1: Trapezoid,
    osc2: Trapezoid,
    /// Oscillator 2's previous output, which is what it contributes when it is
    /// the modulation source. One sample old by necessity: it modulates
    /// something that can modulate it.
    osc2_previous: f64,
    filter: Ladder,
    filter_env: Envelope,
    volume_env: Envelope,
    lfo: Lfo,
    noise: Noise,
    sample_hold: f64,
    dc: DcBlock,
    /// CC 1. On the instrument the mod bus is scaled by the wheel and the
    /// wheel rests at zero; there is no wheel here, so it rests at full and
    /// AMOUNT is the control.
    mod_wheel: f64,
    /// The pitch wheel, -1 to +1, scaled by whichever of the two PITCH BEND
    /// ranges it is heading towards.
    bend: f64,
}

impl LittlePhatty {
    #[must_use]
    pub fn new() -> Self {
        let sr = 44_100.0;
        Self {
            params: PARAM_DEFAULTS,
            sample_rate: sr,
            held: [0; MAX_HELD],
            held_velocity: [0; MAX_HELD],
            held_len: 0,
            current_note: 60,
            current_velocity: 100,
            glide_note: 60.0,
            target_note: 60.0,
            pitched: false,
            osc1: Trapezoid::new(),
            osc2: Trapezoid::new(),
            osc2_previous: 0.0,
            filter: Ladder::new(),
            filter_env: Envelope::new(sr),
            volume_env: Envelope::new(sr),
            lfo: Lfo::new(),
            noise: Noise::new(),
            sample_hold: 0.0,
            dc: DcBlock::new(),
            mod_wheel: 1.0,
            bend: 0.0,
        }
    }

    /// The whole panel for a patch, for a caller that wants to load one
    /// without an engine around it — the editor's patch knob, a level
    /// measurement, a test.
    #[must_use]
    pub fn params_for_patch(patch_value: f32) -> [f32; PARAM_COUNT] {
        let index = selector(patch_value, PATCH_COUNT);
        let mut out = BANK[index].panel();
        out[P_PATCH] = knob_for(index, PATCH_COUNT);
        out
    }

    fn sync_params_from_patch(&mut self) {
        self.params = Self::params_for_patch(self.params[P_PATCH]);
    }

    /// Which held key sounds, under the selected priority.
    fn selected(&self, priority: usize) -> Option<usize> {
        if self.held_len == 0 {
            return None;
        }
        let keys = &self.held[..self.held_len];
        Some(match priority {
            // Low note, as on a Minimoog.
            0 => keys
                .iter()
                .enumerate()
                .min_by_key(|(_, n)| **n)
                .map_or(0, |(i, _)| i),
            1 => keys
                .iter()
                .enumerate()
                .max_by_key(|(_, n)| **n)
                .map_or(0, |(i, _)| i),
            // Last note, which is the instrument's default.
            _ => self.held_len - 1,
        })
    }

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

    fn note_on(&mut self, note: u8, velocity: u8, panel: &Panel) {
        let was_holding = self.held_len > 0;
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

        // Under low- or high-note priority a key that loses the contest does
        // nothing at all: no pitch change, no trigger. It is only remembered,
        // so that letting go of the winner hands the voice to it.
        let Some(at) = self.selected(panel.priority) else { return };
        if self.held[at] != note {
            return;
        }

        self.current_note = note;
        self.current_velocity = velocity;
        self.target_note = f64::from(note);
        if !self.pitched {
            self.glide_note = self.target_note;
            self.pitched = true;
        }

        // A note arriving with nothing held and nothing sounding is a fresh
        // one whatever the gate mode says; legato only applies to a note that
        // overlaps another.
        let fresh = !was_holding && !self.volume_env.is_active();
        if fresh || panel.gate_mode != 0 {
            if panel.gate_mode == 2 {
                self.filter_env.trigger_from_zero();
                self.volume_env.trigger_from_zero();
            } else {
                self.filter_env.trigger();
                self.volume_env.trigger();
            }
        }
        if fresh {
            self.filter.start(panel.resonance);
        }
    }

    fn note_off(&mut self, note: u8, panel: &Panel) {
        self.forget(note);
        match self.selected(panel.priority) {
            // The held-note return: the voice goes back to the key that is
            // still down, at its own pitch and its own velocity, and the
            // envelopes are not retriggered in any gate mode.
            Some(at) => {
                self.current_note = self.held[at];
                self.current_velocity = self.held_velocity[at];
                self.target_note = f64::from(self.current_note);
            }
            None => {
                self.filter_env.release_env();
                self.volume_env.release_env();
            }
        }
    }

    fn all_notes_off(&mut self) {
        self.held_len = 0;
        self.filter_env.release_env();
        self.volume_env.release_env();
    }

    fn kill_all(&mut self) {
        self.held_len = 0;
        self.filter_env.kill();
        self.volume_env.kill();
        self.filter.reset();
        self.osc1.reset();
        self.osc2.reset();
        self.osc2_previous = 0.0;
        self.dc.reset();
    }
}

impl Default for LittlePhatty {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for LittlePhatty {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "Little Phatty".into(),
            version: "0.1.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.filter_env = Envelope::new(sample_rate);
        self.volume_env = Envelope::new(sample_rate);
        self.filter.reset();
        self.osc1.reset();
        self.osc2.reset();
        self.lfo.reset();
        self.dc.reset();
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() {
            return;
        }
        let buf_len = outputs[0].len();
        let sr = self.sample_rate;
        let panel = Panel::read(&self.params);

        self.filter_env.set_times(panel.f_times);
        self.volume_env.set_times(panel.v_times);

        let dc_coefficient = (-std::f64::consts::TAU * DC_BLOCK_HZ / sr).exp();
        // Everything that moves the cutoff — the envelope, tracking, velocity,
        // the mod bus — is added in octaves and then clamped here. The panel's
        // own top is the ceiling: a VCF core has a rail, and a control voltage
        // that asks for more does not get it.
        let cutoff_ceiling = (sr * 0.45).min(CUTOFF_MAX_HZ);
        let glide_step = panel.glide_rate / sr;
        // The manual's own example puts a vibrato at AMOUNT 50%, which a
        // linear taper would make a three-semitone one. Squaring the knob
        // spends its first half on the depths a player reaches for.
        let mod_depth = panel.mod_amount * panel.mod_amount * self.mod_wheel;

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
                    0xE0 => {
                        // Fourteen bits, centre at 8192, and the two halves of
                        // the travel have their own ranges.
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

            // ── Control rate, which on a monosynth is the sample rate ──

            let filter_level = self.filter_env.tick();
            let volume_level = self.volume_env.tick();

            let (lfo_value, lfo_wrapped) = self.lfo.tick(panel.lfo_hz, sr, panel.mod_src.min(3));
            let noise = self.noise.tick();
            if lfo_wrapped {
                self.sample_hold = noise;
            }

            let source = match panel.mod_src {
                4 => {
                    if panel.src5 == 0 {
                        filter_level
                    } else {
                        self.sample_hold
                    }
                }
                5 => {
                    if panel.src6 == 0 {
                        self.osc2_previous
                    } else {
                        noise
                    }
                }
                _ => lfo_value,
            };
            let modulation = source * mod_depth;

            // One amount, up to two destinations: "The Modulation AMOUNT
            // control specifies both the primary and secondary modulation
            // amounts - there is no separate amount control for the secondary
            // modulation" (page 36).
            let mut pitch_mod = 0.0;
            let mut filter_mod = 0.0;
            let mut wave_mod = 0.0;
            let mut osc2_mod = 0.0;
            let destinations = [Some(panel.mod_dest), panel.mod_dest2.checked_sub(1)];
            for destination in destinations.into_iter().flatten() {
                match destination {
                    1 => filter_mod += modulation * MOD_FILTER_OCTAVES,
                    2 => wave_mod += modulation,
                    3 => osc2_mod += modulation * MOD_PITCH_SEMITONES,
                    _ => pitch_mod += modulation * MOD_PITCH_SEMITONES,
                }
            }

            // Glide, at a constant rate in semitones per second.
            if panel.glide_on {
                let remaining = self.target_note - self.glide_note;
                if remaining.abs() <= glide_step {
                    self.glide_note = self.target_note;
                } else {
                    self.glide_note += glide_step.copysign(remaining);
                }
            } else {
                self.glide_note = self.target_note;
            }

            // ── Oscillators ──

            let bend = self.bend * if self.bend >= 0.0 { panel.bend_up } else { -panel.bend_down };
            let base = self.glide_note + panel.fine + panel.octave + bend + pitch_mod;
            let hz1 = 440.0 * ((base - 69.0) / 12.0).exp2() * OCTAVE_FEET[panel.o1_oct];
            let hz2 = 440.0 * ((base - 69.0 + panel.o2_detune + osc2_mod) / 12.0).exp2()
                * OCTAVE_FEET[panel.o2_oct];
            let dt1 = (hz1 / sr).clamp(0.0, 0.45);
            let dt2 = (hz2 / sr).clamp(0.0, 0.45);

            // "Although the waveforms can be set from the front panel
            // individually for each oscillator, modulation is applied to both
            // waveform controls simultaneously" (page 12).
            let shape1 = Shape::at(panel.o1_wave + wave_mod);
            let shape2 = Shape::at(panel.o2_wave + wave_mod);

            // Where in this sample oscillator 1 restarts, which is the only
            // thing oscillator 2 needs from it. Read before the tick, because
            // the tick is what moves the phase past it.
            let sync_at = if panel.sync && dt1 > 0.0 && self.osc1.phase + dt1 >= 1.0 {
                Some((1.0 - self.osc1.phase) / dt1)
            } else {
                None
            };

            let out1 = self.osc1.tick(dt1, &shape1, None);
            let out2 = self.osc2.tick(dt2, &shape2, sync_at);
            self.osc2_previous = out2;
            let mix = out1 * panel.o1_level + out2 * panel.o2_level;

            // ── Filter ──

            let driven = overload_pre(mix, panel.overload);
            let blocked = self.dc.tick(driven, dc_coefficient);

            let octaves = panel.kb_amount * (self.glide_note - 60.0) / 12.0
                + panel.eg_amount * filter_level * EG_AMOUNT_OCTAVES
                + panel.vel_sens * (f64::from(self.current_velocity) / 127.0) * VELOCITY_OCTAVES
                + filter_mod;
            let cutoff = (panel.cutoff * octaves.exp2()).clamp(5.0, cutoff_ceiling);

            let filtered =
                self.filter.process(blocked, cutoff, panel.resonance, panel.poles, sr);
            let shaped = overload_post(filtered, panel.overload);

            // ── Amplifier and output ──
            //
            // Velocity is not here, and that is the instrument: "Hardware
            // velocity affects filter cutoff only." It is why an LP feels the
            // way it does under the fingers.
            let out = shaped * volume_level * panel.volume;
            let sample = soft_saturate(out as f32 * OUTPUT_TRIM);

            outputs[0][i] = sample;
            if stereo {
                outputs[1][i] = sample;
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
                P_F_ATTACK | P_F_DECAY | P_F_RELEASE | P_V_ATTACK | P_V_DECAY | P_V_RELEASE => {
                    "s".into()
                }
                P_LFO_RATE => "Hz".into(),
                P_CUTOFF => "Hz".into(),
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
        self.lfo.reset();
        self.sample_hold = 0.0;
        self.mod_wheel = 1.0;
        self.bend = 0.0;
        self.pitched = false;
    }
}

// ── The preset bank ──
//
// A hundred slots, because the instrument has a hundred slots. What is *in*
// them is ours.
//
// Moog's own Stage II bank is named on the last page of the manual — MOOG
// STAGE II, AMPHIBIANBASS, TAURUS BASS3, and on down to JOTTED 8TH — and that
// is all that is published. The parameter values behind those names have never
// been released, the machine's SysEx dumps are not in circulation, and the
// Moog forum's own request for them went unanswered. A bank claiming to be the
// factory set would therefore be a hundred guesses wearing someone else's
// labels, which is worse than a hundred honest patches.
//
// So these are original, named in our own voice, and voiced to cover the
// ground the real bank covers. The balance follows Moog's: their first
// thirty-six slots are almost entirely basses, and so are ours, because that
// is what this instrument is for.
//
// * **Moog Bass** — the round, weighty end. 16' and 8', the filter envelope
//   doing the work, resonance kept low enough that the fundamental survives.
// * **Overload** — the same thing with the growl knob up, which is the LP's
//   own distortion and the reason its basses are not the Juno's.
// * **Sync** — oscillator 2 reset by oscillator 1, with the OSC 2 FREQ knob
//   or the mod bus sweeping the formant.
// * **Lead** — single-line voices, mostly 8' and 4'.
// * **Morph** — the wave control as the subject rather than as a setting:
//   positions between the four labelled shapes, and the mod bus pointed at
//   WAVE, which is a thing only this instrument in the rack can do.
// * **S&H and effects** — the sample-and-hold source, the audio-rate LFO, and
//   the filter used as an oscillator.
// * **Slope** — one, two and three poles, which are three different filters.
// * **Pluck** — the filter envelope as a percussion generator.
// * **Drone** — long, held, mono textures.

pub const PATCH_COUNT: usize = 100;

/// One patch: every control except the patch selector itself and FINE TUNE,
/// which no patch moves because it is there to match an external reference
/// rather than to voice a sound.
#[derive(Debug, Clone, Copy)]
struct Program {
    /// Twelve characters at most, which is what the editor's selector row
    /// leaves for a label.
    name: &'static str,
    /// OCTAVE transpose (0 = -2 … 4 = +2), GLIDE on/off, GLIDE RATE.
    kbd: (u8, bool, f32),
    /// LFO RATE, AMOUNT.
    lfo: [f32; 2],
    /// SOURCE, DESTINATION, DEST 2, MOD SRC 5, MOD SRC 6.
    md: [u8; 5],
    /// OSC 1 OCTAVE, OSC 2 OCTAVE, 1-2 SYNC.
    feet: (u8, u8, bool),
    /// OSC 1 WAVE, OSC 1 LEVEL, OSC 2 FREQ, OSC 2 WAVE, OSC 2 LEVEL.
    osc: [f32; 5],
    /// CUTOFF, RESONANCE, KB AMOUNT, EGR AMNT, OVERLOAD.
    vcf: [f32; 5],
    /// FILTER POLES (1-4), FILT SENS (-8..+8).
    filt: (u8, i8),
    /// Filter EG: attack, decay, sustain, release.
    feg: [f32; 4],
    /// Volume EG: attack, decay, sustain, release.
    veg: [f32; 4],
    /// EGR RELEASE on, GATE (0 leg on, 1 leg off, 2 EGR reset),
    /// KB PRIORITY (0 low, 1 high, 2 last).
    misc: (bool, u8, u8),
    /// VOLUME.
    level: f32,
}

impl Program {
    /// This patch as a parameter block. The selector itself is left at zero;
    /// [`LittlePhatty::params_for_patch`] fills it in with the slot the caller
    /// asked for.
    fn panel(&self) -> [f32; PARAM_COUNT] {
        let mut p = [0.0f32; PARAM_COUNT];
        p[P_GLIDE] = knob_for(usize::from(self.kbd.1), 2);
        p[P_OCTAVE] = knob_for(self.kbd.0 as usize, 5);
        p[P_FINE] = 0.5;
        // The pitch wheel's ranges are the instrument's own default, which is
        // where its calibration preset leaves them: two semitones each way.
        // No patch in the bank moves them; they are on the panel because they
        // are stored per preset and a player may want them elsewhere.
        p[P_BEND_UP] = knob_for(1, BEND_UP.len());
        p[P_BEND_DOWN] = knob_for(1, BEND_DOWN.len());
        p[P_LFO_RATE] = self.lfo[0];
        p[P_MOD_AMT] = self.lfo[1];
        p[P_MOD_SRC] = knob_for(self.md[0] as usize, 6);
        p[P_MOD_DEST] = knob_for(self.md[1] as usize, 4);
        p[P_MOD_DEST2] = knob_for(self.md[2] as usize, 5);
        p[P_SRC5] = knob_for(self.md[3] as usize, 2);
        p[P_SRC6] = knob_for(self.md[4] as usize, 2);
        p[P_O1_OCT] = knob_for(self.feet.0 as usize, 4);
        p[P_O1_WAVE] = self.osc[0];
        p[P_O1_LEVEL] = self.osc[1];
        p[P_GLIDE_RATE] = self.kbd.2;
        p[P_SYNC] = knob_for(usize::from(self.feet.2), 2);
        p[P_O2_OCT] = knob_for(self.feet.1 as usize, 4);
        p[P_O2_FREQ] = self.osc[2];
        p[P_O2_WAVE] = self.osc[3];
        p[P_O2_LEVEL] = self.osc[4];
        p[P_CUTOFF] = self.vcf[0];
        p[P_RESO] = self.vcf[1];
        p[P_KB_AMT] = self.vcf[2];
        p[P_EG_AMT] = self.vcf[3];
        p[P_OVERLOAD] = self.vcf[4];
        p[P_POLES] = knob_for(self.filt.0 as usize - 1, 4);
        p[P_VEL_SENS] = knob_for((self.filt.1 + 8) as usize, VEL_LABELS.len());
        p[P_F_ATTACK] = self.feg[0];
        p[P_F_DECAY] = self.feg[1];
        p[P_F_SUSTAIN] = self.feg[2];
        p[P_F_RELEASE] = self.feg[3];
        p[P_V_ATTACK] = self.veg[0];
        p[P_V_DECAY] = self.veg[1];
        p[P_V_SUSTAIN] = self.veg[2];
        p[P_V_RELEASE] = self.veg[3];
        p[P_EGR_REL] = knob_for(usize::from(self.misc.0), 2);
        p[P_GATE] = knob_for(self.misc.1 as usize, 3);
        p[P_PRIORITY] = knob_for(self.misc.2 as usize, 3);
        p[P_VOLUME] = self.level;
        p
    }
}

/// The patch names, short enough to be the editor's labels as well.
pub const PATCH_NAMES: [&str; PATCH_COUNT] = derive_names();

const fn derive_names() -> [&'static str; PATCH_COUNT] {
    let mut out = [""; PATCH_COUNT];
    let mut i = 0;
    while i < PATCH_COUNT {
        out[i] = BANK[i].name;
        i += 1;
    }
    out
}

/// The knob position that selects patch `index`, for a caller sweeping the
/// bank from outside — a level measurement, an export, a test.
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

const BANK: [Program; PATCH_COUNT] = [
    // ── Moog bass ──
    Program { name: "Taurus Deep",
        kbd: (2, false, 0.30), lfo: [0.35, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.33, 0.85, 0.500, 0.10, 0.55],
        vcf: [0.30, 0.20, 0.35, 0.72, 0.10], filt: (4, 3),
        feg: [0.00, 0.62, 0.20, 0.45], veg: [0.02, 0.70, 0.85, 0.40],
        misc: (true, 0, 2), level: 0.72 },
    Program { name: "Sub Anchor",
        kbd: (2, false, 0.25), lfo: [0.30, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.05, 0.90, 0.500, 0.00, 0.72],
        vcf: [0.24, 0.05, 0.20, 0.58, 0.00], filt: (4, 0),
        feg: [0.05, 0.55, 0.40, 0.40], veg: [0.05, 0.80, 0.90, 0.55],
        misc: (true, 0, 2), level: 0.80 },
    Program { name: "Round Bass",
        kbd: (2, false, 0.20), lfo: [0.40, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 1, false), osc: [0.67, 0.80, 0.500, 0.33, 0.45],
        vcf: [0.34, 0.44, 0.40, 0.72, 0.05], filt: (4, 4),
        feg: [0.00, 0.46, 0.10, 0.30], veg: [0.01, 0.58, 0.72, 0.32],
        misc: (true, 0, 2), level: 0.70 },
    Program { name: "Bass Pillar",
        kbd: (2, false, 0.35), lfo: [0.28, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.33, 0.95, 0.545, 0.33, 0.90],
        vcf: [0.28, 0.15, 0.30, 0.62, 0.18], filt: (4, 2),
        feg: [0.00, 0.70, 0.30, 0.50], veg: [0.03, 0.85, 0.90, 0.50],
        misc: (true, 0, 2), level: 0.60 },
    Program { name: "Thumb Bass",
        kbd: (2, false, 0.15), lfo: [0.45, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 1, false), osc: [0.33, 0.88, 0.500, 0.67, 0.30],
        vcf: [0.20, 0.55, 0.45, 0.86, 0.12], filt: (4, 6),
        feg: [0.00, 0.36, 0.00, 0.22], veg: [0.00, 0.48, 0.55, 0.22],
        misc: (true, 2, 2), level: 0.72 },
    Program { name: "Wood Bass",
        kbd: (2, false, 0.40), lfo: [0.33, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.00, 0.82, 0.485, 0.67, 0.40],
        vcf: [0.26, 0.08, 0.55, 0.66, 0.00], filt: (4, 2),
        feg: [0.02, 0.48, 0.10, 0.34], veg: [0.03, 0.60, 0.62, 0.32],
        misc: (true, 0, 2), level: 0.78 },
    Program { name: "Rubber Bass",
        kbd: (2, false, 0.22), lfo: [0.42, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.90, 0.85, 0.500, 0.90, 0.55],
        vcf: [0.22, 0.62, 0.38, 0.80, 0.08], filt: (4, 5),
        feg: [0.00, 0.42, 0.05, 0.28], veg: [0.00, 0.58, 0.68, 0.30],
        misc: (true, 1, 2), level: 0.82 },
    Program { name: "Octave Bass",
        kbd: (2, false, 0.28), lfo: [0.36, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 1, false), osc: [0.10, 0.90, 0.500, 0.33, 0.62],
        vcf: [0.36, 0.10, 0.50, 0.64, 0.00], filt: (4, 3),
        feg: [0.03, 0.60, 0.30, 0.42], veg: [0.04, 0.74, 0.88, 0.42],
        misc: (true, 0, 2), level: 0.70 },
    Program { name: "Muted Bass",
        kbd: (2, false, 0.30), lfo: [0.30, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.12, 0.86, 0.500, 0.20, 0.60],
        vcf: [0.16, 0.02, 0.18, 0.56, 0.00], filt: (4, 1),
        feg: [0.06, 0.60, 0.30, 0.44], veg: [0.06, 0.78, 0.85, 0.48],
        misc: (true, 0, 2), level: 0.88 },
    Program { name: "Fifth Bass",
        kbd: (2, false, 0.26), lfo: [0.22, 0.30], md: [0, 1, 0, 0, 0],
        feet: (0, 0, false), osc: [0.33, 0.72, 1.000, 0.67, 0.78],
        vcf: [0.33, 0.62, 0.40, 0.76, 0.28], filt: (3, 5),
        feg: [0.00, 0.40, 0.08, 0.30], veg: [0.00, 0.62, 0.72, 0.34],
        misc: (true, 0, 2), level: 0.58 },
    Program { name: "Tri Sub",
        kbd: (1, false, 0.30), lfo: [0.30, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.00, 1.00, 0.500, 0.00, 0.00],
        vcf: [0.40, 0.00, 0.30, 0.50, 0.00], filt: (4, 0),
        feg: [0.10, 0.50, 0.50, 0.40], veg: [0.04, 0.70, 0.92, 0.45],
        misc: (true, 0, 2), level: 0.86 },
    Program { name: "Pulse Bass",
        kbd: (2, false, 0.24), lfo: [0.38, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 1, false), osc: [1.00, 0.90, 0.500, 1.00, 0.50],
        vcf: [0.30, 0.42, 0.36, 0.76, 0.14], filt: (4, 4),
        feg: [0.00, 0.44, 0.08, 0.30], veg: [0.00, 0.62, 0.70, 0.32],
        misc: (true, 0, 2), level: 0.92 },
    Program { name: "Sine Floor",
        kbd: (2, false, 0.30), lfo: [0.28, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.00, 0.12, 0.500, 0.00, 0.00],
        vcf: [0.18, 0.99, 0.90, 0.56, 0.00], filt: (4, 0),
        feg: [0.02, 0.45, 0.30, 0.40], veg: [0.04, 0.70, 0.88, 0.46],
        misc: (true, 0, 2), level: 0.62 },
    Program { name: "Dub Weight",
        kbd: (2, false, 0.32), lfo: [0.14, 0.42], md: [0, 1, 0, 0, 0],
        feet: (0, 0, false), osc: [0.33, 0.88, 0.510, 0.67, 0.55],
        vcf: [0.24, 0.35, 0.25, 0.60, 0.10], filt: (4, 0),
        feg: [0.08, 0.65, 0.35, 0.48], veg: [0.05, 0.82, 0.90, 0.55],
        misc: (true, 0, 2), level: 0.66 },
    Program { name: "Slide Bass",
        kbd: (2, true, 0.72), lfo: [0.34, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.33, 0.90, 0.500, 0.42, 0.48],
        vcf: [0.29, 0.30, 0.38, 0.72, 0.12], filt: (4, 3),
        feg: [0.00, 0.54, 0.16, 0.36], veg: [0.02, 0.70, 0.82, 0.38],
        misc: (true, 0, 2), level: 0.68 },
    Program { name: "Sequence Lo",
        kbd: (2, false, 0.10), lfo: [0.50, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.58, 0.92, 0.500, 0.58, 0.28],
        vcf: [0.19, 0.48, 0.30, 0.84, 0.16], filt: (4, 7),
        feg: [0.00, 0.30, 0.00, 0.14], veg: [0.00, 0.36, 0.30, 0.16],
        misc: (true, 2, 2), level: 0.84 },
    Program { name: "Fat Unison",
        kbd: (2, false, 0.34), lfo: [0.30, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.33, 0.92, 0.522, 0.33, 0.92],
        vcf: [0.36, 0.20, 0.45, 0.66, 0.20], filt: (4, 4),
        feg: [0.01, 0.58, 0.25, 0.42], veg: [0.03, 0.78, 0.88, 0.44],
        misc: (true, 0, 2), level: 0.54 },
    Program { name: "Bass Bloom",
        kbd: (2, false, 0.30), lfo: [0.26, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 1, false), osc: [0.20, 0.86, 0.500, 0.50, 0.44],
        vcf: [0.22, 0.38, 0.30, 0.78, 0.06], filt: (4, 2),
        feg: [0.56, 0.70, 0.35, 0.52], veg: [0.08, 0.85, 0.90, 0.52],
        misc: (true, 0, 2), level: 0.70 },

    // ── Overload ──
    Program { name: "Growl Bass",
        kbd: (2, false, 0.26), lfo: [0.36, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.33, 0.80, 0.508, 0.33, 0.70],
        vcf: [0.27, 0.40, 0.35, 0.74, 0.70], filt: (4, 4),
        feg: [0.00, 0.52, 0.15, 0.36], veg: [0.01, 0.70, 0.82, 0.38],
        misc: (true, 0, 2), level: 0.46 },
    Program { name: "Tarmac",
        kbd: (2, false, 0.22), lfo: [0.40, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 1, false), osc: [0.67, 0.85, 0.500, 0.67, 0.55],
        vcf: [0.28, 0.48, 0.30, 0.74, 0.90], filt: (4, 3),
        feg: [0.00, 0.38, 0.05, 0.26], veg: [0.00, 0.58, 0.66, 0.30],
        misc: (true, 0, 2), level: 0.40 },
    Program { name: "Coal Face",
        kbd: (2, false, 0.30), lfo: [0.30, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 1, false), osc: [0.00, 0.95, 0.500, 0.06, 0.45],
        vcf: [0.17, 0.10, 0.18, 0.58, 1.00], filt: (2, 2),
        feg: [0.02, 0.58, 0.24, 0.42], veg: [0.03, 0.80, 0.88, 0.46],
        misc: (true, 0, 2), level: 0.40 },
    Program { name: "Snarl",
        kbd: (2, false, 0.20), lfo: [0.44, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.42, 0.78, 0.532, 0.42, 0.66],
        vcf: [0.33, 0.62, 0.42, 0.80, 0.62], filt: (4, 5),
        feg: [0.00, 0.44, 0.10, 0.30], veg: [0.00, 0.62, 0.72, 0.32],
        misc: (true, 1, 2), level: 0.44 },
    Program { name: "Fuzz Anchor",
        kbd: (1, false, 0.28), lfo: [0.32, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.33, 0.90, 0.500, 1.00, 0.60],
        vcf: [0.19, 0.18, 0.20, 0.60, 0.85], filt: (4, 0),
        feg: [0.04, 0.62, 0.30, 0.46], veg: [0.05, 0.84, 0.90, 0.52],
        misc: (true, 0, 2), level: 0.42 },
    Program { name: "Grit Stack",
        kbd: (2, false, 0.34), lfo: [0.30, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 0, false), osc: [0.72, 0.82, 0.556, 0.15, 0.74],
        vcf: [0.38, 0.60, 0.44, 0.66, 0.50], filt: (3, 4),
        feg: [0.00, 0.62, 0.36, 0.44], veg: [0.02, 0.78, 0.86, 0.44],
        misc: (true, 0, 2), level: 0.48 },
    Program { name: "Overdriven",
        kbd: (2, false, 0.24), lfo: [0.30, 0.24], md: [1, 1, 0, 0, 0],
        feet: (1, 2, false), osc: [0.33, 0.88, 0.500, 1.00, 0.55],
        vcf: [0.52, 0.28, 0.62, 0.60, 0.75], filt: (2, 6),
        feg: [0.00, 0.44, 0.30, 0.32], veg: [0.01, 0.62, 0.84, 0.34],
        misc: (true, 0, 2), level: 0.44 },
    Program { name: "Bark Bass",
        kbd: (2, false, 0.18), lfo: [0.46, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.75, 0.86, 0.500, 0.75, 0.40],
        vcf: [0.20, 0.58, 0.32, 0.88, 0.68], filt: (4, 7),
        feg: [0.00, 0.34, 0.00, 0.20], veg: [0.00, 0.46, 0.48, 0.22],
        misc: (true, 2, 2), level: 0.50 },
    Program { name: "Diesel",
        kbd: (2, false, 0.30), lfo: [0.26, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.10, 0.92, 1.000, 0.10, 0.78],
        vcf: [0.18, 0.26, 0.18, 0.58, 0.95], filt: (4, 1),
        feg: [0.06, 0.66, 0.34, 0.48], veg: [0.06, 0.86, 0.90, 0.54],
        misc: (true, 0, 2), level: 0.38 },
    Program { name: "Torn Paper",
        kbd: (2, false, 0.32), lfo: [0.42, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 2, false), osc: [1.00, 0.80, 0.470, 1.00, 0.72],
        vcf: [0.40, 0.34, 0.50, 0.62, 1.00], filt: (4, 3),
        feg: [0.00, 0.46, 0.16, 0.32], veg: [0.01, 0.60, 0.70, 0.34],
        misc: (true, 0, 2), level: 0.44 },
    Program { name: "Anvil Bass",
        kbd: (2, false, 0.20), lfo: [0.36, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.33, 0.84, 0.500, 0.67, 0.52],
        vcf: [0.30, 0.46, 0.34, 0.78, 0.80], filt: (2, 5),
        feg: [0.00, 0.38, 0.05, 0.24], veg: [0.00, 0.52, 0.58, 0.26],
        misc: (true, 2, 2), level: 0.40 },
    Program { name: "Bad Weather",
        kbd: (2, false, 0.30), lfo: [0.10, 0.55], md: [0, 1, 0, 0, 0],
        feet: (0, 0, false), osc: [0.25, 0.86, 0.524, 0.58, 0.66],
        vcf: [0.26, 0.52, 0.26, 0.62, 0.88], filt: (4, 0),
        feg: [0.10, 0.68, 0.40, 0.52], veg: [0.08, 0.88, 0.92, 0.58],
        misc: (true, 0, 2), level: 0.38 },

    // ── Sync ──
    Program { name: "Sync Lead",
        kbd: (2, false, 0.30), lfo: [0.40, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 1, true), osc: [0.33, 0.10, 0.780, 0.33, 0.95],
        vcf: [0.62, 0.24, 0.60, 0.60, 0.15], filt: (4, 4),
        feg: [0.00, 0.55, 0.35, 0.36], veg: [0.02, 0.70, 0.85, 0.34],
        misc: (true, 0, 2), level: 0.62 },
    Program { name: "Sync Scream",
        kbd: (2, false, 0.30), lfo: [0.44, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 2, true), osc: [0.42, 0.08, 0.940, 0.42, 1.00],
        vcf: [0.70, 0.66, 0.70, 0.58, 0.30], filt: (4, 6),
        feg: [0.00, 0.50, 0.40, 0.32], veg: [0.01, 0.66, 0.88, 0.30],
        misc: (true, 1, 2), level: 0.52 },
    Program { name: "Sync Sweep",
        kbd: (2, false, 0.30), lfo: [0.16, 0.60], md: [0, 3, 0, 0, 0],
        feet: (1, 1, true), osc: [0.33, 0.06, 0.620, 0.33, 0.96],
        vcf: [0.66, 0.28, 0.55, 0.54, 0.18], filt: (4, 3),
        feg: [0.04, 0.60, 0.45, 0.40], veg: [0.06, 0.78, 0.90, 0.42],
        misc: (true, 0, 2), level: 0.58 },
    Program { name: "Hard Reset",
        kbd: (2, false, 0.30), lfo: [0.40, 1.00], md: [4, 3, 0, 0, 0],
        feet: (1, 1, true), osc: [0.42, 0.02, 0.500, 0.42, 1.00],
        vcf: [0.48, 0.50, 0.35, 0.88, 0.40], filt: (4, 6),
        feg: [0.00, 0.34, 0.00, 0.22], veg: [0.00, 0.44, 0.30, 0.24],
        misc: (true, 2, 2), level: 0.60 },
    Program { name: "Sync Bell",
        kbd: (3, false, 0.30), lfo: [0.38, 0.00], md: [0, 0, 0, 0, 0],
        feet: (2, 3, true), osc: [0.00, 0.06, 0.700, 0.20, 0.92],
        vcf: [0.74, 0.34, 0.65, 0.62, 0.05], filt: (4, 4),
        feg: [0.00, 0.42, 0.10, 0.44], veg: [0.00, 0.60, 0.45, 0.48],
        misc: (true, 2, 2), level: 0.66 },
    Program { name: "Sync Buzz",
        kbd: (2, false, 0.30), lfo: [0.42, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 1, true), osc: [0.58, 0.12, 0.860, 0.58, 0.94],
        vcf: [0.56, 0.40, 0.48, 0.62, 0.58], filt: (4, 4),
        feg: [0.00, 0.48, 0.30, 0.34], veg: [0.01, 0.64, 0.82, 0.32],
        misc: (true, 0, 2), level: 0.46 },
    Program { name: "Sync Whistle",
        kbd: (3, false, 0.30), lfo: [0.36, 0.00], md: [0, 0, 0, 0, 0],
        feet: (2, 3, true), osc: [0.90, 0.05, 0.880, 0.95, 0.90],
        vcf: [0.80, 0.30, 0.72, 0.54, 0.10], filt: (4, 3),
        feg: [0.02, 0.44, 0.40, 0.36], veg: [0.04, 0.62, 0.88, 0.36],
        misc: (true, 0, 2), level: 0.72 },
    Program { name: "Sync Stab",
        kbd: (2, false, 0.30), lfo: [0.40, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 2, true), osc: [0.33, 0.08, 0.660, 0.67, 0.96],
        vcf: [0.52, 0.44, 0.52, 0.80, 0.20], filt: (4, 7),
        feg: [0.00, 0.32, 0.00, 0.20], veg: [0.00, 0.42, 0.35, 0.22],
        misc: (true, 2, 2), level: 0.70 },

    // ── Lead ──
    Program { name: "Solo Saw",
        kbd: (2, false, 0.30), lfo: [0.42, 0.06], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.33, 0.90, 0.512, 0.33, 0.70],
        vcf: [0.58, 0.26, 0.62, 0.58, 0.12], filt: (4, 4),
        feg: [0.02, 0.56, 0.40, 0.36], veg: [0.03, 0.72, 0.88, 0.34],
        misc: (true, 0, 2), level: 0.58 },
    Program { name: "Reed Lead",
        kbd: (2, false, 0.30), lfo: [0.44, 0.10], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.67, 0.88, 0.500, 0.67, 0.40],
        vcf: [0.48, 0.38, 0.55, 0.62, 0.08], filt: (4, 5),
        feg: [0.04, 0.50, 0.32, 0.34], veg: [0.06, 0.66, 0.86, 0.32],
        misc: (true, 0, 2), level: 0.64 },
    Program { name: "Glass Lead",
        kbd: (3, false, 0.30), lfo: [0.40, 0.08], md: [0, 0, 0, 0, 0],
        feet: (2, 2, false), osc: [0.00, 0.92, 0.505, 0.00, 0.68],
        vcf: [0.82, 0.14, 0.70, 0.52, 0.00], filt: (4, 2),
        feg: [0.06, 0.48, 0.50, 0.38], veg: [0.08, 0.64, 0.90, 0.38],
        misc: (true, 0, 2), level: 0.86 },
    Program { name: "Whistle Top",
        kbd: (3, false, 0.30), lfo: [0.46, 0.12], md: [0, 0, 0, 0, 0],
        feet: (3, 3, false), osc: [0.95, 0.86, 0.500, 0.95, 0.30],
        vcf: [0.78, 0.48, 0.75, 0.52, 0.05], filt: (4, 3),
        feg: [0.05, 0.46, 0.45, 0.36], veg: [0.07, 0.60, 0.88, 0.34],
        misc: (true, 0, 2), level: 0.94 },
    Program { name: "Portamento",
        kbd: (2, true, 0.86), lfo: [0.40, 0.05], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.62, 0.88, 0.528, 0.20, 0.62],
        vcf: [0.50, 0.52, 0.58, 0.66, 0.05], filt: (2, 4),
        feg: [0.10, 0.60, 0.50, 0.44], veg: [0.16, 0.74, 0.90, 0.48],
        misc: (true, 0, 2), level: 0.58 },
    Program { name: "Brass Lead",
        kbd: (2, false, 0.30), lfo: [0.38, 0.07], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.33, 0.86, 0.516, 0.42, 0.74],
        vcf: [0.40, 0.32, 0.50, 0.76, 0.18], filt: (4, 6),
        feg: [0.22, 0.58, 0.30, 0.40], veg: [0.20, 0.72, 0.86, 0.38],
        misc: (true, 0, 2), level: 0.56 },
    Program { name: "Ribbon Lead",
        kbd: (2, true, 0.90), lfo: [0.34, 0.14], md: [0, 0, 0, 0, 0],
        feet: (1, 2, false), osc: [0.20, 0.90, 0.500, 0.33, 0.36],
        vcf: [0.60, 0.36, 0.60, 0.56, 0.10], filt: (4, 4),
        feg: [0.10, 0.52, 0.42, 0.42], veg: [0.14, 0.68, 0.90, 0.44],
        misc: (true, 0, 1), level: 0.70 },
    Program { name: "Flute Solo",
        kbd: (2, false, 0.30), lfo: [0.36, 0.16], md: [0, 0, 0, 0, 0],
        feet: (2, 2, false), osc: [0.00, 0.94, 0.500, 0.00, 0.00],
        vcf: [0.62, 0.06, 0.66, 0.50, 0.00], filt: (4, 1),
        feg: [0.30, 0.50, 0.55, 0.40], veg: [0.28, 0.66, 0.92, 0.40],
        misc: (true, 0, 2), level: 0.98 },
    Program { name: "Nasal Lead",
        kbd: (2, false, 0.30), lfo: [0.44, 0.09], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.96, 0.90, 0.494, 0.88, 0.62],
        vcf: [0.52, 0.70, 0.55, 0.56, 0.16], filt: (4, 5),
        feg: [0.02, 0.48, 0.38, 0.34], veg: [0.04, 0.64, 0.86, 0.32],
        misc: (true, 0, 2), level: 0.78 },
    Program { name: "Octave Lead",
        kbd: (2, false, 0.30), lfo: [0.40, 0.06], md: [0, 0, 0, 0, 0],
        feet: (1, 3, false), osc: [0.00, 0.90, 0.500, 0.67, 0.42],
        vcf: [0.50, 0.14, 0.60, 0.66, 0.00], filt: (4, 4),
        feg: [0.06, 0.58, 0.30, 0.40], veg: [0.08, 0.72, 0.86, 0.40],
        misc: (true, 0, 2), level: 0.62 },
    Program { name: "Vox Lead",
        kbd: (2, false, 0.30), lfo: [0.38, 0.11], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.80, 0.84, 0.540, 0.72, 0.80],
        vcf: [0.46, 0.76, 0.48, 0.62, 0.10], filt: (4, 3),
        feg: [0.08, 0.52, 0.36, 0.38], veg: [0.10, 0.68, 0.88, 0.38],
        misc: (true, 0, 2), level: 0.64 },
    Program { name: "Cut Lead",
        kbd: (2, false, 0.30), lfo: [0.42, 0.05], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.33, 0.92, 0.500, 0.58, 0.44],
        vcf: [0.68, 0.20, 0.64, 0.56, 0.22], filt: (3, 6),
        feg: [0.00, 0.50, 0.34, 0.32], veg: [0.02, 0.66, 0.84, 0.30],
        misc: (true, 1, 2), level: 0.62 },

    // ── Wave morph ──
    Program { name: "Morph Drift",
        kbd: (2, false, 0.30), lfo: [0.09, 0.75], md: [0, 2, 0, 0, 0],
        feet: (1, 1, false), osc: [0.30, 0.86, 0.508, 0.30, 0.72],
        vcf: [0.46, 0.30, 0.50, 0.58, 0.00], filt: (4, 3),
        feg: [0.16, 0.60, 0.50, 0.46], veg: [0.22, 0.78, 0.92, 0.50],
        misc: (true, 0, 2), level: 0.62 },
    Program { name: "Half Saw",
        kbd: (2, false, 0.30), lfo: [0.36, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.17, 0.92, 0.500, 0.17, 0.60],
        vcf: [0.58, 0.16, 0.60, 0.54, 0.00], filt: (4, 2),
        feg: [0.04, 0.52, 0.45, 0.38], veg: [0.05, 0.70, 0.90, 0.38],
        misc: (true, 0, 2), level: 0.80 },
    Program { name: "Between",
        kbd: (2, false, 0.30), lfo: [0.36, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.50, 0.90, 0.500, 0.50, 0.62],
        vcf: [0.50, 0.30, 0.55, 0.62, 0.05], filt: (4, 3),
        feg: [0.03, 0.54, 0.40, 0.36], veg: [0.04, 0.70, 0.88, 0.38],
        misc: (true, 0, 2), level: 0.68 },
    Program { name: "Wave Wash",
        kbd: (2, false, 0.30), lfo: [0.08, 0.72], md: [0, 2, 3, 0, 0],
        feet: (1, 0, false), osc: [0.40, 0.80, 0.518, 0.60, 0.76],
        vcf: [0.44, 0.34, 0.42, 0.56, 0.10], filt: (4, 0),
        feg: [0.40, 0.66, 0.55, 0.58], veg: [0.42, 0.82, 0.92, 0.62],
        misc: (true, 0, 2), level: 0.66 },
    Program { name: "Slow Morph",
        kbd: (2, false, 0.30), lfo: [0.04, 1.00], md: [0, 2, 0, 0, 0],
        feet: (1, 1, false), osc: [0.00, 0.88, 0.500, 0.50, 0.70],
        vcf: [0.56, 0.20, 0.52, 0.52, 0.00], filt: (4, 0),
        feg: [0.30, 0.60, 0.60, 0.52], veg: [0.34, 0.80, 0.94, 0.58],
        misc: (true, 0, 2), level: 0.70 },
    Program { name: "Pulse Width",
        kbd: (2, false, 0.30), lfo: [0.42, 0.34], md: [0, 2, 0, 0, 0],
        feet: (1, 2, false), osc: [0.88, 0.90, 0.500, 0.80, 0.62],
        vcf: [0.58, 0.34, 0.60, 0.54, 0.06], filt: (4, 3),
        feg: [0.02, 0.48, 0.40, 0.36], veg: [0.03, 0.66, 0.90, 0.38],
        misc: (true, 0, 2), level: 0.72 },
    Program { name: "PWM Strings",
        kbd: (2, false, 0.30), lfo: [0.22, 0.20], md: [0, 2, 0, 0, 0],
        feet: (1, 0, false), osc: [0.80, 0.78, 0.534, 0.80, 0.78],
        vcf: [0.46, 0.18, 0.46, 0.54, 0.00], filt: (4, 0),
        feg: [0.48, 0.66, 0.58, 0.60], veg: [0.50, 0.84, 0.94, 0.66],
        misc: (true, 0, 2), level: 0.68 },
    Program { name: "Thin Ice",
        kbd: (3, false, 0.30), lfo: [0.38, 0.00], md: [0, 0, 0, 0, 0],
        feet: (2, 2, false), osc: [1.00, 0.94, 0.502, 1.00, 0.86],
        vcf: [0.72, 0.42, 0.68, 0.52, 0.00], filt: (4, 2),
        feg: [0.08, 0.46, 0.44, 0.40], veg: [0.10, 0.62, 0.90, 0.42],
        misc: (true, 0, 2), level: 0.96 },
    Program { name: "Wave Chase",
        kbd: (2, false, 0.30), lfo: [0.75, 1.00], md: [3, 2, 0, 0, 0],
        feet: (2, 2, false), osc: [0.00, 0.86, 0.512, 0.90, 0.70],
        vcf: [0.66, 0.55, 0.55, 0.54, 0.05], filt: (4, 3),
        feg: [0.02, 0.50, 0.35, 0.36], veg: [0.03, 0.66, 0.86, 0.36],
        misc: (true, 0, 2), level: 0.58 },
    Program { name: "Shape Shift",
        kbd: (2, false, 0.30), lfo: [0.34, 0.80], md: [4, 2, 0, 1, 0],
        feet: (1, 1, false), osc: [0.35, 0.88, 0.500, 0.35, 0.62],
        vcf: [0.48, 0.36, 0.48, 0.58, 0.14], filt: (4, 4),
        feg: [0.02, 0.48, 0.36, 0.34], veg: [0.04, 0.64, 0.84, 0.34],
        misc: (true, 0, 2), level: 0.66 },
    Program { name: "Morph Bass",
        kbd: (2, false, 0.28), lfo: [0.18, 0.44], md: [0, 2, 0, 0, 0],
        feet: (0, 0, false), osc: [0.55, 0.90, 0.500, 0.55, 0.60],
        vcf: [0.28, 0.30, 0.32, 0.72, 0.15], filt: (4, 3),
        feg: [0.00, 0.54, 0.18, 0.36], veg: [0.02, 0.70, 0.82, 0.38],
        misc: (true, 0, 2), level: 0.70 },
    Program { name: "Tri To Saw",
        kbd: (2, false, 0.30), lfo: [0.36, 0.85], md: [4, 2, 0, 0, 0],
        feet: (1, 1, false), osc: [0.00, 0.92, 0.500, 0.00, 0.58],
        vcf: [0.56, 0.20, 0.55, 0.52, 0.06], filt: (4, 2),
        feg: [0.06, 0.60, 0.20, 0.44], veg: [0.05, 0.76, 0.86, 0.42],
        misc: (true, 0, 2), level: 0.74 },

    // ── Sample and hold, and effects ──
    Program { name: "Random Steps",
        kbd: (2, false, 0.30), lfo: [0.40, 0.50], md: [0, 0, 0, 1, 0],
        feet: (1, 1, false), osc: [0.33, 0.86, 0.500, 0.67, 0.44],
        vcf: [0.54, 0.34, 0.50, 0.58, 0.10], filt: (4, 3),
        feg: [0.02, 0.50, 0.40, 0.34], veg: [0.02, 0.66, 0.86, 0.34],
        misc: (true, 0, 2), level: 0.66 },
    Program { name: "Sample Hold",
        kbd: (2, false, 0.30), lfo: [0.36, 0.66], md: [0, 1, 0, 1, 0],
        feet: (1, 1, false), osc: [0.33, 0.88, 0.510, 0.33, 0.66],
        vcf: [0.36, 0.58, 0.35, 0.54, 0.12], filt: (4, 0),
        feg: [0.06, 0.56, 0.45, 0.40], veg: [0.08, 0.74, 0.90, 0.44],
        misc: (true, 0, 2), level: 0.62 },
    Program { name: "Computer",
        kbd: (3, false, 0.30), lfo: [0.62, 0.72], md: [0, 0, 0, 1, 0],
        feet: (2, 2, false), osc: [0.67, 0.90, 0.500, 0.67, 0.30],
        vcf: [0.66, 0.40, 0.30, 0.60, 0.08], filt: (4, 0),
        feg: [0.00, 0.30, 0.10, 0.18], veg: [0.00, 0.34, 0.40, 0.16],
        misc: (true, 2, 2), level: 0.80 },
    Program { name: "Alarm",
        kbd: (2, false, 0.30), lfo: [0.32, 0.62], md: [1, 0, 0, 0, 0],
        feet: (2, 2, false), osc: [0.67, 0.92, 0.500, 0.67, 0.00],
        vcf: [0.70, 0.22, 0.40, 0.52, 0.10], filt: (4, 0),
        feg: [0.02, 0.44, 0.50, 0.30], veg: [0.02, 0.60, 0.92, 0.28],
        misc: (true, 0, 2), level: 0.72 },
    Program { name: "Siren",
        kbd: (2, false, 0.30), lfo: [0.06, 0.90], md: [0, 0, 0, 0, 0],
        feet: (2, 2, false), osc: [0.33, 0.90, 0.500, 0.33, 0.00],
        vcf: [0.68, 0.30, 0.35, 0.50, 0.06], filt: (4, 0),
        feg: [0.10, 0.50, 0.60, 0.40], veg: [0.12, 0.70, 0.94, 0.44],
        misc: (true, 0, 2), level: 0.70 },
    Program { name: "Radio Chirp",
        kbd: (3, false, 0.30), lfo: [0.86, 0.44], md: [3, 0, 0, 0, 0],
        feet: (2, 3, false), osc: [0.33, 0.72, 0.640, 0.90, 0.60],
        vcf: [0.72, 0.50, 0.40, 0.56, 0.20], filt: (4, 0),
        feg: [0.00, 0.36, 0.20, 0.24], veg: [0.00, 0.44, 0.55, 0.22],
        misc: (true, 2, 2), level: 0.68 },
    Program { name: "Wind Tunnel",
        kbd: (2, false, 0.30), lfo: [0.30, 0.72], md: [5, 1, 0, 0, 1],
        feet: (1, 1, false), osc: [0.00, 0.10, 0.500, 0.00, 0.10],
        vcf: [0.42, 0.92, 0.20, 0.52, 0.00], filt: (4, 0),
        feg: [0.40, 0.70, 0.60, 0.60], veg: [0.44, 0.86, 0.94, 0.66],
        misc: (true, 0, 2), level: 0.62 },
    Program { name: "Static",
        kbd: (2, false, 0.30), lfo: [0.60, 0.85], md: [5, 2, 1, 0, 1],
        feet: (2, 2, false), osc: [0.55, 0.84, 0.506, 0.70, 0.60],
        vcf: [0.62, 0.30, 0.45, 0.54, 0.10], filt: (4, 0),
        feg: [0.04, 0.50, 0.40, 0.36], veg: [0.06, 0.66, 0.88, 0.36],
        misc: (true, 0, 2), level: 0.60 },
    Program { name: "Bleep Bloop",
        kbd: (3, false, 0.30), lfo: [0.48, 0.58], md: [0, 3, 0, 1, 0],
        feet: (2, 2, false), osc: [0.90, 0.70, 0.500, 0.90, 0.70],
        vcf: [0.64, 0.44, 0.35, 0.66, 0.05], filt: (4, 0),
        feg: [0.00, 0.28, 0.00, 0.16], veg: [0.00, 0.32, 0.30, 0.14],
        misc: (true, 2, 2), level: 0.84 },
    Program { name: "Sonar",
        kbd: (2, true, 0.62), lfo: [0.20, 0.00], md: [0, 0, 0, 0, 0],
        feet: (2, 2, false), osc: [0.00, 0.30, 0.500, 0.00, 0.00],
        vcf: [0.44, 0.96, 0.60, 0.66, 0.00], filt: (4, 0),
        feg: [0.00, 0.62, 0.00, 0.55], veg: [0.00, 0.80, 0.00, 0.72],
        misc: (true, 2, 2), level: 0.72 },

    // ── Slope ──
    Program { name: "2 Pole Bass",
        kbd: (2, false, 0.26), lfo: [0.34, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.33, 0.88, 0.500, 0.33, 0.55],
        vcf: [0.26, 0.34, 0.34, 0.70, 0.08], filt: (2, 3),
        feg: [0.00, 0.56, 0.18, 0.38], veg: [0.02, 0.72, 0.84, 0.38],
        misc: (true, 0, 2), level: 0.52 },
    Program { name: "1 Pole Pad",
        kbd: (2, false, 0.30), lfo: [0.18, 0.14], md: [0, 1, 0, 0, 0],
        feet: (1, 0, false), osc: [0.20, 0.72, 0.526, 0.45, 0.72],
        vcf: [0.30, 0.10, 0.35, 0.56, 0.00], filt: (1, 0),
        feg: [0.50, 0.70, 0.60, 0.62], veg: [0.52, 0.86, 0.94, 0.68],
        misc: (true, 0, 2), level: 0.52 },
    Program { name: "Open Ladder",
        kbd: (2, false, 0.30), lfo: [0.36, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.42, 0.84, 0.514, 0.42, 0.60],
        vcf: [0.42, 0.82, 0.50, 0.62, 0.10], filt: (2, 4),
        feg: [0.02, 0.52, 0.34, 0.36], veg: [0.04, 0.68, 0.86, 0.36],
        misc: (true, 0, 2), level: 0.48 },
    Program { name: "Leaky",
        kbd: (2, false, 0.30), lfo: [0.30, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 1, false), osc: [0.67, 0.86, 0.500, 0.20, 0.50],
        vcf: [0.14, 0.20, 0.25, 0.60, 0.06], filt: (1, 2),
        feg: [0.04, 0.58, 0.24, 0.42], veg: [0.05, 0.76, 0.86, 0.44],
        misc: (true, 0, 2), level: 0.60 },
    Program { name: "Bright 12dB",
        kbd: (2, false, 0.30), lfo: [0.40, 0.06], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.33, 0.90, 0.508, 0.33, 0.66],
        vcf: [0.60, 0.24, 0.62, 0.56, 0.10], filt: (2, 5),
        feg: [0.02, 0.50, 0.40, 0.34], veg: [0.03, 0.66, 0.88, 0.34],
        misc: (true, 0, 2), level: 0.46 },
    Program { name: "6dB Lead",
        kbd: (2, false, 0.30), lfo: [0.42, 0.08], md: [0, 0, 0, 0, 0],
        feet: (2, 2, false), osc: [0.00, 0.90, 0.500, 0.33, 0.44],
        vcf: [0.46, 0.14, 0.58, 0.58, 0.08], filt: (1, 4),
        feg: [0.04, 0.48, 0.42, 0.34], veg: [0.05, 0.64, 0.88, 0.34],
        misc: (true, 0, 2), level: 0.56 },
    Program { name: "Half Ladder",
        kbd: (2, false, 0.24), lfo: [0.36, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 1, false), osc: [0.58, 0.84, 0.520, 0.58, 0.62],
        vcf: [0.28, 0.46, 0.32, 0.74, 0.66], filt: (2, 4),
        feg: [0.00, 0.44, 0.12, 0.30], veg: [0.00, 0.60, 0.72, 0.32],
        misc: (true, 0, 2), level: 0.36 },
    Program { name: "Slope Swap",
        kbd: (2, false, 0.30), lfo: [0.38, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.33, 0.86, 0.500, 0.67, 0.58],
        vcf: [0.50, 0.40, 0.52, 0.64, 0.12], filt: (3, 4),
        feg: [0.02, 0.52, 0.30, 0.36], veg: [0.03, 0.68, 0.84, 0.36],
        misc: (true, 0, 2), level: 0.56 },

    // ── Pluck and percussion ──
    Program { name: "Zap Pluck",
        kbd: (2, false, 0.30), lfo: [0.40, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.33, 0.90, 0.500, 0.33, 0.40],
        vcf: [0.22, 0.60, 0.40, 0.92, 0.10], filt: (4, 6),
        feg: [0.00, 0.32, 0.00, 0.18], veg: [0.00, 0.42, 0.00, 0.20],
        misc: (true, 2, 2), level: 0.90 },
    Program { name: "Clav Pluck",
        kbd: (2, false, 0.30), lfo: [0.40, 0.00], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.94, 0.88, 0.500, 0.94, 0.42],
        vcf: [0.30, 0.50, 0.55, 0.78, 0.20], filt: (4, 7),
        feg: [0.00, 0.36, 0.00, 0.20], veg: [0.00, 0.46, 0.10, 0.22],
        misc: (true, 2, 2), level: 0.94 },
    Program { name: "Wood Block",
        kbd: (3, false, 0.30), lfo: [0.40, 0.00], md: [0, 0, 0, 0, 0],
        feet: (2, 3, false), osc: [0.00, 0.86, 0.612, 0.00, 0.70],
        vcf: [0.48, 0.72, 0.20, 0.70, 0.00], filt: (4, 5),
        feg: [0.00, 0.20, 0.00, 0.12], veg: [0.00, 0.26, 0.00, 0.14],
        misc: (true, 2, 2), level: 0.96 },
    Program { name: "Kick Drum",
        kbd: (1, false, 0.30), lfo: [0.30, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 0, false), osc: [0.00, 0.94, 0.500, 0.00, 0.00],
        vcf: [0.26, 0.22, 0.00, 0.90, 0.30], filt: (4, 0),
        feg: [0.00, 0.16, 0.00, 0.10], veg: [0.00, 0.30, 0.00, 0.14],
        misc: (true, 2, 2), level: 0.88 },
    Program { name: "Tom Hit",
        kbd: (1, false, 0.30), lfo: [0.30, 0.55], md: [4, 0, 0, 0, 0],
        feet: (0, 1, false), osc: [0.10, 0.88, 0.500, 0.00, 0.40],
        vcf: [0.40, 0.30, 0.15, 0.62, 0.05], filt: (4, 0),
        feg: [0.00, 0.18, 0.00, 0.12], veg: [0.00, 0.50, 0.00, 0.24],
        misc: (true, 2, 2), level: 0.84 },
    Program { name: "Blip",
        kbd: (3, false, 0.30), lfo: [0.40, 0.00], md: [0, 0, 0, 0, 0],
        feet: (3, 3, false), osc: [0.67, 0.84, 0.500, 0.67, 0.00],
        vcf: [0.62, 0.36, 0.60, 0.62, 0.00], filt: (4, 4),
        feg: [0.00, 0.16, 0.00, 0.10], veg: [0.00, 0.20, 0.00, 0.10],
        misc: (true, 2, 2), level: 1.00 },
    Program { name: "Marimba",
        kbd: (2, false, 0.30), lfo: [0.40, 0.00], md: [0, 0, 0, 0, 0],
        feet: (2, 2, false), osc: [0.00, 0.92, 0.500, 0.00, 0.30],
        vcf: [0.52, 0.24, 0.62, 0.66, 0.00], filt: (4, 5),
        feg: [0.00, 0.42, 0.00, 0.26], veg: [0.00, 0.52, 0.00, 0.28],
        misc: (true, 2, 2), level: 0.96 },
    Program { name: "Snap Bass",
        kbd: (2, false, 0.16), lfo: [0.44, 0.00], md: [0, 0, 0, 0, 0],
        feet: (0, 1, false), osc: [0.33, 0.90, 0.500, 0.90, 0.34],
        vcf: [0.20, 0.66, 0.42, 0.90, 0.22], filt: (4, 8),
        feg: [0.00, 0.28, 0.00, 0.16], veg: [0.00, 0.50, 0.60, 0.24],
        misc: (true, 2, 2), level: 0.70 },
    Program { name: "Dry Tick",
        kbd: (3, false, 0.30), lfo: [0.40, 0.00], md: [0, 0, 0, 0, 0],
        feet: (3, 3, false), osc: [1.00, 0.80, 0.560, 1.00, 0.80],
        vcf: [0.70, 0.10, 0.50, 0.54, 0.00], filt: (2, 3),
        feg: [0.00, 0.12, 0.00, 0.08], veg: [0.00, 0.14, 0.00, 0.08],
        misc: (true, 2, 2), level: 1.00 },
    Program { name: "Bell Pluck",
        kbd: (3, false, 0.30), lfo: [0.40, 0.00], md: [0, 0, 0, 0, 0],
        feet: (2, 3, true), osc: [0.00, 0.06, 0.740, 0.00, 0.92],
        vcf: [0.66, 0.40, 0.55, 0.68, 0.00], filt: (4, 4),
        feg: [0.00, 0.38, 0.00, 0.34], veg: [0.00, 0.56, 0.00, 0.40],
        misc: (true, 2, 2), level: 0.86 },

    // ── Drone ──
    Program { name: "Self Osc",
        kbd: (2, false, 0.30), lfo: [0.14, 0.30], md: [0, 1, 0, 0, 0],
        feet: (1, 1, false), osc: [0.00, 0.00, 0.500, 0.00, 0.00],
        vcf: [0.50, 1.00, 0.85, 0.56, 0.00], filt: (4, 0),
        feg: [0.30, 0.60, 0.60, 0.60], veg: [0.30, 0.80, 0.94, 0.64],
        misc: (true, 0, 2), level: 0.68 },
    Program { name: "Slow Swell",
        kbd: (2, false, 0.30), lfo: [0.16, 0.16], md: [0, 1, 0, 0, 0],
        feet: (1, 0, false), osc: [0.33, 0.72, 0.530, 0.33, 0.72],
        vcf: [0.34, 0.28, 0.40, 0.72, 0.00], filt: (4, 0),
        feg: [0.72, 0.80, 0.70, 0.70], veg: [0.70, 0.88, 0.96, 0.74],
        misc: (true, 0, 2), level: 0.54 },
    Program { name: "Held Tone",
        kbd: (2, false, 0.30), lfo: [0.30, 0.04], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.67, 0.86, 0.500, 0.67, 0.00],
        vcf: [0.48, 0.12, 0.50, 0.50, 0.00], filt: (4, 0),
        feg: [0.20, 0.50, 0.70, 0.50], veg: [0.20, 0.70, 1.00, 0.52],
        misc: (true, 0, 2), level: 0.72 },
    Program { name: "Deep Drone",
        kbd: (0, false, 0.30), lfo: [0.10, 0.24], md: [0, 1, 0, 0, 0],
        feet: (0, 0, false), osc: [0.06, 0.88, 0.504, 0.06, 0.88],
        vcf: [0.22, 0.36, 0.20, 0.56, 0.10], filt: (4, 0),
        feg: [0.60, 0.78, 0.70, 0.68], veg: [0.56, 0.90, 0.98, 0.76],
        misc: (true, 0, 2), level: 0.58 },
    Program { name: "Air Pad",
        kbd: (2, false, 0.30), lfo: [0.24, 0.18], md: [0, 2, 2, 0, 0],
        feet: (1, 2, false), osc: [0.78, 0.70, 0.538, 0.78, 0.70],
        vcf: [0.52, 0.20, 0.55, 0.54, 0.00], filt: (4, 0),
        feg: [0.66, 0.76, 0.66, 0.68], veg: [0.64, 0.88, 0.96, 0.74],
        misc: (true, 0, 2), level: 0.66 },
    Program { name: "Ghost Pad",
        kbd: (2, false, 0.30), lfo: [0.12, 0.46], md: [0, 1, 0, 0, 0],
        feet: (1, 1, false), osc: [0.00, 0.62, 0.548, 0.86, 0.52],
        vcf: [0.38, 0.68, 0.42, 0.54, 0.00], filt: (3, 0),
        feg: [0.62, 0.78, 0.62, 0.70], veg: [0.60, 0.90, 0.94, 0.78],
        misc: (true, 0, 2), level: 0.66 },
    Program { name: "Filter Wash",
        kbd: (2, false, 0.30), lfo: [0.06, 0.66], md: [2, 1, 0, 0, 0],
        feet: (1, 0, false), osc: [0.33, 0.80, 0.500, 0.33, 0.80],
        vcf: [0.34, 0.74, 0.30, 0.52, 0.14], filt: (4, 0),
        feg: [0.50, 0.72, 0.66, 0.66], veg: [0.48, 0.86, 0.96, 0.72],
        misc: (true, 0, 2), level: 0.50 },
    Program { name: "Long Fifth",
        kbd: (2, false, 0.30), lfo: [0.20, 0.10], md: [0, 0, 0, 0, 0],
        feet: (1, 1, false), osc: [0.33, 0.78, 1.000, 0.00, 0.78],
        vcf: [0.44, 0.22, 0.48, 0.58, 0.00], filt: (4, 0),
        feg: [0.54, 0.70, 0.64, 0.66], veg: [0.52, 0.86, 0.96, 0.72],
        misc: (true, 0, 2), level: 0.60 },
    Program { name: "Choir Mono",
        kbd: (2, false, 0.30), lfo: [0.26, 0.22], md: [0, 2, 0, 0, 0],
        feet: (1, 1, false), osc: [0.72, 0.74, 0.518, 0.66, 0.74],
        vcf: [0.42, 0.56, 0.44, 0.58, 0.00], filt: (4, 0),
        feg: [0.58, 0.72, 0.62, 0.66], veg: [0.56, 0.86, 0.94, 0.72],
        misc: (true, 0, 2), level: 0.60 },
    Program { name: "Night Hum",
        kbd: (1, false, 0.30), lfo: [0.02, 0.34], md: [0, 1, 0, 0, 0],
        feet: (0, 0, false), osc: [0.00, 0.84, 0.492, 0.00, 0.84],
        vcf: [0.18, 0.48, 0.16, 0.54, 0.00], filt: (4, 0),
        feg: [0.80, 0.86, 0.76, 0.76], veg: [0.76, 0.92, 0.98, 0.82],
        misc: (true, 0, 2), level: 0.60 },
];

/// Patch 0, "Taurus Deep", which is the panel the instrument loads with.
/// [`patch_zero_is_the_default_parameter_block`] holds these and the first row
/// of [`BANK`] together.
pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    0.005,       // patch: 00 Taurus Deep
    0.25,        // glide: off
    0.5,         // octave: 0
    0.5,         // fine: centre
    0.214_285_7, // pitch bend up: +2
    0.214_285_7, // pitch bend down: -2
    0.35,        // lfo rate: 3.1 Hz
    0.0,         // mod amount
    0.083_333_3, // source: LFO triangle
    0.125,       // destination: pitch
    0.1,         // destination 2: off
    0.25,        // mod source 5: filter EG
    0.25,        // mod source 6: osc 2
    0.125,       // osc 1 octave: 16'
    0.33,        // osc 1 wave: sawtooth
    0.85,        // osc 1 level
    0.30,        // glide rate
    0.25,        // 1-2 sync: off
    0.125,       // osc 2 octave: 16'
    0.5,         // osc 2 frequency: unison
    0.10,        // osc 2 wave: near triangle
    0.55,        // osc 2 level
    0.30,        // cutoff: 137 Hz
    0.20,        // resonance
    0.35,        // keyboard amount
    0.72,        // envelope amount: positive
    0.10,        // overload
    0.875,       // poles: 4
    0.676_470_6, // velocity sensitivity: +3
    0.00,        // filter attack
    0.62,        // filter decay
    0.20,        // filter sustain
    0.45,        // filter release
    0.02,        // volume attack
    0.70,        // volume decay
    0.85,        // volume sustain
    0.40,        // volume release
    0.75,        // egr release: on
    0.166_666_7, // gate: legato on
    0.833_333_3, // priority: last note
    0.72,        // volume
];

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 44_100.0;

    fn note_on(note: u8, velocity: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x90, data1: note, data2: velocity }
    }
    fn note_off(note: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x80, data1: note, data2: 0 }
    }
    fn cc(number: u8, value: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0xB0, data1: number, data2: value }
    }
    /// A pitch wheel message, from -1 (hard down) to +1 (hard up).
    fn bend(position: f64, offset: u32) -> MidiEvent {
        let raw = (8_192.0 + position.clamp(-1.0, 1.0) * 8_191.0) as u16;
        MidiEvent {
            sample_offset: offset,
            status: 0xE0,
            data1: (raw & 0x7F) as u8,
            data2: (raw >> 7) as u8,
        }
    }

    /// Render `blocks` buffers of 64 samples, delivering `events` in the
    /// first.
    fn render(synth: &mut LittlePhatty, events: &[MidiEvent], blocks: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(blocks * 64);
        let mut buf = [0.0f32; 64];
        for block in 0..blocks {
            buf.fill(0.0);
            let mut outs: [&mut [f32]; 1] = [&mut buf];
            if block == 0 {
                synth.process(&[], &mut outs, events);
            } else {
                synth.process(&[], &mut outs, &[]);
            }
            out.extend_from_slice(&buf);
        }
        out
    }

    fn fresh(patch: usize) -> LittlePhatty {
        let mut s = LittlePhatty::new();
        s.init(SR, 64);
        s.set_parameter(P_PATCH, patch_knob(patch));
        s
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    /// RMS over a 4096-sample window, which is the window this project
    /// measures with: shorter windows alias against the thing being measured
    /// and a peak is not monotonic in anything.
    fn window_rms(x: &[f32]) -> Vec<f64> {
        x.chunks(4096)
            .filter(|c| c.len() == 4096)
            .map(|c| (c.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / 4096.0).sqrt())
            .collect()
    }

    fn rms(x: &[f32]) -> f64 {
        if x.is_empty() {
            return 0.0;
        }
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64).sqrt()
    }

    // ── The oscillator ──

    /// The trapezoid on its own, with the band-limiting on.
    fn osc_corrected(freq: f64, sr: f64, wave: f64, samples: usize) -> Vec<f64> {
        let shape = Shape::at(wave);
        let dt = freq / sr;
        let mut osc = Trapezoid::new();
        (0..samples).map(|_| osc.tick(dt, &shape, None)).collect()
    }

    /// The same waveform sampled straight off the shape, which is what the
    /// oscillator would be without the corner corrections.
    fn osc_naive(freq: f64, sr: f64, wave: f64, samples: usize) -> Vec<f64> {
        let shape = Shape::at(wave);
        let dt = freq / sr;
        (0..samples).map(|i| shape.value(wrap_phase(i as f64 * dt))).collect()
    }

    /// Magnitude spectrum of a Hann-windowed block, one bin per index.
    fn spectrum(x: &[f64]) -> Vec<f64> {
        let n = x.len();
        let windowed: Vec<f64> = x
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let w = 0.5
                    - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
                v * w
            })
            .collect();
        (0..n / 2)
            .map(|k| {
                let (mut re, mut im) = (0.0, 0.0);
                for (i, v) in windowed.iter().enumerate() {
                    let angle = std::f64::consts::TAU * k as f64 * i as f64 / n as f64;
                    re += v * angle.cos();
                    im -= v * angle.sin();
                }
                (re * re + im * im).sqrt()
            })
            .collect()
    }

    /// The share of a spectrum's energy that is *not* within two bins of a
    /// harmonic of `f0` — that is, the aliasing plus the window's own leakage.
    fn alias_share(spectrum: &[f64], f0: f64, sr: f64) -> f64 {
        let bins = spectrum.len() * 2;
        let bin_hz = sr / bins as f64;
        let mut total = 0.0;
        let mut off = 0.0;
        for (k, magnitude) in spectrum.iter().enumerate() {
            let energy = magnitude * magnitude;
            total += energy;
            let hz = k as f64 * bin_hz;
            let harmonic = (hz / f0).round().max(1.0);
            if (hz - harmonic * f0).abs() > 2.5 * bin_hz && k > 2 {
                off += energy;
            }
        }
        off / total.max(1e-30)
    }

    /// The corner corrections have to *reduce* the energy that is not on a
    /// harmonic, or they are the wrong sign — which is the one thing a
    /// hand-derived polyBLAMP can silently be.
    ///
    /// Measured at 44.1 kHz on a 1479 Hz note (MIDI 90 at 8'), which puts the
    /// fourteenth harmonic past Nyquist and gives aliasing plenty to do.
    #[test]
    fn band_limiting_removes_what_it_is_supposed_to() {
        let f0 = 1_479.98;
        for (wave, name) in [(1.0 / 3.0, "sawtooth"), (2.0 / 3.0, "square"), (1.0, "pulse")] {
            let corrected = alias_share(&spectrum(&osc_corrected(f0, SR, wave, 4096)), f0, SR);
            let naive = alias_share(&spectrum(&osc_naive(f0, SR, wave, 4096)), f0, SR);
            assert!(
                corrected < naive * 0.5,
                "{name}: correction left {corrected:.5} of the energy off-harmonic \
                 where the naive shape has {naive:.5}"
            );
            assert!(
                corrected < 0.01,
                "{name}: {corrected:.5} of the energy is not on a harmonic"
            );
        }
    }

    /// A triangle is odd harmonics falling as 1/n²; a sawtooth is every
    /// harmonic falling as 1/n. Sweeping the wave knob from one to the other
    /// has to *move* between those, monotonically, rather than switch.
    ///
    /// The number measured is the second harmonic against the first, which is
    /// exactly zero for a symmetric triangle and 1/2 for a sawtooth, and the
    /// assertion is that it never steps backwards along the sweep.
    /// The second harmonic against the first, at one wave-knob position: zero
    /// for a symmetric triangle, a half for a sawtooth.
    pub(super) fn second_harmonic_ratio(wave: f64) -> f64 {
        let f0 = 220.0;
        let s = spectrum(&osc_corrected(f0, SR, wave, 8192));
        let bin = |harmonic: f64| {
            let k = (harmonic * f0 * 8192.0 / SR).round() as usize;
            s[k]
        };
        bin(2.0) / bin(1.0)
    }

    #[test]
    fn the_wave_knob_morphs_rather_than_switches() {
        let mut previous = -1.0;
        let mut readings = Vec::new();
        for step in 0..=40 {
            let wave = f64::from(step) / 40.0 / 3.0; // triangle to sawtooth
            let ratio = second_harmonic_ratio(wave);
            assert!(
                ratio >= previous - 1.0e-4,
                "the second harmonic fell back at wave {wave:.4}: {ratio:.5} after {previous:.5}"
            );
            previous = ratio;
            readings.push(ratio);
        }
        // Both ends have to be where the shapes say they are, or "monotonic"
        // is satisfied by a knob that does nothing.
        assert!(readings[0] < 0.005, "the triangle has a second harmonic: {}", readings[0]);
        assert!(
            (readings[40] - 0.5).abs() < 0.03,
            "the sawtooth's second harmonic is {} of its first, not a half",
            readings[40]
        );
    }

    /// The same sweep over the whole travel, measured as spectral centroid:
    /// every position between two labelled shapes has to sit between them.
    #[test]
    fn every_wave_position_is_a_waveform_of_its_own() {
        let f0 = 220.0;
        let mut last = None;
        for step in 0..=60 {
            let wave = f64::from(step) / 60.0;
            let x = osc_corrected(f0, SR, wave, 4096);
            // Peak is 1 at every position, which a crossfade between shapes
            // would not manage.
            let top = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            assert!(
                (0.90..=1.001).contains(&top),
                "wave {wave:.3} peaks at {top:.4}"
            );
            let s = spectrum(&x);
            let bins = s.len() * 2;
            let (mut num, mut den) = (0.0, 0.0);
            for (k, magnitude) in s.iter().enumerate() {
                let energy = magnitude * magnitude;
                num += energy * (k as f64 * SR / bins as f64);
                den += energy;
            }
            let centroid = num / den.max(1e-30);
            assert!(centroid.is_finite() && centroid > 0.0, "wave {wave:.3} has no spectrum");
            if let Some(previous) = last {
                let previous: f64 = previous;
                let jump = (centroid / previous).max(previous / centroid);
                assert!(
                    jump < 1.6,
                    "the centroid jumped {jump:.2}x between wave {:.3} and {wave:.3} — \
                     that is a switch, not a morph",
                    wave - 1.0 / 60.0
                );
            }
            last = Some(centroid);
        }
    }

    /// The four labelled positions are the four shapes the legend names.
    #[test]
    fn the_legend_positions_are_the_shapes_they_name() {
        let triangle = Shape::at(0.0);
        assert!((triangle.rise - 0.5).abs() < 1e-9 && (triangle.fall - 0.5).abs() < 1e-9);
        assert_eq!(triangle.high, 0.0);

        let saw = Shape::at(1.0 / 3.0);
        assert!(saw.rise > 1.0 - 1e-5, "the sawtooth rises over {} of a cycle", saw.rise);
        assert!(saw.fall < 1e-5);

        let square = Shape::at(2.0 / 3.0);
        assert!((square.high - square.low).abs() < 1e-9, "the square is not symmetric");
        assert!((square.high - 0.5).abs() < 1e-5);

        let pulse = Shape::at(1.0);
        assert!((pulse.high - DUTY_MIN).abs() < 1e-9, "the pulse duty is {}", pulse.high);
        // Peak-normalised, so the thin pulse is bounded like everything else.
        assert!((pulse.value(pulse.rise * 0.5 + pulse.rise) - 1.0).abs() < 1e-9);
    }

    // ── The instrument ──

    #[test]
    fn silence_with_no_input() {
        let mut s = fresh(0);
        let out = render(&mut s, &[], 40);
        assert!(out.iter().all(|v| *v == 0.0), "peak {}", peak(&out));
    }

    #[test]
    fn a_note_before_init_is_silence_rather_than_a_panic() {
        // The host calls `init` before `process`, but the audio thread is not
        // the place to find out that something did not.
        let mut s = LittlePhatty::new();
        let mut out = vec![0.0f32; 64];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 0), cc(1, 64, 8)]);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = fresh(0);
        let out = render(&mut s, &[note_on(48, 100, 0)], 400);
        assert!(peak(&out) > 0.01, "peak {}", peak(&out));
    }

    #[test]
    fn silent_after_release() {
        let mut s = fresh(0);
        let _ = render(&mut s, &[note_on(48, 100, 0)], 100);
        let _ = render(&mut s, &[note_off(48, 0)], 600);
        let tail = render(&mut s, &[], 200);
        assert!(peak(&tail) < 1.0e-5, "tail peak {}", peak(&tail));
    }

    #[test]
    fn output_is_finite_across_the_keyboard() {
        for patch in [0, 30, 55, 80, 90] {
            let mut s = fresh(patch);
            for note in [0u8, 12, 36, 60, 84, 108, 127] {
                let out = render(&mut s, &[note_on(note, 127, 0)], 60);
                assert!(
                    out.iter().all(|v| v.is_finite()),
                    "{} went non-finite on note {note}",
                    PATCH_NAMES[patch]
                );
                let _ = render(&mut s, &[note_off(note, 0)], 20);
            }
        }
    }

    #[test]
    fn cc120_kills_and_cc123_releases() {
        let mut s = fresh(0);
        let _ = render(&mut s, &[note_on(48, 110, 0)], 60);
        let out = render(&mut s, &[cc(120, 0, 0)], 8);
        assert!(peak(&out[64..]) < 1.0e-6, "CC 120 left {}", peak(&out[64..]));

        let mut s = fresh(0);
        let _ = render(&mut s, &[note_on(48, 110, 0)], 60);
        let _ = render(&mut s, &[cc(123, 0, 0)], 600);
        let tail = render(&mut s, &[], 100);
        assert!(peak(&tail) < 1.0e-5, "CC 123 left {}", peak(&tail));
    }

    #[test]
    fn sample_accurate_midi() {
        // A note starting 40 samples into a 64-sample block leaves the first
        // 40 silent.
        let mut s = fresh(0);
        let mut buf = [0.0f32; 64];
        let mut outs: [&mut [f32]; 1] = [&mut buf];
        s.process(&[], &mut outs, &[note_on(48, 110, 40)]);
        assert!(buf[..40].iter().all(|v| *v == 0.0), "sound before the note-on");
        assert!(buf[40..].iter().any(|v| v.abs() > 0.0), "no sound after it");
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
        assert_eq!(s.get_parameter(PARAM_COUNT), 0.0);
        assert_eq!(s.parameter_count(), PARAM_COUNT);
    }

    // ── Rate independence ──

    /// Zero crossings per second of a held note, over `seconds` after the
    /// attack has settled. Hysteresis at a tenth of the peak, so that a
    /// wobble around zero counts once rather than three times.
    fn crossings_per_second(x: &[f32], sr: f64) -> f64 {
        let top = peak(x);
        let gate = top * 0.1;
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

    /// The repetition rate of a waveform, by autocorrelation.
    ///
    /// The best-correlating lag, then a check for a *sub*multiple of it that
    /// correlates nearly as well, so that a shape whose second half resembles
    /// its first is not read an octave down; then parabolic interpolation
    /// across the peak, because an integer lag at 440 Hz is only good to about
    /// one percent and this is used to check tuning.
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
        // Every multiple of the period correlates as well as the period does,
        // so the answer is the *first* peak that correlates nearly as well as
        // the best one — and it has to be a peak rather than merely a lag
        // above a threshold, because the autocorrelation of something close to
        // a sine is broad enough that a fixed threshold is crossed several
        // percent early.
        let best = scores[at];
        for i in 1..at {
            if scores[i] >= scores[i - 1] && scores[i] > scores[i + 1] && scores[i] > best * 0.8 {
                at = i;
                break;
            }
        }
        // A parabola through the peak and its two neighbours.
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

    /// A held note has to have the same pitch at every sample rate.
    ///
    /// This test exists because the project shipped an engine that ran 8.84%
    /// sharp for months: every test in the suite ran at 44100, so nothing
    /// noticed that the pitch was a function of the sample rate. It measures
    /// zero crossings rather than a spectrum because a crossing count cannot
    /// be fooled by a window.
    #[test]
    fn the_pitch_is_the_same_at_every_sample_rate() {
        for sr in [22_050.0, 48_000.0, 96_000.0] {
            let measured = crossings_at_rate(sr);
            let reference = crossings_at_rate(44_100.0);
            let error = (measured / reference - 1.0).abs();
            assert!(
                error < 0.02,
                "at {sr} Hz the note runs at {measured:.2} crossings a second \
                 against {reference:.2} at 44100 — {:.2}% out",
                error * 100.0
            );
        }
        // A2 at 8' is 110 Hz, so two crossings a cycle is 220 a second.
        let reference = crossings_at_rate(44_100.0);
        assert!(
            (reference - 220.0).abs() < 4.0,
            "A2 measures {reference:.2} crossings a second, not 220"
        );
    }

    /// A held A2 at 8', through a plain panel with nothing modulating and the
    /// filter well below the second harmonic, as zero crossings a second.
    pub(super) fn crossings_at_rate(sr: f64) -> f64 {
        let mut s = LittlePhatty::new();
        s.init(sr, 64);
        // A plain panel: one sawtooth at 8', the filter well below the second
        // harmonic so the count is the fundamental's, nothing modulating,
        // nothing gliding.
        for (index, value) in [
            (P_O1_OCT, 0.375),
            (P_O1_WAVE, 0.33),
            (P_O1_LEVEL, 0.9),
            (P_O2_LEVEL, 0.0),
            (P_CUTOFF, 0.40),
            (P_RESO, 0.0),
            (P_KB_AMT, 0.0),
            (P_EG_AMT, 0.5),
            (P_OVERLOAD, 0.0),
            (P_MOD_AMT, 0.0),
            (P_GLIDE, 0.25),
            (P_VEL_SENS, knob_for(8, 17)),
            (P_V_ATTACK, 0.0),
            (P_V_DECAY, 0.0),
            (P_V_SUSTAIN, 1.0),
            (P_VOLUME, 0.8),
            (P_OCTAVE, 0.5),
            (P_FINE, 0.5),
        ] {
            s.set_parameter(index, value);
        }
        let blocks = (sr * 2.5 / 64.0) as usize;
        let out = render(&mut s, &[note_on(45, 100, 0)], blocks);
        // Skip the first half second so the envelope and the DC blocker have
        // settled.
        let skip = (sr * 0.5) as usize;
        crossings_per_second(&out[skip..], sr)
    }

    /// The pitch a note sounds, end to end, through whichever octave switch,
    /// transpose and tuning the panel is set to.
    fn sounding_hz(setup: &[(usize, f32)], note: u8) -> f64 {
        let mut s = LittlePhatty::new();
        s.init(SR, 64);
        for (index, value) in [
            (P_O1_OCT, knob_for(1, 4)),
            (P_O1_WAVE, 1.0 / 3.0),
            (P_O1_LEVEL, 0.9),
            (P_O2_LEVEL, 0.0),
            (P_CUTOFF, 0.85),
            (P_RESO, 0.0),
            (P_KB_AMT, 0.0),
            (P_EG_AMT, 0.5),
            (P_OVERLOAD, 0.0),
            (P_MOD_AMT, 0.0),
            (P_V_ATTACK, 0.0),
            (P_V_SUSTAIN, 1.0),
            (P_VOLUME, 0.8),
            (P_OCTAVE, 0.5),
            (P_FINE, 0.5),
        ]
        .iter()
        .chain(setup.iter())
        {
            s.set_parameter(*index, *value);
        }
        let out = render(&mut s, &[note_on(note, 100, 0)], 800);
        // Wide enough to cover 2' two octaves up and 16' two octaves down,
        // which is what this test moves the pitch across.
        fundamental_hz(&out[16384..32768], SR, 20.0, 4_000.0)
    }

    /// A440 is on note 69 at 8', and every switch that moves the pitch moves
    /// it by the interval printed next to it.
    #[test]
    fn the_octave_switches_move_by_the_intervals_they_name() {
        let concert = sounding_hz(&[], 69);
        assert!(
            (concert - 440.0).abs() < 2.0,
            "note 69 at 8' sounds {concert:.2} Hz rather than 440"
        );

        // "The panel markings 16', 8', 4' and 2' are octave standards based on
        // organ stops" (manual page 11): an octave a step.
        for (position, ratio) in [(0usize, 0.5f64), (1, 1.0), (2, 2.0), (3, 4.0)] {
            let hz = sounding_hz(&[(P_O1_OCT, knob_for(position, 4))], 69);
            assert!(
                (hz / (440.0 * ratio) - 1.0).abs() < 0.01,
                "{} sounds {hz:.2} Hz where {:.2} was asked for",
                ["16'", "8'", "4'", "2'"][position],
                440.0 * ratio
            );
        }

        // "The range is -2, -1, 0, +1, +2" (page 21).
        for (position, ratio) in [(0usize, 0.25f64), (2, 1.0), (4, 4.0)] {
            let hz = sounding_hz(&[(P_OCTAVE, knob_for(position, 5))], 69);
            assert!(
                (hz / (440.0 * ratio) - 1.0).abs() < 0.01,
                "octave transpose {} sounds {hz:.2} Hz where {:.2} was asked for",
                position as i32 - 2,
                440.0 * ratio
            );
        }

        // "OSC 2 FREQ... up or down 7 semitones (a fifth)" (page 12), and
        // "FINE TUNE... ±3 semitones" (page 21).
        for (knob, semitones) in [(0.0f32, -7.0f64), (0.5, 0.0), (1.0, 7.0)] {
            let hz = sounding_hz(
                &[(P_O1_LEVEL, 0.0), (P_O2_LEVEL, 0.9), (P_O2_WAVE, 1.0 / 3.0),
                  (P_O2_OCT, knob_for(1, 4)), (P_O2_FREQ, knob)],
                69,
            );
            let want = 440.0 * (semitones / 12.0).exp2();
            assert!(
                (hz / want - 1.0).abs() < 0.01,
                "OSC 2 FREQ at {knob} sounds {hz:.2} Hz where {want:.2} was asked for"
            );
        }
        for (knob, semitones) in [(0.0f32, -3.0f64), (1.0, 3.0)] {
            let hz = sounding_hz(&[(P_FINE, knob)], 69);
            let want = 440.0 * (semitones / 12.0).exp2();
            assert!(
                (hz / want - 1.0).abs() < 0.01,
                "FINE TUNE at {knob} sounds {hz:.2} Hz where {want:.2} was asked for"
            );
        }

        // ...and the keyboard is equal-tempered, an octave for twelve keys.
        let low = sounding_hz(&[], 45);
        let high = sounding_hz(&[], 57);
        assert!(
            (high / low - 2.0).abs() < 0.02,
            "twelve keys is {:.4} of an octave",
            (high / low).log2()
        );
    }

    /// The pitch wheel bends by the interval the PITCH BEND menu names, and
    /// the two directions are set independently, as the menu says.
    #[test]
    fn the_pitch_wheel_bends_by_the_range_it_names() {
        fn bent(up: usize, down: usize, position: f64) -> f64 {
            let mut s = LittlePhatty::new();
            s.init(SR, 64);
            for (index, value) in [
                (P_O1_OCT, knob_for(1, 4)),
                (P_O1_WAVE, 1.0 / 3.0),
                (P_O1_LEVEL, 0.9),
                (P_O2_LEVEL, 0.0),
                (P_CUTOFF, 0.85),
                (P_RESO, 0.0),
                (P_KB_AMT, 0.0),
                (P_EG_AMT, 0.5),
                (P_OVERLOAD, 0.0),
                (P_MOD_AMT, 0.0),
                (P_V_ATTACK, 0.0),
                (P_V_SUSTAIN, 1.0),
                (P_VOLUME, 0.8),
                (P_BEND_UP, knob_for(up, BEND_UP.len())),
                (P_BEND_DOWN, knob_for(down, BEND_DOWN.len())),
            ] {
                s.set_parameter(index, value);
            }
            let mut out = render(&mut s, &[note_on(69, 100, 0)], 200);
            out.extend(render(&mut s, &[bend(position, 0)], 600));
            fundamental_hz(&out[out.len() - 16384..], SR, 20.0, 4_000.0)
        }

        // Up: 0, +2, +3, +4, +5, +7, +12.
        for (position, semitones) in BEND_UP.iter().enumerate() {
            let hz = bent(position, 1, 1.0);
            let want = 440.0 * (semitones / 12.0).exp2();
            assert!(
                (hz / want - 1.0).abs() < 0.01,
                "the wheel hard up with a range of {semitones} sounds {hz:.2} Hz \
                 where {want:.2} was asked for"
            );
        }
        // ...and down, independently.
        for (position, semitones) in BEND_DOWN.iter().enumerate() {
            let hz = bent(1, position, -1.0);
            let want = 440.0 * (semitones / 12.0).exp2();
            assert!(
                (hz / want - 1.0).abs() < 0.01,
                "the wheel hard down with a range of {semitones} sounds {hz:.2} Hz \
                 where {want:.2} was asked for"
            );
        }
        // Centred is centred, and it is where a fresh instrument starts.
        assert!((bent(6, 6, 0.0) - 440.0).abs() < 2.0);
    }

    /// The envelope has to take the time the knob says at every rate too, or
    /// a session opened on a different device plays different articulations.
    #[test]
    fn the_envelope_takes_its_time_at_every_sample_rate() {
        fn decay_seconds(sr: f64) -> f64 {
            let mut s = LittlePhatty::new();
            s.init(sr, 64);
            for (index, value) in [
                (P_O1_WAVE, 0.0), (P_O1_LEVEL, 0.9), (P_O2_LEVEL, 0.0),
                (P_CUTOFF, 0.8), (P_RESO, 0.0), (P_KB_AMT, 0.0), (P_EG_AMT, 0.5),
                (P_OVERLOAD, 0.0), (P_MOD_AMT, 0.0),
                (P_V_ATTACK, 0.0), (P_V_DECAY, 0.75), (P_V_SUSTAIN, 0.0),
                (P_VOLUME, 0.9),
            ] {
                s.set_parameter(index, value);
            }
            let blocks = (sr * 2.0 / 64.0) as usize;
            let out = render(&mut s, &[note_on(60, 100, 0)], blocks);
            // Windowed rms rather than a sample threshold: a window shorter
            // than a cycle can sit entirely near a zero crossing and report a
            // decay that has not happened. Twenty milliseconds of window at
            // every rate, so the resolution is a time rather than a count.
            let window = (sr * 0.02) as usize;
            let levels: Vec<f64> = out
                .chunks(window)
                .filter(|c| c.len() == window)
                .map(|c| {
                    (c.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>()
                        / window as f64)
                        .sqrt()
                })
                .collect();
            let start = levels[0].max(levels[1]);
            let fell = levels.iter().position(|v| *v < start * 0.1).unwrap_or(levels.len());
            fell as f64 * 0.02
        }

        // A DECAY knob at 0.75 is a one-second segment, and the segment runs
        // 3.5 time constants, so 20 dB down arrives at 2.06 of them — 0.59 s.
        let reference = decay_seconds(44_100.0);
        assert!(
            (0.50..0.70).contains(&reference),
            "a one-second decay was 20 dB down after {reference:.3} s"
        );
        for sr in [22_050.0, 48_000.0, 96_000.0] {
            let measured = decay_seconds(sr);
            assert!(
                (measured / reference - 1.0).abs() < 0.03,
                "at {sr} Hz the decay is {measured:.4} s against {reference:.4} at 44100"
            );
        }
    }

    // ── Output safety ──

    #[test]
    fn every_patch_speaks_and_stays_finite() {
        for (index, name) in PATCH_NAMES.iter().enumerate() {
            let mut s = fresh(index);
            let out = render(&mut s, &[note_on(45, 110, 0)], 700);
            assert!(out.iter().all(|v| v.is_finite()), "{name} went non-finite");
            assert!(peak(&out) > 0.002, "{name} is silent: peak {}", peak(&out));
        }
    }

    /// Every patch, on the loudest thing a player can do to it, under the
    /// master limiter's ceiling.
    #[test]
    fn the_bank_stays_under_the_ceiling() {
        /// The master limiter's ceiling, −1 dBFS. Repeated rather than
        /// imported because phosphor-dsp does not depend on phosphor-core.
        const CEILING: f32 = 0.891;
        // The whole keyboard, not three notes of it. Three notes is what this
        // test used to check, and it missed four patches that peaked at 0.93
        // in the top octave — see `Trapezoid::closing` for what was wrong and
        // `wave_modulation_does_not_unbalance_the_corner_pairs` for the
        // regression that pins it.
        let mut worst = (0.0f32, "", 0u8);
        for (index, name) in PATCH_NAMES.iter().enumerate() {
            for note in (12u8..=108).step_by(3) {
                let mut s = fresh(index);
                let out = render(&mut s, &[note_on(note, 127, 0)], 200);
                let p = peak(&out);
                if p > worst.0 {
                    worst = (p, name, note);
                }
                assert!(p < CEILING, "{name} peaks at {p:.4} on note {note}");
            }
        }
        // ...and the saturator is never asked to do anything, so every patch
        // in the bank is the trimmed voice sample for sample.
        assert!(
            worst.0 < crate::level::SATURATION_KNEE,
            "{} on note {} reaches the saturator at {:.4}",
            worst.1,
            worst.2,
            worst.0
        );
    }

    /// The worst panel a hand can reach, which is not a patch: both
    /// oscillators at full into 16', the overload and the resonance hard over,
    /// velocity 127, every pole setting, every waveform.
    #[test]
    fn the_worst_panel_a_hand_can_reach_stays_under_the_ceiling() {
        const CEILING: f32 = 0.891;
        let mut worst = (0.0f32, String::new());
        for poles in 0..4 {
            for overload in [0.0f32, 0.5, 1.0] {
                for resonance in [0.0f32, 0.7, 1.0] {
                    for cutoff in [0.3f32, 0.6, 1.0] {
                        for wave in [0.0f32, 1.0 / 3.0, 2.0 / 3.0, 1.0] {
                            let mut s = LittlePhatty::new();
                            s.init(SR, 64);
                            for (index, value) in [
                                (P_O1_LEVEL, 1.0),
                                (P_O2_LEVEL, 1.0),
                                (P_O1_OCT, knob_for(0, 4)),
                                (P_O2_OCT, knob_for(0, 4)),
                                (P_CUTOFF, cutoff),
                                (P_RESO, resonance),
                                (P_OVERLOAD, overload),
                                (P_POLES, knob_for(poles, 4)),
                                (P_VOLUME, 1.0),
                                (P_V_ATTACK, 0.0),
                                (P_V_DECAY, 0.0),
                                (P_V_SUSTAIN, 1.0),
                                (P_F_SUSTAIN, 1.0),
                                (P_EG_AMT, 1.0),
                                (P_VEL_SENS, 1.0),
                                (P_KB_AMT, 1.0),
                                (P_O1_WAVE, wave),
                                (P_O2_WAVE, wave),
                            ] {
                                s.set_parameter(index, value);
                            }
                            let mut top = 0.0f32;
                            let mut finite = true;
                            for note in [24u8, 60, 99] {
                                for (modulation, rate) in [(0.0f32, 0.3f32), (1.0, 0.55)] {
                                    // With the mod bus hard over on WAVE as
                                    // well as dry, because the wave control is
                                    // the one thing here that can change the
                                    // oscillator's shape between samples.
                                    s.set_parameter(P_MOD_AMT, modulation);
                                    s.set_parameter(P_LFO_RATE, rate);
                                    s.set_parameter(P_MOD_DEST, knob_for(2, 4));
                                    s.reset();
                                    let out = render(&mut s, &[note_on(note, 127, 0)], 200);
                                    finite &= out.iter().all(|v| v.is_finite());
                                    top = top.max(peak(&out));
                                }
                            }
                            let p = top;
                            assert!(finite);
                            if p > worst.0 {
                                worst = (
                                    p,
                                    format!(
                                        "{} poles, overload {overload}, resonance \
                                         {resonance}, cutoff {cutoff}, wave {wave:.2}",
                                        poles + 1
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(
            worst.0 < CEILING,
            "the worst panel peaks at {:.4} — {}",
            worst.0,
            worst.1
        );
    }

    // ── Voice management ──

    /// The pitch the instrument is sounding, read off its own state.
    fn sounding(s: &LittlePhatty) -> u8 {
        s.current_note
    }

    #[test]
    fn last_note_priority_sounds_the_newest_key() {
        let mut s = fresh(0);
        let _ = render(&mut s, &[note_on(48, 100, 0)], 4);
        assert_eq!(sounding(&s), 48);
        let _ = render(&mut s, &[note_on(41, 100, 0)], 4);
        assert_eq!(sounding(&s), 41, "a lower key played later has to win");
        let _ = render(&mut s, &[note_on(55, 100, 0)], 4);
        assert_eq!(sounding(&s), 55, "a higher key played later has to win");
    }

    #[test]
    fn low_and_high_priority_pick_their_own_key() {
        for (position, expected) in [(0usize, 41u8), (1, 55)] {
            let mut s = fresh(0);
            s.set_parameter(P_PRIORITY, knob_for(position, 3));
            let _ = render(&mut s, &[note_on(48, 100, 0)], 4);
            let _ = render(&mut s, &[note_on(41, 100, 0)], 4);
            let _ = render(&mut s, &[note_on(55, 100, 0)], 4);
            assert_eq!(
                sounding(&s),
                expected,
                "{} priority picked the wrong key",
                discrete_label(P_PRIORITY, knob_for(position, 3)).unwrap()
            );
        }
    }

    #[test]
    fn letting_go_returns_to_the_held_note() {
        let mut s = fresh(0);
        let _ = render(&mut s, &[note_on(48, 100, 0)], 4);
        let _ = render(&mut s, &[note_on(55, 100, 0)], 4);
        assert_eq!(sounding(&s), 55);
        let _ = render(&mut s, &[note_off(55, 0)], 4);
        assert_eq!(sounding(&s), 48, "releasing the newer key has to return to the older");
        // ...and the voice is still sounding, rather than having been released.
        let out = render(&mut s, &[], 200);
        assert!(peak(&out) > 0.002, "the held note went silent: {}", peak(&out));
        // Letting the last one go does end the note.
        let _ = render(&mut s, &[note_off(48, 0)], 800);
        let tail = render(&mut s, &[], 200);
        assert!(peak(&tail) < 1.0e-5, "the note kept sounding: {}", peak(&tail));
    }

    /// The envelope level, sampled after `blocks` of 64 samples.
    fn amp_level(s: &LittlePhatty) -> f64 {
        s.volume_env.level
    }

    #[test]
    fn the_gate_modes_do_what_the_manual_says() {
        // A patch with a long attack, so that "retriggered" and "not
        // retriggered" are far apart.
        fn play(mode: usize) -> (f64, f64) {
            let mut s = fresh(0);
            for (index, value) in [
                (P_V_ATTACK, 0.62f32), (P_V_DECAY, 0.9), (P_V_SUSTAIN, 1.0),
                (P_GATE, knob_for(mode, 3)),
            ] {
                s.set_parameter(index, value);
            }
            let _ = render(&mut s, &[note_on(48, 100, 0)], 200);
            let before = amp_level(&s);
            let _ = render(&mut s, &[note_on(55, 100, 0)], 1);
            (before, amp_level(&s))
        }

        // LEG ON: "the envelopes aren't retriggered until the key is fully
        // released", so the level carries straight on.
        let (before, after) = play(0);
        assert!(before > 0.2, "the attack had not started: {before}");
        assert!(after >= before, "legato on retriggered: {before} -> {after}");

        // LEG OFF: "retrigger the envelope on a new note from the current EGR
        // level" — a new attack, but starting from where it was.
        let (before, after) = play(1);
        assert!(after > before * 0.5, "legato off restarted from zero: {before} -> {after}");

        // EGR RESET: "force the envelope generators to start from 0 volts".
        let (before, after) = play(2);
        assert!(
            after < before * 0.2,
            "the reset gate did not start from zero: {before} -> {after}"
        );
    }

    #[test]
    fn the_held_note_return_never_retriggers() {
        for mode in 0..3 {
            let mut s = fresh(0);
            for (index, value) in [
                (P_V_ATTACK, 0.62f32), (P_V_DECAY, 0.9), (P_V_SUSTAIN, 1.0),
                (P_GATE, knob_for(mode, 3)),
            ] {
                s.set_parameter(index, value);
            }
            let _ = render(&mut s, &[note_on(48, 100, 0)], 100);
            let _ = render(&mut s, &[note_on(55, 100, 0)], 100);
            let before = amp_level(&s);
            let _ = render(&mut s, &[note_off(55, 0)], 1);
            let after = amp_level(&s);
            assert!(
                after >= before,
                "gate mode {mode} retriggered on the way back to the held key: \
                 {before} -> {after}"
            );
        }
    }

    /// Velocity is on the filter and nowhere else, which is what the hardware
    /// does and most of why it feels the way it does.
    #[test]
    fn velocity_moves_the_filter_and_not_the_amplifier() {
        fn play(velocity: u8, sensitivity: i8) -> (f64, f64) {
            let mut s = fresh(0);
            for (index, value) in [
                (P_CUTOFF, 0.35f32), (P_EG_AMT, 0.5), (P_RESO, 0.0),
                (P_KB_AMT, 0.0), (P_O1_WAVE, 0.33), (P_O1_LEVEL, 0.9),
                (P_O2_LEVEL, 0.0), (P_V_ATTACK, 0.0), (P_V_SUSTAIN, 1.0),
                (P_VEL_SENS, knob_for((sensitivity + 8) as usize, 17)),
                (P_OVERLOAD, 0.0), (P_VOLUME, 0.8),
            ] {
                s.set_parameter(index, value);
            }
            let out = render(&mut s, &[note_on(45, velocity, 0)], 400);
            // Brightness, not level: the first difference is a one-pole high
            // pass, and what velocity does here is open the filter. The
            // fundamental dominates the plain rms whatever the cutoff, so an
            // rms comparison would report a filter that had barely moved.
            let slope: f64 = out[8192..]
                .windows(2)
                .map(|w| {
                    let d = f64::from(w[1]) - f64::from(w[0]);
                    d * d
                })
                .sum::<f64>()
                / (out.len() - 8193) as f64;
            (rms(&out[8192..]), slope.sqrt())
        }

        // Sensitivity at zero: velocity does nothing at all — not to the
        // amplifier, which is the point, and not to the filter either.
        let (soft_level, soft) = play(20, 0);
        let (hard_level, hard) = play(127, 0);
        assert!(
            (hard / soft - 1.0).abs() < 1.0e-6 && (hard_level / soft_level - 1.0).abs() < 1.0e-6,
            "velocity changed the sound with FILT SENS at 0: {soft} -> {hard}"
        );

        // Sensitivity up: a harder key opens the filter, which is a *timbre*
        // change — the level moves because the filter passes more, not because
        // the amplifier does.
        let (_, soft) = play(20, 6);
        let (_, hard) = play(127, 6);
        assert!(
            hard > soft * 1.5,
            "a hard key did not open the filter: {soft:.5} -> {hard:.5}"
        );
        // Negative sensitivity is the other way round, as the menu says.
        let (_, soft) = play(20, -6);
        let (_, hard) = play(127, -6);
        assert!(
            hard < soft * 0.7,
            "negative FILT SENS did not close the filter: {soft:.5} -> {hard:.5}"
        );
    }

    /// Glide is constant *rate*: the manual measures it as five seconds
    /// across the keyboard's three octaves, which is a figure that only means
    /// anything if a wider interval takes longer.
    #[test]
    fn glide_is_a_rate_rather_than_a_time() {
        fn glide_samples(from: u8, to: u8, rate: f32) -> usize {
            let mut s = fresh(0);
            s.set_parameter(P_GLIDE, knob_for(1, 2));
            s.set_parameter(P_GLIDE_RATE, rate);
            let _ = render(&mut s, &[note_on(from, 100, 0)], 4);
            let mut buf = [0.0f32; 64];
            for block in 0..8000 {
                buf.fill(0.0);
                let mut outs: [&mut [f32]; 1] = [&mut buf];
                if block == 0 {
                    s.process(&[], &mut outs, &[note_on(to, 100, 0)]);
                } else {
                    s.process(&[], &mut outs, &[]);
                }
                if (s.glide_note - f64::from(to)).abs() < 1.0e-9 {
                    return block * 64;
                }
            }
            usize::MAX
        }

        let octave = glide_samples(48, 60, 1.0);
        let two = glide_samples(48, 72, 1.0);
        let ratio = two as f64 / octave as f64;
        assert!(
            (ratio - 2.0).abs() < 0.05,
            "two octaves took {ratio:.3} times as long as one, not twice"
        );
        // The top of the knob is the manual's own figure: 36 semitones in
        // about five seconds.
        let three = glide_samples(48, 84, 1.0) as f64 / SR;
        assert!(
            (three - 5.0).abs() < 0.3,
            "three octaves of glide took {three:.2} s, not the five the manual measures"
        );
        // ...and the bottom is effectively instantaneous.
        assert!(
            glide_samples(48, 84, 0.0) < 1024,
            "the fastest glide takes {} samples",
            glide_samples(48, 84, 0.0)
        );
    }

    // ── The filter ──

    /// Magnitude response of the ladder at one frequency: drive it with a sine
    /// for half a second, then read the amplitude at the drive frequency with
    /// a single-bin transform over the next tenth.
    ///
    /// A peak reading is what this used to do, and it is wrong: the filter's
    /// own transient at the corner is louder than the steady-state response
    /// four octaves above it, so a peak reports the ringing rather than the
    /// slope. A single bin at the drive frequency cannot see the ringing at
    /// all.
    fn ladder_gain(cutoff: f64, resonance: f64, poles: usize, hz: f64) -> f64 {
        const DRIVE: f64 = 0.05;
        let mut f = Ladder::new();
        let settle = (SR * 0.5) as usize;
        let window = (SR * 0.2) as usize;
        let (mut re, mut im) = (0.0, 0.0);
        for i in 0..settle + window {
            let angle = std::f64::consts::TAU * hz * i as f64 / SR;
            let y = f.process(angle.sin() * DRIVE, cutoff, resonance, poles, SR);
            if i >= settle {
                re += y * angle.cos();
                im += y * angle.sin();
            }
        }
        2.0 * (re * re + im * im).sqrt() / window as f64 / DRIVE
    }

    /// One pole is 6 dB an octave, two is 12, three is 18 and four is 24 —
    /// which is the whole reason the switch is worth having.
    #[test]
    fn the_pole_switch_changes_the_slope() {
        let cutoff = 400.0;
        for (poles, expected) in [(1usize, 6.0f64), (2, 12.0), (3, 18.0), (4, 24.0)] {
            // Measured an octave apart, well above the corner where the
            // asymptote has taken over but below where the bilinear transform
            // starts pulling the response down towards Nyquist.
            let low = ladder_gain(cutoff, 0.0, poles, cutoff * 4.0);
            let high = ladder_gain(cutoff, 0.0, poles, cutoff * 8.0);
            let slope = 20.0 * (low / high).log10();
            assert!(
                (slope - expected).abs() < 2.0,
                "{poles} pole(s) roll off at {slope:.1} dB/octave, not {expected}"
            );
        }
    }

    /// The pole switch taps the ladder, so a shallower setting leaks more top
    /// end past the same corner — which is what a player hears — while the
    /// resonance peak stays at the corner, because the feedback still comes
    /// from the fourth section.
    #[test]
    fn a_shallower_slope_leaks_more_and_still_resonates() {
        let cutoff = 400.0;
        let mut previous = 0.0;
        for poles in [4usize, 3, 2, 1] {
            let leak = ladder_gain(cutoff, 0.0, poles, cutoff * 6.0);
            assert!(leak > previous, "{poles} poles leaked less than {} did", poles + 1);
            previous = leak;
        }
        for poles in 1..=4 {
            let at_corner = ladder_gain(cutoff, 0.85, poles, cutoff);
            let below = ladder_gain(cutoff, 0.85, poles, cutoff / 8.0);
            assert!(
                at_corner > below,
                "{poles} poles have no resonant peak: {at_corner:.3} at the corner \
                 against {below:.3} three octaves below"
            );
        }
    }

    /// Resonance peaks, then oscillates, and the ladder loses its bass on the
    /// way — which is the sound, and is why nothing compensates for it.
    #[test]
    fn resonance_peaks_and_then_oscillates() {
        let cutoff = 800.0;
        let mut peak_gain = 0.0;
        for resonance in [0.0, 0.3, 0.6, 0.85] {
            let at = ladder_gain(cutoff, resonance, 4, cutoff);
            assert!(at > peak_gain, "resonance {resonance} did not raise the peak");
            peak_gain = at;
            // ...and the passband drops as it goes: a ladder subtracts its
            // feedback from the signal, so what a real one loses is its bass.
            let bass = ladder_gain(cutoff, resonance, 4, 40.0);
            assert!(bass <= 1.01, "the passband gained at resonance {resonance}: {bass:.3}");
        }
        let quarter = ladder_gain(cutoff, 0.25, 4, 40.0);
        let top = ladder_gain(cutoff, 1.0, 4, 40.0);
        assert!(
            top < quarter * 0.4,
            "the ladder kept its bass: {quarter:.3} at a quarter turn, {top:.3} at the top"
        );

        // With nothing going in at all, the top of the travel produces.
        let mut f = Ladder::new();
        f.start(1.0);
        let mut tail = 0.0f64;
        for i in 0..(SR as usize * 2) {
            let y = f.process(0.0, cutoff, 1.0, 4, SR);
            if i > SR as usize {
                tail = tail.max(y.abs());
            }
        }
        assert!(tail > 0.05, "the filter does not self-oscillate: {tail:.4}");
        // ...and below the knee it does not.
        let mut f = Ladder::new();
        f.start(0.8);
        let mut tail = 0.0f64;
        for i in 0..(SR as usize) {
            let y = f.process(0.0, cutoff, 0.8, 4, SR);
            if i > SR as usize / 2 {
                tail = tail.max(y.abs());
            }
        }
        assert!(tail < 1.0e-6, "the filter oscillates below its knee: {tail:e}");
    }

    /// The cutoff knob is the manual's own range, end to end.
    #[test]
    fn the_cutoff_knob_spans_the_range_the_manual_gives() {
        assert!((cutoff_hz(0.0) - 20.0).abs() < 1.0e-9);
        assert!((cutoff_hz(1.0) - 16_000.0).abs() < 1.0e-6);
        // Geometric, so the octave span is what a keyboard-follow or envelope
        // offset is measured against.
        assert!((cutoff_hz(1.0) / cutoff_hz(0.0) - 2.0f64.powf(CUTOFF_OCTAVES)).abs() < 1.0e-6);
        // Halfway is halfway in octaves, not in hertz.
        assert!((cutoff_hz(0.5) / cutoff_hz(0.0) - cutoff_hz(1.0) / cutoff_hz(0.5)).abs() < 1.0e-6);
    }

    // ── Overload ──

    #[test]
    fn the_overload_knob_is_the_identity_at_zero() {
        let mut x = -3.0f64;
        while x <= 3.0 {
            assert_eq!(overload_pre(x, 0.0).to_bits(), x.to_bits(), "pre stage altered {x}");
            assert_eq!(overload_post(x, 0.0).to_bits(), x.to_bits(), "post stage altered {x}");
            x += 0.0017;
        }
    }

    /// A sine of amplitude `level` through the whole overload stage, as
    /// (rms, total harmonic distortion).
    fn overload_sine(level: f64, amount: f64) -> (f64, f64) {
        const HZ: f64 = 1_000.0;
        let n = 4_410usize;
        let out: Vec<f64> = (0..n)
            .map(|i| {
                let x = (std::f64::consts::TAU * HZ * i as f64 / SR).sin() * level;
                overload_post(overload_pre(x, amount), amount)
            })
            .collect();
        // The fundamental, by a single-bin transform; whatever is left is the
        // harmonics the shaper made, plus the DC the asymmetry leaves.
        let (mut re, mut im) = (0.0, 0.0);
        let mut total = 0.0;
        for (i, y) in out.iter().enumerate() {
            let angle = std::f64::consts::TAU * HZ * i as f64 / SR;
            re += y * angle.cos();
            im += y * angle.sin();
            total += y * y;
        }
        let fundamental = 2.0 * (re * re + im * im).sqrt() / n as f64;
        let rms = (total / n as f64).sqrt();
        let at_fundamental = fundamental * fundamental * 0.5;
        // Reported as an amplitude ratio, which is what "total harmonic
        // distortion" conventionally means.
        (rms, (1.0 - at_fundamental / (rms * rms).max(1.0e-30)).max(0.0).sqrt())
    }

    /// "When set to 100%, Overload adds a volume boost of about +6dB"
    /// (page 14). Exact for a signal under the knee, which is where a decibel
    /// figure for a clipper means anything at all.
    #[test]
    fn overload_at_full_is_six_decibels() {
        let quiet = 1.0e-6;
        for step in 0..=10 {
            let amount = f64::from(step) / 10.0;
            let gain = overload_pre(quiet, amount) / quiet;
            assert!(
                (gain - (1.0 + amount)).abs() < 1.0e-5,
                "at {amount} the knob adds {gain:.6} rather than {:.6}",
                1.0 + amount
            );
        }
        assert!((20.0 * 2.0f64.log10() - 6.0206).abs() < 1.0e-3);
    }

    /// Turning the growl up has to add harmonics, and across the whole travel
    /// rather than only at the end.
    #[test]
    fn overload_adds_harmonics_across_its_travel() {
        // Measured at the top of the knob: 2.5% distortion on a quiet signal,
        // 5.1% at half scale, 8.8% at full scale and 14% on the loudest thing
        // the mixer can produce.
        for (level, expected) in [(0.2, 0.02), (0.5, 0.045), (1.0, 0.08), (2.0, 0.13)] {
            let mut previous = -1.0f64;
            for step in 0..=20 {
                let amount = f64::from(step) / 20.0;
                let (_, distortion) = overload_sine(level, amount);
                assert!(
                    distortion >= previous - 1.0e-9,
                    "at an input of {level} the knob at {amount} makes {distortion:.5} of \
                     harmonics after {previous:.5}"
                );
                previous = distortion;
            }
            // ...and there is something there at the top, more of it the
            // louder the signal. The manual's own description of the travel is
            // "the subtle warmth of soft clipping" running into "the 'growl'
            // provided by the beginnings of hard clipping", which is what a
            // distortion figure that grows with the signal is.
            assert!(
                previous > expected,
                "the top of the knob leaves {previous:.4} of distortion at an input of \
                 {level}, where {expected} was measured"
            );
        }
        // Nothing at all at the bottom.
        let (_, none) = overload_sine(1.0, 0.0);
        assert!(none < 1.0e-6, "the knob at zero already distorts: {none:e}");
    }

    /// Turning the knob up may never take loudness away, at any input the
    /// mixer can produce.
    ///
    /// Measured as the rms of a shaped sine rather than as the instantaneous
    /// transfer, because a clipper is *supposed* to take the peaks down — what
    /// it may not do is make the thing quieter, which is a defect this project
    /// has shipped once already on another instrument's drive knob.
    ///
    /// It is also what sets [`OVERLOAD_KNEE`]. The first version of this stage
    /// borrowed the house synth's level-preserving-at-a-reference curve and
    /// failed here on every patch whose mix ran above half scale.
    #[test]
    fn the_overload_knob_never_takes_loudness_away() {
        let mut level = 0.02f64;
        while level <= 2.0 {
            let mut previous = f64::NEG_INFINITY;
            for step in 0..=40 {
                let amount = f64::from(step) / 40.0;
                let (rms, _) = overload_sine(level, amount);
                assert!(
                    rms >= previous - 1.0e-9,
                    "at an input of {level:.3} the knob at {amount} gives {rms:.6} rms after \
                     {previous:.6}"
                );
                previous = rms;
            }
            level *= 1.09;
        }
    }

    /// ...and through the whole instrument, where it also has to be audible.
    #[test]
    fn overload_is_audible_through_the_instrument() {
        let mut s = fresh(0);
        for (index, value) in [
            (P_O1_WAVE, 1.0f32 / 3.0), (P_O1_LEVEL, 0.8), (P_O2_LEVEL, 0.0),
            (P_CUTOFF, 0.92), (P_RESO, 0.1), (P_EG_AMT, 0.5), (P_KB_AMT, 0.0),
            (P_V_ATTACK, 0.0), (P_V_SUSTAIN, 1.0), (P_VOLUME, 0.5), (P_OVERLOAD, 0.0),
        ] {
            s.set_parameter(index, value);
        }
        let dry = render(&mut s, &[note_on(45, 100, 0)], 400);
        s.set_parameter(P_OVERLOAD, 1.0);
        let wet = render(&mut s, &[note_on(45, 100, 0)], 400);
        let moved = rms(&dry[8192..]
            .iter()
            .zip(wet[8192..].iter())
            .map(|(a, b)| a - b)
            .collect::<Vec<f32>>())
            / rms(&dry[8192..]).max(1.0e-30);
        assert!(moved > 0.3, "the overload knob barely changes the sound: {moved:.4}");
        assert!(
            rms(&wet[8192..]) > rms(&dry[8192..]),
            "the knob made the patch quieter"
        );
    }

    /// The manual's tech note: "The LP's Overload circuit uses asymmetrical
    /// clipping, which clips each side of the waveform differently."
    #[test]
    fn overload_clips_asymmetrically() {
        let up = overload_pre(0.8, 1.0) - overload_pre(0.0, 1.0);
        let down = overload_pre(0.0, 1.0) - overload_pre(-0.8, 1.0);
        assert!(
            (up / down - 1.0).abs() > 0.05,
            "the two halves are clipped the same: {up:.4} against {down:.4}"
        );
        // ...and the offset that makes it asymmetric does not reach the
        // filter, because a lowpass would hand it straight to the amplifier.
        let mut dc = DcBlock::new();
        let coefficient = (-std::f64::consts::TAU * DC_BLOCK_HZ / SR).exp();
        let mut last = 0.0;
        for i in 0..(SR as usize * 2) {
            let x = (std::f64::consts::TAU * 110.0 * i as f64 / SR).sin() * 0.5;
            last = dc.tick(overload_pre(x, 1.0), coefficient);
        }
        let mut mean = 0.0;
        for i in 0..(SR as usize) {
            let x = (std::f64::consts::TAU * 110.0 * (i as f64) / SR).sin() * 0.5;
            mean += dc.tick(overload_pre(x, 1.0), coefficient);
        }
        let _ = last;
        assert!(
            (mean / SR).abs() < 1.0e-3,
            "the overload left {:.5} of DC behind",
            mean / SR
        );
    }

    // ── Sync and the modulation bus ──

    /// With sync on, oscillator 2 restarts every time oscillator 1 does, so
    /// whatever OSC 2 FREQ is set to, the pitch heard is oscillator 1's.
    #[test]
    fn sync_locks_oscillator_2_to_oscillator_1() {
        fn sounded_hz(sync: bool, detune: f32) -> f64 {
            let mut s = fresh(0);
            for (index, value) in [
                (P_O1_LEVEL, 0.0f32), (P_O2_LEVEL, 0.9), (P_O2_WAVE, 0.33),
                (P_O2_FREQ, detune), (P_SYNC, knob_for(usize::from(sync), 2)),
                (P_CUTOFF, 0.95), (P_RESO, 0.0), (P_KB_AMT, 0.0), (P_EG_AMT, 0.5),
                (P_V_ATTACK, 0.0), (P_V_SUSTAIN, 1.0), (P_VOLUME, 0.8),
                (P_OVERLOAD, 0.0), (P_O1_OCT, knob_for(1, 4)), (P_O2_OCT, knob_for(1, 4)),
            ] {
                s.set_parameter(index, value);
            }
            let out = render(&mut s, &[note_on(45, 100, 0)], 1400);
            fundamental_hz(&out[16384..24576], SR, 60.0, 400.0)
        }

        // A2 at 8' is 110 Hz. Detuned up seven semitones and free-running,
        // oscillator 2 sounds its own 164.8 Hz.
        let free = sounded_hz(false, 1.0);
        assert!((free - 164.8).abs() < 3.0, "free-running osc 2 is at {free:.1} Hz");
        // Synced, the repetition rate is oscillator 1's however far
        // oscillator 2 is detuned — which is the definition of hard sync. A
        // crossing count is no use here: a synced waveform crosses zero
        // several times a period, and the period is the thing being tested.
        for detune in [0.6f32, 0.8, 1.0] {
            let locked = sounded_hz(true, detune);
            assert!(
                (locked - 110.0).abs() < 2.0,
                "synced at detune {detune}, osc 2 repeats at {locked:.1} Hz rather than 110"
            );
        }
        // ...and it is not simply the same sound: sweeping OSC 2 FREQ under
        // sync moves the timbre, which is the effect.
        let mut spectra = Vec::new();
        for detune in [0.5f32, 0.75, 1.0] {
            let mut s = fresh(0);
            for (index, value) in [
                (P_O1_LEVEL, 0.0f32), (P_O2_LEVEL, 0.9), (P_O2_WAVE, 0.33),
                (P_O2_FREQ, detune), (P_SYNC, knob_for(1, 2)), (P_CUTOFF, 0.95),
                (P_RESO, 0.0), (P_KB_AMT, 0.0), (P_EG_AMT, 0.5), (P_V_ATTACK, 0.0),
                (P_V_SUSTAIN, 1.0), (P_VOLUME, 0.8), (P_OVERLOAD, 0.0),
                (P_O1_OCT, knob_for(1, 4)), (P_O2_OCT, knob_for(1, 4)),
            ] {
                s.set_parameter(index, value);
            }
            let out = render(&mut s, &[note_on(45, 100, 0)], 800);
            let body: Vec<f64> = out[16384..24576].iter().map(|v| f64::from(*v)).collect();
            let s = spectrum(&body);
            let (mut num, mut den) = (0.0, 0.0);
            for (k, magnitude) in s.iter().enumerate() {
                let energy = magnitude * magnitude;
                num += energy * k as f64;
                den += energy;
            }
            spectra.push(num / den.max(1.0e-30));
        }
        // Measured: 55.3, 53.4 and 73.2. Not monotonic, and it should not be
        // — at a detune of a minor third the reset lands where it reinforces
        // the low harmonics, which pulls the centre of gravity slightly *down*
        // before the formant climbs past it. What the sweep has to do is move
        // the timbre, which by the top of the knob it has by a third.
        assert!(
            spectra[2] > spectra[0] * 1.2,
            "sweeping OSC 2 FREQ under sync did not move the formant: {spectra:?}"
        );
    }

    /// All four destinations reach something, and the right thing.
    #[test]
    fn the_mod_bus_reaches_every_destination() {
        fn play(destination: usize, source: usize, amount: f32) -> Vec<f32> {
            let mut s = fresh(0);
            for (index, value) in [
                (P_O1_WAVE, 0.5f32), (P_O1_LEVEL, 0.9), (P_O2_LEVEL, 0.5),
                (P_O2_WAVE, 0.5), (P_CUTOFF, 0.55), (P_RESO, 0.2), (P_KB_AMT, 0.3),
                (P_EG_AMT, 0.5), (P_V_ATTACK, 0.0), (P_V_SUSTAIN, 1.0),
                (P_VOLUME, 0.7), (P_OVERLOAD, 0.0), (P_LFO_RATE, 0.35),
                (P_MOD_SRC, knob_for(source, 6)),
                (P_MOD_DEST, knob_for(destination, 4)),
                (P_MOD_AMT, amount),
            ] {
                s.set_parameter(index, value);
            }
            render(&mut s, &[note_on(48, 100, 0)], 600)
        }

        for (destination, label) in MOD_DESTS.iter().enumerate() {
            let off = play(destination, 0, 0.0);
            let on = play(destination, 0, 0.8);
            let moved: f64 = off
                .iter()
                .zip(on.iter())
                .map(|(a, b)| f64::from((a - b).abs()))
                .sum();
            assert!(moved > 1.0, "destination {label} does nothing");
        }

        // The secondary destination is a second route at the same amount.
        let mut single = fresh(0);
        let mut both = fresh(0);
        for s in [&mut single, &mut both] {
            for (index, value) in [
                (P_LFO_RATE, 0.30f32), (P_MOD_AMT, 0.7),
                (P_MOD_DEST, knob_for(1, 4)), (P_V_SUSTAIN, 1.0), (P_V_ATTACK, 0.0),
            ] {
                s.set_parameter(index, value);
            }
        }
        both.set_parameter(P_MOD_DEST2, knob_for(1, 5));
        let a = render(&mut single, &[note_on(48, 100, 0)], 400);
        let b = render(&mut both, &[note_on(48, 100, 0)], 400);
        let moved: f64 = a.iter().zip(b.iter()).map(|(x, y)| f64::from((x - y).abs())).sum();
        assert!(moved > 1.0, "the second destination did nothing");

        // Both alternate sources are reachable and are not the source they
        // share a position with.
        for (position, alternate) in [(4usize, P_SRC5), (5, P_SRC6)] {
            let mut first = fresh(0);
            let mut second = fresh(0);
            for s in [&mut first, &mut second] {
                for (index, value) in [
                    (P_MOD_SRC, knob_for(position, 6)),
                    (P_MOD_DEST, knob_for(1, 4)),
                    (P_MOD_AMT, 0.8f32),
                    (P_V_SUSTAIN, 1.0),
                    (P_V_ATTACK, 0.0),
                    (P_LFO_RATE, 0.4),
                ] {
                    s.set_parameter(index, value);
                }
            }
            second.set_parameter(alternate, knob_for(1, 2));
            let a = render(&mut first, &[note_on(48, 100, 0)], 400);
            let b = render(&mut second, &[note_on(48, 100, 0)], 400);
            let moved: f64 = a.iter().zip(b.iter()).map(|(x, y)| f64::from((x - y).abs())).sum();
            assert!(moved > 1.0, "{} does not change the source", PARAM_NAMES[alternate]);
        }
    }

    /// "Although the waveforms can be set from the front panel individually
    /// for each oscillator, modulation is applied to both waveform controls
    /// simultaneously" (page 12).
    #[test]
    fn wave_modulation_reaches_both_oscillators() {
        fn play(which: usize, amount: f32) -> Vec<f32> {
            let mut s = fresh(0);
            for (index, value) in [
                (P_O1_WAVE, 0.5f32), (P_O2_WAVE, 0.5),
                (P_O1_LEVEL, if which == 0 { 0.9 } else { 0.0 }),
                (P_O2_LEVEL, if which == 1 { 0.9 } else { 0.0 }),
                (P_CUTOFF, 0.9), (P_RESO, 0.0), (P_KB_AMT, 0.0), (P_EG_AMT, 0.5),
                (P_V_ATTACK, 0.0), (P_V_SUSTAIN, 1.0), (P_VOLUME, 0.7),
                (P_OVERLOAD, 0.0), (P_LFO_RATE, 0.3),
                (P_MOD_DEST, knob_for(2, 4)), (P_MOD_AMT, amount),
            ] {
                s.set_parameter(index, value);
            }
            render(&mut s, &[note_on(48, 100, 0)], 400)
        }
        for which in 0..2 {
            let off = play(which, 0.0);
            let on = play(which, 0.9);
            let moved: f64 = off
                .iter()
                .zip(on.iter())
                .map(|(a, b)| f64::from((a - b).abs()))
                .sum();
            assert!(moved > 1.0, "wave modulation missed oscillator {}", which + 1);
        }
    }

    /// The LFO runs at the rate the knob says, at both ends of the manual's
    /// 0.2 Hz to 500 Hz range.
    #[test]
    fn the_lfo_runs_at_the_rate_the_knob_says() {
        assert!((lfo_hz(0.0) - LFO_MIN_HZ).abs() < 1.0e-12);
        assert!((lfo_hz(1.0) - LFO_MAX_HZ).abs() < 1.0e-9);
        // Timed between wraps rather than counted over a fixed window: at
        // 0.96 Hz a four-second count is three or four cycles, and the
        // quantisation is bigger than the tolerance.
        for knob in [0.0, 0.2, 0.5, 0.8, 1.0] {
            let hz = lfo_hz(knob);
            let mut lfo = Lfo::new();
            let (mut first, mut last, mut wraps) = (0usize, 0usize, 0u32);
            let limit = (SR * 30.0 / hz.clamp(0.02, 1.0)).min(SR * 60.0) as usize;
            for i in 0..limit {
                if lfo.tick(hz, SR, 0).1 {
                    wraps += 1;
                    if wraps == 1 {
                        first = i;
                    }
                    last = i;
                }
            }
            assert!(wraps >= 3, "only {wraps} cycles at {hz:.3} Hz");
            let measured = f64::from(wraps - 1) * SR / (last - first) as f64;
            assert!(
                (measured / hz - 1.0).abs() < 0.01,
                "the LFO asked for {hz:.3} Hz ran at {measured:.3}"
            );
        }
    }

    // ── The panel ──

    #[test]
    fn the_panel_is_in_front_panel_order() {
        // The indices are a contiguous run in the order the manual walks the
        // instrument: the user interface strip, MODULATION, OSCILLATORS,
        // FILTER, the ENVELOPE GENERATORS, the keyboard, OUTPUT.
        let order = [
            P_PATCH, P_GLIDE, P_OCTAVE, P_FINE, P_BEND_UP, P_BEND_DOWN,
            P_LFO_RATE, P_MOD_AMT, P_MOD_SRC, P_MOD_DEST, P_MOD_DEST2, P_SRC5, P_SRC6,
            P_O1_OCT, P_O1_WAVE, P_O1_LEVEL, P_GLIDE_RATE, P_SYNC,
            P_O2_OCT, P_O2_FREQ, P_O2_WAVE, P_O2_LEVEL,
            P_CUTOFF, P_RESO, P_KB_AMT, P_EG_AMT, P_OVERLOAD, P_POLES, P_VEL_SENS,
            P_F_ATTACK, P_F_DECAY, P_F_SUSTAIN, P_F_RELEASE,
            P_V_ATTACK, P_V_DECAY, P_V_SUSTAIN, P_V_RELEASE, P_EGR_REL, P_GATE,
            P_PRIORITY, P_VOLUME,
        ];
        assert_eq!(order.len(), PARAM_COUNT);
        for (position, index) in order.into_iter().enumerate() {
            assert_eq!(position, index, "{} is out of panel order", PARAM_NAMES[index]);
        }
        assert_eq!(PARAM_NAMES.len(), PARAM_COUNT);
        assert_eq!(PARAM_DEFAULTS.len(), PARAM_COUNT);
        for name in PARAM_NAMES {
            assert!(name.chars().count() <= 8, "{name:?} overflows the editor's column");
            assert!(!name.is_empty());
        }
    }

    #[test]
    fn switches_step_one_position_per_press() {
        // A stepper that adds a fraction of the travel walks a switch part of
        // a position at a time and stalls on a boundary; the DX7's bank knob
        // did exactly that. Every selector here steps by index.
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            let Some(count) = discrete_steps(index) else {
                assert_eq!(step_discrete(index, 0.42, true), 0.42, "knob {name} moved");
                assert_eq!(step_discrete(index, 0.42, false), 0.42, "knob {name} moved");
                continue;
            };
            let mut knob = knob_for(0, count);
            for step in 1..count {
                knob = step_discrete(index, knob, true);
                assert_eq!(selector(knob, count), step, "{name} up to {step}");
            }
            knob = step_discrete(index, knob, true);
            assert_eq!(selector(knob, count), count - 1, "{name} ran off the top");
            for step in (0..count - 1).rev() {
                knob = step_discrete(index, knob, false);
                assert_eq!(selector(knob, count), step, "{name} down to {step}");
            }
            knob = step_discrete(index, knob, false);
            assert_eq!(selector(knob, count), 0, "{name} ran off the bottom");
        }
    }

    #[test]
    fn switch_labels_read_as_the_panel_does() {
        // The two selector orders are the manual's own, from the Overview on
        // page 7 and the MIDI CC table on page 55, which agree with each other
        // against the Modulation section's prose.
        for (position, label) in ["tri", "square", "saw", "ramp", "filt eg", "osc 2"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(discrete_label(P_MOD_SRC, knob_for(position, 6)), Some(label));
        }
        for (position, label) in ["pitch", "filter", "wave", "osc 2"].into_iter().enumerate() {
            assert_eq!(discrete_label(P_MOD_DEST, knob_for(position, 4)), Some(label));
        }
        assert_eq!(discrete_label(P_MOD_DEST2, knob_for(0, 5)), Some("off"));
        assert_eq!(discrete_label(P_SRC5, knob_for(0, 2)), Some("filt eg"));
        assert_eq!(discrete_label(P_SRC5, knob_for(1, 2)), Some("s&h"));
        assert_eq!(discrete_label(P_SRC6, knob_for(1, 2)), Some("noise"));
        for (position, label) in ["16'", "8'", "4'", "2'"].into_iter().enumerate() {
            assert_eq!(discrete_label(P_O1_OCT, knob_for(position, 4)), Some(label));
            assert_eq!(discrete_label(P_O2_OCT, knob_for(position, 4)), Some(label));
        }
        for (position, label) in ["6dB", "12dB", "18dB", "24dB"].into_iter().enumerate() {
            assert_eq!(discrete_label(P_POLES, knob_for(position, 4)), Some(label));
        }
        assert_eq!(discrete_label(P_OCTAVE, knob_for(0, 5)), Some("-2"));
        assert_eq!(discrete_label(P_OCTAVE, knob_for(4, 5)), Some("+2"));
        assert_eq!(discrete_label(P_VEL_SENS, knob_for(0, 17)), Some("-8"));
        assert_eq!(discrete_label(P_VEL_SENS, knob_for(8, 17)), Some("0"));
        assert_eq!(discrete_label(P_VEL_SENS, knob_for(16, 17)), Some("+8"));
        assert_eq!(discrete_label(P_GATE, knob_for(0, 3)), Some("leg on"));
        assert_eq!(discrete_label(P_GATE, knob_for(2, 3)), Some("reset"));
        assert_eq!(discrete_label(P_PRIORITY, knob_for(2, 3)), Some("last"));
        assert_eq!(discrete_label(P_CUTOFF, 0.5), None);
        // Out-of-range knobs are labelled, not panicked on: `params` is public.
        assert_eq!(discrete_label(P_O1_OCT, 9.0), Some("2'"));
        assert_eq!(discrete_label(P_O1_OCT, -1.0), Some("16'"));
        assert_eq!(discrete_label(P_PATCH, f32::NAN), Some(PATCH_NAMES[0]));
        // Every label fits the twelve columns the editor's selector row has.
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            let Some(count) = discrete_steps(index) else { continue };
            for position in 0..count {
                let label = discrete_label(index, knob_for(position, count)).unwrap();
                assert!(
                    label.chars().count() <= 12,
                    "{label:?} on {name} needs {} columns",
                    label.chars().count()
                );
            }
        }
    }

    #[test]
    fn the_patch_knob_lands_on_the_patch_it_names() {
        for (index, name) in PATCH_NAMES.iter().enumerate() {
            assert_eq!(patch_index(patch_knob(index)), index, "patch {index}");
            assert_eq!(discrete_label(P_PATCH, patch_knob(index)), Some(*name));
        }
        assert_eq!(patch_index(0.0), 0);
        assert_eq!(patch_index(1.0), PATCH_COUNT - 1);
        assert_eq!(patch_index(9.0), PATCH_COUNT - 1);
        assert_eq!(patch_index(-1.0), 0);
        assert_eq!(patch_index(f32::NAN), 0);
        assert_eq!(patch_knob(PATCH_COUNT + 50), patch_knob(PATCH_COUNT - 1));
    }

    #[test]
    fn patch_zero_is_the_default_parameter_block() {
        let loaded = LittlePhatty::params_for_patch(0.0);
        for index in 0..PARAM_COUNT {
            assert!(
                (loaded[index] - PARAM_DEFAULTS[index]).abs() < 5.0e-4,
                "default {index} ({}) is {} but patch 0 loads {}",
                PARAM_NAMES[index],
                PARAM_DEFAULTS[index],
                loaded[index]
            );
        }
        // ...and a fresh instrument is at that patch.
        let s = LittlePhatty::new();
        for (index, value) in PARAM_DEFAULTS.iter().enumerate() {
            assert_eq!(s.get_parameter(index), *value);
        }
    }

    #[test]
    fn the_patch_knob_loads_the_whole_panel() {
        let mut s = fresh(0);
        s.set_parameter(P_PATCH, patch_knob(42));
        let panel = BANK[42].panel();
        for index in 1..PARAM_COUNT {
            assert!(
                (s.get_parameter(index) - panel[index]).abs() < 1.0e-6,
                "{} did not follow the patch knob",
                PARAM_NAMES[index]
            );
        }
        assert_eq!(patch_index(s.get_parameter(P_PATCH)), 42);
        // FINE TUNE is centred in every patch: it is there to match an
        // external reference, not to voice a sound.
        for index in 0..PATCH_COUNT {
            assert_eq!(BANK[index].panel()[P_FINE], 0.5, "{} moved FINE", PATCH_NAMES[index]);
        }
    }

    /// A control the engine reads has to have an index, and moving it has to
    /// change the sound. Nine of the Juno's panel controls were unreachable
    /// for a while for want of this test.
    #[test]
    fn every_engine_control_is_reachable() {
        fn play(s: &mut LittlePhatty) -> Vec<f32> {
            // The second key is *above* the first, so that low-note and
            // last-note priority disagree about which one sounds.
            let mut out = render(s, &[note_on(60, 100, 0)], 60);
            out.extend(render(s, &[note_on(67, 100, 0)], 40));
            // The wheel both ways, so that the two PITCH BEND ranges have
            // something to scale.
            out.extend(render(s, &[bend(1.0, 0)], 30));
            out.extend(render(s, &[bend(-1.0, 0)], 30));
            out.extend(render(s, &[bend(0.0, 0)], 20));
            out.extend(render(s, &[note_off(67, 0)], 40));
            out.extend(render(s, &[note_off(60, 0)], 60));
            out
        }
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            if index == P_PATCH {
                continue;
            }
            let mut low = LittlePhatty::new();
            let mut high = LittlePhatty::new();
            for s in [&mut low, &mut high] {
                s.init(SR, 64);
                // Every path live, so that nothing is masked by a dead one:
                // both oscillators up, sync off, the mod bus running with a
                // second destination, glide on, an envelope with all four
                // segments doing something.
                for (control, value) in [
                    (P_O1_LEVEL, 0.7f32), (P_O2_LEVEL, 0.7), (P_O1_WAVE, 0.45),
                    (P_O2_WAVE, 0.55), (P_O2_FREQ, 0.56), (P_CUTOFF, 0.55),
                    (P_RESO, 0.35), (P_KB_AMT, 0.5), (P_EG_AMT, 0.72),
                    (P_OVERLOAD, 0.3), (P_MOD_AMT, 0.5), (P_LFO_RATE, 0.45),
                    (P_MOD_DEST2, knob_for(2, 5)), (P_GLIDE, knob_for(1, 2)),
                    (P_GLIDE_RATE, 0.6), (P_VEL_SENS, knob_for(12, 17)),
                    (P_F_ATTACK, 0.25), (P_F_DECAY, 0.5), (P_F_SUSTAIN, 0.4),
                    (P_F_RELEASE, 0.45), (P_V_ATTACK, 0.2), (P_V_DECAY, 0.5),
                    (P_V_SUSTAIN, 0.5), (P_V_RELEASE, 0.45), (P_VOLUME, 0.7),
                    (P_OCTAVE, 0.5), (P_FINE, 0.5),
                ] {
                    s.set_parameter(control, value);
                }
            }
            // MOD SRC 5 and MOD SRC 6 choose between the two readings of one
            // position of the SOURCE selector, so they only do anything with
            // the selector standing on it.
            if let Some(position) = match index {
                P_SRC5 => Some(4),
                P_SRC6 => Some(5),
                _ => None,
            } {
                for s in [&mut low, &mut high] {
                    s.set_parameter(P_MOD_SRC, knob_for(position, 6));
                    s.set_parameter(P_MOD_DEST, knob_for(1, 4));
                    s.set_parameter(P_MOD_AMT, 0.8);
                }
            }
            low.set_parameter(index, 0.0);
            high.set_parameter(index, 1.0);
            let a = play(&mut low);
            let b = play(&mut high);
            let moved: f64 = a
                .iter()
                .zip(b.iter())
                .map(|(x, y)| f64::from((x - y).abs()))
                .sum();
            assert!(moved > 1.0e-3, "{name} (index {index}) changes nothing: {moved:e}");
        }
    }

    // ── The bank ──

    #[test]
    fn patch_names_are_unique_and_fit_the_editor() {
        for (index, name) in PATCH_NAMES.iter().enumerate() {
            assert!(!name.trim().is_empty(), "patch {index} has no name");
            assert!(
                name.chars().count() <= 12,
                "{name:?} needs {} of the twelve columns the editor leaves",
                name.chars().count()
            );
            assert!(
                !PATCH_NAMES[index + 1..].contains(name),
                "{name:?} appears twice in the bank"
            );
        }
        assert_eq!(PATCH_NAMES.len(), PATCH_COUNT);
        assert_eq!(BANK.len(), PATCH_COUNT);
    }

    /// The bank has to use the instrument, not one corner of it.
    #[test]
    fn the_bank_covers_the_instrument() {
        let panels: Vec<[f32; PARAM_COUNT]> = BANK.iter().map(Program::panel).collect();
        let count = |predicate: &dyn Fn(&[f32; PARAM_COUNT]) -> bool| {
            panels.iter().filter(|p| predicate(p)).count()
        };

        // Every position of every selector that names a sound is used by
        // something.
        for index in [P_O1_OCT, P_O2_OCT, P_POLES, P_MOD_SRC, P_MOD_DEST, P_GATE] {
            let count_positions = discrete_steps(index).unwrap();
            for position in 0..count_positions {
                let used = panels
                    .iter()
                    .filter(|p| selector(p[index], count_positions) == position)
                    .count();
                assert!(
                    used > 0,
                    "no patch uses {} on {}",
                    discrete_label(index, knob_for(position, count_positions)).unwrap(),
                    PARAM_NAMES[index]
                );
            }
        }
        assert!(count(&|p| selector(p[P_SYNC], 2) == 1) >= 8, "not enough sync patches");
        assert!(count(&|p| p[P_OVERLOAD] > 0.5) >= 12, "not enough overload patches");
        assert!(
            count(&|p| selector(p[P_MOD_DEST], 4) == 2) >= 8,
            "not enough patches point the mod bus at WAVE"
        );
        assert!(
            count(&|p| selector(p[P_SRC5], 2) == 1) >= 5,
            "not enough sample-and-hold patches"
        );
        assert!(
            count(&|p| selector(p[P_POLES], 4) < 3) >= 8,
            "not enough patches away from four poles"
        );
        assert!(count(&|p| selector(p[P_GLIDE], 2) == 1) >= 4, "not enough glide patches");
        assert!(count(&|p| p[P_V_SUSTAIN] <= 0.01) >= 8, "not enough percussive patches");
        assert!(count(&|p| p[P_RESO] > 0.7) >= 8, "not enough resonant patches");
        assert!(count(&|p| selector(p[P_MOD_DEST2], 5) > 0) >= 2, "the second destination is unused");
        assert!(count(&|p| selector(p[P_SRC6], 2) == 1) >= 2, "the noise source is unused");
        // The wave knob is the instrument, so the bank has to visit the whole
        // travel rather than sit on the four labelled positions.
        let between = panels
            .iter()
            .filter(|p| {
                let w = f64::from(p[P_O1_WAVE]);
                [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]
                    .iter()
                    .all(|landmark| (w - landmark).abs() > 0.04)
            })
            .count();
        assert!(between >= 25, "only {between} patches sit between the labelled shapes");
    }

    /// What one patch sounds like, as seven numbers.
    ///
    /// Aggregate energy measurements rather than a hash of the samples, for
    /// the reason set out at the drum rack's `RENDERED`: this project runs on
    /// three platforms with three libms, `exp`, `tan` and `exp2` all round the
    /// last bit their own way, and a digest of the raw output reports a
    /// completely different number when one sample rounds the other way. A sum
    /// of a million squares does not care.
    fn fingerprint(index: usize) -> [f64; 9] {
        // A held note, then a leap to a second one without letting go, then
        // the release. The leap is what makes glide, keyboard tracking and the
        // gate mode visible: without it two patches that differ only in how
        // they are *played* look identical.
        let mut s = fresh(index);
        let mut out = render(&mut s, &[note_on(45, 100, 0)], 350);
        let second = render(&mut s, &[note_on(57, 100, 0)], 350);
        let tail_render = render(&mut s, &[note_off(57, 0)], 300);
        let first_level = rms(&out);
        let second_level = rms(&second);
        out.extend_from_slice(&second);
        out.extend_from_slice(&tail_render);

        let (mut low, mut high, mut total, mut slope) = (0.0, 0.0, 0.0, 0.0);
        let (mut lp, mut hp, mut last) = (0.0f64, 0.0f64, 0.0f64);
        let a_low = (-std::f64::consts::TAU * 300.0 / SR).exp();
        let a_high = (-std::f64::consts::TAU * 3_000.0 / SR).exp();
        for v in &out {
            let x = f64::from(*v);
            lp = x * (1.0 - a_low) + lp * a_low;
            hp = x * (1.0 - a_high) + hp * a_high;
            let top = x - hp;
            total += x * x;
            low += lp * lp;
            high += top * top;
            slope += (x - last) * (x - last);
            last = x;
        }
        let level = (total / out.len() as f64).sqrt();
        // Brightness as a frequency: the rms of the first difference over the
        // rms of the signal is the centre of gravity of the spectrum in
        // radians a sample.
        let brightness = (slope / total.max(1.0e-30)).sqrt() * SR / std::f64::consts::TAU;

        // How much the level moves over the note, which is what separates a
        // patch with an LFO or a sample-and-hold on it from one without.
        let windows = window_rms(&out);
        let mean = windows.iter().sum::<f64>() / windows.len().max(1) as f64;
        let spread = (windows.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>()
            / windows.len().max(1) as f64)
            .sqrt()
            / mean.max(1.0e-30);

        let top = f64::from(peak(&out));
        let at = out
            .iter()
            .position(|v| f64::from(v.abs()) >= top * 0.95)
            .unwrap_or(0) as f64
            / out.len() as f64;
        let quarter = out.len() / 4;
        let head = rms(&out[..quarter]);
        let tail = rms(&out[out.len() - quarter..]);
        [
            level,
            low / total.max(1.0e-30),
            high / total.max(1.0e-30),
            at,
            top / level.max(1.0e-30),
            tail / head.max(1.0e-30),
            brightness,
            spread,
            second_level / first_level.max(1.0e-30),
        ]
    }

    /// How far one fingerprint sits from another, in tolerances. Anything
    /// under 1.0 is two patches that are the same sound with a different name.
    fn apart(a: &[f64; 9], b: &[f64; 9]) -> f64 {
        // What counts as a difference, per feature: level, crest, brightness
        // and the two ratios move by a share of themselves; the shares and the
        // peak arrival are already fractions and move by a share of full
        // scale.
        const RATIO: [bool; 9] =
            [true, false, false, false, true, true, true, true, true];
        const TOLERANCE: [f64; 9] =
            [0.12, 0.06, 0.06, 0.05, 0.10, 0.15, 0.12, 0.20, 0.12];
        let mut worst = 0.0f64;
        for i in 0..9 {
            let d = if RATIO[i] {
                (a[i] - b[i]).abs() / a[i].abs().max(b[i].abs()).max(1.0e-12)
            } else {
                (a[i] - b[i]).abs()
            };
            worst = worst.max(d / TOLERANCE[i]);
        }
        worst
    }

    /// No two patches in the bank are the same sound.
    ///
    /// The rule the drum kits are held to, applied here: a hundred slots is a
    /// hundred sounds, not eighty sounds and twenty near-misses. Each patch is
    /// rendered, held and released, and reduced to level, the share of its
    /// energy below 300 Hz and above 3 kHz, where its peak arrives, its crest
    /// factor, and how much of it is left at the end — and every pair has to
    /// differ in at least one of those by more than the tolerance.
    #[test]
    fn no_two_patches_are_the_same_sound() {
        let prints: Vec<[f64; 9]> = (0..PATCH_COUNT).map(fingerprint).collect();
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        for a in 0..PATCH_COUNT {
            for b in a + 1..PATCH_COUNT {
                pairs.push((apart(&prints[a], &prints[b]), a, b));
            }
        }
        pairs.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        if pairs[0].0 <= 1.0 {
            let mut report = String::new();
            for (distance, a, b) in pairs.iter().take(12) {
                report.push_str(&format!(
                    "\n  {distance:.3}  {:<13} / {:<13}",
                    PATCH_NAMES[*a], PATCH_NAMES[*b]
                ));
            }
            panic!(
                "the bank has patches that are the same sound (1.0 is the edge \
                 of the allowance):{report}"
            );
        }
    }

    // ── Real-time safety ──

    #[test]
    fn the_audio_path_does_not_allocate() {
        // "No allocation in `process`" is a property of the code rather than
        // of its output, so it is counted rather than listened to. The
        // counting allocator lives in synth.rs and is installed for the whole
        // test binary; this is the Little Phatty's half of it.
        use crate::synth::tests::allocations_during;

        let mut s = LittlePhatty::new();
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
            s.process(&[], &mut outs, &[cc(1, 64, 0)]);
            s.process(&[], &mut outs, &releases);
            for index in 0..PATCH_COUNT {
                s.set_parameter(P_PATCH, patch_knob(index));
                s.process(&[], &mut outs, &[note_on(60, 110, 0)]);
            }
            s.process(&[], &mut outs, &[cc(123, 0, 0)]);
            s.process(&[], &mut outs, &[cc(120, 0, 0)]);
        });
        assert_eq!(allocations, 0, "the audio path allocated {allocations} times");
    }

    /// The oscillator has to be free of the spike that half a corner pair
    /// produces, on the first sample of a note and on every sync reset.
    ///
    /// Both were real: before [`Trapezoid::open`] existed, a sawtooth's first
    /// sample was 1662 and every sync reset put a comparable one out. The
    /// waveform is bounded by 1 by construction, so anything past a little
    /// over that is a correction that lost its partner.
    #[test]
    fn wave_modulation_does_not_unbalance_the_corner_pairs() {
        for wave in [0.0f64, 0.2, 1.0 / 3.0, 0.5, 2.0 / 3.0, 0.85, 1.0] {
            for hz in [27.5f64, 110.0, 440.0, 1_760.0, 5_000.0] {
                let x = osc_corrected(hz, SR, wave, 8192);
                let top = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
                assert!(
                    top < 1.35,
                    "wave {wave:.3} at {hz} Hz reached {top:.4} — a corner lost its partner"
                );
            }
        }
        // ...and under a wave knob that moves every sample, which is what the
        // mod bus does to it. An edge that opens at one width and closes at
        // another leaves the difference between two enormous corrections
        // behind; four patches in the bank peaked at 0.93 doing exactly that
        // before `Trapezoid::closing` carried the correction rather than
        // recomputing it.
        // Up to the LFO's own ceiling, which is the fastest the panel can move
        // the wave control: "The frequency is adjustable from 0.2 Hz to 500
        // Hz" (manual page 17). Past that the shape moves faster than the
        // waveform does, and the *value* discontinuity a shape change makes —
        // as opposed to the slope discontinuity the corners are — is not
        // corrected by anything here; a sweep at four times the LFO's maximum
        // measures 1.62. It is not reachable, and it is worth knowing.
        for hz in [110.0f64, 1_000.0, 2_500.0, 5_000.0] {
            for rate in [3.0f64, 100.0, LFO_MAX_HZ] {
                let mut osc = Trapezoid::new();
                let dt = hz / SR;
                let mut top = 0.0f64;
                for i in 0..16384 {
                    let sweep = 0.5
                        + 0.5 * (std::f64::consts::TAU * rate * i as f64 / SR).sin();
                    let shape = Shape::at(sweep);
                    top = top.max(osc.tick(dt, &shape, None).abs());
                }
                assert!(
                    top < 1.35,
                    "a wave sweep at {rate} Hz on a {hz} Hz oscillator reached {top:.4}"
                );
            }
        }

        // ...and under sync, where the reset lands on a corner every cycle.
        for wave in [0.0f64, 1.0 / 3.0, 2.0 / 3.0, 1.0] {
            let shape = Shape::at(wave);
            let mut master = 0.0f64;
            let mut slave = Trapezoid::new();
            let dt_master = 110.0 / SR;
            let dt_slave = 271.3 / SR;
            let mut top = 0.0f64;
            for _ in 0..8192 {
                let sync_at = if master + dt_master >= 1.0 {
                    Some((1.0 - master) / dt_master)
                } else {
                    None
                };
                master = wrap_phase(master + dt_master);
                top = top.max(slave.tick(dt_slave, &shape, sync_at).abs());
            }
            assert!(top < 1.35, "sync at wave {wave:.3} reached {top:.4}");
        }
    }
}

#[cfg(test)]
mod measure {
    use super::*;

    fn note(note: u8, velocity: u8) -> MidiEvent {
        MidiEvent { sample_offset: 0, status: 0x90, data1: note, data2: velocity }
    }

    fn render(synth: &mut LittlePhatty, events: &[MidiEvent], blocks: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(blocks * 64);
        let mut buf = [0.0f32; 64];
        for block in 0..blocks {
            buf.fill(0.0);
            let mut outs: [&mut [f32]; 1] = [&mut buf];
            if block == 0 {
                synth.process(&[], &mut outs, events);
            } else {
                synth.process(&[], &mut outs, &[]);
            }
            out.extend_from_slice(&buf);
        }
        out
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64).sqrt()
    }

    /// Not an assertion — a printout, for setting `OUTPUT_TRIM` and for
    /// reading the bank's spread.
    #[test]
    #[ignore]
    fn report_levels() {
        let mut worst = (0.0f32, "");
        let mut loudest = (0.0f64, "");
        let mut quietest = (f64::MAX, "");
        for (index, name) in PATCH_NAMES.iter().enumerate() {
            let mut s = LittlePhatty::new();
            s.init(44_100.0, 64);
            s.set_parameter(P_PATCH, patch_knob(index));
            let out = render(&mut s, &[note(36, 127)], 800);
            let (p, r) = (peak(&out), rms(&out));
            if p > worst.0 { worst = (p, name); }
            if r > loudest.0 { loudest = (r, name); }
            if r < quietest.0 { quietest = (r, name); }
            println!("{index:>3} {name:<13} peak {p:.4}  rms {r:.5}");
        }
        println!("worst peak:   {} {:.4}", worst.1, worst.0);
        println!("loudest rms:  {} {:.5}", loudest.1, loudest.0);
        println!("quietest rms: {} {:.5}", quietest.1, quietest.0);
        let mut s = LittlePhatty::new();
        s.init(44_100.0, 64);
        let out = render(&mut s, &[note(60, 100)], 800);
        println!("default note 60 v100: rms {:.6} peak {:.4}", rms(&out), peak(&out));

        // The loudest patch on the workspace headroom sweep's own voicing.
        let mut loudest = (0.0f32, 0usize);
        for index in 0..PATCH_COUNT {
            let mut s = LittlePhatty::new();
            s.init(44_100.0, 64);
            s.set_parameter(P_PATCH, patch_knob(index));
            let p = peak(&render(&mut s, &[note(72, 127)], 200));
            if p > loudest.0 {
                loudest = (p, index);
            }
        }
        println!("loudest on note 72 v127: {} ({}) {:.4}", PATCH_NAMES[loudest.1], loudest.1, loudest.0);

        // The same measurement the workspace's level-match test makes: the
        // C major triad at velocity 100, which on a monosynth is one note.
        let triad = [note(60, 100), note(64, 100), note(67, 100)];
        let mut levels: Vec<(f64, usize)> = (0..PATCH_COUNT)
            .map(|index| {
                let mut s = LittlePhatty::new();
                s.init(44_100.0, 64);
                s.set_parameter(P_PATCH, patch_knob(index));
                (rms(&render(&mut s, &triad, 800)), index)
            })
            .collect();
        levels.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        for slot in [0, 25, 50, 75, 99] {
            let (value, index) = levels[slot];
            println!("triad rms percentile {slot}: {} {value:.6}", PATCH_NAMES[index]);
        }
    }

    /// The worst thing a hand can dial in, printed rather than asserted.
    #[test]
    #[ignore]
    fn report_worst_panel() {
        let mut worst = (0.0f32, String::new());
        for poles in 0..4 {
            for overload in [0.0f32, 0.5, 1.0] {
                for resonance in [0.0f32, 0.7, 1.0] {
                    for cutoff in [0.3f32, 0.6, 1.0] {
                        for wave in [0.0f32, 0.33, 0.67, 1.0] {
                            let mut s = LittlePhatty::new();
                            s.init(44_100.0, 64);
                            for (index, value) in [
                                (P_O1_LEVEL, 1.0), (P_O2_LEVEL, 1.0),
                                (P_O1_OCT, 0.125), (P_O2_OCT, 0.125),
                                (P_CUTOFF, cutoff), (P_RESO, resonance),
                                (P_OVERLOAD, overload), (P_POLES, knob_for(poles, 4)),
                                (P_VOLUME, 1.0), (P_V_ATTACK, 0.0), (P_V_DECAY, 0.0),
                                (P_V_SUSTAIN, 1.0), (P_F_SUSTAIN, 1.0), (P_EG_AMT, 1.0),
                                (P_VEL_SENS, 1.0), (P_KB_AMT, 1.0),
                                (P_O1_WAVE, wave), (P_O2_WAVE, wave),
                            ] {
                                s.set_parameter(index, value);
                            }
                            let mut p = 0.0f32;
                            for note_number in [24u8, 60, 99] {
                                for (modulation, rate) in [(0.0f32, 0.3f32), (1.0, 0.55)] {
                                    s.set_parameter(P_MOD_AMT, modulation);
                                    s.set_parameter(P_LFO_RATE, rate);
                                    s.set_parameter(P_MOD_DEST, knob_for(2, 4));
                                    s.reset();
                                    p = p.max(peak(&render(
                                        &mut s,
                                        &[note(note_number, 127)],
                                        200,
                                    )));
                                }
                            }
                            let label = format!(
                                "poles {} ovl {overload} res {resonance} cut {cutoff} wave {wave}",
                                poles + 1
                            );
                            if p > 0.25 {
                                println!("{label}: peak {p:.4}");
                            }
                            if p > worst.0 {
                                worst = (p, label);
                            }
                        }
                    }
                }
            }
        }
        println!("worst panel: {} peak {:.4}", worst.1, worst.0);
    }

    /// The numbers behind `the_pitch_is_the_same_at_every_sample_rate` and
    /// `the_wave_knob_morphs_rather_than_switches`, printed rather than
    /// asserted.
    #[test]
    #[ignore]
    fn report_rate_and_morph() {
        use super::tests::{crossings_at_rate, second_harmonic_ratio};
        let reference = crossings_at_rate(44_100.0);
        println!("A2 at 8', zero crossings a second:");
        for sr in [22_050.0, 44_100.0, 48_000.0, 96_000.0] {
            let measured = crossings_at_rate(sr);
            println!(
                "  {sr:>7}: {measured:8.3}  ({:+.3}% against 44100)",
                (measured / reference - 1.0) * 100.0
            );
        }
        println!("wave knob, second harmonic against the first:");
        for step in 0..=8 {
            let wave = f64::from(step) / 8.0 / 3.0;
            println!("  {wave:.4}: {:.5}", second_harmonic_ratio(wave));
        }
    }

    /// Every patch across the whole keyboard, to find where the peaks are.
    #[test]
    #[ignore]
    fn report_worst_notes() {
        let mut worst = (0.0f32, String::new());
        for (index, name) in PATCH_NAMES.iter().enumerate() {
            let mut top = (0.0f32, 0u8);
            for n in (12u8..=108).step_by(3) {
                let mut s = LittlePhatty::new();
                s.init(44_100.0, 64);
                s.set_parameter(P_PATCH, patch_knob(index));
                let p = peak(&render(&mut s, &[note(n, 127)], 200));
                if p > top.0 {
                    top = (p, n);
                }
            }
            if top.0 > worst.0 {
                worst = (top.0, format!("{name} at note {}", top.1));
            }
            if top.0 > 0.35 {
                println!("{name:<13} {:.4} at note {}", top.0, top.1);
            }
        }
        println!("worst: {} {:.4}", worst.1, worst.0);
    }
}
