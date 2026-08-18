//! Rhodes electric piano, modelled rather than sampled.
//!
//! The sound source is a **tine** — a cylindrical spring-steel cantilever
//! struck by a hammer — paired with a **tonebar** of matched frequency. The two
//! together are an asymmetric tuning fork, and a coil-and-magnet **pickup**
//! reads the tine's motion. Every part of that chain is here:
//!
//! * a bank of six resonators per voice: the tine's fundamental, the tonebar's
//!   partner mode a fraction of a hertz away, and the four inharmonic
//!   cantilever overtones that give the attack its bell;
//! * a hammer whose contact time falls with strike force, so a harder blow puts
//!   more of those overtones into the fork;
//! * a pickup modelled as the gradient of its own flux, which is where the
//!   bark comes from and which reproduces the documented effect of voicing
//!   without being told to;
//! * the amplifier: bass, treble, and the Suitcase's stereo tremolo, which is
//!   a pan between two amp channels rather than an amplitude modulation.
//!
//! ## Why a model
//!
//! crates.io allows 10 MB per crate. One velocity layer every third key is
//! already 8.5 MB and a usable three-layer set is 57 MB, so a sampled Rhodes is
//! not shippable here. It would also be the worse instrument: on a Rhodes,
//! velocity changes the *spectrum* rather than the level, and a layer boundary
//! is a step in a curve that should be continuous.
//!
//! ## Sources
//!
//! * Greg Shear, *The Electromagnetically Sustained Rhodes Piano*, MSc thesis,
//!   Media Arts & Technology, UC Santa Barbara, December 2011. Table 2.1 is the
//!   measured Q of a 1974 Mark I 88 and is what [`SUSTAIN_TAU`] is derived
//!   from; §2.2.1 is Faraday's law at the pickup; §2.2.2 is the voicing
//!   adjustment, quoted at [`VOICING_W_MAX`].
//! * A. Falaize and T. Hélie, "Passive simulation of the nonlinear
//!   port-Hamiltonian modeling of a Rhodes piano", *Journal of Sound and
//!   Vibration* **390** (2017) 289-309 — the hammer / beam / pickup
//!   decomposition this follows.
//! * ISMA 2014, on non-linear behaviour in Rhodes sound production — the
//!   pickup as the source of the added harmonics.
//! * Any beam text for the clamped-free eigenvalues in [`MODE_RATIOS`].
//!
//! Where a number came from one of those it says so at the constant. Where it
//! was fitted by ear or by measurement of this model, it says that instead.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

use crate::level::soft_saturate;

/// A Rhodes is fully polyphonic — 73 or 88 tines, all of them able to ring at
/// once. Sixteen is what the voice allocator can hold without the note-on
/// search becoming the expensive part of a chord, and it is four more than a
/// two-handed voicing with the sustain pedal down.
const MAX_VOICES: usize = 16;

const PI: f64 = std::f64::consts::PI;
const TAU_F64: f64 = std::f64::consts::TAU;

/// Fixed headroom trim on the output, applied after the LEVEL knob.
///
/// Sized on ordinary playing, in step with the other five — see `OUTPUT_TRIM`
/// in dx7.rs, which carries the full reasoning. The trim lands this
/// instrument's median patch at the same loudness as theirs;
/// `instruments_are_level_matched` in tests/headroom.rs is the assertion that
/// holds the six together.
///
/// Measured across all 26 patches on an eight-note chord at velocity 127: the
/// loudest is Hard Bark at 0.520 out of the instrument and the quietest Dyno
/// Ballad at 0.223, so the whole bank stays 3.2 dB or more under the
/// saturator's knee and every patch at every voicing is the trimmed voice sum
/// sample for sample. With the LEVEL knob driven to the top on every patch the
/// worst is still Hard Bark, at 0.703 — under the knee as well.
///
/// What sizes it is the panel rather than the bank: with every control at the
/// top of its travel, an eight-note chord at velocity 127 reaches 0.912 before
/// the bounding stage and 0.848 after it, which is 0.4 dB under the master
/// limiter's ceiling. That setting is not a sound anyone would choose — STRIKE
/// and LEVEL at maximum together — but it is reachable from the panel, and the
/// trim is what keeps it off the limiter.
const OUTPUT_TRIM: f32 = 0.185;

// ── Parameter indices ──
//
// Front-panel order, which for a modelled instrument is signal-flow order:
// hammer, then fork, then pickup, then the amplifier the pickup feeds. `patch`
// is first because index 0 is where the editor looks for a preset selector.

pub const P_PATCH: usize = 0;
// HAMMER
pub const P_HAMMER: usize = 1;
pub const P_STRIKE: usize = 2;
pub const P_VELOCITY: usize = 3;
// FORK
pub const P_BELL: usize = 4;
pub const P_DECAY: usize = 5;
pub const P_BELL_DECAY: usize = 6;
pub const P_TONEBAR: usize = 7;
// PICKUP
pub const P_VOICING: usize = 8;
pub const P_PICKUP: usize = 9;
// AMP
pub const P_BASS: usize = 10;
pub const P_TREBLE: usize = 11;
pub const P_VIBRATO: usize = 12;
pub const P_SPEED: usize = 13;
pub const P_LEVEL: usize = 14;
pub const PARAM_COUNT: usize = 15;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "patch",
    "hammer", "strike", "velocity",
    "bell", "decay", "belldcy", "tonebar",
    "voicing", "pickup",
    "bass", "treble", "vibrato", "speed", "level",
];

/// Patch 0, "MK1 Stage", the panel the instrument loads with.
/// `patch_zero_is_the_default_parameter_block` holds these and the first row of
/// `BANK` together.
pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    0.0,    // patch: MK1 Stage
    0.50,   // hammer
    0.55,   // strike
    0.50,   // velocity
    0.55,   // bell
    0.50,   // decay
    0.50,   // bell decay
    0.55,   // tonebar
    0.45,   // voicing
    0.55,   // pickup
    0.50,   // bass
    0.50,   // treble
    0.0,    // vibrato: a Stage has no vibrato circuit
    0.35,   // speed
    0.62,   // level
];

// ── The fork ──

/// Frequency ratios of a clamped-free cantilever's first five modes.
///
/// The eigenvalues `βₙL` of `cos(βL)·cosh(βL) = -1` are 1.8751, 4.6941, 7.8548,
/// 10.9955 and 14.1372, and a beam's modal frequency goes as `β²`, so the
/// ratios are `(βₙ/β₁)²`. That is what makes a tine ring like a bell and a
/// string ring like a string: a string's overtones are near-integer multiples
/// and a cantilever's are 6.27, 17.5, 34.4 and 56.8 times the fundamental.
///
/// `the_modes_are_a_cantilevers` re-derives these from the eigenvalues rather
/// than trusting the digits typed here.
const MODE_RATIOS: [f64; 5] =
    [1.0, 6.266_893_025_770_668, 17.547_481_936_808_452, 34.386_061_157_203_01, 56.842_622_928_102_03];

/// Clamped-free eigenvalues `βₙL`, kept so the ratios above can be checked
/// against their own derivation.
#[cfg(test)]
const BEAM_EIGENVALUES: [f64; 5] = [
    1.875_104_068_711_961,
    4.694_091_132_974_175,
    7.854_757_438_237_613,
    10.995_540_734_875_467,
    14.137_168_391_046_47,
];

/// Resonators per voice: the tine's fundamental, the tonebar's partner mode,
/// and the four inharmonic overtones. Fixed and bounded — this is the whole
/// reason modal synthesis is safe in a real-time thread.
const MODES: usize = 6;

/// Which entry of [`MODE_RATIOS`] each resonator carries. Slots 0 and 1 are the
/// two normal modes of the coupled fork, so both sit on the fundamental.
const MODE_OF: [usize; MODES] = [0, 0, 1, 2, 3, 4];

/// The tonebar's slot. Its frequency is the fundamental plus the fork's
/// normal-mode split.
const TONEBAR: usize = 1;

/// A mode's contribution to the tine's *displacement* is its contribution to
/// the tip velocity divided by its frequency ratio, because velocity leads
/// displacement by a factor of ω. That is why the pickup's argument is almost
/// a pure fundamental even when the attack is full of bell: the 17.5× mode
/// moves the tip a seventeenth as far as it moves it fast.
const DISPLACEMENT_SCALE: [f64; MODES] = [
    1.0,
    1.0,
    1.0 / MODE_RATIOS[1],
    1.0 / MODE_RATIOS[2],
    1.0 / MODE_RATIOS[3],
    1.0 / MODE_RATIOS[4],
];

/// How much of each mode a hammer blow near the tine's base puts into the
/// fork, before the contact-time filter in [`hammer_weights`] shapes it
/// further.
///
/// The signs are the mode shapes' own: a clamped-free beam's modes alternate
/// sign at the free end, so consecutive overtones push the tip in opposite
/// directions at the instant of the strike. That alternation is what makes the
/// first millisecond of a Rhodes a thud rather than a click.
///
/// The magnitudes are fitted, not measured: the real participation depends on
/// where along the tine the hammer lands and on the tuning spring's mass, and
/// neither is published for a Rhodes.
const MODE_STRIKE: [f64; 5] = [1.0, -0.62, 0.34, -0.20, 0.12];

/// Measured sustain of a 1974 Mark I 88, as the time for the tine's amplitude
/// to fall to 1/e.
///
/// From Q values of 949, 731, 1520, 2175 and 1761 at E♭2 to E♭6, through
/// `Q = π·f₀·τ` — so 3.88 s at E♭2, 1.50 at E♭3, 1.56 at E♭4, 1.11 at E♭5 and
/// 0.45 at E♭6, which is 26.8 s to 3.1 s to −60 dB.
///
/// **Not monotonic**, and deliberately left that way: E♭3 is shorter-lived
/// than both its neighbours and E♭5 has the highest Q on the instrument. A
/// curve fitted through five points would only be smoothing the measurement,
/// so this interpolates between them instead. The irregularity is the
/// instrument.
///
/// Outside E♭2..E♭6 the ends are held rather than extrapolated. The trend
/// below E♭2 is steep enough that continuing it would invent a sustain nobody
/// measured.
const SUSTAIN_NOTES: [f64; 5] = [39.0, 51.0, 63.0, 75.0, 87.0];
const SUSTAIN_TAU: [f64; 5] = [3.88, 1.50, 1.56, 1.11, 0.45];

/// How much faster the inharmonic modes die than the fundamental:
/// `τₙ = τ₀ · ratioₙ^-γ`.
///
/// Fitted rather than measured — the published Q figures are for the
/// fundamental only. Damping in a beam rises with frequency through
/// thermoelastic and air losses, and γ near 1 is the usual order; 1.2 is what
/// puts the bell partial's −60 dB point about a second after the strike in the
/// middle of the keyboard, which is where a Rhodes leaves it.
const BELL_DAMPING_EXPONENT: f64 = 1.2;

/// Bounds on any mode's decay, so that neither a panel extreme nor an
/// out-of-range note can ask for a resonator that never stops or one that
/// finishes inside a single sample.
const TAU_MIN: f64 = 0.0005;
const TAU_MAX: f64 = 30.0;

/// The fork's normal-mode split, as a fraction of the fundamental.
///
/// A tine and its tonebar are two oscillators tuned to the same note and
/// coupled through the mounting block, so the pair has two normal modes a
/// little either side of that note. The pickup sees only the tine, whose
/// motion is the sum of its share of both — which beats, slowly, as energy
/// crosses to the tonebar and back. That undulation is audible on any Rhodes
/// held long enough to hear it.
///
/// 2.6 cents, floored at 0.35 Hz so the bass end beats at a rate a player can
/// hear rather than once per note.
const FORK_SPLIT_RATIO: f64 = 0.001_5;
const FORK_SPLIT_MIN_HZ: f64 = 0.35;

/// The lowest note that has a tonebar at all.
///
/// "On the 88 key piano, the lowest seven tines have no tonebars" — Shear,
/// §6.1. A0 is note 21, so the seven without one are 21 to 27 and the fork
/// starts at 28. Below that there is nothing for the tine to trade energy
/// with, so those notes decay smoothly where the rest of the keyboard
/// undulates.
const FORK_LOWEST_TONEBAR: f64 = 28.0;

/// The largest share of the fundamental the TONEBAR knob can move into the
/// second normal mode. At 0.5 the two modes are equal and the beat nulls
/// completely, which a real fork does not do; 0.32 leaves the undulation about
/// 6 dB deep at its strongest.
const TONEBAR_MAX_SHARE: f64 = 0.32;

// ── The hammer ──

