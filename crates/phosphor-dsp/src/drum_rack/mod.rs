//! Drum rack — synthesized drum machines (808, 909, 707, 606).
//!
//! The four Roland machines are not four voicings of one synthesizer; they do
//! not even generate sound the same way as each other, and each one here is
//! built as the machine it is:
//!
//! * **808** — fully analog. Bridged-T resonators for the drums, one bank of
//!   six free-running square oscillators for everything metal, a noise source
//!   gated four times for the clap.
//! * **909** — hybrid, and the split is the instrument. Kick, snare, toms,
//!   rimshot and clap are analog circuits; the hi-hat, the ride and the crash
//!   are 6-bit samples clocked at 18 kHz through an analog envelope, and the
//!   two hats read the same one.
//! * **707** — fully PCM. Fifteen sounds read out of mask ROM at 25 kHz and
//!   contoured by analog envelope generators *after* the converter, which is
//!   why its quantisation noise decays with the note instead of sitting under
//!   it.
//! * **606** — fully analog and deliberately small: seven voices, no clap, no
//!   congas, no cowbell, no rimshot, and a panel with nothing on it but
//!   levels and accent.
//!
//! # The panel
//!
//! One strip per instrument, in front-panel order, shared by all four machines
//! because `PARAM_COUNT` is fixed and their instrument sets overlap almost
//! entirely. A note reaches its strip through [`instrument_of`], which is the
//! join between the General MIDI note map and the front panel: instruments
//! that share a knob on the hardware share one here, so claves and rimshot are
//! one strip, maracas and the hand clap are another, and each tom and the
//! conga at the same pitch are the one board behind one TUNING knob.
//!
//! Which knobs are live on which machine:
//!
//! | strip   | 808                  | 909                          | 707 | 606 |
//! |---------|----------------------|------------------------------|-----|-----|
//! | BD      | level, tone, decay   | level, tune, attack, decay   | level | level |
//! | SD      | level, tone, snappy  | level, tune, tone, snappy    | level | level |
//! | LT/MT/HT| level, tune          | level, tune, decay           | level | level |
//! | RS, CP, CB | level             | level (no cowbell circuit)   | level | — |
//! | CY      | level, tone, decay   | level, tune                  | level | level |
//! | RD      | — (no ride circuit)  | level, tune                  | level | — |
//! | OH      | level, decay         | level, decay                 | level | level |
//! | CH      | level                | level, decay                 | level | level |
//!
//! A knob a machine does not have reads as centred on that machine — see
//! [`DrumKit::is_live`] — rather than being invented for it. The 707 and the
//! 606 have no shaping controls at all on the instrument: a level fader per
//! voice and the accent bus, and that is the whole panel.
//!
//! A *voice* a machine does not have is folded onto the nearest one it does
//! have, at the strip that voice is played from — see [`instrument_of`]. That
//! is why the 606's cowbell notes answer to its high tom's level and the
//! 909's to its rimshot's: those machines have no cowbell, and playing the
//! part on the nearest voice is what the hardware leaves you.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

use crate::level::soft_saturate;

/// Fixed headroom trim on the voice sum, applied after the gain knob.
///
/// Sized in step with the five keyboards — see `OUTPUT_TRIM` in dx7.rs, which
/// carries the full reasoning.
///
/// Trimmed to the same *peak* as the synths rather than to their RMS: drums
/// are transients, and matching a kit's RMS to a sustained pad's would bury
/// it. Measured worst case across the ten kits: 1.85 for eight pads struck on
/// the same sample at velocity 127 (kit 4). Like the phosphor synth, this
/// rack had no output bound at all and handed values above 1.0 straight to
/// the mixer.
///
/// That worst case is the one voicing in the rack that crosses the saturator
/// knee, by about the same 1.3 dB as the DX7's loudest transient. A full kit
/// landing on one sample is a quantised fill, not ordinary playing, and the
/// alternative — trimming the whole rack down to accommodate it — is what
/// made every beat too quiet to use.
const OUTPUT_TRIM: f32 = 0.5557;

/// Per-voice trim, applied before the voices are summed. Unchanged from when
/// it was written inline at the end of `DrumVoice::tick`.
const VOICE_TRIM: f64 = 0.4;

/// Below this the voice has finished and its slot can be reused. −100 dBFS,
/// which is under the noise floor of anything this rack feeds.
const SILENCE: f64 = 1e-5;

/// A floor on how long a hit holds its voice, so that a sound whose exciter
/// starts from silence is not freed before it has begun.
const MIN_VOICE_SECONDS: f64 = 0.01;

/// A ceiling on how long one hit can hold a voice, whatever its envelope says.
/// Longer than the longest cymbal in any of the ten kits by a factor of three;
/// it exists so that a kit added later cannot strand a voice.
const MAX_VOICE_SECONDS: f64 = 12.0;

/// How fast a choked open hat is taken down. 4 ms: short enough to read as the
/// closed hat cutting it off, long enough not to be a click.
const CHOKE_TAU: f64 = 0.004;

// ── Parameters ──
//
// A per-instrument panel, laid out in the order the knobs sit on a TR-808:
// accent first, then each instrument group left to right, then the two rack
// controls that are ours rather than Roland's.
//
// The rack used to carry six knobs — kit, decay, tone, noise, drive, gain —
// and the middle four were global across every voice, so shortening the kick
// shortened the hats and the cymbal with it. No drum machine works that way.
// This block is the union of the front panels of the four machines the rack
// models, so one fixed `PARAM_COUNT` serves all of them:
//
// * the 808 has LEVEL for all sixteen instruments, TONE and DECAY on the bass
//   drum, TONE and SNAPPY on the snare, TUNING on the three tom/conga pairs,
//   TONE and DECAY on the cymbal, DECAY on the open hat, and ACCENT;
// * the 909 adds TUNE on the bass and snare drums, ATTACK on the bass drum,
//   DECAY on each tom, separate closed- and open-hat DECAY, and TUNE on the
//   crash and the ride, which are separate voices from its hats.
//
// A knob a machine does not have is inert on that machine rather than being
// invented — see `is_live` for which is which, and the module docs for the
// per-kit table. The 909's are reserved here so that phase 2 does not have to
// renumber the panel: sessions store `synth_params` positionally.

pub const P_KIT: usize = 0;
pub const P_ACCENT: usize = 1;
// BASS DRUM
pub const P_BD_LEVEL: usize = 2;
pub const P_BD_TUNE: usize = 3;
pub const P_BD_TONE: usize = 4;
pub const P_BD_ATTACK: usize = 5;
pub const P_BD_DECAY: usize = 6;
// SNARE DRUM
pub const P_SD_LEVEL: usize = 7;
pub const P_SD_TUNE: usize = 8;
pub const P_SD_TONE: usize = 9;
pub const P_SD_SNAPPY: usize = 10;
// LOW TOM / LOW CONGA
pub const P_LT_LEVEL: usize = 11;
pub const P_LT_TUNE: usize = 12;
pub const P_LT_DECAY: usize = 13;
// MID TOM / MID CONGA
pub const P_MT_LEVEL: usize = 14;
pub const P_MT_TUNE: usize = 15;
pub const P_MT_DECAY: usize = 16;
// HIGH TOM / HIGH CONGA
pub const P_HT_LEVEL: usize = 17;
pub const P_HT_TUNE: usize = 18;
pub const P_HT_DECAY: usize = 19;
// CLAVES / RIMSHOT
pub const P_RS_LEVEL: usize = 20;
// MARACAS / HAND CLAP
pub const P_CP_LEVEL: usize = 21;
// COWBELL
pub const P_CB_LEVEL: usize = 22;
// CYMBAL / CRASH
pub const P_CY_LEVEL: usize = 23;
pub const P_CY_TUNE: usize = 24;
pub const P_CY_TONE: usize = 25;
pub const P_CY_DECAY: usize = 26;
// RIDE
pub const P_RD_LEVEL: usize = 27;
pub const P_RD_TUNE: usize = 28;
// HI-HAT
pub const P_OH_LEVEL: usize = 29;
pub const P_OH_DECAY: usize = 30;
pub const P_CH_LEVEL: usize = 31;
pub const P_CH_DECAY: usize = 32;
// RACK
pub const P_DRIVE: usize = 33;
pub const P_GAIN: usize = 34;
pub const PARAM_COUNT: usize = 35;

/// Panel labels, eight columns or fewer because that is what the editor's
/// parameter column leaves for a name.
pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "kit", "accent",
    "bd level", "bd tune", "bd tone", "bd attk", "bd decay",
    "sd level", "sd tune", "sd tone", "sd snap",
    "lt level", "lt tune", "lt decay",
    "mt level", "mt tune", "mt decay",
    "ht level", "ht tune", "ht decay",
    "rs level", "cp level", "cb level",
    "cy level", "cy tune", "cy tone", "cy decay",
    "rd level", "rd tune",
    "oh level", "oh decay", "ch level", "ch decay",
    "drive", "gain",
];

/// The panel the rack loads with: every instrument up, every shaping control
/// centred, no drive.
///
/// The levels sit at the top of their travel rather than in the middle so that
/// the default kit is exactly as loud as the rack was before it had level
/// knobs at all — a level knob that can only cut is one that cannot introduce
/// a headroom case that `tests/headroom.rs` has not already measured.
pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    0.0,  // kit: 808
    1.0,  // accent: the trigger bus fully open, so velocity reads as written
    1.0, 0.5, 0.5, 0.5, 0.5, // bass drum
    1.0, 0.5, 0.5, 0.5,      // snare drum
    1.0, 0.5, 0.5,           // low tom / low conga
    1.0, 0.5, 0.5,           // mid tom / mid conga
    1.0, 0.5, 0.5,           // high tom / high conga
    1.0,                     // claves / rimshot
    1.0,                     // maracas / hand clap
    1.0,                     // cowbell
    1.0, 0.5, 0.5, 0.5,      // cymbal / crash
    1.0, 0.5,                // ride
    1.0, 0.5,                // open hat
    1.0, 0.5,                // closed hat
    0.0,                     // drive
    0.75,                    // gain
];

// ── Discrete controls ──

