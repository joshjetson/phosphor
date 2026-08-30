//! Oberheim TEO-5: five-voice analog poly with the SEM state-variable filter,
//! through-zero FM, a sixteen-slot modulation matrix and all 256 factory
//! programs.
//!
//! Oberheim's 2024 instrument on the Sequential platform, and the filter
//! architecture the rest of this rack does not have. Two VCOs per voice with
//! three independently mixable waveshapes each, hard sync, **through-zero FM**
//! from oscillator 2's triangle into oscillator 1's frequency, a sub
//! oscillator and noise, and then the reason the instrument exists: a 2-pole
//! **state-variable filter** whose STATE control morphs continuously from low
//! pass through notch to high pass, with band pass replacing the notch on a
//! switch. Two DADSR envelopes with the OB-8's curves, a global LFO and a
//! per-voice LFO, and a 16 × 20 × 65 modulation matrix.
//!
//! ## Sources
//!
//! * *TEO-5 User's Guide* 1.0.0 and 1.0.2 (Oberheim, 2024). Every range,
//!   enumeration and behaviour in this file that says "the manual" is from
//!   one of those two; their Appendix A and Appendix B — the modulation
//!   source and destination lists — are byte-identical between the two
//!   revisions.
//! * *TEO-5 MIDI Implementation Document* (Oberheim, August 2024): NRPN
//!   Program Parameter Data 1–187, the CC map, and the packed SysEx format.
//! * `TEO5_Factory_Programs_v1.00.syx`, Oberheim's factory program set of
//!   8 July 2024, and `TEO5_Factory_Programs_Categorized_v1.00.syx`, the same
//!   256 programs in category order. See [`ROM`] and `examples/teo5_rom.rs`.
//!
//! ## The byte map, and why it is not guesswork
//!
//! Unlike the Prophet-6, whose parameter order in a program dump is *not* its
//! NRPN order, on the TEO-5 the file offset **is** the NRPN parameter number
//! for offsets 1–158 and 180–187. That was established from the bank rather
//! than assumed: the six holes the NRPN table leaves (29, 41, 49, 57, 74, 92)
//! are zero in all 256 programs and almost nothing else is; ten independent
//! fields top out at exactly their documented ceilings; the modulation matrix
//! lands where the table says with its sources inside 0–19 and its
//! destinations inside 0–64; and the routings that fall out are the three
//! most idiomatic in synthesis — mod wheel to LFO amount 127 times, pressure
//! to cutoff 123 times, voice spread to panning 71 times. The name is 20
//! ASCII at 159 and the category is the single byte at 179, which the
//! categorized bank proves by being this file sorted on it.
//!
//! ## Where this differs from the hardware, and why
//!
//! * **The reverb is stored and not rendered.** Effect 2 is a fixed plate
//!   reverb — not one of the twelve selectable algorithms, a separate
//!   always-available unit — and 171 of the 256 factory programs push its
//!   decay past 127. All six of its controls are on the panel, all six
//!   round-trip, and five of the 65 modulation destinations address it and
//!   are accepted. None of them make a sound yet: the effects milestone
//!   builds the shared reverb bus and connects it, exactly as the
//!   Prophet-6's four stored reverb types wait for the same bus. **Until
//!   then this bank renders drier than the hardware does** — 196 of the 256
//!   programs have reverb switched on, with a median mix of 22 of 127, so
//!   the difference is real and it is a tail rather than a timbre.
//! * **The arpeggiator and the 64-step sequencer are not here.** Both are in
//!   the factory dump, both are sequencing rather than sound, and that is the
//!   DAW's job — the same line the Prophet-6 draws. What *is* on the panel is
//!   the tempo and the effect and LFO sync divisions, because a synced delay
//!   and a synced LFO are sounds. The master clock's own divide control is
//!   not, because what it divides is the arpeggiator's step rate.
//! * **The alternative tunings are not here.** The parameter exists (65
//!   scales in the manual's Appendix F) and every one of the 256 factory
//!   programs is set to equal temperament, so there is nothing in this bank
//!   to render and no published table to render it with.
//! * **Chord memory arrives with the program.** Unlike the Prophet-6, whose
//!   memorised chord is not in its dump, the TEO-5 stores five semitone
//!   offsets per program and twenty of the 256 use them. Loading a program
//!   loads its chord; holding keys and switching unison on captures a new one,
//!   which is the hardware gesture.
//! * **Aftertouch works end to end**, as channel pressure, and so do breath
//!   (CC 2), foot (CC 4) and expression (CC 11) — three matrix sources the
//!   factory bank never uses but the matrix names.
//!
//! ## Raw values and physical units
//!
//! The factory programs are raw instrument bytes — 0–1, 0–7, 0–63, 0–127,
//! 0–254, 0–255, 0–511, 0–1023 — and every conversion into hertz, seconds,
//! semitones or cents is this file's, in the [`raw`] module, one function per
//! law with the manual's own words above it where the manual gives them and
//! an explicit note where it does not. The TEO-5 publishes more than most:
//! the oscillator's span (a 5-octave-plus-minor-third range), the detune's
//! (±49.2 cents), the pulse width's (50 % at minimum narrowing to 100 % and
//! silence), the LFO's (0.022 Hz to 500 Hz) and the cutoff's resolution
//! (0–1024, fully closed to fully open). It publishes nothing at all for the
//! envelope times, for what a modulation amount is worth at each destination,
//! or for the two clock-division tables, and those are marked as judgment at
//! their own constants with the bank's own numbers behind the choice.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

use crate::level::soft_saturate;

const PI: f64 = std::f64::consts::PI;
const TAU: f64 = std::f64::consts::TAU;

/// Fixed headroom trim on the output, applied after the program volume.
///
/// Sized the way the other nine are — see `OUTPUT_TRIM` in dx7.rs for the
/// full reasoning and level.rs for the gain structure — on the median program
/// of the bank playing the same C major triad at velocity 100 that
/// `instruments_are_level_matched` uses, landing inside the 0.0187 to 0.0314
/// band the rest of the rack occupies.
///
/// Two numbers set it and they pull against each other. The median program on
/// that triad has to land in that band, and the loudest thing the bank can be
/// asked for — five voices in unison on the hottest program at velocity 127
/// with the overdrive up — has to stay under the master limiter's ceiling of
/// 0.891 and, for ordinary playing, under the saturator's knee.
///
/// This filter makes that easier than the Prophet-6's did. A 2-pole
/// state-variable filter that does not self-oscillate has a bounded peak
/// gain — [`Q_MAX`] is 6, so +15.6 dB at the corner and nothing anywhere
/// else — where a compensated four-pole loop approaching marginal has none,
/// so the 21 programs that sit at the top of this resonance knob do not tower
/// over the bank the way the Prophet-6's twenty-nine did.
const OUTPUT_TRIM: f32 = 0.035;

// ── Parameter indices ──
//
// Front-panel order, section by section, as the manual's Chapter 2 walks it:
// the program selectors, OSCILLATORS 1 and 2, OSC MOD, the FILTER with its
// mixer, ENVELOPE 1, ENVELOPE 2, the envelope routing, the two LOW FREQUENCY
// OSCILLATORS, the MODULATION matrix, EFFECT, REVERB, the OVERDRIVE and
// VINTAGE knobs, UNISON, PORTAMENTO, the keyboard split and the miscellaneous
// program settings.
//
// `program` is first because index 0 is where the editor looks for a preset
// selector, and `bank` is second because the two are one control between them.

pub const P_PROGRAM: usize = 0;
pub const P_BANK: usize = 1;
// Oscillator 1
pub const P_O1_FREQ: usize = 2;
pub const P_O1_FINE: usize = 3;
pub const P_O1_TRI: usize = 4;
pub const P_O1_SAW: usize = 5;
pub const P_O1_PULSE: usize = 6;
pub const P_O1_WIDTH: usize = 7;
pub const P_O1_KEY: usize = 8;
pub const P_O1_GLIDE: usize = 9;
pub const P_O1_ON: usize = 10;
pub const P_O1_LEVEL: usize = 11;
// Oscillator 2
pub const P_O2_FREQ: usize = 12;
pub const P_O2_FINE: usize = 13;
pub const P_O2_TRI: usize = 14;
pub const P_O2_SAW: usize = 15;
pub const P_O2_PULSE: usize = 16;
pub const P_O2_WIDTH: usize = 17;
pub const P_O2_KEY: usize = 18;
pub const P_O2_GLIDE: usize = 19;
pub const P_O2_ON: usize = 20;
pub const P_O2_LEVEL: usize = 21;
pub const P_O2_BYPASS: usize = 22;
// Oscillator modulation
pub const P_XMOD: usize = 23;
pub const P_SYNC: usize = 24;
// Mixer
pub const P_SUB_ON: usize = 25;
pub const P_SUB_LEVEL: usize = 26;
pub const P_NOISE_ON: usize = 27;
pub const P_NOISE_TYPE: usize = 28;
pub const P_NOISE_LEVEL: usize = 29;
// Filter
pub const P_CUTOFF: usize = 30;
pub const P_RESONANCE: usize = 31;
pub const P_STATE: usize = 32;
pub const P_BANDPASS: usize = 33;
pub const P_FILTER_KEY: usize = 34;
// Envelope 1
pub const P_E1_AMOUNT: usize = 35;
pub const P_E1_VEL: usize = 36;
pub const P_E1_DELAY: usize = 37;
pub const P_E1_ATTACK: usize = 38;
pub const P_E1_DECAY: usize = 39;
pub const P_E1_SUSTAIN: usize = 40;
pub const P_E1_RELEASE: usize = 41;
// Envelope 2
pub const P_E2_AMOUNT: usize = 42;
pub const P_E2_VEL: usize = 43;
pub const P_E2_DELAY: usize = 44;
pub const P_E2_ATTACK: usize = 45;
pub const P_E2_DECAY: usize = 46;
pub const P_E2_SUSTAIN: usize = 47;
pub const P_E2_RELEASE: usize = 48;
// Envelope routing
pub const P_ENV_ROUTE: usize = 49;
pub const P_E1_DEST: usize = 50;
pub const P_ENV_REPEAT: usize = 51;
// LFO 1, the global one
pub const P_L1_FREQ: usize = 52;
pub const P_L1_SHAPE: usize = 53;
pub const P_L1_AMOUNT: usize = 54;
pub const P_L1_DEST: usize = 55;
pub const P_L1_SYNC: usize = 56;
pub const P_L1_DIV: usize = 57;
pub const P_L1_RESET: usize = 58;
pub const P_L1_SLEW: usize = 59;
// LFO 2, the per-voice one
pub const P_L2_FREQ: usize = 60;
pub const P_L2_SHAPE: usize = 61;
pub const P_L2_AMOUNT: usize = 62;
pub const P_L2_DEST: usize = 63;
pub const P_L2_SYNC: usize = 64;
pub const P_L2_DIV: usize = 65;
pub const P_L2_RESET: usize = 66;
pub const P_L2_SLEW: usize = 67;
/// The first of the modulation matrix's 48 controls. Slot `i` is
/// `P_MOD + 3*i` (source), `+ 1` (amount) and `+ 2` (destination).
pub const P_MOD: usize = 68;
/// Modulation slots, which is what the NRPN table enumerates and what six of
/// the factory programs fill completely.
pub const MOD_SLOTS: usize = 16;
// Effect 1
pub const P_FX_ON: usize = P_MOD + 3 * MOD_SLOTS;
pub const P_FX_TYPE: usize = P_FX_ON + 1;
pub const P_FX_MIX: usize = P_FX_ON + 2;
pub const P_FX_TIME: usize = P_FX_ON + 3;
pub const P_FX_MISC: usize = P_FX_ON + 4;
pub const P_FX_SYNC: usize = P_FX_ON + 5;
pub const P_FX_DIV: usize = P_FX_ON + 6;
// Effect 2: the dedicated plate reverb
pub const P_RV_ON: usize = P_FX_ON + 7;
pub const P_RV_MIX: usize = P_FX_ON + 8;
pub const P_RV_SIZE: usize = P_FX_ON + 9;
pub const P_RV_PREDELAY: usize = P_FX_ON + 10;
pub const P_RV_DECAY: usize = P_FX_ON + 11;
pub const P_RV_TONE: usize = P_FX_ON + 12;
// The voice's output stage
pub const P_OVERDRIVE: usize = P_FX_ON + 13;
pub const P_VINTAGE: usize = P_FX_ON + 14;
pub const P_VOLUME: usize = P_FX_ON + 15;
pub const P_PAN: usize = P_FX_ON + 16;
// Unison
pub const P_UNISON: usize = P_FX_ON + 17;
pub const P_UNISON_VOICES: usize = P_FX_ON + 18;
pub const P_UNISON_DETUNE: usize = P_FX_ON + 19;
pub const P_KEY_MODE: usize = P_FX_ON + 20;
pub const P_RETRIGGER: usize = P_FX_ON + 21;
// Portamento
pub const P_GLIDE: usize = P_FX_ON + 22;
pub const P_GLIDE_MODE: usize = P_FX_ON + 23;
// Keyboard split
pub const P_SPLIT_1: usize = P_FX_ON + 24;
pub const P_SPLIT_2: usize = P_FX_ON + 25;
pub const P_SPLIT_NOTE: usize = P_FX_ON + 26;
// Miscellaneous program settings
pub const P_BEND_UP: usize = P_FX_ON + 27;
pub const P_BEND_DOWN: usize = P_FX_ON + 28;
pub const P_TRANSPOSE: usize = P_FX_ON + 29;
pub const P_BPM: usize = P_FX_ON + 30;

pub const PARAM_COUNT: usize = P_BPM + 1;

/// Panel names, eight columns wide because that is the editor's column.
pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "program", "bank",
    "1 freq", "1 fine", "1 tri", "1 saw", "1 pulse", "1 width", "1 keybd", "1 glide", "osc 1",
    "1 level",
    "2 freq", "2 fine", "2 tri", "2 saw", "2 pulse", "2 width", "2 keybd", "2 glide", "osc 2",
    "2 level", "2 bypass",
    "x-mod", "sync 2>1",
    "sub", "sub lvl", "noise", "noise wv", "nois lvl",
    "cutoff", "res", "state", "bandpass", "flt keyb",
    "e1 amt", "e1 vel", "e1 delay", "e1 atk", "e1 dec", "e1 sus", "e1 rel",
    "e2 amt", "e2 vel", "e2 delay", "e2 atk", "e2 dec", "e2 sus", "e2 rel",
    "env rout", "e1 dest", "e repeat",
    "l1 rate", "l1 wave", "l1 amt", "l1 dest", "l1 sync", "l1 div", "l1 reset", "l1 slew",
    "l2 rate", "l2 wave", "l2 amt", "l2 dest", "l2 sync", "l2 div", "l2 reset", "l2 slew",
    "m1 src", "m1 amt", "m1 dst",
    "m2 src", "m2 amt", "m2 dst",
    "m3 src", "m3 amt", "m3 dst",
    "m4 src", "m4 amt", "m4 dst",
    "m5 src", "m5 amt", "m5 dst",
    "m6 src", "m6 amt", "m6 dst",
    "m7 src", "m7 amt", "m7 dst",
    "m8 src", "m8 amt", "m8 dst",
    "m9 src", "m9 amt", "m9 dst",
    "m10 src", "m10 amt", "m10 dst",
    "m11 src", "m11 amt", "m11 dst",
    "m12 src", "m12 amt", "m12 dst",
    "m13 src", "m13 amt", "m13 dst",
    "m14 src", "m14 amt", "m14 dst",
    "m15 src", "m15 amt", "m15 dst",
    "m16 src", "m16 amt", "m16 dst",
    "fx on", "fx type", "fx mix", "fx time", "fx misc", "fx sync", "fx div",
    "verb on", "verb mix", "verb siz", "verb pre", "verb dec", "verb ton",
    "overdriv", "vintage", "volume", "pan",
    "unison", "voices", "uni detu", "key mode", "retrig",
    "glide", "gld mode",
    "split -1", "split -2", "split at",
    "bend up", "bend dn", "transpos", "bpm",
];

// ── Panel selectors ──

/// The sixteen banks, as the front panel numbers them: 1 to 9, then A to G.
const BANK_DIGITS: [u8; BANK_COUNT] = *b"123456789ABCDEFG";

/// LFO shapes, manual page 31. The first is a **triangle**, not a sine, and
/// it is the only bipolar one: "The triangle wave is bipolar... The square,
/// sawtooth, reverse sawtooth, and sample & hold waves generate only positive
/// values. In the case of the square wave, this makes it possible to generate
/// trills."
const LFO_SHAPES: [&str; 5] = ["tri", "saw", "rev saw", "square", "s & h"];

/// Glide modes, manual page 51: fixed rate, fixed rate legato-only, fixed
/// time, fixed time legato-only. The two "A" modes are unison-only on the
/// hardware and are treated the same way here.
const GLIDE_MODES: [&str; 4] = ["rate", "rate A", "time", "time A"];

/// Unison note priority, manual page 40: "Low selects low-note priority.
/// High selects high-note priority. Last selects last-note priority."
const KEY_MODES: [&str; 3] = ["low", "high", "last"];

/// The three fixed envelope routings, manual page 24. They are not free
/// assignments — "envelopes are not freely assignable, but instead toggle
/// through the fixed routing schemes".
const ENV_ROUTES: [&str; 3] = ["1flt 2amp", "1aux 2fa", "1aux 2fg"];

/// "When on, the Delay, Attack, and Decay segments of the selected envelopes
/// repeat indefinitely."
const ENV_REPEATS: [&str; 4] = ["off", "env 1", "env 2", "both"];

/// Effect 1's thirteen positions: Off, then the twelve algorithms in the
/// manual's own order.
///
/// **The origin is fixed by two independent names.** *Ring Laboratory* stores
/// 10 and *Lofi Pad* stores 12; a zero-based list of the twelve without Off
/// would leave 12 undefined and would make *Ring Laboratory* a rotating
/// speaker. So Off is 0 and the twelve follow.
const FX_TYPES: [&str; 13] = [
    "off", "delay", "bbd", "tape 1", "tape 2", "chorus", "flanger", "phaser", "hp filter",
    "distort", "ring mod", "rotary", "lo-fi",
];

/// "Noise: Toggles the white/pink noise generator." The NRPN table prints
/// this parameter as 0–127 and CC 108 prints it as 0–1; the data is 0 or 1
/// and 30 of the 256 programs choose the second.
const NOISE_TYPES: [&str; 2] = ["white", "pink"];

/// How many voices unison stacks. **The legend is not published anywhere**;
/// this is the decode's inference from the chord memories, and the two
/// anchors are *Quintuple Mono*, which stores a five-note chord with the
/// value 4, and *Bouncy Min9*, which stores a five-note chord with the
/// value 5. Both are only playable if 4 means five voices, which makes 5 the
/// "all" position the manual's photograph shows.
const UNISON_VOICES: [&str; 6] = ["1 voice", "2 voice", "3 voice", "4 voice", "5 voice", "all"];

/// Octave transpose, raw 0–4 with 2 as concert pitch. Verified against the
/// bank: the bass and percussion categories cluster at 1 and everything else
/// at 2.
const TRANSPOSE_LABELS: [&str; 5] = ["-2 oct", "-1 oct", "0", "+1 oct", "+2 oct"];

/// Modulation sources, the manual's Appendix A, indices 0–19. "19 different
/// modulation sources" in the prose is these twenty minus *Off*.
const MOD_SOURCES: [&str; 20] = [
    "off", "osc 2", "noise", "lfo 1", "lfo 2", "env 1", "env 2", "vc spread", "bend", "mod whl",
    "pressure", "breath", "foot", "express", "velocity", "note num", "filter out", "random",
    "dc", "audio out",
];

/// Modulation destinations, the manual's Appendix B, indices 0–64 — "65
/// different destinations".
///
/// The same list serves the three direct routings (`lfo 1 dest`, `lfo 2 dest`
/// and `e1 dest`) as well as the sixteen matrix slots. The NRPN table prints
/// the direct ones as 0–61 and the matrix ones as 0–64 while the manual
/// publishes exactly one list of 65 and points the LFO destination control at
/// it; no factory program exceeds 54 on a direct routing, so which three
/// entries the shorter list is missing — if any — does not arise here.
const MOD_DESTS: [&str; 65] = [
    "no dest",
    "osc 1 freq", "osc 2 freq", "osc frq", "osc 1 fine", "osc 2 fine", "osc fine",
    "osc 1 width", "osc 2 width", "osc width",
    "osc 1 level", "osc 2 level", "sub level", "noise", "x-mod",
    "cutoff", "resonance", "state",
    "fx mix", "fx time", "fx misc",
    "verb mix", "verb size", "verb pre", "verb decay", "verb tone",
    "lfo 1 rate", "lfo 2 rate", "lfo rate", "lfo 1 amt", "lfo 2 amt", "lfo amt",
    "env 1 amt", "env 2 amt", "e1 delay", "e2 delay", "e1 attack", "e2 attack", "e1 decay",
    "e2 decay", "e1 sustain", "e2 sustain", "e1 release", "e2 release",
    "volume", "panning", "overdrive", "vintage", "uni detune",
    "mod 1 amt", "mod 2 amt", "mod 3 amt", "mod 4 amt", "mod 5 amt", "mod 6 amt", "mod 7 amt",
    "mod 8 amt", "mod 9 amt", "mod 10 amt", "mod 11 amt", "mod 12 amt", "mod 13 amt",
    "mod 14 amt", "mod 15 amt", "mod 16 amt",
];

/// The eleven clock divisions the synced effects use, as labels.
///
/// **Not published in any Oberheim document**, so the ordering is judgment
/// and the argument is the bank's own: `fx_sync_rate` is 3 in 188 of the 256
/// programs, which makes 3 the value the instrument writes when nobody has
/// touched the control, and index 4 is the next most used with 33. Ordering
/// the eleven from short to long puts an eighth note at 3 and a dotted eighth
/// at 4 — which is what a delay defaults to and what the next-most-common
/// setting on a delay is — and leaves the quarter, the dotted quarter and the
/// half at 6, 7 and 9, where the remaining 31 uses sit. Any other ordering
/// makes the bank's default an odd division.
const FX_DIVISIONS: [&str; 11] =
    ["1/16", "1/16 d", "1/8 T", "1/8", "1/8 d", "1/4 T", "1/4", "1/4 d", "1/2 T", "1/2", "1/1"];

/// The sixteen clock divisions the synced LFOs use.
///
/// **Also unpublished**, and ordered the other way round for the same kind of
/// reason: `lfo_freq_sync` is 0 in more clock-synced programs than any other
/// value, and a synced LFO that nobody has adjusted should be a slow sweep
/// rather than a buzz. So index 0 is four bars and the list runs down to a
/// thirty-second, which puts the bank's other clusters — 10, 11 and 12 — on
/// the eighth, the eighth triplet and the dotted sixteenth, which is where a
/// rhythmic LFO lives.
const LFO_DIVISIONS: [&str; 16] = [
    "4/1", "2/1", "1/1", "1/2 d", "1/2", "1/2 T", "1/4 d", "1/4", "1/4 T", "1/8 d", "1/8",
    "1/8 T", "1/16 d", "1/16", "1/16 T", "1/32",
];

const OFF_ON: [&str; 2] = ["off", "on"];

/// `"0"` through `"24"`, so that the two bend-range switches read as
/// semitones. "The upward range is 12 semitones (1 octave). The downward
/// range is 24 semitones (2 octaves)."
const BEND_LABELS: [&str; 25] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
    "17", "18", "19", "20", "21", "22", "23", "24",
];

/// Unison detune, raw 0–7: "A setting of 0 is minimum detuning. A setting of
/// 7 is maximum detuning."
const UNISON_DETUNE_LABELS: [&str; 8] = ["0", "1", "2", "3", "4", "5", "6", "7"];

/// Which modulation slot a parameter index belongs to, and which of its three
/// controls it is.
const fn mod_slot(index: usize) -> Option<(usize, usize)> {
    if index >= P_MOD && index < P_MOD + 3 * MOD_SLOTS {
        let offset = index - P_MOD;
        Some((offset / 3, offset % 3))
    } else {
        None
    }
}