/// Contact-time corner of the hammer, in hertz, across the HAMMER knob.
///
/// A hammer in contact with the tine for time T cannot excite a mode whose
/// period is much shorter than T, so the strike's spectrum rolls off above
/// roughly `1/(πT)`. Soft neoprene at the bottom of the knob, hard at the top.
const HAMMER_FC_MIN: f64 = 700.0;
const HAMMER_FC_MAX: f64 = 2600.0;

/// How far the strike force moves that corner. A harder blow compresses the
/// hammer tip further, which stiffens it and shortens the contact — so the
/// same hammer is effectively harder when it is swung faster.
///
/// This is the first of the two paths by which velocity changes the *spectrum*
/// rather than the level; the pickup is the second.
const HAMMER_VELOCITY_TILT: [f64; 2] = [0.35, 1.15];

// ── The pickup ──
//
// A coil wound round a permanent magnet, with the magnetised tine tip swinging
// through its field. Faraday gives `V = -N·dΦ/dt`, and by the chain rule that
// is `-N·Φ'(u)·u̇` — the flux *gradient* at wherever the tine currently is,
// times how fast it is moving. Both factors are available exactly from the
// modal states, so no differentiator is needed and none of this is a guess
// about what a nonlinearity ought to look like.
//
// The flux linkage falls off sharply with distance from the pickup axis, which
// is modelled as `Φ(w) = (1 + w²)^-2` with `w` the tine's offset from that
// axis in units of the pickup's own width. Two consequences fall straight out,
// and both are the documented behaviour of the real adjustment:
//
// * `Φ'` is an **odd** function of `w`. With the tine's rest position on the
//   axis its motion is symmetric about the peak, so `Φ'(x)·ẋ` contains only
//   *even* harmonics — the fundamental and every odd partial vanish and the
//   second partial is left dominant. That is the hollow, barking voicing.
// * Off the axis the symmetry breaks and the fundamental returns. At
//   `w = 1/√5` the gradient is at its own maximum, where its curvature is
//   zero — the most linear point on the whole transfer, and therefore the
//   mellowest, most fundamental-dominant voicing the pickup has.
//
// So the VOICING knob is one number: where the tine sits.

/// `w` at the gradient's peak: the root of `1 - 5w² = 0`.
const PICKUP_INFLECTION: f64 = 0.447_213_595_499_958;

/// `|Φ'|` there, which normalises the transfer to a gain of at most 1.
/// `4w/(1+w²)³` at `w = 1/√5` is `1.788854/1.728`.
const PICKUP_NORM: f64 = 1.035_216_656;

/// The VOICING knob's travel: from the inflection down to just off the axis.
///
/// Shear, §2.2.2, on what the real adjustment does:
///
/// > As the equilibrium point of the free end of the tine approaches the
/// > pickup axis, the fundamental and all odd partials are attenuated, leaving
/// > the second partial as the dominant frequency in the series. This vertical
/// > adjustment of the tine (in the direction of oscillation) is known as
/// > voicing.
///
/// "In the direction of oscillation" is the part that makes the model
/// one-dimensional: the offset and the swing are the same axis, so they add,
/// and `w = rest + x` is the whole geometry.
///
/// Not to zero. On the axis the linear term vanishes exactly and a quiet note
/// would be inaudible while a loud one barked, which is a discontinuity the
/// real adjustment does not have — a technician can get close to the axis and
/// no closer.
const VOICING_W_MAX: f64 = PICKUP_INFLECTION;
const VOICING_W_MIN: f64 = 0.06;

/// How far the tine tip swings, in the same units as `w`, at the top of the
/// STRIKE knob and full velocity.
///
/// 0.62 takes the tip past the gradient's peak and out the far side on the
/// loud half of the swing and nowhere near it on the quiet half, which is the
/// asymmetry the bark is made of.
const STRIKE_MAX_W: f64 = 0.62;

/// How much further a bass tine swings than a treble one for the same blow.
///
/// A longer tine is a softer spring, so the same hammer velocity moves its tip
/// further and pushes it deeper into the pickup's nonlinearity. Left to the
/// undamped physics that would be a factor of 16 across the keyboard, which is
/// far more than a real instrument shows because the tines are graded and each
/// note is voiced by hand. The exponent below leaves about 2 dB per octave of
/// extra drive toward the bass.
/// ...and the tilt is held flat below the bottom of an 88, because there is no
/// tine down there to be softer. A MIDI file can carry note 0; a Rhodes stops
/// at A0.
const KEY_DRIVE_TILT: f64 = 0.33;
const KEY_DRIVE_LOWEST_NOTE: f64 = 21.0;

/// Middle C: the note the key tilt is referenced to and the note the panel
/// quotes its two time controls at, since both are per-note quantities and a
/// panel has to name one of them.
const REFERENCE_NOTE: f64 = 60.0;

/// Corner of the pickup's own low-pass, across the PICKUP knob.
///
/// The coil is several thousand turns of fine wire; its inductance and the
/// capacitance of the winding and the cable put a corner in the audio band.
/// A passive Stage loaded by a long cable sits at the bottom of this range and
/// an active Suitcase preamp, which loads the coil far less, near the top.
const PICKUP_FC_MIN: f64 = 1_100.0;
const PICKUP_FC_MAX: f64 = 8_000.0;
/// Damping of that two-pole, giving about 1 dB of presence at the corner.
const PICKUP_Q: f64 = 1.1;

// ── The damper ──

/// Time constant of the felt damper falling back on a released tine.
const DAMPER_TAU: f64 = 0.06;

/// Where the dampers start losing their grip, and how far up they take to lose
/// it entirely.
///
/// Every note on a Rhodes has a damper, but they are not the same damper: the
/// modules are graduated across the keyboard and the treble ones are short,
/// narrow felts landing on a short, stiff tine. They take much less energy out
/// of it, which is why the top of the instrument rings on under fast playing
/// where the middle stops dead. Modelled as the damped decay fading back to the
/// tine's own by the top of the keyboard — a simplification at that end, but
/// the tine's own sustain up there is 0.45 s, so the note is gone quickly
/// either way.
const DAMPER_LAST_NOTE: f64 = 84.0;
const DAMPER_FADE_NOTES: f64 = 12.0;

/// Amplitude, relative to the strike, below which a voice is finished and can
/// be reallocated. −60 dB of a signal already trimmed by [`OUTPUT_TRIM`].
const VOICE_FLOOR: f64 = 1.0e-3;

// ── The amplifier ──

/// Corner of the bass shelf and of the treble shelf, and how far the two of
/// them can tilt the band against each other. ±12 dB, which is the span of the
/// two controls on a Suitcase.
const BASS_SHELF_HZ: f64 = 180.0;
const TREBLE_SHELF_HZ: f64 = 2_400.0;
const SHELF_RANGE_DB: f64 = 12.0;

/// The vibrato LFO's range. Roland-style rate markings do not apply here —
/// this is Fender's own circuit, and the sweep on a Suitcase runs from about a
/// third of a hertz to a fast flutter.
const VIBRATO_HZ_MIN: f64 = 0.4;
const VIBRATO_HZ_MAX: f64 = 9.0;

// ── Patches ──

pub const PATCH_COUNT: usize = 26;

/// One patch: a panel position for every control except the selector itself.
#[derive(Debug, Clone, Copy)]
struct Program {
    name: &'static str,
    /// HAMMER, STRIKE, VELOCITY.
    hammer: [f32; 3],
    /// BELL, DECAY, BELL DECAY, TONEBAR.
    fork: [f32; 4],
    /// VOICING, PICKUP.
    pickup: [f32; 2],
    /// BASS, TREBLE, VIBRATO, SPEED, LEVEL.
    amp: [f32; 5],
}

/// The bank, by instrument.
///
/// A Rhodes has no patch memory, so unlike the Juno's or the Jupiter's this is
/// not a factory set — it is the instruments themselves, in the four families
/// worth having, plus the voicings a technician would set up on request.
///
/// * **Mark I** — the 1970s Stage and its variations. Passive pickups straight
///   out of a jack, so the coil is loaded by whatever it is plugged into: the
///   PICKUP knob sits low and the top end is soft.
/// * **Suitcase** — the same piano with an active preamp and the stereo
///   vibrato, into a 4×12. Fuller bottom, more top, and the tremolo.
/// * **Mark II** — 1979 onward. Harder hammer tips and a flat top; a little
///   tighter and less bell than a Mark I.
/// * **Dyno** — the Dyno-My-Rhodes modification. The tines are re-voiced close
///   to the pickup and the preamp lifts the top, which is why a Dyno is the
///   bright, bell-forward Rhodes of every early-eighties ballad.
/// * **Character** — the ends of the voicing adjustment, which is a real thing
///   a technician does per note and the difference between a mellow Rhodes and
///   a barking one.
const BANK: [Program; PATCH_COUNT] = [
    // ── Mark I ──
    Program { name: "MK1 Stage",
        hammer: [0.50, 0.55, 0.50], fork: [0.55, 0.50, 0.50, 0.55],
        pickup: [0.45, 0.55], amp: [0.50, 0.50, 0.00, 0.35, 0.62] },
    Program { name: "MK1 Bright",
        hammer: [0.72, 0.60, 0.55], fork: [0.72, 0.50, 0.58, 0.50],
        pickup: [0.58, 0.78], amp: [0.42, 0.68, 0.00, 0.35, 0.58] },
    Program { name: "MK1 Mellow",
        hammer: [0.22, 0.42, 0.45], fork: [0.32, 0.55, 0.38, 0.62],
        pickup: [0.15, 0.32], amp: [0.60, 0.32, 0.00, 0.30, 0.72] },
    Program { name: "MK1 Bark",
        hammer: [0.66, 0.82, 0.62], fork: [0.62, 0.48, 0.52, 0.48],
        pickup: [0.92, 0.62], amp: [0.48, 0.58, 0.00, 0.35, 0.76] },
    Program { name: "MK1 Ballad",
        hammer: [0.34, 0.46, 0.42], fork: [0.44, 0.62, 0.44, 0.68],
        pickup: [0.26, 0.44], amp: [0.58, 0.44, 0.00, 0.28, 0.68] },
    Program { name: "MK1 Funk",
        hammer: [0.62, 0.72, 0.70], fork: [0.68, 0.36, 0.60, 0.42],
        pickup: [0.74, 0.66], amp: [0.44, 0.62, 0.00, 0.42, 0.66] },
    Program { name: "MK1 Bass",
        hammer: [0.44, 0.66, 0.52], fork: [0.48, 0.58, 0.42, 0.58],
        pickup: [0.55, 0.38], amp: [0.72, 0.34, 0.00, 0.32, 0.60] },
    // ── Suitcase ──
    Program { name: "SC Classic",
        hammer: [0.50, 0.58, 0.52], fork: [0.58, 0.52, 0.52, 0.60],
        pickup: [0.44, 0.70], amp: [0.62, 0.58, 0.00, 0.38, 0.58] },
    Program { name: "SC Tremolo",
        hammer: [0.50, 0.58, 0.52], fork: [0.58, 0.52, 0.52, 0.60],
        pickup: [0.44, 0.70], amp: [0.62, 0.58, 0.72, 0.52, 0.58] },
    Program { name: "SC SlowTrem",
        hammer: [0.46, 0.54, 0.48], fork: [0.50, 0.58, 0.46, 0.64],
        pickup: [0.36, 0.62], amp: [0.66, 0.52, 0.92, 0.16, 0.60] },
    Program { name: "SC Deep",
        hammer: [0.42, 0.52, 0.46], fork: [0.46, 0.62, 0.42, 0.66],
        pickup: [0.30, 0.56], amp: [0.78, 0.42, 0.55, 0.30, 0.64] },
    Program { name: "SC Warm",
        hammer: [0.30, 0.48, 0.44], fork: [0.38, 0.60, 0.40, 0.66],
        pickup: [0.20, 0.46], amp: [0.70, 0.34, 0.30, 0.26, 0.70] },
    Program { name: "SC 88",
        hammer: [0.54, 0.60, 0.54], fork: [0.60, 0.46, 0.56, 0.54],
        pickup: [0.50, 0.74], amp: [0.56, 0.62, 0.24, 0.44, 0.58] },
    // ── Mark II ──
    Program { name: "MK2 Stage",
        hammer: [0.58, 0.54, 0.50], fork: [0.50, 0.44, 0.58, 0.48],
        pickup: [0.48, 0.60], amp: [0.46, 0.54, 0.00, 0.35, 0.62] },
    Program { name: "MK2 Tight",
        hammer: [0.66, 0.56, 0.58], fork: [0.46, 0.34, 0.64, 0.40],
        pickup: [0.56, 0.64], amp: [0.44, 0.58, 0.00, 0.38, 0.62] },
    Program { name: "MK2 Dark",
        hammer: [0.30, 0.50, 0.46], fork: [0.34, 0.48, 0.44, 0.56],
        pickup: [0.24, 0.34], amp: [0.62, 0.28, 0.00, 0.30, 0.72] },
    Program { name: "MK2 Suitcase",
        hammer: [0.58, 0.56, 0.52], fork: [0.52, 0.46, 0.58, 0.54],
        pickup: [0.46, 0.72], amp: [0.60, 0.60, 0.58, 0.46, 0.58] },
    // ── Dyno ──
    Program { name: "Dyno",
        hammer: [0.76, 0.62, 0.56], fork: [0.80, 0.50, 0.66, 0.46],
        pickup: [0.66, 0.86], amp: [0.54, 0.76, 0.00, 0.35, 0.52] },
    Program { name: "Dyno Bell",
        hammer: [0.88, 0.64, 0.60], fork: [0.92, 0.52, 0.74, 0.44],
        pickup: [0.72, 0.92], amp: [0.50, 0.82, 0.00, 0.35, 0.50] },
    Program { name: "Dyno Ballad",
        hammer: [0.68, 0.52, 0.46], fork: [0.74, 0.62, 0.60, 0.60],
        pickup: [0.52, 0.80], amp: [0.62, 0.70, 0.36, 0.22, 0.54] },
    Program { name: "Dyno Bright",
        hammer: [0.94, 0.66, 0.64], fork: [0.88, 0.46, 0.78, 0.42],
        pickup: [0.80, 0.96], amp: [0.44, 0.88, 0.00, 0.35, 0.48] },
    // ── Character ──
    Program { name: "Bell Tine",
        hammer: [0.84, 0.58, 0.58], fork: [1.00, 0.54, 0.82, 0.50],
        pickup: [0.60, 0.90], amp: [0.46, 0.78, 0.00, 0.35, 0.50] },
    Program { name: "Hard Bark",
        hammer: [0.78, 0.94, 0.72], fork: [0.70, 0.44, 0.56, 0.44],
        pickup: [1.00, 0.70], amp: [0.50, 0.64, 0.00, 0.35, 0.74] },
    Program { name: "Soft Silk",
        hammer: [0.10, 0.36, 0.40], fork: [0.18, 0.64, 0.30, 0.70],
        pickup: [0.08, 0.24], amp: [0.64, 0.24, 0.20, 0.22, 0.86] },
    Program { name: "Woody",
        hammer: [0.40, 0.68, 0.54], fork: [0.52, 0.30, 0.34, 0.52],
        pickup: [0.68, 0.40], amp: [0.56, 0.40, 0.00, 0.35, 0.76] },
    Program { name: "Growl Bass",
        hammer: [0.52, 0.80, 0.66], fork: [0.44, 0.56, 0.48, 0.56],
        pickup: [0.86, 0.44], amp: [0.74, 0.30, 0.00, 0.35, 0.66] },
];