/// How many positions a switch has, or `None` for a knob. The kit selector is
/// the only one on this panel.
fn discrete_steps(index: usize) -> Option<usize> {
    match index {
        P_KIT => Some(KIT_COUNT),
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

/// The knob position that selects kit `index`, for a caller sweeping the rack
/// from outside — a level measurement, an export, a test.
#[must_use]
pub fn kit_knob(index: usize) -> f32 {
    knob_for(index.min(KIT_COUNT - 1), KIT_COUNT)
}

/// Which parameter indices are switches (rendered as labels, not bars).
#[must_use]
pub fn is_discrete(index: usize) -> bool {
    discrete_steps(index).is_some()
}

/// The knob position one step up or down from `value`. Knobs are unchanged.
///
/// Steps by *index* rather than by adding a fraction of the travel. Adding
/// 1/10 of the range ten times does not arrive at 1.0 — the error is a few ulps
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

/// Label for a switch position, or `None` for a knob.
#[must_use]
pub fn discrete_label(index: usize, value: f32) -> Option<&'static str> {
    let count = discrete_steps(index)?;
    Some(match index {
        P_KIT => KIT_LABELS[selector(value, count)],
        _ => return None,
    })
}

/// A decay knob's setting in seconds, or `None` for the knobs that read as a
/// percentage.
///
/// Calibrated on the 808, whose decay times are the ones the circuit analyses
/// give; the other nine kits scale their own envelopes by the same knob, so
/// this is the 808's number rather than a promise about all ten. Reported as
/// the −20 dB time, which is how the published figures for that machine are
/// quoted — see [`DECAY_REFERENCE`].
///
/// The tom and closed-hat knobs are not on the 808's panel at all, so what
/// they report is that machine's fixed ring time scaled by a knob only the
/// other kits answer to.
///
/// The 909 now has tapers of its own — its bass drum reaches a second and a
/// half where the 808's stops at 800 ms — so on that kit these numbers are
/// the wrong machine's. Correcting them means giving this function the kit,
/// which is a change to the editor that reads it rather than to the rack.
#[must_use]
pub fn param_seconds(index: usize, value: f32) -> Option<f64> {
    let v = f64::from(value);
    Some(match index {
        P_BD_DECAY => bd_decay_tau(v) * DECAY_REFERENCE,
        P_LT_DECAY => TOM_TAU[0] * decay_scale(v) * DECAY_REFERENCE,
        P_MT_DECAY => TOM_TAU[1] * decay_scale(v) * DECAY_REFERENCE,
        P_HT_DECAY => TOM_TAU[2] * decay_scale(v) * DECAY_REFERENCE,
        P_CY_DECAY => cy_decay_tau(v) * DECAY_REFERENCE,
        P_OH_DECAY => oh_decay_tau(v) * DECAY_REFERENCE,
        P_CH_DECAY => CH_TAU * decay_scale(v) * DECAY_REFERENCE,
        _ => return None,
    })
}

// ── Kit definitions ──

pub const KIT_COUNT: usize = 10;

/// Kit names, in selector order.
pub const KIT_LABELS: [&str; KIT_COUNT] = [
    "808", "909", "707", "606", "777", "tsty-1", "tsty-2", "tsty-3", "tsty-4", "tsty-5",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrumKit {
    Kit808,
    Kit909,
    Kit707,
    Kit606,
    Kit777,
    KitTsty1,
    KitTsty2,
    KitTsty3,
    KitTsty4,
    KitTsty5,
}

impl DrumKit {
    #[must_use]
    pub fn from_param(val: f32) -> Self {
        Self::from_index(selector(val, KIT_COUNT))
    }

    #[must_use]
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Kit808,
            1 => Self::Kit909,
            2 => Self::Kit707,
            3 => Self::Kit606,
            4 => Self::Kit777,
            5 => Self::KitTsty1,
            6 => Self::KitTsty2,
            7 => Self::KitTsty3,
            8 => Self::KitTsty4,
            _ => Self::KitTsty5,
        }
    }

    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Kit808 => 0,
            Self::Kit909 => 1,
            Self::Kit707 => 2,
            Self::Kit606 => 3,
            Self::Kit777 => 4,
            Self::KitTsty1 => 5,
            Self::KitTsty2 => 6,
            Self::KitTsty3 => 7,
            Self::KitTsty4 => 8,
            Self::KitTsty5 => 9,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        KIT_LABELS[self.index()]
    }

    /// Whether a panel control does anything on this machine.
    ///
    /// The panel is the union of four front panels, so most of it is inert on
    /// any one of them:
    ///
    /// * the **808** has no bass-drum tune, no bass-drum attack, no tom decay,
    ///   no closed-hat decay and no separate ride;
    /// * the **909** has no bass-drum tone and no cymbal tone or decay — its
    ///   crash and ride take TUNE, which is the playback rate of a sample —
    ///   and no cowbell circuit at all;
    /// * the **707** and the **606** have nothing but levels and accent. Every
    ///   sound on a 707 is a fixed recording and every voice on a 606 is a
    ///   fixed circuit; neither machine gives the player a tuning, a tone or a
    ///   decay control anywhere on the instrument.
    ///
    /// Level is not gated here even where the hardware shares one fader
    /// between two of our strips — the 606's two toms are behind one L,H TOM
    /// knob and its hats behind one O,C HIHAT knob, and the 909's hats behind
    /// one HI HAT knob. A fader that cannot be pulled down is worse than one
    /// that is finer-grained than the original.
    #[must_use]
    pub fn is_live(self, index: usize) -> bool {
        match self {
            Self::Kit808 => !matches!(
                index,
                P_BD_TUNE
                    | P_BD_ATTACK
                    | P_SD_TUNE
                    | P_LT_DECAY
                    | P_MT_DECAY
                    | P_HT_DECAY
                    | P_CY_TUNE
                    | P_RD_LEVEL
                    | P_RD_TUNE
                    | P_CH_DECAY
            ),
            Self::Kit909 => !matches!(index, P_BD_TONE | P_CB_LEVEL | P_CY_TONE | P_CY_DECAY),
            // Levels and accent, which is the whole of both front panels. The
            // strips these two machines have no voice for are listed as dead
            // as well, so that the panel reads as the instrument does.
            Self::Kit707 => matches!(
                index,
                P_KIT
                    | P_ACCENT
                    | P_BD_LEVEL
                    | P_SD_LEVEL
                    | P_LT_LEVEL
                    | P_MT_LEVEL
                    | P_HT_LEVEL
                    | P_RS_LEVEL
                    | P_CP_LEVEL
                    | P_CB_LEVEL
                    | P_CY_LEVEL
                    | P_RD_LEVEL
                    | P_OH_LEVEL
                    | P_CH_LEVEL
                    | P_DRIVE
                    | P_GAIN
            ),
            // Seven voices: no mid tom either, so that strip is dead too and
            // the notes that would play it are folded onto the low tom.
            Self::Kit606 => matches!(
                index,
                P_KIT
                    | P_ACCENT
                    | P_BD_LEVEL
                    | P_SD_LEVEL
                    | P_LT_LEVEL
                    | P_HT_LEVEL
                    | P_CY_LEVEL
                    | P_OH_LEVEL
                    | P_CH_LEVEL
                    | P_DRIVE
                    | P_GAIN
            ),
            _ => true,
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Panel tapers
// ══════════════════════════════════════════════════════════════════════════════
//
// The numbers here are the 808's, from the circuit analyses of its voice
// boards. They are quoted as decay *times* rather than as time constants, and
// the ones that can be cross-checked — the bass drum's "50 to 800 ms, 300 ms
// at centre" against a recording of the instrument — line up when read as the
// −20 dB time, so that is how they are read throughout. A drum quoted at
// 300 ms is audible for about twice that.

/// Published decay figures are −20 dB times; an exponential's time constant is
/// that divided by ln 10.
const DECAY_REFERENCE: f64 = std::f64::consts::LN_10;

/// Straight-line interpolation into a table given at the bottom of a knob,
/// its centre detent and its top.
fn interpolate3(table: &[f64; 3], knob: f64) -> f64 {
    let x = knob.clamp(0.0, 1.0) * 2.0;
    let i = (x as usize).min(1);
    let f = x - i as f64;
    table[i] + f * (table[i + 1] - table[i])
}

/// A knob across a range where equal turns are equal ratios, which is what a
/// decay control has to be to feel even.
fn geometric(lo: f64, hi: f64, knob: f64) -> f64 {
    lo * (hi / lo).powf(knob.clamp(0.0, 1.0))
}

/// Bass drum decay knob to time constant. Measured range 50 ms to 800 ms with
/// 300 ms at the centre detent, which is not the geometric middle — the pot
/// varies feedback around the bridged-T rather than a time directly, so the
/// three published points are interpolated rather than fitted.
const BD_DECAY_SECONDS: [f64; 3] = [0.050, 0.300, 0.800];

fn bd_decay_tau(knob: f64) -> f64 {
    interpolate3(&BD_DECAY_SECONDS, knob) / DECAY_REFERENCE
}

/// Cymbal decay knob: 350 ms to 1.2 s.
fn cy_decay_tau(knob: f64) -> f64 {
    geometric(0.350, 1.200, knob) / DECAY_REFERENCE
}

/// Open hat decay knob: 90 ms to 600 ms.
fn oh_decay_tau(knob: f64) -> f64 {
    geometric(0.090, 0.600, knob) / DECAY_REFERENCE
}

/// The closed hat is fixed at 50 ms on the 808 — the knob next to it on the
/// 909's panel is what `P_CH_DECAY` is reserved for.
const CH_TAU: f64 = 0.050 / DECAY_REFERENCE;

/// Tom and conga time constants, low to high. The conga circuits are quoted at
/// 180, 100 and 80 ms and the toms, which are the same three boards switched
/// to a lower tuning, at 100-200 ms.
const TOM_TAU: [f64; 3] = [0.200 / DECAY_REFERENCE, 0.140 / DECAY_REFERENCE, 0.110 / DECAY_REFERENCE];
const CONGA_TAU: [f64; 3] = [0.180 / DECAY_REFERENCE, 0.100 / DECAY_REFERENCE, 0.080 / DECAY_REFERENCE];

/// A decay knob as a plain multiplier, for the nine kits that do not yet have
/// per-circuit calibration. Centre is unity, so the panel's defaults leave
/// those kits exactly where the old global decay knob did.
fn decay_scale(knob: f64) -> f64 {
    0.3 + knob.clamp(0.0, 1.0) * 1.4
}

/// How far a tuning knob swings, in semitones either side of centre.
///
/// The three tom/conga circuits are published with their low, centre and high
/// tunings: low conga 164.8 / 185 / 220 Hz, mid 249.9 / 280 / 310, high
/// 370 / 400 / 455. That is −11% to +19%, −11% to +11% and −7.5% to +14%, so
/// a couple of semitones either way rather than the octave a synthesizer would
/// give you.
const TUNE_SEMITONES: f64 = 2.4;

fn tune_mult(knob: f64) -> f64 {
    ((knob.clamp(0.0, 1.0) - 0.5) * 2.0 * TUNE_SEMITONES / 12.0).exp2()
}

/// A level knob to a gain. Squared, which is close enough to the audio taper
/// of the pot and puts the centre of the travel at −12 dB.
fn level_gain(knob: f64) -> f64 {
    let k = knob.clamp(0.0, 1.0);
    k * k
}

/// The lowest the trigger bus goes: an unaccented step arrives at 3.5 V and
/// the accent knob adds up to 10 V on top of it, so a fully accented step
/// strikes with 3.86 times the pulse of an unaccented one.
const TRIGGER_MIN: f64 = 3.5 / 13.5;

/// Trigger-bus level for one hit. Velocity is the accent pattern — the 808 has
/// one accent bit per step and this rack has 127 — and the accent knob is how
/// much of it reaches the voices. With the knob down every step arrives at the
/// same 3.5 V, which is what that knob does on the instrument.
fn trigger_level(accent: f64, velocity: f64) -> f64 {
    TRIGGER_MIN + (1.0 - TRIGGER_MIN) * accent.clamp(0.0, 1.0) * velocity.clamp(0.0, 1.0)
}

/// How much longer a fully accented hit rings than an unaccented one. The
/// accent bus feeds the trigger, and a louder pulse makes a louder *and*
/// longer sound; this is the "and longer" half.
const ACCENT_DECAY_RANGE: f64 = 0.25;

// ══════════════════════════════════════════════════════════════════════════════
// Instrument groups
// ══════════════════════════════════════════════════════════════════════════════

/// Which front-panel instrument a note belongs to.
///
/// The panel has one strip per instrument and the note map is General MIDI, so
/// this is the join between them. Instruments that share a knob on the machine
/// share one here: claves and rimshot are one strip on the 808, so are maracas
/// and the hand clap, and the tom and conga of each pitch are the same circuit
/// behind one TUNING knob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Instrument {
    Bd,
    Sd,
    LowTom,
    MidTom,
    HighTom,
    Rim,
    Clap,
    Cowbell,
    Cymbal,
    Ride,
    OpenHat,
    ClosedHat,
}

const INSTRUMENT_COUNT: usize = 12;

impl Instrument {
    fn index(self) -> usize {
        match self {
            Self::Bd => 0,
            Self::Sd => 1,
            Self::LowTom => 2,
            Self::MidTom => 3,
            Self::HighTom => 4,
            Self::Rim => 5,
            Self::Clap => 6,
            Self::Cowbell => 7,
            Self::Cymbal => 8,
            Self::Ride => 9,
            Self::OpenHat => 10,
            Self::ClosedHat => 11,
        }
    }
}

/// Which strip a sound is played from.
///
/// The 808 has no ride cymbal: its one CY circuit is what a ride part is
/// played on, so on that kit the ride notes answer to the cymbal strip and the
/// ride knobs are inert. The 606 has seven voices and the 909 has no cowbell,
/// so those two fold further — the 606's whole map is `racks::kit_606::
/// voice_606`, which is the same table its synthesis dispatches on so the two
/// cannot disagree. A folded note answers to the strip of the voice it lands
/// on, which is the point of folding it there: the level knob in front of the
/// player is the one that moves the sound they hear.
pub(crate) fn instrument_of(sound: DrumSound, kit: DrumKit) -> Instrument {
    match kit {
        DrumKit::Kit606 => return racks::kit_606::voice_606(sound).strip(),
        DrumKit::Kit909 => {
            // The 909 has no cowbell circuit. Its rimshot is the only pitched
            // click on the machine, so that is where a cowbell part goes.
            if matches!(sound, DrumSound::Cowbell | DrumSound::Agogo(_)) {
                return Instrument::Rim;
            }
        }
        _ => {}
    }
    use DrumSound as S;
    match sound {
        S::Kick | S::SubKick(_) => Instrument::Bd,
        S::Snare | S::SnareAlt => Instrument::Sd,
        S::LowTom => Instrument::LowTom,
        S::MidTom => Instrument::MidTom,
        S::HighTom => Instrument::HighTom,
        // Congas, bongos and timbales are the tom circuits at another tuning;
        // they sort onto the three strips by pitch.
        S::Conga(f) | S::Bongo(f) | S::Timbale(f) => {
            if f < 260.0 {
                Instrument::LowTom
            } else if f < 360.0 {
                Instrument::MidTom
            } else {
                Instrument::HighTom
            }
        }
        S::Rimshot | S::Clave => Instrument::Rim,
        S::Cowbell | S::Agogo(_) => Instrument::Cowbell,
        S::ClosedHat | S::PedalHat => Instrument::ClosedHat,
        S::OpenHat => Instrument::OpenHat,
        S::Crash | S::Splash | S::Cymbal => Instrument::Cymbal,
        S::Ride | S::RideBell => {
            if matches!(kit, DrumKit::Kit808) { Instrument::Cymbal } else { Instrument::Ride }
        }
        // Everything shaken, scraped, blown or synthesised is on the strip the
        // 808 shares between the maracas and the hand clap.
        S::Clap
        | S::Maracas
        | S::Cabasa
        | S::Tambourine
        | S::Vibraslap
        | S::Guiro(_)
        | S::Whistle(_)
        | S::FxNoise(_) => Instrument::Clap,
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Panel
// ══════════════════════════════════════════════════════════════════════════════

/// One instrument strip, resolved from the panel once per block.
///
/// The tapers are applied here rather than in the voices: `powf` and `exp2`
/// are how a knob position becomes a ring time or a frequency ratio, and none
/// of them belong in a loop that runs at the sample rate.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Controls {
    /// LEVEL knob as a gain.
    pub(crate) level: f64,
    /// The strip's calibrated ring time, for the four that have one.
    pub(crate) tau: f64,
    /// DECAY knob as a plain multiplier, unity at the centre detent.
    pub(crate) decay_mult: f64,
    /// DECAY knob, panel position, for a machine with its own taper for it.
    /// The 909's bass drum runs to a second and a half where the 808's stops
    /// at 800 ms, and a machine that has the knob should scale it its own way
    /// rather than through the 808's calibration.
    pub(crate) decay: f64,
    /// TUNE knob as a frequency multiplier, unity at the centre detent.
    pub(crate) tune_ratio: f64,
    /// TUNE knob, panel position, for the one voice on these four machines
    /// where that knob is not a frequency: the 909's bass drum, whose TUNE
    /// control sets the length of its pitch sweep.
    pub(crate) tune: f64,
    /// TONE knob, panel position.
    pub(crate) tone: f64,
    /// SNAPPY knob, panel position.
    pub(crate) snappy: f64,
    /// ATTACK knob, panel position.
    pub(crate) attack: f64,
    /// The rack's drive control, which is global.
    pub(crate) drive: f64,
}

impl Controls {
    const CENTRED: Self = Self {
        level: 1.0,
        tau: 0.0,
        decay_mult: 1.0,
        decay: 0.5,
        tune_ratio: 1.0,
        tune: 0.5,
        tone: 0.5,
        snappy: 0.5,
        attack: 0.5,
        drive: 0.0,
    };

    /// The four modifiers the nine kits that still share one synthesis path
    /// take. At the panel's defaults every one of them is 1.0, which is where
    /// the old global knobs left them, so those kits render exactly as they
    /// did before the panel existed.
    ///
    /// The second is a *frequency* multiplier whatever the kit calls its
    /// parameter: every one of them uses it to scale an oscillator and none of
    /// them uses it on a filter, so it comes from the strip's TUNE knob rather
    /// than its TONE knob. That is also what the old global "tone" knob was
    /// actually doing.
    pub(crate) fn legacy(&self) -> (f64, f64, f64, f64) {
        (self.decay_mult, self.tune_ratio, 0.5 + self.snappy, self.drive)
    }
}

/// The whole panel, resolved once per block.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Panel {
    strips: [Controls; INSTRUMENT_COUNT],
    /// ACCENT knob, panel position.
    accent: f64,
    /// One-pole coefficient for the per-voice silence detector.
    follow: f64,
}

impl Panel {
    fn new(params: &[f32; PARAM_COUNT], kit: DrumKit, sr: f64) -> Self {
        // A knob a machine does not have reads as centred, which is unity for
        // every taper on this panel. Level is never gated that way: every
        // machine has a level knob on every instrument.
        let level = |i: usize| level_gain(f64::from(params[i]));
        let knob = |i: usize| if kit.is_live(i) { f64::from(params[i]) } else { 0.5 };
        let drive = f64::from(params[P_DRIVE]);

        let mut strips = [Controls::CENTRED; INSTRUMENT_COUNT];
        let set = |strips: &mut [Controls; INSTRUMENT_COUNT], which: Instrument, c: Controls| {
            strips[which.index()] = c;
        };
        set(&mut strips, Instrument::Bd, Controls {
            level: level(P_BD_LEVEL),
            tau: bd_decay_tau(knob(P_BD_DECAY)),
            decay_mult: decay_scale(knob(P_BD_DECAY)),
            decay: knob(P_BD_DECAY),
            tune_ratio: tune_mult(knob(P_BD_TUNE)),
            tune: knob(P_BD_TUNE),
            tone: knob(P_BD_TONE),
            attack: knob(P_BD_ATTACK),
            ..Controls::CENTRED
        });
        set(&mut strips, Instrument::Sd, Controls {
            level: level(P_SD_LEVEL),
            tune_ratio: tune_mult(knob(P_SD_TUNE)),
            tune: knob(P_SD_TUNE),
            tone: knob(P_SD_TONE),
            snappy: knob(P_SD_SNAPPY),
            ..Controls::CENTRED
        });
        for (which, l, t, d, tau) in [
            (Instrument::LowTom, P_LT_LEVEL, P_LT_TUNE, P_LT_DECAY, TOM_TAU[0]),
            (Instrument::MidTom, P_MT_LEVEL, P_MT_TUNE, P_MT_DECAY, TOM_TAU[1]),
            (Instrument::HighTom, P_HT_LEVEL, P_HT_TUNE, P_HT_DECAY, TOM_TAU[2]),
        ] {
            set(&mut strips, which, Controls {
                level: level(l),
                tau: tau * decay_scale(knob(d)),
                decay_mult: decay_scale(knob(d)),
                decay: knob(d),
                tune_ratio: tune_mult(knob(t)),
                tune: knob(t),
                ..Controls::CENTRED
            });
        }
        set(&mut strips, Instrument::Rim, Controls { level: level(P_RS_LEVEL), ..Controls::CENTRED });
        set(&mut strips, Instrument::Clap, Controls { level: level(P_CP_LEVEL), ..Controls::CENTRED });
        set(&mut strips, Instrument::Cowbell, Controls { level: level(P_CB_LEVEL), ..Controls::CENTRED });
        set(&mut strips, Instrument::Cymbal, Controls {
            level: level(P_CY_LEVEL),
            tau: cy_decay_tau(knob(P_CY_DECAY)),
            decay_mult: decay_scale(knob(P_CY_DECAY)),
            decay: knob(P_CY_DECAY),
            tune_ratio: tune_mult(knob(P_CY_TUNE)),
            tune: knob(P_CY_TUNE),
            tone: knob(P_CY_TONE),
            ..Controls::CENTRED
        });
        // No machine here has a ride tone or a hat tone, so those stay centred
        // rather than being wired to the cymbal's knob: a control that moves
        // an instrument it does not belong to is worse than one that is inert.
        set(&mut strips, Instrument::Ride, Controls {
            level: level(P_RD_LEVEL),
            tau: cy_decay_tau(knob(P_CY_DECAY)),
            tune_ratio: tune_mult(knob(P_RD_TUNE)),
            tune: knob(P_RD_TUNE),
            ..Controls::CENTRED
        });
        set(&mut strips, Instrument::OpenHat, Controls {
            level: level(P_OH_LEVEL),
            tau: oh_decay_tau(knob(P_OH_DECAY)),
            decay_mult: decay_scale(knob(P_OH_DECAY)),
            decay: knob(P_OH_DECAY),
            ..Controls::CENTRED
        });
        set(&mut strips, Instrument::ClosedHat, Controls {
            level: level(P_CH_LEVEL),
            tau: CH_TAU * decay_scale(knob(P_CH_DECAY)),
            decay_mult: decay_scale(knob(P_CH_DECAY)),
            decay: knob(P_CH_DECAY),
            ..Controls::CENTRED
        });
        for s in &mut strips {
            s.drive = drive;
        }
        Self {
            strips,
            accent: f64::from(params[P_ACCENT]),
            // 30 ms: long enough that the detector rides over the zero
            // crossings of a 50 Hz kick, short enough to free the voice
            // promptly once the tail really has gone.
            follow: (-1.0 / (sr * 0.030)).exp(),
        }
    }

    pub(crate) fn strip(&self, instrument: Instrument) -> &Controls {
        &self.strips[instrument.index()]
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Synthesis helpers
// ══════════════════════════════════════════════════════════════════════════════

use std::f64::consts::TAU;

/// Deterministic white noise from a seed. Returns value in -1..1.
#[inline]
pub(crate) fn white_noise(seed: u64) -> f64 {
    // Fast hash-based noise (no state needed beyond the seed)
    let mut x = seed;
    x ^= x >> 13;
    x = x.wrapping_mul(0x5bd1_e995_5bd1_e995);
    x ^= x >> 15;
    x = x.wrapping_mul(0x3f3f_3f3f_3f3f_3f3f);
    x ^= x >> 17;
    (x as i64 as f64) / (i64::MAX as f64)
}

/// Advance a phase accumulator, return new phase (wrapped to 0..1).
#[inline]
pub(crate) fn advance_phase(phase: &mut f64, freq: f64, sr: f64) {
    *phase += freq / sr;
    *phase -= (*phase).floor();
}

/// Sine oscillator.
#[inline]
pub(crate) fn osc_sine(phase: f64) -> f64 {
    (phase * TAU).sin()
}

/// Square wave oscillator (band-limited via first few harmonics approximation).
#[inline]
pub(crate) fn osc_square(phase: f64) -> f64 {
    if phase < 0.5 { 1.0 } else { -1.0 }
}

/// Triangle wave oscillator.
#[inline]
pub(crate) fn osc_triangle(phase: f64) -> f64 {
    if phase < 0.25 {
        phase * 4.0
    } else if phase < 0.75 {
        2.0 - phase * 4.0
    } else {
        phase * 4.0 - 4.0
    }
}

/// Soft-clip distortion.
#[inline]
pub(crate) fn soft_clip(x: f64, drive: f64) -> f64 {
    let gained = x * (1.0 + drive * 8.0);
    gained / (1.0 + gained.abs()).sqrt()
}

/// Simple one-pole low-pass filter state.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OnePole {
    y1: f64,
}

impl OnePole {
    fn new() -> Self {
        Self { y1: 0.0 }
    }

    fn tick_lp(&mut self, x: f64, cutoff: f64, sr: f64) -> f64 {
        let w = (TAU * cutoff / sr).min(1.0);
        let a = w / (1.0 + w);
        self.y1 += a * (x - self.y1);
        self.y1
    }

    fn tick_hp(&mut self, x: f64, cutoff: f64, sr: f64) -> f64 {
        x - self.tick_lp(x, cutoff, sr)
    }
}

/// State-variable filter (SVF) for bandpass/lowpass/highpass.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Svf {
    ic1eq: f64,
    ic2eq: f64,
}

impl Svf {
    fn new() -> Self {
        Self { ic1eq: 0.0, ic2eq: 0.0 }
    }

    fn tick(&mut self, x: f64, cutoff: f64, q: f64, sr: f64) -> (f64, f64, f64) {
        let g = (std::f64::consts::PI * cutoff / sr).tan();
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = x - self.ic2eq;
        let v1 = a1 * self.ic1eq + a2 * v3;
        let v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;

        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        let lp = v2;
        let bp = v1;
        let hp = x - k * v1 - v2;
        (lp, bp, hp)
    }

    fn bandpass(&mut self, x: f64, cutoff: f64, q: f64, sr: f64) -> f64 {
        self.tick(x, cutoff, q, sr).1
    }

    fn lowpass(&mut self, x: f64, cutoff: f64, q: f64, sr: f64) -> f64 {
        self.tick(x, cutoff, q, sr).0
    }

    fn highpass(&mut self, x: f64, cutoff: f64, q: f64, sr: f64) -> f64 {
        self.tick(x, cutoff, q, sr).2
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Drum sound enum — what kind of sound to synthesize
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DrumSound {
    Kick,
    Snare,
    SnareAlt,
    Clap,
    ClosedHat,
    PedalHat,
    OpenHat,
    Rimshot,
    LowTom,
    MidTom,
    HighTom,
    Crash,
    Ride,
    RideBell,
    Cowbell,
    Clave,
    Maracas,
    Tambourine,
    Splash,
    Cymbal,
    Vibraslap,
    Bongo(f64),      // freq
    Conga(f64),      // freq
    Timbale(f64),    // freq
    Agogo(f64),      // freq
    Cabasa,
    Guiro(f64),      // decay
    Whistle(f64),    // decay
    SubKick(f64),    // freq multiplier
    FxNoise(f64),    // character
}

/// Map a MIDI note number to a DrumSound.
fn note_to_sound(note: u8) -> DrumSound {
    match note {
        // Sub kicks (24-35)
        0..=35 => {
            let mult = if note < 24 { 0.3 + note as f64 * 0.02 } else { 0.5 + (note - 24) as f64 * 0.05 };
            DrumSound::SubKick(mult)
        }
        36 => DrumSound::Kick,
        37 => DrumSound::Rimshot,
        38 => DrumSound::Snare,
        39 => DrumSound::Clap,
        40 => DrumSound::SnareAlt,
        41 => DrumSound::LowTom,
        42 => DrumSound::ClosedHat,
        43 => DrumSound::LowTom,       // Low Tom 2
        44 => DrumSound::PedalHat,
        45 => DrumSound::MidTom,
        46 => DrumSound::OpenHat,
        47 => DrumSound::MidTom,       // Mid Tom 2
        48 => DrumSound::HighTom,
        49 => DrumSound::Crash,
        50 => DrumSound::HighTom,      // High Tom 2
        51 => DrumSound::Ride,
        52 => DrumSound::Cymbal,
        53 => DrumSound::RideBell,
        54 => DrumSound::Tambourine,
        55 => DrumSound::Splash,
        56 => DrumSound::Cowbell,
        57 => DrumSound::Crash,        // Crash 2
        58 => DrumSound::Vibraslap,
        59 => DrumSound::Ride,         // Ride 2
        60 => DrumSound::Bongo(400.0), // Hi Bongo
        61 => DrumSound::Bongo(300.0), // Low Bongo
        62 => DrumSound::Conga(350.0), // Mute Hi Conga
        63 => DrumSound::Conga(300.0), // Open Hi Conga
        64 => DrumSound::Conga(200.0), // Low Conga
        65 => DrumSound::Timbale(500.0), // Hi Timbale
        66 => DrumSound::Timbale(350.0), // Lo Timbale
        67 => DrumSound::Agogo(900.0),   // Hi Agogo
        68 => DrumSound::Agogo(650.0),   // Lo Agogo
        69 => DrumSound::Cabasa,
        70 => DrumSound::Maracas,
        71 => DrumSound::Whistle(0.08),  // Short Whistle
        72 => DrumSound::Whistle(0.30),  // Long Whistle
        73 => DrumSound::Guiro(0.06),    // Short Guiro
        74 => DrumSound::Guiro(0.20),    // Long Guiro
        75 => DrumSound::Clave,
        // FX sounds (76-127)
        _ => {
            let v = (note - 76) as f64 / 51.0;
            DrumSound::FxNoise(v)
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Per-voice state: holds all oscillator phases and filter states
// ══════════════════════════════════════════════════════════════════════════════

const MAX_VOICES: usize = 16;

/// Six square-wave oscillator phases for metallic hat sounds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HatOscillators {
    phases: [f64; 6],
}

impl HatOscillators {
    fn new() -> Self {
        Self { phases: [0.0; 6] }
    }

    fn reset(&mut self) {
        self.phases = [0.0; 6];
    }

    /// Tick the 6 square oscillators at the canonical 808 hat frequencies.
    fn tick(&mut self, sr: f64, freqs: &[f64; 6]) -> f64 {
        let mut sum = 0.0;
        for i in 0..6 {
            advance_phase(&mut self.phases[i], freqs[i], sr);
            sum += osc_square(self.phases[i]);
        }
        sum / 6.0
    }
}

/// The six square-wave oscillators of the 808's metal section.
///
/// They are one bank on the instrument, free-running and always on: the
/// cymbal, both hi-hats and the cowbell gate the *same* six oscillators, which
/// is part of why no two hi-hat hits on an 808 are quite alike — the gate
/// opens wherever the oscillators happen to be at the time.
///
/// The rack used to give every voice its own copy and set all six to phase
/// zero on each trigger, so every hit began with all six squares at +1
/// together: the same full-scale step into the band-pass, every time, and the
/// closed hat's onset identical to the open hat's sample for sample.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MetalBank {
    phases: [f64; 6],
    squares: [f64; 6],
}

impl MetalBank {
    fn new() -> Self {
        let mut phases = [0.0f64; 6];
        for (i, p) in phases.iter_mut().enumerate() {
            // Spread, not aligned: six oscillators that powered up together is
            // the thing this type exists in order not to be.
            *p = (i as f64 * 0.137 + 0.31).fract();
        }
        Self { phases, squares: [0.0; 6] }
    }

    fn tick(&mut self, sr: f64, freqs: &[f64; 6]) {
        for ((phase, square), &freq) in
            self.phases.iter_mut().zip(self.squares.iter_mut()).zip(freqs)
        {
            advance_phase(phase, freq, sr);
            *square = osc_square(*phase);
        }
    }

    /// All six mixed, which is what the cymbal and both hat circuits take.
    pub(crate) fn hash(&self) -> f64 {
        self.squares.iter().sum::<f64>() / 6.0
    }

    /// The two the cowbell takes: the 540 Hz and 800 Hz oscillators, the pair
    /// with the trimpots.
    pub(crate) fn cowbell(&self) -> f64 {
        (self.squares[4] + self.squares[5]) * 0.5
    }
}

/// The 808's six oscillator frequencies, from the analysis of its cymbal
/// board. Four are fixed by their components; 540 Hz and 800 Hz are the two
/// with trimpots, and are the pair the cowbell uses.
pub(crate) const HAT_FREQS_808: [f64; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

/// The 606's six, worked out of its service notes the same way.
///
/// IC16 is an HD14584B hex Schmitt trigger, one relaxation oscillator per
/// inverter, each with its own feedback resistor and capacitor: R244 560k with
/// C107 0.015 µF, R245 560k with C108 0.012, R246 470k with C109 0.012, R226
/// 330k with C99 0.015, R225 470k with C98 0.01 and R224 330k with C97 0.01.
/// The oscillation period of that circuit is 0.82·RC for a 5 V CMOS Schmitt,
/// which is the same relation the 808's table above comes from — and it
/// cross-checks, because the 808 uses a 330k/0.01 µF pair too and both tables
/// put it at 369.6 Hz.
///
/// The six sum through 150k resistors into two multiple-feedback band-passes,
/// IC15B at 3.5 kHz (R220 82k, R222 560, C90/C91 0.0068) and IC15A at 7.2 kHz
/// (R212 82k, R223 560, C94/C95 0.0033) — within a few percent of the 808's
/// 3440 Hz and 7100 Hz. So the machines' metal sections differ in their
/// oscillators, not in what is done to them: the 606's run about an octave
/// below the 808's, which is what puts more of its comb inside the pass band
/// and is why its hats read as thinner and busier rather than duller.
///
/// This is the correction of a much larger error: these six used to be listed
/// at 10.2 kHz to 12.5 kHz. Square waves up there have no harmonic below
/// 30 kHz, so at a 44.1 kHz sample rate the hat was built almost entirely out
/// of aliases, and the band-passes at 3.5 and 7.2 kHz were filtering nothing
/// that belonged to them.
///
/// Late production changed R226 to 680k, which drops its oscillator to
/// 119.6 Hz; the first-lot value is the one used here.
pub(crate) const HAT_FREQS_606: [f64; 6] = [145.2, 181.5, 216.3, 246.4, 259.5, 369.6];

// ── Sampled voices ──
//
// The 909's hi-hat, ride and crash and every one of the 707's fifteen sounds
// are read out of mask ROM, and two properties of that converter are most of
// what those voices sound like: the step size of its word length, and the
// ceiling of its clock. Both are modelled directly — see `DrumVoice::convert`.

/// The 909's cymbal ROM: 6-bit words clocked at 18 kHz.
pub(crate) const PCM_909_BITS: u32 = 6;
pub(crate) const PCM_909_RATE: f64 = 18_000.0;

/// The 707's ROM: 8-bit words at 25 kHz, and 6-bit for the crash and the ride.
pub(crate) const PCM_707_BITS: u32 = 8;
pub(crate) const PCM_707_CYMBAL_BITS: u32 = 6;
pub(crate) const PCM_707_RATE: f64 = 25_000.0;

#[derive(Debug)]
pub(crate) struct DrumVoice {
    active: bool,
    time: f64,
    note: u8,
    velocity: f32,
    /// Trigger-bus level for this hit: velocity through the accent knob.
    trigger: f64,
    /// Peak follower on the voice's own output, which is what decides when the
    /// voice is finished.
    follow: f64,
    /// When a closed hat cut this voice off, or infinity if it did not.
    choked_at: f64,
    sound: DrumSound,
    instrument: Instrument,
    kit: DrumKit,
    // Global noise sample counter for this voice
    noise_counter: u64,
    noise_seed: u64,

    // Oscillator phases
    phase1: f64,
    phase2: f64,
    phase3: f64,

    // Hat oscillators
    hat_oscs: HatOscillators,

    // Filters
    svf1: Svf,
    svf2: Svf,
    svf3: Svf,
    hp1: OnePole,
    hp2: OnePole,
    lp1: OnePole,

    // Clap burst state
    clap_burst_index: usize,

    // Tape LP state (for tsty-1/tsty-2)
    lp1_state: f64,

    // Modal bank for tsty-2 (realistic acoustic drum modes)
    modal_phases: [f64; 8],
    modal_amps: [f64; 8],    // per-mode amplitude (set on trigger for per-hit variation)
    modal_decays: [f64; 8],  // per-mode decay time in seconds
    hit_seed: u32,           // per-hit random seed for variation

    /// The sample-playback clock of the two machines that have one, as a
    /// fraction of one conversion.
    dac_phase: f64,
    /// The word the converter is currently holding.
    dac_hold: f64,
    /// How many words have been read, which is the address counter: the
    /// 707's and the 909's sampled voices index their ROM with it.
    dac_address: u64,
}

// ══════════════════════════════════════════════════════════════════════════════
// TSTY-5 Recipe Table: resonator-based drum synthesis
// Each sound is defined by parameters, not code. The engine is shared.
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Clone, Copy)]
pub(crate) struct T5Recipe {
    // Exciter type: 0=impulse+noise, 1=noise only, 2=click+noise, 3=multi-burst
    exciter: u8,
    impulse_level: f64,
    noise_level: f64,
    noise_decay: f64,       // seconds
    burst_count: u8,        // for exciter type 3
    burst_spread: f64,      // seconds, for exciter type 3
    // Resonator 1 (primary body)
    r1_freq: f64,           // Hz, 0 = disabled
    r1_q: f64,              // Q factor (higher = more tonal, longer ring)
    r1_level: f64,
    r1_decay: f64,          // seconds
    pitch_sweep: f64,       // Hz added at t=0, decays away
    pitch_time: f64,        // sweep decay time
    // Resonator 2 (secondary mode)
    r2_freq: f64, r2_q: f64, r2_level: f64, r2_decay: f64,
    // Resonator 3 (third mode / brightness)
    r3_freq: f64, r3_q: f64, r3_level: f64, r3_decay: f64,
    // Noise shaping (wires, shimmer, wash)
    noise_filter_freq: f64, // HP cutoff for shaped noise, 0 = disabled
    noise_mix: f64,
    noise_filter_decay: f64,
    wire_coupling: f64,     // 0-1, how much body amplitude modulates wire noise
}

pub(crate) const T5_DEFAULT: T5Recipe = T5Recipe {
    exciter: 0, impulse_level: 1.0, noise_level: 0.5, noise_decay: 0.01,
    burst_count: 0, burst_spread: 0.0,
    r1_freq: 0.0, r1_q: 5.0, r1_level: 0.5, r1_decay: 0.15, pitch_sweep: 0.0, pitch_time: 0.02,
    r2_freq: 0.0, r2_q: 3.0, r2_level: 0.3, r2_decay: 0.1,
    r3_freq: 0.0, r3_q: 2.0, r3_level: 0.2, r3_decay: 0.08,
    noise_filter_freq: 0.0, noise_mix: 0.0, noise_filter_decay: 0.15, wire_coupling: 0.0,
};

pub(crate) fn t5_recipe(note: u8) -> T5Recipe {
    match note {
    // ══ KICKS (24-31): impulse → low resonators with pitch sweep ══
    24 => T5Recipe { exciter:0, impulse_level:1.0, noise_level:0.4, noise_decay:0.003,
        r1_freq:62.0, r1_q:12.0, r1_level:0.8, r1_decay:0.3, pitch_sweep:120.0, pitch_time:0.018,
        r2_freq:98.0, r2_q:5.0, r2_level:0.2, r2_decay:0.08, // 1.593x Bessel mode
        r3_freq:142.0, r3_q:3.0, r3_level:0.1, r3_decay:0.05, // 2.296x mode
        noise_filter_freq:2000.0, noise_mix:0.15, noise_filter_decay:0.004, wire_coupling:0.0,
        ..T5_DEFAULT },
    25 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.6, noise_decay:0.002,
        r1_freq:72.0, r1_q:15.0, r1_level:0.85, r1_decay:0.15, pitch_sweep:80.0, pitch_time:0.012,
        r2_freq:115.0, r2_q:4.0, r2_level:0.15, r2_decay:0.06,
        noise_filter_freq:4000.0, noise_mix:0.2, noise_filter_decay:0.003, ..T5_DEFAULT },
    26 => T5Recipe { exciter:0, impulse_level:1.0, noise_level:0.2, noise_decay:0.005,
        r1_freq:50.0, r1_q:18.0, r1_level:0.9, r1_decay:0.45, pitch_sweep:60.0, pitch_time:0.025,
        r2_freq:80.0, r2_q:6.0, r2_level:0.2, r2_decay:0.15, ..T5_DEFAULT }, // deep
    27 => T5Recipe { exciter:0, impulse_level:0.6, noise_level:0.3, noise_decay:0.004,
        r1_freq:68.0, r1_q:20.0, r1_level:0.8, r1_decay:0.2, pitch_sweep:150.0, pitch_time:0.015,
        r2_freq:108.0, r2_q:8.0, r2_level:0.25, r2_decay:0.08,
        r3_freq:156.0, r3_q:4.0, r3_level:0.1, r3_decay:0.04,
        noise_filter_freq:5000.0, noise_mix:0.25, noise_filter_decay:0.002, ..T5_DEFAULT }, // rock
    28 => T5Recipe { exciter:0, impulse_level:1.2, noise_level:0.15, noise_decay:0.003,
        r1_freq:55.0, r1_q:10.0, r1_level:0.7, r1_decay:0.35, pitch_sweep:40.0, pitch_time:0.03,
        ..T5_DEFAULT }, // round muffled
    29 => T5Recipe { exciter:0, impulse_level:0.9, noise_level:0.5, noise_decay:0.002,
        r1_freq:78.0, r1_q:25.0, r1_level:0.85, r1_decay:0.1, pitch_sweep:100.0, pitch_time:0.008,
        noise_filter_freq:3500.0, noise_mix:0.3, noise_filter_decay:0.002, ..T5_DEFAULT }, // tight click
    30 => T5Recipe { exciter:0, impulse_level:1.0, noise_level:0.3, noise_decay:0.004,
        r1_freq:48.0, r1_q:14.0, r1_level:0.8, r1_decay:0.5, pitch_sweep:50.0, pitch_time:0.02,
        r2_freq:76.0, r2_q:7.0, r2_level:0.2, r2_decay:0.2,
        r3_freq:220.0, r3_q:15.0, r3_level:0.08, r3_decay:0.08, ..T5_DEFAULT }, // boomy shell
    31 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.2, noise_decay:0.006,
        r1_freq:58.0, r1_q:22.0, r1_level:0.75, r1_decay:0.25, pitch_sweep:70.0, pitch_time:0.02,
        r2_freq:92.0, r2_q:6.0, r2_level:0.15, r2_decay:0.1, ..T5_DEFAULT }, // warm

    // ══ SNARES (32-41): click exciter → mid resonators + wire noise ══
    32 => T5Recipe { exciter:2, impulse_level:0.8, noise_level:0.6, noise_decay:0.008,
        r1_freq:305.0, r1_q:8.0, r1_level:0.5, r1_decay:0.12,
        r2_freq:485.0, r2_q:4.0, r2_level:0.2, r2_decay:0.07,
        noise_filter_freq:2500.0, noise_mix:0.45, noise_filter_decay:0.25, wire_coupling:0.6,
        ..T5_DEFAULT }, // funk tight
    33 => T5Recipe { exciter:2, impulse_level:0.7, noise_level:0.7, noise_decay:0.01,
        r1_freq:235.0, r1_q:6.0, r1_level:0.55, r1_decay:0.15,
        r2_freq:375.0, r2_q:3.5, r2_level:0.25, r2_decay:0.1,
        r3_freq:500.0, r3_q:2.5, r3_level:0.12, r3_decay:0.06,
        noise_filter_freq:2000.0, noise_mix:0.5, noise_filter_decay:0.35, wire_coupling:0.5,
        ..T5_DEFAULT }, // fat backbeat
    34 => T5Recipe { exciter:2, impulse_level:0.9, noise_level:0.5, noise_decay:0.005,
        r1_freq:285.0, r1_q:10.0, r1_level:0.45, r1_decay:0.08,
        noise_filter_freq:3000.0, noise_mix:0.35, noise_filter_decay:0.12, wire_coupling:0.4,
        ..T5_DEFAULT }, // dry studio
    35 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:0.15,
        r1_freq:260.0, r1_q:3.0, r1_level:0.2, r1_decay:0.1,
        noise_filter_freq:1500.0, noise_mix:0.5, noise_filter_decay:0.2, wire_coupling:0.0,
        ..T5_DEFAULT }, // brush — noise exciter, low Q resonator
    36 => T5Recipe { exciter:2, impulse_level:1.0, noise_level:0.3, noise_decay:0.003,
        r1_freq:580.0, r1_q:12.0, r1_level:0.4, r1_decay:0.025,
        r2_freq:1520.0, r2_q:8.0, r2_level:0.25, r2_decay:0.015,
        ..T5_DEFAULT }, // cross-stick — high resonators, no wires
    37 => T5Recipe { exciter:2, impulse_level:0.4, noise_level:0.3, noise_decay:0.005,
        r1_freq:295.0, r1_q:5.0, r1_level:0.2, r1_decay:0.05,
        noise_filter_freq:3500.0, noise_mix:0.25, noise_filter_decay:0.08, wire_coupling:0.7,
        ..T5_DEFAULT }, // ghost note — quiet, wire-dominant
    38 => T5Recipe { exciter:2, impulse_level:0.8, noise_level:0.6, noise_decay:0.008,
        r1_freq:340.0, r1_q:9.0, r1_level:0.45, r1_decay:0.12,
        r2_freq:540.0, r2_q:18.0, r2_level:0.15, r2_decay:0.15, // shell ring!
        noise_filter_freq:2500.0, noise_mix:0.4, noise_filter_decay:0.25, wire_coupling:0.5,
        ..T5_DEFAULT }, // metal shell ring
    39 => T5Recipe { exciter:2, impulse_level:0.7, noise_level:0.7, noise_decay:0.012,
        r1_freq:270.0, r1_q:5.0, r1_level:0.4, r1_decay:0.1,
        noise_filter_freq:2000.0, noise_mix:0.55, noise_filter_decay:0.45, wire_coupling:0.3,
        ..T5_DEFAULT }, // loose wires — long wire decay
    40 => T5Recipe { exciter:2, impulse_level:1.0, noise_level:0.5, noise_decay:0.004,
        r1_freq:380.0, r1_q:12.0, r1_level:0.4, r1_decay:0.06,
        noise_filter_freq:4000.0, noise_mix:0.35, noise_filter_decay:0.12, wire_coupling:0.5,
        ..T5_DEFAULT }, // piccolo — high, bright
    41 => T5Recipe { exciter:2, impulse_level:0.7, noise_level:0.6, noise_decay:0.01,
        r1_freq:225.0, r1_q:7.0, r1_level:0.5, r1_decay:0.15,
        r2_freq:358.0, r2_q:4.0, r2_level:0.2, r2_decay:0.1,
        r3_freq:480.0, r3_q:3.0, r3_level:0.1, r3_decay:0.06,
        noise_filter_freq:2000.0, noise_mix:0.5, noise_filter_decay:0.35, wire_coupling:0.5,
        ..T5_DEFAULT }, // big 3-mode Bessel + shell

    // ══ CLAPS (42-47): multi-burst exciter → mid resonators ══
    42 => T5Recipe { exciter:3, burst_count:5, burst_spread:0.008,
        noise_level:0.7, noise_decay:0.15,
        r1_freq:2300.0, r1_q:1.5, r1_level:0.3, r1_decay:0.15,
        noise_filter_freq:800.0, noise_mix:0.3, noise_filter_decay:0.15,
        ..T5_DEFAULT }, // tight group
    43 => T5Recipe { exciter:3, burst_count:8, burst_spread:0.02,
        noise_level:0.6, noise_decay:0.2,
        r1_freq:1800.0, r1_q:1.2, r1_level:0.25, r1_decay:0.2,
        noise_filter_freq:600.0, noise_mix:0.35, noise_filter_decay:0.2,
        ..T5_DEFAULT }, // loose group
    44 => T5Recipe { exciter:2, impulse_level:0.8, noise_level:0.5, noise_decay:0.03,
        r1_freq:2100.0, r1_q:1.8, r1_level:0.3, r1_decay:0.03,
        ..T5_DEFAULT }, // single dry
    45 => T5Recipe { exciter:2, impulse_level:1.0, noise_level:0.4, noise_decay:0.008,
        r1_freq:3400.0, r1_q:2.5, r1_level:0.3, r1_decay:0.015,
        noise_filter_freq:2000.0, noise_mix:0.2, noise_filter_decay:0.01,
        ..T5_DEFAULT }, // finger snap
    46 => T5Recipe { exciter:2, impulse_level:0.6, noise_level:0.5, noise_decay:0.02,
        r1_freq:185.0, r1_q:3.0, r1_level:0.3, r1_decay:0.08,
        noise_filter_freq:500.0, noise_mix:0.3, noise_filter_decay:0.03,
        ..T5_DEFAULT }, // hand slap
    47 => T5Recipe { exciter:3, burst_count:4, burst_spread:0.012,
        noise_level:0.6, noise_decay:0.4,
        r1_freq:2200.0, r1_q:1.3, r1_level:0.25, r1_decay:0.4,
        noise_filter_freq:700.0, noise_mix:0.4, noise_filter_decay:0.4,
        ..T5_DEFAULT }, // hall reverb clap

    // ══ CLOSED HATS (48-55): noise exciter → high resonators, short decay ══
    48 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:0.06,
        r1_freq:4200.0, r1_q:8.0, r1_level:0.5, r1_decay:0.06,
        r2_freq:7800.0, r2_q:6.0, r2_level:0.3, r2_decay:0.04,
        r3_freq:11500.0, r3_q:4.0, r3_level:0.15, r3_decay:0.03,
        noise_filter_freq:5500.0, noise_mix:0.2, noise_filter_decay:0.05, ..T5_DEFAULT },
    49 => T5Recipe { exciter:1, noise_level:0.9, noise_decay:0.05,
        r1_freq:5500.0, r1_q:10.0, r1_level:0.45, r1_decay:0.045,
        r2_freq:9000.0, r2_q:7.0, r2_level:0.25, r2_decay:0.035,
        noise_filter_freq:6500.0, noise_mix:0.25, noise_filter_decay:0.04, ..T5_DEFAULT },
    50 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:0.07,
        r1_freq:3200.0, r1_q:6.0, r1_level:0.4, r1_decay:0.07,
        r2_freq:6000.0, r2_q:4.0, r2_level:0.2, r2_decay:0.05,
        noise_filter_freq:4000.0, noise_mix:0.2, noise_filter_decay:0.06, ..T5_DEFAULT }, // dark
    51 => T5Recipe { exciter:1, noise_level:0.85, noise_decay:0.04,
        r1_freq:6800.0, r1_q:12.0, r1_level:0.5, r1_decay:0.035,
        r2_freq:10200.0, r2_q:8.0, r2_level:0.3, r2_decay:0.025,
        noise_filter_freq:7000.0, noise_mix:0.15, noise_filter_decay:0.03, ..T5_DEFAULT }, // bright thin
    52 => T5Recipe { exciter:1, noise_level:0.75, noise_decay:0.055,
        r1_freq:3800.0, r1_q:5.0, r1_level:0.35, r1_decay:0.055,
        r2_freq:7200.0, r2_q:4.0, r2_level:0.2, r2_decay:0.04,
        r3_freq:10500.0, r3_q:3.0, r3_level:0.1, r3_decay:0.03,
        noise_filter_freq:4500.0, noise_mix:0.2, noise_filter_decay:0.05, ..T5_DEFAULT }, // medium
    53 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:0.065,
        r1_freq:4800.0, r1_q:15.0, r1_level:0.5, r1_decay:0.06,
        r2_freq:8500.0, r2_q:10.0, r2_level:0.3, r2_decay:0.045,
        noise_filter_freq:5000.0, noise_mix:0.15, noise_filter_decay:0.05, ..T5_DEFAULT }, // ringy
    54 => T5Recipe { exciter:1, noise_level:0.9, noise_decay:0.035,
        r1_freq:8000.0, r1_q:4.0, r1_level:0.3, r1_decay:0.03,
        noise_filter_freq:7500.0, noise_mix:0.3, noise_filter_decay:0.03, ..T5_DEFAULT }, // pure noise tick
    55 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:0.06,
        r1_freq:2800.0, r1_q:4.0, r1_level:0.3, r1_decay:0.065,
        r2_freq:5500.0, r2_q:3.0, r2_level:0.2, r2_decay:0.05,
        noise_filter_freq:3500.0, noise_mix:0.25, noise_filter_decay:0.06, ..T5_DEFAULT }, // chunky dark

    // ══ OPEN HATS (56-63): noise exciter → high resonators, LONG decay 0.8-2.5s ══
    56 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:1.5,
        r1_freq:4200.0, r1_q:12.0, r1_level:0.5, r1_decay:1.5,
        r2_freq:7800.0, r2_q:8.0, r2_level:0.3, r2_decay:1.8,
        r3_freq:11500.0, r3_q:5.0, r3_level:0.15, r3_decay:2.2,
        noise_filter_freq:3000.0, noise_mix:0.2, noise_filter_decay:1.2, ..T5_DEFAULT }, // standard
    57 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:2.0,
        r1_freq:5500.0, r1_q:14.0, r1_level:0.5, r1_decay:2.0,
        r2_freq:9000.0, r2_q:10.0, r2_level:0.3, r2_decay:2.5,
        noise_filter_freq:4000.0, noise_mix:0.15, noise_filter_decay:1.8, ..T5_DEFAULT }, // bright
    58 => T5Recipe { exciter:1, noise_level:0.6, noise_decay:1.0,
        r1_freq:3200.0, r1_q:8.0, r1_level:0.4, r1_decay:1.0,
        r2_freq:6000.0, r2_q:5.0, r2_level:0.25, r2_decay:0.8,
        noise_filter_freq:2500.0, noise_mix:0.25, noise_filter_decay:0.9, ..T5_DEFAULT }, // dark warm
    59 => T5Recipe { exciter:1, noise_level:0.75, noise_decay:1.8,
        r1_freq:3800.0, r1_q:10.0, r1_level:0.4, r1_decay:1.8,
        r2_freq:7200.0, r2_q:6.0, r2_level:0.25, r2_decay:2.0,
        r3_freq:10500.0, r3_q:4.0, r3_level:0.12, r3_decay:2.2,
        noise_filter_freq:3500.0, noise_mix:0.2, noise_filter_decay:1.5, ..T5_DEFAULT }, // washy long
    60 => T5Recipe { exciter:1, noise_level:0.85, noise_decay:2.5,
        r1_freq:4800.0, r1_q:6.0, r1_level:0.35, r1_decay:2.5,
        noise_filter_freq:5000.0, noise_mix:0.3, noise_filter_decay:2.0, ..T5_DEFAULT }, // breathy airy
    61 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:1.3,
        r1_freq:2800.0, r1_q:10.0, r1_level:0.45, r1_decay:1.3,
        r2_freq:5500.0, r2_q:6.0, r2_level:0.25, r2_decay:1.0,
        noise_filter_freq:2500.0, noise_mix:0.3, noise_filter_decay:1.0, ..T5_DEFAULT }, // dark open
    62 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:1.6,
        r1_freq:6800.0, r1_q:16.0, r1_level:0.5, r1_decay:1.6,
        r2_freq:10200.0, r2_q:12.0, r2_level:0.3, r2_decay:2.0,
        noise_filter_freq:5500.0, noise_mix:0.15, noise_filter_decay:1.5, ..T5_DEFAULT }, // bright shimmer
    63 => T5Recipe { exciter:1, noise_level:0.65, noise_decay:0.8,
        r1_freq:3500.0, r1_q:7.0, r1_level:0.35, r1_decay:0.8,
        r2_freq:6500.0, r2_q:5.0, r2_level:0.2, r2_decay:0.6,
        noise_filter_freq:3000.0, noise_mix:0.3, noise_filter_decay:0.7, ..T5_DEFAULT }, // half-open

    // ══ PEDAL HATS (64-65) ══
    64 => T5Recipe { exciter:0, impulse_level:0.6, noise_level:0.5, noise_decay:0.02,
        r1_freq:4000.0, r1_q:5.0, r1_level:0.3, r1_decay:0.025,
        r2_freq:1300.0, r2_q:2.5, r2_level:0.15, r2_decay:0.015,
        ..T5_DEFAULT }, // pedal chick
    65 => T5Recipe { exciter:1, noise_level:0.6, noise_decay:0.06,
        r1_freq:3500.0, r1_q:4.0, r1_level:0.25, r1_decay:0.06,
        r2_freq:1500.0, r2_q:2.0, r2_level:0.12, r2_decay:0.03,
        ..T5_DEFAULT }, // pedal loose

    // ══ TOMS (66-71): impulse → mid resonators with pitch sweep ══
    66 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.3, noise_decay:0.005,
        r1_freq:82.0, r1_q:10.0, r1_level:0.7, r1_decay:0.35, pitch_sweep:30.0, pitch_time:0.015,
        r2_freq:130.0, r2_q:5.0, r2_level:0.2, r2_decay:0.12,
        noise_filter_freq:2500.0, noise_mix:0.1, noise_filter_decay:0.005, ..T5_DEFAULT }, // floor deep
    67 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.3, noise_decay:0.004,
        r1_freq:110.0, r1_q:9.0, r1_level:0.65, r1_decay:0.28, pitch_sweep:35.0, pitch_time:0.012,
        r2_freq:175.0, r2_q:4.0, r2_level:0.18, r2_decay:0.1, ..T5_DEFAULT }, // floor medium
    68 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.3, noise_decay:0.004,
        r1_freq:140.0, r1_q:8.0, r1_level:0.6, r1_decay:0.24, pitch_sweep:30.0, pitch_time:0.01,
        r2_freq:223.0, r2_q:4.0, r2_level:0.15, r2_decay:0.08, ..T5_DEFAULT }, // low rack
    69 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.35, noise_decay:0.003,
        r1_freq:175.0, r1_q:8.0, r1_level:0.55, r1_decay:0.22, pitch_sweep:25.0, pitch_time:0.01,
        r2_freq:279.0, r2_q:4.0, r2_level:0.15, r2_decay:0.07, ..T5_DEFAULT }, // mid rack
    70 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.35, noise_decay:0.003,
        r1_freq:220.0, r1_q:8.0, r1_level:0.5, r1_decay:0.18, pitch_sweep:25.0, pitch_time:0.008,
        r2_freq:350.0, r2_q:4.0, r2_level:0.12, r2_decay:0.06, ..T5_DEFAULT }, // high rack
    71 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.25, noise_decay:0.005,
        r1_freq:155.0, r1_q:12.0, r1_level:0.6, r1_decay:0.4, pitch_sweep:20.0, pitch_time:0.012,
        r2_freq:247.0, r2_q:5.0, r2_level:0.2, r2_decay:0.15,
        r3_freq:355.0, r3_q:3.0, r3_level:0.1, r3_decay:0.1, ..T5_DEFAULT }, // concert long

    // ══ CYMBALS (72-77): noise exciter → high resonators, long decay ══
    72 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:1.5,
        r1_freq:3200.0, r1_q:6.0, r1_level:0.35, r1_decay:1.5,
        r2_freq:6000.0, r2_q:4.0, r2_level:0.2, r2_decay:1.2,
        noise_filter_freq:2000.0, noise_mix:0.25, noise_filter_decay:1.3, ..T5_DEFAULT }, // crash dark
    73 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:1.8,
        r1_freq:5000.0, r1_q:8.0, r1_level:0.4, r1_decay:1.8,
        r2_freq:8500.0, r2_q:5.0, r2_level:0.25, r2_decay:2.0,
        noise_filter_freq:3000.0, noise_mix:0.2, noise_filter_decay:1.5, ..T5_DEFAULT }, // crash bright
    74 => T5Recipe { exciter:1, noise_level:0.6, noise_decay:1.0,
        r1_freq:4500.0, r1_q:12.0, r1_level:0.45, r1_decay:1.0,
        r2_freq:7500.0, r2_q:8.0, r2_level:0.25, r2_decay:0.8,
        noise_filter_freq:3500.0, noise_mix:0.15, noise_filter_decay:0.8, ..T5_DEFAULT }, // ride ping
    75 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:2.0,
        r1_freq:3800.0, r1_q:5.0, r1_level:0.3, r1_decay:2.0,
        r2_freq:7000.0, r2_q:3.0, r2_level:0.18, r2_decay:1.8,
        noise_filter_freq:2500.0, noise_mix:0.25, noise_filter_decay:1.8, ..T5_DEFAULT }, // ride wash
    76 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:0.5,
        r1_freq:6000.0, r1_q:8.0, r1_level:0.4, r1_decay:0.5,
        r2_freq:10000.0, r2_q:5.0, r2_level:0.2, r2_decay:0.35,
        noise_filter_freq:4000.0, noise_mix:0.2, noise_filter_decay:0.4, ..T5_DEFAULT }, // splash
    77 => T5Recipe { exciter:1, noise_level:0.75, noise_decay:1.2,
        r1_freq:2800.0, r1_q:5.0, r1_level:0.35, r1_decay:1.2,
        r2_freq:5200.0, r2_q:3.0, r2_level:0.2, r2_decay:1.0,
        noise_filter_freq:1800.0, noise_mix:0.3, noise_filter_decay:1.0, ..T5_DEFAULT }, // china trashy

    // ══ PERCUSSION (78-89) ══
    78 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.2, noise_decay:0.003,
        r1_freq:580.0, r1_q:25.0, r1_level:0.5, r1_decay:0.07,
        r2_freq:870.0, r2_q:20.0, r2_level:0.35, r2_decay:0.06, ..T5_DEFAULT }, // cowbell
    79 => T5Recipe { exciter:0, impulse_level:1.0, noise_level:0.3, noise_decay:0.002,
        r1_freq:1900.0, r1_q:30.0, r1_level:0.4, r1_decay:0.015,
        r2_freq:3200.0, r2_q:20.0, r2_level:0.2, r2_decay:0.01, ..T5_DEFAULT }, // woodblock
    80 => T5Recipe { exciter:0, impulse_level:1.0, noise_level:0.1, noise_decay:0.002,
        r1_freq:2500.0, r1_q:50.0, r1_level:0.45, r1_decay:0.022, ..T5_DEFAULT }, // clave
    81 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.1, noise_decay:0.002,
        r1_freq:1200.0, r1_q:80.0, r1_level:0.4, r1_decay:0.9,
        r2_freq:3600.0, r2_q:40.0, r2_level:0.2, r2_decay:0.7, ..T5_DEFAULT }, // triangle metal
    82 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:0.2,
        r1_freq:8500.0, r1_q:6.0, r1_level:0.3, r1_decay:0.2,
        r2_freq:12000.0, r2_q:4.0, r2_level:0.2, r2_decay:0.15,
        noise_filter_freq:5000.0, noise_mix:0.2, noise_filter_decay:0.18, ..T5_DEFAULT }, // tambourine
    83 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:0.07,
        noise_filter_freq:5500.0, noise_mix:0.35, noise_filter_decay:0.07, ..T5_DEFAULT }, // shaker
    84 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:0.1,
        noise_filter_freq:6500.0, noise_mix:0.3, noise_filter_decay:0.1, ..T5_DEFAULT }, // cabasa
    85 => T5Recipe { exciter:0, impulse_level:0.6, noise_level:0.15, noise_decay:0.005,
        r1_freq:930.0, r1_q:20.0, r1_level:0.4, r1_decay:0.16,
        r2_freq:1400.0, r2_q:15.0, r2_level:0.25, r2_decay:0.14, ..T5_DEFAULT }, // agogo high
    86 => T5Recipe { exciter:0, impulse_level:0.6, noise_level:0.15, noise_decay:0.005,
        r1_freq:670.0, r1_q:20.0, r1_level:0.4, r1_decay:0.16,
        r2_freq:1010.0, r2_q:15.0, r2_level:0.25, r2_decay:0.14, ..T5_DEFAULT }, // agogo low
    87 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.1, noise_decay:0.003,
        r1_freq:760.0, r1_q:30.0, r1_level:0.35, r1_decay:0.8,
        r2_freq:1140.0, r2_q:25.0, r2_level:0.25, r2_decay:0.65,
        r3_freq:1710.0, r3_q:15.0, r3_level:0.15, r3_decay:0.5, ..T5_DEFAULT }, // ride bell
    88 => T5Recipe { exciter:1, noise_level:0.6, noise_decay:0.04,
        noise_filter_freq:7000.0, noise_mix:0.3, noise_filter_decay:0.04, ..T5_DEFAULT }, // maracas
    89 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:0.5,
        r1_freq:3500.0, r1_q:6.0, r1_level:0.3, r1_decay:0.45,
        noise_filter_freq:4000.0, noise_mix:0.25, noise_filter_decay:0.4, ..T5_DEFAULT }, // vibraslap

    // ══ MORE PERCUSSION (90-101) ══
    90 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.2, noise_decay:0.005,
        r1_freq:340.0, r1_q:12.0, r1_level:0.6, r1_decay:0.22, pitch_sweep:25.0, pitch_time:0.01,
        ..T5_DEFAULT }, // conga open
    91 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.15, noise_decay:0.003,
        r1_freq:320.0, r1_q:8.0, r1_level:0.5, r1_decay:0.06, ..T5_DEFAULT }, // conga mute
    92 => T5Recipe { exciter:2, impulse_level:0.8, noise_level:0.4, noise_decay:0.005,
        r1_freq:355.0, r1_q:6.0, r1_level:0.35, r1_decay:0.04,
        noise_filter_freq:2000.0, noise_mix:0.3, noise_filter_decay:0.008, ..T5_DEFAULT }, // conga slap
    93 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.2, noise_decay:0.003,
        r1_freq:425.0, r1_q:10.0, r1_level:0.5, r1_decay:0.1, pitch_sweep:35.0, pitch_time:0.008,
        ..T5_DEFAULT }, // bongo high
    94 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.2, noise_decay:0.004,
        r1_freq:315.0, r1_q:10.0, r1_level:0.5, r1_decay:0.15, pitch_sweep:25.0, pitch_time:0.01,
        ..T5_DEFAULT }, // bongo low
    95 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.25, noise_decay:0.004,
        r1_freq:530.0, r1_q:8.0, r1_level:0.45, r1_decay:0.2,
        r2_freq:1590.0, r2_q:12.0, r2_level:0.15, r2_decay:0.15, ..T5_DEFAULT }, // timbale high
    96 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.25, noise_decay:0.004,
        r1_freq:370.0, r1_q:8.0, r1_level:0.45, r1_decay:0.22,
        r2_freq:925.0, r2_q:10.0, r2_level:0.12, r2_decay:0.15, ..T5_DEFAULT }, // timbale low
    97 => T5Recipe { exciter:1, noise_level:0.5, noise_decay:0.15,
        r1_freq:600.0, r1_q:3.0, r1_level:0.3, r1_decay:0.15, pitch_sweep:400.0, pitch_time:0.1,
        ..T5_DEFAULT }, // cuica high
    98 => T5Recipe { exciter:1, noise_level:0.5, noise_decay:0.2,
        r1_freq:350.0, r1_q:3.0, r1_level:0.3, r1_decay:0.2, pitch_sweep:200.0, pitch_time:0.12,
        ..T5_DEFAULT }, // cuica low
    99 => T5Recipe { exciter:0, impulse_level:0.5, noise_level:0.1, noise_decay:0.002,
        r1_freq:2300.0, r1_q:40.0, r1_level:0.35, r1_decay:0.1, ..T5_DEFAULT }, // whistle
    100 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:0.22,
        r1_freq:4200.0, r1_q:4.0, r1_level:0.3, r1_decay:0.2,
        noise_filter_freq:3000.0, noise_mix:0.3, noise_filter_decay:0.18, ..T5_DEFAULT }, // guiro
    101 => T5Recipe { exciter:1, noise_level:0.6, noise_decay:1.5,
        r1_freq:3500.0, r1_q:8.0, r1_level:0.3, r1_decay:1.5,
        r2_freq:6500.0, r2_q:5.0, r2_level:0.2, r2_decay:1.2,
        noise_filter_freq:3000.0, noise_mix:0.2, noise_filter_decay:1.2, ..T5_DEFAULT }, // sizzle cymbal

    // ══ EXTRAS (102-111) ══
    102 => T5Recipe { exciter:3, burst_count:5, burst_spread:0.008,
        noise_level:0.65, noise_decay:0.2,
        r1_freq:1900.0, r1_q:1.5, r1_level:0.25, r1_decay:0.2,
        noise_filter_freq:700.0, noise_mix:0.35, noise_filter_decay:0.2, ..T5_DEFAULT }, // clap vinyl
    103 => T5Recipe { exciter:1, noise_level:0.6, noise_decay:0.15,
        r1_freq:4000.0, r1_q:5.0, r1_level:0.3, r1_decay:0.15,
        noise_filter_freq:4500.0, noise_mix:0.2, noise_filter_decay:0.12, ..T5_DEFAULT }, // hat pedal splash
    104 => T5Recipe { exciter:2, impulse_level:1.0, noise_level:0.3, noise_decay:0.002,
        r1_freq:4200.0, r1_q:20.0, r1_level:0.35, r1_decay:0.01, ..T5_DEFAULT }, // snap knuckle
    105 => T5Recipe { exciter:0, impulse_level:1.0, noise_level:0.2, noise_decay:0.002,
        r1_freq:2200.0, r1_q:25.0, r1_level:0.3, r1_decay:0.012,
        r2_freq:4800.0, r2_q:15.0, r2_level:0.15, r2_decay:0.008, ..T5_DEFAULT }, // stick click
    106 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.15, noise_decay:0.003,
        r1_freq:1800.0, r1_q:50.0, r1_level:0.35, r1_decay:0.6, pitch_sweep:3000.0, pitch_time:0.3,
        r2_freq:2700.0, r2_q:30.0, r2_level:0.2, r2_decay:0.5, ..T5_DEFAULT }, // bell tree sweep
    107 => T5Recipe { exciter:2, impulse_level:0.8, noise_level:0.3, noise_decay:0.005,
        r1_freq:1800.0, r1_q:2.0, r1_level:0.3, r1_decay:0.02, ..T5_DEFAULT }, // tongue click
    108 => T5Recipe { exciter:1, noise_level:0.5, noise_decay:0.4,
        r1_freq:3000.0, r1_q:2.0, r1_level:0.2, r1_decay:0.35,
        noise_filter_freq:1500.0, noise_mix:0.3, noise_filter_decay:0.35, ..T5_DEFAULT }, // brush sweep
    109 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:0.8,
        r1_freq:3500.0, r1_q:5.0, r1_level:0.3, r1_decay:0.8,
        r2_freq:6500.0, r2_q:4.0, r2_level:0.18, r2_decay:0.6,
        noise_filter_freq:3000.0, noise_mix:0.2, noise_filter_decay:0.7, ..T5_DEFAULT }, // hat foot splash
    110 => T5Recipe { exciter:3, burst_count:8, burst_spread:0.025,
        noise_level:0.6, noise_decay:0.5,
        r1_freq:2000.0, r1_q:1.2, r1_level:0.2, r1_decay:0.5,
        noise_filter_freq:600.0, noise_mix:0.4, noise_filter_decay:0.5, ..T5_DEFAULT }, // stadium clap
    111 => T5Recipe { exciter:1, noise_level:0.4, noise_decay:0.08,
        r1_freq:300.0, r1_q:4.0, r1_level:0.15, r1_decay:0.06,
        noise_filter_freq:3000.0, noise_mix:0.2, noise_filter_decay:0.08, wire_coupling:0.5,
        ..T5_DEFAULT }, // brush tap

    _ => T5_DEFAULT,
    }
}