/// How many positions a selector has, or `None` for a knob.
fn discrete_steps(index: usize) -> Option<usize> {
    if let Some((_, which)) = mod_slot(index) {
        return match which {
            0 => Some(MOD_SOURCES.len()),
            1 => None,
            _ => Some(MOD_DESTS.len()),
        };
    }
    match index {
        P_PROGRAM => Some(PROGRAMS_PER_BANK),
        P_BANK => Some(BANK_COUNT),
        P_O1_TRI | P_O1_SAW | P_O1_PULSE | P_O1_KEY | P_O1_ON | P_O2_TRI | P_O2_SAW
        | P_O2_PULSE | P_O2_KEY | P_O2_ON | P_O2_BYPASS | P_SYNC | P_SUB_ON | P_NOISE_ON
        | P_BANDPASS | P_E1_VEL | P_E2_VEL | P_L1_SYNC | P_L2_SYNC | P_L1_RESET | P_L2_RESET
        | P_FX_ON | P_FX_SYNC | P_RV_ON | P_UNISON | P_RETRIGGER | P_GLIDE | P_SPLIT_1
        | P_SPLIT_2 => Some(2),
        P_NOISE_TYPE => Some(NOISE_TYPES.len()),
        P_GLIDE_MODE => Some(GLIDE_MODES.len()),
        P_KEY_MODE => Some(KEY_MODES.len()),
        P_ENV_ROUTE => Some(ENV_ROUTES.len()),
        P_ENV_REPEAT => Some(ENV_REPEATS.len()),
        P_L1_SHAPE | P_L2_SHAPE => Some(LFO_SHAPES.len()),
        P_L1_DIV | P_L2_DIV => Some(LFO_DIVISIONS.len()),
        P_L1_DEST | P_L2_DEST | P_E1_DEST => Some(MOD_DESTS.len()),
        P_FX_TYPE => Some(FX_TYPES.len()),
        P_FX_DIV => Some(FX_DIVISIONS.len()),
        P_UNISON_VOICES => Some(UNISON_VOICES.len()),
        P_UNISON_DETUNE => Some(UNISON_DETUNE_LABELS.len()),
        P_TRANSPOSE => Some(TRANSPOSE_LABELS.len()),
        P_BEND_UP => Some(13),
        P_BEND_DOWN => Some(25),
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
/// reason the other banks give: adding 1/n of the range n times does not
/// arrive at 1.0, and a step boundary missed by one ulp is a keypress that
/// visibly does nothing. With sixty-five destinations on a selector that
/// error would accumulate over a walk down the list.
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
/// Takes the whole parameter block rather than one value, as the DX7's and
/// the Prophet-6's do and for the same reason: the two program selectors are
/// one control between them, and the name on the program knob depends on
/// which bank the bank knob is pointing at.
#[must_use]
pub fn discrete_label(params: &[f32], index: usize) -> Option<&'static str> {
    let value = params.get(index).copied().unwrap_or(0.0);
    let count = discrete_steps(index)?;
    let step = selector(value, count);
    if let Some((_, which)) = mod_slot(index) {
        return Some(if which == 0 { MOD_SOURCES[step] } else { MOD_DESTS[step] });
    }
    Some(match index {
        P_PROGRAM => program_label(program_index(
            params.get(P_BANK).copied().unwrap_or(0.0),
            value,
        )),
        // The bank knob reads as the bank *and* the category the instrument
        // files the selected program under — "4 lead", "G tuned perc". The
        // twelve columns the editor leaves fit both, the parameter's own name
        // already says "bank", and a player stepping through sixteen banks of
        // sixteen wants to know what kind of sound is under the cursor.
        P_BANK => {
            let _ = step;
            program_bank_label(program_index(value, params.get(P_PROGRAM).copied().unwrap_or(0.0)))
        }
        P_NOISE_TYPE => NOISE_TYPES[step],
        P_GLIDE_MODE => GLIDE_MODES[step],
        P_KEY_MODE => KEY_MODES[step],
        P_ENV_ROUTE => ENV_ROUTES[step],
        P_ENV_REPEAT => ENV_REPEATS[step],
        P_L1_SHAPE | P_L2_SHAPE => LFO_SHAPES[step],
        P_L1_DIV | P_L2_DIV => LFO_DIVISIONS[step],
        P_L1_DEST | P_L2_DEST | P_E1_DEST => MOD_DESTS[step],
        P_FX_TYPE => FX_TYPES[step],
        P_FX_DIV => FX_DIVISIONS[step],
        P_UNISON_VOICES => UNISON_VOICES[step],
        P_UNISON_DETUNE => UNISON_DETUNE_LABELS[step],
        P_TRANSPOSE => TRANSPOSE_LABELS[step],
        P_BEND_UP | P_BEND_DOWN => BEND_LABELS[step],
        _ => OFF_ON[step],
    })
}

/// A knob's value in seconds, for the eleven that measure time.
#[must_use]
pub fn param_seconds(index: usize, value: f32) -> Option<f64> {
    match index {
        P_E1_ATTACK | P_E1_DECAY | P_E1_RELEASE | P_E2_ATTACK | P_E2_DECAY | P_E2_RELEASE => {
            Some(raw::env_seconds(f64::from(value) * 255.0))
        }
        P_E1_DELAY | P_E2_DELAY => Some(raw::env_seconds(f64::from(value) * 255.0)),
        P_O1_GLIDE | P_O2_GLIDE => Some(raw::glide_seconds(f64::from(value) * 127.0)),
        _ => None,
    }
}

// ── Raw values into physical units ──

/// Every conversion from a factory program's raw byte into the units the
/// engine works in.
///
/// The instrument stores 0–1, 0–7, 0–63, 0–127, 0–254, 0–255, 0–511 and
/// 0–1023, and publishes some of what those mean and not the rest. Each
/// function below says which it is: the manual's own words, or judgment with
/// the reasoning written out and the bank's own numbers behind it. The panel
/// knob is the raw value divided by its maximum, so every one of these takes
/// the raw number back.
pub mod raw {
    use super::{PI, TAU};

    /// Oscillator base frequency, raw 0–63, in semitones above concert pitch.
    ///
    /// "Sets the base frequency of an oscillator over a 5-octave + minor
    /// third range." Five octaves and a minor third is 63 semitones, which is
    /// exactly the parameter's range, so one count is one semitone. Zero is
    /// concert pitch and the knob only goes up — which is why the bank's
    /// basses reach *down* with the transpose switch instead, and why the
    /// tuned-percussion category is the only one whose median `osc1_freq` is
    /// not zero.
    #[must_use]
    pub fn osc_semitones(v: f64) -> f64 {
        v.clamp(0.0, 63.0)
    }

    /// Oscillator fine tune, raw 0–63, in cents.
    ///
    /// "Fine tune control with a range of +/- 49.2 cents up or down. The 12
    /// o'clock position is detented." Sixty-four positions over 98.4 cents,
    /// with the detent at 31 and 32 — which is where 239 of the 256 programs
    /// leave oscillator 1 and 134 of them leave oscillator 2.
    #[must_use]
    pub fn fine_cents(v: f64) -> f64 {
        (v.clamp(0.0, 63.0) - 31.5) / 31.5 * 49.2
    }

    /// Pulse width, raw 0–127, as a duty cycle.
    ///
    /// "Sets pulse width of the Oscillator 1 pulse wave from %50-%100 duty
    /// cycle, with %50 at minimum pot setting. At %100 the pulse width
    /// narrows to silence." So the bottom of the knob is a square and the top
    /// is a pulse that is high for the whole cycle — which, once its own mean
    /// is taken out, is nothing at all. The oscillator does exactly that
    /// rather than normalising the shape back up: an analog pulse wave keeps
    /// its rails as its duty closes, and its energy goes to zero because the
    /// spike gets shorter, not because it gets smaller.
    #[must_use]
    pub fn duty(v: f64) -> f64 {
        0.5 + 0.5 * (v / 127.0).clamp(0.0, 1.0)
    }

    /// A mixer level or any other raw 0–127 fader, as an amplitude.
    #[must_use]
    pub fn level(v: f64) -> f64 {
        (v / 127.0).clamp(0.0, 1.0)
    }

    /// A raw 0–1 switch. Written as a threshold rather than an equality
    /// because one factory program stores 14 in a 0–1 field — see
    /// [`super::Panel::read`].
    #[must_use]
    pub fn on(v: f64) -> bool {
        v >= 0.5
    }

    /// A bipolar control, raw 0–`max` with `max/2` as zero, into −1…+1.
    #[must_use]
    pub fn bipolar(v: f64, max: f64) -> f64 {
        ((v - max * 0.5) / (max * 0.5)).clamp(-1.0, 1.0)
    }

    /// Filter cutoff, raw 0–1023, as a MIDI note number.
    ///
    /// **Judgment, and here is the argument.** The manual gives the
    /// resolution and both ends — "filter cutoff frequency within its full
    /// range of 0-1024 (fully closed to fully open)", and "if you turn the
    /// cutoff knob fully counterclockwise you'll filter out all frequencies
    /// and hear nothing" — and no hertz anywhere. What it does give is a
    /// keyboard-tracking control calibrated in the keyboard's own units, so
    /// the cutoff's control voltage is a pitch and reading the parameter as a
    /// pitch is the reading that makes the knob and the tracking share a
    /// unit.
    ///
    /// **Eight counts to the semitone** is what makes the two ends land where
    /// the manual puts them: the travel is then 128 semitones, the bottom is
    /// [`CUTOFF_LOW_NOTE`] at 10 Hz — under anything a speaker reproduces, so
    /// "hear nothing" is true — and the top is 21 kHz, past the audible band,
    /// so the last stretch of the knob is genuinely open.
    ///
    /// It also puts the bank where a bank should be. The 256 programs' median
    /// cutoff is 381, which is 203 Hz, and their median envelope amount adds
    /// 50 semitones on top of it: a filter that opens to 3.7 kHz under the
    /// envelope, which is what an analog polysynth sounds like. The
    /// categories separate the way the names say too — the plucks close the
    /// filter to 49 Hz and let the envelope do the work, and the organs leave
    /// it at 1.4 kHz.
    #[must_use]
    pub fn cutoff_note(v: f64) -> f64 {
        CUTOFF_LOW_NOTE + v.clamp(0.0, 1023.0) / 8.0
    }

    /// Where the cutoff knob sits with nothing at all through it.
    pub const CUTOFF_LOW_NOTE: f64 = 8.0;

    /// How far a full modulation amount moves the cutoff, in semitones — the
    /// whole travel of the cutoff control, so that a modulation at full can
    /// take the filter from shut to open.
    pub const CUTOFF_SEMITONES: f64 = 128.0;

    /// A note number as a frequency in hertz.
    #[must_use]
    pub fn note_hz(note: f64) -> f64 {
        440.0 * ((note - 69.0) / 12.0).exp2()
    }

    /// Filter resonance, raw 0–255, as a fraction of the travel.
    ///
    /// The NRPN table prints the range as "0–265", which is a typo, and the
    /// manual's own prose prints it as 0–254; the data reaches 255.
    #[must_use]
    pub fn resonance(v: f64) -> f64 {
        (v / 255.0).clamp(0.0, 1.0)
    }

    /// The state control, raw 0–511, as a position on the morph: 0 is low
    /// pass, 0.5 is the notch (or the band pass), 1 is high pass.
    #[must_use]
    pub fn state(v: f64) -> f64 {
        (v / 511.0).clamp(0.0, 1.0)
    }

    /// Envelope segment time, raw 0–255, in seconds.
    ///
    /// **Judgment.** The manual describes all five segments and times none of
    /// them. Ten seconds at the top is the range the rest of this class of
    /// instrument uses and the range the rack's other envelopes already have,
    /// and the exponent is what the *bank* settled: at 3.5 the median
    /// amplitude decay of 159 is 1.9 s and the median release of 160 is
    /// 2.0 s, which is a polysynth; *Glacial*'s stored attack of 186 is 3.3 s
    /// and its release of 214 is 5.4 s, which is its name; and the bank's
    /// 25th-percentile attack of 0 is an instant one. A cubic law stretches
    /// the median decay to 2.4 s and the median release to 2.5 s, which is a
    /// pad bank rather than a general one.
    #[must_use]
    pub fn env_seconds(v: f64) -> f64 {
        let t = (v / 255.0).clamp(0.0, 1.0);
        (ENV_MAX_S * t * t * t * t.sqrt()).max(ENV_MIN_S)
    }

    /// The shortest segment the envelope will produce. Half a millisecond is
    /// under a sample at every rate this runs at, so the bottom of the knob
    /// is an instant transition rather than a slow one.
    pub const ENV_MIN_S: f64 = 0.0005;
    /// The longest segment, at the top of the knob.
    pub const ENV_MAX_S: f64 = 10.0;

    /// LFO frequency, raw 0–255, in hertz.
    ///
    /// The one rate control on the instrument whose range is published:
    /// "Sets the frequency of the selected LFO from a slow .022Hz to a fast
    /// 500Hz." Geometric between them, because that is what a rate control
    /// is — and the bank agrees, which is the check that the law is right
    /// rather than merely plausible: the median stored rate is 125 of 255,
    /// and a geometric law puts 125 at 3.0 Hz, a vibrato, within 10 % of the
    /// geometric centre of the published range.
    ///
    /// The field is eight bits and both the NRPN table and the CC map print
    /// it as 0–127. Reading it as seven would halve every LFO in the bank:
    /// 121 of the 256 programs store a value above 127.
    #[must_use]
    pub fn lfo_hz(v: f64) -> f64 {
        let t = (v / 255.0).clamp(0.0, 1.0);
        LFO_MIN_HZ * (LFO_MAX_HZ / LFO_MIN_HZ).powf(t)
    }

    pub const LFO_MIN_HZ: f64 = 0.022;
    pub const LFO_MAX_HZ: f64 = 500.0;

    /// Glide, raw 0–127, in seconds — an octave's worth in the two
    /// fixed-rate modes, the whole transition in the two fixed-time modes.
    ///
    /// **Judgment**: the manual describes the four modes, says "low values
    /// are shorter/faster", and times none of them. Geometric from a
    /// millisecond to ten seconds, which is the same span the envelopes have.
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

/// Banks, as the front panel numbers them: 1–9 then A–G.
pub const BANK_COUNT: usize = 16;
/// Programs in a bank. [`P_PROGRAM`] picks one of these; [`P_BANK`] picks the
/// bank, exactly as the instrument's 16 × 16 addressing does.
pub const PROGRAMS_PER_BANK: usize = 16;
/// Every factory program, across all sixteen banks.
pub const PROGRAM_COUNT: usize = BANK_COUNT * PROGRAMS_PER_BANK;

/// Bytes per program in [`ROM`]: everything in the program record except the
/// note grid.
const PACKED_PROGRAM: usize = 190;

/// Oberheim's factory program set, as the instrument stores it.
///
/// `TEO5_Factory_Programs_v1.00.syx`, published 8 July 2024, unpacked out of
/// the packed-MS-bit SysEx format and stripped to the part that is a program:
/// the 159 parameter bytes, the name, the category, the split and
/// arpeggiator settings and the sequence length. The 64-step, 5-track
/// sequencer and the 3,192 reserved bytes that follow it are dropped.
///
/// Kept as the machine's own bytes and decoded at startup rather than
/// transcribed into source, for the reason the DX7's ROM gives: 48 KB of data
/// plus a documented byte map is exact and testable, where 256 struct
/// literals is neither. `examples/teo5_rom.rs` is the whole provenance — it
/// rebuilds these bytes from the decode and, given the original SysEx file as
/// well, asserts that the two agree byte for byte across all 256 programs.
const ROM: &[u8; PROGRAM_COUNT * PACKED_PROGRAM] = include_bytes!("teo5_programs.bin");

/// Where each panel control's raw byte lives in a program block, and the
/// maximum that byte can hold.
///
/// For offsets 1–158 and 180–187 the offset **is** the NRPN parameter number
/// — see the module documentation for the five independent checks that
/// establish it — so this table is the MIDI Implementation Document's own
/// numbering rather than an inference.
///
/// Two controls are not here because they are two bytes each: the cutoff is
/// `34 + 256 × 35` over 0–1023 and the state is `38 + 256 × 39` over 0–511,
/// and [`Program::decode`] reads them itself.
///
/// The maxima are the *data's*, not always the document's. Eight of them are
/// eight-bit where both the NRPN table and the CC map print seven — the two
/// effect knobs, the four reverb knobs and the two LFO rates — and reading
/// any of those as 0–127 would silently halve it: `reverb_decay` alone
/// exceeds 127 in 171 of the 256 programs and `lfo1_freq` in 121.
fn raw_offset(index: usize) -> Option<(usize, f64)> {
    if let Some((slot, which)) = mod_slot(index) {
        return Some(match which {
            0 => (105 + slot, 19.0),
            1 => (121 + slot, 254.0),
            _ => (137 + slot, 64.0),
        });
    }
    Some(match index {
        P_O1_FREQ => (1, 63.0),
        P_O2_FREQ => (2, 63.0),
        P_O1_FINE => (3, 63.0),
        P_O2_FINE => (4, 63.0),
        P_O1_WIDTH => (5, 127.0),
        P_O2_WIDTH => (6, 127.0),
        P_O1_TRI => (7, 1.0),
        P_O2_TRI => (8, 1.0),
        P_O1_SAW => (9, 1.0),
        P_O2_SAW => (10, 1.0),
        P_O1_PULSE => (11, 1.0),
        P_O2_PULSE => (12, 1.0),
        P_O1_ON => (13, 1.0),
        P_O2_ON => (14, 1.0),
        P_O1_LEVEL => (15, 127.0),
        P_O2_LEVEL => (16, 127.0),
        P_SUB_ON => (17, 1.0),
        P_SUB_LEVEL => (18, 127.0),
        P_NOISE_ON => (19, 1.0),
        P_NOISE_TYPE => (20, 1.0),
        P_NOISE_LEVEL => (21, 127.0),
        P_O1_GLIDE => (22, 127.0),
        P_O2_GLIDE => (23, 127.0),
        P_O1_KEY => (24, 1.0),
        P_O2_KEY => (25, 1.0),
        P_XMOD => (26, 127.0),
        P_SYNC => (27, 1.0),
        P_O2_BYPASS => (28, 1.0),
        P_GLIDE_MODE => (30, 3.0),
        P_GLIDE => (31, 1.0),
        P_BEND_UP => (32, 12.0),
        P_BEND_DOWN => (33, 24.0),
        P_RESONANCE => (36, 255.0),
        P_BANDPASS => (37, 1.0),
        P_FILTER_KEY => (40, 127.0),
        P_FX_ON => (42, 1.0),
        P_FX_TYPE => (43, 12.0),
        P_FX_MIX => (44, 127.0),
        P_FX_TIME => (45, 255.0),
        P_FX_MISC => (46, 255.0),
        P_FX_SYNC => (47, 1.0),
        P_FX_DIV => (48, 10.0),
        P_RV_ON => (50, 1.0),
        P_RV_MIX => (52, 127.0),
        P_RV_SIZE => (53, 255.0),
        P_RV_PREDELAY => (54, 255.0),
        P_RV_DECAY => (55, 255.0),
        P_RV_TONE => (56, 255.0),
        P_L1_FREQ => (58, 255.0),
        P_L2_FREQ => (59, 255.0),
        P_L1_AMOUNT => (60, 254.0),
        P_L2_AMOUNT => (61, 254.0),
        P_L1_SHAPE => (62, 4.0),
        P_L2_SHAPE => (63, 4.0),
        P_L1_SYNC => (64, 1.0),
        P_L2_SYNC => (65, 1.0),
        P_L1_DEST => (66, 64.0),
        P_L2_DEST => (67, 64.0),
        P_L1_DIV => (68, 15.0),
        P_L2_DIV => (69, 15.0),
        P_L1_RESET => (70, 1.0),
        P_L2_RESET => (71, 1.0),
        P_L1_SLEW => (72, 127.0),
        P_L2_SLEW => (73, 127.0),
        P_E1_AMOUNT => (75, 254.0),
        P_E2_AMOUNT => (76, 127.0),
        P_E1_VEL => (77, 1.0),
        P_E2_VEL => (78, 1.0),
        P_E1_DELAY => (79, 127.0),
        P_E2_DELAY => (80, 127.0),
        P_E1_ATTACK => (81, 255.0),
        P_E2_ATTACK => (82, 255.0),
        P_E1_DECAY => (83, 255.0),
        P_E2_DECAY => (84, 255.0),
        P_E1_SUSTAIN => (85, 127.0),
        P_E2_SUSTAIN => (86, 127.0),
        P_E1_RELEASE => (87, 255.0),
        P_E2_RELEASE => (88, 255.0),
        P_ENV_ROUTE => (89, 2.0),
        P_E1_DEST => (90, 64.0),
        P_ENV_REPEAT => (91, 3.0),
        P_VOLUME => (93, 127.0),
        P_PAN => (94, 254.0),
        P_OVERDRIVE => (95, 127.0),
        P_VINTAGE => (96, 127.0),
        P_UNISON => (97, 1.0),
        P_UNISON_VOICES => (98, 5.0),
        P_UNISON_DETUNE => (99, 7.0),
        P_KEY_MODE => (153, 2.0),
        P_RETRIGGER => (154, 1.0),
        P_TRANSPOSE => (156, 4.0),
        P_BPM => (157, 250.0),
        P_SPLIT_1 => (180, 1.0),
        P_SPLIT_2 => (181, 1.0),
        P_SPLIT_NOTE => (182, 43.0),
        _ => return None,
    })
}

/// The two-byte controls: `(low byte, high byte, maximum)`.
const CUTOFF_BYTES: (usize, usize, f64) = (34, 35, 1023.0);
const STATE_BYTES: (usize, usize, f64) = (38, 39, 511.0);

/// The empty slot in a stored chord. Not in the manual, which prints the
/// field as 0–43; 236 of the 256 programs store this in all five slots.
const CHORD_EMPTY: u8 = 127;

/// One factory program, decoded once and shared by every instance.
#[derive(Clone, Copy)]
struct Program {
    /// The panel, as knob positions. The program and bank selectors are
    /// filled in by the caller, since they are where the program came from.
    panel: [f32; PARAM_COUNT],
    /// The chord memory, as semitone offsets from the played root, empty
    /// slots removed. Not a panel control on either the hardware or here: it
    /// is captured by a keyboard gesture, and it arrives with the program.
    chord: [u8; MAX_CHORD],
    chord_len: u8,
    /// The category byte, 1–15. Not in any NRPN or CC table; the categorized
    /// bank is this file sorted on it.
    category: u8,
    /// The bank digit and the category, as the bank knob reads them.
    bank_label: [u8; 12],
    bank_label_len: u8,
    /// The 20 characters the instrument stores, trimmed of trailing spaces.
    name: [u8; 20],
    name_len: u8,
    /// How much of the name fits the editor's twelve columns.
    label_len: u8,
}

/// The program categories, offset 179. There is no category 0.
pub const CATEGORIES: [&str; 16] = [
    "-", "pad", "lead", "bass", "poly", "keys", "string", "pluck", "bell", "arp", "brass",
    "voice", "organ", "perc", "tuned perc", "sfx",
];

impl Program {
    fn decode(block: &[u8]) -> Self {
        let mut panel = [0.0f32; PARAM_COUNT];
        for (index, slot) in panel.iter_mut().enumerate() {
            let raw = match index {
                P_CUTOFF => {
                    let (lo, hi, max) = CUTOFF_BYTES;
                    Some((f64::from(block[lo]) + 256.0 * f64::from(block[hi]), max))
                }
                P_STATE => {
                    let (lo, hi, max) = STATE_BYTES;
                    Some((f64::from(block[lo]) + 256.0 * f64::from(block[hi]), max))
                }
                _ => raw_offset(index).map(|(offset, max)| (f64::from(block[offset]), max)),
            };
            *slot = match raw {
                // A selector reads back as the middle of its step, so that
                // stepping away from a loaded program and back arrives at the
                // same byte. A knob reads back as its share of its range.
                Some((value, max)) => {
                    let value = value.min(max);
                    match discrete_steps(index) {
                        Some(count) => knob_for((value as usize).min(count - 1), count),
                        None => (value / max) as f32,
                    }
                }
                None => 0.0,
            };
        }

        let mut chord = [0u8; MAX_CHORD];
        let mut chord_len = 0usize;
        for note in &block[100..105] {
            if *note != CHORD_EMPTY && chord_len < MAX_CHORD {
                chord[chord_len] = *note;
                chord_len += 1;
            }
        }

        let mut name = [b' '; 20];
        name.copy_from_slice(&block[159..179]);
        let name_len = name.iter().rposition(|c| *c != b' ').map_or(0, |i| i + 1);
        Self {
            panel,
            chord,
            chord_len: chord_len as u8,
            category: block[179].min(15),
            bank_label: [b' '; 12],
            bank_label_len: 0,
            name,
            name_len: name_len as u8,
            label_len: name_len.min(LABEL_WIDTH) as u8,
        }
    }

    /// Fill in the bank knob's reading, which needs to know where in the
    /// bank the program sits and is therefore not decodable from its own
    /// bytes.
    fn label_bank(&mut self, index: usize) {
        let tag = CATEGORIES[self.category as usize];
        self.bank_label[0] = BANK_DIGITS[index / PROGRAMS_PER_BANK];
        let mut end = 2;
        for c in tag.bytes() {
            if end < self.bank_label.len() {
                self.bank_label[end] = c;
                end += 1;
            }
        }
        self.bank_label_len = end as u8;
    }

    fn bank_label(&'static self) -> &'static str {
        std::str::from_utf8(&self.bank_label[..self.bank_label_len as usize]).unwrap_or("?")
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
/// parameter's own name, so a selector's label gets the other 12. Oberheim's
/// names run to 20 characters — *Put Everything in it*, *Grandmothers Choir* —
/// so the panel shows the first 12 and [`program_name`] keeps the whole thing
/// for anything with room to print it.
const LABEL_WIDTH: usize = 12;

/// The 256 factory programs, decoded once for the whole process.
fn programs() -> &'static [Program; PROGRAM_COUNT] {
    static DECODED: std::sync::OnceLock<Box<[Program; PROGRAM_COUNT]>> =
        std::sync::OnceLock::new();
    DECODED.get_or_init(|| {
        let mut bank = Box::new(
            [Program {
                panel: [0.0; PARAM_COUNT],
                chord: [0; MAX_CHORD],
                chord_len: 0,
                category: 0,
                bank_label: [b' '; 12],
                bank_label_len: 0,
                name: [b' '; 20],
                name_len: 0,
                label_len: 0,
            }; PROGRAM_COUNT],
        );
        for (index, (slot, block)) in
            bank.iter_mut().zip(ROM.chunks_exact(PACKED_PROGRAM)).enumerate()
        {
            *slot = Program::decode(block);
            slot.label_bank(index);
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

/// The absolute program number, 0–255, that the two knobs select together.
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

/// A factory program's name, all 20 characters of it, trailing spaces
/// removed.
#[must_use]
pub fn program_name(index: usize) -> &'static str {
    programs()[index.min(PROGRAM_COUNT - 1)].name()
}

/// As much of the name as the editor's column fits. See [`LABEL_WIDTH`].
#[must_use]
pub fn program_label(index: usize) -> &'static str {
    programs()[index.min(PROGRAM_COUNT - 1)].label()
}

/// A factory program's category, as the instrument files it.
#[must_use]
pub fn program_category(index: usize) -> &'static str {
    CATEGORIES[programs()[index.min(PROGRAM_COUNT - 1)].category as usize]
}

/// What the bank knob reads when it is pointing at this program: the bank's
/// own digit and the program's category.
#[must_use]
pub fn program_bank_label(index: usize) -> &'static str {
    programs()[index.min(PROGRAM_COUNT - 1)].bank_label()
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

/// The panel the instrument loads with: bank 1, program 1, *It's an
/// Oberheim*.
#[must_use]
pub fn param_defaults() -> [f32; PARAM_COUNT] {
    params_for_program(0.0, 0.0)
}

// ── The oscillator ──
//
// Three shapes, and unlike the Prophet-6's single morphing knob they are
// **independently mixable**: "These buttons toggle on and off the waveshapes
// generated by the oscillator... All waveshapes can be simultaneously
// selected." So one phase accumulator drives a triangle, a sawtooth and a
// pulse, and the three sum.
//
// Band limiting is the corner-pair scheme the rest of the rack uses, written
// out for these shapes: a step at a known phase gets a two-sample polyBLEP
// residual and a slope change gets a two-sample polyBLAMP residual, both
// spread over the sample before the event and the sample after it, which is
// why the oscillator holds one sample back. The landmarks are fixed — phase 0
// for the sawtooth's drop, the pulse's rise and the triangle's trough, phase
// 0.5 for the triangle's peak, and the duty for the pulse's fall — so the
// walk is three tests rather than a search.
//
// **Negative phase advance is a first-class case here**, which it is nowhere
// else in this rack: through-zero FM drives oscillator 1's instantaneous
// frequency below zero and the phase runs backwards. The accumulator wraps in
// both directions; the corrections are applied only while the phase is moving
// forwards, because a residual derived for a forward crossing is the wrong
// shape for a backward one and because a reversing oscillator is producing FM
// sidebands across the whole spectrum anyway — the missing sub-sample
// correction on those samples is far under what the modulation itself puts
// there. See [`X_MOD_INDEX`].

/// What one oscillator is producing: the three shape switches as gains, and
/// the pulse's duty cycle.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Waveset {
    tri: f64,
    saw: f64,
    pulse: f64,
    duty: f64,
}

impl Waveset {
    /// The waveform at a phase. The pulse has its own mean taken out, which
    /// is what makes the top of the pulse-width knob silence rather than a
    /// direct voltage: at a duty of 1 the shape is high for the whole cycle
    /// and its mean is 1, so the difference is zero everywhere.
    #[inline]
    fn value(&self, phase: f64) -> f64 {
        let mut v = 0.0;
        if self.tri != 0.0 {
            v += self.tri * (1.0 - 4.0 * (phase - 0.5).abs());
        }
        if self.saw != 0.0 {
            v += self.saw * (2.0 * phase - 1.0);
        }
        if self.pulse != 0.0 {
            let high = if phase < self.duty { 1.0 } else { -1.0 };
            v += self.pulse * (high - (2.0 * self.duty - 1.0));
        }
        v
    }

    /// Whether the pulse has edges at all. At a duty of 1 it does not: the
    /// shape is a constant, and firing a rise and a fall in the same sample
    /// would put a spike where there is silence.
    #[inline]
    fn pulse_edge(&self) -> f64 {
        if self.duty < 1.0 { self.pulse } else { 0.0 }
    }
}

/// The two samples a band-limiting correction is spread over.
#[derive(Debug, Clone, Copy, Default)]
struct Correction {
    before: f64,
    after: f64,
}

/// A step of `height` at `t` of the way through the current sample.
#[inline]
fn blep(height: f64, t: f64, fix: &mut Correction) {
    let back = 1.0 - t;
    fix.before += 0.5 * height * back * back;
    fix.after -= 0.5 * height * t * t;
}

/// A slope change of `m` output units per sample at `t` of the way through
/// the current sample.
#[inline]
fn blamp(m: f64, t: f64, fix: &mut Correction) {
    let back = 1.0 - t;
    fix.before += m * back * back * back / 6.0;
    fix.after += m * t * t * t / 6.0;
}

#[derive(Debug, Clone)]
struct Osc {
    phase: f64,
    /// The sample computed on the previous call, still open to corrections
    /// from events in this one — the one sample of latency a two-sided
    /// correction needs. Every oscillator in the voice is delayed
    /// identically, so sync timing and relative phase are unaffected.
    held: f64,
}

impl Osc {
    fn new(phase: f64) -> Self {
        Self { phase, held: 0.0 }
    }

    fn reset(&mut self, phase: f64) {
        self.phase = phase;
        self.held = 0.0;
    }

    /// One sample. `dt` is the phase advance, which may be negative;
    /// `sync_at` is the fraction of this step at which an external reset
    /// arrives.
    #[inline]
    fn tick(&mut self, dt: f64, w: &Waveset, sync_at: Option<f64>) -> f64 {
        let mut fix = Correction::default();
        let end;
        if let Some(u) = sync_at {
            let u = u.clamp(0.0, 1.0);
            let span = u * dt;
            self.walk(w, self.phase, span, dt, 0.0, u, true, &mut fix);
            let at = wrap_phase(self.phase + span);
            blep(w.value(0.0) - w.value(at), u, &mut fix);
            let rest = (1.0 - u) * dt;
            self.walk(w, 0.0, rest, dt, u, 1.0, false, &mut fix);
            end = wrap_phase(rest);
        } else {
            self.walk(w, self.phase, dt, dt, 0.0, 1.0, true, &mut fix);
            end = wrap_phase(self.phase + dt);
        }
        let out = self.held + fix.before;
        self.held = w.value(end) + fix.after;
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

    /// Every landmark crossed by a forward stretch of phase, corrected.
    ///
    /// `open` is whether a landmark sitting exactly at `from` counts. It does
    /// on an ordinary sample, where the previous sample stopped just short of
    /// it; it does not on the stretch after a sync reset, where the reset's
    /// own step correction has already accounted for the waveform arriving at
    /// phase zero.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    fn walk(
        &self,
        w: &Waveset,
        from: f64,
        span: f64,
        dt: f64,
        t0: f64,
        t1: f64,
        open: bool,
        fix: &mut Correction,
    ) {
        if span <= 0.0 {
            return;
        }
        let hit = |pos: f64| -> Option<f64> {
            let reached = (pos - from).rem_euclid(1.0);
            if reached < span && (open || reached > 0.0) {
                Some(t0 + (reached / span) * (t1 - t0))
            } else {
                None
            }
        };
        let edge = w.pulse_edge();
        // Phase zero: the sawtooth drops, the pulse rises, the triangle turns
        // round at its trough.
        let step0 = 2.0 * (edge - w.saw);
        let corner0 = 8.0 * dt * w.tri;
        if step0 != 0.0 || corner0 != 0.0 {
            if let Some(t) = hit(0.0) {
                blep(step0, t, fix);
                blamp(corner0, t, fix);
            }
        }
        // Half way: the triangle turns round at its peak.
        if corner0 != 0.0 {
            if let Some(t) = hit(0.5) {
                blamp(-corner0, t, fix);
            }
        }
        // The pulse falls at its duty.
        if edge != 0.0 {
            if let Some(t) = hit(w.duty) {
                blep(-2.0 * edge, t, fix);
            }
        }
    }
}

#[inline]
fn wrap_phase(p: f64) -> f64 {
    p - p.floor()
}

/// Rational tanh: one divide instead of a libm call, within 0.5 % of the real
/// thing over the range it is defined on.
///
/// The Padé form is only a tanh up to ±3, where it reaches exactly 1 **with
/// exactly zero slope** — and past that it climbs again, which for a
/// distortion driven forty times is not a saturator at all. Clamping the
/// input at 3 is therefore free: the curve is C1 across the join because the
/// derivative there is already zero, and the result is bounded by 1 for any
/// input at all, which is what everything downstream of it assumes.
#[inline]
fn tanh_approx(x: f64) -> f64 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

// ── The SEM filter ──
//
// "The function of the TEO-5's 12dB/2-pole state variable filter is to
// subtract frequencies from the sound produced by the oscillators and noise
// generator... Note that the TEO-5's filter does not self-oscillate."
//
// That is the whole character in two sentences, and it is what makes this
// filter different from every other one in the rack. The two ladders
// (`phatty.rs`, `synth.rs`) and the Prophet-6's SSM2040-lineage low-pass are
// all four-pole, all 24 dB an octave, and two of them self-oscillate. This
// one is two poles, twelve decibels, and its resonance stops well short of
// producing its own tone.
//
// What it has instead is the **state** control:
//
// > "State: Smoothly mixes between low pass, notch, and high pass filter
// > states. If the band pass state is set active in the Program menu, it
// > replaces notch at the center position of the state knob."
//
// A state-variable filter produces all three outputs from the same pair of
// integrators, and the SEM's mode control is a pot across the low-pass and
// high-pass outputs. That is not an approximation of the notch — it *is* the
// notch: at the corner frequency the low-pass output is `-jQ` and the
// high-pass output is `+jQ`, so half of each is exactly zero there and their
// sum is the classic `(s² + ω₀²)` numerator everywhere else. So the morph is
// one crossfade, the notch falls out of the middle of it for free, and the
// 6 dB the passband loses at the centre position is the crossfade's, which is
// what the hardware pot does too.
//
// With band pass switched on the middle of the travel becomes the band-pass
// output instead, so the morph is two crossfades — low pass to band pass, then
// band pass to high pass — with the same two endpoints.

/// The narrowest the resonance gets, and the widest.
///
/// A 2-pole filter lifts its corner by `Q`, so [`Q_MAX`] is +15.6 dB at the
/// corner and nothing anywhere else. The top of the travel stops there
/// because the manual says it does: this filter has no self-oscillation to
/// reach, and 46 of the 256 factory programs sit at the top of the resonance
/// knob expecting a formant rather than a tone.
const Q_MIN: f64 = 0.5;
const Q_MAX: f64 = 6.0;

/// Where the voice's output stage runs out of headroom, and where it stops.
///
/// Three waveshapes at once on each of two oscillators, plus a sub and noise,
/// is a mixer that can hand the filter ±8 before a resonant peak has been
/// applied to it, and the factory bank goes most of the way there: 117
/// *Processional* has both oscillators on all three shapes with the sub and
/// the noise underneath, which is seven units of mixer. The knee sits above
/// that, so an ordinary program passes through untouched; what it is really
/// there for is the product of a full mixer with a filter whose corner gain
/// is six.
const VOICE_KNEE: f64 = 6.0;
const VOICE_RAIL: f64 = 14.0;

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

/// A topology-preserving 2-pole state-variable filter: the SEM's two
/// integrators, and all three of their outputs.
#[derive(Debug, Clone)]
struct Svf {
    s1: f64,
    s2: f64,
}

impl Svf {
    fn new() -> Self {
        Self { s1: 0.0, s2: 0.0 }
    }

    fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    /// One sample, morphed. `state` is 0 for low pass, 0.5 for the notch (or
    /// the band pass) and 1 for high pass.
    #[inline]
    fn process(
        &mut self,
        input: f64,
        cutoff: f64,
        q: f64,
        state: f64,
        bandpass: bool,
        sr: f64,
    ) -> f64 {
        let g = raw::tpt_g(cutoff, sr);
        let damp = 1.0 / q;
        let hp = (input - (damp + g) * self.s1 - self.s2) / (1.0 + damp * g + g * g);
        let v1 = g * hp;
        let bp = v1 + self.s1;
        // The two integrator states are the only memory in the filter and the
        // only thing that can carry a transient forward, so they are the one
        // place worth bounding outright. Well clear of anything a program
        // produces: this is a numerical backstop, not a sound.
        self.s1 = (bp + v1).clamp(-64.0, 64.0);
        let v2 = g * bp;
        let lp = v2 + self.s2;
        self.s2 = (lp + v2).clamp(-64.0, 64.0);
        if bandpass {
            if state < 0.5 {
                let a = state * 2.0;
                lp * (1.0 - a) + bp * a
            } else {
                let a = (state - 0.5) * 2.0;
                bp * (1.0 - a) + hp * a
            }
        } else {
            lp * (1.0 - state) + hp * state
        }
    }
}

/// The resonance knob as a `Q`.
#[inline]
fn resonance_q(amount: f64) -> f64 {
    Q_MIN * (Q_MAX / Q_MIN).powf(amount.clamp(0.0, 1.0))
}

// ── Envelopes ──
//
// "The TEO-5 has two 5-stage DADSR envelope generators (delay, attack, decay,
// sustain, release), using the voltage curves from the historic Oberheim
// OB-8."
//
// The OB-8's envelopes are CEM3310 generators, and what that chip does is the
// whole character. Its attack charges the timing capacitor toward a rail well
// above the peak-detect threshold and *stops when the threshold is reached*,
// so the attack traverses only the first, straightest part of an exponential
// — an envelope that arrives rather than one that creeps up on its target.
// Its decay and release are ordinary exponential discharges. Two constants
// carry that:
//
// * [`ATTACK_AIM`] is 2, so the attack terminates after 0.69 of a time
//   constant and is nearly a straight line. The Prophet-6's digital envelopes
//   aim at 1.58 and spend a full time constant, which visibly rounds off the
//   top of the attack; side by side on the same attack time this one is the
//   punchier of the two, which is what the OB-8 is known for.
// * [`ENV_CONSTANTS`] is 4 rather than 3.5, so a decay of a given length
//   starts 14 % steeper and spends longer near the sustain level. A short
//   percussive decay gets out of the way faster; a long one still arrives.
//
// The envelopes are otherwise digital: nothing about them varies from note to
// note except what the vintage knob adds — see [`VINTAGE_ENV_SPREAD`].

/// The attack's target, above the 1.0 at which it terminates. See the section
/// comment: this is what makes an OB-8 attack straight.
const ATTACK_AIM: f64 = 2.0;
/// Time constants spanned by an attack, given [`ATTACK_AIM`]: `ln 2`, which
/// is where a charge toward 2 crosses 1.
const ATTACK_CONSTANTS: f64 = std::f64::consts::LN_2;
/// Time constants spanned by a decay or release.
const ENV_CONSTANTS: f64 = 4.0;
/// How far past its target a decay or release aims so that it arrives after
/// `ENV_CONSTANTS` of them: `e^-4 / (1 - e^-4)`.
const ENV_UNDERSHOOT: f64 = 0.018_657_360_929_167;

fn env_rate(seconds: f64, constants: f64, sr: f64) -> f64 {
    if seconds <= 0.0 {
        return 1.0;
    }
    (1.0 - (-constants / (seconds * sr)).exp()).min(1.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvStage {
    Idle,
    Delay,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct EnvTimes {
    delay: f64,
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
    /// Samples left in the delay segment.
    delay_left: f64,
    times: EnvTimes,
    /// Per-sample coefficients for the three timed segments, recomputed only
    /// when a knob moves.
    rates: [f64; 3],
    /// "the Delay, Attack, and Decay segments of the selected envelopes
    /// repeat indefinitely" while the key is held.
    repeat: bool,
    sample_rate: f64,
}

impl Envelope {
    fn new(sr: f64) -> Self {
        let mut env = Self {
            stage: EnvStage::Idle,
            level: 0.0,
            aim: 0.0,
            delay_left: 0.0,
            times: EnvTimes { delay: 0.0, attack: 0.01, decay: 0.3, sustain: 0.7, release: 0.2 },
            rates: [0.0; 3],
            repeat: false,
            sample_rate: sr,
        };
        env.retime();
        env
    }

    fn retime(&mut self) {
        let sr = self.sample_rate;
        self.rates = [
            env_rate(self.times.attack, ATTACK_CONSTANTS, sr),
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
            self.times.delay = times.delay;
        }
    }

    /// A new envelope from wherever it already is. The delay segment comes
    /// first: "Sets a delay between the time the envelope is triggered (note
    /// on) and when the attack portion begins."
    fn trigger(&mut self, from_zero: bool) {
        if from_zero {
            self.level = 0.0;
        }
        if self.times.delay > raw::ENV_MIN_S * 2.0 {
            self.stage = EnvStage::Delay;
            self.delay_left = self.times.delay * self.sample_rate;
        } else {
            self.enter_attack();
        }
    }

    fn enter_attack(&mut self) {
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
        self.delay_left = 0.0;
    }

    fn is_active(&self) -> bool {
        self.stage != EnvStage::Idle
    }

    fn enter_decay(&mut self) {
        self.stage = EnvStage::Decay;
        self.aim = self.times.sustain - ENV_UNDERSHOOT * (self.level - self.times.sustain);
    }

    fn enter_sustain(&mut self) {
        if self.repeat {
            self.trigger(true);
            return;
        }
        self.level = self.times.sustain;
        self.stage = if self.times.sustain <= 0.0 { EnvStage::Idle } else { EnvStage::Sustain };
    }

    #[inline]
    fn tick(&mut self) -> f64 {
        match self.stage {
            EnvStage::Idle => 0.0,
            EnvStage::Delay => {
                self.delay_left -= 1.0;
                if self.delay_left <= 0.0 {
                    self.enter_attack();
                }
                self.level
            }
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

// ── Noise, randomness and the LFOs ──

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

/// White noise. One per voice, with its own stream: nothing in this file
/// shares randomness, because six coherent copies of the same sequence sum to
/// 18 dB rather than 8 and that is audible as a hard bright edge on every
/// noisy program.
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

/// The corners and weights of the pink-noise filter bank, in hertz.
///
/// Paul Kellet's three-pole economy approximation, with its poles written as
/// frequencies rather than as the 44.1 kHz coefficients they are usually
/// quoted at, so that the noise has the same spectrum at every sample rate
/// rather than only at the one it was published for. The direct term is what
/// carries the top octave.
const PINK_HZ: [f64; 3] = [16.5, 264.6, 3_944.0];
const PINK_GAIN: [f64; 3] = [0.099_046, 0.296_516_4, 1.052_691_3];
const PINK_DIRECT: f64 = 0.1848;
/// What the sum of the four terms has to be multiplied by to sit at the same
/// level as the white noise it is made from.
const PINK_TRIM: f64 = 0.115;

/// Pink noise: "Toggles the white/pink noise generator."
#[derive(Debug, Clone, Default)]
struct Pink {
    b: [f64; 3],
    /// `(pole, gain)` per section, from the sample rate.
    coefficients: [(f64, f64); 3],
}

impl Pink {
    fn init(&mut self, sr: f64) {
        for i in 0..3 {
            let a = raw::one_pole(PINK_HZ[i], sr);
            // The published gains give each section a fixed DC gain at
            // 44.1 kHz; scaling by the section's own `a` keeps that gain, and
            // therefore the tilt, wherever the corner lands.
            let reference = raw::one_pole(PINK_HZ[i], 44_100.0);
            self.coefficients[i] = (1.0 - a, PINK_GAIN[i] * a / reference);
        }
        self.b = [0.0; 3];
    }

    fn reset(&mut self) {
        self.b = [0.0; 3];
    }

    #[inline]
    fn tick(&mut self, white: f64) -> f64 {
        let mut sum = white * PINK_DIRECT;
        for (state, (pole, gain)) in self.b.iter_mut().zip(&self.coefficients) {
            *state = *state * *pole + white * *gain;
            sum += *state;
        }
        sum * PINK_TRIM
    }
}

/// The band-limited step correction for the LFO's own edges, `(height/2)·r(s)`
/// with `r` the two-point residual. The LFO reaches 500 Hz on this
/// instrument, where a naive edge folds a spectrum back that would then be
/// heard through whatever it modulates, differently at every sample rate.
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

/// One low-frequency oscillator. There are two per voice's worth of state —
/// LFO 1 is one per instrument and LFO 2 is one per voice, which is the
/// difference the manual spends a page on.
///
/// The polarity is the manual's rather than the obvious one: "The triangle
/// wave is bipolar... The square, sawtooth, reverse sawtooth, and sample &
/// hold waves generate only positive values. In the case of the square wave,
/// this makes it possible to generate trills." A trill goes up from the note
/// and comes back, which a bipolar square could not do.
#[derive(Debug, Clone)]
struct Lfo {
    phase: f64,
    sample_hold: f64,
    slewed: f64,
    /// This LFO's own random stream, and how many cycles it has completed.
    ///
    /// The sample-and-hold shape draws from the *cycle count* rather than
    /// from a generator ticked once a sample, and that is the difference
    /// between an instrument that sounds the same on every audio device and
    /// one that does not: a per-sample stream visits a different value at
    /// each wrap depending on how many samples went by, so the same program
    /// picks a different sequence of pitches at 44.1 kHz and at 48. Keying
    /// the draw to the cycle makes the sequence a property of the program.
    stream: u32,
    cycles: u32,
}

impl Lfo {
    fn new(seed: u32) -> Self {
        Self { phase: 0.0, sample_hold: 0.0, slewed: 0.0, stream: mix32(seed), cycles: 0 }
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.sample_hold = 0.0;
        self.slewed = 0.0;
        self.cycles = 0;
    }

    /// Restart the phase, for the note-reset switch.
    fn restart(&mut self) {
        self.phase = 0.0;
        self.cycles = 0;
        self.sample_hold = self.draw();
        self.slewed = 0.0;
    }

    #[inline]
    fn draw(&mut self) -> f64 {
        self.cycles = self.cycles.wrapping_add(1);
        let n = mix32(self.stream ^ self.cycles.wrapping_mul(0x9E37_79B9));
        f64::from(n) / f64::from(u32::MAX) * 2.0 - 1.0
    }

    /// One sample. `slew` is the one-pole coefficient the slew knob asks for,
    /// or 1.0 for none.
    #[inline]
    fn tick(&mut self, hz: f64, sr: f64, shape: usize, slew: f64) -> f64 {
        let dt = (hz / sr).clamp(0.0, 0.45);
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            self.sample_hold = self.draw();
        }
        let t = self.phase;
        let raw = match shape {
            1 => (t + 0.5 * poly_blep(t, dt)).clamp(0.0, 1.0),
            2 => (1.0 - t - 0.5 * poly_blep(t, dt)).clamp(0.0, 1.0),
            3 => {
                let mut v = if t < 0.5 { 1.0 } else { 0.0 };
                v += 0.5 * poly_blep(t, dt);
                v -= 0.5 * poly_blep((t - 0.5).rem_euclid(1.0), dt);
                v.clamp(0.0, 1.0)
            }
            4 => self.sample_hold * 0.5 + 0.5,
            // Triangle, naive and bipolar: its harmonics fall as 1/n², so
            // what folds back sits far enough under the fundamental to be
            // irrelevant.
            _ => {
                if t < 0.5 {
                    4.0 * t - 1.0
                } else {
                    3.0 - 4.0 * t
                }
            }
        };
        if slew >= 1.0 {
            self.slewed = raw;
        } else {
            self.slewed += slew * (raw - self.slewed);
        }
        self.slewed
    }
}

/// How much smoothing the slew knob asks for, as a one-pole corner relative
/// to the LFO's own rate.
///
/// "Adding slew to an LFO smooths out LFO waveshapes by altering the speed at
/// which voltage levels change. Adding slew can change a square wave LFO to a
/// triangle or sine shape." A smoothing whose corner is a *fixed frequency*
/// cannot do that — it would flatten a fast LFO and leave a slow one
/// untouched — so the corner tracks the LFO: six octaves above its rate at
/// the bottom of the knob, where the edges are left alone, down to half its
/// rate at the top, where a square really does arrive as a triangle.
fn slew_coefficient(amount: f64, lfo_hz: f64, sr: f64) -> f64 {
    if amount <= 0.0 {
        return 1.0;
    }
    let corner = lfo_hz * (6.0 * (1.0 - amount.clamp(0.0, 1.0)) - 1.0).exp2();
    raw::one_pole(corner.clamp(0.001, sr * 0.45), sr).min(1.0)
}

// ── Overdrive ──
//
// "The TEO-5 has an Overdrive effect that is separate from the digital
// effects available in the Effect and Reverb sections... The character of the
// Overdrive is affected by the harmonic content of a program."
//
// The curve is the soft clipper `x / (1 + k|x|)`, which is the identity in
// f64 at `k = 0` — so the bottom of the knob is the program as voiced,
// exactly — and is monotonic and bounded, so it folds nothing back. The
// makeup gain is `1 + k`, which passes a signal at the rails through
// unchanged at every setting: quiet material gets the drive as gain, loud
// material gets squashed.
//
// It sits on the summed voices rather than inside one, because that is where
// the hardware puts it and because *Overdrive* is a modulation destination —
// see the mono pass in [`Teo5::process`].

/// How hard the top of the knob drives.
const OVERDRIVE_KNEE: f64 = 24.0;
/// The signal level the clipper's rails sit at, and therefore the level whose
/// loudness the knob leaves alone. Five incoherent voices at 0.707 after the
/// pan law is `√5 × 0.707`, which is where a full stack lands.
const OVERDRIVE_RAIL: f64 = 1.6;
/// How much *less* the negative half of the curve compresses than the
/// positive, which is what makes the overdrive produce even harmonics as well
/// as odd. The asymmetry is in the denominator only, so both halves keep the
/// same slope through zero: a kink at the zero crossing is crossover
/// distortion, which is a much nastier sound than a clipper whose bias is off
/// centre.
const OVERDRIVE_ASYMMETRY: f64 = 0.7;
/// Corner of the DC blocker that follows it.
const DC_BLOCK_HZ: f64 = 12.0;

#[inline]
fn overdrive(x: f64, amount: f64) -> f64 {
    if amount <= 0.0 {
        return x;
    }
    let k = amount * OVERDRIVE_KNEE;
    let bias = if x < 0.0 { OVERDRIVE_ASYMMETRY } else { 1.0 };
    x * (1.0 + k) / (1.0 + k * bias * x.abs() / OVERDRIVE_RAIL)
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

// ── Effect 1 ──
//
// "You can add up to two digital effects per program. The first effect can be
// one of several classic device emulations... The second effect is a
// dedicated reverb."
//
// **All twelve of the first unit's algorithms render.** They are all cheap,
// they are all measurable, and between them they cover the whole bank: 68 of
// the 256 programs choose the chorus, 114 choose one of the four delays, and
// the remaining 73 are spread across the other seven. The one that does not
// render is the *second* unit — the dedicated plate reverb — and that is a
// deferral rather than an omission: see the module documentation.
//
// The three knobs are re-purposed per algorithm, which is the manual's own
// table on page 41 and is why [`FxSetting`]'s fields are called `time`,
// `mix` and `misc` rather than being named after any one meaning. Two
// algorithms do not use `mix` as a wet/dry blend at all — the distortion's is
// an output level and the lo-fi's is a wow-and-flutter depth — and the ring
// modulator's `misc` is a switch rather than a knob.
mod fx {
    pub const OFF: usize = 0;
    pub const DELAY: usize = 1;
    pub const BBD: usize = 2;
    pub const TAPE1: usize = 3;
    pub const TAPE2: usize = 4;
    pub const CHORUS: usize = 5;
    pub const FLANGER: usize = 6;
    pub const PHASER: usize = 7;
    pub const HPF: usize = 8;
    pub const DISTORT: usize = 9;
    pub const RING: usize = 10;
    pub const ROTARY: usize = 11;
    pub const LOFI: usize = 12;

    /// Whether an algorithm's `time` knob is a delay time, and therefore
    /// whether the clock-sync switch has anything to sync.
    #[must_use]
    pub fn is_delay(kind: usize) -> bool {
        matches!(kind, DELAY | BBD | TAPE1 | TAPE2)
    }
}

/// The eleven synced divisions, in beats. See [`FX_DIVISIONS`] for why they
/// are in this order.
const FX_SYNC_BEATS: [f64; 11] =
    [0.25, 0.375, 1.0 / 3.0, 0.5, 0.75, 2.0 / 3.0, 1.0, 1.5, 4.0 / 3.0, 2.0, 4.0];

/// The sixteen synced LFO divisions, in beats. See [`LFO_DIVISIONS`].
const LFO_SYNC_BEATS: [f64; 16] = [
    16.0, 8.0, 4.0, 3.0, 2.0, 4.0 / 3.0, 1.5, 1.0, 2.0 / 3.0, 0.75, 0.5, 1.0 / 3.0, 0.375,
    0.25, 1.0 / 6.0, 0.125,
];

/// The longest delay the time knob reaches, and the shortest.
const DELAY_MAX_S: f64 = 1.0;
const DELAY_MIN_S: f64 = 0.002;
/// The most feedback the knob allows. Short of 1 so that a delay cannot run
/// away, which on an instrument whose output must stay under the limiter on
/// its own is a requirement rather than a taste.
const DELAY_FEEDBACK_MAX: f64 = 0.92;

/// Where a bucket-brigade line loses its treble, per repeat, and how far and
/// how fast its clock wanders. "BBD - vintage bucket-brigade delay emulation."
const BBD_LOSS_HZ: f64 = 2_600.0;
const BBD_WOW: f64 = 0.0025;
/// How often the clock picks a new place to drift to.
///
/// This number is the whole difference between wow and hiss. The read head
/// sits `delay × sr` samples behind the write head, so a modulator carrying
/// *any* energy near the top of the band moves the read head by that whole
/// factor between one sample and the next. See [`FxUnit::wander`] for how the
/// band limit is enforced rather than merely attenuated.
const BBD_WOW_HZ: f64 = 0.6;

/// A tape machine keeps more of its treble than a bucket brigade and wobbles
/// a little faster and a little less far. Tape 2 is "vintage tape delay
/// emulation with more tape saturation", so the only difference between the
/// two is how hard the loop is driven.
const TAPE_LOSS_HZ: f64 = 5_000.0;
const TAPE_WOW: f64 = 0.0016;
const TAPE_WOW_HZ: f64 = 1.1;
const TAPE1_DRIVE: f64 = 1.0;
const TAPE2_DRIVE: f64 = 3.0;

/// Chorus sweep centre and the widest it moves, in seconds, and the span of
/// its rate knob.
const CHORUS_CENTRE_S: f64 = 0.0072;
const CHORUS_SWEEP_S: f64 = 0.0048;
const CHORUS_MIN_HZ: f64 = 0.05;
const CHORUS_MAX_HZ: f64 = 8.0;

/// Flanger: "vintage through-zero flanger". The dry path is delayed by the
/// sweep's centre and the wet path sweeps from zero to twice it, so the
/// difference between them passes *through* zero rather than stopping at a
/// minimum — which is the whole point of the name, and the only way to get
/// the notch to sweep down through DC and back.
const FLANGER_CENTRE_S: f64 = 0.004;
const FLANGER_MIN_HZ: f64 = 0.02;
const FLANGER_MAX_HZ: f64 = 6.0;
const FLANGER_FEEDBACK_MAX: f64 = 0.88;

/// Phaser: "vintage 6-stage phaser". Six one-pole all-pass sections swept
/// between these corners, with feedback round the lot.
const PHASER_STAGES: usize = 6;
const PHASER_MIN_HZ: f64 = 0.02;
const PHASER_MAX_HZ: f64 = 8.0;
const PHASER_LOW_HZ: f64 = 180.0;
const PHASER_HIGH_HZ: f64 = 2_400.0;
const PHASER_FEEDBACK_MAX: f64 = 0.85;

/// The high-pass effect's corner span, and the ring modulator's carrier span.
const HPF_MIN_HZ: f64 = 20.0;
const HPF_MAX_HZ: f64 = 8_000.0;
const RING_MIN_HZ: f64 = 2.0;
const RING_MAX_HZ: f64 = 4_000.0;
/// The note the ring modulator's carrier tracks from when pitch tracking is
/// on, so that the carrier knob reads as an interval rather than as a pitch.
const RING_TRACK_NOTE: f64 = 60.0;

/// Rotating speaker: the horn's slow and fast speeds in hertz, the drum's
/// ratio to the horn, and where the cabinet's crossover sits.
const ROTARY_SLOW_HZ: f64 = 0.7;
const ROTARY_FAST_HZ: f64 = 6.8;
const ROTARY_DRUM_RATIO: f64 = 0.78;
const ROTARY_CROSSOVER_HZ: f64 = 800.0;
/// How far the horn's Doppler shift moves the read head, in seconds.
const ROTARY_DOPPLER_S: f64 = 0.0016;

/// Lo-Fi: "emulates the transformative effects of a badly-calibrated tape
/// machine". The frequency knob is the rate it re-samples at, the depth knob
/// is how much the transport wanders, and the misc knob drives it.
const LOFI_MIN_HZ: f64 = 700.0;
const LOFI_MAX_HZ: f64 = 24_000.0;
const LOFI_WOW_S: f64 = 0.004;
const LOFI_WOW_HZ: f64 = 1.7;
const LOFI_DRIVE_MAX: f64 = 12.0;

/// How long the short modulated line has to be: the longest centre plus the
/// widest sweep plus slack.
const SHORT_MAX_S: f64 = 0.03;

/// What the effect slot is set to, read once a block.
#[derive(Debug, Clone, Copy)]
struct FxSetting {
    kind: usize,
    /// The depth/mix knob: a wet/dry blend for nine of the twelve, an output
    /// level for the distortion and a wow depth for the lo-fi.
    mix: f64,
    /// The time knob, already in the units the algorithm wants: seconds for
    /// the delays, hertz for everything with a rate or a corner, a gain for
    /// the distortion.
    time: f64,
    /// The feedback/misc knob, likewise.
    misc: f64,
}

/// The effect unit: one long delay line, one short modulated line, and the
/// small amount of state the other algorithms need. Everything is allocated
/// in `init` and nothing is ever resized, so nothing here allocates on the
/// audio thread.
#[derive(Debug, Clone)]
struct FxUnit {
    /// Stereo, interleaved. Long enough for a one-second delay plus the
    /// furthest a wandering clock can push the read head past it.
    delay: Vec<f32>,
    write: usize,
    /// Stereo, interleaved, a few tens of milliseconds.
    short: Vec<f32>,
    short_write: usize,
    loop_lp: [f64; 2],
    wow_from: f64,
    wow_to: f64,
    wow_phase: f64,
    wow_noise: Noise,
    lfo_phase: f64,
    allpass: [[f64; PHASER_STAGES]; 2],
    feedback: [f64; 2],
    hp: [Svf; 2],
    carrier: f64,
    horn: f64,
    drum: f64,
    crossover: [f64; 2],
    hold: [f64; 2],
    hold_phase: f64,
}

impl FxUnit {
    fn new(seed: u32) -> Self {
        Self {
            delay: Vec::new(),
            write: 0,
            short: Vec::new(),
            short_write: 0,
            loop_lp: [0.0; 2],
            wow_from: 0.0,
            wow_to: 0.0,
            wow_phase: 0.0,
            wow_noise: Noise::new(seed),
            lfo_phase: 0.0,
            allpass: [[0.0; PHASER_STAGES]; 2],
            feedback: [0.0; 2],
            hp: [Svf::new(), Svf::new()],
            carrier: 0.0,
            horn: 0.0,
            drum: 0.0,
            crossover: [0.0; 2],
            hold: [0.0; 2],
            hold_phase: 0.0,
        }
    }

    fn init(&mut self, sr: f64) {
        // The longest delay, plus the furthest a wandering clock can drift
        // past it, plus the samples either side that the cubic read wants.
        let frames = (sr * DELAY_MAX_S * (1.0 + BBD_WOW)) as usize + 8;
        self.delay.clear();
        self.delay.resize(frames * 2, 0.0);
        let short = (sr * SHORT_MAX_S) as usize + 4;
        self.short.clear();
        self.short.resize(short * 2, 0.0);
        self.reset();
    }

    fn reset(&mut self) {
        self.delay.fill(0.0);
        self.short.fill(0.0);
        self.write = 0;
        self.short_write = 0;
        self.loop_lp = [0.0; 2];
        self.wow_from = 0.0;
        self.wow_to = 0.0;
        self.wow_phase = 0.0;
        self.lfo_phase = 0.0;
        self.allpass = [[0.0; PHASER_STAGES]; 2];
        self.feedback = [0.0; 2];
        self.hp[0].reset();
        self.hp[1].reset();
        self.carrier = 0.0;
        self.horn = 0.0;
        self.drum = 0.0;
        self.crossover = [0.0; 2];
        self.hold = [0.0; 2];
        self.hold_phase = 0.0;
    }

    /// A transport's drift, in ±1, with no high end at all.
    ///
    /// A one-pole on white noise is the obvious way to write this and it is
    /// the wrong one: a filter *attenuates* the top of the band, it does not
    /// remove it, and the residue is multiplied by the delay length in
    /// samples before it reaches the read head. What is needed is a modulator
    /// whose slope is bounded by construction, so this is a new random target
    /// `hz` times a second with a smoothstep in between — the value is
    /// continuous, its first derivative is continuous, and the fastest it can
    /// move is `1.5 × span × hz` per second.
    #[inline]
    fn wander(&mut self, hz: f64, sr: f64) -> f64 {
        self.wow_phase += hz / sr;
        if self.wow_phase >= 1.0 {
            self.wow_phase -= self.wow_phase.floor();
            self.wow_from = self.wow_to;
            self.wow_to = self.wow_noise.tick();
        }
        let t = self.wow_phase;
        self.wow_from + (self.wow_to - self.wow_from) * t * t * (3.0 - 2.0 * t)
    }

    /// Reads `back` frames behind the write head of an interleaved stereo
    /// buffer, linearly interpolated.
    #[inline]
    fn tap(buffer: &[f32], write: usize, back: f64, channel: usize) -> f64 {
        let frames = buffer.len() / 2;
        if frames < 4 {
            return 0.0;
        }
        let back = back.clamp(1.0, frames as f64 - 2.0);
        let whole = back as usize;
        let frac = back - whole as f64;
        let i0 = (write + frames - whole) % frames;
        let i1 = (i0 + frames - 1) % frames;
        let a = f64::from(buffer[i0 * 2 + channel]);
        let b = f64::from(buffer[i1 * 2 + channel]);
        a + (b - a) * frac
    }

    /// Reads `back` frames behind the write head, cubically interpolated.
    ///
    /// Four-point Catmull-Rom rather than a line. Both are exact on a
    /// stationary head; the difference shows only when the head is moving,
    /// where a line puts a slow ripple on the top of the band as the fraction
    /// sweeps and a cubic does not. It is worth the extra multiplies on the
    /// three lines whose read head wanders.
    #[inline]
    fn tap_cubic(buffer: &[f32], write: usize, back: f64, channel: usize) -> f64 {
        let frames = buffer.len() / 2;
        if frames < 8 {
            return 0.0;
        }
        let back = back.clamp(2.0, frames as f64 - 3.0);
        let whole = back as usize;
        let f = back - whole as f64;
        let at = |n: usize| f64::from(buffer[((write + frames - n) % frames) * 2 + channel]);
        let (p0, p1, p2, p3) = (at(whole - 1), at(whole), at(whole + 1), at(whole + 2));
        let a = 3.0 * (p1 - p2) + p3 - p0;
        let b = 2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3;
        let c = p2 - p0;
        p1 + 0.5 * f * (c + f * (b + f * a))
    }

    /// One first-order all-pass section.
    #[inline]
    fn allpass_stage(x: f64, a: f64, state: &mut f64) -> f64 {
        let y = -a * x + *state;
        *state = x + a * y;
        y
    }

    #[inline]
    fn process(&mut self, left: f64, right: f64, set: &FxSetting, sr: f64) -> (f64, f64) {
        match set.kind {
            fx::OFF => (left, right),
            // ── The four delays ──
            fx::DELAY => {
                if self.delay.len() < 16 {
                    return (left, right);
                }
                let frames = self.delay.len() / 2;
                let back = set.time * sr;
                let wet_l = Self::tap(&self.delay, self.write, back, 0);
                let wet_r = Self::tap(&self.delay, self.write, back, 1);
                // Ping-pong: each side's repeats feed the other, which is
                // what makes a stereo delay stereo rather than two mono ones.
                self.delay[self.write * 2] = (left + wet_r * set.misc) as f32;
                self.delay[self.write * 2 + 1] = (right + wet_l * set.misc) as f32;
                self.write = (self.write + 1) % frames;
                (
                    left * (1.0 - set.mix) + wet_l * set.mix,
                    right * (1.0 - set.mix) + wet_r * set.mix,
                )
            }
            fx::BBD | fx::TAPE1 | fx::TAPE2 => {
                if self.delay.len() < 16 {
                    return (left, right);
                }
                let frames = self.delay.len() / 2;
                let bucket = set.kind == fx::BBD;
                let (span, rate, loss, drive) = if bucket {
                    (BBD_WOW, BBD_WOW_HZ, BBD_LOSS_HZ, 1.0)
                } else if set.kind == fx::TAPE1 {
                    (TAPE_WOW, TAPE_WOW_HZ, TAPE_LOSS_HZ, TAPE1_DRIVE)
                } else {
                    (TAPE_WOW, TAPE_WOW_HZ, TAPE_LOSS_HZ, TAPE2_DRIVE)
                };
                let mono = (left + right) * 0.5;
                let back = set.time * sr * (1.0 + self.wander(rate, sr) * span);
                let wet = Self::tap_cubic(&self.delay, self.write, back, 0);
                let a = raw::one_pole(loss, sr);
                self.loop_lp[0] += a * (wet - self.loop_lp[0]);
                let fed = tanh_approx(self.loop_lp[0] * drive) / drive;
                self.delay[self.write * 2] = (mono + fed * set.misc) as f32;
                self.delay[self.write * 2 + 1] = 0.0;
                self.write = (self.write + 1) % frames;
                (left * (1.0 - set.mix) + wet * set.mix, right * (1.0 - set.mix) + wet * set.mix)
            }
            // ── Chorus ──
            fx::CHORUS => {
                if self.short.len() < 16 {
                    return (left, right);
                }
                let frames = self.short.len() / 2;
                self.lfo_phase = (self.lfo_phase + set.time / sr).fract();
                // Two sweeps a quarter cycle apart, which is what makes a
                // chorus wide rather than merely detuned.
                let sweep_l = (TAU * self.lfo_phase).sin();
                let sweep_r = (TAU * self.lfo_phase + PI * 0.5).sin();
                // The knob is a *depth* rather than a wet/dry blend — the
                // manual's table says so — and it moves both how far the line
                // sweeps and how much of it is heard, so that the bottom of
                // the travel is the dry signal rather than a static comb.
                let depth = (0.3 + 0.7 * set.mix) * CHORUS_SWEEP_S;
                let wet_l =
                    Self::tap(&self.short, self.short_write, (CHORUS_CENTRE_S + depth * sweep_l) * sr, 0);
                let wet_r =
                    Self::tap(&self.short, self.short_write, (CHORUS_CENTRE_S + depth * sweep_r) * sr, 1);
                self.short[self.short_write * 2] = left as f32;
                self.short[self.short_write * 2 + 1] = right as f32;
                self.short_write = (self.short_write + 1) % frames;
                // The misc knob is an LPF on the wet path: "Chorus ... FBACK/
                // MISC = LPF Cutoff".
                let a = raw::one_pole(set.misc, sr);
                self.loop_lp[0] += a * (wet_l - self.loop_lp[0]);
                self.loop_lp[1] += a * (wet_r - self.loop_lp[1]);
                (left + self.loop_lp[0] * set.mix, right + self.loop_lp[1] * set.mix)
            }
            // ── Through-zero flanger ──
            fx::FLANGER => {
                if self.short.len() < 16 {
                    return (left, right);
                }
                let frames = self.short.len() / 2;
                self.lfo_phase = (self.lfo_phase + set.time / sr).fract();
                let sweep = (TAU * self.lfo_phase).sin();
                let wet_back = (FLANGER_CENTRE_S * (1.0 + sweep)).max(0.0) * sr;
                let dry_back = FLANGER_CENTRE_S * sr;
                let wet_l = Self::tap_cubic(&self.short, self.short_write, wet_back, 0);
                let wet_r = Self::tap_cubic(&self.short, self.short_write, wet_back, 1);
                let dry_l = Self::tap(&self.short, self.short_write, dry_back, 0);
                let dry_r = Self::tap(&self.short, self.short_write, dry_back, 1);
                self.short[self.short_write * 2] = (left + wet_l * set.misc) as f32;
                self.short[self.short_write * 2 + 1] = (right + wet_r * set.misc) as f32;
                self.short_write = (self.short_write + 1) % frames;
                let depth = set.mix;
                (dry_l * (1.0 - depth * 0.5) + wet_l * depth, dry_r * (1.0 - depth * 0.5) + wet_r * depth)
            }
            // ── Six-stage phaser ──
            fx::PHASER => {
                self.lfo_phase = (self.lfo_phase + set.time / sr).fract();
                let sweep = 0.5 - 0.5 * (TAU * self.lfo_phase).cos();
                let corner = PHASER_LOW_HZ * (PHASER_HIGH_HZ / PHASER_LOW_HZ).powf(sweep);
                let g = raw::tpt_g(corner, sr);
                let a = (1.0 - g) / (1.0 + g);
                let mut out = [0.0f64; 2];
                for (channel, input) in [left, right].into_iter().enumerate() {
                    let mut x = input + self.feedback[channel] * set.misc;
                    for stage in 0..PHASER_STAGES {
                        x = Self::allpass_stage(x, a, &mut self.allpass[channel][stage]);
                    }
                    self.feedback[channel] = tanh_approx(x);
                    out[channel] = input * (1.0 - set.mix * 0.5) + x * set.mix;
                }
                (out[0], out[1])
            }
            // ── High-pass filter ──
            fx::HPF => {
                let q = Q_MIN * (Q_MAX / Q_MIN).powf(set.misc);
                let wet_l = self.hp[0].process(left, set.time, q, 1.0, false, sr);
                let wet_r = self.hp[1].process(right, set.time, q, 1.0, false, sr);
                (
                    left * (1.0 - set.mix) + wet_l * set.mix,
                    right * (1.0 - set.mix) + wet_r * set.mix,
                )
            }
            // ── Op-amp distortion ──
            //
            // "Distortion: Gain, Output Level, Tone" — the depth knob is a
            // level rather than a blend, so this one is always fully wet.
            fx::DISTORT => {
                let tone = raw::one_pole(set.misc, sr);
                let mut out = [0.0f64; 2];
                for (channel, input) in [left, right].into_iter().enumerate() {
                    let driven = tanh_approx(input * set.time);
                    self.loop_lp[channel] += tone * (driven - self.loop_lp[channel]);
                    // The tone knob tilts rather than cuts: at the bottom it
                    // is the low-passed signal, at the top the whole of it.
                    out[channel] = self.loop_lp[channel] * set.mix;
                }
                (out[0], out[1])
            }
            // ── Ring modulator ──
            fx::RING => {
                self.carrier = (self.carrier + set.time / sr).fract();
                let c = (TAU * self.carrier).sin();
                (
                    left * (1.0 - set.mix) + left * c * set.mix,
                    right * (1.0 - set.mix) + right * c * set.mix,
                )
            }
            // ── Rotating speaker ──
            //
            // A cabinet is a horn and a drum turning at different speeds
            // behind one crossover, and what a microphone in front of it
            // hears is amplitude from the direction the mouth is pointing and
            // pitch from how fast it is moving toward you. Both are here; the
            // misc knob is the microphone's distance, which trades the
            // amplitude swing for the room.
            fx::ROTARY => {
                if self.short.len() < 16 {
                    return (left, right);
                }
                let frames = self.short.len() / 2;
                let horn_hz = set.time;
                let drum_hz = horn_hz * ROTARY_DRUM_RATIO;
                self.horn = (self.horn + horn_hz / sr).fract();
                self.drum = (self.drum + drum_hz / sr).fract();
                let mono = (left + right) * 0.5;
                // The crossover: a one-pole split, so the two halves sum flat.
                let a = raw::one_pole(ROTARY_CROSSOVER_HZ, sr);
                self.crossover[0] += a * (mono - self.crossover[0]);
                let low = self.crossover[0];
                let high = mono - low;
                self.short[self.short_write * 2] = high as f32;
                self.short[self.short_write * 2 + 1] = low as f32;
                self.short_write = (self.short_write + 1) % frames;
                let swing = (1.0 - set.misc).clamp(0.0, 1.0);
                let horn_angle = TAU * self.horn;
                let drum_angle = TAU * self.drum;
                let doppler = (ROTARY_DOPPLER_S * (1.0 + horn_angle.cos())) * sr + 2.0;
                let horn = Self::tap_cubic(&self.short, self.short_write, doppler, 0);
                let drum = Self::tap(&self.short, self.short_write, 4.0, 1);
                let horn_am = 1.0 - swing * 0.6 * (0.5 - 0.5 * horn_angle.sin());
                let drum_am = 1.0 - swing * 0.35 * (0.5 - 0.5 * drum_angle.sin());
                // The two rotors are heard from opposite sides, which is what
                // makes a cabinet wide on two microphones.
                let wet_l = horn * horn_am * (0.5 + 0.5 * horn_angle.cos())
                    + drum * drum_am * (0.5 - 0.5 * drum_angle.cos());
                let wet_r = horn * horn_am * (0.5 - 0.5 * horn_angle.cos())
                    + drum * drum_am * (0.5 + 0.5 * drum_angle.cos());
                // The depth knob drives the cabinet: "Rotating Speaker:
                // Speed, Drive, Mic Distance".
                let drive = 1.0 + set.mix * 6.0;
                (tanh_approx(wet_l * drive) / drive.sqrt(), tanh_approx(wet_r * drive) / drive.sqrt())
            }
            // ── Lo-fi ──
            fx::LOFI => {
                if self.short.len() < 16 {
                    return (left, right);
                }
                let frames = self.short.len() / 2;
                // A badly calibrated transport: the tape wanders...
                let wander = self.wander(LOFI_WOW_HZ, sr) * set.mix * LOFI_WOW_S;
                self.short[self.short_write * 2] = left as f32;
                self.short[self.short_write * 2 + 1] = right as f32;
                self.short_write = (self.short_write + 1) % frames;
                let back = (LOFI_WOW_S + wander).max(0.0) * sr + 2.0;
                let wow_l = Self::tap_cubic(&self.short, self.short_write, back, 0);
                let wow_r = Self::tap_cubic(&self.short, self.short_write, back, 1);
                // ...and the machine samples at a rate of its own.
                self.hold_phase += set.time / sr;
                if self.hold_phase >= 1.0 {
                    self.hold_phase -= self.hold_phase.floor();
                    self.hold = [wow_l, wow_r];
                }
                let drive = 1.0 + set.misc * LOFI_DRIVE_MAX;
                (
                    tanh_approx(self.hold[0] * drive) / drive.sqrt(),
                    tanh_approx(self.hold[1] * drive) / drive.sqrt(),
                )
            }
            _ => (left, right),
        }
    }
}

// ── The modulation matrix ──
//
// "16 slots, 19 modulation sources, 65 destinations", plus three *direct*
// routings that index the same destination list with their own amount knobs:
// LFO 1's, LFO 2's and envelope 1's. Nineteen sources and sixty-five
// destinations is 1,235 combinations per slot, so nothing here is a special
// case: a slot is a source index, a signed amount and a destination index,
// and the destination index is turned into an *engine target* once, when the
// panel is read.
//
// **A modulation amount is a fraction of the destination parameter's own
// travel.** That is the one law, and it is what makes the sixty-five
// destinations one mechanism rather than sixty-five: an amount of +1 into
// *Cutoff* opens the filter by the whole of the cutoff knob, an amount of +1
// into *Osc 2 Detune* moves it the whole of the detune knob, and an amount of
// +1 into *Osc 1 Level* opens the fader completely. The accumulators below
// therefore hold fractions, and the voice multiplies by the travel when it
// applies them. Two destinations get a scale that is *not* their parameter's
// travel, and both are marked where they are defined: see
// [`OSC_MOD_SEMITONES`].
//
// Evaluation is per sample and per voice for the destinations that are
// per-voice, and once per sample in a **mono pass** for the eight that belong
// to the instrument rather than to a voice — LFO 1's rate and amount, the
// three effect knobs, the overdrive, the vintage knob and the unison detune.
// A poly source feeding a global destination has to come from somewhere, and
// the mono pass takes it from the voice that started most recently, which is
// the note the player is holding.
//
// Cost is what the *program* asks for, not what the matrix could do: the
// panel drops every slot whose source is Off, whose destination is No Dest,
// whose amount is exactly zero *and unmodulated*, or whose destination is one
// of the five that address the unrendered reverb. The bank's median program
// is left with five live routings and its worst with sixteen.
//
// Two things inside the loop are a sample behind, and both are deliberate.
// The three direct routings' amounts — the LFO amount knobs and envelope 1's
// — are modulatable destinations in their own right, and a modulator of an
// amount is itself part of the same accumulator, so they read the previous
// sample's value rather than iterating to a fixed point. So do the two LFO
// rates, because an LFO is a *source* and has to have produced its sample
// before the matrix that modulates it can run. Both are control-rate
// quantities and 23 microseconds is not a rate.

/// Modulation sources, the manual's Appendix A.
mod src {
    pub const OFF: usize = 0;
    pub const OSC2: usize = 1;
    pub const NOISE: usize = 2;
    pub const LFO1: usize = 3;
    pub const LFO2: usize = 4;
    pub const ENV1: usize = 5;
    pub const ENV2: usize = 6;
    pub const SPREAD: usize = 7;
    pub const BEND: usize = 8;
    pub const WHEEL: usize = 9;
    pub const PRESSURE: usize = 10;
    pub const BREATH: usize = 11;
    pub const FOOT: usize = 12;
    pub const EXPRESSION: usize = 13;
    pub const VELOCITY: usize = 14;
    pub const NOTE: usize = 15;
    pub const FILTER_OUT: usize = 16;
    pub const RANDOM: usize = 17;
    pub const DC: usize = 18;
    pub const AUDIO_OUT: usize = 19;
}

/// Per-voice modulation targets: the engine parameters a voice owns.
mod tgt {
    pub const O1_FREQ: usize = 0;
    pub const O2_FREQ: usize = 1;
    pub const ALL_FREQ: usize = 2;
    pub const O1_FINE: usize = 3;
    pub const O2_FINE: usize = 4;
    pub const ALL_FINE: usize = 5;
    pub const O1_WIDTH: usize = 6;
    pub const O2_WIDTH: usize = 7;
    pub const ALL_WIDTH: usize = 8;
    pub const O1_LEVEL: usize = 9;
    pub const O2_LEVEL: usize = 10;
    pub const SUB_LEVEL: usize = 11;
    pub const NOISE_LEVEL: usize = 12;
    pub const XMOD: usize = 13;
    pub const CUTOFF: usize = 14;
    pub const RESONANCE: usize = 15;
    pub const STATE: usize = 16;
    pub const LFO2_FREQ: usize = 17;
    pub const LFO2_AMT: usize = 18;
    pub const ENV1_AMT: usize = 19;
    pub const ENV2_AMT: usize = 20;
    pub const E1_DELAY: usize = 21;
    pub const E1_ATTACK: usize = 22;
    pub const E1_DECAY: usize = 23;
    pub const E1_SUSTAIN: usize = 24;
    pub const E1_RELEASE: usize = 25;
    pub const E2_DELAY: usize = 26;
    pub const E2_ATTACK: usize = 27;
    pub const E2_DECAY: usize = 28;
    pub const E2_SUSTAIN: usize = 29;
    pub const E2_RELEASE: usize = 30;
    pub const VOLUME: usize = 31;
    pub const PAN: usize = 32;
    pub const COUNT: usize = 33;
}

/// The destinations that belong to the instrument rather than to a voice.
mod gtgt {
    pub const LFO1_FREQ: usize = 0;
    pub const LFO1_AMT: usize = 1;
    pub const FX_MIX: usize = 2;
    pub const FX_TIME: usize = 3;
    pub const FX_MISC: usize = 4;
    pub const OVERDRIVE: usize = 5;
    pub const VINTAGE: usize = 6;
    pub const UNISON_DETUNE: usize = 7;
    pub const COUNT: usize = 8;
}

/// How far a full modulation amount moves an oscillator's *frequency*, in
/// semitones.
///
/// **The one destination whose depth is not its parameter's own travel, and
/// the reason is the bank.** The frequency parameter runs 0–63 semitones, and
/// reading a full amount as all 63 of them makes the single most common
/// gesture in the factory bank absurd: 127 of the 256 programs route the mod
/// wheel to LFO 1's amount — on 61 of them with LFO 1 pointed at *Osc All
/// Frequency* — at a median amount of 10 counts of 127, and 63 semitones
/// would turn the wheel into a five-octave siren instead of a vibrato. The Prophet-6's twelve
/// would fix that and break the other end: *Sync Growl* asks its filter
/// envelope for +0.49 of this depth, and a sync formant that sweeps by a
/// fifth is not a growl.
///
/// Two octaves is what fits both. The wheel gesture lands at 1.9 semitones at
/// the top of its travel — wide, which is what a wheel at maximum should be —
/// and *Sync Growl* gets a twelve-semitone sweep, which moves the sync
/// formant by a factor of two over the note.
const OSC_MOD_SEMITONES: f64 = 24.0;
/// How far a full modulation amount moves a *detune*: the detune control's
/// own travel, which is what the manual points vibrato at. "Route an LFO to
/// the osc 1 and/or osc 2 detune parameter to create vibrato."
const DETUNE_MOD_CENTS: f64 = 49.2;
/// How far a full modulation amount moves a pulse width: the whole of the
/// duty control, 50 % to 100 %.
const WIDTH_MOD: f64 = 0.5;

/// Which per-voice target a destination index reaches, if any.
const fn dest_voice_target(dest: usize) -> Option<usize> {
    Some(match dest {
        1 => tgt::O1_FREQ,
        2 => tgt::O2_FREQ,
        3 => tgt::ALL_FREQ,
        4 => tgt::O1_FINE,
        5 => tgt::O2_FINE,
        6 => tgt::ALL_FINE,
        7 => tgt::O1_WIDTH,
        8 => tgt::O2_WIDTH,
        9 => tgt::ALL_WIDTH,
        10 => tgt::O1_LEVEL,
        11 => tgt::O2_LEVEL,
        12 => tgt::SUB_LEVEL,
        13 => tgt::NOISE_LEVEL,
        14 => tgt::XMOD,
        15 => tgt::CUTOFF,
        16 => tgt::RESONANCE,
        17 => tgt::STATE,
        // 27 is LFO 2's rate; 28 is both LFOs, and its global half is picked
        // up by `dest_global_target`.
        27 | 28 => tgt::LFO2_FREQ,
        30 | 31 => tgt::LFO2_AMT,
        32 => tgt::ENV1_AMT,
        33 => tgt::ENV2_AMT,
        34 => tgt::E1_DELAY,
        35 => tgt::E2_DELAY,
        36 => tgt::E1_ATTACK,
        37 => tgt::E2_ATTACK,
        38 => tgt::E1_DECAY,
        39 => tgt::E2_DECAY,
        40 => tgt::E1_SUSTAIN,
        41 => tgt::E2_SUSTAIN,
        42 => tgt::E1_RELEASE,
        43 => tgt::E2_RELEASE,
        44 => tgt::VOLUME,
        45 => tgt::PAN,
        _ => return None,
    })
}

/// Which instrument-wide target a destination index reaches, if any.
const fn dest_global_target(dest: usize) -> Option<usize> {
    Some(match dest {
        18 => gtgt::FX_MIX,
        19 => gtgt::FX_TIME,
        20 => gtgt::FX_MISC,
        26 | 28 => gtgt::LFO1_FREQ,
        29 | 31 => gtgt::LFO1_AMT,
        46 => gtgt::OVERDRIVE,
        47 => gtgt::VINTAGE,
        48 => gtgt::UNISON_DETUNE,
        _ => return None,
    })
}

/// Which modulation slot's amount a destination index reaches, if any.
const fn dest_amount_slot(dest: usize) -> Option<usize> {
    if dest >= 49 && dest < 49 + MOD_SLOTS {
        Some(dest - 49)
    } else {
        None
    }
}

/// One live routing: where it comes from, which slot's amount it uses, what
/// that amount is, and what it reaches.
#[derive(Debug, Clone, Copy)]
struct Route {
    source: u8,
    /// Where this routing's own amount modulation is accumulated: its matrix
    /// slot, or one of the three reserved entries for the direct routings.
    slot: u8,
    amount: f64,
    target: u8,
}

/// The most routings either list can hold: the sixteen matrix slots plus the
/// three direct ones.
const MAX_ROUTES: usize = MOD_SLOTS + 3;

/// Slots in the amount-modulation scratch array: one per matrix slot, then
/// one each for LFO 1's, LFO 2's and envelope 1's own amount knobs.
const MAX_EXTRA: usize = MOD_SLOTS + 3;
/// Where the three direct routings' amounts live in that array.
const EXTRA_LFO1: usize = MOD_SLOTS;
const EXTRA_LFO2: usize = MOD_SLOTS + 1;
const EXTRA_ENV1: usize = MOD_SLOTS + 2;

/// Everything one voice can offer the matrix in one sample.
#[derive(Debug, Clone, Copy, Default)]
struct SourceSet {
    osc2: f64,
    noise: f64,
    lfo1: f64,
    lfo2: f64,
    env1: f64,
    env2: f64,
    spread: f64,
    bend: f64,
    wheel: f64,
    pressure: f64,
    breath: f64,
    foot: f64,
    expression: f64,
    velocity: f64,
    note: f64,
    filter_out: f64,
    random: f64,
    audio_out: f64,
}

impl SourceSet {
    #[inline]
    fn value(&self, source: usize) -> f64 {
        match source {
            src::OSC2 => self.osc2,
            src::NOISE => self.noise,
            src::LFO1 => self.lfo1,
            src::LFO2 => self.lfo2,
            src::ENV1 => self.env1,
            src::ENV2 => self.env2,
            src::SPREAD => self.spread,
            src::BEND => self.bend,
            src::WHEEL => self.wheel,
            src::PRESSURE => self.pressure,
            src::BREATH => self.breath,
            src::FOOT => self.foot,
            src::EXPRESSION => self.expression,
            src::VELOCITY => self.velocity,
            src::NOTE => self.note,
            src::FILTER_OUT => self.filter_out,
            src::RANDOM => self.random,
            src::DC => 1.0,
            src::AUDIO_OUT => self.audio_out,
            _ => 0.0,
        }
    }
}

/// Run one list of routings into an accumulator array.
///
/// `extra` carries the amount-modulating-amount pre-pass: slot *i*'s
/// effective amount is its knob plus whatever the pre-pass added. That pass
/// uses the *knob* amounts of the slots doing the modulating, so a chain of
/// three amount modulations applies only its first link — which is what a
/// single evaluation order can do without either iterating to a fixed point
/// or making the result depend on slot order.
#[inline]
fn run_routes(routes: &[Route], sources: &SourceSet, extra: &[f64; MAX_EXTRA], out: &mut [f64]) {
    for route in routes {
        let amount = route.amount + extra[route.slot as usize];
        out[route.target as usize] += sources.value(route.source as usize) * amount;
    }
}

/// The amount-modulating-amount pre-pass. The caller has already zeroed
/// `extra` and filled its last three entries with the previous sample's
/// modulation of the three direct amount knobs.
#[inline]
fn run_amounts(routes: &[(u8, f64, u8)], sources: &SourceSet, extra: &mut [f64; MAX_EXTRA]) {
    for (source, amount, slot) in routes {
        extra[*slot as usize] += sources.value(*source as usize) * amount;
    }
}

/// A geometric interpolation between two ends, for the knobs that are rates
/// or corners.
#[inline]
fn geometric(low: f64, high: f64, t: f64) -> f64 {
    low * (high / low).powf(t.clamp(0.0, 1.0))
}

/// The most gain the distortion effect's own knob reaches, on top of the
/// voice's overdrive.
const DISTORT_GAIN_MAX: f64 = 40.0;

/// The effect's three knobs as fractions of their travel, kept beside the
/// converted [`FxSetting`] so that modulation can move them and the physical
/// units can be recomputed from the same law rather than interpolated in the
/// wrong space.
#[derive(Debug, Clone, Copy)]
struct FxRaw {
    kind: usize,
    mix: f64,
    time: f64,
    misc: f64,
    /// The delay time the clock asks for, when the effect is synced and is
    /// one of the four that has a delay time to sync.
    synced: Option<f64>,
}

/// The effect's settings in the units each algorithm works in.
fn fx_setting(r: &FxRaw, mix_mod: f64, time_mod: f64, misc_mod: f64, ring_track: f64) -> FxSetting {
    let mix = (r.mix + mix_mod).clamp(0.0, 1.0);
    let t = (r.time + time_mod).clamp(0.0, 1.0);
    let m = (r.misc + misc_mod).clamp(0.0, 1.0);
    let (time, misc) = match r.kind {
        fx::DELAY | fx::BBD | fx::TAPE1 | fx::TAPE2 => (
            r.synced.unwrap_or_else(|| geometric(DELAY_MIN_S, DELAY_MAX_S, t)),
            m * DELAY_FEEDBACK_MAX,
        ),
        fx::CHORUS => (
            geometric(CHORUS_MIN_HZ, CHORUS_MAX_HZ, t),
            geometric(300.0, 12_000.0, m),
        ),
        fx::FLANGER => (
            geometric(FLANGER_MIN_HZ, FLANGER_MAX_HZ, t),
            m * FLANGER_FEEDBACK_MAX,
        ),
        fx::PHASER => (
            geometric(PHASER_MIN_HZ, PHASER_MAX_HZ, t),
            m * PHASER_FEEDBACK_MAX,
        ),
        fx::HPF => (geometric(HPF_MIN_HZ, HPF_MAX_HZ, t), m),
        fx::DISTORT => (1.0 + t * DISTORT_GAIN_MAX, geometric(300.0, 16_000.0, m)),
        // "Ring Modulation: Pitch/Carrier Freq, Mix, Pitch Track On/Off." The
        // switch makes the carrier knob an interval from the note played
        // rather than a fixed pitch, which is what makes a ring modulator
        // playable.
        fx::RING => {
            let carrier = geometric(RING_MIN_HZ, RING_MAX_HZ, t);
            if m >= 0.5 {
                (carrier * ring_track, 1.0)
            } else {
                (carrier, 0.0)
            }
        }
        fx::ROTARY => (ROTARY_SLOW_HZ + t * (ROTARY_FAST_HZ - ROTARY_SLOW_HZ), m),
        fx::LOFI => (geometric(LOFI_MIN_HZ, LOFI_MAX_HZ, t), m),
        _ => (0.0, 0.0),
    };
    FxSetting { kind: r.kind, mix, time, misc }
}

// ── The panel, read once a block ──

/// One envelope's five knobs, as fractions of their travel.
#[derive(Debug, Clone, Copy)]
struct EnvKnobs {
    delay: f64,
    attack: f64,
    decay: f64,
    sustain: f64,
    release: f64,
}

/// The five knobs as times, with each segment's modulation applied through
/// the same law the knob uses and the whole envelope scaled by whatever the
/// vintage knob has given this voice.
fn env_times(k: &EnvKnobs, m: &[f64], first: usize, scale: f64) -> EnvTimes {
    let seconds = |v: f64, offset: f64| raw::env_seconds((v + offset).clamp(0.0, 1.0) * 255.0) * scale;
    EnvTimes {
        delay: seconds(k.delay, m[first]),
        attack: seconds(k.attack, m[first + 1]),
        decay: seconds(k.decay, m[first + 2]),
        sustain: (k.sustain + m[first + 3]).clamp(0.0, 1.0),
        release: seconds(k.release, m[first + 4]),
    }
}

/// One oscillator's controls, converted.
#[derive(Debug, Clone, Copy)]
struct OscPanel {
    semitones: f64,
    cents: f64,
    wave: Waveset,
    key: bool,
    glide_seconds: f64,
    level: f64,
}

/// One LFO's controls, converted.
#[derive(Debug, Clone, Copy)]
struct LfoPanel {
    hz: f64,
    shape: usize,
    slew: f64,
    reset: bool,
}

/// Every live modulation routing a program asks for, sorted into the two
/// passes and the pre-pass.
#[derive(Debug, Clone, Copy)]
struct Routing {
    voice: [Route; MAX_ROUTES],
    voice_len: usize,
    global: [Route; MAX_ROUTES],
    global_len: usize,
    amounts: [(u8, f64, u8); MOD_SLOTS],
    amount_len: usize,
    /// Which routings have their *amount* modulated by something else. A
    /// routing whose knob is at zero still has to be kept when this is set,
    /// and that is not a corner case: 127 of the 256 factory programs leave
    /// LFO 1's amount at zero and put the mod wheel on it, which is the most
    /// common gesture in the bank.
    modulated: [bool; MAX_EXTRA],
}

impl Routing {
    fn new() -> Self {
        const EMPTY: Route = Route { source: 0, slot: 0, amount: 0.0, target: 0 };
        Self {
            voice: [EMPTY; MAX_ROUTES],
            voice_len: 0,
            global: [EMPTY; MAX_ROUTES],
            global_len: 0,
            amounts: [(0, 0.0, 0); MOD_SLOTS],
            amount_len: 0,
            modulated: [false; MAX_EXTRA],
        }
    }

    /// Note that something reaches `dest`, before any routing is pushed, so
    /// that an amount knob at zero with a modulator on it survives.
    fn note_amount_destination(&mut self, dest: usize) {
        if let Some(slot) = dest_amount_slot(dest) {
            self.modulated[slot] = true;
        }
        // 29 and 30 are the two LFO amounts and 31 is both of them; 32 is
        // envelope 1's.
        if dest == 29 || dest == 31 {
            self.modulated[EXTRA_LFO1] = true;
        }
        if dest == 30 || dest == 31 {
            self.modulated[EXTRA_LFO2] = true;
        }
        if dest == 32 {
            self.modulated[EXTRA_ENV1] = true;
        }
    }

    /// Add a routing, dropping the ones that cannot do anything: a source of
    /// *Off*, a destination of *No Dest*, an amount of exactly zero with
    /// nothing modulating it, and the five destinations that address the
    /// reverb this build does not render.
    fn push(&mut self, source: usize, slot: usize, amount: f64, dest: usize) {
        if source == src::OFF
            || dest == 0
            || (amount == 0.0 && !self.modulated[slot.min(MAX_EXTRA - 1)])
        {
            return;
        }
        let slot = slot.min(MAX_EXTRA - 1) as u8;
        if let Some(target) = dest_amount_slot(dest) {
            if self.amount_len < MOD_SLOTS {
                self.amounts[self.amount_len] = (source as u8, amount, target as u8);
                self.amount_len += 1;
            }
        }
        if let Some(target) = dest_voice_target(dest) {
            if self.voice_len < MAX_ROUTES {
                self.voice[self.voice_len] =
                    Route { source: source as u8, slot, amount, target: target as u8 };
                self.voice_len += 1;
            }
        }
        if let Some(target) = dest_global_target(dest) {
            if self.global_len < MAX_ROUTES {
                self.global[self.global_len] =
                    Route { source: source as u8, slot, amount, target: target as u8 };
                self.global_len += 1;
            }
        }
    }
}

/// Every control, converted out of knob positions into the units the engine
/// works in. Built once per `process` call rather than per sample: none of it
/// can change inside a block, and a hundred and fifty exponentials per sample
/// for numbers that move when a finger does is not a trade worth making.
#[derive(Debug, Clone, Copy)]
struct Panel {
    o1: OscPanel,
    o2: OscPanel,
    o2_bypass: bool,
    x_mod: f64,
    sync: bool,

    sub_level: f64,
    noise_level: f64,
    noise_pink: bool,

    cutoff_note: f64,
    resonance: f64,
    state: f64,
    bandpass: bool,
    filter_key: f64,

    e1: EnvKnobs,
    e2: EnvKnobs,
    e1_amount: f64,
    e2_amount: f64,
    e1_velocity: bool,
    e2_velocity: bool,
    env_route: usize,
    e1_repeat: bool,
    e2_repeat: bool,

    lfo1: LfoPanel,
    lfo2: LfoPanel,

    routing: Routing,

    fx_on: bool,
    fx_raw: FxRaw,

    overdrive: f64,
    vintage: f64,
    volume: f64,
    pan: f64,

    unison: bool,
    unison_voices: usize,
    unison_detune: f64,
    key_mode: usize,
    retrigger: bool,

    glide_on: bool,
    glide_mode: usize,

    split_shift: f64,
    split_note: f64,

    bend_up: f64,
    bend_down: f64,
    transpose: f64,
}

/// A continuous control's raw instrument value.
fn knob(params: &[f32; PARAM_COUNT], index: usize) -> f64 {
    let max = match index {
        P_CUTOFF => CUTOFF_BYTES.2,
        P_STATE => STATE_BYTES.2,
        _ => raw_offset(index).map_or(1.0, |(_, max)| max),
    };
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
        let beat = 60.0 / bpm;

        let oscillator = |freq, fine, tri, saw, pulse, width, key, glide, on, level| OscPanel {
            semitones: raw::osc_semitones(knob(params, freq)),
            cents: raw::fine_cents(knob(params, fine)),
            wave: Waveset {
                tri: f64::from(u8::from(flag(params, tri))),
                saw: f64::from(u8::from(flag(params, saw))),
                pulse: f64::from(u8::from(flag(params, pulse))),
                duty: raw::duty(knob(params, width)),
            },
            key: flag(params, key),
            glide_seconds: raw::glide_seconds(knob(params, glide)),
            level: if flag(params, on) { raw::level(knob(params, level)) } else { 0.0 },
        };

        // The envelope knobs stay as fractions of their travel rather than
        // becoming seconds here, because ten of the sixty-five modulation
        // destinations are envelope segments and a modulated segment has to
        // be evaluated through the same law rather than interpolated between
        // two times. The voice turns them into seconds once a block.
        let envelope = |delay, attack, decay, sustain, release| EnvKnobs {
            delay: raw::level(knob(params, delay)),
            attack: (knob(params, attack) / 255.0).clamp(0.0, 1.0),
            decay: (knob(params, decay) / 255.0).clamp(0.0, 1.0),
            sustain: raw::level(knob(params, sustain)),
            release: (knob(params, release) / 255.0).clamp(0.0, 1.0),
        };

        let lfo = |freq, shape, sync, div, reset, slew| {
            let hz = if flag(params, sync) {
                (1.0 / (LFO_SYNC_BEATS[step_of(params, div)] * beat)).min(sr * 0.45)
            } else {
                raw::lfo_hz(knob(params, freq)).min(sr * 0.45)
            };
            LfoPanel {
                hz,
                shape: step_of(params, shape),
                // The slew's corner tracks the rate, so it has to be worked
                // out after the clock-sync switch has had its say.
                slew: slew_coefficient(raw::level(knob(params, slew)), hz, sr),
                reset: flag(params, reset),
            }
        };
        let lfo1 = lfo(P_L1_FREQ, P_L1_SHAPE, P_L1_SYNC, P_L1_DIV, P_L1_RESET, P_L1_SLEW);
        let lfo2 = lfo(P_L2_FREQ, P_L2_SHAPE, P_L2_SYNC, P_L2_DIV, P_L2_RESET, P_L2_SLEW);

        // ── The matrix ──
        //
        // The three direct routings first, so that a program with no matrix
        // slots still has its LFOs and its auxiliary envelope, then the
        // sixteen slots.
        let mut routing = Routing::new();
        // Two passes: the first only notes which amounts are themselves
        // modulated, so that the second knows which zeroed knobs to keep.
        routing.note_amount_destination(step_of(params, P_L1_DEST));
        routing.note_amount_destination(step_of(params, P_L2_DEST));
        routing.note_amount_destination(step_of(params, P_E1_DEST));
        for slot in 0..MOD_SLOTS {
            let base = P_MOD + 3 * slot;
            if step_of(params, base) != src::OFF {
                routing.note_amount_destination(step_of(params, base + 2));
            }
        }
        routing.push(
            src::LFO1,
            EXTRA_LFO1,
            raw::bipolar(knob(params, P_L1_AMOUNT), 254.0),
            step_of(params, P_L1_DEST),
        );
        routing.push(
            src::LFO2,
            EXTRA_LFO2,
            raw::bipolar(knob(params, P_L2_AMOUNT), 254.0),
            step_of(params, P_L2_DEST),
        );
        routing.push(
            src::ENV1,
            EXTRA_ENV1,
            raw::bipolar(knob(params, P_E1_AMOUNT), 254.0),
            step_of(params, P_E1_DEST),
        );
        for slot in 0..MOD_SLOTS {
            let base = P_MOD + 3 * slot;
            routing.push(
                step_of(params, base),
                slot,
                raw::bipolar(knob(params, base + 1), 254.0),
                step_of(params, base + 2),
            );
        }

        let kind = step_of(params, P_FX_TYPE);
        let fx_raw = FxRaw {
            kind,
            mix: raw::level(knob(params, P_FX_MIX)),
            time: (knob(params, P_FX_TIME) / 255.0).clamp(0.0, 1.0),
            misc: (knob(params, P_FX_MISC) / 255.0).clamp(0.0, 1.0),
            synced: if flag(params, P_FX_SYNC) && fx::is_delay(kind) {
                // A synced division longer than the line is halved until it
                // fits, which is what the Prophet-6's manual spells out for
                // the same machinery.
                let mut seconds = FX_SYNC_BEATS[step_of(params, P_FX_DIV)] * beat;
                while seconds > DELAY_MAX_S {
                    seconds *= 0.5;
                }
                Some(seconds)
            } else {
                None
            },
        };

        let unison_step = step_of(params, P_UNISON_VOICES);
        Self {
            o1: oscillator(
                P_O1_FREQ, P_O1_FINE, P_O1_TRI, P_O1_SAW, P_O1_PULSE, P_O1_WIDTH, P_O1_KEY,
                P_O1_GLIDE, P_O1_ON, P_O1_LEVEL,
            ),
            o2: oscillator(
                P_O2_FREQ, P_O2_FINE, P_O2_TRI, P_O2_SAW, P_O2_PULSE, P_O2_WIDTH, P_O2_KEY,
                P_O2_GLIDE, P_O2_ON, P_O2_LEVEL,
            ),
            o2_bypass: flag(params, P_O2_BYPASS),
            x_mod: raw::level(knob(params, P_XMOD)),
            sync: flag(params, P_SYNC),

            sub_level: if flag(params, P_SUB_ON) {
                raw::level(knob(params, P_SUB_LEVEL))
            } else {
                0.0
            },
            noise_level: if flag(params, P_NOISE_ON) {
                raw::level(knob(params, P_NOISE_LEVEL))
            } else {
                0.0
            },
            noise_pink: flag(params, P_NOISE_TYPE),

            cutoff_note: raw::cutoff_note(knob(params, P_CUTOFF)),
            resonance: raw::resonance(knob(params, P_RESONANCE)),
            state: raw::state(knob(params, P_STATE)),
            bandpass: flag(params, P_BANDPASS),
            filter_key: raw::level(knob(params, P_FILTER_KEY)),

            e1: envelope(P_E1_DELAY, P_E1_ATTACK, P_E1_DECAY, P_E1_SUSTAIN, P_E1_RELEASE),
            e2: envelope(P_E2_DELAY, P_E2_ATTACK, P_E2_DECAY, P_E2_SUSTAIN, P_E2_RELEASE),
            e1_amount: raw::bipolar(knob(params, P_E1_AMOUNT), 254.0),
            e2_amount: raw::level(knob(params, P_E2_AMOUNT)),
            e1_velocity: flag(params, P_E1_VEL),
            e2_velocity: flag(params, P_E2_VEL),
            env_route: step_of(params, P_ENV_ROUTE),
            e1_repeat: matches!(step_of(params, P_ENV_REPEAT), 1 | 3),
            e2_repeat: matches!(step_of(params, P_ENV_REPEAT), 2 | 3),

            lfo1,
            lfo2,
            routing,

            fx_on: flag(params, P_FX_ON),
            fx_raw,

            overdrive: raw::level(knob(params, P_OVERDRIVE)),
            vintage: raw::level(knob(params, P_VINTAGE)),
            volume: raw::level(knob(params, P_VOLUME)),
            pan: raw::bipolar(knob(params, P_PAN), 254.0),

            unison: flag(params, P_UNISON),
            // 0 is one voice and 5 is "all", which on a five-voice
            // instrument is the same as 4. See [`UNISON_VOICES`].
            unison_voices: (unison_step + 1).min(VOICES),
            unison_detune: step_of(params, P_UNISON_DETUNE) as f64 / 7.0,
            key_mode: step_of(params, P_KEY_MODE),
            retrigger: flag(params, P_RETRIGGER),

            glide_on: flag(params, P_GLIDE),
            glide_mode: step_of(params, P_GLIDE_MODE),

            // "Low Split, lower half -1 octave" and "-2 octaves". Two
            // switches rather than a selector on the hardware, and both are
            // on in no factory program, so the deeper one wins.
            split_shift: if flag(params, P_SPLIT_2) {
                -24.0
            } else if flag(params, P_SPLIT_1) {
                -12.0
            } else {
                0.0
            },
            split_note: SPLIT_LOW_NOTE + knob(params, P_SPLIT_NOTE),

            bend_up: step_of(params, P_BEND_UP) as f64,
            bend_down: step_of(params, P_BEND_DOWN) as f64,
            transpose: (step_of(params, P_TRANSPOSE) as f64 - 2.0) * 12.0,
        }
    }
}

/// The MIDI note the TEO-5's leftmost key plays.
///
/// The keyboard is 44 keys and `key_split_note` is 0–43, so the split point
/// is a key number rather than a note number and something has to say where
/// the keyboard starts. C2 puts the bank's stock split of 19 at G3, in the
/// middle of the keyboard, which is where a low split belongs.
const SPLIT_LOW_NOTE: f64 = 36.0;

// ── The voice ──
//
// Five of them, which is the instrument. Each carries two oscillators, the
// sub, its own noise and pink filter, the state-variable filter, both
// envelopes and **LFO 2**, which is the per-voice one — "LFO 2 is a per-voice
// modulator that is applied to each voice individually, and can vary from
// voice to voice." LFO 1 is one per instrument, as the same page says.
//
// The oscillators **free-run**: nothing resets a phase on note-on, and each
// voice starts life at its own offset. That is what an analog polysynth does,
// and it is what makes a five-voice unison stack five voices rather than one
// voice 14 dB louder.

pub const VOICES: usize = 5;

/// "Hold down a chord on the keyboard (5 notes maximum)."
const MAX_CHORD: usize = 5;
/// How many keys the keyboard remembers at once.
const MAX_HELD: usize = 16;
/// The maximum number of MIDI events sorted in place per block.
const MAX_EVENTS: usize = 256;

/// The note an oscillator plays from when its keyboard switch is off: "the
/// oscillator plays at its base frequency setting".
const OSC_NO_KEY_NOTE: f64 = 60.0;

/// The narrowest duty modulation will drive the pulse to. Zero would be
/// silence and negative would be the same pulse the other way up; the knob
/// itself stops at 0.5, and only modulation gets past it.
const DUTY_MIN: f64 = 0.02;

/// How deep the through-zero FM goes at the top of the X-Mod knob, as a
/// multiple of the carrier frequency.
///
/// "X-Mod: Sets the amount of through zero fequency modulation from
/// Oscillator 2's triangle waveshape to Oscillator 1's frequency. X-Mod in
/// the TEO-5, unlike digital FM techniques, uses modulation between analog
/// oscillators."
///
/// **Linear, and proportional to the carrier.** Both halves matter. Linear —
/// adding hertz rather than semitones — is what "through zero" means: the
/// instantaneous frequency swings symmetrically about the carrier and its
/// *average* is therefore still the carrier, so the pitch does not move as
/// the depth rises. An exponential modulation of the same depth would raise
/// the average frequency, because the mean of an exponential is not the
/// exponential of the mean, and the note would sharpen as the knob turned.
/// Proportional to the carrier keeps the modulation index, and therefore the
/// timbre, the same at every note.
///
/// Four is where the knob stops: at the top the deviation is four times the
/// carrier, so with the two oscillators in tune the instantaneous frequency
/// spends a third of every cycle *negative* and the phase runs backwards.
/// That is the region the hardware's own manual warns about — "some pitch
/// instability may be present at certain settings" — and it is where the
/// textures the instrument was sold on live.
const X_MOD_INDEX: f64 = 4.0;

/// How many octaves a full modulation of an LFO's rate moves it: the whole of
/// the published 0.022 Hz to 500 Hz travel.
const LFO_MOD_OCTAVES: f64 = 14.472;

/// Where the filter's cutoff stops, whatever the knob and the modulation ask
/// for.
///
/// A VCF core has a rail, and the top of the cutoff parameter is deliberately
/// past the audible band, so something has to stop it. Stopping it at the
/// *sample rate* is not that something: a resonant peak parked at 0.45 of the
/// sample rate is at 19.8 kHz at 44.1 kHz and at 43 kHz at 96 kHz, so a
/// program with the filter wide open and the resonance up would be a
/// different sound on a different audio device.
const CUTOFF_MAX_HZ: f64 = 18_000.0;

/// How far the vintage knob spreads the voices at the top of its travel:
/// cents of pitch, semitones of cutoff, and a fraction of every envelope
/// segment's length.
///
/// "Turning up the vintage knob adds progressively more filter, pitch, and
/// envelope variation between voices." All three, and no more than that — the
/// knob is not a slop knob and the manual does not describe it drifting, so
/// each voice takes a fixed offset of its own rather than a random walk. The
/// square taper is what makes the bottom of the knob subtle: the bank's
/// median non-zero setting of 55 comes out at 4.7 cents.
const VINTAGE_CENTS: f64 = 25.0;
const VINTAGE_CUTOFF_SEMITONES: f64 = 8.0;
const VINTAGE_ENV_SPREAD: f64 = 0.3;

/// How far the unison detune knob spreads a stack at its top setting, in
/// cents. "A setting of 0 is minimum detuning. A setting of 7 is maximum
/// detuning."
const UNISON_DETUNE_CENTS: f64 = 28.0;

/// The sub oscillator's shape: a square.
///
/// **Judgment**: the manual says only "Sub: Toggles the sub oscillator's
/// output, which is generated from Osc 1", and gives no waveform. A
/// sub-octave generated *from* another oscillator is a flip-flop on its
/// cycle, and a flip-flop makes a square; that is what this platform's
/// sub-octave has always been, and it is what the ninety programs that switch
/// it on — median level 107 of 127 across the bass category — are leaning on.
const SUB_WAVE: Waveset = Waveset { tri: 0.0, saw: 0.0, pulse: 1.0, duty: 0.5 };

/// Where each voice's oscillators start, in cycles.
///
/// **Not** a low-discrepancy sequence, and that is the point. Five phases
/// spread evenly round the circle is exactly the configuration in which five
/// oscillators at the same frequency cancel, and the instrument's headline
/// sound is five of them at the same frequency. So the phases are a hash of
/// the oscillator's index, and [`PHASE_SEED`] was chosen by measuring the
/// resulting stack.
fn start_phase(voice: usize, osc: usize) -> f64 {
    let n = ((voice * 3 + osc + 1) as u32).wrapping_mul(0x9E37_79B9).wrapping_add(PHASE_SEED);
    f64::from(mix32(n)) / f64::from(u32::MAX)
}

/// See [`start_phase`]. Chosen by measurement, not by taste.
const PHASE_SEED: u32 = 0x03F2_EC8A;

/// Each voice's place in the spread — the *Voice Spread* modulation source,
/// and what the unison detune is multiplied by. Alternating rather than
/// left-to-right, so that three sounding voices out of five are still spread
/// rather than all on one side.
const VOICE_SPREAD: [f64; VOICES] = [-1.0, 0.6, -0.2, 1.0, -0.6];

/// The three fixed offsets the vintage knob gives one voice: pitch, cutoff
/// and envelope length, each −1…+1.
fn vintage_offsets(index: usize) -> (f64, f64, f64) {
    let at = |salt: u32| {
        let n = (index as u32 + 1).wrapping_mul(0x9E37_79B9).wrapping_add(salt);
        f64::from(mix32(n)) / f64::from(u32::MAX) * 2.0 - 1.0
    };
    (at(0x0000_0011), at(0x0000_2222), at(0x0033_3333))
}

/// What every voice shares within one sample.
#[derive(Debug, Clone, Copy)]
struct Shared {
    lfo1: f64,
    /// The pitch wheel in semitones, already scaled by the range it is bent
    /// into, and the raw −1…+1 the matrix sees.
    bend: f64,
    bend_raw: f64,
    wheel: f64,
    pressure: f64,
    breath: f64,
    foot: f64,
    expression: f64,
    sr: f64,
    cutoff_ceiling_hz: f64,
    gate_rate: f64,
    vintage: f64,
    unison: bool,
    unison_detune: f64,
    global: [f64; gtgt::COUNT],
}

struct Voice {
    index: usize,
    osc1: Osc,
    osc2: Osc,
    sub: Osc,
    noise: Noise,
    pink: Pink,
    filter: Svf,
    env1: Envelope,
    env2: Envelope,
    lfo2: Lfo,
    /// Where the matrix leaves its per-voice modulation, refilled every
    /// sample. Read at the top of a block for the envelope segments, whose
    /// times are only worth recomputing when they can actually have moved.
    targets: [f64; tgt::COUNT],
    note: u8,
    velocity: u8,
    /// The pitch this voice is heading for, after the transpose switch and
    /// the keyboard split.
    base_note: f64,
    /// One glide position per oscillator: "Portamento can be set individually
    /// for each oscillator."
    glide: [f64; 2],
    glide_rate: [f64; 2],
    pitched: bool,
    gate: bool,
    /// The gated amplifier of the third envelope routing, smoothed.
    gate_level: f64,
    age: u64,
    /// The *Random* modulation source: one value per note, held. A
    /// sample-and-hold on note-on rather than a running noise, which is what
    /// the 38 factory routings from it want — a fresh detune or a fresh
    /// filter offset per key, not a hiss.
    random: f64,
    random_stream: Noise,
    spread: f64,
    vintage: (f64, f64, f64),
    /// Last sample's oscillator 2, filter and voice outputs — the three
    /// modulation sources that are the engine listening to itself.
    osc2_last: f64,
    lfo2_last: f64,
    filter_last: f64,
    audio_last: f64,
}

impl Voice {
    fn new(index: usize, sr: f64) -> Self {
        // Every seed distinct, and distinct from every other voice's.
        let seed = |slot: u32| (index as u32 * 4 + slot + 1).wrapping_mul(0x9E37_79B9) | 1;
        let mut pink = Pink::default();
        pink.init(sr);
        Self {
            index,
            osc1: Osc::new(start_phase(index, 0)),
            osc2: Osc::new(start_phase(index, 1)),
            sub: Osc::new(start_phase(index, 2)),
            noise: Noise::new(seed(0)),
            pink,
            filter: Svf::new(),
            env1: Envelope::new(sr),
            env2: Envelope::new(sr),
            lfo2: Lfo::new(seed(1)),
            targets: [0.0; tgt::COUNT],
            note: 60,
            velocity: 100,
            base_note: 60.0,
            glide: [60.0; 2],
            glide_rate: [0.0; 2],
            pitched: false,
            gate: false,
            gate_level: 0.0,
            age: 0,
            random: 0.0,
            random_stream: Noise::new(seed(2)),
            spread: VOICE_SPREAD[index % VOICES],
            vintage: vintage_offsets(index),
            osc2_last: 0.0,
            lfo2_last: 0.0,
            filter_last: 0.0,
            audio_last: 0.0,
        }
    }

    fn reset(&mut self) {
        self.osc1.reset(start_phase(self.index, 0));
        self.osc2.reset(start_phase(self.index, 1));
        self.sub.reset(start_phase(self.index, 2));
        self.filter.reset();
        self.pink.reset();
        self.lfo2.reset();
        self.env1.kill();
        self.env2.kill();
        self.targets = [0.0; tgt::COUNT];
        self.gate = false;
        self.gate_level = 0.0;
        self.pitched = false;
        self.osc2_last = 0.0;
        self.lfo2_last = 0.0;
        self.filter_last = 0.0;
        self.audio_last = 0.0;
    }

    fn is_free(&self) -> bool {
        !self.gate && !self.env2.is_active() && self.gate_level < 1.0e-4
    }

    /// The envelope times, once a block: they involve three exponentials
    /// each, they can only move when a knob or a modulator moves, and a
    /// modulator that moves inside 5.8 ms is not modulating an envelope
    /// length.
    fn begin_block(&mut self, p: &Panel, vintage: f64) {
        let scale = (1.0 + vintage * vintage * VINTAGE_ENV_SPREAD * self.vintage.2).max(0.05);
        self.env1.repeat = p.e1_repeat;
        self.env2.repeat = p.e2_repeat;
        let e1 = env_times(&p.e1, &self.targets, tgt::E1_DELAY, scale);
        let e2 = env_times(&p.e2, &self.targets, tgt::E2_DELAY, scale);
        self.env1.set_times(e1);
        self.env2.set_times(e2);
    }

    /// Point the voice at a note. `glide` is whether it should slide there
    /// from where it is rather than jump.
    fn retune(&mut self, note: u8, p: &Panel, sr: f64, glide: bool) {
        self.note = note;
        // "Low Split, lower half -1 octave": the split transposes the keys
        // under the split point and leaves the rest alone.
        let split = if f64::from(note) < p.split_note { p.split_shift } else { 0.0 };
        self.base_note = f64::from(note) + p.transpose + split;
        if !self.pitched || !glide {
            self.glide = [self.base_note; 2];
            self.pitched = true;
        }
        for (rate, seconds) in
            self.glide_rate.iter_mut().zip([p.o1.glide_seconds, p.o2.glide_seconds])
        {
            let seconds = seconds.max(1.0e-6);
            // Fixed rate: an octave takes the knob's time whatever the
            // interval. Fixed time: the whole interval takes it.
            *rate = if p.glide_mode >= 2 {
                (self.base_note - self.glide[0]).abs() / seconds / sr
            } else {
                12.0 / seconds / sr
            };
        }
    }

    /// Recompute the envelope times from the modulation as it stands at
    /// note-on, before the envelopes are triggered.
    ///
    /// Ten of the sixty-five destinations are envelope segments, and the two
    /// sources the bank most often points at them — velocity and note number
    /// — are only *knowable* at note-on. Leaving those to the per-block
    /// update would give the note being struck the previous note's attack,
    /// which is exactly backwards.
    fn latch_envelopes(&mut self, p: &Panel, mono: &SourceSet, vintage: f64) {
        let sources = SourceSet {
            osc2: self.osc2_last,
            lfo2: self.lfo2_last,
            env1: self.env1.level,
            env2: self.env2.level,
            spread: self.spread,
            velocity: f64::from(self.velocity) / 127.0,
            note: self.base_note / 127.0,
            filter_out: self.filter_last,
            random: self.random,
            audio_out: self.audio_last,
            ..*mono
        };
        let mut extra = [0.0f64; MAX_EXTRA];
        extra[EXTRA_LFO2] = self.targets[tgt::LFO2_AMT];
        extra[EXTRA_ENV1] = self.targets[tgt::ENV1_AMT];
        run_amounts(&p.routing.amounts[..p.routing.amount_len], &sources, &mut extra);
        let mut targets = [0.0f64; tgt::COUNT];
        run_routes(&p.routing.voice[..p.routing.voice_len], &sources, &extra, &mut targets);
        let scale = (1.0 + vintage * vintage * VINTAGE_ENV_SPREAD * self.vintage.2).max(0.05);
        self.env1.set_times(env_times(&p.e1, &targets, tgt::E1_DELAY, scale));
        self.env2.set_times(env_times(&p.e2, &targets, tgt::E2_DELAY, scale));
    }

    fn start(&mut self, velocity: u8, age: u64, from_zero: bool, lfo_reset: bool) {
        self.velocity = velocity;
        self.gate = true;
        self.age = age;
        self.random = self.random_stream.tick();
        self.env1.trigger(from_zero);
        self.env2.trigger(from_zero);
        // "restart LFO phase on note-on", which for the per-voice LFO means
        // this voice's note rather than anybody else's — the whole point of
        // LFO 2 being per voice.
        if lfo_reset {
            self.lfo2.restart();
        }
    }

    fn release(&mut self) {
        self.gate = false;
        self.env1.release_env();
        self.env2.release_env();
    }

    #[inline]
    fn tick(&mut self, p: &Panel, s: &Shared) -> (f64, f64) {
        if self.is_free() {
            return (0.0, 0.0);
        }
        let env1 = self.env1.tick();
        let env2 = self.env2.tick();

        // LFO 2, whose rate the matrix can move. It has to be computed before
        // the matrix runs, because it is one of the matrix's own sources, so
        // its rate modulation is the previous sample's — 23 microseconds of
        // latency on a control that moves at LFO rate.
        let lfo2_hz = if self.targets[tgt::LFO2_FREQ] == 0.0 {
            p.lfo2.hz
        } else {
            (p.lfo2.hz * (self.targets[tgt::LFO2_FREQ] * LFO_MOD_OCTAVES).exp2())
                .clamp(0.001, s.sr * 0.45)
        };
        let lfo2 = self.lfo2.tick(lfo2_hz, s.sr, p.lfo2.shape, p.lfo2.slew);
        self.lfo2_last = lfo2;

        let white = self.noise.tick();
        let velocity = f64::from(self.velocity) / 127.0;
        // "Velocity Off/On: Allows key velocity to influence filter cutoff
        // frequency and amp volume." Envelope 1's switch scales the envelope
        // itself, so it reaches wherever the envelope reaches — the filter in
        // the first routing, and whatever the matrix points it at in the
        // other two, where it is the auxiliary envelope. Envelope 2's cannot
        // be written that way, because the manual carves out an exception for
        // it: "if the Env 2 velocity button is on, it affects only Filter
        // Cutoff and not Amplifier volume."
        let env1 = env1 * if p.e1_velocity { velocity } else { 1.0 };
        let sources = SourceSet {
            osc2: self.osc2_last,
            noise: white,
            lfo1: s.lfo1,
            lfo2,
            env1,
            env2,
            spread: self.spread,
            bend: s.bend_raw,
            wheel: s.wheel,
            pressure: s.pressure,
            breath: s.breath,
            foot: s.foot,
            expression: s.expression,
            velocity,
            note: self.base_note / 127.0,
            filter_out: self.filter_last,
            random: self.random,
            audio_out: self.audio_last,
        };

        let mut extra = [0.0f64; MAX_EXTRA];
        extra[EXTRA_LFO1] = s.global[gtgt::LFO1_AMT];
        extra[EXTRA_LFO2] = self.targets[tgt::LFO2_AMT];
        extra[EXTRA_ENV1] = self.targets[tgt::ENV1_AMT];
        let routing = &p.routing;
        run_amounts(&routing.amounts[..routing.amount_len], &sources, &mut extra);
        self.targets = [0.0; tgt::COUNT];
        run_routes(&routing.voice[..routing.voice_len], &sources, &extra, &mut self.targets);
        let t = &self.targets;

        // Glide, at whatever rate the mode chose, once per oscillator.
        if p.glide_on {
            for (position, rate) in self.glide.iter_mut().zip(&self.glide_rate) {
                let remaining = self.base_note - *position;
                if remaining.abs() <= *rate {
                    *position = self.base_note;
                } else {
                    *position += rate.copysign(remaining);
                }
            }
        } else {
            self.glide = [self.base_note; 2];
        }

        let detune = self.spread
            * (if s.unison { s.unison_detune * UNISON_DETUNE_CENTS } else { 0.0 }
                + s.vintage * s.vintage * VINTAGE_CENTS * self.vintage.0);

        // ── Oscillator 2, computed first because it is the sync master and
        // the modulator for through-zero FM ──

        let key2 = if p.o2.key { self.glide[1] } else { OSC_NO_KEY_NOTE };
        let note2 = key2
            + p.o2.semitones
            + (t[tgt::O2_FREQ] + t[tgt::ALL_FREQ]) * OSC_MOD_SEMITONES
            + (p.o2.cents + detune + (t[tgt::O2_FINE] + t[tgt::ALL_FINE]) * DETUNE_MOD_CENTS)
                / 100.0
            + s.bend;
        let dt2 = (raw::note_hz(note2) / s.sr).clamp(0.0, 0.45);
        let wave2 = Waveset {
            duty: (p.o2.wave.duty + (t[tgt::O2_WIDTH] + t[tgt::ALL_WIDTH]) * WIDTH_MOD)
                .clamp(DUTY_MIN, 1.0),
            ..p.o2.wave
        };
        let sync_at = if p.sync { self.osc2.wraps_at(dt2) } else { None };
        let out2 = self.osc2.tick(dt2, &wave2, None);
        // "from Oscillator 2's triangle waveshape" — the modulator is the
        // triangle whether or not the triangle is switched into the mixer.
        let modulator = 1.0 - 4.0 * (self.osc2.phase - 0.5).abs();

        // ── Oscillator 1 ──

        let key1 = if p.o1.key { self.glide[0] } else { OSC_NO_KEY_NOTE };
        let note1 = key1
            + p.o1.semitones
            + (t[tgt::O1_FREQ] + t[tgt::ALL_FREQ]) * OSC_MOD_SEMITONES
            + (p.o1.cents + detune + (t[tgt::O1_FINE] + t[tgt::ALL_FINE]) * DETUNE_MOD_CENTS)
                / 100.0
            + s.bend;
        let index = (p.x_mod + t[tgt::XMOD]).clamp(0.0, 1.0) * X_MOD_INDEX;
        let dt1 = (raw::note_hz(note1) * (1.0 + index * modulator) / s.sr).clamp(-0.45, 0.45);
        let wave1 = Waveset {
            duty: (p.o1.wave.duty + (t[tgt::O1_WIDTH] + t[tgt::ALL_WIDTH]) * WIDTH_MOD)
                .clamp(DUTY_MIN, 1.0),
            ..p.o1.wave
        };
        let out1 = self.osc1.tick(dt1, &wave1, sync_at);
        let sub = self.sub.tick(dt1 * 0.5, &SUB_WAVE, None);
        let noise = if p.noise_pink { self.pink.tick(white) } else { white };

        // ── The mixer ──

        let osc2_out = out2 * (p.o2.level + t[tgt::O2_LEVEL]).clamp(0.0, 1.0);
        let mut pre = out1 * (p.o1.level + t[tgt::O1_LEVEL]).clamp(0.0, 1.0)
            + sub * (p.sub_level + t[tgt::SUB_LEVEL]).clamp(0.0, 1.0)
            + noise * (p.noise_level + t[tgt::NOISE_LEVEL]).clamp(0.0, 1.0);
        if !p.o2_bypass {
            pre += osc2_out;
        }

        // ── The envelopes, wherever the routing switch sends them ──
        //
        // "Env 1 Destinations: filter, aux. Env 2 Destinations: amp, filter +
        // amp, filter + gate." So in the first routing envelope 1 is the
        // filter's and envelope 2 is the amplifier's; in the other two
        // envelope 1 has gone to the matrix and envelope 2 has taken the
        // filter over, which is exactly where the bank puts its `env2_amount`
        // variation — 184 of the 210 programs in the first routing leave that
        // control at maximum and only 3 of the 46 in the other two do.
        let e2_amount = (p.e2_amount + t[tgt::ENV2_AMT]).clamp(0.0, 1.0);
        let e2_velocity = if p.e2_velocity { velocity } else { 1.0 };
        let (filter_env, amp_env, gated) = if p.env_route == 0 {
            (env1 * (p.e1_amount + t[tgt::ENV1_AMT]), env2 * e2_amount * e2_velocity, false)
        } else {
            // "if the Env 2 velocity button is on, it affects only Filter
            // Cutoff and not Amplifier volume."
            (env2 * e2_amount * e2_velocity, env2, p.env_route == 2)
        };

        // ── The filter ──

        // "Filter Key Amt: any setting above zero means that the higher the
        // note played on the keyboard, the more the filter opens." A semitone
        // of cutoff per semitone of keyboard at the top of the knob, pivoted
        // at middle C, which is the tracking every filter in this rack has.
        let cutoff_note = p.cutoff_note
            + p.filter_key * (self.glide[0] - 60.0)
            + (filter_env + t[tgt::CUTOFF]) * raw::CUTOFF_SEMITONES
            + s.vintage * s.vintage * VINTAGE_CUTOFF_SEMITONES * self.vintage.1;
        let hz = raw::note_hz(cutoff_note).clamp(5.0, s.cutoff_ceiling_hz);
        let q = resonance_q((p.resonance + t[tgt::RESONANCE]).clamp(0.0, 1.0));
        let state = (p.state + t[tgt::STATE]).clamp(0.0, 1.0);
        let filtered = self.filter.process(pre, hz, q, state, p.bandpass, s.sr);
        self.filter_last = filtered;

        // "Osc 2 Filter Bypass: Turning this on causes Oscillator 2 to be
        // directly routed to the VCA, bypassing the filter."
        let signal = voice_limit(if p.o2_bypass { filtered + osc2_out } else { filtered });

        // ── The amplifier ──

        self.gate_level += s.gate_rate * (f64::from(u8::from(self.gate)) - self.gate_level);
        let gain = if gated { self.gate_level } else { amp_env };
        let out = signal * gain.clamp(0.0, 1.0) * (p.volume + t[tgt::VOLUME]).clamp(0.0, 1.0);
        self.osc2_last = out2;
        self.audio_last = out;

        // Equal power across the field, so that panning does not change how
        // loud the instrument is.
        let angle = ((p.pan + t[tgt::PAN]).clamp(-1.0, 1.0) + 1.0) * (PI / 4.0);
        (out * angle.cos(), out * angle.sin())
    }
}

// ── The instrument ──

/// The chord a factory program remembers, as semitone offsets from the note
/// played. Empty for the 236 programs that store the empty-slot code in all
/// five slots.
#[must_use]
pub fn program_chord(index: usize) -> &'static [u8] {
    let program = &programs()[index.min(PROGRAM_COUNT - 1)];
    &program.chord[..program.chord_len as usize]
}

pub struct Teo5 {
    params: [f32; PARAM_COUNT],
    sample_rate: f64,
    voices: [Voice; VOICES],
    /// "LFO 1 is a Global LFO and is a single modulator that is applied to
    /// all voices in a program equally."
    lfo1: Lfo,
    mono_noise: Noise,
    fx: FxUnit,
    dc_left: DcBlock,
    dc_right: DcBlock,
    /// The mono pass's accumulator, kept between samples so that the three
    /// direct routings can read last sample's amount modulation.
    global: [f64; gtgt::COUNT],
    /// The instrument-wide half of the matrix's sources, as they stood at the
    /// last sample. A note-on lands in the middle of a block and has to latch
    /// its envelope times from something.
    mono_sources: SourceSet,

    held: [u8; MAX_HELD],
    held_velocity: [u8; MAX_HELD],
    held_len: usize,
    /// The chord memory, as intervals from the note played. Loaded with the
    /// program and replaced by the keyboard gesture.
    chord: [u8; MAX_CHORD],
    chord_len: usize,
    /// Which note the unison stack is currently sounding, if any.
    unison_note: Option<u8>,
    /// Where the unison switch was, so that a move *to* on can be told from a
    /// program load that happens to select it.
    unison_was: bool,
    /// Which voice started most recently: the one the mono pass reads its
    /// poly sources from, and the note the ring modulator tracks.
    lead: usize,
    last_note: f64,
    /// Increments on every note-on, so that "oldest" has a meaning.
    clock: u64,

    /// CC 1, CC 2, CC 4, CC 11 and channel pressure — five of the twenty
    /// modulation sources arrive from outside.
    wheel: f64,
    pressure: f64,
    breath: f64,
    foot: f64,
    expression: f64,
    /// The pitch wheel, −1…+1, before the two range switches scale it.
    bend: f64,
}

impl Teo5 {
    #[must_use]
    pub fn new() -> Self {
        let sr = 44_100.0;
        let mut synth = Self {
            params: param_defaults(),
            sample_rate: sr,
            voices: std::array::from_fn(|i| Voice::new(i, sr)),
            lfo1: Lfo::new(0x1F35_1F35),
            mono_noise: Noise::new(0x5A5A_0F0F),
            fx: FxUnit::new(0x0FF1_CE21),
            dc_left: DcBlock::default(),
            dc_right: DcBlock::default(),
            global: [0.0; gtgt::COUNT],
            mono_sources: SourceSet::default(),
            held: [0; MAX_HELD],
            held_velocity: [0; MAX_HELD],
            held_len: 0,
            chord: [0; MAX_CHORD],
            chord_len: 0,
            unison_note: None,
            unison_was: false,
            lead: 0,
            last_note: 60.0,
            clock: 0,
            wheel: 0.0,
            pressure: 0.0,
            breath: 0.0,
            foot: 0.0,
            expression: 0.0,
            bend: 0.0,
        };
        synth.sync_params_from_program();
        synth.fx.init(sr);
        synth
    }

    /// Which factory program the two selectors are pointing at, 0–255.
    #[must_use]
    pub fn current_program(&self) -> usize {
        program_index(self.params[P_BANK], self.params[P_PROGRAM])
    }

    /// The chord memory, as intervals from the note played. Empty when the
    /// program carries none and nothing has been captured.
    #[must_use]
    pub fn chord_memory(&self) -> &[u8] {
        &self.chord[..self.chord_len]
    }

    fn sync_params_from_program(&mut self) {
        let index = self.current_program();
        self.params = params_for_program(self.params[P_BANK], self.params[P_PROGRAM]);
        let program = &programs()[index];
        self.chord = program.chord;
        self.chord_len = program.chord_len as usize;
        self.unison_was = flag(&self.params, P_UNISON);
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

    /// Which held key wins under the selected key-assign mode. "Unison Key
    /// Mode: Low, High, Last."
    fn winner(&self, key_mode: usize) -> Option<usize> {
        if self.held_len == 0 {
            return None;
        }
        let keys = &self.held[..self.held_len];
        Some(match key_mode {
            0 => keys.iter().enumerate().min_by_key(|(_, n)| **n).map_or(0, |(i, _)| i),
            1 => keys.iter().enumerate().max_by_key(|(_, n)| **n).map_or(0, |(i, _)| i),
            _ => self.held_len - 1,
        })
    }

    /// Capture the keys that are down as the chord memory. "Hold down a chord
    /// on the keyboard (5 notes maximum). Press the unison switch."
    fn capture_chord(&mut self) {
        let mut notes = self.held;
        notes[..self.held_len].sort_unstable();
        let len = self.held_len.min(MAX_CHORD);
        self.chord_len = len;
        let base = i16::from(notes[0]);
        for (slot, held) in self.chord[..len].iter_mut().zip(&notes[..len]) {
            *slot = (i16::from(*held) - base).clamp(0, 127) as u8;
        }
    }

    /// Clear it: "Turn off Unison. Hold down a single note. Press the unison
    /// button."
    fn clear_chord(&mut self) {
        self.chord_len = 0;
    }

    /// Give a voice a note, gliding if the mode and the moment say so.
    fn place(&mut self, voice: usize, note: u8, velocity: u8, panel: &Panel, retrigger: bool) {
        let sr = self.sample_rate;
        let clock = self.clock;
        let mono = self.mono_sources;
        let vintage = (panel.vintage + self.global[gtgt::VINTAGE]).clamp(0.0, 1.0);
        let v = &mut self.voices[voice];
        // "Fixed Rate A" and "Fixed Time A" only glide when playing legato.
        let legato = v.gate || v.env2.is_active();
        let glide = panel.glide_on && if panel.glide_mode % 2 == 1 { legato } else { v.pitched };
        v.retune(note, panel, sr, glide);
        v.velocity = velocity;
        v.latch_envelopes(panel, &mono, vintage);
        if retrigger || !legato {
            v.start(velocity, clock, !legato, panel.lfo2.reset);
        } else {
            v.velocity = velocity;
            v.gate = true;
            v.age = clock;
        }
        self.lead = voice;
        self.last_note = f64::from(note);
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

        // Chord memory transposes the stored voicing so that its root is the
        // key played: "Single notes played on the keyboard then trigger all
        // notes of the stored chord, transposing them as you play up or down
        // the keyboard."
        let mut notes = [note; MAX_CHORD];
        let mut count = 1usize;
        if self.chord_len > 0 {
            count = self.chord_len;
            for (slot, interval) in notes[..count].iter_mut().zip(&self.chord[..count]) {
                *slot = (i16::from(note) + i16::from(*interval)).clamp(0, 127) as u8;
            }
        }

        let stack = panel.unison_voices.max(count).min(VOICES);
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
        let released =
            self.voices.iter().enumerate().filter(|(_, v)| !v.gate).min_by_key(|(_, v)| v.age);
        if let Some((index, _)) = released {
            return index;
        }
        self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map_or(0, |(index, _)| index)
    }

    fn note_on(&mut self, note: u8, velocity: u8, panel: &Panel) {
        // "LFO 1 is a Global LFO", so its note reset is the start of a
        // phrase rather than every key: restarting it on the fourth note of a
        // held chord would step the modulation of the three already sounding.
        if panel.lfo1.reset && self.voices.iter().all(Voice::is_free) {
            self.lfo1.restart();
        }
        self.remember(note, velocity);
        self.clock += 1;
        if panel.unison {
            self.retarget_unison(panel, panel.retrigger);
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
        self.fx.reset();
        self.dc_left.reset();
        self.dc_right.reset();
        self.global = [0.0; gtgt::COUNT];
    }
}

impl Default for Teo5 {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for Teo5 {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "TEO-5".into(),
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
        self.lfo1.reset();
        self.fx.init(sample_rate);
        self.dc_left.reset();
        self.dc_right.reset();
        self.global = [0.0; gtgt::COUNT];
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
        let sr = self.sample_rate;
        let panel = Panel::read(&self.params, sr);

        let vintage = (panel.vintage + self.global[gtgt::VINTAGE]).clamp(0.0, 1.0);
        for voice in &mut self.voices {
            voice.begin_block(&panel, vintage);
        }

        let shared_base = Shared {
            lfo1: 0.0,
            bend: 0.0,
            bend_raw: 0.0,
            wheel: 0.0,
            pressure: 0.0,
            breath: 0.0,
            foot: 0.0,
            expression: 0.0,
            sr,
            cutoff_ceiling_hz: (sr * 0.45).min(CUTOFF_MAX_HZ),
            gate_rate: raw::one_pole(GATE_HZ, sr),
            vintage,
            unison: panel.unison,
            unison_detune: panel.unison_detune,
            global: self.global,
        };
        let dc = (-TAU * DC_BLOCK_HZ / sr).exp();
        let ring_track = raw::note_hz(self.last_note) / raw::note_hz(RING_TRACK_NOTE);
        let base_fx = fx_setting(&panel.fx_raw, 0.0, 0.0, 0.0, ring_track);

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
        let routing = &panel.routing;

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
                    0xD0 => self.pressure = f64::from(event.data1) / 127.0,
                    0xE0 => {
                        let raw = i32::from(event.data2) * 128 + i32::from(event.data1);
                        self.bend = f64::from(raw - 8_192) / 8_192.0;
                    }
                    0xB0 => match event.data1 {
                        1 => self.wheel = f64::from(event.data2) / 127.0,
                        2 => self.breath = f64::from(event.data2) / 127.0,
                        4 => self.foot = f64::from(event.data2) / 127.0,
                        11 => self.expression = f64::from(event.data2) / 127.0,
                        120 => self.kill_all(),
                        123 => self.all_notes_off(),
                        _ => {}
                    },
                    _ => {}
                }
                next_event += 1;
            }

            // LFO 1, the global one. Its rate modulation is the previous
            // sample's for the same reason LFO 2's is: it is one of the
            // matrix's own sources.
            let lfo1_hz = if self.global[gtgt::LFO1_FREQ] == 0.0 {
                panel.lfo1.hz
            } else {
                (panel.lfo1.hz * (self.global[gtgt::LFO1_FREQ] * LFO_MOD_OCTAVES).exp2())
                    .clamp(0.001, sr * 0.45)
            };
            let lfo1 = self.lfo1.tick(lfo1_hz, sr, panel.lfo1.shape, panel.lfo1.slew);

            let bend_semitones =
                self.bend * if self.bend >= 0.0 { panel.bend_up } else { panel.bend_down };

            // ── The mono pass ──
            //
            // Eight of the sixty-five destinations belong to the instrument
            // rather than to a voice. A poly source feeding one of them has to
            // come from somewhere, and it comes from the voice that started
            // most recently — the note under the player's hand.
            let (lead_osc2, lead_lfo2, lead_env1, lead_env2, lead_spread, lead_velocity,
                 lead_note, lead_filter, lead_random, lead_audio, lead_lfo2_amt, lead_env1_amt) = {
                let lead = &self.voices[self.lead];
                (
                    lead.osc2_last,
                    lead.lfo2_last,
                    lead.env1.level,
                    lead.env2.level,
                    lead.spread,
                    f64::from(lead.velocity) / 127.0,
                    lead.base_note / 127.0,
                    lead.filter_last,
                    lead.random,
                    lead.audio_last,
                    lead.targets[tgt::LFO2_AMT],
                    lead.targets[tgt::ENV1_AMT],
                )
            };
            let mono = SourceSet {
                osc2: lead_osc2,
                noise: self.mono_noise.tick(),
                lfo1,
                lfo2: lead_lfo2,
                env1: lead_env1,
                env2: lead_env2,
                spread: lead_spread,
                bend: self.bend,
                wheel: self.wheel,
                pressure: self.pressure,
                breath: self.breath,
                foot: self.foot,
                expression: self.expression,
                velocity: lead_velocity,
                note: lead_note,
                filter_out: lead_filter,
                random: lead_random,
                audio_out: lead_audio,
            };
            let mut extra = [0.0f64; MAX_EXTRA];
            extra[EXTRA_LFO1] = self.global[gtgt::LFO1_AMT];
            extra[EXTRA_LFO2] = lead_lfo2_amt;
            extra[EXTRA_ENV1] = lead_env1_amt;
            run_amounts(&routing.amounts[..routing.amount_len], &mono, &mut extra);
            self.global = [0.0; gtgt::COUNT];
            run_routes(&routing.global[..routing.global_len], &mono, &extra, &mut self.global);
            self.mono_sources = mono;

            let shared = Shared {
                lfo1,
                bend: bend_semitones,
                bend_raw: self.bend,
                wheel: self.wheel,
                pressure: self.pressure,
                breath: self.breath,
                foot: self.foot,
                expression: self.expression,
                vintage: (panel.vintage + self.global[gtgt::VINTAGE]).clamp(0.0, 1.0),
                unison_detune: (panel.unison_detune + self.global[gtgt::UNISON_DETUNE])
                    .clamp(0.0, 1.0),
                global: self.global,
                ..shared_base
            };

            let mut left = 0.0;
            let mut right = 0.0;
            for voice in &mut self.voices {
                let (l, r) = voice.tick(&panel, &shared);
                left += l;
                right += r;
            }

            let drive = (panel.overdrive + self.global[gtgt::OVERDRIVE]).clamp(0.0, 1.0);
            if drive > 0.0 {
                left = self.dc_left.tick(overdrive(left, drive), dc);
                right = self.dc_right.tick(overdrive(right, drive), dc);
            }

            if panel.fx_on {
                let setting = if self.global[gtgt::FX_MIX] == 0.0
                    && self.global[gtgt::FX_TIME] == 0.0
                    && self.global[gtgt::FX_MISC] == 0.0
                {
                    base_fx
                } else {
                    fx_setting(
                        &panel.fx_raw,
                        self.global[gtgt::FX_MIX],
                        self.global[gtgt::FX_TIME],
                        self.global[gtgt::FX_MISC],
                        ring_track,
                    )
                };
                let (l, r) = self.fx.process(left, right, &setting, sr);
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
                P_E1_DELAY | P_E1_ATTACK | P_E1_DECAY | P_E1_RELEASE | P_E2_DELAY
                | P_E2_ATTACK | P_E2_DECAY | P_E2_RELEASE | P_O1_GLIDE | P_O2_GLIDE => "s".into(),
                P_L1_FREQ | P_L2_FREQ | P_CUTOFF => "Hz".into(),
                P_O1_FREQ | P_O2_FREQ => "semi".into(),
                P_O1_FINE | P_O2_FINE => "cents".into(),
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
            // The hardware gesture: "hold down a chord on the keyboard, press
            // the unison switch" memorises it, and pressing it with a single
            // note held clears the memory.
            P_UNISON => {
                let now = flag(&self.params, P_UNISON);
                if now && !self.unison_was {
                    if self.held_len > 1 {
                        self.capture_chord();
                    } else if self.held_len == 1 {
                        self.clear_chord();
                    }
                }
                self.unison_was = now;
            }
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.kill_all();
        self.lfo1.reset();
        self.wheel = 0.0;
        self.pressure = 0.0;
        self.breath = 0.0;
        self.foot = 0.0;
        self.expression = 0.0;
        self.bend = 0.0;
        self.clock = 0;
        self.lead = 0;
    }
}

/// How fast the gated amplifier of the third envelope routing opens and
/// closes. Fast enough to be a gate, slow enough not to be a click.
const GATE_HZ: f64 = 400.0;

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
    pub(crate) fn fresh(index: usize) -> Teo5 {
        let mut s = Teo5::new();
        s.init(SR, BLOCK);
        let (bank, program) = program_knobs(index);
        s.set_parameter(P_BANK, bank);
        s.set_parameter(P_PROGRAM, program);
        s.reset();
        s
    }

    /// A synth with the panel set by hand rather than by a program.
    pub(crate) fn built(setup: &[(usize, f32)]) -> Teo5 {
        at_rate(setup, SR)
    }

    pub(crate) fn at_rate(setup: &[(usize, f32)], sr: f64) -> Teo5 {
        let mut s = Teo5::new();
        s.init(sr, BLOCK);
        for (index, value) in setup {
            s.set_parameter(*index, *value);
        }
        s.reset();
        s
    }

    pub(crate) fn render(synth: &mut Teo5, events: &[MidiEvent], blocks: usize) -> Vec<f32> {
        let mut left = vec![0.0f32; BLOCK];
        let mut right = vec![0.0f32; BLOCK];
        let mut out = Vec::with_capacity(blocks * BLOCK);
        for block in 0..blocks {
            left.fill(0.0);
            right.fill(0.0);
            let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
            synth.process(&[], &mut outs, if block == 0 { events } else { &[] });
            out.extend_from_slice(&left);
        }
        out
    }

    /// Both channels, for the tests that care about the stereo field.
    fn render_stereo(synth: &mut Teo5, events: &[MidiEvent], blocks: usize) -> (Vec<f32>, Vec<f32>) {
        let mut left = vec![0.0f32; BLOCK];
        let mut right = vec![0.0f32; BLOCK];
        let (mut l, mut r) = (Vec::new(), Vec::new());
        for block in 0..blocks {
            left.fill(0.0);
            right.fill(0.0);
            let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
            synth.process(&[], &mut outs, if block == 0 { events } else { &[] });
            l.extend_from_slice(&left);
            r.extend_from_slice(&right);
        }
        (l, r)
    }

    /// A held chord, rendered from a synth already pointed at a program.
    pub(crate) fn render_program(
        synth: &mut Teo5,
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

    fn window_rms(x: &[f32]) -> Vec<f64> {
        x.chunks(4096).filter(|c| c.len() == 4096).map(rms).collect()
    }

    /// The centre of gravity of the spectrum, in hertz: the rms of the first
    /// difference over the rms of the signal is the spectral centroid in
    /// radians a sample.
    pub(crate) fn brightness(x: &[f32], sr: f64) -> f64 {
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

    /// The centre of gravity of the spectrum *under* `hz`, which is what a
    /// spectrum can be compared on across sample rates: at 22.05 kHz there is
    /// no band above 11 kHz to have a centre of gravity in.
    pub(crate) fn brightness_below(x: &[f32], hz: f64, sr: f64) -> f64 {
        let a = raw::one_pole(hz, sr);
        let (mut s1, mut s2, mut s3, mut s4) = (0.0f64, 0.0, 0.0, 0.0);
        let limited: Vec<f32> = x
            .iter()
            .map(|v| {
                s1 += a * (f64::from(*v) - s1);
                s2 += a * (s1 - s2);
                s3 += a * (s2 - s3);
                s4 += a * (s3 - s4);
                s4 as f32
            })
            .collect();
        brightness(&limited, sr)
    }

    /// The magnitude of one frequency in a signal, by a single DFT bin.
    pub(crate) fn harmonic(x: &[f32], hz: f64, sr: f64) -> f64 {
        let w = TAU * hz / sr;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, v) in x.iter().enumerate() {
            let phase = w * n as f64;
            re += f64::from(*v) * phase.cos();
            im += f64::from(*v) * phase.sin();
        }
        (re * re + im * im).sqrt() / x.len() as f64
    }

    pub(crate) fn db(x: f64) -> f64 {
        20.0 * x.max(1.0e-30).log10()
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

    /// The repetition rate of a waveform, by autocorrelation.
    pub(crate) fn fundamental_hz(x: &[f32], sr: f64, low: f64, high: f64) -> f64 {
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

    /// Every control at a neutral position: no modulation anywhere, one
    /// sawtooth at unity, the filter out of the way and the amplifier open
    /// for as long as the key is down.
    ///
    /// Built explicitly rather than by starting from a factory program and
    /// overriding what a test cares about, so that no program's stored
    /// routing can quietly appear in a measurement of something else.
    pub(crate) fn neutral() -> Vec<(usize, f32)> {
        let off = |index: usize| (index, 0.0f32);
        let mut panel = vec![
            off(P_O1_FREQ),
            (P_O1_FINE, 31.5 / 63.0),
            off(P_O1_TRI),
            (P_O1_SAW, 1.0),
            off(P_O1_PULSE),
            off(P_O1_WIDTH),
            (P_O1_KEY, 1.0),
            off(P_O1_GLIDE),
            (P_O1_ON, 1.0),
            (P_O1_LEVEL, 1.0),
            off(P_O2_FREQ),
            (P_O2_FINE, 31.5 / 63.0),
            off(P_O2_TRI),
            (P_O2_SAW, 1.0),
            off(P_O2_PULSE),
            off(P_O2_WIDTH),
            (P_O2_KEY, 1.0),
            off(P_O2_GLIDE),
            off(P_O2_ON),
            (P_O2_LEVEL, 1.0),
            off(P_O2_BYPASS),
            off(P_XMOD),
            off(P_SYNC),
            off(P_SUB_ON),
            (P_SUB_LEVEL, 1.0),
            off(P_NOISE_ON),
            off(P_NOISE_TYPE),
            (P_NOISE_LEVEL, 1.0),
            (P_CUTOFF, 1.0),
            off(P_RESONANCE),
            off(P_STATE),
            off(P_BANDPASS),
            off(P_FILTER_KEY),
            (P_E1_AMOUNT, 0.5),
            off(P_E1_VEL),
            off(P_E1_DELAY),
            off(P_E1_ATTACK),
            (P_E1_DECAY, 1.0),
            (P_E1_SUSTAIN, 1.0),
            off(P_E1_RELEASE),
            (P_E2_AMOUNT, 1.0),
            off(P_E2_VEL),
            off(P_E2_DELAY),
            off(P_E2_ATTACK),
            (P_E2_DECAY, 1.0),
            (P_E2_SUSTAIN, 1.0),
            off(P_E2_RELEASE),
            off(P_ENV_ROUTE),
            off(P_E1_DEST),
            off(P_ENV_REPEAT),
            (P_L1_FREQ, 0.5),
            off(P_L1_SHAPE),
            (P_L1_AMOUNT, 0.5),
            off(P_L1_DEST),
            off(P_L1_SYNC),
            off(P_L1_DIV),
            off(P_L1_RESET),
            off(P_L1_SLEW),
            (P_L2_FREQ, 0.5),
            off(P_L2_SHAPE),
            (P_L2_AMOUNT, 0.5),
            off(P_L2_DEST),
            off(P_L2_SYNC),
            off(P_L2_DIV),
            off(P_L2_RESET),
            off(P_L2_SLEW),
            off(P_FX_ON),
            off(P_FX_TYPE),
            off(P_FX_MIX),
            (P_FX_TIME, 0.5),
            (P_FX_MISC, 0.5),
            off(P_FX_SYNC),
            off(P_FX_DIV),
            off(P_RV_ON),
            off(P_RV_MIX),
            off(P_OVERDRIVE),
            off(P_VINTAGE),
            (P_VOLUME, 1.0),
            (P_PAN, 0.5),
            off(P_UNISON),
            off(P_UNISON_VOICES),
            off(P_UNISON_DETUNE),
            off(P_KEY_MODE),
            off(P_RETRIGGER),
            off(P_GLIDE),
            off(P_GLIDE_MODE),
            off(P_SPLIT_1),
            off(P_SPLIT_2),
            (P_SPLIT_NOTE, 19.0 / 43.0),
            (P_BEND_UP, knob_for(2, 13)),
            (P_BEND_DOWN, knob_for(2, 25)),
            (P_TRANSPOSE, knob_for(2, 5)),
            (P_BPM, 120.0 / 250.0),
        ];
        // Every one of the sixteen modulation slots off.
        for slot in 0..MOD_SLOTS {
            let base = P_MOD + 3 * slot;
            panel.push(off(base));
            panel.push((base + 1, 0.5));
            panel.push(off(base + 2));
        }
        panel
    }

    /// The neutral panel with one control moved.
    pub(crate) fn with(setup: &[(usize, f32)], changes: &[(usize, f32)]) -> Vec<(usize, f32)> {
        let mut out = setup.to_vec();
        out.extend_from_slice(changes);
        out
    }

    /// A modulation slot, as three knob positions.
    fn slot(index: usize, source: usize, amount: f64, dest: usize) -> Vec<(usize, f32)> {
        let base = P_MOD + 3 * index;
        vec![
            (base, knob_for(source, MOD_SOURCES.len())),
            (base + 1, ((amount * 127.0 + 127.0) / 254.0) as f32),
            (base + 2, knob_for(dest, MOD_DESTS.len())),
        ]
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
        let mut s = Teo5::new();
        let mut left = [0.0f32; 64];
        let mut outs: [&mut [f32]; 1] = [&mut left];
        s.process(&[], &mut outs, &[note_on(60, 100, 0)]);
        assert!(left.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = fresh(0);
        let out = render(&mut s, &[note_on(60, 100, 0)], 40);
        assert!(rms(&out) > 1.0e-4, "a held note is silent");
    }

    #[test]
    fn silent_after_release() {
        let mut s = built(&neutral());
        let _ = render(&mut s, &[note_on(60, 100, 0)], 10);
        let tail = render(&mut s, &[note_off(60, 0)], 40);
        assert!(rms(&tail[tail.len() / 2..]) < 1.0e-6, "the note never stops");
    }

    #[test]
    fn output_is_finite_across_the_keyboard() {
        let mut s = fresh(0);
        for note in (12u8..=120).step_by(6) {
            let out = render(&mut s, &[note_on(note, 110, 0), note_off(note, 4_000)], 30);
            assert!(out.iter().all(|v| v.is_finite()), "note {note} produced a non-finite sample");
        }
    }

    #[test]
    fn cc120_kills_and_cc123_releases() {
        let mut s = built(&with(&neutral(), &[(P_E2_RELEASE, 0.9)]));
        let _ = render(&mut s, &[note_on(60, 100, 0)], 10);
        let out = render(&mut s, &[cc(123, 0, 0)], 4);
        assert!(rms(&out) > 1.0e-6, "all-notes-off cut the release short");

        let mut s = built(&with(&neutral(), &[(P_E2_RELEASE, 0.9)]));
        let _ = render(&mut s, &[note_on(60, 100, 0)], 10);
        let out = render(&mut s, &[cc(120, 0, 0)], 4);
        assert!(rms(&out[64..]) < 1.0e-9, "all-sound-off left the voice ringing");
    }

    #[test]
    fn sample_accurate_midi() {
        let mut s = built(&neutral());
        let out = render(&mut s, &[note_on(60, 100, 128)], 1);
        assert_eq!(peak(&out[..120]), 0.0, "the note started before its offset");
        assert!(peak(&out[128..]) > 0.0, "the note never started");
    }

    #[test]
    fn all_params_readable() {
        let s = Teo5::new();
        assert_eq!(s.parameter_count(), PARAM_COUNT);
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            let info = s.parameter_info(index).expect("every parameter has info");
            assert_eq!(info.name, *name);
        }
        assert!(s.parameter_info(PARAM_COUNT).is_none());
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

    // ── Through-zero FM ──

    /// The net number of waveform cycles in a sawtooth, counting a cycle that
    /// runs backwards as minus one.
    ///
    /// This is the measurement the through-zero claim needs and the only one
    /// that will do. A sawtooth is a straight line in its own phase, so the
    /// signal *is* the phase and unwrapping it gives the net advance over the
    /// window — which is the integral of the instantaneous frequency,
    /// including the stretches where that frequency is negative and the phase
    /// is running backwards. Counting zero crossings would not do: with the
    /// frequency swinging through zero the crossing rate is the mean of
    /// `|f|`, which climbs with the depth whatever the mean of `f` does.
    pub(crate) fn net_cycles(x: &[f32]) -> f64 {
        // A robust amplitude rather than the peak: the band-limiting
        // correction overshoots the rails by a few percent for a sample at a
        // time.
        let mut sorted: Vec<f64> = x.iter().map(|v| f64::from(v.abs())).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let scale = sorted[sorted.len() * 95 / 100].max(1.0e-12);

        // A wrap is a *jump* between the two ends of the ramp, and what tells
        // it from an ordinary traversal is how long it takes: the fastest the
        // phase ever moves here is a thirtieth of a cycle a sample, so
        // crossing the middle half of the ramp legitimately takes at least
        // seventeen samples and a wrap takes one — or, once the band-limiting
        // correction has spread it, two or three. Reading the wrap off a
        // single sample difference does not work, because that correction
        // pulls the sample before the wrap halfway down and the sample after
        // it halfway up, and neither step is then a whole cycle.
        const GAP: usize = 5;
        let mut net = 0.0f64;
        let mut last_end: Option<(usize, bool)> = None;
        for (i, v) in x.iter().enumerate() {
            let phase = ((f64::from(*v) / scale).clamp(-1.0, 1.0) + 1.0) * 0.5;
            let end = if phase < 0.25 {
                Some(false)
            } else if phase > 0.75 {
                Some(true)
            } else {
                None
            };
            let Some(high) = end else { continue };
            if let Some((at, was_high)) = last_end {
                if was_high != high && i - at <= GAP {
                    net += if was_high { 1.0 } else { -1.0 };
                }
            }
            last_end = Some((i, high));
        }
        net
    }

    #[test]
    fn through_zero_fm_holds_the_carriers_pitch() {
        // "X-Mod: Sets the amount of through zero fequency modulation from
        // Oscillator 2's triangle waveshape to Oscillator 1's frequency."
        //
        // Through zero is the claim and this is the measurement: the carrier
        // keeps its keyboard pitch as the depth rises, because the deviation
        // is symmetric about it and the phase runs backwards rather than
        // stopping when the instantaneous frequency goes negative.
        //
        // Oscillator 1 is off the keyboard at middle C and oscillator 2 is
        // on it two octaves below, so the modulator is slow enough that the
        // instantaneous frequency spends whole milliseconds negative at the
        // top of the knob.
        let setup = with(
            &neutral(),
            &[(P_O1_KEY, 0.0), (P_O2_ON, 0.0), (P_CUTOFF, 1.0), (P_E2_ATTACK, 0.0)],
        );
        let carrier = raw::note_hz(OSC_NO_KEY_NOTE);
        let mut measured = Vec::new();
        for depth in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let mut s = built(&with(&setup, &[(P_XMOD, depth)]));
            let out = render(&mut s, &[note_on(36, 100, 0)], 60);
            // Whole modulator cycles only, so that a partial swing at either
            // end of the window cannot bias the average.
            let modulator = raw::note_hz(36.0);
            let span = (out.len() as f64 / SR * modulator).floor() / modulator;
            let window = &out[..(span * SR) as usize];
            let hz = net_cycles(window) / span;
            measured.push(hz);
            assert!(
                (hz - carrier).abs() / carrier < 0.02,
                "at an X-Mod depth of {depth} the carrier's mean frequency is {hz:.1} Hz \
                 against the {carrier:.1} Hz it plays with the knob down - {:.1}% of drift",
                100.0 * (hz / carrier - 1.0)
            );
        }
        // ...and the knob is doing something. Measured on a triangle
        // carrier, which has almost nothing above its third harmonic to start
        // with, so what appears is the modulation's own sidebands rather than
        // the sawtooth's own spectrum being rearranged.
        let bright_at = |depth: f32| {
            let mut s = built(&with(
                &setup,
                &[(P_O1_SAW, 0.0), (P_O1_TRI, 1.0), (P_XMOD, depth)],
            ));
            let out = render(&mut s, &[note_on(36, 100, 0)], 60);
            brightness(&out[out.len() / 3..], SR)
        };
        let dry = bright_at(0.0);
        let wet = bright_at(1.0);
        assert!(
            wet > dry * 2.5,
            "the X-Mod knob does not change the timbre: {dry:.0} Hz to {wet:.0} Hz"
        );
        // The phase really does reverse: at the top of the knob the
        // instantaneous frequency is four times the carrier either way, so
        // some of the sawtooth's cycles run backwards.
        let mut s = built(&with(&setup, &[(P_XMOD, 1.0)]));
        let out = render(&mut s, &[note_on(36, 100, 0)], 60);
        let gate = f64::from(peak(&out)) * 0.5;
        let backwards = out
            .windows(2)
            .filter(|p| f64::from(p[1]) - f64::from(p[0]) > gate)
            .count();
        // The carrier makes 91 forward cycles in the window; at this depth
        // the instantaneous frequency is negative for a third of every
        // modulator cycle, so there are nearly as many backward ones.
        assert!(
            backwards > 40,
            "nothing ran backwards, so this is frequency modulation that stops at zero \
             rather than through-zero FM: {backwards} reversed edges"
        );
    }

    // ── The SEM filter ──

    /// The gain of a bare filter at one frequency, by driving it with a sine.
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

    #[test]
    fn the_filter_is_two_poles_and_does_not_self_oscillate() {
        // "The function of the TEO-5's 12dB/2-pole state variable filter..."
        // and "Note that the TEO-5's filter does not self-oscillate."
        let cutoff = 400.0;
        let mut f = Svf::new();
        let a = db(filter_gain(|x| f.process(x, cutoff, 0.7, 0.0, false, SR), 1_600.0));
        let mut f = Svf::new();
        let b = db(filter_gain(|x| f.process(x, cutoff, 0.7, 0.0, false, SR), 3_200.0));
        assert!(
            (-14.0..-10.0).contains(&(b - a)),
            "the low pass rolls off at {:.1} dB an octave, not 12",
            a - b
        );
        // The high-pass end of the morph has the same slope the other way.
        let mut f = Svf::new();
        let a = db(filter_gain(|x| f.process(x, cutoff, 0.7, 1.0, false, SR), 100.0));
        let mut f = Svf::new();
        let b = db(filter_gain(|x| f.process(x, cutoff, 0.7, 1.0, false, SR), 50.0));
        assert!(
            (-14.0..-10.0).contains(&(b - a)),
            "the high pass rolls off at {:.1} dB an octave, not 12",
            a - b
        );

        // Resonance lifts the corner, and stops well short of a tone: with no
        // input at all the top of the travel produces nothing.
        let peak_at = |resonance: f64| {
            let mut f = Svf::new();
            db(filter_gain(|x| f.process(x, cutoff, resonance_q(resonance), 0.0, false, SR), cutoff))
        };
        let flat = peak_at(0.0);
        let full = peak_at(1.0);
        assert!(
            full - flat > 12.0 && full - flat < 24.0,
            "the resonance travel is worth {:.1} dB at the corner",
            full - flat
        );
        let mut f = Svf::new();
        f.s1 = 0.5;
        f.s2 = 0.5;
        let mut tail = 0.0f64;
        for n in 0..40_000 {
            let v = f.process(0.0, cutoff, resonance_q(1.0), 0.0, false, SR);
            if n > 20_000 {
                tail = tail.max(v.abs());
            }
        }
        assert!(
            tail < 1.0e-6,
            "the filter self-oscillates at the top of the resonance travel: {tail:.2e}"
        );
    }

    /// A sawtooth at note 36 through the filter, with the cutoff parked on
    /// its eighth harmonic, measured at three of its partials: one three
    /// octaves under the corner, one exactly on it, and one two and a third
    /// octaves above.
    ///
    /// Partials rather than bands, because a sawtooth is a comb and the three
    /// numbers are then the filter's response at three frequencies rather
    /// than three integrals of it against a sloping source.
    pub(crate) fn morph_sweep(bandpass: f32) -> Vec<(f64, f64, f64)> {
        const NOTE: u8 = 36;
        const CORNER_HARMONIC: f64 = 8.0;
        let root = raw::note_hz(f64::from(NOTE));
        let corner = root * CORNER_HARMONIC;
        // 0-1023, in eighths of a semitone above the closed end.
        let cutoff = ((12.0 * (corner / 440.0).log2() + 69.0 - raw::CUTOFF_LOW_NOTE) * 8.0
            / 1023.0) as f32;
        let setup = with(
            &neutral(),
            &[
                (P_CUTOFF, cutoff),
                (P_RESONANCE, 0.7),
                (P_BANDPASS, bandpass),
                (P_E2_ATTACK, 0.0),
            ],
        );
        (0..=8)
            .map(|step| {
                let mut s = built(&with(&setup, &[(P_STATE, step as f32 / 8.0)]));
                let out = render(&mut s, &[note_on(NOTE, 100, 0)], 60);
                let tail = &out[out.len() / 2..];
                (
                    harmonic(tail, root, SR),
                    harmonic(tail, corner, SR),
                    harmonic(tail, root * 40.0, SR),
                )
            })
            .collect()
    }

    #[test]
    fn the_state_control_inverts_the_spectral_tilt_through_a_notch() {
        // "State: Smoothly mixes between low pass, notch, and high pass
        // filter states."
        //
        // This is the test no other filter in the rack passes, and it is the
        // reason this instrument exists. One sweep of one knob has to take
        // the spectrum from low-passed to high-passed — an inversion, not a
        // change of degree — and the partial sitting on the corner has to
        // vanish half way through, because half of the low-pass output plus
        // half of the high-pass output is `s² + w0²` over the same
        // denominator, which is a notch.
        let sweep = morph_sweep(0.0);
        let tilt: Vec<f64> = sweep.iter().map(|(low, _, high)| db(high / low)).collect();
        assert!(
            tilt[0] < -25.0,
            "at the bottom of the state knob the filter is not low-passing: tilt {:.1} dB",
            tilt[0]
        );
        assert!(
            tilt[8] > 0.0,
            "at the top of the state knob the filter is not high-passing: tilt {:.1} dB",
            tilt[8]
        );
        assert!(
            tilt[8] - tilt[0] > 30.0,
            "the state knob is worth only {:.1} dB of tilt across its whole travel",
            tilt[8] - tilt[0]
        );
        // The bass partial, three octaves under the corner, falls all the way
        // across the travel: it is in the low pass's passband and in the high
        // pass's stopband and there is nothing in between to complicate it.
        //
        // The treble partial is *not* asserted to be monotonic, and that is
        // the crossfade being right rather than the measurement being loose.
        // A mix of `(1-t)` low pass and `t` high pass nulls wherever
        // `(1-t)w0^2 = t*w^2`, so the notch does not sit still at the corner —
        // it sweeps down from infinity as the knob leaves the low-pass end,
        // crosses the corner at the centre and carries on down. Anything
        // above the corner is therefore dipped once on the way past.
        let bass: Vec<f64> = sweep.iter().map(|(low, _, _)| *low).collect();
        for pair in bass.windows(2) {
            assert!(
                pair[1] < pair[0],
                "the bass does not fall all the way across the state travel: {bass:?}"
            );
        }
        // The notch: the partial parked on the corner is buried at the centre
        // of the travel and present at both ends.
        let corner: Vec<f64> = sweep.iter().map(|(_, at, _)| *at).collect();
        let middle = corner[4];
        let ends = corner[0].min(corner[8]);
        assert!(
            db(ends / middle) > 20.0,
            "the middle of the state knob is not a notch: the partial on the corner is \
             {:.1} dB down there against the ends",
            db(ends / middle)
        );
        let (deepest, _) = corner
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        assert_eq!(deepest, 4, "the notch is at step {deepest} of the travel rather than the middle");
    }

    #[test]
    fn band_pass_mode_peaks_where_the_notch_dipped() {
        // "If the band pass state is set active in the Program menu, it
        // replaces notch at the center position of the state knob."
        let notch = morph_sweep(0.0);
        let band = morph_sweep(1.0);
        let notch_middle = notch[4].1;
        let band_middle = band[4].1;
        assert!(
            db(band_middle / notch_middle) > 20.0,
            "band pass mode did not replace the notch: the partial on the corner is only \
             {:.1} dB louder there",
            db(band_middle / notch_middle)
        );
        // A band pass is band-limited on both sides, which the notch is not.
        let (low, _, high) = band[4];
        let (notch_low, _, notch_high) = notch[4];
        assert!(
            low < notch_low * 0.5 && high < notch_high * 0.5,
            "band pass mode kept the ends of the spectrum a notch throws away: \
             {low:.5} against {notch_low:.5} below, {high:.5} against {notch_high:.5} above"
        );
        // The two endpoints are the same low pass and high pass either way.
        for step in [0usize, 8] {
            let ratio = db(band[step].1 / notch[step].1);
            assert!(
                ratio.abs() < 3.0,
                "the band-pass switch moved the ends of the morph as well as its centre: \
                 {ratio:.1} dB at step {step}"
            );
        }
    }

    // ── Rate independence ──
    //
    // Everything below is measured at four sample rates and asserted to
    // agree, because the defect this guards against is the one that only
    // shows up on someone else's audio device: a coefficient with 44100 baked
    // into it sounds right on the machine it was voiced on and wrong
    // everywhere else.

    const RATES: [f64; 4] = [22_050.0, 44_100.0, 48_000.0, 96_000.0];

    fn render_at(synth: &mut Teo5, events: &[MidiEvent], blocks: usize) -> Vec<f32> {
        render(synth, events, blocks)
    }

    #[test]
    fn the_pitch_is_the_same_at_every_sample_rate() {
        for note in [36u8, 48, 60, 72] {
            let reference = {
                let mut s = at_rate(&neutral(), 44_100.0);
                let out = render_at(&mut s, &[note_on(note, 100, 0)], 60);
                crossings_per_second(&out, 44_100.0)
            };
            for rate in RATES {
                let mut s = at_rate(&neutral(), rate);
                let blocks = (60.0 * rate / 44_100.0) as usize;
                let out = render_at(&mut s, &[note_on(note, 100, 0)], blocks);
                let measured = crossings_per_second(&out, rate);
                let error = (measured - reference).abs() / reference;
                assert!(
                    error < 0.02,
                    "note {note} at {rate} Hz sounds {measured:.1} crossings a second \
                     against {reference:.1} at 44100 - {:.1}% out",
                    error * 100.0
                );
            }
        }
    }

    #[test]
    fn the_lfo_runs_at_the_rate_the_knob_says_at_every_sample_rate() {
        // LFO 1 on to the amplifier by way of Voice Volume, square wave, so
        // the modulation is a gate whose rate can be counted.
        let mut setup = with(
            &neutral(),
            &[
                (P_L1_SHAPE, knob_for(3, LFO_SHAPES.len())),
                (P_L1_FREQ, 0.62),
                (P_L1_AMOUNT, 1.0),
                (P_L1_DEST, knob_for(44, MOD_DESTS.len())),
                (P_VOLUME, 0.0),
            ],
        );
        setup.push((P_L1_RESET, 1.0));
        let expected = raw::lfo_hz(0.62 * 255.0);
        for rate in RATES {
            let mut s = at_rate(&setup, rate);
            let blocks = (1.2 * rate / BLOCK as f64) as usize;
            let out = render_at(&mut s, &[note_on(60, 100, 0)], blocks);
            // Count the gate's openings: the envelope of the output, in
            // windows of a fixed length of *time* so that the measurement is
            // the same measurement at every rate.
            let window = (rate / 689.0) as usize;
            let windows: Vec<f64> = out.chunks(window).map(rms).collect();
            let gate = windows.iter().copied().fold(0.0f64, f64::max) * 0.3;
            let mut cycles = 0u32;
            let mut open = false;
            for w in &windows {
                if *w > gate && !open {
                    open = true;
                    cycles += 1;
                } else if *w < gate * 0.5 {
                    open = false;
                }
            }
            let seconds = out.len() as f64 / rate;
            let measured = f64::from(cycles) / seconds;
            assert!(
                (measured - expected).abs() / expected < 0.15,
                "the LFO runs at {measured:.2} Hz at {rate} Hz against the {expected:.2} Hz \
                 the knob asks for"
            );
        }
    }

    #[test]
    fn the_envelope_takes_its_time_at_every_sample_rate() {
        let setup = with(&neutral(), &[(P_E2_ATTACK, 0.5), (P_E2_SUSTAIN, 1.0)]);
        let expected = raw::env_seconds(0.5 * 255.0);
        for rate in RATES {
            let mut s = at_rate(&setup, rate);
            // Three seconds of audio at every rate, which is three times the
            // segment being measured.
            let blocks = (3.0 * rate / BLOCK as f64) as usize;
            let out = render_at(&mut s, &[note_on(60, 100, 0)], blocks);
            let top = peak(&out);
            let reached = out
                .iter()
                .position(|v| v.abs() >= top * 0.95)
                .map_or(f64::MAX, |i| i as f64 / rate);
            assert!(
                (reached - expected).abs() / expected < 0.2,
                "a {expected:.2} s attack takes {reached:.2} s at {rate} Hz"
            );
        }
    }

    // ── The oscillators ──

    /// The share of the output at each of the first `count` harmonics of
    /// `hz`, relative to the first.
    fn harmonics(x: &[f32], hz: f64, sr: f64, count: usize) -> Vec<f64> {
        let first = harmonic(x, hz, sr).max(1.0e-30);
        (1..=count).map(|n| harmonic(x, hz * n as f64, sr) / first).collect()
    }

    fn shape_harmonics(setup: &[(usize, f32)], note: u8) -> Vec<f64> {
        let mut s = built(setup);
        let out = render(&mut s, &[note_on(note, 100, 0)], 40);
        let hz = raw::note_hz(f64::from(note));
        harmonics(&out[out.len() / 2..], hz, SR, 6)
    }

    #[test]
    fn the_three_waveshapes_are_the_shapes_they_name_and_mix_independently() {
        // "These buttons toggle on and off the waveshapes generated by the
        // oscillator... All waveshapes can be simultaneously selected."
        let base = with(&neutral(), &[(P_O1_SAW, 0.0)]);
        let triangle = shape_harmonics(&with(&base, &[(P_O1_TRI, 1.0)]), 48);
        let sawtooth = shape_harmonics(&with(&base, &[(P_O1_SAW, 1.0)]), 48);
        let square = shape_harmonics(&with(&base, &[(P_O1_PULSE, 1.0)]), 48);

        // A triangle has odd harmonics falling as 1/n squared: the third is
        // a ninth of the first and the second is nothing at all.
        assert!(triangle[1] < 0.05, "the triangle has a second harmonic: {:.3}", triangle[1]);
        assert!(
            (triangle[2] - 1.0 / 9.0).abs() < 0.04,
            "the triangle's third harmonic is {:.3}, not a ninth",
            triangle[2]
        );
        // A sawtooth has every harmonic, falling as 1/n.
        assert!(
            (sawtooth[1] - 0.5).abs() < 0.08 && (sawtooth[2] - 1.0 / 3.0).abs() < 0.08,
            "the sawtooth's first harmonics are {sawtooth:?}"
        );
        // A square has odd harmonics falling as 1/n.
        assert!(square[1] < 0.05, "the square has a second harmonic: {:.3}", square[1]);
        assert!(
            (square[2] - 1.0 / 3.0).abs() < 0.08,
            "the square's third harmonic is {:.3}, not a third",
            square[2]
        );

        // All three at once is all three at once, not a switch.
        let mut s = built(&with(&base, &[(P_O1_TRI, 1.0), (P_O1_SAW, 1.0), (P_O1_PULSE, 1.0)]));
        let all = render(&mut s, &[note_on(48, 100, 0)], 40);
        let mut s = built(&with(&base, &[(P_O1_SAW, 1.0)]));
        let one = render(&mut s, &[note_on(48, 100, 0)], 40);
        assert!(
            rms(&all) > rms(&one) * 1.25,
            "three waveshapes together are no louder than one: {:.4} against {:.4}",
            rms(&all),
            rms(&one)
        );
        // ...and it is a sum rather than a switch: the second harmonic comes
        // only from the sawtooth and survives the other two being added.
        let mixed = shape_harmonics(
            &with(&base, &[(P_O1_TRI, 1.0), (P_O1_SAW, 1.0), (P_O1_PULSE, 1.0)]),
            48,
        );
        assert!(
            mixed[1] > 0.1 && (mixed[2] - square[2]).abs() > 0.02,
            "the three shapes do not sum: {mixed:?}"
        );
    }

    #[test]
    fn the_pulse_width_knob_opens_at_a_square_and_closes_to_silence() {
        // "Sets pulse width of the Oscillator 1 pulse wave from %50-%100 duty
        // cycle, with %50 at minimum pot setting. At %100 the pulse width
        // narrows to silence."
        let base = with(&neutral(), &[(P_O1_SAW, 0.0), (P_O1_PULSE, 1.0)]);
        let square = shape_harmonics(&with(&base, &[(P_O1_WIDTH, 0.0)]), 48);
        assert!(square[1] < 0.05, "the bottom of the width knob is not a square");
        let narrow = shape_harmonics(&with(&base, &[(P_O1_WIDTH, 0.55)]), 48);
        assert!(
            narrow[1] > 0.4,
            "a narrowed pulse has no even harmonics: {:.3}",
            narrow[1]
        );
        let mut s = built(&with(&base, &[(P_O1_WIDTH, 1.0)]));
        let out = render(&mut s, &[note_on(48, 100, 0)], 40);
        assert!(
            rms(&out) < 1.0e-6,
            "the top of the width knob is not silence: {:.6}",
            rms(&out)
        );
    }

    #[test]
    fn the_sub_is_an_octave_below_oscillator_1() {
        let setup = with(
            &neutral(),
            &[(P_O1_LEVEL, 0.0), (P_SUB_ON, 1.0), (P_SUB_LEVEL, 1.0)],
        );
        let mut s = built(&setup);
        let out = render(&mut s, &[note_on(60, 100, 0)], 60);
        let hz = fundamental_hz(&out[out.len() / 2..], SR, 60.0, 600.0);
        let expected = raw::note_hz(60.0) / 2.0;
        assert!(
            (hz - expected).abs() / expected < 0.03,
            "the sub sounds at {hz:.1} Hz, not the {expected:.1} Hz an octave below"
        );
    }

    #[test]
    fn sync_locks_oscillator_1_to_oscillator_2() {
        // "Osc 2's wave cycle forces Oscillator 1's waveform to reset to its
        // zero phase on each cycle of Oscillator 2."
        let setup = with(
            &neutral(),
            &[(P_SYNC, 1.0), (P_O1_FREQ, 17.0 / 63.0), (P_O2_ON, 0.0)],
        );
        let master = raw::note_hz(48.0);
        // The slave's own pitch is seventeen semitones up — 2.67 times the
        // master, deliberately nowhere near a whole number — so with sync off
        // it has no energy whatever at the master's fundamental or its
        // octave.
        // With sync on the waveform repeats at the master's period and every
        // one of the master's harmonics appears.
        let at = |sync: f32| {
            let mut s = built(&with(&setup, &[(P_SYNC, sync)]));
            let out = render(&mut s, &[note_on(48, 100, 0)], 60);
            let tail = out[out.len() / 2..].to_vec();
            let reference = harmonic(&tail, raw::note_hz(48.0 + 17.0), SR).max(1.0e-30);
            (
                harmonic(&tail, master, SR) / reference,
                harmonic(&tail, master * 2.0, SR) / reference,
            )
        };
        let (free_first, free_second) = at(0.0);
        let (sync_first, sync_second) = at(1.0);
        assert!(
            free_first < 0.02 && free_second < 0.02,
            "the free-running slave already has the master's harmonics: \
             {free_first:.3} {free_second:.3}"
        );
        assert!(
            sync_first > 0.1 && sync_second > 0.1,
            "sync did not lock the slave to the master's period: \
             {sync_first:.3} {sync_second:.3}"
        );

        // Moving the slave's own frequency moves the timbre without moving
        // the pitch, which is what makes a sync sweep a sweep.
        let bright = |semitones: f32| {
            let mut s = built(&with(&setup, &[(P_O1_FREQ, semitones / 63.0)]));
            let out = render(&mut s, &[note_on(48, 100, 0)], 60);
            brightness(&out[out.len() / 2..], SR)
        };
        let low = bright(7.0);
        let high = bright(31.0);
        assert!(
            high > low * 1.6,
            "sweeping the synced oscillator did not brighten it: {low:.0} Hz to {high:.0} Hz"
        );
    }

    #[test]
    fn one_out_of_range_byte_in_the_factory_bank_loads_as_a_switch() {
        // Bank 8 program 4, "Phat Boi", stores 14 in `lfo2_sync_on`, a field
        // the NRPN table prints as 0-1. It has to load as "on" rather than as
        // an index into a two-entry list.
        let index = 8 * PROGRAMS_PER_BANK + 4;
        assert_eq!(program_name(index), "Phat Boi", "the bank moved under this test");
        let panel = params_for_program(program_knobs(index).0, program_knobs(index).1);
        assert_eq!(selector(panel[P_L2_SYNC], 2), 1, "the stray byte did not clamp to on");
        let mut s = fresh(index);
        let out = render(&mut s, &[note_on(48, 100, 0)], 30);
        assert!(out.iter().all(|v| v.is_finite()) && rms(&out) > 1.0e-5);
    }
    // ── The modulation matrix ──

    /// A patch with something switched on in every section, so that every one
    /// of the sixty-five destinations has something to move.
    fn rich() -> Vec<(usize, f32)> {
        let mut panel = with(
            &neutral(),
            &[
                (P_O1_PULSE, 1.0),
                (P_O1_WIDTH, 0.3),
                (P_O2_ON, 1.0),
                (P_O2_PULSE, 1.0),
                (P_O2_WIDTH, 0.3),
                (P_O1_LEVEL, 0.6),
                (P_O2_LEVEL, 0.6),
                (P_SUB_ON, 1.0),
                (P_SUB_LEVEL, 0.5),
                (P_NOISE_ON, 1.0),
                (P_NOISE_LEVEL, 0.2),
                (P_XMOD, 0.1),
                (P_CUTOFF, 0.6),
                (P_RESONANCE, 0.4),
                (P_STATE, 0.2),
                (P_E1_AMOUNT, 0.75),
                (P_E2_AMOUNT, 0.6),
                (P_E1_DECAY, 0.4),
                (P_E1_SUSTAIN, 0.5),
                (P_E1_RELEASE, 0.3),
                (P_E2_ATTACK, 0.1),
                (P_E2_DECAY, 0.5),
                (P_E2_SUSTAIN, 0.6),
                (P_E2_RELEASE, 0.3),
                (P_L1_AMOUNT, 0.7),
                (P_L1_DEST, knob_for(15, MOD_DESTS.len())),
                (P_L1_FREQ, 0.45),
                (P_L2_AMOUNT, 0.7),
                (P_L2_DEST, knob_for(7, MOD_DESTS.len())),
                (P_L2_FREQ, 0.4),
                (P_FX_ON, 1.0),
                (P_FX_TYPE, knob_for(fx::CHORUS, FX_TYPES.len())),
                (P_FX_MIX, 0.5),
                (P_OVERDRIVE, 0.2),
                (P_VINTAGE, 0.3),
                (P_UNISON, 1.0),
                (P_UNISON_VOICES, knob_for(3, UNISON_VOICES.len())),
                (P_UNISON_DETUNE, knob_for(3, UNISON_DETUNE_LABELS.len())),
                (P_VOLUME, 0.8),
            ],
        );
        // All sixteen slots carry a routing of their own, so that the
        // sixteen *Mod n Amount* destinations have something to open and the
        // sixteen source selectors have somewhere to send it. A test that
        // wants a slot to itself writes over one.
        const FILLED: [usize; MOD_SLOTS] =
            [3, 15, 7, 1, 44, 45, 16, 17, 13, 10, 12, 26, 27, 14, 2, 46];
        for (index, dest) in FILLED.iter().enumerate() {
            panel.extend(slot(index, src::LFO2, 0.06, *dest));
        }
        panel
    }

    /// The events a source needs before it has anything to say.
    fn stimulus(source: usize) -> Vec<MidiEvent> {
        let mut events = vec![note_on(48, 100, 0)];
        match source {
            src::BEND => events.push(MidiEvent {
                sample_offset: 8,
                status: 0xE0,
                data1: 0x7F,
                data2: 0x7F,
            }),
            src::WHEEL => events.push(cc(1, 127, 8)),
            src::PRESSURE => events.push(aftertouch(120, 8)),
            src::BREATH => events.push(cc(2, 127, 8)),
            src::FOOT => events.push(cc(4, 127, 8)),
            src::EXPRESSION => events.push(cc(11, 127, 8)),
            _ => {}
        }
        events
    }

    #[test]
    fn every_modulation_source_reaches_the_engine() {
        // Nineteen sources and *Off*, the manual's Appendix A. Each one is
        // pointed at the cutoff at full depth and compared against the same
        // patch with the slot switched off; five of them arrive from outside
        // the instrument and are given something to arrive with.
        let base = rich();
        for (source, source_name) in MOD_SOURCES.iter().enumerate().skip(1) {
            let events = stimulus(source);
            let mut off = built(&with(&base, &slot(0, src::OFF, 1.0, 15)));
            let dry = render(&mut off, &events, 40);
            let mut on = built(&with(&base, &slot(0, source, 1.0, 15)));
            let wet = render(&mut on, &events, 40);
            assert!(
                wet.iter().all(|v| v.is_finite()),
                "{source_name} produced a non-finite sample"
            );
            let change = dry
                .iter()
                .zip(&wet)
                .map(|(a, b)| f64::from(*a - *b).abs())
                .fold(0.0f64, f64::max);
            assert!(
                change > 1.0e-4,
                "{source_name} reaches nothing: the render is unchanged to {change:.2e}"
            );
        }
    }

    #[test]
    fn every_modulation_destination_is_accepted_and_the_rendered_ones_move() {
        // Sixty-five destinations, the manual's Appendix B. Every one has to
        // load, render and stay finite; the five that address the reverb this
        // build stores rather than renders have to leave the output alone,
        // because "accepted and applied to nothing" is the promise; and every
        // other one has to change the sound.
        let base = rich();
        let events = stimulus(src::DC);
        // Held and then released, because two of the sixty-five destinations
        // are release times and a held note never reaches them.
        let sound = |setup: &[(usize, f32)]| {
            let mut s = built(setup);
            let mut out = render(&mut s, &events, 24);
            out.extend_from_slice(&render(&mut s, &[note_off(48, 0)], 24));
            out
        };
        let dry = sound(&with(&base, &slot(0, src::OFF, 0.5, 0)));
        let mut inert = Vec::new();
        for (dest, dest_name) in MOD_DESTS.iter().enumerate().skip(1) {
            let wet = sound(&with(&base, &slot(0, src::DC, 0.5, dest)));
            assert!(
                wet.iter().all(|v| v.is_finite()),
                "{dest_name} produced a non-finite sample"
            );
            let change = dry
                .iter()
                .zip(&wet)
                .map(|(a, b)| f64::from(*a - *b).abs())
                .fold(0.0f64, f64::max);
            // The one destination that cannot be shown to move from here is
            // the amount of the slot doing the moving: a slot pointed at its
            // own amount changes nothing else, and `a_slot_can_modulate_
            // another_slots_amount` covers the mechanism.
            if dest == 49 {
                continue;
            }
            let stored = (21..=25).contains(&dest);
            if stored {
                assert_eq!(
                    change, 0.0,
                    "{dest_name} is documented as stored and not rendered, but it changed \
                     the output"
                );
            } else if change <= 1.0e-5 {
                inert.push(*dest_name);
            }
        }
        assert!(inert.is_empty(), "these destinations reached nothing: {inert:?}");
    }

    #[test]
    fn a_slot_can_modulate_another_slots_amount() {
        // Sixteen of the sixty-five destinations are the matrix's own amount
        // knobs, and the factory bank uses fifteen of them.
        // Slot 2's own amount is zero, so it does nothing at all until slot 1
        // opens it — which is the shape of the bank's commonest gesture, the
        // mod wheel on an LFO amount that the program leaves at zero.
        let base = with(&rich(), &slot(1, src::DC, 0.0, 15));
        let mut without = built(&with(&base, &slot(0, src::OFF, 0.0, 0)));
        let quiet = render(&mut without, &[note_on(48, 100, 0)], 40);
        let mut with_amount = built(&with(&base, &slot(0, src::DC, 0.6, 50)));
        let loud = render(&mut with_amount, &[note_on(48, 100, 0)], 40);
        let change = quiet
            .iter()
            .zip(&loud)
            .map(|(a, b)| f64::from(*a - *b).abs())
            .fold(0.0f64, f64::max);
        assert!(change > 1.0e-4, "opening a slot's amount from another slot did nothing");
    }

    // ── The two LFOs ──

    #[test]
    fn the_lfo_shapes_have_the_polarity_the_manual_gives() {
        // "The triangle wave is bipolar... The square, sawtooth, reverse
        // sawtooth, and sample & hold waves generate only positive values. In
        // the case of the square wave, this makes it possible to generate
        // trills."
        let base = with(
            &neutral(),
            &[
                (P_L1_FREQ, 0.45),
                (P_L1_AMOUNT, 1.0),
                (P_L1_DEST, knob_for(1, MOD_DESTS.len())),
                (P_CUTOFF, 1.0),
            ],
        );
        let swing = |shape: usize| {
            let mut s = built(&with(&base, &[(P_L1_SHAPE, knob_for(shape, LFO_SHAPES.len()))]));
            let out = render(&mut s, &[note_on(60, 100, 0)], 120);
            // The pitch in each eighth of the render, as crossings a second.
            let window = out.len() / 24;
            let rates: Vec<f64> = out
                .chunks(window)
                .map(|c| crossings_per_second(c, SR))
                .collect();
            let rest = raw::note_hz(60.0) * 2.0;
            (
                rates.iter().copied().fold(f64::MAX, f64::min) / rest,
                rates.iter().copied().fold(0.0f64, f64::max) / rest,
            )
        };
        let (tri_low, tri_high) = swing(0);
        assert!(
            tri_low < 0.8 && tri_high > 1.2,
            "the triangle is not bipolar: it swings {tri_low:.2} to {tri_high:.2} of the note"
        );
        for shape in [1usize, 2, 3] {
            let (low, high) = swing(shape);
            assert!(
                low > 0.9 && high > 1.3,
                "{} goes below the note, so it is not unipolar: {low:.2} to {high:.2}",
                LFO_SHAPES[shape]
            );
        }
    }

    #[test]
    fn lfo_two_is_per_voice_and_lfo_one_is_not() {
        // "LFO 1 is a Global LFO and is a single modulator that is applied to
        // all voices in a program equally... LFO 2 is a per-voice modulator
        // that is applied to each voice individually."
        //
        // Two notes struck half a modulation cycle apart, with note reset on
        // so that each voice's own LFO starts where its key did. Under the
        // global LFO the two voices are modulated together and the pair's
        // amplitude swings; under the per-voice one they are modulated in
        // opposition and the swing cancels.
        let rate = 0.55f32;
        let hz = raw::lfo_hz(f64::from(rate) * 255.0);
        let half = ((SR / hz * 0.5) / BLOCK as f64).round().max(1.0) as usize;
        let base = with(
            &neutral(),
            &[
                (P_L1_FREQ, rate),
                (P_L2_FREQ, rate),
                (P_L1_RESET, 1.0),
                (P_L2_RESET, 1.0),
                (P_E2_ATTACK, 0.0),
                (P_VOLUME, 0.5),
            ],
        );
        let swing = |which: usize| {
            let dest = knob_for(44, MOD_DESTS.len());
            let changes = if which == 1 {
                [(P_L1_AMOUNT, 0.7), (P_L1_DEST, dest), (P_L1_SHAPE, 0.0)]
            } else {
                [(P_L2_AMOUNT, 0.7), (P_L2_DEST, dest), (P_L2_SHAPE, 0.0)]
            };
            let mut s = built(&with(&base, &changes));
            let _ = render(&mut s, &[note_on(48, 100, 0)], half);
            let out = render(&mut s, &[note_on(60, 100, 0)], 200);
            let settled = &out[out.len() / 4..];
            let windows: Vec<f64> = settled.chunks(1_024).map(rms).collect();
            let loud = windows.iter().copied().fold(0.0f64, f64::max);
            let quiet = windows.iter().copied().fold(f64::MAX, f64::min);
            loud / quiet.max(1.0e-12)
        };
        let global = swing(1);
        let per_voice = swing(2);
        assert!(
            global > per_voice * 1.5,
            "the two LFOs behave the same on two notes struck apart: global {global:.2}, \
             per voice {per_voice:.2} - LFO 2 is not per voice"
        );
    }

    #[test]
    fn the_lfo_slew_rounds_a_square_off() {
        // "Adding slew to an LFO smooths out LFO waveshapes by altering the
        // speed at which voltage levels change. Adding slew can change a
        // square wave LFO to a triangle or sine shape."
        let base = with(
            &neutral(),
            &[
                (P_L1_SHAPE, knob_for(3, LFO_SHAPES.len())),
                (P_L1_FREQ, 0.6),
                (P_L1_AMOUNT, 1.0),
                (P_L1_DEST, knob_for(44, MOD_DESTS.len())),
                (P_VOLUME, 0.0),
                (P_L1_RESET, 1.0),
            ],
        );
        let edge = |slew: f32| {
            let mut s = built(&with(&base, &[(P_L1_SLEW, slew)]));
            let out = render(&mut s, &[note_on(72, 100, 0)], 80);
            // Windows long enough to hold five cycles of the note, so that
            // what moves between them is the modulation rather than the
            // waveform.
            let envelope: Vec<f64> = out.chunks(256).map(rms).collect();
            let top = envelope.iter().copied().fold(0.0f64, f64::max);
            // How many windows sit in the middle third of the swing: a square
            // passes through it in one and a rounded one lingers.
            envelope.iter().filter(|v| **v > top * 0.25 && **v < top * 0.75).count()
        };
        let square = edge(0.0);
        let rounded = edge(1.0);
        assert!(
            rounded > square * 2,
            "the slew knob did not round the square off: {square} windows on the edge \
             against {rounded}"
        );
    }

    #[test]
    fn clock_sync_puts_the_lfo_on_the_beat() {
        // The division table is judgment - see LFO_DIVISIONS - but the tempo
        // arithmetic is not: a synced LFO has to run at the tempo the panel
        // says divided by the division it names, at any tempo.
        for (bpm, division) in [(120.0f32, 7usize), (90.0, 10)] {
            let setup = with(
                &neutral(),
                &[
                    (P_L1_SYNC, 1.0),
                    (P_L1_DIV, knob_for(division, LFO_DIVISIONS.len())),
                    (P_BPM, bpm / 250.0),
                    (P_L1_SHAPE, knob_for(3, LFO_SHAPES.len())),
                    (P_L1_AMOUNT, 1.0),
                    (P_L1_DEST, knob_for(44, MOD_DESTS.len())),
                    (P_VOLUME, 0.0),
                    (P_L1_RESET, 1.0),
                ],
            );
            let mut s = built(&setup);
            let out = render(&mut s, &[note_on(60, 100, 0)], 400);
            let envelope: Vec<f64> = out.chunks(64).map(rms).collect();
            let gate = envelope.iter().copied().fold(0.0f64, f64::max) * 0.3;
            let mut cycles = 0u32;
            let mut open = false;
            for v in &envelope {
                if *v > gate && !open {
                    open = true;
                    cycles += 1;
                } else if *v < gate * 0.5 {
                    open = false;
                }
            }
            let seconds = out.len() as f64 / SR;
            let measured = f64::from(cycles) / seconds;
            let expected = 1.0 / (LFO_SYNC_BEATS[division] * 60.0 / f64::from(bpm));
            assert!(
                (measured - expected).abs() / expected < 0.15,
                "at {bpm} bpm on {} the LFO runs at {measured:.2} Hz rather than {expected:.2}",
                LFO_DIVISIONS[division]
            );
        }
    }

    // ── Voices, unison and the keyboard ──

    fn sounding(s: &Teo5) -> Vec<u8> {
        let mut notes: Vec<u8> = s.voices.iter().filter(|v| v.gate).map(|v| v.note).collect();
        notes.sort_unstable();
        notes
    }

    #[test]
    fn five_voices_and_the_sixth_note_steals_the_oldest() {
        let mut s = built(&neutral());
        let events: Vec<MidiEvent> =
            [60u8, 62, 64, 65, 67].iter().map(|&n| note_on(n, 100, 0)).collect();
        let _ = render(&mut s, &events, 2);
        assert_eq!(sounding(&s), vec![60, 62, 64, 65, 67], "five keys do not fill five voices");
        let _ = render(&mut s, &[note_on(69, 100, 0)], 2);
        assert_eq!(sounding(&s), vec![62, 64, 65, 67, 69], "the sixth note did not steal the oldest");
    }

    #[test]
    fn a_released_voice_is_taken_before_a_held_one() {
        let mut s = built(&with(&neutral(), &[(P_E2_RELEASE, 0.6)]));
        let events: Vec<MidiEvent> =
            [60u8, 62, 64, 65, 67].iter().map(|&n| note_on(n, 100, 0)).collect();
        let _ = render(&mut s, &events, 2);
        let _ = render(&mut s, &[note_off(62, 0)], 2);
        let _ = render(&mut s, &[note_on(69, 100, 0)], 2);
        assert_eq!(
            sounding(&s),
            vec![60, 64, 65, 67, 69],
            "the new note stole a held key rather than the ringing one"
        );
    }

    #[test]
    fn unison_stacks_the_number_of_voices_it_names() {
        for (count, label) in UNISON_VOICES.iter().enumerate() {
            let mut s = built(&with(
                &neutral(),
                &[(P_UNISON, 1.0), (P_UNISON_VOICES, knob_for(count, UNISON_VOICES.len()))],
            ));
            let _ = render(&mut s, &[note_on(60, 100, 0)], 2);
            let expected = (count + 1).min(VOICES);
            assert_eq!(
                s.voices.iter().filter(|v| v.gate).count(),
                expected,
                "{label} stacked the wrong number of voices"
            );
        }
    }

    #[test]
    fn a_unison_stack_is_louder_than_one_voice() {
        let base = with(&neutral(), &[(P_UNISON_DETUNE, knob_for(3, 8))]);
        let mut one = built(&base);
        let single = render(&mut one, &[note_on(48, 100, 0)], 60);
        let mut five = built(&with(
            &base,
            &[(P_UNISON, 1.0), (P_UNISON_VOICES, knob_for(4, UNISON_VOICES.len()))],
        ));
        let stack = render(&mut five, &[note_on(48, 100, 0)], 60);
        let ratio = rms(&stack) / rms(&single);
        // Five uncorrelated sources sum to root five, which is 2.24. The
        // phases are hashed rather than spread evenly for exactly this
        // reason - see `start_phase`.
        assert!(
            (1.8..3.0).contains(&ratio),
            "a five-voice stack is {ratio:.2} times one voice rather than about root five"
        );
    }

    #[test]
    fn chord_memory_arrives_with_the_program() {
        // Bank 4 program 6, "Bouncy Min9", stores [0, 3, 7, 10, 14]: root,
        // minor third, fifth, minor seventh and ninth. One key has to play
        // all five.
        let index = 3 * PROGRAMS_PER_BANK + 5;
        assert_eq!(program_name(index), "Bouncy Min9", "the bank moved under this test");
        assert_eq!(program_chord(index), &[0, 3, 7, 10, 14]);
        let mut s = fresh(index);
        let _ = render(&mut s, &[note_on(48, 100, 0)], 2);
        let mut notes: Vec<u8> = s.voices.iter().filter(|v| v.gate).map(|v| v.note).collect();
        notes.sort_unstable();
        assert_eq!(
            notes,
            vec![48, 51, 55, 58, 62],
            "one key did not play the stored minor ninth"
        );
        // ...and they really are five different pitches in the audio.
        let out = render(&mut s, &[], 60);
        for note in [48u8, 51, 55, 58, 62] {
            let hz = raw::note_hz(f64::from(note));
            assert!(
                harmonic(&out[out.len() / 2..], hz, SR) > 1.0e-5,
                "nothing at {hz:.1} Hz, so the chord is not sounding"
            );
        }
    }

    #[test]
    fn chord_memory_is_captured_and_cleared_from_the_keyboard() {
        // "Hold down a chord on the keyboard (5 notes maximum). Press the
        // unison switch." And to clear it: "Turn off Unison. Hold down a
        // single note. Press the unison button."
        let mut s = built(&neutral());
        assert!(s.chord_memory().is_empty(), "an empty program came with a chord");
        let held = [note_on(52, 100, 0), note_on(55, 100, 4), note_on(59, 100, 8)];
        let _ = render(&mut s, &held, 2);
        s.set_parameter(P_UNISON, 1.0);
        assert_eq!(s.chord_memory(), &[0, 3, 7], "the unison switch did not capture the chord");
        let _ = render(
            &mut s,
            &[note_off(52, 0), note_off(55, 1), note_off(59, 2)],
            2,
        );
        let _ = render(&mut s, &[note_on(60, 100, 0)], 2);
        let mut notes: Vec<u8> = s.voices.iter().filter(|v| v.gate).map(|v| v.note).collect();
        notes.sort_unstable();
        assert_eq!(notes, vec![60, 63, 67], "the captured chord did not transpose to the key");

        s.set_parameter(P_UNISON, 0.0);
        let _ = render(&mut s, &[note_off(60, 0)], 2);
        let _ = render(&mut s, &[note_on(72, 100, 0)], 2);
        s.set_parameter(P_UNISON, 1.0);
        assert!(s.chord_memory().is_empty(), "one held note did not clear the memory");
    }

    #[test]
    fn key_priority_applies_in_unison_and_not_in_poly() {
        let base = with(
            &neutral(),
            &[(P_UNISON, 1.0), (P_UNISON_VOICES, knob_for(2, UNISON_VOICES.len()))],
        );
        for (mode, winner) in [(0usize, 48u8), (1, 67), (2, 55)] {
            let mut s = built(&with(&base, &[(P_KEY_MODE, knob_for(mode, KEY_MODES.len()))]));
            let _ = render(
                &mut s,
                &[note_on(48, 100, 0), note_on(67, 100, 4), note_on(55, 100, 8)],
                2,
            );
            let notes: Vec<u8> = s.voices.iter().filter(|v| v.gate).map(|v| v.note).collect();
            assert!(
                notes.iter().all(|n| *n == winner),
                "{} priority sounded {notes:?} rather than {winner}",
                KEY_MODES[mode]
            );
        }
        // In poly every key sounds, whatever the priority says.
        let mut s = built(&with(&neutral(), &[(P_KEY_MODE, knob_for(0, KEY_MODES.len()))]));
        let _ = render(&mut s, &[note_on(48, 100, 0), note_on(67, 100, 4)], 2);
        assert_eq!(sounding(&s), vec![48, 67], "key priority leaked into polyphonic play");
    }

    #[test]
    fn the_vintage_knob_spreads_the_voices() {
        // "Turning up the vintage knob adds progressively more filter, pitch,
        // and envelope variation between voices."
        let base = with(
            &neutral(),
            &[(P_UNISON, 1.0), (P_UNISON_VOICES, knob_for(4, UNISON_VOICES.len()))],
        );
        let beating = |vintage: f32| {
            let mut s = built(&with(&base, &[(P_VINTAGE, vintage)]));
            let out = render(&mut s, &[note_on(48, 100, 0)], 200);
            let windows = window_rms(&out[out.len() / 4..]);
            let loud = windows.iter().copied().fold(0.0f64, f64::max);
            let quiet = windows.iter().copied().fold(f64::MAX, f64::min);
            loud / quiet.max(1.0e-12)
        };
        let still = beating(0.0);
        let vintage = beating(1.0);
        assert!(
            vintage > still * 1.3,
            "the vintage knob does not detune the stack: {still:.3} against {vintage:.3}"
        );
    }

    #[test]
    fn glide_slides_between_notes() {
        // In unison, which is where glide belongs on a poly synth: in
        // polyphony the second note lands on a second voice, which has no
        // pitch to glide from.
        let base = with(
            &neutral(),
            &[(P_GLIDE, 1.0), (P_O1_GLIDE, 0.6), (P_O2_GLIDE, 0.6), (P_UNISON, 1.0)],
        );
        let mut s = built(&base);
        let _ = render(&mut s, &[note_on(48, 100, 0)], 20);
        let out = render(&mut s, &[note_off(48, 0), note_on(60, 100, 1)], 40);
        // A tenth of a second in, the pitch is somewhere between the two.
        let window = &out[2_000..6_000];
        let hz = fundamental_hz(window, SR, 80.0, 400.0);
        let from = raw::note_hz(48.0);
        let to = raw::note_hz(60.0);
        assert!(
            hz > from * 1.05 && hz < to * 0.95,
            "the glide did not slide: {hz:.1} Hz is not between {from:.1} and {to:.1}"
        );
    }

    #[test]
    fn the_key_split_transposes_the_lower_half() {
        // "Low Split, lower half -1 octave" and "-2 octaves", with the split
        // point on the 44-note keyboard.
        let base = with(&neutral(), &[(P_SPLIT_NOTE, 19.0 / 43.0)]);
        let split_at = SPLIT_LOW_NOTE + 19.0;
        let pitch = |setup: &[(usize, f32)], note: u8| {
            let mut s = built(setup);
            let out = render(&mut s, &[note_on(note, 100, 0)], 60);
            fundamental_hz(&out[out.len() / 2..], SR, 20.0, 800.0)
        };
        let low = (split_at - 5.0) as u8;
        let high = (split_at + 5.0) as u8;
        for (switch, octaves) in [(P_SPLIT_1, 1.0f64), (P_SPLIT_2, 2.0)] {
            let setup = with(&base, &[(switch, 1.0)]);
            let below = pitch(&setup, low);
            let expected = raw::note_hz(f64::from(low) - 12.0 * octaves);
            assert!(
                (below - expected).abs() / expected < 0.04,
                "under the split the note sounds at {below:.1} Hz rather than {expected:.1}"
            );
            let above = pitch(&setup, high);
            let unshifted = raw::note_hz(f64::from(high));
            assert!(
                (above - unshifted).abs() / unshifted < 0.04,
                "the split moved a note above the split point: {above:.1} against {unshifted:.1}"
            );
        }
    }

    #[test]
    fn the_envelope_routings_hand_the_filter_from_one_envelope_to_the_other() {
        // "Env 1 Destinations: filter, aux. Env 2 Destinations: amp,
        // filter + amp, filter + gate."
        let base = with(
            &neutral(),
            &[
                (P_CUTOFF, 0.1),
                (P_E1_AMOUNT, 1.0),
                (P_E1_DECAY, 0.5),
                (P_E1_SUSTAIN, 0.0),
                (P_E2_AMOUNT, 1.0),
                (P_E2_SUSTAIN, 1.0),
            ],
        );
        let sweep = |route: usize| {
            let mut s = built(&with(
                &base,
                &[(P_ENV_ROUTE, knob_for(route, ENV_ROUTES.len()))],
            ));
            let out = render(&mut s, &[note_on(48, 100, 0)], 120);
            let early = brightness(&out[..4_096], SR);
            let late = brightness(&out[out.len() - 4_096..], SR);
            early / late.max(1.0)
        };
        // With envelope 1 on the filter and a sustain of zero, the note opens
        // bright and closes down.
        assert!(sweep(0) > 2.0, "envelope 1 does not sweep the filter in the first routing");
        // In the other two envelope 1 has gone to the matrix, so it no longer
        // does - envelope 2 has the filter and it sustains.
        assert!(
            sweep(1) < 1.5,
            "envelope 1 still reaches the filter after the routing sent it to aux"
        );
    }

    #[test]
    fn the_gated_amplifier_routing_opens_and_closes_with_the_key() {
        // "filter + gate - Envelope 2 controls Filter Cutoff frequency while
        // Amplifier volume is simply triggered (gated) on/off each time you
        // press a key."
        let setup = with(
            &neutral(),
            &[
                (P_ENV_ROUTE, knob_for(2, ENV_ROUTES.len())),
                (P_E2_ATTACK, 0.8),
                (P_E2_RELEASE, 0.8),
                (P_E2_SUSTAIN, 1.0),
            ],
        );
        let mut s = built(&setup);
        let out = render(&mut s, &[note_on(48, 100, 0)], 20);
        // A slow amplitude envelope would still be climbing here; the gate is
        // already open.
        assert!(rms(&out[..2_048]) > rms(&out) * 0.5, "the gated amplifier ramped up like an envelope");
        let tail = render(&mut s, &[note_off(48, 0)], 20);
        assert!(rms(&tail[4_096..]) < 1.0e-6, "the gated amplifier did not close with the key");
    }

    #[test]
    fn env_repeat_loops_the_first_three_segments() {
        // "When on, the Delay, Attack, and Decay segments of the selected
        // envelopes repeat indefinitely."
        let base = with(
            &neutral(),
            &[
                (P_E2_ATTACK, 0.25),
                (P_E2_DECAY, 0.3),
                (P_E2_SUSTAIN, 0.0),
                (P_E2_RELEASE, 0.1),
            ],
        );
        let mut once = built(&base);
        let single = render(&mut once, &[note_on(48, 100, 0)], 200);
        let mut looping = built(&with(
            &base,
            &[(P_ENV_REPEAT, knob_for(2, ENV_REPEATS.len()))],
        ));
        let repeated = render(&mut looping, &[note_on(48, 100, 0)], 200);
        let tail = single.len() * 3 / 4;
        assert!(
            rms(&single[tail..]) < 1.0e-6,
            "the envelope did not finish, so this test proves nothing"
        );
        assert!(
            rms(&repeated[tail..]) > 1.0e-4,
            "the repeat switch did not loop the envelope"
        );
    }

    #[test]
    fn no_two_voices_share_a_random_stream() {
        let mut s = built(&neutral());
        let _ = render(&mut s, &[note_on(48, 100, 0), note_on(52, 100, 8), note_on(55, 100, 16),
                                 note_on(59, 100, 24), note_on(62, 100, 32)], 4);
        let randoms: Vec<f64> = s.voices.iter().map(|v| v.random).collect();
        for (i, a) in randoms.iter().enumerate() {
            for b in &randoms[i + 1..] {
                assert!(
                    (a - b).abs() > 1.0e-6,
                    "two voices drew the same random value: {randoms:?}"
                );
            }
        }
        let phases: Vec<f64> = s.voices.iter().map(|v| v.osc1.phase).collect();
        for (i, a) in phases.iter().enumerate() {
            for b in &phases[i + 1..] {
                assert!((a - b).abs() > 1.0e-6, "two voices share an oscillator phase");
            }
        }
    }

    #[test]
    fn the_pitch_wheel_bends_by_the_range_it_names() {
        // "The upward range is 12 semitones (1 octave). The downward range is
        // 24 semitones (2 octaves)."
        let bent = |up: usize, down: usize, value: u16| {
            let mut s = built(&with(
                &neutral(),
                &[(P_BEND_UP, knob_for(up, 13)), (P_BEND_DOWN, knob_for(down, 25))],
            ));
            let bend = MidiEvent {
                sample_offset: 0,
                status: 0xE0,
                data1: (value & 0x7F) as u8,
                data2: (value >> 7) as u8,
            };
            let out = render(&mut s, &[note_on(60, 100, 0), bend], 60);
            fundamental_hz(&out[out.len() / 2..], SR, 60.0, 1_200.0)
        };
        let up = bent(7, 2, 16_383);
        let expected_up = raw::note_hz(67.0);
        assert!(
            (up - expected_up).abs() / expected_up < 0.02,
            "a seven-semitone bend up sounds at {up:.1} Hz rather than {expected_up:.1}"
        );
        let down = bent(2, 12, 0);
        let expected_down = raw::note_hz(48.0);
        assert!(
            (down - expected_down).abs() / expected_down < 0.02,
            "a twelve-semitone bend down sounds at {down:.1} Hz rather than {expected_down:.1}"
        );
    }

    #[test]
    fn keyboard_tracking_opens_the_filter_with_the_note() {
        // "Filter Key Amt: Any setting above zero means that the higher the
        // note played on the keyboard, the more the filter opens."
        let base = with(&neutral(), &[(P_CUTOFF, 0.35)]);
        let bright = |amount: f32, note: u8| {
            let mut s = built(&with(&base, &[(P_FILTER_KEY, amount)]));
            let out = render(&mut s, &[note_on(note, 100, 0)], 60);
            brightness(&out[out.len() / 2..], SR) / raw::note_hz(f64::from(note))
        };
        let off_low = bright(0.0, 36);
        let off_high = bright(0.0, 72);
        let on_low = bright(1.0, 36);
        let on_high = bright(1.0, 72);
        assert!(
            on_high / on_low > off_high / off_low * 1.5,
            "keyboard tracking did not open the filter with the note: off {:.2}, on {:.2}",
            off_high / off_low,
            on_high / on_low
        );
    }

    #[test]
    fn the_noise_generator_makes_white_and_pink() {
        // "Noise: Toggles the white/pink noise generator."
        let base = with(
            &neutral(),
            &[(P_O1_ON, 0.0), (P_NOISE_ON, 1.0), (P_NOISE_LEVEL, 1.0), (P_CUTOFF, 1.0)],
        );
        let tilt = |pink: f32| {
            let mut s = built(&with(&base, &[(P_NOISE_TYPE, pink)]));
            let out = render(&mut s, &[note_on(60, 100, 0)], 80);
            let settled = &out[out.len() / 4..];
            db(low_band(settled, 200.0, SR) / rms(settled))
        };
        let white = tilt(0.0);
        let pink = tilt(1.0);
        assert!(
            pink > white + 6.0,
            "pink noise is no darker than white: {white:.1} dB against {pink:.1} dB under 200 Hz"
        );
    }

    #[test]
    fn oscillator_two_can_bypass_the_filter() {
        // "Osc 2 Filter Bypass: Turning this on causes Oscillator 2 to be
        // directly routed to the VCA, bypassing the filter."
        let base = with(
            &neutral(),
            &[(P_O1_ON, 0.0), (P_O2_ON, 1.0), (P_O2_LEVEL, 1.0), (P_CUTOFF, 0.05)],
        );
        let mut through = built(&base);
        let filtered = render(&mut through, &[note_on(60, 100, 0)], 60);
        let mut around = built(&with(&base, &[(P_O2_BYPASS, 1.0)]));
        let direct = render(&mut around, &[note_on(60, 100, 0)], 60);
        assert!(
            rms(&direct) > rms(&filtered) * 10.0,
            "the bypass switch did not take oscillator 2 round the filter: {:.5} against {:.5}",
            rms(&filtered),
            rms(&direct)
        );
    }

    // ── Effect 1 ──

    /// One signal through the effect unit, without an instrument round it.
    fn through_fx(input: &[f64], set: &FxSetting) -> (Vec<f32>, Vec<f32>) {
        let mut unit = FxUnit::new(0x1234_5678);
        unit.init(SR);
        let mut left = Vec::with_capacity(input.len());
        let mut right = Vec::with_capacity(input.len());
        for x in input {
            let (l, r) = unit.process(*x, *x, set, SR);
            left.push(l as f32);
            right.push(r as f32);
        }
        (left, right)
    }

    fn impulse(len: usize) -> Vec<f64> {
        let mut x = vec![0.0; len];
        x[0] = 1.0;
        x
    }

    fn sine(len: usize, hz: f64) -> Vec<f64> {
        (0..len).map(|n| (TAU * hz * n as f64 / SR).sin()).collect()
    }

    #[test]
    fn the_delays_repeat_at_the_time_they_name() {
        for kind in [fx::DELAY, fx::BBD, fx::TAPE1, fx::TAPE2] {
            for seconds in [0.05f64, 0.2] {
                let set = FxSetting { kind, mix: 1.0, time: seconds, misc: 0.4 };
                let (out, _) = through_fx(&impulse((SR * 0.6) as usize), &set);
                let at = out
                    .iter()
                    .enumerate()
                    .skip((SR * 0.01) as usize)
                    .max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap())
                    .map_or(0, |(i, _)| i);
                let measured = at as f64 / SR;
                assert!(
                    (measured - seconds).abs() < 0.01,
                    "{} repeats at {measured:.3} s rather than {seconds:.3}",
                    FX_TYPES[kind]
                );
            }
        }
    }

    #[test]
    fn clock_sync_divides_the_tempo() {
        // The division table is judgment — see FX_DIVISIONS — but the tempo
        // arithmetic is not.
        for (bpm, division) in [(120.0f32, 3usize), (75.0, 6)] {
            let panel: [f32; PARAM_COUNT] = {
                let mut params = param_defaults();
                for (index, value) in with(
                    &neutral(),
                    &[
                        (P_FX_ON, 1.0),
                        (P_FX_TYPE, knob_for(fx::DELAY, FX_TYPES.len())),
                        (P_FX_SYNC, 1.0),
                        (P_FX_DIV, knob_for(division, FX_DIVISIONS.len())),
                        (P_BPM, bpm / 250.0),
                    ],
                ) {
                    params[index] = value;
                }
                params
            };
            let read = Panel::read(&panel, SR);
            let expected = FX_SYNC_BEATS[division] * 60.0 / f64::from(bpm);
            let measured = read.fx_raw.synced.expect("a synced delay has a time");
            assert!(
                (measured - expected).abs() < 1.0e-4,
                "at {bpm} bpm on {} the delay is {measured:.4} s rather than {expected:.4}",
                FX_DIVISIONS[division]
            );
        }
        // A division longer than the line halves until it fits, and the free
        // switch turns the knob back into a time.
        let mut params = param_defaults();
        for (index, value) in with(
            &neutral(),
            &[
                (P_FX_ON, 1.0),
                (P_FX_TYPE, knob_for(fx::DELAY, FX_TYPES.len())),
                (P_FX_SYNC, 1.0),
                (P_FX_DIV, knob_for(10, FX_DIVISIONS.len())),
                (P_BPM, 40.0 / 250.0),
            ],
        ) {
            params[index] = value;
        }
        let read = Panel::read(&params, SR);
        assert!(read.fx_raw.synced.unwrap() <= DELAY_MAX_S, "a synced delay ran past the line");
    }

    #[test]
    fn a_delay_at_full_feedback_never_stops_decaying() {
        for kind in [fx::DELAY, fx::BBD, fx::TAPE1, fx::TAPE2] {
            let set = FxSetting { kind, mix: 1.0, time: 0.05, misc: DELAY_FEEDBACK_MAX };
            let (out, _) = through_fx(&impulse((SR * 8.0) as usize), &set);
            assert!(out.iter().all(|v| v.is_finite()), "{} produced a non-finite sample", FX_TYPES[kind]);
            let early = rms(&out[(SR * 0.5) as usize..(SR * 1.0) as usize]);
            let late = rms(&out[(SR * 7.0) as usize..]);
            assert!(late < early, "{} at full feedback is not decaying", FX_TYPES[kind]);
        }
    }

    #[test]
    fn the_chorus_and_the_flanger_and_the_phaser_move_the_spectrum() {
        // Three sweeping effects with three different mechanisms: a detuned
        // copy, a comb whose notch passes through zero, and six all-pass
        // sections. All three have to *move* — a partial's level has to
        // change across the sweep — and the flanger has to move furthest,
        // because a through-zero comb takes its notch right down through DC.
        let tone = sine((SR * 4.0) as usize, 400.0);
        let swing = |kind: usize, misc: f64| {
            let set = FxSetting { kind, mix: 1.0, time: 1.0, misc };
            let (left, _) = through_fx(&tone, &set);
            let windows: Vec<f64> = left
                .chunks(2_048)
                .skip(4)
                .map(|c| harmonic(c, 400.0, SR))
                .collect();
            let loud = windows.iter().copied().fold(0.0f64, f64::max);
            let quiet = windows.iter().copied().fold(f64::MAX, f64::min);
            db(loud / quiet.max(1.0e-12))
        };
        let chorus = swing(fx::CHORUS, 8_000.0);
        let flanger = swing(fx::FLANGER, 0.6);
        let phaser = swing(fx::PHASER, 0.6);
        assert!(chorus > 0.5, "the chorus does not move the tone: {chorus:.1} dB");
        assert!(flanger > 12.0, "the flanger's comb does not sweep: {flanger:.1} dB");
        assert!(phaser > 3.0, "the phaser's notches do not sweep: {phaser:.1} dB");
        assert!(
            flanger > chorus,
            "the flanger is no deeper than the chorus, so it is not through-zero"
        );

        // The chorus is also *wide*: its two sweeps are a quarter cycle apart.
        let set = FxSetting { kind: fx::CHORUS, mix: 1.0, time: 1.0, misc: 8_000.0 };
        let (left, right) = through_fx(&tone, &set);
        let difference: f64 = left
            .iter()
            .zip(&right)
            .map(|(a, b)| f64::from(*a - *b).abs())
            .fold(0.0, f64::max);
        assert!(difference > 0.05, "the chorus is mono: {difference:.4}");
    }

    #[test]
    fn the_high_pass_effect_removes_bass() {
        let bass = sine((SR * 0.5) as usize, 60.0);
        let treble = sine((SR * 0.5) as usize, 4_000.0);
        let set = FxSetting { kind: fx::HPF, mix: 1.0, time: 800.0, misc: 0.2 };
        let (low, _) = through_fx(&bass, &set);
        let (high, _) = through_fx(&treble, &set);
        assert!(
            db(rms(&high) / rms(&low)) > 18.0,
            "the high-pass effect is not high-passing: {:.1} dB between 60 Hz and 4 kHz",
            db(rms(&high) / rms(&low))
        );
    }

    #[test]
    fn the_distortion_effect_adds_harmonics_and_the_tone_knob_shapes_them() {
        let tone = sine((SR * 0.4) as usize, 300.0);
        let harsh = FxSetting { kind: fx::DISTORT, mix: 1.0, time: 30.0, misc: 12_000.0 };
        let (out, _) = through_fx(&tone, &harsh);
        let third = harmonic(&out, 900.0, SR) / harmonic(&out, 300.0, SR);
        assert!(third > 0.1, "the distortion added no third harmonic: {third:.3}");
        let dark = FxSetting { misc: 500.0, ..harsh };
        let (dull, _) = through_fx(&tone, &dark);
        assert!(
            brightness(&dull, SR) < brightness(&out, SR) * 0.8,
            "the tone knob does not shape the distortion"
        );
    }

    #[test]
    fn the_ring_modulator_puts_sidebands_where_the_carrier_says() {
        let tone = sine((SR * 0.5) as usize, 600.0);
        let carrier = 220.0;
        let set = FxSetting { kind: fx::RING, mix: 1.0, time: carrier, misc: 0.0 };
        let (out, _) = through_fx(&tone, &set);
        let upper = harmonic(&out, 600.0 + carrier, SR);
        let lower = harmonic(&out, 600.0 - carrier, SR);
        let centre = harmonic(&out, 600.0, SR);
        assert!(
            upper > centre * 2.0 && lower > centre * 2.0,
            "the ring modulator left the carrier in place: {centre:.4} against {lower:.4} \
             and {upper:.4}"
        );
    }

    #[test]
    fn the_rotating_speaker_swings_between_the_channels() {
        let tone = sine((SR * 3.0) as usize, 1_200.0);
        let set = FxSetting { kind: fx::ROTARY, mix: 0.2, time: ROTARY_FAST_HZ, misc: 0.1 };
        let (left, right) = through_fx(&tone, &set);
        let settled = SR as usize;
        let swing: Vec<f64> = left[settled..]
            .chunks(1_024)
            .zip(right[settled..].chunks(1_024))
            .map(|(l, r)| rms(l) - rms(r))
            .collect();
        let widest = swing.iter().copied().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(widest > 0.02, "the cabinet does not turn: {widest:.4}");
        assert!(
            swing.iter().any(|v| *v > 0.0) && swing.iter().any(|v| *v < 0.0),
            "the cabinet turns one way only"
        );
    }

    #[test]
    fn the_lo_fi_effect_resamples_and_aliases() {
        // "Lo-Fi - emulates the transformative effects of a badly-calibrated
        // tape machine." A 2 kHz tone held at 3 kHz has to come back with an
        // image at the difference, 1 kHz, which is what a sample rate too low
        // for its material does.
        let tone = sine((SR * 0.5) as usize, 2_000.0);
        let set = FxSetting { kind: fx::LOFI, mix: 0.0, time: 3_000.0, misc: 0.0 };
        let (out, _) = through_fx(&tone, &set);
        let alias = harmonic(&out, 1_000.0, SR);
        let original = harmonic(&out, 2_000.0, SR);
        assert!(
            alias > original * 0.3,
            "no image appeared where the resampling should have put one: \
             {alias:.4} against {original:.4}"
        );
        // ...and the wow knob moves the tape: the transport wanders, so the
        // same input comes back displaced in time.
        let tape = sine((SR * 2.0) as usize, 500.0);
        let (wobbled, _) = through_fx(&tape, &FxSetting { mix: 1.0, ..set });
        let (still, _) = through_fx(&tape, &FxSetting { mix: 0.0, ..set });
        let moved = wobbled
            .iter()
            .zip(&still)
            .map(|(a, b)| f64::from(*a - *b).abs())
            .fold(0.0f64, f64::max);
        assert!(moved > 0.2, "the wow and flutter knob does nothing: {moved:.4}");
    }

    #[test]
    fn every_effect_is_bounded_and_finite_at_every_extreme() {
        let panel = with(
            &rich(),
            &[(P_FX_ON, 1.0), (P_FX_MIX, 1.0), (P_OVERDRIVE, 1.0), (P_VOLUME, 1.0)],
        );
        for (kind, kind_name) in FX_TYPES.iter().enumerate() {
            for time in [0.0f32, 0.5, 1.0] {
                for misc in [0.0f32, 1.0] {
                    let mut s = built(&with(
                        &panel,
                        &[
                            (P_FX_TYPE, knob_for(kind, FX_TYPES.len())),
                            (P_FX_TIME, time),
                            (P_FX_MISC, misc),
                        ],
                    ));
                    let out = render_program(&mut s, &[36, 48, 55, 60, 67], 127, 80);
                    assert!(
                        out.iter().all(|v| v.is_finite()),
                        "{kind_name} at time {time} misc {misc} produced a non-finite sample"
                    );
                    assert!(
                        peak(&out) < 1.0,
                        "{kind_name} at time {time} misc {misc} pinned the output at {:.3}",
                        peak(&out)
                    );
                }
            }
        }
    }

    // ── Overdrive ──

    #[test]
    fn the_overdrive_knob_is_the_identity_at_zero() {
        for x in [-1.5f64, -0.4, 0.0, 0.3, 2.0] {
            assert_eq!(overdrive(x, 0.0), x, "the bottom of the overdrive knob is not a wire");
        }
    }

    #[test]
    fn overdrive_adds_harmonics_and_bounds_the_output() {
        let base = with(&neutral(), &[(P_O1_SAW, 0.0), (P_O1_TRI, 1.0), (P_CUTOFF, 1.0)]);
        let content = |amount: f32| {
            let mut s = built(&with(&base, &[(P_OVERDRIVE, amount)]));
            let out = render(&mut s, &[note_on(48, 100, 0)], 60);
            let tail = &out[out.len() / 2..];
            let root = raw::note_hz(48.0);
            (
                harmonic(tail, root * 2.0, SR) / harmonic(tail, root, SR).max(1.0e-30),
                peak(&out),
            )
        };
        let (clean, _) = content(0.0);
        let (driven, top) = content(1.0);
        assert!(
            driven > clean * 4.0,
            "the overdrive knob added no harmonics: {clean:.4} to {driven:.4}"
        );
        assert!(top < 1.0, "the overdrive ran past full scale: {top:.3}");
        // The asymmetry is real: the negative half compresses less, which is
        // where the even harmonics come from.
        assert!(
            overdrive(-0.8, 1.0).abs() > overdrive(0.8, 1.0).abs() * 1.1,
            "the overdrive is symmetric, so it makes only odd harmonics"
        );
    }

    // ── The panel ──

    #[test]
    fn the_panel_is_in_front_panel_order() {
        // Every index used exactly once, no gaps, and the sections in the
        // order the manual's Chapter 2 walks the front panel.
        let mut indices = vec![
            P_PROGRAM, P_BANK,
            P_O1_FREQ, P_O1_FINE, P_O1_TRI, P_O1_SAW, P_O1_PULSE, P_O1_WIDTH, P_O1_KEY,
            P_O1_GLIDE, P_O1_ON, P_O1_LEVEL,
            P_O2_FREQ, P_O2_FINE, P_O2_TRI, P_O2_SAW, P_O2_PULSE, P_O2_WIDTH, P_O2_KEY,
            P_O2_GLIDE, P_O2_ON, P_O2_LEVEL, P_O2_BYPASS,
            P_XMOD, P_SYNC,
            P_SUB_ON, P_SUB_LEVEL, P_NOISE_ON, P_NOISE_TYPE, P_NOISE_LEVEL,
            P_CUTOFF, P_RESONANCE, P_STATE, P_BANDPASS, P_FILTER_KEY,
            P_E1_AMOUNT, P_E1_VEL, P_E1_DELAY, P_E1_ATTACK, P_E1_DECAY, P_E1_SUSTAIN,
            P_E1_RELEASE,
            P_E2_AMOUNT, P_E2_VEL, P_E2_DELAY, P_E2_ATTACK, P_E2_DECAY, P_E2_SUSTAIN,
            P_E2_RELEASE,
            P_ENV_ROUTE, P_E1_DEST, P_ENV_REPEAT,
            P_L1_FREQ, P_L1_SHAPE, P_L1_AMOUNT, P_L1_DEST, P_L1_SYNC, P_L1_DIV, P_L1_RESET,
            P_L1_SLEW,
            P_L2_FREQ, P_L2_SHAPE, P_L2_AMOUNT, P_L2_DEST, P_L2_SYNC, P_L2_DIV, P_L2_RESET,
            P_L2_SLEW,
        ];
        for index in 0..MOD_SLOTS {
            indices.extend([P_MOD + 3 * index, P_MOD + 3 * index + 1, P_MOD + 3 * index + 2]);
        }
        indices.extend([
            P_FX_ON, P_FX_TYPE, P_FX_MIX, P_FX_TIME, P_FX_MISC, P_FX_SYNC, P_FX_DIV,
            P_RV_ON, P_RV_MIX, P_RV_SIZE, P_RV_PREDELAY, P_RV_DECAY, P_RV_TONE,
            P_OVERDRIVE, P_VINTAGE, P_VOLUME, P_PAN,
            P_UNISON, P_UNISON_VOICES, P_UNISON_DETUNE, P_KEY_MODE, P_RETRIGGER,
            P_GLIDE, P_GLIDE_MODE,
            P_SPLIT_1, P_SPLIT_2, P_SPLIT_NOTE,
            P_BEND_UP, P_BEND_DOWN, P_TRANSPOSE, P_BPM,
        ]);
        assert_eq!(indices.len(), PARAM_COUNT, "the panel list and PARAM_COUNT disagree");
        for (position, index) in indices.iter().enumerate() {
            assert_eq!(*index, position, "parameter {position} is out of panel order");
        }
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            assert!(!name.is_empty(), "parameter {index} has no name");
            assert!(name.chars().count() <= 8, "parameter name {name:?} overflows its column");
        }
        // Every engine control has a byte in the program block, except the two
        // program selectors, which are where the program came from, and the
        // two two-byte controls, which read their own pair.
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            let expected = !matches!(index, P_PROGRAM | P_BANK | P_CUTOFF | P_STATE);
            assert_eq!(
                raw_offset(index).is_some(),
                expected,
                "{name} is {} a stored parameter",
                if expected { "not" } else { "unexpectedly" }
            );
        }
        // ...and no two controls read the same byte.
        let mut offsets: Vec<usize> = (0..PARAM_COUNT)
            .filter_map(|i| raw_offset(i).map(|(offset, _)| offset))
            .chain([CUTOFF_BYTES.0, CUTOFF_BYTES.1, STATE_BYTES.0, STATE_BYTES.1])
            .collect();
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
                assert_eq!(selector(knob, count), position, "{name} skipped a position going up");
                knob = step_discrete(index, knob, true);
            }
            assert_eq!(selector(knob, count), count - 1, "{name} ran off the top");
            for position in (0..count).rev() {
                assert_eq!(selector(knob, count), position, "{name} skipped a position going down");
                knob = step_discrete(index, knob, false);
            }
            assert_eq!(selector(knob, count), 0, "{name} ran off the bottom");
        }
    }

    #[test]
    fn switch_labels_read_as_the_panel_does() {
        // Twelve columns is what the editor's parameter row leaves after the
        // indicator and the control's own name.
        const ROOM: usize = 12;
        let mut params = vec![0.0f32; PARAM_COUNT];
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
                assert!(
                    !label.is_empty() && label.chars().count() <= ROOM,
                    "{} position {position} reads {label:?}, which does not fit",
                    PARAM_NAMES[index]
                );
            }
            params[index] = 0.0;
        }
        // Every factory program's name fits the same column once truncated,
        // and the truncation is the front of the name rather than a rewrite.
        for index in 0..PROGRAM_COUNT {
            let label = program_label(index);
            let name = program_name(index);
            assert!(label.chars().count() <= ROOM, "program {index} {label:?} does not fit");
            assert!(
                !name.is_empty() && name.chars().count() <= 20 && name.starts_with(label),
                "program {index} name {name:?} and label {label:?} disagree"
            );
        }
    }

    #[test]
    fn the_program_knobs_land_on_the_program_they_name() {
        for index in 0..PROGRAM_COUNT {
            let (bank, program) = program_knobs(index);
            assert_eq!(program_index(bank, program), index, "program {index} is not selectable");
            assert_eq!(bank_index(bank), index / PROGRAMS_PER_BANK);
            assert_eq!(patch_index(program), index % PROGRAMS_PER_BANK);
        }
        // Out of range in either direction lands on a real program.
        assert_eq!(program_index(-1.0, -1.0), 0);
        assert_eq!(program_index(2.0, 2.0), PROGRAM_COUNT - 1);
    }

    #[test]
    fn the_program_knob_loads_the_whole_panel() {
        let mut s = Teo5::new();
        for index in [0usize, 7, 91, 200, PROGRAM_COUNT - 1] {
            let (bank, program) = program_knobs(index);
            s.set_parameter(P_BANK, bank);
            s.set_parameter(P_PROGRAM, program);
            assert_eq!(s.current_program(), index);
            let expected = params_for_program(bank, program);
            for control in 0..PARAM_COUNT {
                assert_eq!(
                    s.get_parameter(control),
                    expected[control],
                    "{} did not follow the program knob to {index}",
                    PARAM_NAMES[control]
                );
            }
            assert_eq!(s.chord_memory(), program_chord(index), "the chord did not follow");
        }
    }

    /// A panel with something engaged in every section, for the reachability
    /// sweep: both oscillators up with all three shapes, the sub and the
    /// noise in, sync and cross-modulation on, the filter part way through
    /// its morph, both envelopes shaped, both LFOs running with one of them
    /// clock-synced, the matrix full, an effect on, unison and glide on and
    /// the keyboard split.
    fn everything() -> Vec<(usize, f32)> {
        with(
            &rich(),
            &[
                (P_O1_TRI, 1.0),
                (P_O2_TRI, 1.0),
                (P_O1_KEY, 1.0),
                (P_O2_KEY, 1.0),
                (P_O1_GLIDE, 0.4),
                (P_O2_GLIDE, 0.5),
                (P_SYNC, 1.0),
                (P_E1_VEL, 1.0),
                (P_E2_VEL, 1.0),
                (P_E1_DELAY, 0.1),
                (P_E2_DELAY, 0.1),
                // Short enough that both envelopes reach their sustain
                // inside the render, which is what the repeat switch needs
                // in order to have something to loop back from.
                (P_E1_DECAY, 0.3),
                (P_E2_DECAY, 0.3),
                (P_ENV_ROUTE, knob_for(1, ENV_ROUTES.len())),
                (P_E1_DEST, knob_for(15, MOD_DESTS.len())),
                (P_L1_SYNC, 1.0),
                (P_L1_DIV, knob_for(9, LFO_DIVISIONS.len())),
                (P_L1_SLEW, 0.5),
                (P_L2_SLEW, 0.5),
                (P_L1_RESET, 1.0),
                (P_FX_TYPE, knob_for(fx::DELAY, FX_TYPES.len())),
                (P_FX_SYNC, 1.0),
                // The shortest synced division, so that several repeats land
                // inside the render and the feedback knob has something to
                // feed back.
                (P_FX_DIV, knob_for(0, FX_DIVISIONS.len())),
                (P_FX_MISC, 0.5),
                (P_BPM, 100.0 / 250.0),
                (P_RV_ON, 1.0),
                (P_RV_MIX, 0.5),
                (P_KEY_MODE, knob_for(2, KEY_MODES.len())),
                (P_RETRIGGER, 1.0),
                (P_GLIDE, 1.0),
                (P_SPLIT_1, 1.0),
                (P_SPLIT_NOTE, 30.0 / 43.0),
                (P_BEND_UP, knob_for(7, 13)),
                (P_BEND_DOWN, knob_for(7, 25)),
                (P_TRANSPOSE, knob_for(3, TRANSPOSE_LABELS.len())),
                (P_PAN, 0.3),
            ],
        )
    }

    #[test]
    fn every_engine_control_is_reachable_and_the_reverb_is_not() {
        // A leap of fourteen semitones rather than twelve: the fixed-rate
        // glide modes take the knob's time for an octave and the fixed-time
        // modes take it for the whole interval, so an octave would make the
        // two indistinguishable.
        fn play(s: &mut Teo5) -> Vec<f32> {
            let events = [
                note_on(45, 90, 0),
                cc(1, 100, 40),
                MidiEvent { sample_offset: 60, status: 0xE0, data1: 0, data2: 100 },
                aftertouch(90, 80),
            ];
            // A little silence first, so that the global LFO has moved off
            // its start phase before the first key: a note reset that lands
            // on phase zero is not a reset.
            let mut out = render(s, &[], 5);
            out.extend_from_slice(&render(s, &events, 40));
            out.extend_from_slice(&render(s, &[note_on(59, 120, 0)], 40));
            let bend_down = MidiEvent { sample_offset: 0, status: 0xE0, data1: 0, data2: 20 };
            out.extend_from_slice(&render(s, &[bend_down], 20));
            out.extend_from_slice(&render(s, &[note_off(59, 0), note_off(45, 1)], 40));
            out
        }
        let base = everything();
        for (index, name) in PARAM_NAMES.iter().enumerate() {
            if index == P_PROGRAM || index == P_BANK {
                continue;
            }
            // Two knobs are read only when their own clock-sync switch is the
            // other way round, so each is probed with it moved.
            let mut panel = base.clone();
            match index {
                P_FX_TIME => panel.push((P_FX_SYNC, 0.0)),
                P_L1_FREQ => panel.push((P_L1_SYNC, 0.0)),
                P_L2_DIV => panel.push((P_L2_SYNC, 1.0)),
                _ => {}
            }
            // The effect selector is stepped between two algorithms rather
            // than between the ends of its travel, because position zero is
            // *off* and comparing a wire with a wire proves nothing.
            let (down, up) = match index {
                P_FX_TYPE => (
                    knob_for(fx::BBD, FX_TYPES.len()),
                    knob_for(fx::PHASER, FX_TYPES.len()),
                ),
                _ => (0.15, 0.85),
            };
            let mut low = built(&panel);
            low.set_parameter(index, down);
            let mut high = built(&panel);
            high.set_parameter(index, up);
            let a = play(&mut low);
            let b = play(&mut high);
            let same = a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1.0e-7);
            // The six reverb controls are the deferral, stated as a test: the
            // panel keeps them, the program round-trips them, and until the
            // shared reverb bus exists they make no sound at all.
            let stored = matches!(
                index,
                P_RV_ON | P_RV_MIX | P_RV_SIZE | P_RV_PREDELAY | P_RV_DECAY | P_RV_TONE
            );
            if stored {
                assert!(same, "{name} is documented as stored and not rendered, but it sounds");
            } else {
                assert!(!same, "{name} does nothing to the sound");
            }
        }
    }

    // ── The factory bank ──

    #[test]
    fn the_rom_is_the_shape_the_decoder_expects() {
        assert_eq!(ROM.len(), PROGRAM_COUNT * PACKED_PROGRAM);
        for index in 0..PROGRAM_COUNT {
            let block = &ROM[index * PACKED_PROGRAM..(index + 1) * PACKED_PROGRAM];
            for (control, name) in PARAM_NAMES.iter().enumerate() {
                if let Some((offset, max)) = raw_offset(control) {
                    assert!(
                        f64::from(block[offset]) <= max || max == 1.0,
                        "program {index}: {name} at byte {offset} is {} (max {max})",
                        block[offset]
                    );
                }
            }
            // The two-byte controls, and the two bytes the decode says are
            // always zero, always one and always 127.
            assert!(
                f64::from(block[CUTOFF_BYTES.0]) + 256.0 * f64::from(block[CUTOFF_BYTES.1])
                    <= CUTOFF_BYTES.2,
                "program {index}: the cutoff is out of range"
            );
            assert!(
                f64::from(block[STATE_BYTES.0]) + 256.0 * f64::from(block[STATE_BYTES.1])
                    <= STATE_BYTES.2,
                "program {index}: the state is out of range"
            );
            for offset in [29usize, 41, 49, 57, 74, 80, 92, 155, 188] {
                assert_eq!(block[offset], 0, "program {index}: byte {offset} is not zero");
            }
            assert_eq!(block[51], 1, "program {index}: byte 51 is not the constant 1");
            assert_eq!(block[94], 127, "program {index}: byte 94 is not the constant 127");
            // The name and the category.
            assert!(
                block[159..179].iter().all(|c| (0x20..=0x7E).contains(c)),
                "program {index}: the name is not printable ASCII"
            );
            assert!(
                (1..=15).contains(&block[179]),
                "program {index}: the category is {}",
                block[179]
            );
        }
    }

    #[test]
    fn program_names_and_categories_come_from_the_rom() {
        assert_eq!(program_name(0), "It's an Oberheim");
        assert_eq!(program_name(PROGRAM_COUNT - 1), "Spiral flow");
        assert_eq!(program_category(0), "pad");
        let mut names = std::collections::HashSet::new();
        let mut counts = [0usize; 16];
        for index in 0..PROGRAM_COUNT {
            let name = program_name(index);
            assert!(!name.is_empty(), "program {index} has no name");
            assert!(names.insert(name), "two programs are called {name:?}");
            let category = program_category(index);
            assert!(category != "-", "program {index} {name:?} has no category");
            counts[CATEGORIES.iter().position(|c| *c == category).unwrap()] += 1;
            // The bank knob reads as the bank and the category together.
            let label = program_bank_label(index);
            assert_eq!(
                label,
                format!("{} {category}", BANK_DIGITS[index / PROGRAMS_PER_BANK] as char),
                "program {index} {name:?} does not read as its bank and category"
            );
        }
        // The counts the categorized bank has, which is this bank sorted on
        // the same byte.
        assert_eq!(
            &counts[1..],
            &[35, 23, 23, 59, 16, 8, 13, 9, 27, 8, 3, 7, 7, 7, 11],
            "the categories are not the bank's"
        );
    }

    #[test]
    fn no_factory_program_is_silent() {
        let mut quietest = (f64::MAX, 0usize);
        for index in 0..PROGRAM_COUNT {
            let mut s = fresh(index);
            let mut out = render_program(&mut s, &[36, 48, 60], 110, 300);
            out.extend_from_slice(&render(&mut s, &[note_off(36, 0)], 40));
            assert!(
                out.iter().all(|v| v.is_finite()),
                "{} produced a non-finite sample",
                program_name(index)
            );
            let level = rms(&out);
            if level < quietest.0 {
                quietest = (level, index);
            }
        }
        assert!(
            quietest.0 > 1.0e-4,
            "{} renders at {:.6} rms, which is nothing",
            program_name(quietest.1),
            quietest.0
        );
    }

    #[test]
    fn the_bank_covers_the_instrument() {
        // Every selector position the 256 programs use, and every one they
        // do not, so that a byte map that quietly stopped reaching a control
        // shows up as a bank that stopped using it.
        let mut seen: Vec<std::collections::HashSet<usize>> =
            vec![std::collections::HashSet::new(); PARAM_COUNT];
        for index in 0..PROGRAM_COUNT {
            let (bank, program) = program_knobs(index);
            let panel = params_for_program(bank, program);
            for control in 0..PARAM_COUNT {
                if let Some(count) = discrete_steps(control) {
                    seen[control].insert(selector(panel[control], count));
                }
            }
        }
        // Both positions of every switch, every waveshape, every effect, and
        // every one of the twenty modulation sources.
        for control in [
            P_O1_TRI, P_O1_SAW, P_O1_PULSE, P_O2_TRI, P_O2_SAW, P_O2_PULSE, P_SUB_ON,
            P_NOISE_ON, P_NOISE_TYPE, P_SYNC, P_BANDPASS, P_UNISON, P_FX_ON, P_RV_ON,
            P_GLIDE, P_O2_BYPASS, P_E1_VEL, P_E2_VEL,
        ] {
            assert_eq!(
                seen[control].len(),
                2,
                "the bank never uses both positions of {}",
                PARAM_NAMES[control]
            );
        }
        assert_eq!(seen[P_FX_TYPE].len(), FX_TYPES.len(), "the bank does not use every effect");
        assert_eq!(seen[P_L1_SHAPE].len(), LFO_SHAPES.len(), "the bank does not use every LFO shape");
        let mut sources = std::collections::HashSet::new();
        for slot in 0..MOD_SLOTS {
            sources.extend(seen[P_MOD + 3 * slot].iter().copied());
        }
        // Breath, foot and expression arrive from pedals the factory bank has
        // no reason to assume, so seventeen of the twenty appear.
        assert!(
            sources.len() >= 17,
            "the bank only reaches {} of the twenty modulation sources",
            sources.len()
        );
    }

    // ── Headroom ──

    /// The worst panel a hand can reach: five voices in unison on "all", all
    /// three waveshapes on both oscillators, the sub and the noise at full,
    /// the resonance at the top of its travel and both envelopes wide open.
    ///
    /// The effect unit is switched off here on purpose. Its distortion
    /// algorithm bounds its own output at full scale whatever it is given, so
    /// a worst case measured through it would be measuring the clipper rather
    /// than the voices; every one of the thirteen algorithms is bounded
    /// separately, at every extreme of its own knobs, by
    /// `every_effect_is_bounded_and_finite_at_every_extreme`.
    pub(crate) fn worst_panel() -> Vec<(usize, f32)> {
        with(
            &neutral(),
            &[
                (P_O1_TRI, 1.0), (P_O1_SAW, 1.0), (P_O1_PULSE, 1.0), (P_O1_LEVEL, 1.0),
                (P_O2_ON, 1.0), (P_O2_TRI, 1.0), (P_O2_SAW, 1.0), (P_O2_PULSE, 1.0),
                (P_O2_LEVEL, 1.0), (P_O2_FINE, 0.6),
                (P_SUB_ON, 1.0), (P_SUB_LEVEL, 1.0),
                (P_NOISE_ON, 1.0), (P_NOISE_LEVEL, 1.0),
                (P_RESONANCE, 1.0),
                (P_E1_AMOUNT, 1.0), (P_E1_SUSTAIN, 1.0),
                (P_E2_AMOUNT, 1.0), (P_E2_SUSTAIN, 1.0),
                (P_OVERDRIVE, 1.0), (P_VOLUME, 1.0),
                (P_UNISON, 1.0),
                (P_UNISON_VOICES, knob_for(5, UNISON_VOICES.len())),
                (P_UNISON_DETUNE, knob_for(7, UNISON_DETUNE_LABELS.len())),
            ],
        )
    }

    #[test]
    fn the_worst_panel_a_hand_can_reach_stays_under_the_ceiling() {
        // Swept rather than sampled, because the peak of a resonant filter is
        // at one cutoff and not at the ends of the knob.
        let worst = worst_panel();
        /// The master limiter's ceiling, -1 dBFS. Repeated rather than
        /// imported because phosphor-dsp does not depend on phosphor-core.
        const CEILING: f32 = 0.891;
        let mut top = (0.0f32, String::new());
        for cutoff in [0.0f32, 0.2, 0.35, 0.5, 0.7, 1.0] {
            for state in [0.0f32, 0.5, 1.0] {
                for drive in [0.0f32, 0.3, 1.0] {
                    for note in [24u8, 36, 48, 60, 84] {
                        let mut s = built(&with(
                            &worst,
                            &[(P_CUTOFF, cutoff), (P_STATE, state), (P_OVERDRIVE, drive)],
                        ));
                        let out = render(&mut s, &[note_on(note, 127, 0)], 60);
                        let peak = peak(&out);
                        assert!(out.iter().all(|v| v.is_finite()));
                        if peak > top.0 {
                            top = (
                                peak,
                                format!("cutoff {cutoff} state {state} drive {drive} note {note}"),
                            );
                        }
                    }
                }
            }
        }
        assert!(
            top.0 < CEILING,
            "the worst panel peaks at {:.4} ({}), which is past the master limiter's ceiling",
            top.0,
            top.1
        );
        // ...and it uses the headroom it is given, so a trim quietly deepened
        // to make this pass would fail instead.
        assert!(
            top.0 > 0.2,
            "the worst panel a hand can reach only reaches {:.4}, so the trim is too deep",
            top.0
        );
    }

    // ── Real-time safety ──

    #[test]
    fn the_audio_path_does_not_allocate() {
        // "No allocation in `process`" is a property of the code rather than
        // of its output, so it is counted rather than listened to. The
        // counting allocator lives in synth.rs and is installed for the whole
        // test binary; this is the TEO-5's half of it.
        use crate::synth::tests::allocations_during;

        let mut s = Teo5::new();
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
            s.process(
                &[],
                &mut outs,
                &[cc(1, 64, 0), cc(2, 90, 1), cc(4, 30, 2), cc(11, 100, 3), aftertouch(80, 4)],
            );
            s.process(&[], &mut outs, &releases);
            // Every program in the bank, loaded while the instrument is
            // sounding, which is what a preset sweep does.
            for index in 0..PROGRAM_COUNT {
                let (bank, program) = program_knobs(index);
                s.set_parameter(P_BANK, bank);
                s.set_parameter(P_PROGRAM, program);
                s.process(&[], &mut outs, &[note_on(60, 110, 0)]);
            }
            // Unison, the chord-memory gesture and the two panic controls.
            s.set_parameter(P_UNISON, 0.0);
            s.process(&[], &mut outs, &[note_on(48, 100, 0), note_on(55, 100, 8)]);
            s.set_parameter(P_UNISON, 1.0);
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
        /// How far the two channels differ, as a share of the level.
        width: f64,
    }

    fn character(index: usize) -> Character {
        let mut s = fresh(index);
        let (mut left, mut right) = render_stereo(&mut s, &[note_on(48, 100, 0)], 200);
        let (tail_l, tail_r) = render_stereo(&mut s, &[note_off(48, 0)], 60);
        left.extend_from_slice(&tail_l);
        right.extend_from_slice(&tail_r);
        let windows = window_rms(&left);
        let loud = windows.iter().copied().fold(0.0f64, f64::max);
        let quiet = windows.iter().copied().fold(f64::MAX, f64::min);
        let brights: Vec<f64> = left
            .chunks(4_096)
            .filter(|c| c.len() == 4_096 && rms(c) > loud * 0.2)
            .map(|c| brightness(c, SR))
            .collect();
        let quarter = left.len() / 4;
        let level = rms(&left);
        let difference: Vec<f32> =
            left.iter().zip(&right).map(|(a, b)| a - b).collect();
        Character {
            level,
            brightness: brightness(&left, SR),
            bass: low_band(&left, 300.0, SR) / level.max(1.0e-30),
            tail: rms(&left[left.len() - quarter..]) / rms(&left[..quarter]).max(1.0e-30),
            movement: loud / quiet.max(1.0e-12),
            sweep: brights.iter().copied().fold(0.0f64, f64::max)
                / brights.iter().copied().fold(f64::MAX, f64::min).max(1.0e-12),
            width: rms(&difference) / level.max(1.0e-30),
        }
    }

    #[test]
    fn the_anchor_programs_sound_like_their_names() {
        // The programs the decode was checked against, checked again by
        // rendering them rather than by reading their bytes.

        // 0:0 "It's an Oberheim" - the SEM showcase: state at 0, which is a
        // pure low pass, with voice spread on the panning.
        let oberheim = character(0);
        assert_eq!(program_name(0), "It's an Oberheim");
        assert!(
            oberheim.bass > 0.6 && oberheim.brightness < 400.0,
            "the showcase pad is not low-passed: {:.2} bass, {:.0} Hz",
            oberheim.bass,
            oberheim.brightness
        );
        assert!(
            oberheim.width > 0.1,
            "voice spread is not reaching the panning: the two channels differ by {:.3}",
            oberheim.width
        );

        // 0:15 "Sync Growl" - hard sync with the envelope on oscillator 1's
        // frequency, which is a formant sweeping across a held note.
        let growl = character(15);
        assert_eq!(program_name(15), "Sync Growl");
        assert!(
            growl.sweep > 1.5,
            "the sync growl does not move: its brightest window is only {:.2} times its dullest",
            growl.sweep
        );

        // 4:9 "Weeping Wah" - band pass at the exact centre of the state
        // control with the resonance at maximum, swept.
        let wah = character(73);
        assert_eq!(program_name(73), "Weeping Wah");
        assert!(
            wah.bass > 0.5,
            "the wah is not band-limited around a low formant: {:.2} of its energy under 300 Hz",
            wah.bass
        );
        assert!(
            wah.sweep > 1.4 || wah.movement > 3.0,
            "the wah does not weep: sweep {:.2}, movement {:.2}",
            wah.sweep,
            wah.movement
        );

        // 2:13 "Bandpass Arp" - band pass on, and a plucked shape.
        let arp = character(45);
        assert_eq!(program_name(45), "Bandpass Arp");
        assert!(arp.tail < 0.5, "the plucked arp does not decay: {:.2}", arp.tail);

        // 4:7 "Quintuple Mono" - five voices on one key.
        let mut s = fresh(71);
        assert_eq!(program_name(71), "Quintuple Mono");
        let _ = render(&mut s, &[note_on(48, 100, 0)], 2);
        assert_eq!(
            s.voices.iter().filter(|v| v.gate).count(),
            VOICES,
            "Quintuple Mono does not stack five voices"
        );

        // 10:4 "OB-X  S & H" - a sample-and-hold LFO on the cutoff, which
        // steps the filter rather than sweeping it.
        let sh = character(164);
        assert_eq!(program_name(164), "OB-X  S & H");
        assert!(
            sh.sweep > 1.3,
            "the sample-and-hold does not move the filter: {:.2}",
            sh.sweep
        );
    }

    #[test]
    fn programs_from_every_bank_render_plausibly() {
        // Three programs from each of the sixteen banks: a real level, no
        // non-finite samples, and an envelope that goes somewhere.
        for bank in 0..BANK_COUNT {
            for n in 0..3 {
                let index = bank * PROGRAMS_PER_BANK + n * 5 + 1;
                let mut s = fresh(index);
                let out = render_program(&mut s, &[48], 100, 200);
                let name = program_name(index);
                assert!(out.iter().all(|v| v.is_finite()), "{name} produced a non-finite sample");
                assert!(peak(&out) < 0.891, "{name} peaks at {:.3}", peak(&out));
                assert!(rms(&out) > 1.0e-4, "{name} renders at {:.6} rms", rms(&out));
                // The note has to be going away rather than staying. The
                // bank has releases of four seconds and more, so what is
                // asserted is the direction and not a deadline — and it has
                // to allow for the programs whose release is instant, where
                // both ends of the measurement are already silence.
                let released = render(&mut s, &[note_off(48, 0)], 300);
                let quarter = released.len() / 4;
                let opening = rms(&released[..quarter]);
                let closing = rms(&released[3 * quarter..]);
                assert!(
                    closing <= opening,
                    "{name} is not decaying after the key came up: {opening:.6} to {closing:.6}"
                );
            }
        }
    }

    #[test]
    fn the_bank_is_two_hundred_and_fifty_six_sounds_rather_than_one() {
        // Sixteen programs spread across the sixteen banks, measured on four
        // axes. If the byte map were wrong in a way that made every program
        // render the same, this is what would catch it.
        let mut brightness_range = (f64::MAX, 0.0f64);
        let mut bass_range = (f64::MAX, 0.0f64);
        let mut tail_range = (f64::MAX, 0.0f64);
        let mut level_range = (f64::MAX, 0.0f64);
        for bank in 0..BANK_COUNT {
            let c = character(bank * PROGRAMS_PER_BANK + 3);
            brightness_range = (brightness_range.0.min(c.brightness), brightness_range.1.max(c.brightness));
            bass_range = (bass_range.0.min(c.bass), bass_range.1.max(c.bass));
            tail_range = (tail_range.0.min(c.tail), tail_range.1.max(c.tail));
            level_range = (level_range.0.min(c.level), level_range.1.max(c.level));
        }
        assert!(
            brightness_range.1 / brightness_range.0 > 8.0,
            "every program has the same spectrum: {brightness_range:?}"
        );
        assert!(
            bass_range.1 - bass_range.0 > 0.2,
            "every program has the same bass: {bass_range:?}"
        );
        assert!(
            tail_range.1 / tail_range.0.max(1.0e-6) > 5.0,
            "every program has the same envelope: {tail_range:?}"
        );
        assert!(
            level_range.1 / level_range.0 > 3.0,
            "every program is the same level: {level_range:?}"
        );
    }

    #[test]
    fn a_program_sounds_the_same_at_every_sample_rate() {
        // Aggregate fingerprints rather than samples: three C libraries'
        // worth of `exp` and `tan` disagree in the last bits, so what is
        // asserted is level, spectrum and shape rather than a hash.
        for index in [0usize, 15, 45, 73, 105, 164] {
            let fingerprint = |rate: f64| {
                let mut s = Teo5::new();
                s.init(rate, BLOCK);
                let (bank, program) = program_knobs(index);
                s.set_parameter(P_BANK, bank);
                s.set_parameter(P_PROGRAM, program);
                s.reset();
                let blocks = (2.0 * rate / BLOCK as f64) as usize;
                let out = render(&mut s, &[note_on(48, 100, 0)], blocks);
                let half = out.len() / 2;
                (
                    rms(&out),
                    brightness_below(&out, 3_000.0, rate),
                    rms(&out[half..]) / rms(&out[..half]).max(1.0e-30),
                )
            };
            let (level, bright, shape) = fingerprint(44_100.0);
            for rate in [22_050.0f64, 48_000.0, 96_000.0] {
                let (l, b, sh) = fingerprint(rate);
                let name = program_name(index);
                for (what, a, c) in
                    [("level", level, l), ("spectrum", bright, b), ("shape", shape, sh)]
                {
                    let error = (c - a).abs() / a.max(1.0e-12);
                    assert!(
                        error < 0.02,
                        "{name} at {rate} Hz differs in {what} by {:.1}%: {a:.5} against {c:.5}",
                        error * 100.0
                    );
                }
            }
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

    const SR_M: f64 = 44_100.0;

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
        for (label, at) in [("min", 0), ("p25", 64), ("median", 128), ("p75", 192), ("max", 255)] {
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
        println!("  before the saturator: {:.4}", crate::level::saturation_input(worst.0));
    }

    #[test]
    #[ignore]
    fn report_anchors() {
        println!(
            "{:>4} {:<20} {:>8} {:>9} {:>8} {:>6} {:>6} {:>7}",
            "idx", "name", "peak", "rms", "bright", "bass", "tail", "crest"
        );
        for index in [0usize, 15, 45, 53, 71, 73, 132, 164] {
            let mut s = fresh(index);
            let mut out = render_program(&mut s, &[48], 100, 150);
            out.extend_from_slice(&render(&mut s, &[note_off(48, 0)], 60));
            let quarter = out.len() / 4;
            let level = rms(&out);
            println!(
                "{index:>4} {:<20} {:>8.4} {:>9.5} {:>8.0} {:>6.2} {:>6.2} {:>7.2}",
                program_name(index),
                peak(&out),
                level,
                brightness(&out, SR_M),
                low_band(&out, 300.0, SR_M) / level.max(1.0e-30),
                rms(&out[out.len() - quarter..]) / rms(&out[..quarter]).max(1.0e-30),
                f64::from(peak(&out)) / level.max(1.0e-30),
            );
        }
    }

    #[test]
    #[ignore]
    fn report_bank_spread() {
        println!(
            "{:>4} {:<22} {:<11} {:>9} {:>8} {:>6} {:>6}",
            "idx", "name", "category", "rms", "bright", "bass", "tail"
        );
        for bank in 0..BANK_COUNT {
            for n in 0..3 {
                let index = bank * PROGRAMS_PER_BANK + n * 5 + 1;
                let mut s = fresh(index);
                let mut out = render_program(&mut s, &[48], 100, 150);
                out.extend_from_slice(&render(&mut s, &[note_off(48, 0)], 60));
                let quarter = out.len() / 4;
                let level = rms(&out);
                println!(
                    "{index:>4} {:<22} {:<11} {level:>9.5} {:>8.0} {:>6.2} {:>6.2}",
                    program_name(index),
                    program_category(index),
                    brightness(&out, SR_M),
                    low_band(&out, 300.0, SR_M) / level.max(1.0e-30),
                    rms(&out[out.len() - quarter..]) / rms(&out[..quarter]).max(1.0e-30),
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn report_rate_independence() {
        for index in [0usize, 15, 45, 73, 105, 164] {
            let fingerprint = |rate: f64| {
                let mut s = Teo5::new();
                s.init(rate, 256);
                let (bank, program) = program_knobs(index);
                s.set_parameter(P_BANK, bank);
                s.set_parameter(P_PROGRAM, program);
                s.reset();
                let blocks = (2.0 * rate / 256.0) as usize;
                let out = render(&mut s, &[note_on(48, 100, 0)], blocks);
                (rms(&out), brightness_below(&out, 3_000.0, rate))
            };
            let (level, bright) = fingerprint(44_100.0);
            let mut line = format!("{index:>4} {:<20}", program_name(index));
            for rate in [22_050.0f64, 48_000.0, 96_000.0] {
                let (l, b) = fingerprint(rate);
                line.push_str(&format!(
                    "  {rate:>6.0}: rms {:+.2}% bright {:+.2}%",
                    100.0 * (l / level - 1.0),
                    100.0 * (b / bright - 1.0)
                ));
            }
            println!("{line}");
        }
    }

    /// The morph, as numbers: what one sweep of the state knob does to a
    /// sawtooth's bass, to the partial parked on the corner, and to its
    /// treble, with the notch and with the band pass.
    #[test]
    #[ignore]
    fn report_filter_morph() {
        for (label, bandpass) in [("notch", 0.0f32), ("band pass", 1.0)] {
            println!("state sweep, {label} at the centre:");
            println!("{:>6} {:>10} {:>10} {:>10} {:>9}", "state", "h1", "h8 corner", "h40", "tilt dB");
            for (step, (low, corner, high)) in morph_sweep(bandpass).into_iter().enumerate() {
                println!(
                    "{:>6.3} {low:>10.6} {corner:>10.6} {high:>10.6} {:>9.1}",
                    step as f64 / 8.0,
                    db(high / low)
                );
            }
        }
    }

    /// Through-zero FM, as numbers: the carrier's mean frequency against the
    /// depth of the modulation, and how bright it gets on the way.
    #[test]
    #[ignore]
    fn report_through_zero() {
        let setup = with(
            &neutral(),
            &[(P_O1_KEY, 0.0), (P_O2_ON, 0.0), (P_CUTOFF, 1.0), (P_E2_ATTACK, 0.0)],
        );
        let carrier = raw::note_hz(OSC_NO_KEY_NOTE);
        println!("{:>6} {:>10} {:>9} {:>10}", "depth", "mean Hz", "drift %", "centroid");
        for depth in [0.0f32, 0.125, 0.25, 0.5, 0.75, 1.0] {
            let mut s = built(&with(&setup, &[(P_XMOD, depth)]));
            let out = render(&mut s, &[note_on(36, 100, 0)], 60);
            let modulator = raw::note_hz(36.0);
            let span = (out.len() as f64 / SR_M * modulator).floor() / modulator;
            let window = &out[..(span * SR_M) as usize];
            let hz = net_cycles(window) / span;
            println!(
                "{depth:>6.3} {hz:>10.2} {:>9.2} {:>10.0}",
                100.0 * (hz / carrier - 1.0),
                brightness(window, SR_M)
            );
        }
    }

    /// The worst panel a hand can reach, swept: where the peak lands and how
    /// much of the master bus's headroom the instrument can use on its own.
    #[test]
    #[ignore]
    fn report_ceiling() {
        let worst = worst_panel();
        let mut top = (0.0f32, String::new());
        for cutoff in [0.0f32, 0.2, 0.35, 0.5, 0.7, 1.0] {
            for state in [0.0f32, 0.5, 1.0] {
                for drive in [0.0f32, 0.3, 1.0] {
                    for note in [24u8, 36, 48, 60, 84] {
                        let mut s = built(&with(
                            &worst,
                            &[(P_CUTOFF, cutoff), (P_STATE, state), (P_OVERDRIVE, drive)],
                        ));
                        let peak = peak(&render(&mut s, &[note_on(note, 127, 0)], 60));
                        if peak > top.0 {
                            top = (
                                peak,
                                format!("cutoff {cutoff} state {state} drive {drive} note {note}"),
                            );
                        }
                    }
                }
            }
        }
        println!("worst panel: {:.4} ({})", top.0, top.1);
        println!("  master limiter ceiling 0.891, saturator knee 0.75");
        println!("  before the saturator: {:.4}", crate::level::saturation_input(top.0));
    }

    #[test]
    #[ignore]
    fn report_cost() {
        // Wall-clock cost of a five-voice chord, against the audio it makes,
        // and the Prophet-6 doing the same work for comparison.
        let mut s = Teo5::new();
        s.init(SR_M, 512);
        let mut left = vec![0.0f32; 512];
        let mut right = vec![0.0f32; 512];
        let events: Vec<MidiEvent> = [48u8, 52, 55, 59, 62]
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
            "TEO-5, five voices: {:.1} us a 512-sample block, {:.2}% of one core at 44.1 kHz",
            elapsed / f64::from(blocks) * 1.0e6,
            100.0 * elapsed / audio
        );

        let mut p6 = crate::prophet6::Prophet6::new();
        p6.init(SR_M, 512);
        let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
        p6.process(&[], &mut outs, &events);
        let start = std::time::Instant::now();
        for _ in 0..blocks {
            let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
            p6.process(&[], &mut outs, &[]);
        }
        let elapsed6 = start.elapsed().as_secs_f64();
        println!(
            "Prophet-6, five voices: {:.1} us a 512-sample block, {:.2}% of one core",
            elapsed6 / f64::from(blocks) * 1.0e6,
            100.0 * elapsed6 / audio
        );
        println!("  TEO-5 / Prophet-6 = {:.2}x", elapsed / elapsed6);
    }
}