/// The patch names, which are short enough to be the panel's labels as well.
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

// ── Discrete controls ──
//
// One: the patch selector. Everything else on this panel is a continuous
// adjustment of a physical quantity, which is what a modelled instrument's
// panel is.

fn discrete_steps(index: usize) -> Option<usize> {
    match index {
        P_PATCH => Some(PATCH_COUNT),
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

/// The knob position one step up or down from `value`. Sliders are unchanged.
///
/// Steps by *index* rather than by adding a fraction of the travel: adding
/// 1/26 of the range 26 times does not arrive at 1.0, and a step boundary
/// missed by one ulp is a keypress that visibly does nothing.
#[must_use]
pub fn step_discrete(index: usize, value: f32, up: bool) -> f32 {
    let Some(count) = discrete_steps(index) else { return value };
    let current = selector(value, count);
    knob_for(
        if up { (current + 1).min(count - 1) } else { current.saturating_sub(1) },
        count,
    )
}

/// Label for a selector position, or `None` for a continuous control.
#[must_use]
pub fn discrete_label(index: usize, value: f32) -> Option<&'static str> {
    let count = discrete_steps(index)?;
    match index {
        P_PATCH => Some(PATCH_NAMES[selector(value, count)]),
        _ => None,
    }
}

/// A control's value in seconds, for the two that measure time.
///
/// Both are scalings of a measured table rather than absolute times, so the
/// number reported is what that setting means at middle C — the note in the
/// middle of the measurements.
#[must_use]
pub fn param_seconds(index: usize, value: f32) -> Option<f64> {
    let tau = sustain_tau(REFERENCE_NOTE);
    match index {
        P_DECAY => Some(tau * decay_scale(f64::from(value))),
        P_BELL_DECAY => Some(bell_tau(tau, MODE_RATIOS[1], f64::from(value))),
        _ => None,
    }
}

// ── Panel tapers ──

/// Straight-line interpolation into the measured sustain table, in MIDI note
/// numbers. Held flat outside the measured range — see [`SUSTAIN_TAU`].
fn sustain_tau(note: f64) -> f64 {
    if note <= SUSTAIN_NOTES[0] {
        return SUSTAIN_TAU[0];
    }
    for i in 1..SUSTAIN_NOTES.len() {
        if note <= SUSTAIN_NOTES[i] {
            let span = SUSTAIN_NOTES[i] - SUSTAIN_NOTES[i - 1];
            let f = (note - SUSTAIN_NOTES[i - 1]) / span;
            return SUSTAIN_TAU[i - 1] + f * (SUSTAIN_TAU[i] - SUSTAIN_TAU[i - 1]);
        }
    }
    SUSTAIN_TAU[SUSTAIN_TAU.len() - 1]
}

/// The DECAY knob as a multiplier on the measured table: a quarter of the
/// instrument's own sustain at the bottom, four times it at the top, and the
/// measurement itself at the centre detent.
fn decay_scale(knob: f64) -> f64 {
    4.0f64.powf(2.0 * knob.clamp(0.0, 1.0) - 1.0)
}

/// An inharmonic mode's decay, from the fundamental's and the mode's ratio.
///
/// The BELL DECAY knob scales it either way by a factor of three, so a patch
/// can hold the bell into the body of the note or cut it back to a strike
/// transient.
fn bell_tau(fundamental_tau: f64, ratio: f64, knob: f64) -> f64 {
    let scale = 3.0f64.powf(2.0 * knob.clamp(0.0, 1.0) - 1.0);
    (fundamental_tau * ratio.powf(-BELL_DAMPING_EXPONENT) * scale).clamp(TAU_MIN, TAU_MAX)
}

/// Where the hammer's contact-time corner sits, given the knob and how hard
/// the key was struck.
fn hammer_corner(knob: f64, velocity: f64) -> f64 {
    let base = HAMMER_FC_MIN + knob.clamp(0.0, 1.0) * (HAMMER_FC_MAX - HAMMER_FC_MIN);
    base * (HAMMER_VELOCITY_TILT[0] + HAMMER_VELOCITY_TILT[1] * velocity.clamp(0.0, 1.0))
}

/// How much of each cantilever mode a strike of this force puts into the fork.
///
/// Two factors: the mode's own participation from [`MODE_STRIKE`], and the
/// contact-time roll-off `1/(1 + (f/f_c)²)`, normalised so the fundamental
/// always comes out at 1. The second factor is in *absolute* frequency, which
/// is why the top of the keyboard is nearly a pure tone however hard it is
/// struck and the bottom is full of bell — the same hammer, the same contact
/// time, a fundamental an order of magnitude lower.
fn hammer_weights(f0: f64, corner: f64, bell: f64) -> [f64; 5] {
    let roll = |f: f64| 1.0 / (1.0 + (f / corner) * (f / corner));
    let reference = roll(f0);
    let mut out = [0.0; 5];
    for (i, w) in out.iter_mut().enumerate() {
        let relative = roll(f0 * MODE_RATIOS[i]) / reference;
        *w = MODE_STRIKE[i] * relative * if i == 0 { 1.0 } else { bell };
    }
    out
}

/// The pickup's normalised flux gradient at offset `w`.
///
/// `Φ(w) = (1 + w²)^-2`, so `Φ'(w) = -4w/(1 + w²)³`; the sign is dropped
/// because the winding direction is arbitrary, and the magnitude is divided by
/// its own peak so the transfer has a gain of at most one.
///
/// Bounded by construction and finite everywhere: the denominator is at least
/// 1 and the whole expression tends to zero as the tine leaves the field. A
/// pickup cannot produce more signal by being hit harder past a point, which
/// is a real property of the instrument and not a limiter bolted on.
#[inline]
fn pickup_gradient(w: f64) -> f64 {
    let d = 1.0 + w * w;
    4.0 * w / (d * d * d * PICKUP_NORM)
}

/// Note to hertz.
fn note_to_freq(note: f64) -> f64 {
    440.0 * 2.0f64.powf((note - 69.0) / 12.0)
}

/// A shelf gain from a knob: unity at the centre detent, ±`SHELF_RANGE_DB` at
/// the ends.
fn shelf_gain(knob: f64) -> f64 {
    10.0f64.powf(SHELF_RANGE_DB * (2.0 * knob.clamp(0.0, 1.0) - 1.0) / 20.0)
}

/// The two shelf gains, normalised so that neither band leaves the amplifier
/// louder than it arrived.
///
/// The tone controls here can only take away. On a Suitcase the preamp's EQ
/// boosts as well and the VOLUME control after it puts the level back; the
/// LEVEL knob is that control, and normalising the pair means the tone section
/// cannot be turned into a second fader by accident. Musically nothing is
/// lost — a tilt is a tilt, and both controls at the top is the same flat
/// response a passive Fender stack gives for the same reason.
///
/// It is also what bounds the amplifier: the pickup's own transfer is at most
/// unity by construction, the stack is at most unity by this, and the pan law
/// is normalised to a peak of one — so the only gain anywhere after the fork is
/// the LEVEL knob and the fixed trim, both of which are measured.
fn tone_gains(bass_knob: f64, treble_knob: f64) -> (f64, f64) {
    let bass = shelf_gain(bass_knob);
    let treble = shelf_gain(treble_knob);
    let norm = bass.max(treble).max(1.0);
    (bass / norm, treble / norm)
}

// ── Internal preset ──
//
// Physical units, because that is what the engine runs on. The panel block is
// the f32 parameter vector; `params_for_patch` loads a row of `BANK` into it
// and `active_patch` reads the whole block back out in these units, so every
// control stays live after a preset is loaded.

#[derive(Debug, Clone, Copy)]
struct RhodesPatch {
    // HAMMER
    hammer: f64,
    strike: f64,
    velocity_curve: f64,
    // FORK
    bell: f64,
    decay: f64,
    bell_decay: f64,
    tonebar: f64,
    // PICKUP
    voicing: f64,
    pickup_hz: f64,
    // AMP
    bass: f64,
    treble: f64,
    vibrato: f64,
    speed: f64,
    level: f64,
}

// ── Resonator ──
//
// One mode is one complex number rotated by its own angle each sample and
// scaled by its own decay. The real part is the mode's share of the tip's
// velocity and the imaginary part its share of the displacement, in quadrature
// as they must be — so the pickup gets both of the quantities Faraday needs
// without a differentiator anywhere in the path.
//
// A rotation is four multiplies and two adds, it cannot go unstable while the
// decay is under one, and it hits its frequency exactly rather than to within
// a bilinear warp. That is the whole argument for modal synthesis in a
// real-time thread: the cost is fixed, bounded and known before the note
// starts.

#[derive(Debug, Clone, Copy, Default)]
struct Resonator {
    re: f64,
    im: f64,
    /// `decay·cos(ω/sr)` and `decay·sin(ω/sr)`, the rotation with its decay
    /// already folded in.
    a: f64,
    b: f64,
    /// Kept apart so the damper can change the decay without recomputing a
    /// trigonometric function per note-off.
    cos_w: f64,
    sin_w: f64,
}

impl Resonator {
    fn tune(&mut self, freq: f64, tau: f64, sr: f64) {
        let w = TAU_F64 * freq / sr;
        self.cos_w = w.cos();
        self.sin_w = w.sin();
        self.set_decay(decay_per_sample(tau, sr));
    }

    #[inline]
    fn set_decay(&mut self, decay: f64) {
        self.a = decay * self.cos_w;
        self.b = decay * self.sin_w;
    }

    #[inline]
    fn strike(&mut self, amplitude: f64) {
        // A struck beam starts with velocity and no displacement, so the whole
        // of the initial state is in the real part. The tine's position is
        // continuous through the note-on, which is why the attack has no click
        // in it that the modes did not put there.
        self.re = amplitude;
        self.im = 0.0;
    }

    #[inline]
    fn step(&mut self) {
        let re = self.re * self.a - self.im * self.b;
        let im = self.re * self.b + self.im * self.a;
        self.re = re;
        self.im = im;
    }

    fn clear(&mut self) {
        self.re = 0.0;
        self.im = 0.0;
    }
}

/// Per-sample amplitude multiplier for a 1/e time of `tau`.
fn decay_per_sample(tau: f64, sr: f64) -> f64 {
    (-1.0 / (tau.clamp(TAU_MIN, TAU_MAX) * sr)).exp()
}

// ── Pickup coil low-pass ──
//
// Topology-preserving two-pole, so the corner lands where it is asked for
// rather than half an octave below its own coefficient.

/// The three coefficients the two-pole needs, worked out once per block
/// because the corner only moves when a knob does. The tangent behind them is
/// not something to evaluate per sample for an answer that changes when a
/// finger does.
#[derive(Debug, Clone, Copy, Default)]
struct CoilCoeffs {
    a1: f64,
    a2: f64,
    a3: f64,
}

fn coil_coeffs(freq: f64, sr: f64) -> CoilCoeffs {
    let g = (PI * freq.clamp(20.0, sr * 0.45) / sr).tan();
    let k = 1.0 / PICKUP_Q;
    let a1 = 1.0 / (1.0 + g * (g + k));
    let a2 = g * a1;
    CoilCoeffs { a1, a2, a3: g * a2 }
}

#[derive(Debug, Clone, Copy, Default)]
struct CoilFilter {
    ic1: f64,
    ic2: f64,
}

impl CoilFilter {
    #[inline]
    fn process(&mut self, x: f64, c: &CoilCoeffs) -> f64 {
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
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
}

// ── Tone stack ──
//
// Two first-order shelves, which is what the two controls on a Suitcase are.
// A one-pole split gives both halves of the band from one state variable, so
// each shelf is a multiply on each half and an add.

#[derive(Debug, Clone, Copy, Default)]
struct Shelf {
    state: f64,
}

/// `g/(1+g)` for a one-pole at `freq`, worked out once per block for the same
/// reason [`coil_coeffs`] is.
fn shelf_coeff(freq: f64, sr: f64) -> f64 {
    let g = (PI * freq.min(sr * 0.45) / sr).tan();
    g / (1.0 + g)
}

impl Shelf {
    #[inline]
    fn process(&mut self, x: f64, gg: f64, low_gain: f64, high_gain: f64) -> f64 {
        let v = (x - self.state) * gg;
        let lp = v + self.state;
        self.state = lp + v;
        if self.state.abs() < 1e-18 { self.state = 0.0; }
        lp * low_gain + (x - lp) * high_gain
    }

    fn reset(&mut self) {
        self.state = 0.0;
    }
}

// ── Voice ──

#[derive(Debug, Clone)]
struct RhodesVoice {
    modes: [Resonator; MODES],
    note: u8,
    age: u64,
    /// Whether the key is down.
    held: bool,
    /// Whether the sustain pedal is holding this note after the key came up.
    pedalled: bool,
    /// Analytic estimate of what is left of the strike, used to decide when
    /// the voice is finished. Follows whichever decay is currently the slowest
    /// thing in the voice, which is the fundamental's or the damper's.
    envelope: f64,
    envelope_decay: f64,
    /// The fundamental's undamped per-sample decay, kept so the damper can be
    /// lifted again by the pedal.
    free_decay: f64,
    /// The damper's, or the free decay when this note has no damper.
    damped_decay: f64,
    /// How far this note's tip swings, in pickup units.
    drive: f64,
    sample_rate: f64,
}

impl RhodesVoice {
    fn new(sr: f64) -> Self {
        Self {
            modes: [Resonator::default(); MODES],
            note: 255,
            age: 0,
            held: false,
            pedalled: false,
            envelope: 0.0,
            envelope_decay: 0.0,
            free_decay: 0.0,
            damped_decay: 0.0,
            drive: 0.0,
            sample_rate: sr,
        }
    }

    fn is_sounding(&self) -> bool {
        self.envelope > VOICE_FLOOR
    }

    fn is_held(&self) -> bool {
        self.held || self.pedalled
    }

    fn kill(&mut self) {
        self.note = 255;
        self.held = false;
        self.pedalled = false;
        self.envelope = 0.0;
        for m in &mut self.modes {
            m.clear();
        }
    }

    fn note_on(&mut self, note: u8, velocity: u8, patch: &RhodesPatch, age: u64) {
        let sr = self.sample_rate;
        self.note = note;
        self.age = age;
        self.held = true;
        self.pedalled = false;

        let key = f64::from(note);
        let f0 = note_to_freq(key);

        // Velocity, through the sensitivity curve. The exponent runs from a
        // compressed response an organ player would recognise to one wider
        // than the keyboard sends.
        let raw = f64::from(velocity) / 127.0;
        let shaped = raw.powf(patch.velocity_curve);

        // Two independent things come out of the strike, and this is the point
        // of the whole model: how far the tip swings, which the pickup turns
        // into harmonics, and how much of the inharmonic modes the hammer
        // leaves behind, which the contact time decides. Neither of them is a
        // brightness control.
        let corner = hammer_corner(patch.hammer, raw);
        let weights = hammer_weights(f0, corner, patch.bell);

        // The bass end swings further for the same blow — see KEY_DRIVE_TILT.
        let reference = note_to_freq(REFERENCE_NOTE);
        let tilt = (reference / f0.max(note_to_freq(KEY_DRIVE_LOWEST_NOTE))).powf(KEY_DRIVE_TILT);
        self.drive = STRIKE_MAX_W * patch.strike * shaped * tilt;

        let tau0 = sustain_tau(key) * patch.decay;
        self.free_decay = decay_per_sample(tau0, sr);
        self.damped_decay = self.damper_decay(key, tau0, sr);

        // The fork's two normal modes. A strike on the tine excites both, and
        // the tine's own motion is their sum — which beats as energy crosses
        // to the tonebar and back. Sharing one unit between them rather than
        // adding a second one keeps the attack level independent of the knob.
        let share = if key < FORK_LOWEST_TONEBAR {
            0.0
        } else {
            patch.tonebar * TONEBAR_MAX_SHARE
        };
        let split = (f0 * FORK_SPLIT_RATIO).max(FORK_SPLIT_MIN_HZ);
        let nyquist = sr * 0.40;

        for (slot, (resonator, &mode)) in self.modes.iter_mut().zip(MODE_OF.iter()).enumerate() {
            let (freq, tau, amplitude) = if slot == TONEBAR {
                (f0 + split, tau0, weights[0] * share)
            } else if mode == 0 {
                (f0, tau0, weights[0] * (1.0 - share))
            } else {
                let f = f0 * MODE_RATIOS[mode];
                (f, bell_tau(tau0, MODE_RATIOS[mode], patch.bell_decay), weights[mode])
            };
            resonator.tune(freq.min(nyquist), tau, sr);
            // A mode above the band cannot be represented, so it is not struck
            // at all rather than folded back down as an alias.
            resonator.strike(if freq <= nyquist { amplitude } else { 0.0 });
        }

        self.envelope = 1.0;
        self.envelope_decay = self.free_decay;
    }

    /// The per-sample decay a released tine falls to — see
    /// [`DAMPER_LAST_NOTE`] for why it depends on which tine.
    fn damper_decay(&self, key: f64, tau0: f64, sr: f64) -> f64 {
        let past = ((key - DAMPER_LAST_NOTE) / DAMPER_FADE_NOTES).clamp(0.0, 1.0);
        if past >= 1.0 {
            return decay_per_sample(tau0, sr);
        }
        let tau = DAMPER_TAU + past * (tau0 - DAMPER_TAU).max(0.0);
        decay_per_sample(tau.min(tau0), sr)
    }

    /// Key up. The tine keeps ringing under the pedal, and under the damper
    /// otherwise.
    fn note_off(&mut self, pedal: bool) {
        self.held = false;
        if pedal {
            self.pedalled = true;
        } else {
            self.damp();
        }
    }

    fn damp(&mut self) {
        self.pedalled = false;
        for m in &mut self.modes {
            m.set_decay(self.damped_decay);
        }
        self.envelope_decay = self.damped_decay;
    }

    /// One sample of tine and pickup. Returns the pickup's output voltage.
    #[inline]
    fn tick(&mut self, rest: f64) -> f64 {
        // Displacement and velocity of the tip, both from the same states.
        let mut x = 0.0;
        let mut v = 0.0;
        for (slot, m) in self.modes.iter_mut().enumerate() {
            m.step();
            x += m.im * DISPLACEMENT_SCALE[slot];
            v += m.re;
        }
        self.envelope *= self.envelope_decay;

        // Faraday: the flux gradient where the tine currently is, times how
        // fast it is going. The rest position is the voicing adjustment.
        pickup_gradient(rest + x * self.drive) * v * self.drive
    }
}

// ── Rhodes ──

pub struct RhodesPiano {
    voices: Vec<RhodesVoice>,
    coil: CoilFilter,
    bass: Shelf,
    treble: Shelf,
    vibrato_phase: f64,
    sample_rate: f64,
    pub params: [f32; PARAM_COUNT],
    voice_counter: u64,
    pedal: bool,
    last_patch_index: usize,
}

impl RhodesPiano {
    #[must_use]
    pub fn new() -> Self {
        Self {
            voices: Vec::new(),
            coil: CoilFilter::default(),
            bass: Shelf::default(),
            treble: Shelf::default(),
            vibrato_phase: 0.0,
            sample_rate: 44_100.0,
            params: PARAM_DEFAULTS,
            voice_counter: 0,
            pedal: false,
            last_patch_index: 0,
        }
    }

    fn current_patch_index(&self) -> usize {
        selector(self.params[P_PATCH], PATCH_COUNT)
    }

    /// The whole panel as this patch sets it.
    #[must_use]
    pub fn params_for_patch(patch_value: f32) -> [f32; PARAM_COUNT] {
        let p = &BANK[selector(patch_value, PATCH_COUNT)];
        let mut params = [0.0f32; PARAM_COUNT];
        params[P_PATCH] = patch_value;
        params[P_HAMMER] = p.hammer[0];
        params[P_STRIKE] = p.hammer[1];
        params[P_VELOCITY] = p.hammer[2];
        params[P_BELL] = p.fork[0];
        params[P_DECAY] = p.fork[1];
        params[P_BELL_DECAY] = p.fork[2];
        params[P_TONEBAR] = p.fork[3];
        params[P_VOICING] = p.pickup[0];
        params[P_PICKUP] = p.pickup[1];
        params[P_BASS] = p.amp[0];
        params[P_TREBLE] = p.amp[1];
        params[P_VIBRATO] = p.amp[2];
        params[P_SPEED] = p.amp[3];
        params[P_LEVEL] = p.amp[4];
        params
    }

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

    /// The panel as it stands, in the units the engine works in. Every control
    /// is live — the patch is only where the knobs started.
    fn active_patch(&self) -> RhodesPatch {
        let p = &self.params;
        let voicing = f64::from(p[P_VOICING]).clamp(0.0, 1.0);
        let (bass, treble) = tone_gains(f64::from(p[P_BASS]), f64::from(p[P_TREBLE]));
        RhodesPatch {
            hammer: f64::from(p[P_HAMMER]),
            strike: f64::from(p[P_STRIKE]),
            // 0.45 at the centre detent, which is close enough to linear that
            // a player's idea of "harder" and the model's agree.
            velocity_curve: 0.45 + 1.6 * f64::from(p[P_VELOCITY]),
            bell: f64::from(p[P_BELL]),
            decay: decay_scale(f64::from(p[P_DECAY])),
            bell_decay: f64::from(p[P_BELL_DECAY]),
            tonebar: f64::from(p[P_TONEBAR]),
            voicing: VOICING_W_MAX + voicing * (VOICING_W_MIN - VOICING_W_MAX),
            pickup_hz: PICKUP_FC_MIN
                + f64::from(p[P_PICKUP]).clamp(0.0, 1.0) * (PICKUP_FC_MAX - PICKUP_FC_MIN),
            bass,
            treble,
            vibrato: f64::from(p[P_VIBRATO]).clamp(0.0, 1.0),
            speed: VIBRATO_HZ_MIN
                + f64::from(p[P_SPEED]).clamp(0.0, 1.0) * (VIBRATO_HZ_MAX - VIBRATO_HZ_MIN),
            level: f64::from(p[P_LEVEL]),
        }
    }

    fn next_age(&mut self) -> u64 {
        self.voice_counter += 1;
        self.voice_counter
    }

    /// A free voice, or the oldest one that is only ringing out, or the oldest
    /// of all. A note already sounding on the same key is re-used rather than
    /// stacked on: a hammer that strikes a moving tine takes energy out of it
    /// as well as putting energy in, so a tremolando does not build without
    /// bound on the real instrument either.
    fn allocate_voice(&mut self, note: u8) -> usize {
        if let Some(i) = self.voices.iter().position(|v| v.note == note && v.is_sounding()) {
            return i;
        }
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
        let pedal = self.pedal;
        for v in &mut self.voices {
            if v.note == note && v.held {
                v.note_off(pedal);
            }
        }
    }

    fn set_pedal(&mut self, down: bool) {
        self.pedal = down;
        if down {
            return;
        }
        for v in &mut self.voices {
            if v.pedalled {
                v.damp();
            }
        }
    }

    fn kill_all(&mut self) {
        for v in &mut self.voices {
            v.kill();
        }
        self.pedal = false;
        self.coil.reset();
        self.bass.reset();
        self.treble.reset();
    }
}

impl Default for RhodesPiano {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for RhodesPiano {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "Rhodes".into(),
            version: "0.1.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..MAX_VOICES).map(|_| RhodesVoice::new(sample_rate)).collect();
        self.coil.reset();
        self.bass.reset();
        self.treble.reset();
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() {
            return;
        }

        let buf_len = outputs[0].len();
        let patch = self.active_patch();
        let sr = self.sample_rate;
        let gain = (patch.level as f32) * OUTPUT_TRIM;

        // Constant-power pan between the two amp channels, normalised so that
        // whichever channel is loudest reaches the same level whatever the
        // depth. Without it the vibrato would be a 3 dB level control as well
        // as a pan, and the dry patches would pay for the wet ones' headroom.
        let swing = PI * 0.25 * patch.vibrato;
        let pan_norm = 1.0 / (PI * 0.25 - swing).cos();

        // Filter coefficients, once per block.
        let coil = coil_coeffs(patch.pickup_hz, sr);
        let bass_g = shelf_coeff(BASS_SHELF_HZ, sr);
        let treble_g = shelf_coeff(TREBLE_SHELF_HZ, sr);

        // MIDI event sorting (allocation-free)
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

        let stereo = outputs.len() >= 2;

        for i in 0..buf_len {
            while ei < event_count && midi_events[event_indices[ei]].sample_offset as usize <= i {
                let ev = &midi_events[event_indices[ei]];
                match ev.status & 0xF0 {
                    0x90 => {
                        if ev.data2 > 0 {
                            let age = self.next_age();
                            let idx = self.allocate_voice(ev.data1);
                            // `get_mut` rather than an index: `process` is
                            // reachable before `init` has built the voices, and
                            // an out-of-range index in the audio thread is a
                            // panic in a callback that cannot catch it.
                            if let Some(voice) = self.voices.get_mut(idx) {
                                voice.note_on(ev.data1, ev.data2, &patch, age);
                            }
                        } else {
                            self.release_note(ev.data1);
                        }
                    }
                    0x80 => self.release_note(ev.data1),
                    0xB0 => match ev.data1 {
                        // Sustain pedal. A Rhodes has one, and it does what a
                        // piano's does: it holds the dampers off the tines.
                        64 => self.set_pedal(ev.data2 >= 64),
                        120 => self.kill_all(),
                        123 => {
                            self.pedal = false;
                            for v in &mut self.voices {
                                if v.is_held() {
                                    v.note_off(false);
                                }
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
                ei += 1;
            }

            let mut sum = 0.0;
            for v in &mut self.voices {
                if v.is_sounding() {
                    sum += v.tick(patch.voicing);
                }
            }

            // The coil, then the amplifier's two shelves.
            let mut signal = self.coil.process(sum, &coil);
            signal = self.bass.process(signal, bass_g, patch.bass, 1.0);
            signal = self.treble.process(signal, treble_g, 1.0, patch.treble);

            // The Suitcase's vibrato: a pan between two amp channels rather
            // than an amplitude modulation, which is why it never gets quieter
            // in the middle of a sweep.
            self.vibrato_phase += patch.speed / sr;
            if self.vibrato_phase >= 1.0 {
                self.vibrato_phase -= 1.0;
            }
            let angle = PI * 0.25 + swing * (self.vibrato_phase * TAU_F64).sin();
            let left = (signal * angle.cos() * pan_norm) as f32;
            let right = (signal * angle.sin() * pan_norm) as f32;

            // Bound both channels without hard clipping them. The trim keeps
            // the whole bank under the knee, so this is the identity for
            // everything except a patch pushed past it by the level knob.
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
                P_DECAY | P_BELL_DECAY => "s".into(),
                P_SPEED => "Hz".into(),
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
        self.vibrato_phase = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 44_100.0;

    fn note_on(note: u8, vel: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x90, data1: note, data2: vel }
    }
    fn note_off(note: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x80, data1: note, data2: 0 }
    }
    fn cc(number: u8, value: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0xB0, data1: number, data2: value }
    }

    /// `count` buffers of 64 samples, the events landing in the first.
    fn process_buffers(synth: &mut RhodesPiano, events: &[MidiEvent], count: usize) -> Vec<f32> {
        let mut all = Vec::with_capacity(count * 64);
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

    /// Both channels, for the tests that are about the amplifier's two of them.
    fn process_stereo(
        synth: &mut RhodesPiano,
        events: &[MidiEvent],
        count: usize,
    ) -> (Vec<f32>, Vec<f32>) {
        let mut left = Vec::new();
        let mut right = Vec::new();
        let mut l = vec![0.0f32; 64];
        let mut r = vec![0.0f32; 64];
        for block in 0..count {
            l.fill(0.0);
            r.fill(0.0);
            let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
            synth.process(&[], &mut outs, if block == 0 { events } else { &[] });
            left.extend_from_slice(&l);
            right.extend_from_slice(&r);
        }
        (left, right)
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().map(|v| v.abs()).fold(0.0f32, f32::max)
    }

    /// Amplitude at `hz`, Hann-windowed so that two partials a few percent
    /// apart do not read as each other.
    fn magnitude_at(x: &[f32], hz: f64) -> f64 {
        let n = x.len() as f64;
        let w = TAU_F64 * hz / SR;
        let (mut re, mut im) = (0.0, 0.0);
        let mut window_sum = 0.0;
        for (i, v) in x.iter().enumerate() {
            let t = i as f64;
            let win = 0.5 - 0.5 * (TAU_F64 * t / n).cos();
            window_sum += win;
            re += f64::from(*v) * win * (w * t).cos();
            im -= f64::from(*v) * win * (w * t).sin();
        }
        2.0 * (re * re + im * im).sqrt() / window_sum
    }

    fn db(x: f64) -> f64 {
        if x <= 0.0 { -300.0 } else { 20.0 * x.log10() }
    }

    /// One note into a fresh instrument, mono, `blocks` of 64 samples.
    fn render_note(patch: usize, note: u8, velocity: u8, blocks: usize) -> Vec<f32> {
        let mut s = RhodesPiano::new();
        s.init(SR, 64);
        s.set_parameter(P_PATCH, patch_knob(patch));
        process_buffers(&mut s, &[note_on(note, velocity, 0)], blocks)
    }

    // ── The instrument as a plugin ──

    #[test]
    fn silence_with_no_input() {
        let mut s = RhodesPiano::new();
        s.init(SR, 64);
        let out = process_buffers(&mut s, &[], 4);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn the_audio_path_does_not_allocate() {
        // "No allocation in `process`" is a property of the code rather than
        // of its output, so it is counted rather than listened to. The
        // counting allocator lives in synth.rs and is installed for the whole
        // test binary; this is the Rhodes' half of it.
        //
        // Modal synthesis is the easy case — a fixed bank of resonators, all
        // of them built in `init` — so what is actually being checked here is
        // the bookkeeping around it: note-on under more simultaneous keys than
        // there are voices, the event sort, the pedal, and a patch change
        // arriving between blocks.
        use crate::synth::tests::allocations_during;

        let mut s = RhodesPiano::new();
        s.init(SR, 256);
        let mut out = vec![0.0f32; 256];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 0)]);

        let chord: Vec<MidiEvent> =
            (36u8..72).map(|n| note_on(n, 127, u32::from(n) % 8)).collect();
        let releases: Vec<MidiEvent> = (36u8..72).map(|n| note_off(n, 0)).collect();

        let allocations = allocations_during(|| {
            let mut outs: [&mut [f32]; 1] = [&mut out];
            s.process(&[], &mut outs, &chord);
            for _ in 0..8 {
                s.process(&[], &mut outs, &[]);
            }
            s.process(&[], &mut outs, &[cc(64, 127, 0)]);
            s.process(&[], &mut outs, &releases);
            s.process(&[], &mut outs, &[cc(64, 0, 0)]);
            for index in 0..PATCH_COUNT {
                s.set_parameter(P_PATCH, patch_knob(index));
                s.process(&[], &mut outs, &[note_on(60, 110, 0)]);
            }
            s.process(&[], &mut outs, &[cc(120, 0, 0)]);
        });
        assert_eq!(allocations, 0, "the audio path allocated {allocations} times");
    }

    #[test]
    fn a_note_before_init_is_silence_rather_than_a_panic() {
        // The host calls `init` before it calls `process`, but the audio
        // thread is not the place to find out that something did not.
        let mut s = RhodesPiano::new();
        let mut out = vec![0.0f32; 64];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 0), cc(64, 127, 8)]);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = RhodesPiano::new();
        s.init(SR, 64);
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 200);
        assert!(peak(&out) > 0.005, "peak={}", peak(&out));
    }

    #[test]
    fn output_is_finite_across_the_keyboard() {
        let mut s = RhodesPiano::new();
        s.init(SR, 64);
        // The ends of an 88-key Rhodes and past them, since a MIDI file can
        // carry any note at all.
        let events: Vec<MidiEvent> =
            [0u8, 21, 36, 60, 84, 108, 127].iter().map(|&n| note_on(n, 127, 0)).collect();
        let out = process_buffers(&mut s, &events, 1500);
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(peak(&out) < 1.0, "peak={}", peak(&out));
    }

    #[test]
    fn all_params_readable() {
        let s = RhodesPiano::new();
        assert_eq!(s.parameter_count(), PARAM_COUNT);
        for i in 0..PARAM_COUNT {
            assert!(s.parameter_info(i).is_some());
            let val = s.get_parameter(i);
            assert!((0.0..=1.0).contains(&val), "param {i} = {val}");
        }
        assert!(s.parameter_info(PARAM_COUNT).is_none());
    }

    #[test]
    fn sample_accurate_midi() {
        let mut s = RhodesPiano::new();
        s.init(SR, 128);
        let mut out = vec![0.0f32; 128];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 64)]);
        assert_eq!(peak(&out[..64]), 0.0, "the note started early");
        assert!(peak(&out[64..]) > 1e-4, "post={}", peak(&out[64..]));
    }

    #[test]
    fn cc120_kills() {
        let mut s = RhodesPiano::new();
        s.init(SR, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 4);
        process_buffers(&mut s, &[cc(120, 0, 0)], 1);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn polyphony_holds_a_two_handed_voicing() {
        let mut s = RhodesPiano::new();
        s.init(SR, 64);
        let events: Vec<MidiEvent> = [36u8, 43, 48, 55, 60, 64, 67, 72]
            .iter()
            .map(|&n| note_on(n, 100, 0))
            .collect();
        let out = process_buffers(&mut s, &events, 400);
        assert!(peak(&out) > 0.05 && peak(&out) < 1.0, "peak={}", peak(&out));
        // Every one of the eight is still there a second later: the allocator
        // has sixteen voices and must not have stolen from itself.
        let tail = &out[22_050..];
        for note in [36u8, 43, 48, 55, 60, 64, 67, 72] {
            let f0 = note_to_freq(f64::from(note));
            assert!(
                magnitude_at(tail, f0) > 1e-5,
                "note {note} is gone from the chord: {:.6}",
                magnitude_at(tail, f0)
            );
        }
    }

    // ── The fork ──

    #[test]
    fn the_modes_are_a_cantilevers() {
        // A clamped-free beam's modal frequencies go as the square of the
        // eigenvalue, so the ratios are the published ones rather than
        // integers. This is the whole reason a Rhodes rings like a bell and a
        // piano does not, so it is re-derived here rather than trusted.
        for (i, ratio) in MODE_RATIOS.iter().enumerate() {
            let want = (BEAM_EIGENVALUES[i] / BEAM_EIGENVALUES[0]).powi(2);
            assert!(
                (ratio - want).abs() < 1e-5,
                "mode {i} is {ratio}, the eigenvalue gives {want}"
            );
        }
        // And the figures the literature quotes, to the digits it quotes them.
        let quoted = [1.0, 6.267, 17.55, 34.39, 56.84];
        for (got, want) in MODE_RATIOS.iter().zip(quoted) {
            assert!((got - want).abs() < 0.01, "{got} against the published {want}");
        }
        // Neither of the two overtones that survive long enough to be heard is
        // anywhere near a harmonic: 6.27 sits a quarter of the fundamental
        // away from the sixth and 17.55 half of it away from the eighteenth.
        // The top two are not checked — by the time a listener could place
        // them against a harmonic series they have been gone for tens of
        // milliseconds.
        for ratio in &MODE_RATIOS[1..3] {
            let nearest = ratio.round();
            assert!(
                (ratio - nearest).abs() > 0.2,
                "{ratio} is close enough to {nearest} to be a harmonic"
            );
        }
    }

    #[test]
    fn the_sustain_table_is_the_measured_one() {
        // The five measured points, from Q values of 949, 731, 1520, 2175 and
        // 1761 through Q = pi*f0*tau. `sustain_tau` has to return them exactly
        // at the notes they were measured at.
        for (note, want) in SUSTAIN_NOTES.iter().zip(SUSTAIN_TAU) {
            assert!((sustain_tau(*note) - want).abs() < 1e-12, "note {note}");
        }
        // Derived from the Q figures rather than typed independently.
        for ((note, q), want) in
            [(39.0, 949.0), (51.0, 731.0), (63.0, 1520.0), (75.0, 2175.0), (87.0, 1761.0)]
                .iter()
                .zip(SUSTAIN_TAU)
        {
            let derived = q / (PI * note_to_freq(*note));
            assert!(
                (derived - want).abs() < 0.006,
                "Q {q} at note {note} gives tau {derived:.4}, the table says {want}"
            );
        }
        // Not monotonic, and that is the point: E flat 3 is shorter-lived than
        // both of its neighbours. A curve fitted through these would smooth
        // that away, so this asserts the dip is still there — through
        // `sustain_tau` rather than the table, so that a fit quietly put in
        // front of the measurements would fail here.
        let dip = sustain_tau(SUSTAIN_NOTES[1]);
        assert!(dip < sustain_tau(SUSTAIN_NOTES[0]), "the E flat 3 dip is gone");
        assert!(dip < sustain_tau(SUSTAIN_NOTES[2]), "the E flat 3 dip is gone");
        assert!(
            sustain_tau(SUSTAIN_NOTES[3]) < sustain_tau(SUSTAIN_NOTES[2]),
            "the E flat 5 peak is gone"
        );
        // Interpolated between, held outside.
        assert!((sustain_tau(45.0) - (3.88 + 1.50) / 2.0).abs() < 1e-9);
        assert!((sustain_tau(21.0) - SUSTAIN_TAU[0]).abs() < 1e-12);
        assert!((sustain_tau(108.0) - SUSTAIN_TAU[4]).abs() < 1e-12);
    }

    /// The 1/e time of the fundamental, fitted by least squares on the log of
    /// its amplitude so that the fork's beat averages out rather than deciding
    /// the answer.
    fn measured_tau(note: u8, blocks: usize) -> f64 {
        // Two settings, because the quantity being measured is the fork's Q
        // and not the rest of the instrument:
        //
        // * the tonebar uncoupled, so the fit sees the tine's decay rather
        //   than the fork's beat — a beat period comparable to the render
        //   window would tilt the fit and the answer would be the coupling;
        // * a gentle strike with the voicing at the pickup's inflection, where
        //   the transfer's curvature is zero. Struck hard the tine swings past
        //   the gradient's peak, so the pickup's gain on the fundamental
        //   *rises* as the note dies away and the note sounds longer than the
        //   fork is ringing. That is a real property of a Rhodes and it is
        //   measured on its own in `a_hard_strike_outlasts_its_own_envelope`.
        let mut s = RhodesPiano::new();
        s.init(SR, 64);
        s.set_parameter(P_TONEBAR, 0.0);
        s.set_parameter(P_VOICING, 0.0);
        let out = process_buffers(&mut s, &[note_on(note, 40, 0)], blocks);
        let f0 = note_to_freq(f64::from(note));
        let window = 4410;
        let hop = 2205;
        let mut points = Vec::new();
        let mut first = 0.0;
        let mut i = 4410;
        while i + window < out.len() {
            let m = magnitude_at(&out[i..i + window], f0);
            if first == 0.0 {
                first = m;
            }
            if m < first * 0.02 {
                break;
            }
            points.push((i as f64 / SR, m.ln()));
            i += hop;
        }
        let n = points.len() as f64;
        let sx: f64 = points.iter().map(|p| p.0).sum();
        let sy: f64 = points.iter().map(|p| p.1).sum();
        let sxx: f64 = points.iter().map(|p| p.0 * p.0).sum();
        let sxy: f64 = points.iter().map(|p| p.0 * p.1).sum();
        -(n * sxx - sx * sx) / (n * sxy - sx * sy)
    }

    #[test]
    fn the_fundamental_decays_at_the_rate_the_q_table_says() {
        // Rendered and measured, not asserted about a coefficient: this is the
        // one claim the whole instrument stands on, and a resonator whose
        // decay is applied in the wrong place would still have the right
        // constant in it.
        //
        // Five seconds at E flat 2, which is 1.3 of that note's time constant,
        // and proportionally less further up.
        for (note, want, blocks) in
            [(39u8, 3.88, 3400), (51, 1.50, 1600), (63, 1.56, 1600), (75, 1.11, 1200), (87, 0.45, 900)]
        {
            let got = measured_tau(note, blocks);
            assert!(
                (got - want).abs() < want * 0.08,
                "note {note}: {got:.3} s to 1/e where the Q table says {want:.3} s"
            );
        }
    }

    #[test]
    fn a_hard_strike_outlasts_its_own_envelope() {
        // The other half of the pickup being a nonlinearity, and one of the
        // things a Rhodes does that a sampler cannot: struck hard, the tine
        // swings past the point where the pickup's flux gradient is steepest,
        // so the gain it sees on the fundamental *rises* as the swing dies
        // back. The note's fundamental therefore decays more slowly than the
        // mode behind it, and a loud note rings on after the bark has gone.
        //
        // Measured as the apparent 1/e time of the fundamental at two
        // velocities, everything else equal.
        fn apparent_tau(velocity: u8) -> f64 {
            let mut s = RhodesPiano::new();
            s.init(SR, 64);
            s.set_parameter(P_TONEBAR, 0.0);
            let out = process_buffers(&mut s, &[note_on(39, velocity, 0)], 3400);
            let f0 = note_to_freq(39.0);
            let early = magnitude_at(&out[8820..22_050], f0);
            let late = magnitude_at(&out[176_400..189_630], f0);
            (176_400.0 - 8_820.0) / SR / (early / late).ln()
        }
        let soft = apparent_tau(40);
        let hard = apparent_tau(127);
        assert!(
            hard > soft * 1.1,
            "the strike force does not change how long the note takes to die: {soft:.2} s at \
             velocity 40 and {hard:.2} s at 127"
        );
    }

    #[test]
    fn the_attack_is_inharmonic_and_the_sustain_is_not() {
        // The claim that makes it a Rhodes rather than a bell or an organ:
        // the strike is full of the cantilever's overtones and a second later
        // almost nothing but the fundamental is left. Measured as the level of
        // the 6.27x partial relative to the fundamental in the same window.
        for note in [51u8, 60, 63] {
            let out = render_note(0, note, 110, 1400);
            let f0 = note_to_freq(f64::from(note));
            let bell = f0 * MODE_RATIOS[1];
            let attack = 20.0f64.mul_add(0.0, db(magnitude_at(&out[..1323], bell)))
                - db(magnitude_at(&out[..1323], f0));
            let held = db(magnitude_at(&out[44_100..57_330], bell))
                - db(magnitude_at(&out[44_100..57_330], f0));
            assert!(
                attack > -22.0,
                "note {note}: the attack has no bell in it, {attack:.1} dB under the fundamental"
            );
            assert!(
                held < attack - 25.0,
                "note {note}: the bell is still there a second later, {attack:.1} dB at the \
                 strike and {held:.1} dB at one second"
            );
            assert!(
                held < -40.0,
                "note {note}: the sustain is not a near-pure fundamental, {held:.1} dB"
            );
        }
    }

    #[test]
    fn the_bell_partial_is_where_a_cantilever_puts_it() {
        // Not at six times the fundamental — at 6.267 times it. The peak is
        // found by search rather than asserted at the frequency the model was
        // told to use, so a mode tuned to a harmonic by accident would fail.
        let out = render_note(0, 51, 120, 200);
        let f0 = note_to_freq(51.0);
        let mut best = (0.0f64, 0.0f64);
        let mut ratio = 5.6;
        while ratio < 7.0 {
            let m = magnitude_at(&out, f0 * ratio);
            if m > best.0 {
                best = (m, ratio);
            }
            ratio += 0.002;
        }
        assert!(
            (best.1 - MODE_RATIOS[1]).abs() < 0.03,
            "the second mode is at {:.3} times the fundamental, a cantilever puts it at {:.3}",
            best.1,
            MODE_RATIOS[1]
        );
        // And it is a real peak, not the skirt of something else.
        assert!(
            best.0 > magnitude_at(&out, f0 * 6.0) * 1.5,
            "no peak at the cantilever ratio, only a shoulder"
        );
    }

    #[test]
    fn the_fork_beats() {
        // A tine and its tonebar are two coupled oscillators, so the pickup
        // sees energy crossing between them: the sustain undulates rather than
        // decaying smoothly. The TONEBAR knob is how deep that goes, and at
        // zero it has to stop entirely.
        fn ripple_db(coupling: f32) -> f64 {
            let mut s = RhodesPiano::new();
            s.init(SR, 64);
            s.set_parameter(P_TONEBAR, coupling);
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 2200);
            let f0 = note_to_freq(60.0);
            // Envelope in 50 ms windows over the second and third seconds,
            // detrended by the exponential the modes are decaying with.
            let mut points = Vec::new();
            let mut i = 22_050;
            while i + 2205 < out.len() {
                points.push((i as f64 / SR, magnitude_at(&out[i..i + 2205], f0).ln()));
                i += 2205;
            }
            let n = points.len() as f64;
            let sx: f64 = points.iter().map(|p| p.0).sum();
            let sy: f64 = points.iter().map(|p| p.1).sum();
            let sxx: f64 = points.iter().map(|p| p.0 * p.0).sum();
            let sxy: f64 = points.iter().map(|p| p.0 * p.1).sum();
            let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
            let intercept = (sy - slope * sx) / n;
            let mut lo = f64::MAX;
            let mut hi = f64::MIN;
            for (t, ln_m) in &points {
                let residual = ln_m - (intercept + slope * t);
                lo = lo.min(residual);
                hi = hi.max(residual);
            }
            20.0 * (hi - lo) / std::f64::consts::LN_10
        }

        assert!(ripple_db(0.0) < 0.5, "the fork beats with the tonebar uncoupled");
        assert!(ripple_db(1.0) > 4.0, "the fork does not beat at full coupling");
    }

    #[test]
    fn the_bottom_seven_tines_have_no_tonebar() {
        // Shear, section 6.1: on the 88-key piano the lowest seven tines have
        // no tonebar at all. Nothing down there for the tine to trade energy
        // with, so those notes decay smoothly however the knob is set.
        let mut voice = RhodesVoice::new(SR);
        let patch = RhodesPiano::new().active_patch();
        for (note, want) in [(21u8, 0.0), (27, 0.0), (28, 1.0), (60, 1.0)] {
            voice.note_on(note, 100, &patch, 1);
            let coupled = voice.modes[TONEBAR].re.abs();
            assert_eq!(
                coupled > 1e-12,
                want > 0.5,
                "note {note}: the tonebar's share of the strike is {coupled}"
            );
        }
    }

    // ── The hammer ──

    #[test]
    fn velocity_changes_the_spectrum_and_not_only_the_level() {
        // The brief the instrument is built to: a harder strike is not a
        // louder one, it is a different one. Two independent mechanisms are
        // being asserted at once — the hammer's contact time shortening, which
        // puts more of the cantilever's overtones into the fork, and the tine
        // swinging further into the pickup's nonlinearity, which adds
        // harmonics the fork does not have.
        let f0 = note_to_freq(60.0);
        let measure = |velocity: u8| {
            let out = render_note(0, 60, velocity, 30);
            let level = db(magnitude_at(&out, f0));
            (
                level,
                db(magnitude_at(&out, f0 * MODE_RATIOS[1])) - level,
                db(magnitude_at(&out, f0 * 2.0)) - level,
            )
        };
        let (soft, soft_bell, soft_second) = measure(20);
        let (hard, hard_bell, hard_second) = measure(127);

        assert!(hard - soft > 12.0, "velocity barely changes the level: {:.1} dB", hard - soft);
        assert!(
            hard_bell - soft_bell > 5.0,
            "the bell does not grow with the strike: {soft_bell:.1} dB at velocity 20 and \
             {hard_bell:.1} dB at 127, relative to the fundamental in the same window"
        );
        assert!(
            hard_second - soft_second > 15.0,
            "the pickup does not bark harder when struck harder: {soft_second:.1} dB against \
             {hard_second:.1} dB of second harmonic"
        );
        // Monotone all the way up, so it is a response rather than two points
        // that happen to differ.
        let mut previous = f64::NEG_INFINITY;
        for velocity in [20u8, 40, 64, 90, 110, 127] {
            let (_, _, second) = measure(velocity);
            assert!(second > previous, "velocity {velocity} broke the trend");
            previous = second;
        }
    }

    #[test]
    fn the_hammer_shapes_the_strike_and_not_the_note() {
        // The contact-time roll-off is in absolute frequency, so the same
        // hammer leaves a bass note full of bell and a treble note nearly
        // pure. The fundamental's own weight is always one, which is what
        // keeps the knob a tone control rather than a fader.
        for corner in [700.0, 1500.0, 2600.0] {
            let w = hammer_weights(note_to_freq(60.0), corner, 1.0);
            assert!((w[0] - MODE_STRIKE[0]).abs() < 1e-12, "the fundamental moved");
            for i in 1..5 {
                assert!(w[i].abs() <= MODE_STRIKE[i].abs() + 1e-12, "mode {i} gained weight");
            }
        }
        let soft = hammer_weights(note_to_freq(60.0), hammer_corner(0.0, 0.2), 1.0);
        let hard = hammer_weights(note_to_freq(60.0), hammer_corner(1.0, 1.0), 1.0);
        assert!(
            hard[1].abs() > soft[1].abs() * 3.0,
            "a hard hammer at full force leaves {:.3} of the bell where a soft one at a \
             whisper leaves {:.3}",
            hard[1].abs(),
            soft[1].abs()
        );
        // Bass against treble, same hammer.
        let bass = hammer_weights(note_to_freq(39.0), 1500.0, 1.0);
        let treble = hammer_weights(note_to_freq(87.0), 1500.0, 1.0);
        assert!(
            bass[1].abs() > treble[1].abs() * 4.0,
            "the top of the keyboard is as belly as the bottom: {:.3} against {:.3}",
            treble[1].abs(),
            bass[1].abs()
        );
        // The modes alternate sign at the free end, as a clamped-free beam's
        // do. A strike that pushed every mode the same way would start with a
        // click rather than a thud.
        for i in 1..MODE_STRIKE.len() {
            assert!(
                MODE_STRIKE[i] * MODE_STRIKE[i - 1] < 0.0,
                "mode {i} does not alternate sign at the tip"
            );
        }
    }

    // ── The pickup ──

    #[test]
    fn the_pickup_transfer_is_odd_and_bounded() {
        // Odd, because the flux is an even function of the tine's offset and
        // this is its gradient — which is what makes a centred tine produce
        // only even harmonics. Bounded by one, because it is normalised by its
        // own peak, which is what makes the instrument's output bounded
        // without a limiter in the path.
        let mut worst = 0.0f64;
        let mut w = -40.0;
        while w <= 40.0 {
            let g = pickup_gradient(w);
            assert!(g.is_finite(), "gradient at {w} is not finite");
            assert!(
                (g + pickup_gradient(-w)).abs() < 1e-12,
                "the transfer is not odd at {w}"
            );
            worst = worst.max(g.abs());
            w += 0.001;
        }
        assert!(worst <= 1.0 + 1e-9, "the transfer reaches {worst}, past its own normalisation");
        assert!(worst > 0.999, "the transfer never reaches its normalised peak: {worst}");
        // The peak is at the inflection, where the curvature vanishes — the
        // most linear point on the transfer, and therefore the mellowest
        // voicing there is.
        assert!(
            (pickup_gradient(PICKUP_INFLECTION) - 1.0).abs() < 1e-6,
            "the inflection is not where the gradient peaks"
        );
        // Far from the axis the tine leaves the field, so a harder strike
        // eventually produces *less* signal rather than more.
        assert!(pickup_gradient(6.0) < 0.02);
    }

    #[test]
    fn voicing_attenuates_the_fundamental_and_leaves_the_second_partial() {
        // The documented behaviour of the real adjustment, quoted in the file
        // header: as the tine's rest position approaches the pickup axis the
        // fundamental and all odd partials are attenuated, leaving the second
        // partial dominant. Nothing in the model was told to do this — it
        // falls out of the flux gradient being an odd function.
        let f0 = note_to_freq(60.0);
        let measure = |voicing: f32| {
            let mut s = RhodesPiano::new();
            s.init(SR, 64);
            s.set_parameter(P_VOICING, voicing);
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 400);
            // A window well past the strike, so this is the fork's steady tone
            // through the pickup rather than the hammer's transient.
            let w = &out[8820..22_050];
            (
                db(magnitude_at(w, f0)),
                db(magnitude_at(w, f0 * 2.0)),
                db(magnitude_at(w, f0 * 3.0)),
            )
        };
        let (off_axis, off_second, off_third) = measure(0.0);
        let (on_axis, on_second, on_third) = measure(1.0);

        assert!(
            off_axis - on_axis > 8.0,
            "the fundamental is not attenuated as the tine centres: {off_axis:.1} dB off the \
             axis, {on_axis:.1} dB on it"
        );
        assert!(
            on_second > on_axis,
            "the second partial is not dominant with the tine centred: {on_second:.1} dB \
             against a fundamental at {on_axis:.1} dB"
        );
        assert!(
            off_second < off_axis - 25.0,
            "the second partial is already there with the tine off the axis: {off_second:.1} dB"
        );
        // The third is an odd partial, so it goes the way the fundamental
        // goes: far below the second by the time the tine is centred, and
        // above it when the tine is not.
        assert!(
            off_third > off_second + 8.0,
            "off the axis the third partial should lead the second: {off_third:.1} against \
             {off_second:.1}"
        );
        assert!(
            on_third < on_second - 20.0,
            "on the axis the third partial should be far under the second: {on_third:.1} \
             against {on_second:.1}"
        );
        // Monotone across the knob, so it is an adjustment rather than two
        // end stops.
        let mut previous = f64::NEG_INFINITY;
        for voicing in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let (fundamental, second, _) = measure(voicing);
            let ratio = second - fundamental;
            assert!(ratio > previous, "voicing {voicing} broke the trend: {ratio:.1} dB");
            previous = ratio;
        }
    }

    #[test]
    fn the_top_of_the_keyboard_does_not_alias() {
        // The pickup is a nonlinearity and a nonlinearity folds. What keeps
        // this one clean is that its argument is the tine's *displacement*,
        // which the 1/ratio scaling leaves almost a pure fundamental however
        // much bell is in the velocity. Worst case: the top of an 88, struck
        // at full force, with the voicing centred so the nonlinearity is
        // working as hard as it can.
        for note in [84u8, 90, 96] {
            let mut s = RhodesPiano::new();
            s.init(SR, 64);
            s.set_parameter(P_VOICING, 1.0);
            s.set_parameter(P_STRIKE, 1.0);
            let out = process_buffers(&mut s, &[note_on(note, 127, 0)], 400);
            let f0 = note_to_freq(f64::from(note));
            let reference = db(magnitude_at(&out[8820..22_050], f0));
            let mut worst = (-300.0, 0.0f64);
            let mut probe = 60.0;
            while probe < f0 * 0.95 {
                let level = db(magnitude_at(&out[8820..22_050], probe)) - reference;
                if level > worst.0 {
                    worst = (level, probe);
                }
                probe *= 1.02;
            }
            assert!(
                worst.0 < -60.0,
                "note {note}: {:.1} dB of something at {:.0} Hz, below a fundamental at \
                 {f0:.0} Hz",
                worst.0,
                worst.1
            );
        }
    }

    // ── The action ──

    #[test]
    fn the_damper_stops_a_released_note() {
        let mut s = RhodesPiano::new();
        s.init(SR, 64);
        let held = process_buffers(&mut s, &[note_on(60, 110, 0)], 400);
        let before = peak(&held[17_640..]);
        let released = process_buffers(&mut s, &[note_off(60, 0)], 200);
        // The damper is felt on a steel tine, not a switch: a fifth of a
        // second in it is well down but not gone.
        assert!(peak(&released[8820..]) < before * 0.1, "the damper did nothing");
        // ...and a second after the key came up there is nothing left at all.
        let after = process_buffers(&mut s, &[], 700);
        let tail = &after[35_000..];
        assert!(peak(tail) < 1e-5, "the note is still ringing: {}", peak(tail));
    }

    #[test]
    fn the_treble_dampers_take_less_out_than_the_middle_ones() {
        // Every note on a Rhodes has a damper, but the treble modules are
        // short, narrow felts on short, stiff tines and they take far less
        // energy out — which is why the top of the instrument rings on under
        // fast playing where the middle stops dead.
        //
        // Measured as what is left 0.2 s after the key comes up as a fraction
        // of what was there when it did, so that two notes with quite
        // different levels and quite different sustains can be compared.
        let survives = |note: u8| {
            let mut s = RhodesPiano::new();
            s.init(SR, 64);
            let held = process_buffers(&mut s, &[note_on(note, 110, 0)], 200);
            let before = peak(&held[6000..]);
            let released = process_buffers(&mut s, &[note_off(note, 0)], 300);
            f64::from(peak(&released[8820..])) / f64::from(before)
        };
        let damped = survives(60);
        let treble = survives(100);
        assert!(damped < 0.1, "a middle note keeps {damped:.4} of itself under the damper");
        assert!(
            treble > damped * 8.0,
            "the top of the keyboard is damped as hard as the middle: it keeps {treble:.4} \
             against {damped:.4}"
        );
    }

    #[test]
    fn the_sustain_pedal_holds_the_dampers_off() {
        // Pedal down, note played, key released: the tine keeps its own
        // decay. Pedal up: the damper falls.
        let mut s = RhodesPiano::new();
        s.init(SR, 64);
        process_buffers(&mut s, &[cc(64, 127, 0)], 1);
        process_buffers(&mut s, &[note_on(60, 110, 0)], 200);
        let pedalled = process_buffers(&mut s, &[note_off(60, 0)], 300);
        assert!(peak(&pedalled[13_000..]) > 0.002, "the pedal did not hold the note");
        let lifted = process_buffers(&mut s, &[cc(64, 0, 0)], 300);
        assert!(
            peak(&lifted[13_000..]) < 1e-4,
            "the note survived the pedal coming up: {}",
            peak(&lifted[13_000..])
        );
    }

    #[test]
    fn a_restruck_key_does_not_stack() {
        // A hammer that strikes a moving tine takes energy out of it as well
        // as putting energy in, so a tremolando on one key cannot build
        // without bound. The defect this guards: a voice per strike, summing.
        let mut s = RhodesPiano::new();
        s.init(SR, 64);
        let single = peak(&process_buffers(&mut s, &[note_on(60, 127, 0)], 60));
        let mut s = RhodesPiano::new();
        s.init(SR, 64);
        let mut worst = 0.0f32;
        for _ in 0..40 {
            worst = worst.max(peak(&process_buffers(&mut s, &[note_on(60, 127, 0)], 30)));
        }
        assert!(
            worst < single * 1.2,
            "forty strikes on one key reached {worst:.4} where one reaches {single:.4}"
        );
    }

    // ── The amplifier ──

    #[test]
    fn the_tremolo_pans_rather_than_modulating() {
        // The Suitcase's vibrato is a pan between two amp channels. Two
        // things follow, and neither is true of an amplitude modulation: the
        // two channels move in opposite directions, and the pair together
        // never gets quieter than the middle of the sweep.
        let mut s = RhodesPiano::new();
        s.init(SR, 64);
        s.set_parameter(P_VIBRATO, 1.0);
        s.set_parameter(P_SPEED, 1.0);
        let (left, right) = process_stereo(&mut s, &[note_on(60, 110, 0)], 600);

        // Envelopes over 5 ms windows, well past the attack.
        let envelope = |x: &[f32]| -> Vec<f32> {
            x[8820..].chunks(220).map(peak).collect()
        };
        let el = envelope(&left);
        let er = envelope(&right);
        let mut both_up = 0;
        let mut opposed = 0;
        for i in 1..el.len() {
            let dl = el[i] - el[i - 1];
            let dr = er[i] - er[i - 1];
            if dl * dr < 0.0 {
                opposed += 1;
            } else if dl > 0.0 && dr > 0.0 {
                both_up += 1;
            }
        }
        assert!(
            opposed > both_up * 3,
            "the channels move together {both_up} times against {opposed} apart, which is a \
             tremolo rather than a pan"
        );
        // Constant power: the sum of squares does not dip where an amplitude
        // modulation would.
        let power: Vec<f32> = el.iter().zip(&er).map(|(a, b)| a * a + b * b).collect();
        let window = &power[..power.len() / 3];
        let lo = window.iter().copied().fold(f32::MAX, f32::min);
        let hi = window.iter().copied().fold(0.0f32, f32::max);
        assert!(
            hi / lo < 2.5,
            "the pair's power swings by {:.1} dB across the sweep",
            10.0 * (hi / lo).log10()
        );
    }

    #[test]
    fn the_tone_stack_can_only_cut() {
        // A control that adds level is a fader wearing a tone control's label,
        // and the headroom trim is measured at one position of every fader.
        // Both shelves here are normalised so the loudest band leaves the
        // amplifier no louder than it arrived.
        for bass in [0.0f64, 0.25, 0.5, 0.75, 1.0] {
            for treble in [0.0f64, 0.25, 0.5, 0.75, 1.0] {
                let (low, high) = tone_gains(bass, treble);
                assert!(low <= 1.0 + 1e-12 && high <= 1.0 + 1e-12, "{bass}/{treble} boosts");
                assert!(low > 0.0 && high > 0.0);
            }
        }
        // The centre detent is flat, and it is still a tilt control: bass up
        // is treble down by the same amount.
        let (low, high) = tone_gains(0.5, 0.5);
        assert!((low - 1.0).abs() < 1e-12 && (high - 1.0).abs() < 1e-12);
        let (bass_up, treble_down) = tone_gains(1.0, 0.5);
        assert!((bass_up - 1.0).abs() < 1e-12);
        assert!(
            (20.0 * treble_down.log10() + SHELF_RANGE_DB).abs() < 1e-9,
            "bass at the top does not take {SHELF_RANGE_DB} dB off the treble"
        );
    }

    // ── The panel ──

    #[test]
    fn the_panel_is_in_front_panel_order() {
        // Signal-flow order, which for a modelled instrument is what a front
        // panel is: hammer, fork, pickup, amplifier. Sessions store the block
        // positionally, which is what makes the order worth pinning down.
        assert_eq!(PARAM_NAMES[P_PATCH], "patch");
        assert_eq!(&PARAM_NAMES[P_HAMMER..=P_VELOCITY], &["hammer", "strike", "velocity"]);
        assert_eq!(
            &PARAM_NAMES[P_BELL..=P_TONEBAR],
            &["bell", "decay", "belldcy", "tonebar"]
        );
        assert_eq!(&PARAM_NAMES[P_VOICING..=P_PICKUP], &["voicing", "pickup"]);
        assert_eq!(
            &PARAM_NAMES[P_BASS..=P_LEVEL],
            &["bass", "treble", "vibrato", "speed", "level"]
        );
        assert_eq!(PARAM_COUNT, 15);
        // Every name fits the editor's column.
        for name in PARAM_NAMES {
            assert!(name.chars().count() <= 8, "{name:?} overflows its column");
        }
    }

    #[test]
    fn the_patch_knob_lands_on_the_patch_it_names() {
        for (index, name) in PATCH_NAMES.iter().enumerate() {
            let knob = patch_knob(index);
            assert_eq!(patch_index(knob), index, "patch {index} knob {knob}");
            assert_eq!(discrete_label(P_PATCH, knob), Some(*name));
            let mut s = RhodesPiano::new();
            s.set_parameter(P_PATCH, knob);
            assert_eq!(s.current_patch_index(), index);
        }
        // Out-of-range knobs are labelled rather than panicked on: `params` is
        // public and a session file is a text file someone can edit.
        assert_eq!(discrete_label(P_PATCH, 9.0), Some(PATCH_NAMES[PATCH_COUNT - 1]));
        assert_eq!(discrete_label(P_PATCH, -1.0), Some(PATCH_NAMES[0]));
        assert_eq!(discrete_label(P_PATCH, f32::NAN), Some(PATCH_NAMES[0]));
        assert_eq!(discrete_label(P_VOICING, 0.5), None);
    }

    #[test]
    fn the_bank_is_the_instruments_it_says_it_is() {
        assert_eq!(PATCH_COUNT, 26);
        // Every name distinct, and short enough to be the panel's label too.
        for (i, name) in PATCH_NAMES.iter().enumerate() {
            assert!(!name.is_empty());
            assert!(name.chars().count() <= 12, "{name:?} is too wide for the panel");
            assert!(!PATCH_NAMES[i + 1..].contains(name), "{name:?} appears twice");
        }
        // The four families the bank is organised into are all present.
        for family in ["MK1", "SC", "MK2", "Dyno"] {
            assert!(
                PATCH_NAMES.iter().filter(|n| n.starts_with(family)).count() >= 4,
                "the {family} family has fewer than four patches"
            );
        }
        assert_eq!(PATCH_NAMES[0], "MK1 Stage", "the instrument no longer loads as a Stage");
    }

    #[test]
    fn switches_step_one_position_per_press() {
        // A float-fraction stepper walks a selector a fraction of a position
        // at a time and stalls on a boundary. The patch knob steps by index.
        for index in 0..PARAM_COUNT {
            let Some(count) = discrete_steps(index) else {
                assert_eq!(step_discrete(index, 0.42, true), 0.42, "control {index} moved");
                assert_eq!(step_discrete(index, 0.42, false), 0.42, "control {index} moved");
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
    fn patch_zero_is_the_default_parameter_block() {
        let loaded = RhodesPiano::params_for_patch(0.0);
        for i in 0..PARAM_COUNT {
            assert!(
                (loaded[i] - PARAM_DEFAULTS[i]).abs() < 1e-6,
                "default {i} ({}) is {} but patch 0 loads {}",
                PARAM_NAMES[i],
                PARAM_DEFAULTS[i],
                loaded[i]
            );
        }
    }

    #[test]
    fn preset_round_trip() {
        // Bank row to panel to engine. Every column of every row has to arrive
        // in the control it belongs to — the defect this catches is a preset
        // loaded one slot out, which is silent about itself.
        for (index, program) in BANK.iter().enumerate() {
            let mut s = RhodesPiano::new();
            s.init(SR, 64);
            s.set_parameter(P_PATCH, patch_knob(index));
            let panel = s.params;
            let name = program.name;
            let columns = [
                (P_HAMMER, program.hammer[0]),
                (P_STRIKE, program.hammer[1]),
                (P_VELOCITY, program.hammer[2]),
                (P_BELL, program.fork[0]),
                (P_DECAY, program.fork[1]),
                (P_BELL_DECAY, program.fork[2]),
                (P_TONEBAR, program.fork[3]),
                (P_VOICING, program.pickup[0]),
                (P_PICKUP, program.pickup[1]),
                (P_BASS, program.amp[0]),
                (P_TREBLE, program.amp[1]),
                (P_VIBRATO, program.amp[2]),
                (P_SPEED, program.amp[3]),
                (P_LEVEL, program.amp[4]),
            ];
            for (param, want) in columns {
                assert!(
                    (panel[param] - want).abs() < 1e-9,
                    "{name} {}: {} where the bank says {want}",
                    PARAM_NAMES[param],
                    panel[param]
                );
            }
            // ...and back out in the engine's own units.
            let engine = s.active_patch();
            assert!((engine.strike - f64::from(program.hammer[1])).abs() < 1e-9);
            assert!(
                (engine.decay - decay_scale(f64::from(program.fork[1]))).abs() < 1e-9,
                "{name} decay"
            );
            let want_voicing = VOICING_W_MAX
                + f64::from(program.pickup[0]) * (VOICING_W_MIN - VOICING_W_MAX);
            assert!((engine.voicing - want_voicing).abs() < 1e-9, "{name} voicing");
            assert!((engine.level - f64::from(program.amp[4])).abs() < 1e-9, "{name} level");
        }
    }

    #[test]
    fn every_engine_control_is_reachable() {
        // A control the engine reads has to have an index, and moving it has
        // to change the sound. The failure this guards is silent: a knob that
        // draws a bar and does nothing.
        fn render(s: &mut RhodesPiano) -> Vec<f32> {
            s.init(SR, 64);
            let mut out = process_buffers(s, &[note_on(60, 100, 0)], 120);
            out.extend(process_buffers(s, &[note_off(60, 0)], 60));
            out
        }
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            if index == P_PATCH {
                continue;
            }
            let mut low = RhodesPiano::new();
            let mut high = RhodesPiano::new();
            // The vibrato has to be running for its speed to mean anything.
            for s in [&mut low, &mut high] {
                s.set_parameter(P_VIBRATO, 0.6);
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
    fn the_time_controls_report_seconds() {
        // The two knobs that scale a measured time report what that setting
        // means at middle C, which is the note in the middle of the
        // measurements. Everything else on the panel reads as a percentage.
        let middle = param_seconds(P_DECAY, 0.5).unwrap();
        assert!((middle - sustain_tau(60.0)).abs() < 1e-9, "the centre detent is not the table");
        assert!(param_seconds(P_DECAY, 1.0).unwrap() > middle * 3.5);
        assert!(param_seconds(P_DECAY, 0.0).unwrap() < middle * 0.3);
        let bell = param_seconds(P_BELL_DECAY, 0.5).unwrap();
        assert!(bell < middle * 0.2, "the bell outlives the note it is on: {bell:.3} s");
        assert!(param_seconds(P_BELL_DECAY, 1.0).unwrap() > bell * 2.5);
        for index in 0..PARAM_COUNT {
            if index != P_DECAY && index != P_BELL_DECAY {
                assert!(param_seconds(index, 0.5).is_none(), "control {index} reports seconds");
            }
        }
    }

    // ── Level ──

    #[test]
    fn every_patch_speaks() {
        // A bank sweep for silence, at a velocity a player would use and at
        // one they would not. The failure this catches is a patch voiced into
        // inaudibility by a column typed in the wrong place.
        for (index, name) in PATCH_NAMES.iter().enumerate() {
            for velocity in [40u8, 100] {
                let out = render_note(index, 60, velocity, 300);
                assert!(
                    peak(&out) > 2e-3,
                    "{name} at velocity {velocity} peaks at {:.6}",
                    peak(&out)
                );
                assert!(out.iter().all(|v| v.is_finite()), "{name} is not finite");
            }
        }
    }

    #[test]
    fn no_setting_of_the_panel_reaches_full_scale() {
        // Every control at both ends, then all of them at once, on an
        // eight-note chord at full velocity. The panel's own worst case, which
        // is what the trim is sized against.
        let chord: Vec<MidiEvent> = [36u8, 43, 48, 55, 60, 64, 67, 72]
            .iter()
            .map(|&n| note_on(n, 127, 0))
            .collect();
        let mut worst = (0.0f32, String::new());
        let mut check = |s: &mut RhodesPiano, what: String| {
            s.init(SR, 64);
            let out = process_buffers(s, &chord, 400);
            assert!(out.iter().all(|v| v.is_finite()), "{what} is not finite");
            let p = peak(&out);
            assert!(p < 1.0, "{what} reaches full scale: {p}");
            if p > worst.0 {
                worst = (p, what);
            }
        };
        for (index, name) in PARAM_NAMES.iter().enumerate().skip(1) {
            for value in [0.0f32, 1.0] {
                let mut s = RhodesPiano::new();
                s.set_parameter(index, value);
                check(&mut s, format!("{name} at {value}"));
            }
        }
        let mut s = RhodesPiano::new();
        for index in 1..PARAM_COUNT {
            s.set_parameter(index, 1.0);
        }
        check(&mut s, "every control at the top".into());

        // Measured: 0.848 with every control at the top, which is 0.4 dB under
        // the master limiter's ceiling and 0.63 dB of soft saturation. The
        // ceiling is what makes this worth asserting — a peak past it on one
        // track ducks every other track with it.
        assert!(
            worst.0 <= 0.891,
            "{} peaks at {:.4}, past the master limiter's ceiling",
            worst.1,
            worst.0
        );
    }
}