impl DrumVoice {
    fn new() -> Self {
        Self {
            active: false,
            time: 0.0,
            note: 0,
            velocity: 0.0,
            trigger: 0.0,
            follow: 0.0,
            choked_at: f64::INFINITY,
            sound: DrumSound::Kick,
            instrument: Instrument::Bd,
            kit: DrumKit::Kit808,
            noise_counter: 0,
            noise_seed: 0,
            phase1: 0.0,
            phase2: 0.0,
            phase3: 0.0,
            hat_oscs: HatOscillators::new(),
            svf1: Svf::new(),
            svf2: Svf::new(),
            svf3: Svf::new(),
            hp1: OnePole::new(),
            hp2: OnePole::new(),
            lp1: OnePole::new(),
            clap_burst_index: 0,
            lp1_state: 0.0,
            modal_phases: [0.0; 8],
            modal_amps: [0.0; 8],
            modal_decays: [0.0; 8],
            hit_seed: 0,
            dac_phase: 0.0,
            dac_hold: 0.0,
            dac_address: 0,
        }
    }

    fn trigger(&mut self, note: u8, velocity: u8, sound: DrumSound, kit: DrumKit, accent: f64) {
        self.active = true;
        self.time = 0.0;
        self.note = note;
        self.velocity = velocity as f32 / 127.0;
        // The accent bus is sampled when the step fires, as it is on the
        // instrument: turning the knob does not change a hit already ringing.
        self.trigger = trigger_level(accent, f64::from(self.velocity));
        self.follow = 0.0;
        self.choked_at = f64::INFINITY;
        self.sound = sound;
        self.instrument = instrument_of(sound, kit);
        self.kit = kit;
        self.noise_counter = 0;
        // Use note as part of noise seed for variation
        self.noise_seed = (note as u64) * 127 + (velocity as u64) * 31;
        self.phase1 = 0.0;
        self.phase2 = 0.0;
        self.phase3 = 0.0;
        self.hat_oscs.reset();
        self.svf1 = Svf::new();
        self.svf2 = Svf::new();
        self.svf3 = Svf::new();
        self.hp1 = OnePole::new();
        self.hp2 = OnePole::new();
        self.lp1 = OnePole::new();
        self.clap_burst_index = 0;
        self.lp1_state = 0.0;
        self.modal_phases = [0.0; 8];
        self.modal_amps = [0.0; 8];
        self.modal_decays = [0.0; 8];
        // A sampled voice starts at address zero: the same words in the same
        // order, every hit, which is one of the things that separates the two
        // PCM machines from the two analog ones.
        self.dac_phase = 0.0;
        self.dac_hold = 0.0;
        self.dac_address = 0;
        // Per-hit seed: combine note, velocity, and a simple counter for variation
        self.hit_seed = self.hit_seed.wrapping_add(note as u32 * 7 + velocity as u32 * 13 + 1);
    }

    fn tick(&mut self, sr: f64, panel: &Panel, metal: &MetalBank) -> f32 {
        if !self.active {
            return 0.0;
        }
        let dt = 1.0 / sr;
        self.time += dt;
        self.noise_counter += 1;

        let c = *panel.strip(self.instrument);
        let sample = match self.kit {
            DrumKit::Kit808 => self.synth_808(sr, &c, metal),
            DrumKit::Kit909 => self.synth_909(sr, &c),
            DrumKit::Kit707 => self.synth_707(sr, &c),
            // The 606's metal section is six free-running oscillators like the
            // 808's, so it takes the same bank — at its own frequencies.
            DrumKit::Kit606 => self.synth_606(sr, &c, metal),
            DrumKit::Kit777 => self.synth_777(sr, &c),
            DrumKit::KitTsty1 => self.synth_tsty1(sr, &c),
            DrumKit::KitTsty2 => self.synth_tsty2(sr, &c),
            DrumKit::KitTsty3 => self.synth_tsty3(sr, &c),
            DrumKit::KitTsty4 => self.synth_tsty4(sr, &c),
            DrumKit::KitTsty5 => self.synth_tsty5(sr, &c),
        };

        // A closed hat cuts an open hat off on the instrument — one VCA, and
        // the closed-hat trigger takes it. Short ramp rather than a hard stop,
        // because a hard stop on a ringing hat is a click.
        let choke = if self.time > self.choked_at {
            (-(self.time - self.choked_at) / CHOKE_TAU).exp()
        } else {
            1.0
        };
        let voiced = sample * self.trigger * choke * VOICE_TRIM;
        let out = voiced * c.level;

        // Free the voice once it has actually finished, measured on a peak
        // follower rather than on the current sample. Measured before the
        // level knob, so how long a drum holds its voice does not depend on
        // where the player left its fader.
        //
        // Testing the instantaneous sample is what this used to do, and a
        // decaying sine passes through zero twice a cycle: the voice was
        // deactivated the first time a sample happened to land near a
        // crossing, which for the 808 kick was 81 ms into a 3 s tail. Every
        // drum in the rack was being cut off at a time that had nothing to do
        // with its envelope — the open hat at 19 ms, so it was the closed hat;
        // the clap at 12 ms, so three of its four bursts and its whole tail
        // never played at all.
        //
        // The four tsty kits used to be held active for a fixed 2.5 s each to
        // keep the same defect from cutting *their* tails off. With a follower
        // that is no longer needed: it takes 276 ms of true silence to fall
        // from a peak of 0.1 to the threshold, which bridges every gap any kit
        // in the rack has between one burst of a sound and the next.
        let mag = voiced.abs();
        self.follow = if mag > self.follow { mag } else { self.follow * panel.follow };
        if (self.time > MIN_VOICE_SECONDS && self.follow < SILENCE) || self.time > MAX_VOICE_SECONDS
        {
            self.active = false;
        }

        out as f32
    }

    /// Cut this voice off over [`CHOKE_TAU`], if it is not already going.
    fn choke(&mut self) {
        if self.active {
            self.choked_at = self.choked_at.min(self.time);
        }
    }

    fn noise(&self) -> f64 {
        white_noise(self.noise_counter.wrapping_add(self.noise_seed))
    }

    /// How much longer this hit rings than an unaccented one. The accent bus
    /// sums into the trigger, and a bigger pulse makes a louder *and* longer
    /// sound; the louder half is applied in [`DrumVoice::tick`].
    ///
    /// True of all four machines: every one of them has an accent bus that
    /// feeds the trigger rather than a mixer.
    pub(crate) fn accent_stretch(&self) -> f64 {
        1.0 + ACCENT_DECAY_RANGE * (self.trigger - 1.0)
    }

    /// One word of noise out of mask ROM.
    ///
    /// Indexed by the address counter rather than by the per-hit seed the
    /// analog voices use, so the same sound twice is the same waveform twice,
    /// sample for sample. That is what a machine that plays back a recording
    /// does, and it is the plainest difference between the two PCM machines
    /// here and the two analog ones: a 707 hi-hat is identical on every step
    /// of the bar, an 808 hi-hat never is.
    fn rom_noise(&self, tag: u64) -> f64 {
        white_noise(self.dac_address ^ tag.wrapping_mul(0x9e37_79b9_7f4a_7c15))
    }

    /// The 909's noise, which is not the 808's.
    ///
    /// The 808 amplifies a noisy transistor; the 909 clocks a pair of 18-stage
    /// shift registers, so its noise is a random *binary* sequence — every
    /// sample at one rail or the other, with none of the analog source's
    /// spikes and dips. Taking the sign of the hash is exactly that.
    fn digital_noise(&self) -> f64 {
        if self.noise() < 0.0 { -1.0 } else { 1.0 }
    }

    /// True on the samples where a sample-playback clock at `rate` converts a
    /// new word, advancing the address counter when it does.
    ///
    /// Everything a sampled voice does belongs inside `if self.convert(..)`:
    /// the ROM is read at 18 or 25 kHz and held between reads, and that hold
    /// — a zero-order one, with only the analog output filter after it — is
    /// half of why these machines sound like the decade they are from.
    fn convert(&mut self, sr: f64, rate: f64) -> bool {
        self.dac_phase += rate / sr;
        if self.dac_phase >= 1.0 {
            // Whole words, not one: at a sample rate below the playback clock
            // the address still has to advance at the clock's rate, or the
            // sound plays back slow. The cast saturates, so no rate can walk
            // the counter backwards.
            let words = self.dac_phase.floor();
            self.dac_phase -= words;
            self.dac_address = self.dac_address.wrapping_add(words as u64);
            true
        } else {
            false
        }
    }

    /// One word of ROM: `x` rounded to `bits`, two's complement.
    ///
    /// Quantising here rather than after the envelope is the whole point. On
    /// both machines the envelope generator is *after* the converter, so the
    /// quantisation noise is multiplied by the envelope along with the signal
    /// and decays with the note. Quantise after the VCA instead and the same
    /// step size leaves a constant fizz under everything, which is what a
    /// bit-crusher on the output of a synthesized voice does and is not what
    /// either of these instruments does.
    fn quantize(x: f64, bits: u32) -> f64 {
        let steps = f64::from(1u32 << (bits - 1));
        (x * steps).round().clamp(-steps, steps - 1.0) / steps
    }
}

// Per-kit synthesis methods live in separate files under racks/
mod racks;

// ══════════════════════════════════════════════════════════════════════════════
// DrumRack Plugin
// ══════════════════════════════════════════════════════════════════════════════

pub struct DrumRack {
    voices: Vec<DrumVoice>,
    sample_rate: f64,
    /// The six free-running square oscillators the metal voices gate.
    metal: MetalBank,
    pub kit: DrumKit,
    pub params: [f32; PARAM_COUNT],
}

impl DrumRack {
    #[must_use]
    pub fn new() -> Self {
        Self {
            voices: Vec::new(),
            sample_rate: 44100.0,
            metal: MetalBank::new(),
            kit: DrumKit::Kit808,
            params: PARAM_DEFAULTS,
        }
    }

    fn find_voice(&mut self, note: u8) -> &mut DrumVoice {
        // Reuse voice with same note
        if let Some(i) = self.voices.iter().position(|v| v.note == note) {
            return &mut self.voices[i];
        }
        // Find inactive voice
        if let Some(i) = self.voices.iter().position(|v| !v.active) {
            return &mut self.voices[i];
        }
        // Steal oldest voice
        &mut self.voices[0]
    }

    /// The frequency table the metal section runs at on this kit.
    fn metal_freqs(&self) -> &'static [f64; 6] {
        match self.kit {
            DrumKit::Kit606 => &HAT_FREQS_606,
            _ => &HAT_FREQS_808,
        }
    }
}

impl Default for DrumRack {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for DrumRack {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "Phosphor Drums".into(),
            version: "0.2.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..MAX_VOICES).map(|_| DrumVoice::new()).collect();
    }

    fn process(
        &mut self,
        _inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        midi_events: &[MidiEvent],
    ) {
        if outputs.is_empty() {
            return;
        }
        let buf_len = outputs[0].len();
        let gain = self.params[P_GAIN] * OUTPUT_TRIM;
        let sr = self.sample_rate;
        let kit = self.kit;
        // The panel is resolved once per block: thirty-five knobs into twelve
        // instrument strips, no allocation and nothing per sample.
        let panel = Panel::new(&self.params, kit, sr);
        let metal_freqs = self.metal_freqs();

        // Avoid heap allocation — use fixed-size index buffer for sorting
        let mut event_indices: [usize; 256] = [0; 256];
        let event_count = midi_events.len().min(256);
        for idx in 0..event_count { event_indices[idx] = idx; }
        for idx in 1..event_count {
            let mut j = idx;
            while j > 0 && midi_events[event_indices[j]].sample_offset < midi_events[event_indices[j-1]].sample_offset {
                event_indices.swap(j, j - 1);
                j -= 1;
            }
        }
        let mut ei = 0;

        for i in 0..buf_len {
            while ei < event_count && midi_events[event_indices[ei]].sample_offset as usize <= i {
                let ev = &midi_events[event_indices[ei]];
                if ev.status & 0xF0 == 0x90 && ev.data2 > 0 {
                    let sound = note_to_sound(ev.data1);
                    // A closed hat takes the hi-hat VCA from an open one.
                    if matches!(sound, DrumSound::ClosedHat | DrumSound::PedalHat) {
                        for voice in &mut self.voices {
                            if voice.instrument == Instrument::OpenHat {
                                voice.choke();
                            }
                        }
                    }
                    let voice = self.find_voice(ev.data1);
                    voice.trigger(ev.data1, ev.data2, sound, kit, panel.accent);
                }
                ei += 1;
            }

            self.metal.tick(sr, metal_freqs);
            let mut sample = 0.0f32;
            for voice in &mut self.voices {
                sample += voice.tick(sr, &panel, &self.metal);
            }
            sample *= gain;
            // Bound the output without hard clipping it. The trim above keeps
            // ordinary playing under the knee, so this is the identity for
            // everything except a kit pushed past it by the gain knob.
            sample = soft_saturate(sample);
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
            unit: "".into(),
        })
    }

    fn get_parameter(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    fn set_parameter(&mut self, index: usize, value: f32) {
        if let Some(p) = self.params.get_mut(index) {
            *p = phosphor_plugin::clamp_parameter(value);
            if index == P_KIT {
                self.kit = DrumKit::from_param(*p);
            }
        }
    }

    fn reset(&mut self) {
        for v in &mut self.voices {
            v.active = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_on(note: u8, vel: u8, offset: u32) -> MidiEvent {
        MidiEvent {
            sample_offset: offset,
            status: 0x90,
            data1: note,
            data2: vel,
        }
    }

    const SR: f64 = 44_100.0;

    /// One hit, rendered mono for `seconds`, with the panel knobs in `knobs`.
    fn strike(kit: usize, note: u8, velocity: u8, seconds: f64, knobs: &[(usize, f32)]) -> Vec<f32> {
        let mut rack = DrumRack::new();
        rack.init(SR, 256);
        rack.set_parameter(P_KIT, kit_knob(kit));
        for &(i, v) in knobs {
            rack.set_parameter(i, v);
        }
        rack.reset();
        let events = [note_on(note, velocity, 0)];
        let mut left = vec![0.0f32; 256];
        let mut right = vec![0.0f32; 256];
        let mut out = Vec::with_capacity((SR * seconds) as usize + 256);
        let mut first = true;
        while (out.len() as f64) < SR * seconds {
            left.fill(0.0);
            right.fill(0.0);
            let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
            if first {
                rack.process(&[], &mut outs, &events);
                first = false;
            } else {
                rack.process(&[], &mut outs, &[]);
            }
            out.extend_from_slice(&left);
        }
        out
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |a, s| a.max(s.abs()))
    }

    /// Seconds until the signal's peak envelope falls `db` below its maximum
    /// and stays there. Measured on 1.5 ms windows, which is short against
    /// every decay on this panel and long against the lowest note in it.
    fn decay_time(x: &[f32], db: f32) -> f64 {
        const WIN: usize = 64;
        let env: Vec<f32> = x.chunks(WIN).map(peak).collect();
        let top = env.iter().copied().fold(0.0f32, f32::max);
        let target = top * 10f32.powf(db / 20.0);
        let last = env.iter().rposition(|&e| e > target).unwrap_or(0);
        (last + 1) as f64 * WIN as f64 / SR
    }

    /// Magnitude of one frequency, Hann-windowed.
    fn magnitude(x: &[f32], hz: f64) -> f64 {
        let w = std::f64::consts::TAU * hz / SR;
        let (mut re, mut im) = (0.0, 0.0);
        for (n, &s) in x.iter().enumerate() {
            let h = 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / x.len() as f64).cos();
            let v = f64::from(s) * h;
            re += v * (w * n as f64).cos();
            im -= v * (w * n as f64).sin();
        }
        (re * re + im * im).sqrt() / x.len() as f64
    }

    /// The strongest frequency between `lo` and `hi`, to within `step`.
    fn strongest(x: &[f32], lo: f64, hi: f64, step: f64) -> f64 {
        let mut best = (lo, 0.0);
        let mut f = lo;
        while f <= hi {
            let m = magnitude(x, f);
            if m > best.1 {
                best = (f, m);
            }
            f += step;
        }
        best.0
    }

    /// Share of the signal's energy above `hz`.
    ///
    /// Measured through a filter rather than by sampling a transform on a log
    /// sweep: the metal voices are six square waves, so their spectrum is a
    /// comb of narrow lines, and a sweep that steps by a fixed ratio walks
    /// between the lines and reports whatever it happens to land on. Two
    /// sweeps a percent apart disagreed by a factor of three on the same hat.
    fn energy_above(x: &[f32], hz: f64) -> f64 {
        let mut a = Svf::new();
        let mut b = Svf::new();
        let (mut high, mut total) = (0.0, 0.0);
        for &s in x {
            let v = f64::from(s);
            let f = b.highpass(a.highpass(v, hz, 0.707, SR), hz, 0.707, SR);
            high += f * f;
            total += v * v;
        }
        high / total.max(1e-30)
    }

    #[test]
    fn silent_without_input() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 64);
        let mut out = vec![0.0f32; 64];
        dr.process(&[], &mut [&mut out], &[]);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn all_note_ranges_produce_sound() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 512);
        // Test every 8th note across the range
        for note in (24..112).step_by(8) {
            let mut out = vec![0.0f32; 512];
            dr.process(&[], &mut [&mut out], &[note_on(note, 100, 0)]);
            let peak = out.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!(
                peak > 0.001,
                "Note {note} should produce sound, peak={peak}"
            );
            dr.reset();
        }
    }

    #[test]
    fn kits_sound_different() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 512);
        dr.set_parameter(P_KIT, kit_knob(0));
        let mut out808 = vec![0.0f32; 512];
        dr.process(&[], &mut [&mut out808], &[note_on(24, 100, 0)]);

        dr.reset();
        dr.set_parameter(P_KIT, kit_knob(1));
        let mut out909 = vec![0.0f32; 512];
        dr.process(&[], &mut [&mut out909], &[note_on(24, 100, 0)]);

        let diff: f32 = out808
            .iter()
            .zip(out909.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 0.5, "808 and 909 should differ, diff={diff}");
    }

    #[test]
    fn output_is_finite() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 64);
        for note in 0..128u8 {
            let mut out = vec![0.0f32; 64];
            dr.process(&[], &mut [&mut out], &[note_on(note, 127, 0)]);
            assert!(
                out.iter().all(|s| s.is_finite()),
                "Note {note} output not finite"
            );
        }
    }

    #[test]
    fn kit_switch_changes_sound() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 512);
        dr.set_parameter(P_KIT, kit_knob(0));
        let mut out808 = vec![0.0f32; 512];
        dr.process(&[], &mut [&mut out808], &[note_on(36, 100, 0)]);

        dr.reset();
        dr.set_parameter(P_KIT, kit_knob(3));
        let mut out606 = vec![0.0f32; 512];
        dr.process(&[], &mut [&mut out606], &[note_on(36, 100, 0)]);

        let diff: f32 = out808
            .iter()
            .zip(out606.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 0.5, "808 and 606 kicks should differ, diff={diff}");
    }

    #[test]
    fn varied_sounds_across_range() {
        // Verify different sound types produce different output
        let mut dr = DrumRack::new();
        dr.init(44100.0, 1024);

        let mut out_kick = vec![0.0f32; 1024];
        dr.process(&[], &mut [&mut out_kick], &[note_on(36, 100, 0)]);
        dr.reset();

        let mut out_hat = vec![0.0f32; 1024];
        dr.process(&[], &mut [&mut out_hat], &[note_on(42, 100, 0)]);

        let diff: f32 = out_kick
            .iter()
            .zip(out_hat.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 1.0, "Kick and hat should sound different, diff={diff}");
    }

    // ── The panel ──

    #[test]
    fn the_panel_is_the_shape_the_editor_expects() {
        assert_eq!(PARAM_NAMES.len(), PARAM_COUNT);
        assert_eq!(PARAM_DEFAULTS.len(), PARAM_COUNT);
        for (i, name) in PARAM_NAMES.iter().enumerate() {
            assert!(name.len() <= 8, "{i} {name:?} is wider than the parameter column");
        }
        // The kit selector is at index 0 because that is where the editor
        // looks for a preset selector, and it is the only discrete control.
        assert!(is_discrete(P_KIT));
        for (i, name) in PARAM_NAMES.iter().enumerate().skip(1) {
            assert!(!is_discrete(i), "{name} should be a knob");
        }
        assert_eq!(P_DRIVE, PARAM_COUNT - 2);
        assert_eq!(P_GAIN, PARAM_COUNT - 1);
    }

    #[test]
    fn the_kit_selector_steps_one_kit_per_press() {
        // Ten kits stepped by index. Adding a tenth of the travel ten times
        // does not arrive at 1.0, and a step boundary missed by an ulp is a
        // keypress that visibly does nothing.
        let mut knob = PARAM_DEFAULTS[P_KIT];
        for label in KIT_LABELS.iter().skip(1) {
            knob = step_discrete(P_KIT, knob, true);
            assert_eq!(discrete_label(P_KIT, knob), Some(*label));
        }
        knob = step_discrete(P_KIT, knob, true);
        assert_eq!(discrete_label(P_KIT, knob), Some("tsty-5"), "ran off the top");
        for kit in (0..KIT_COUNT - 1).rev() {
            knob = step_discrete(P_KIT, knob, false);
            assert_eq!(discrete_label(P_KIT, knob), Some(KIT_LABELS[kit]));
        }
        knob = step_discrete(P_KIT, knob, false);
        assert_eq!(discrete_label(P_KIT, knob), Some("808"), "ran off the bottom");

        // Total: every float lands on a kit, because `params` is public.
        assert_eq!(DrumKit::from_param(-1.0), DrumKit::Kit808);
        assert_eq!(DrumKit::from_param(9.0), DrumKit::KitTsty5);
        assert_eq!(DrumKit::from_param(f32::NAN), DrumKit::Kit808);
        for kit in 0..KIT_COUNT {
            assert_eq!(DrumKit::from_param(kit_knob(kit)).index(), kit);
        }
    }

    #[test]
    fn the_default_panel_leaves_the_shared_kits_where_the_global_knobs_did() {
        // Nine of the ten kits still take one decay, one tune, one noise and
        // one drive modifier. Every one of them has to come out at unity from
        // the panel's defaults, or switching to this panel would have
        // revoiced nine kits by accident.
        let panel = Panel::new(&PARAM_DEFAULTS, DrumKit::Kit909, SR);
        for which in [
            Instrument::Bd,
            Instrument::Sd,
            Instrument::LowTom,
            Instrument::MidTom,
            Instrument::HighTom,
            Instrument::Rim,
            Instrument::Clap,
            Instrument::Cowbell,
            Instrument::Cymbal,
            Instrument::Ride,
            Instrument::OpenHat,
            Instrument::ClosedHat,
        ] {
            let c = panel.strip(which);
            let (decay, tune, noise, drive) = c.legacy();
            assert!((decay - 1.0).abs() < 1e-9, "{which:?} decay {decay}");
            assert!((tune - 1.0).abs() < 1e-9, "{which:?} tune {tune}");
            assert!((noise - 1.0).abs() < 1e-9, "{which:?} noise {noise}");
            assert_eq!(drive, 0.0);
            assert_eq!(c.level, 1.0, "{which:?} level");
        }
    }

    /// The knobs the 808 does not have read as centred whatever the panel
    /// says, so a session that moved them on another kit does not detune this
    /// one when the kit is switched back.
    #[test]
    fn the_knobs_the_808_lacks_do_nothing_on_it() {
        let missing = [
            P_BD_TUNE,
            P_BD_ATTACK,
            P_SD_TUNE,
            P_LT_DECAY,
            P_MT_DECAY,
            P_HT_DECAY,
            P_CY_TUNE,
            P_RD_LEVEL,
            P_RD_TUNE,
            P_CH_DECAY,
        ];
        for index in missing {
            assert!(!DrumKit::Kit808.is_live(index), "{}", PARAM_NAMES[index]);
            assert!(DrumKit::Kit909.is_live(index), "{}", PARAM_NAMES[index]);
            for note in [36u8, 38, 41, 42, 46, 49, 51, 52] {
                let plain = strike(0, note, 127, 0.3, &[]);
                let moved = strike(0, note, 127, 0.3, &[(index, 1.0)]);
                assert_eq!(
                    plain, moved,
                    "{} moved note {note} on the 808, where that knob is not on the panel",
                    PARAM_NAMES[index]
                );
            }
        }
        // ...and the ones it does have are not inert.
        for index in [P_BD_TONE, P_BD_DECAY, P_SD_TONE, P_SD_SNAPPY, P_LT_TUNE, P_CY_TONE,
                      P_CY_DECAY, P_OH_DECAY, P_ACCENT] {
            assert!(DrumKit::Kit808.is_live(index), "{}", PARAM_NAMES[index]);
        }
    }

    /// Every level knob reaches its own instruments and only its own.
    #[test]
    fn each_level_knob_silences_only_its_own_strip() {
        // note, the level knob that should silence it
        const ROUTING: &[(u8, usize)] = &[
            (36, P_BD_LEVEL),   // kick
            (30, P_BD_LEVEL),   // sub kick
            (38, P_SD_LEVEL),   // snare
            (40, P_SD_LEVEL),   // snare 2
            (41, P_LT_LEVEL),   // low tom
            (45, P_MT_LEVEL),   // mid tom
            (48, P_HT_LEVEL),   // high tom
            (64, P_LT_LEVEL),   // low conga
            (63, P_MT_LEVEL),   // open hi conga
            (37, P_RS_LEVEL),   // rimshot
            (75, P_RS_LEVEL),   // clave
            (39, P_CP_LEVEL),   // hand clap
            (70, P_CP_LEVEL),   // maracas
            (56, P_CB_LEVEL),   // cowbell
            (49, P_CY_LEVEL),   // crash
            (52, P_CY_LEVEL),   // cymbal
            (51, P_CY_LEVEL),   // ride — the 808 has no ride circuit of its own
            (46, P_OH_LEVEL),   // open hat
            (42, P_CH_LEVEL),   // closed hat
            (44, P_CH_LEVEL),   // pedal hat
        ];
        for &(note, knob) in ROUTING {
            let loud = peak(&strike(0, note, 127, 0.5, &[]));
            assert!(loud > 0.001, "note {note} is silent at full level");
            for &(_, other) in ROUTING {
                let cut = peak(&strike(0, note, 127, 0.5, &[(other, 0.0)]));
                if other == knob {
                    assert_eq!(cut, 0.0, "note {note} survived {}", PARAM_NAMES[other]);
                } else {
                    assert_eq!(
                        cut, loud,
                        "note {note} answered to {}, which is not its strip",
                        PARAM_NAMES[other]
                    );
                }
            }
        }
    }

    /// Every kit routes its notes to the strips the panel says, and answers
    /// the shaping knobs its machine has.
    #[test]
    fn every_kit_answers_the_panel() {
        for (kit, name) in KIT_LABELS.iter().enumerate() {
            let machine = DrumKit::from_index(kit);
            for (note, own, other) in [
                (36u8, P_BD_LEVEL, P_CY_LEVEL),
                (38, P_SD_LEVEL, P_BD_LEVEL),
                (42, P_CH_LEVEL, P_OH_LEVEL),
                (46, P_OH_LEVEL, P_CH_LEVEL),
                (49, P_CY_LEVEL, P_BD_LEVEL),
            ] {
                let loud = peak(&strike(kit, note, 127, 0.5, &[]));
                assert!(loud > 0.001, "{name} note {note} is silent");
                assert_eq!(peak(&strike(kit, note, 127, 0.5, &[(own, 0.0)])), 0.0, "{name} {note}");
                assert_eq!(peak(&strike(kit, note, 127, 0.5, &[(other, 0.0)])), loud, "{name} {note}");
            }
            // The kick's decay knob shortens the kick on every machine that
            // has one — and does nothing at all on the two that do not. A
            // TR-707 is fifteen fixed recordings and a TR-606 is seven fixed
            // circuits; neither of them has a decay control anywhere.
            let short = strike(kit, 36, 127, 3.0, &[(P_BD_DECAY, 0.0)]);
            let long = strike(kit, 36, 127, 3.0, &[(P_BD_DECAY, 1.0)]);
            if machine.is_live(P_BD_DECAY) {
                assert!(
                    decay_time(&long, -20.0) > decay_time(&short, -20.0) * 1.5,
                    "{name}: bd decay moved {:.3} s to {:.3} s",
                    decay_time(&short, -20.0),
                    decay_time(&long, -20.0)
                );
            } else {
                assert_eq!(short, long, "{name} has no bass drum decay knob");
            }
            // ...and it leaves the hats alone, which is the whole point of the
            // panel: one global decay knob used to shorten everything at once.
            let hat_short = strike(kit, 42, 127, 1.0, &[(P_BD_DECAY, 0.0)]);
            let hat_long = strike(kit, 42, 127, 1.0, &[(P_BD_DECAY, 1.0)]);
            assert_eq!(hat_short, hat_long, "{name}: the kick's decay knob moved the closed hat");
        }
    }

    // ── The 808's own voices ──

    /// The regression this whole file was rewritten around.
    ///
    /// Voices used to be freed when the *current sample* fell near zero, which
    /// a decaying sine does twice a cycle: the kick was cut off 81 ms into a
    /// tail that should have run for a second, the open hat at 19 ms, and the
    /// clap at 12 ms — before three of its four bursts had fired.
    #[test]
    fn a_hit_is_not_cut_off_part_way_through_its_tail() {
        // Bass drum at the centre detent: −20 dB at 300 ms, and still ringing
        // well past that.
        let kick = strike(0, 36, 127, 3.0, &[]);
        assert!(
            decay_time(&kick, -40.0) > 0.5,
            "kick reached −40 dB after only {:.3} s",
            decay_time(&kick, -40.0)
        );
        // Open hat, which used to be a byte-identical copy of the closed one.
        let open = strike(0, 46, 127, 2.0, &[]);
        let closed = strike(0, 42, 127, 2.0, &[]);
        assert!(
            decay_time(&open, -20.0) > decay_time(&closed, -20.0) * 3.0,
            "open hat {:.3} s vs closed {:.3} s",
            decay_time(&open, -20.0),
            decay_time(&closed, -20.0)
        );
        // Clap: four gates 10 ms apart, so there is still signal at 35 ms.
        let clap = strike(0, 39, 127, 1.0, &[]);
        let late = peak(&clap[(0.030 * SR) as usize..(0.040 * SR) as usize]);
        assert!(late > 0.2 * peak(&clap), "clap was over by 30 ms: {late}");
    }

    /// Published: 50 ms, 300 ms and 800 ms at the two ends and the centre
    /// detent of the bass drum's decay knob, quoted as −20 dB times.
    #[test]
    fn the_bass_drum_rings_for_its_published_decay_time() {
        for (knob, want) in [(0.0f32, 0.050f64), (0.5, 0.300), (1.0, 0.800)] {
            let x = strike(0, 36, 127, 4.0, &[(P_BD_DECAY, knob)]);
            let got = decay_time(&x, -20.0);
            assert!(
                (got - want).abs() < want * 0.1,
                "decay knob {knob}: −20 dB at {got:.3} s, published {want:.3} s"
            );
            assert!(
                (param_seconds(P_BD_DECAY, knob).unwrap() - want).abs() < 0.001,
                "the panel reads back a different time than it renders"
            );
        }
    }

    /// The bridged-T settles at 49.4 Hz, having started its ring near 130 Hz.
    ///
    /// It used to sit at 42 Hz and start at 378 — nine times its resting
    /// pitch, where the instrument's own attack is under three times.
    #[test]
    fn the_bass_drum_settles_at_the_frequency_of_its_resonator() {
        let x = strike(0, 36, 127, 2.0, &[]);
        let settled = &x[(0.100 * SR) as usize..(0.100 * SR) as usize + 16384];
        let f = strongest(settled, 30.0, 200.0, 0.1);
        assert!((f - 49.4).abs() < 1.0, "settled at {f:.1} Hz, want 49.4");

        // And the attack is up around 130 Hz for a few milliseconds. Six
        // milliseconds is too short a window to resolve 130 Hz with a
        // transform, so this measures the first half cycle instead: at a flat
        // 49.4 Hz it would arrive at 10.1 ms, with the sweep the circuit has
        // at 4.6 ms, and with the ninefold sweep this used to have at 1.5 ms.
        let half_cycle = x
            .windows(2)
            .position(|w| w[0] > 0.0 && w[1] <= 0.0)
            .map(|i| (i + 1) as f64 / SR)
            .expect("the kick never crossed zero");
        assert!(
            (0.0035..0.0060).contains(&half_cycle),
            "the first half cycle took {:.1} ms; the attack sweep puts it at 4.6",
            half_cycle * 1000.0
        );
    }

    /// The snare's two bridged-T oscillators are 238 Hz and 476 Hz in the
    /// service notes. TONE is the balance between them, not their tuning.
    #[test]
    fn the_snare_oscillators_are_at_the_service_note_frequencies() {
        let low = strike(0, 38, 127, 1.0, &[(P_SD_TONE, 0.0), (P_SD_SNAPPY, 0.0)]);
        let high = strike(0, 38, 127, 1.0, &[(P_SD_TONE, 1.0), (P_SD_SNAPPY, 0.0)]);
        assert!((strongest(&low[..8192], 100.0, 800.0, 0.5) - 238.0).abs() < 3.0);
        assert!((strongest(&high[..8192], 100.0, 800.0, 0.5) - 476.0).abs() < 5.0);

        // The tone knob moves the balance without moving the tuning: both
        // partials are present at both ends.
        for x in [&low, &high] {
            for hz in [238.0, 476.0] {
                assert!(magnitude(&x[..8192], hz) > 1e-5, "{hz} Hz is missing");
            }
        }

        // SNAPPY is the noise against the drum, so it moves the balance of the
        // spectrum without silencing either half.
        let dry = strike(0, 38, 127, 1.0, &[(P_SD_SNAPPY, 0.0)]);
        let snappy = strike(0, 38, 127, 1.0, &[(P_SD_SNAPPY, 1.0)]);
        let dry_hf = energy_above(&dry[..8192], 1500.0);
        let snappy_hf = energy_above(&snappy[..8192], 1500.0);
        assert!(snappy_hf > dry_hf * 3.0, "snappy {snappy_hf:.3} vs dry {dry_hf:.3}");
    }

    /// The six oscillators are one bank, running whether or not anything is
    /// gating them, so two hats struck at different times are not the same
    /// waveform.
    #[test]
    fn the_metal_oscillators_free_run() {
        let mut rack = DrumRack::new();
        rack.init(SR, 512);
        let mut left = vec![0.0f32; 512];
        let mut right = vec![0.0f32; 512];

        let mut hit = |rack: &mut DrumRack, offset: u32, blocks: usize| {
            let mut first: Vec<f32> = Vec::new();
            for b in 0..blocks {
                left.fill(0.0);
                right.fill(0.0);
                let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
                if b == 0 {
                    rack.process(&[], &mut outs, &[note_on(42, 127, offset)]);
                } else {
                    rack.process(&[], &mut outs, &[]);
                }
                if b == 0 {
                    first.extend_from_slice(&left[offset as usize..]);
                }
            }
            first
        };

        let a = hit(&mut rack, 0, 4);
        // 231 samples later: not a whole number of cycles of any of the six.
        let b = hit(&mut rack, 231, 4);
        let n = a.len().min(b.len());
        let diff: f32 = a[..n].iter().zip(&b[..n]).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff > 0.01, "two hats at different times were identical: {diff}");

        // The frequencies themselves are the published ones.
        assert_eq!(HAT_FREQS_808, [205.3, 304.4, 369.6, 522.7, 540.0, 800.0]);
    }

    /// A closed hat takes the hi-hat VCA from an open one.
    #[test]
    fn a_closed_hat_chokes_an_open_one() {
        let mut rack = DrumRack::new();
        rack.init(SR, 256);
        let mut left = vec![0.0f32; 256];
        let mut right = vec![0.0f32; 256];
        let mut tail = 0.0f32;
        for block in 0..40 {
            left.fill(0.0);
            right.fill(0.0);
            let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
            let events: &[MidiEvent] = match block {
                0 => &[MidiEvent { sample_offset: 0, status: 0x90, data1: 46, data2: 127 }],
                8 => &[MidiEvent { sample_offset: 0, status: 0x90, data1: 42, data2: 127 }],
                _ => &[],
            };
            rack.process(&[], &mut outs, events);
            // 90 ms after the closed hat, which is well past its own 50 ms.
            if block >= 24 {
                tail = tail.max(peak(&left));
            }
        }
        let open_alone = strike(0, 46, 127, 0.30, &[]);
        let uncut = peak(&open_alone[(0.140 * SR) as usize..]);
        assert!(
            tail < uncut * 0.25,
            "the open hat was still ringing at {tail:.5} after a closed hat; \
             on its own it is {uncut:.5}"
        );
    }

    /// The oscillators are 205-800 Hz square waves and the cymbal is not a low
    /// sound: each metal VCA runs into a high-pass, and it has to be steep
    /// enough to keep the fundamentals out. Half the cymbal's energy used to
    /// sit below 2 kHz.
    #[test]
    fn the_cymbal_keeps_the_oscillator_fundamentals_out_of_its_output() {
        let x = strike(0, 52, 127, 1.0, &[]);
        // Measured 0.05. It was 0.15 when the cymbal path had no high-pass
        // filter at all, so the bound has to be inside that to mean anything.
        let low = 1.0 - energy_above(&x[..16384], 1000.0);
        assert!(low < 0.10, "{low:.3} of the cymbal is below 1 kHz");
        let hat = strike(0, 42, 127, 0.5, &[]);
        let hat_low = 1.0 - energy_above(&hat[..16384], 2000.0);
        assert!(hat_low < 0.1, "{hat_low:.3} of the closed hat is below 2 kHz");
        // ...and the hat is the brighter of the two, which is what the two
        // different high-pass corners are for.
        assert!(energy_above(&hat[..16384], 6000.0) > energy_above(&x[..16384], 6000.0));
    }

    /// The cymbal's DECAY knob is on the 3440 Hz body path and its TONE knob
    /// is the balance between the two paths.
    #[test]
    fn the_cymbal_decay_and_tone_do_what_the_panel_says() {
        let short = strike(0, 52, 127, 4.0, &[(P_CY_DECAY, 0.0)]);
        let long = strike(0, 52, 127, 4.0, &[(P_CY_DECAY, 1.0)]);
        assert!(
            decay_time(&long, -20.0) > decay_time(&short, -20.0) * 2.5,
            "decay knob moved {:.3} s to {:.3} s",
            decay_time(&short, -20.0),
            decay_time(&long, -20.0)
        );
        let dark = strike(0, 52, 127, 1.0, &[(P_CY_TONE, 0.0)]);
        let bright = strike(0, 52, 127, 1.0, &[(P_CY_TONE, 1.0)]);
        assert!(
            energy_above(&bright[..16384], 5000.0) > energy_above(&dark[..16384], 5000.0) * 2.0
        );
    }

    /// The clap is one noise source gated four times by a 100 Hz retrigger,
    /// over a long tail on a second VCA.
    #[test]
    fn the_clap_fires_four_times_ten_milliseconds_apart() {
        let x = strike(0, 39, 127, 1.0, &[]);
        // Peak in each 10 ms window: four gates, then tail only.
        let step = (0.010 * SR) as usize;
        let gates: Vec<f32> = (0..5).map(|i| peak(&x[i * step..(i + 1) * step])).collect();
        for (i, g) in gates.iter().take(4).enumerate() {
            assert!(*g > 0.3 * gates[0], "gate {i} is missing: {gates:?}");
        }
        assert!(gates[4] < gates[0] * 0.6, "the gates never stopped: {gates:?}");
        // The band-pass is at 1 kHz.
        let f = strongest(&x[..4096], 200.0, 4000.0, 5.0);
        assert!((f - 1000.0).abs() < 250.0, "clap centred at {f:.0} Hz");
        // The tail outlasts the gates.
        assert!(decay_time(&x, -40.0) > 0.15, "no tail: {:.3} s", decay_time(&x, -40.0));
    }

    /// Toms and congas are the same three bridged-T boards at two tunings, and
    /// the TUNING knob moves them a couple of semitones either way rather than
    /// the octave a synthesizer would give.
    #[test]
    fn the_tom_boards_are_tuned_where_the_circuit_analysis_puts_them() {
        for (note, want) in [(41u8, 92.0), (45, 140.0), (48, 200.0)] {
            let x = strike(0, note, 127, 1.0, &[]);
            let body = &x[(0.050 * SR) as usize..(0.050 * SR) as usize + 8192];
            let f = strongest(body, 40.0, 500.0, 0.25);
            assert!((f - want).abs() < want * 0.03, "note {note} at {f:.0} Hz, want {want}");
        }
        let knob = [(P_LT_TUNE, 0.0f32)];
        let down = strike(0, 41, 127, 1.0, &knob);
        let f_down = strongest(&down[(0.050 * SR) as usize..(0.050 * SR) as usize + 8192], 40.0, 500.0, 0.25);
        let ratio = 92.0 / f_down;
        assert!(
            (1.10..1.20).contains(&ratio),
            "the tuning knob moved the low tom by {:.2}x, want about a tone and a half",
            ratio
        );
    }

    /// The accent bus sets how much of the velocity reaches the voices, and a
    /// bigger trigger pulse makes a louder *and* a longer sound.
    #[test]
    fn accent_is_the_depth_of_the_trigger_bus() {
        let open = |vel| peak(&strike(0, 36, vel, 2.0, &[(P_ACCENT, 1.0)]));
        let shut = |vel| peak(&strike(0, 36, vel, 2.0, &[(P_ACCENT, 0.0)]));
        // Knob down: every step arrives at the same 3.5 V, which is what that
        // knob does on the instrument.
        assert!((shut(30) - shut(127)).abs() < 1e-6);
        // Knob up: velocity reads as written, and a full-scale hit is exactly
        // as loud as it was before the rack had an accent knob at all.
        assert!(open(127) > open(10) * 3.0, "{} vs {}", open(127), open(10));
        assert!((open(127) - shut(127) * (1.0 / TRIGGER_MIN as f32)).abs() < 0.01);
        // ...and rings longer, because the accent feeds the trigger.
        let loud = strike(0, 36, 127, 3.0, &[(P_ACCENT, 1.0)]);
        let quiet = strike(0, 36, 20, 3.0, &[(P_ACCENT, 1.0)]);
        assert!(
            decay_time(&loud, -20.0) > decay_time(&quiet, -20.0) * 1.1,
            "{:.3} s vs {:.3} s",
            decay_time(&loud, -20.0),
            decay_time(&quiet, -20.0)
        );
    }

    /// A voice is freed when it has finished and not before, and it is freed:
    /// sixteen slots do not survive a bar of hats if nothing ever lets go.
    ///
    /// Every kit, because the two PCM machines lean on the rack's peak
    /// follower for it — their sampled drums decay inside the data and have no
    /// envelope of their own to test.
    #[test]
    fn voices_are_freed_when_they_finish() {
        for (kit, name) in KIT_LABELS.iter().enumerate() {
            let mut rack = DrumRack::new();
            rack.init(SR, 256);
            rack.set_parameter(P_KIT, kit_knob(kit));
            let mut left = vec![0.0f32; 256];
            let mut right = vec![0.0f32; 256];
            for block in 0..200 {
                left.fill(0.0);
                right.fill(0.0);
                let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
                let events = [note_on(42 + (block % 3) as u8 * 2, 127, 0)];
                rack.process(&[], &mut outs, &events);
            }
            // Let everything ring out: 12 s is the ceiling on one voice.
            for _ in 0..2100 {
                left.fill(0.0);
                right.fill(0.0);
                let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
                rack.process(&[], &mut outs, &[]);
            }
            assert!(
                rack.voices.iter().all(|v| !v.active),
                "{name}: {} voices never finished",
                rack.voices.iter().filter(|v| v.active).count()
            );
        }
    }

    /// Normalised correlation of two signals, 1.0 when one is a scalar
    /// multiple of the other.
    fn correlation(a: &[f32], b: &[f32]) -> f64 {
        let n = a.len().min(b.len());
        let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
        for i in 0..n {
            let (x, y) = (f64::from(a[i]), f64::from(b[i]));
            ab += x * y;
            aa += x * x;
            bb += y * y;
        }
        ab / (aa * bb).sqrt().max(1e-30)
    }

    /// The same note struck twice at different points in the bar, as one rack
    /// renders it. `offset` is where inside the first block the trigger lands.
    fn two_hits(kit: usize, note: u8, blocks: usize) -> (Vec<f32>, Vec<f32>) {
        let mut rack = DrumRack::new();
        rack.init(SR, 512);
        let mut left = vec![0.0f32; 512];
        let mut right = vec![0.0f32; 512];
        let mut hit = |rack: &mut DrumRack, offset: u32| {
            let mut first: Vec<f32> = Vec::new();
            for b in 0..blocks {
                left.fill(0.0);
                right.fill(0.0);
                let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
                if b == 0 {
                    rack.process(&[], &mut outs, &[note_on(note, 127, offset)]);
                    first.extend_from_slice(&left[offset as usize..]);
                } else {
                    rack.process(&[], &mut outs, &[]);
                    first.extend_from_slice(&left);
                }
            }
            first
        };
        rack.set_parameter(P_KIT, kit_knob(kit));
        let a = hit(&mut rack, 0);
        // 231 samples later, which is not a whole cycle of anything.
        let b = hit(&mut rack, 231);
        let n = a.len().min(b.len());
        (a[..n].to_vec(), b[..n].to_vec())
    }

    // ── The 909: analog drums, three sampled cymbals ──

    /// The knobs on the panel that are not on a TR-909: it has no bass-drum
    /// tone, no cymbal tone or decay — its crash and its ride take TUNE, which
    /// is a playback rate — and no cowbell circuit at all.
    #[test]
    fn the_knobs_the_909_lacks_do_nothing_on_it() {
        for index in [P_BD_TONE, P_CY_TONE, P_CY_DECAY] {
            assert!(!DrumKit::Kit909.is_live(index), "{}", PARAM_NAMES[index]);
            for note in [36u8, 38, 42, 46, 49, 51, 52] {
                let plain = strike(1, note, 127, 0.3, &[]);
                let moved = strike(1, note, 127, 0.3, &[(index, 1.0)]);
                assert_eq!(
                    plain, moved,
                    "{} moved note {note} on the 909, where that knob is not on the panel",
                    PARAM_NAMES[index]
                );
            }
        }
        // ...and the ones it does have, which are most of what phase 1
        // reserved room for.
        for index in [
            P_BD_TUNE,
            P_BD_ATTACK,
            P_BD_DECAY,
            P_SD_TUNE,
            P_SD_TONE,
            P_SD_SNAPPY,
            P_LT_DECAY,
            P_MT_DECAY,
            P_HT_DECAY,
            P_RD_LEVEL,
            P_RD_TUNE,
            P_CH_DECAY,
        ] {
            assert!(DrumKit::Kit909.is_live(index), "{}", PARAM_NAMES[index]);
        }
    }

    /// The one control on this machine that is most often described backwards.
    ///
    /// The 909's bass-drum TUNE knob does not tune the bass drum. It sets how
    /// long the oscillator runs above its resting frequency at the start of
    /// the note, so the drum settles at the same pitch whatever it says and
    /// what changes is the length of the sweep.
    #[test]
    fn the_909_bass_drums_tune_knob_is_the_length_of_its_sweep_not_its_pitch() {
        let settled = |x: &[f32]| {
            let from = (0.5 * SR) as usize;
            strongest(&x[from..from + 16384], 30.0, 200.0, 0.25)
        };
        let at_20ms = |x: &[f32]| {
            let from = (0.020 * SR) as usize;
            strongest(&x[from..from + 4096], 30.0, 400.0, 1.0)
        };
        let down = strike(1, 36, 127, 2.0, &[(P_BD_TUNE, 0.0)]);
        let up = strike(1, 36, 127, 2.0, &[(P_BD_TUNE, 1.0)]);

        assert!((settled(&down) - 55.0).abs() < 2.0, "settled at {:.1} Hz", settled(&down));
        assert!((settled(&up) - 55.0).abs() < 3.0, "settled at {:.1} Hz", settled(&up));
        assert!(
            at_20ms(&up) > at_20ms(&down) * 2.0,
            "20 ms in, the sweep is at {:.0} Hz with the knob up and {:.0} Hz with it down",
            at_20ms(&up),
            at_20ms(&down)
        );
    }

    /// The 909's decay knob extends the ring where the 808's gates it: 100 ms
    /// to 1.5 s against the 808's 50 to 800.
    #[test]
    fn the_909_bass_drum_decay_reaches_a_second_and_a_half() {
        let short = decay_time(&strike(1, 36, 127, 4.0, &[(P_BD_DECAY, 0.0)]), -20.0);
        let long = decay_time(&strike(1, 36, 127, 4.0, &[(P_BD_DECAY, 1.0)]), -20.0);
        assert!((0.06..0.16).contains(&short), "knob down: {short:.3} s");
        assert!((1.3..1.7).contains(&long), "knob up: {long:.3} s");
        // Twice the 808's, which is the difference the two kicks are known by.
        let eight = decay_time(&strike(0, 36, 127, 4.0, &[(P_BD_DECAY, 1.0)]), -20.0);
        assert!(long > eight * 1.7, "909 {long:.3} s vs 808 {eight:.3} s");
    }

    /// TONE is the length of the snare's noise and SNAPPY is its level. The
    /// two oscillators reach the output at the same level whatever either
    /// knob says, which is what makes them the two knobs they are.
    #[test]
    fn the_909_snare_tone_is_the_length_of_its_noise_and_snappy_is_its_level() {
        let short = strike(1, 38, 127, 2.0, &[(P_SD_TONE, 0.0)]);
        let long = strike(1, 38, 127, 2.0, &[(P_SD_TONE, 1.0)]);
        assert!(
            decay_time(&long, -20.0) > decay_time(&short, -20.0) * 3.0,
            "tone moved the noise from {:.3} s to {:.3} s",
            decay_time(&short, -20.0),
            decay_time(&long, -20.0)
        );

        let dry = strike(1, 38, 127, 2.0, &[(P_SD_SNAPPY, 0.0)]);
        let snappy = strike(1, 38, 127, 2.0, &[(P_SD_SNAPPY, 1.0)]);
        assert!(
            energy_above(&snappy[..8192], 2000.0) > energy_above(&dry[..8192], 2000.0) * 3.0,
            "snappy {:.3} vs dry {:.3}",
            energy_above(&snappy[..8192], 2000.0),
            energy_above(&dry[..8192], 2000.0)
        );
        let low = |x: &[f32]| magnitude(&x[..8192], racks::kit_909::SD_LOW_HZ);
        assert!(
            (low(&dry) - low(&snappy)).abs() < low(&dry) * 0.05,
            "snappy moved the drum as well as the wires: {:.6} to {:.6}",
            low(&dry),
            low(&snappy)
        );
    }

    /// Each of the three toms has its own DECAY knob, which the 808 does not
    /// have at all, and a band of noise ringing under the tone, which the
    /// 808's pure sine does not have either.
    #[test]
    fn the_909_toms_take_a_decay_knob_and_carry_noise_under_the_tone() {
        for (note, knob) in [(41u8, P_LT_DECAY), (45, P_MT_DECAY), (48, P_HT_DECAY)] {
            let short = decay_time(&strike(1, note, 127, 3.0, &[(knob, 0.0)]), -20.0);
            let long = decay_time(&strike(1, note, 127, 3.0, &[(knob, 1.0)]), -20.0);
            assert!(long > short * 3.0, "note {note}: {short:.3} s to {long:.3} s");
        }
        let nine = strike(1, 41, 127, 1.0, &[]);
        let eight = strike(0, 41, 127, 1.0, &[]);
        assert!(
            energy_above(&nine[..16384], 1000.0) > energy_above(&eight[..16384], 1000.0) * 3.0,
            "909 tom {:.4} vs 808 tom {:.4} above 1 kHz",
            energy_above(&nine[..16384], 1000.0),
            energy_above(&eight[..16384], 1000.0)
        );
    }

    /// The hybrid, measured: the hi-hat, the ride and the crash are read out
    /// of ROM at 18 kHz, so nothing in them can be above 9 kHz. The snare next
    /// to them is a circuit and has no such ceiling.
    #[test]
    fn the_909s_cymbals_are_sampled_and_the_drums_beside_them_are_not() {
        for (name, note) in [("closed hat", 42u8), ("open hat", 46), ("crash", 49), ("ride", 51)] {
            let x = strike(1, note, 127, 2.0, &[]);
            let over = energy_above(&x[..16384], 9500.0);
            assert!(over < 0.03, "{name} has {over:.4} of its energy above the 9 kHz ceiling");
        }
        // The snare beside them is a circuit: measured at 0.41, twenty-five
        // times what the sampled voices have up there.
        let snare = strike(1, 38, 127, 1.0, &[]);
        assert!(
            energy_above(&snare[..16384], 9500.0) > 0.2,
            "the 909's snare is analog and should not be band-limited: {:.4}",
            energy_above(&snare[..16384], 9500.0)
        );
    }

    /// Open and closed hat are one recording with two envelope generators,
    /// which is why choking one with the other works the way it does.
    #[test]
    fn the_909s_two_hats_read_the_same_sample() {
        let closed = strike(1, 42, 127, 0.2, &[]);
        let open = strike(1, 46, 127, 0.2, &[]);
        let n = (0.003 * SR) as usize;
        let r = correlation(&closed[..n], &open[..n]);
        assert!(r > 0.99, "the two hats' first 3 ms correlate at only {r:.4}");
        // ...and the envelopes are what separate them.
        assert!(decay_time(&open, -20.0) > decay_time(&closed, -20.0) * 4.0);
    }

    /// A TR-909 has no cowbell. The rimshot is the only pitched click on the
    /// machine, so that is where a cowbell part goes — and it answers the
    /// rimshot's fader, because that is the one in front of the player.
    #[test]
    fn the_909_has_no_cowbell() {
        assert!(!DrumKit::Kit909.is_live(P_CB_LEVEL));
        let plain = peak(&strike(1, 56, 127, 0.5, &[]));
        assert!(plain > 0.001);
        assert_eq!(peak(&strike(1, 56, 127, 0.5, &[(P_CB_LEVEL, 0.0)])), plain);
        assert_eq!(peak(&strike(1, 56, 127, 0.5, &[(P_RS_LEVEL, 0.0)])), 0.0);
    }

    // ── The 707: fifteen recordings ──

    /// The knobs on the panel that are not on a TR-707, which is all of them
    /// but the faders and the accent.
    #[test]
    fn the_knobs_the_707_lacks_do_nothing_on_it() {
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            if DrumKit::Kit707.is_live(index) {
                continue;
            }
            for note in [36u8, 38, 41, 42, 46, 49, 51, 56] {
                let plain = strike(2, note, 127, 0.3, &[]);
                let moved = strike(2, note, 127, 0.3, &[(index, 1.0)]);
                assert_eq!(
                    plain, moved,
                    "{name} moved note {note} on the 707, which has no such control"
                );
            }
        }
        // Every fader and the accent bus are live, and they are the panel.
        for index in [P_ACCENT, P_BD_LEVEL, P_SD_LEVEL, P_RS_LEVEL, P_CB_LEVEL, P_RD_LEVEL] {
            assert!(DrumKit::Kit707.is_live(index), "{}", PARAM_NAMES[index]);
        }
    }

    /// A sampled machine plays the same waveform every time, and an analog one
    /// cannot. This is the plainest difference between the two kinds of
    /// machine in this rack, and it is what a 707 hi-hat pattern sounds like.
    #[test]
    fn the_707_plays_the_same_waveform_every_time_it_is_struck() {
        let (a, b) = two_hits(2, 42, 3);
        let diff: f32 = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff < 1e-6, "two 707 hats at different times differed by {diff}");
        // The same test on the 808, whose six oscillators free-run.
        let (a, b) = two_hits(0, 42, 3);
        let diff: f32 = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff > 0.01, "two 808 hats at different times were identical: {diff}");
    }

    /// Nothing in a 707 can be above 12.5 kHz, because its converter runs at
    /// 25 kHz. What is left up there is the hold's own images through the
    /// output filter, which is a percent or so — against the 45% an analog
    /// noise voice puts there.
    #[test]
    fn the_707_has_the_ceiling_of_its_own_converter() {
        for note in [38u8, 42, 46, 49] {
            let x = strike(2, note, 127, 1.0, &[]);
            let over = energy_above(&x[..16384], 12_500.0);
            assert!(over < 0.03, "note {note} has {over:.4} of its energy above 12.5 kHz");
        }
        // A shaker made by a circuit for comparison: the 808's maracas are
        // high-passed noise and nothing bounds them at the top, so 45% of
        // that sound is above 12.5 kHz where a 707's tambourine has 1%.
        let analog_shaker = strike(0, 70, 127, 1.0, &[]);
        assert!(
            energy_above(&analog_shaker[..16384], 12_500.0) > 0.3,
            "the 808's maracas are noise and have no ceiling: {:.4}",
            energy_above(&analog_shaker[..16384], 12_500.0)
        );
    }

    /// The envelope generator is *after* the converter on both PCM machines,
    /// so the quantisation noise is multiplied by the envelope along with the
    /// signal and decays with the note.
    ///
    /// Crush after the envelope instead — which is what this rack used to do
    /// to the 909's hats — and the step size stays fixed while the signal
    /// falls, so the noise's share of what is left grows as the note decays.
    /// That is what this measures: the share does not grow.
    #[test]
    fn the_sampled_voices_take_their_quantisation_noise_down_with_the_note() {
        for (name, kit, note) in [("707 crash", 2usize, 49u8), ("909 crash", 1, 49)] {
            let x = strike(kit, note, 127, 2.0, &[]);
            let win = 8192;
            let early = energy_above(&x[..win], 6000.0);
            let late = {
                let from = (0.7 * SR) as usize;
                energy_above(&x[from..from + win], 6000.0)
            };
            assert!(
                late < early * 2.0,
                "{name}: the noise above 6 kHz went from {early:.4} of the sound to {late:.4} \
                 as it decayed, which is a bit-crusher after the envelope rather than a \
                 converter before it"
            );
        }
    }

    // ── The 606: seven voices ──

    /// The knobs on the panel that are not on a TR-606, which is all of them
    /// but the faders and the accent — and the faders of the five instruments
    /// the machine does not have.
    #[test]
    fn the_knobs_the_606_lacks_do_nothing_on_it() {
        // A dead fader is checked at the bottom of its travel, where it would
        // silence a strip if anything were routed to it; every other dead
        // knob at the top of its.
        const DEAD_FADERS: [usize; 5] =
            [P_MT_LEVEL, P_RS_LEVEL, P_CP_LEVEL, P_CB_LEVEL, P_RD_LEVEL];
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            if DrumKit::Kit606.is_live(index) {
                continue;
            }
            let value = if DEAD_FADERS.contains(&index) { 0.0 } else { 1.0 };
            for note in [36u8, 38, 41, 45, 48, 42, 46, 49, 56, 37] {
                let plain = strike(3, note, 127, 0.3, &[]);
                let moved = strike(3, note, 127, 0.3, &[(index, value)]);
                assert_eq!(
                    plain, moved,
                    "{name} moved note {note} on the 606, which has no such control"
                );
            }
        }
        for index in [P_ACCENT, P_BD_LEVEL, P_SD_LEVEL, P_LT_LEVEL, P_HT_LEVEL, P_CY_LEVEL,
                      P_OH_LEVEL, P_CH_LEVEL] {
            assert!(DrumKit::Kit606.is_live(index), "{}", PARAM_NAMES[index]);
        }
    }

    /// Seven voices and no more, and the decision about the rest stated where
    /// it can be checked: a note the machine has no voice for is played on the
    /// nearest voice it does have, at that voice's fader.
    #[test]
    fn the_606_has_seven_voices_and_folds_everything_else_onto_them() {
        use racks::kit_606::{voice_606, Voice606};
        // note, the voice it lands on, the fader that silences it
        const FOLDS: &[(u8, Voice606, usize)] = &[
            (36, Voice606::Bd, P_BD_LEVEL),
            (38, Voice606::Sd, P_SD_LEVEL),
            (37, Voice606::Sd, P_SD_LEVEL),   // rimshot: no circuit
            (75, Voice606::Sd, P_SD_LEVEL),   // clave: no circuit
            (39, Voice606::Sd, P_SD_LEVEL),   // hand clap: no circuit
            (41, Voice606::LowTom, P_LT_LEVEL),
            (45, Voice606::LowTom, P_LT_LEVEL), // no mid tom
            (64, Voice606::LowTom, P_LT_LEVEL), // low conga: no circuit
            (48, Voice606::HighTom, P_HT_LEVEL),
            (56, Voice606::HighTom, P_HT_LEVEL), // cowbell: no circuit
            (49, Voice606::Cymbal, P_CY_LEVEL),
            (51, Voice606::Cymbal, P_CY_LEVEL),  // ride: one cymbal on this machine
            (52, Voice606::Cymbal, P_CY_LEVEL),
            (46, Voice606::OpenHat, P_OH_LEVEL),
            (42, Voice606::ClosedHat, P_CH_LEVEL),
            (70, Voice606::ClosedHat, P_CH_LEVEL), // maracas: no circuit
        ];
        for &(note, voice, fader) in FOLDS {
            assert_eq!(voice_606(note_to_sound(note)), voice, "note {note}");
            let loud = peak(&strike(3, note, 127, 0.6, &[]));
            assert!(loud > 0.001, "note {note} is silent on the 606");
            assert_eq!(
                peak(&strike(3, note, 127, 0.6, &[(fader, 0.0)])),
                0.0,
                "note {note} did not answer {}",
                PARAM_NAMES[fader]
            );
        }
        // The five strips the machine has no voice behind are dead, not
        // quietly playing something borrowed.
        for dead in [P_MT_LEVEL, P_RS_LEVEL, P_CP_LEVEL, P_CB_LEVEL, P_RD_LEVEL] {
            for &(note, _, _) in FOLDS {
                assert_eq!(
                    peak(&strike(3, note, 127, 0.6, &[(dead, 0.0)])),
                    peak(&strike(3, note, 127, 0.6, &[])),
                    "note {note} answered {}, which is not a fader on this machine",
                    PARAM_NAMES[dead]
                );
            }
        }
    }

    /// Where the service notes put the four pitched voices.
    #[test]
    fn the_606_voices_are_tuned_where_its_service_notes_put_them() {
        // Bass drum: IC5A at 60.0 Hz, with IC5B's 192 Hz knock over it for
        // the first few milliseconds. The 808's is 49.4 Hz and has no knock.
        let kick = strike(3, 36, 127, 2.0, &[]);
        let from = (0.080 * SR) as usize;
        let settled = strongest(&kick[from..from + 16384], 30.0, 300.0, 0.25);
        assert!((settled - 60.0).abs() < 1.5, "kick settled at {settled:.1} Hz, want 60");
        let knock = magnitude(&kick[..1024], 192.0);
        let body = magnitude(&kick[..1024], 60.0);
        assert!(knock > body * 0.1, "no second resonance: {knock:.6} against {body:.6}");

        // Snare: one oscillator at 358 Hz, where the 808 has two at 238 and
        // 476 Hz.
        let snare = strike(3, 38, 127, 1.0, &[]);
        let f = strongest(&snare[..8192], 150.0, 800.0, 0.5);
        assert!((f - 358.0).abs() < 5.0, "snare at {f:.1} Hz, want 358");

        // Two toms, an octave apart, and no third.
        for (note, want) in [(41u8, 150.0f64), (48, 300.0)] {
            let x = strike(3, note, 127, 1.0, &[]);
            let from = (0.050 * SR) as usize;
            let f = strongest(&x[from..from + 8192], 60.0, 500.0, 0.25);
            assert!((f - want).abs() < want * 0.03, "note {note} at {f:.0} Hz, want {want}");
        }
    }

    /// The 606's six metal oscillators run *below* its band-passes, as the
    /// 808's do.
    ///
    /// They used to be listed at 10.2 to 12.5 kHz, which is above the sample
    /// rate's ability to carry a square wave at all: every harmonic of a
    /// 12.5 kHz square is an alias, and the band-passes at 3.5 and 7.2 kHz
    /// were filtering nothing that belonged to them.
    #[test]
    fn the_606_metal_bank_runs_below_its_band_passes() {
        assert_eq!(HAT_FREQS_606, [145.2, 181.5, 216.3, 246.4, 259.5, 369.6]);
        for f in HAT_FREQS_606 {
            assert!(f < 400.0, "{f} Hz is not a Schmitt-trigger oscillator on this board");
        }
        // ...and what comes out is still a hi-hat: the fundamentals stay out
        // of it, as they do on the 808.
        for note in [42u8, 46, 49] {
            let x = strike(3, note, 127, 1.0, &[]);
            let low = 1.0 - energy_above(&x[..16384], 1000.0);
            assert!(low < 0.10, "note {note}: {low:.3} of it is below 1 kHz");
        }
    }

    /// Every 606 voice rings for the time this file says it does. The machine
    /// has no decay control, so these are fixed and there is nothing else to
    /// check them against.
    #[test]
    fn the_606_voices_ring_for_the_times_this_file_gives_them() {
        // note, seconds to −20 dB, tolerance
        const RING: &[(u8, f64, f64)] = &[
            (38, 0.170, 0.04),  // snare
            (41, 0.218, 0.04),  // low tom
            (48, 0.155, 0.04),  // high tom
            (49, 0.743, 0.10),  // cymbal
            (46, 0.332, 0.06),  // open hat
            (42, 0.067, 0.02),  // closed hat
        ];
        for &(note, want, tol) in RING {
            let got = decay_time(&strike(3, note, 127, 3.0, &[]), -20.0);
            assert!((got - want).abs() < tol, "note {note}: {got:.3} s, want {want:.3}");
        }
        // The kick is measured past its strike, which is louder than the body
        // it sets ringing: 250 ms of envelope after the first 15 ms.
        let kick = strike(3, 36, 127, 3.0, &[]);
        let body = decay_time(&kick[(0.015 * SR) as usize..], -20.0);
        assert!((body - 0.235).abs() < 0.05, "kick body {body:.3} s, want 0.235");
    }
}
