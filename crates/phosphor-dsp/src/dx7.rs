//! DX7-style 6-operator FM synthesizer.
//!
//! Authentic recreation of the Yamaha DX7's FM synthesis engine:
//! 6 sine-wave operators, 32 algorithms, 4-rate/4-level envelopes,
//! operator feedback, and the 256 factory voices decoded from the original
//! ROM cartridges.

use std::sync::OnceLock;

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

use crate::level::soft_saturate;

const MAX_VOICES: usize = 16;
const NUM_OPERATORS: usize = 6;
const TWO_PI: f64 = std::f64::consts::TAU;

/// Fixed headroom trim on the voice sum, applied after the gain knob.
///
/// Measured, not guessed — and the one instrument in the project whose trim is
/// sized by its **loudest** voice rather than by ordinary playing. That is a
/// deliberate exception, and the reason is the factory bank.
///
/// Measured on an eight-note chord at velocity 127, at the bounding stage, the
/// hand-voiced bank this replaced ran from 0.16 (Organ) to 1.03 (Timpani) — its
/// loudest patch 5.8 dB above its median. The factory set has almost exactly
/// the same median, 0.52, and runs from 0.08 (ROM1B GUITAR 4) to 1.65 (ROM3A
/// TIMPANI): 10.1 dB above the median, and 26 dB end to end. Eight notes
/// triggered on the same sample start with their operator phases reset
/// together, so they sum very nearly linearly — 7.3x a single note, not the
/// sqrt(8) = 2.8x uncorrelated voices would give.
///
/// One constant cannot serve both ends of that, so this one serves the loud
/// end. `SATURATION_KNEE`'s doc pins 1.07 as the input that maps to the master
/// limiter's −1 dBFS ceiling, and 1.65 x (0.1100 / 0.1738) = 1.04 lands TIMPANI
/// at 0.885 with 1.4 dB of saturation on its attack transient, with every other
/// voice under it.
///
/// What that costs: 3.97 dB on all 256 voices, which puts ordinary playing —
/// the voice the instrument loads with, a triad at velocity 100 — at −14.1
/// dBFS rather than the −12 the other instruments are trimmed to. The
/// alternative was letting 16 of the 256 voices past the ceiling by up to
/// 0.5 dB, and a peak past the ceiling on one track ducks *every* track
/// through the master limiter. A track fader can recover 4 dB; nothing
/// downstream can un-duck a mix.
///
/// It is a constant on purpose: dividing by the number of sounding voices
/// would pump as notes are released.
const OUTPUT_TRIM: f32 = 0.1100;

// ── Parameter indices ──
/// Which of the 32 voices of the selected cartridge is playing.
pub const P_PATCH: usize = 0;
/// Feedback trim. Not an absolute setting: it is a **relative offset** on the
/// patch's own feedback index, centred at 0.5. See [`resolve_feedback`].
pub const P_FEEDBACK: usize = 1;
/// Brightness trim. Also a **relative offset**, in dB on the modulator output
/// levels, centred at 0.5. See [`Dx7Synth::brightness`].
pub const P_BRIGHTNESS: usize = 2;
pub const P_ATTACK: usize = 3;
pub const P_DECAY: usize = 4;
pub const P_SUSTAIN: usize = 5;
pub const P_RELEASE: usize = 6;
pub const P_GAIN: usize = 7;
/// Which of the eight factory cartridges is loaded.
///
/// Appended after the gain knob rather than filed next to [`P_PATCH`], where it
/// belongs on the panel, because a session stores `synth_params` as a positional
/// list: inserting an index would load every saved value of every existing
/// session into the wrong parameter.
pub const P_BANK: usize = 8;
pub const PARAM_COUNT: usize = 9;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "patch", "feedback", "bright", "attack", "decay", "sustain", "release", "gain",
    "bank",
];

pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    // patch: voice 11 of the selected cartridge, which in ROM1A is E.PIANO 1 —
    // the sound the DX7 is remembered for, and the same electric piano the
    // instrument loaded with before the factory banks were wired up.
    DEFAULT_PATCH_KNOB,
    0.5,   // feedback trim: centred, i.e. exactly the patch's authored index
    0.5,   // brightness trim: centred, i.e. the modulator levels as authored
    0.3,   // attack time scale
    0.5,   // decay time scale
    0.7,   // sustain level scale
    0.3,   // release time scale
    0.75,  // gain
    0.0,   // bank: ROM1A
];

/// Knob position that selects voice 11 of a cartridge, in the middle of its
/// step. `knob_for(10, PATCH_COUNT)`, which is not a const fn.
const DEFAULT_PATCH_KNOB: f32 = 10.5 / PATCH_COUNT as f32;

// ── The factory banks ──

/// The eight factory ROM cartridge banks, in the order [`ROM`] stores them.
pub const BANK_NAMES: [&str; BANK_COUNT] = [
    "ROM1A", "ROM1B", "ROM2A", "ROM2B", "ROM3A", "ROM3B", "ROM4A", "ROM4B",
];
pub const BANK_COUNT: usize = 8;

/// Voices per bank. [`P_PATCH`] selects one of these; [`P_BANK`] picks the bank.
///
/// Two selectors rather than one 256-position knob because that is how the
/// instrument works — a cartridge and then a voice button — and because a single
/// knob would take 256 keypresses to walk end to end.
pub const PATCH_COUNT: usize = 32;

/// Every factory voice, across all eight banks.
pub const VOICE_COUNT: usize = BANK_COUNT * PATCH_COUNT;

/// One voice as a cartridge stores it: 128 bytes with several parameters packed
/// into shared bytes. See [`decode_voice`] for the layout.
const PACKED_VOICE: usize = 128;

/// The four factory cartridges: eight banks of 32 voices, 4096 bytes each.
///
/// This is the payload of the eight original bank dumps concatenated, with the
/// sysex header, terminator and checksum stripped — every bank was verified
/// against its own 7-bit checksum before extraction. Kept as bytes and unpacked
/// at startup rather than transcribed into source: 32 KB of the machine's own
/// data plus a decoder is exact and testable, where 5,000 lines of generated
/// struct literals is neither.
const ROM: &[u8; VOICE_COUNT * PACKED_VOICE] = include_bytes!("dx7_roms.bin");

/// How an operator gets its pitch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum OpFreqMode {
    /// Frequency tracks the played note, multiplied by the dialled ratio.
    #[default]
    Ratio,
    /// Absolute frequency, independent of the played note. Only the low two bits
    /// of `coarse` and the whole of `fine` are used, giving 1 Hz to 9.772 kHz.
    Fixed,
}

/// Keyboard level scaling curve, as stored in the patch: 0 = -LIN, 1 = -EXP,
/// 2 = +EXP, 3 = +LIN. "Negative" curves cut as you move away from the
/// breakpoint, positive ones boost.
///
/// All four appear in the factory banks, and 166 of the 256 voices set a scaling
/// depth deep enough for the choice to matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScaleCurve {
    LinNeg,
    ExpNeg,
    ExpPos,
    LinPos,
}

impl ScaleCurve {
    /// The two bits a packed voice stores this in.
    fn from_bits(bits: u8) -> Self {
        match bits & 3 {
            0 => Self::LinNeg,
            1 => Self::ExpNeg,
            2 => Self::ExpPos,
            _ => Self::LinPos,
        }
    }

    /// Curves 0 and 3 are straight lines in the level domain; 1 and 2 read the
    /// `EXP_SCALE_DATA` table instead.
    fn is_linear(self) -> bool {
        matches!(self, Self::LinNeg | Self::LinPos)
    }

    /// Curves 0 and 1 subtract level; 2 and 3 add it.
    fn is_negative(self) -> bool {
        matches!(self, Self::LinNeg | Self::ExpNeg)
    }
}

/// The six LFO shapes, in the order the patch byte numbers them.
///
/// All six are used by the factory banks: 129 voices ask for the sine, 90 for
/// the triangle and the remaining 37 are spread across the other four.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LfoWave {
    /// Rises from trough to peak over the first half cycle and falls back over
    /// the second. The DX7's own INIT VOICE shape.
    #[default]
    Triangle,
    SawDown,
    SawUp,
    Square,
    /// The only shape whose phase 0 sits at the centre rather than the trough.
    Sine,
    /// Stepped random, one new value per cycle, from an 8-bit LCG.
    SampleHold,
}

impl LfoWave {
    /// The three bits a packed voice stores this in. Only six of the eight
    /// values name a shape; the ROM never uses the other two, and a cartridge
    /// that did would land on the INIT VOICE triangle.
    fn from_bits(bits: u8) -> Self {
        match bits & 7 {
            1 => Self::SawDown,
            2 => Self::SawUp,
            3 => Self::Square,
            4 => Self::Sine,
            5 => Self::SampleHold,
            _ => Self::Triangle,
        }
    }
}

/// Patch-level LFO settings.
///
/// The DX7 has exactly one LFO for the whole instrument, so everything here is a
/// property of the patch rather than of an operator. The single per-operator part
/// of the LFO's reach is [`OpPreset::amp_mod_sens`].
#[derive(Debug, Clone, Copy)]
struct LfoPreset {
    /// Speed, 0-99. Indexes [`LFO_RATE_HZ`]; not a linear scale.
    speed: u8,
    /// Delay, 0-99. 0 is no delay at all; 99 is a little over three seconds.
    delay: u8,
    /// Pitch modulation depth, 0-99. This multiplies with `pitch_mod_sens`, so
    /// either one at zero means no vibrato whatsoever.
    pmd: u8,
    /// Amplitude modulation depth, 0-99. Needs a nonzero per-operator
    /// [`OpPreset::amp_mod_sens`] as well before it reaches anything.
    amd: u8,
    waveform: LfoWave,
    /// Key sync. When set, a key-down restarts the LFO phase, so every note hears
    /// the vibrato from the same point. The delay restarts either way.
    sync: bool,
    /// Pitch modulation sensitivity, 0-7. Patch-global, not per operator.
    pitch_mod_sens: u8,
}

/// Pitch-mod sensitivity, 0-7, as the hardware weights it. Not a linear ramp —
/// the top two steps are worth more than the bottom five together.
const PITCH_MOD_SENS: [u8; 8] = [0, 10, 20, 33, 55, 92, 153, 255];

/// Per-operator amplitude-mod sensitivity as a fraction of full depth. The patch
/// stores 0-3 but the steps are uneven: the hardware's weights are 0, 66, 109 and
/// 255 out of 255.
const AMP_MOD_SENS: [f64; 4] = [0.0, 66.0 / 255.0, 109.0 / 255.0, 1.0];

impl LfoPreset {
    /// The DX7's own INIT VOICE LFO: a mid-speed triangle with key sync on and
    /// both depths at zero, which makes it completely inert.
    const fn neutral() -> Self {
        Self {
            speed: 35,
            delay: 0,
            pmd: 0,
            amd: 0,
            waveform: LfoWave::Triangle,
            sync: true,
            pitch_mod_sens: 3,
        }
    }

    /// Octaves of pitch deviation at full LFO swing, once the delay has opened.
    ///
    /// `(pmd * 165) >> 6` is the hardware's rescale of the 0-99 patch value into
    /// its internal 0-255 range; the sensitivity weighting is a second 0-255
    /// factor, and the two simply multiply. Full depth at full sensitivity comes
    /// out at 255 * 255 / 65536 = 0.992 octaves either side of the note.
    fn pitch_mod_depth(self) -> f64 {
        let depth = (u32::from(self.pmd.min(99)) * 165) >> 6;
        let sens = u32::from(PITCH_MOD_SENS[usize::from(self.pitch_mod_sens.min(7))]);
        f64::from(depth * sens) / 65536.0
    }

    /// Amplitude modulation depth as a 0..1 fraction, before the per-operator
    /// sensitivity is applied. Same 0-99 to 0-255 rescale as the pitch depth.
    fn amp_mod_depth(self) -> f64 {
        f64::from((u32::from(self.amd.min(99)) * 165) >> 6) / 256.0
    }
}

/// Patch-level pitch envelope settings: one 4-rate / 4-level EG for the whole
/// voice, not one per operator.
#[derive(Debug, Clone, Copy)]
struct PitchEgPreset {
    rates: [u8; 4],
    /// 0-99, with **50 as the neutral centre**. Above 50 bends sharp, below bends
    /// flat; see [`pitch_env_offset`] for the (very non-linear) conversion.
    levels: [u8; 4],
}

impl PitchEgPreset {
    /// Flat: every level at the neutral 50 and every rate at maximum, so the
    /// envelope can never produce a pitch offset. This is the DX7's INIT VOICE.
    const fn neutral() -> Self {
        Self { rates: [99, 99, 99, 99], levels: [50, 50, 50, 50] }
    }
}

/// Per-operator preset data. This is the full DX7 operator parameter set minus
/// the LFO speed/depth/shape, which is patch-global and lives on
/// [`PatchPreset::lfo`].
///
/// Presets are written as a delta from [`OpPreset::neutral`], so a literal only
/// spells out the fields it actually changes.
#[derive(Debug, Clone, Copy)]
struct OpPreset {
    /// Coarse frequency, 0-31. 0 means ratio 0.5; N >= 1 means ratio N.
    coarse: u8,
    /// Fine frequency, 0-99: multiplies the coarse ratio by `1 + fine/100`.
    /// Coarse and fine together are the only ratios a DX7 can actually dial.
    fine: u8,
    /// Detune, 0-14 with 7 = centre. Not a fixed number of cents — see
    /// [`OpPreset::detune_factor`].
    detune: u8,
    /// Ratio or fixed-frequency mode.
    mode: OpFreqMode,
    output_level: u8,  // 0-99
    rates: [u8; 4],    // R1-R4
    levels: [u8; 4],   // L1-L4
    vel_sens: u8,      // 0-7
    /// Keyboard level scaling breakpoint. This is the raw patch parameter 0-99
    /// (0 = A-1), *not* a MIDI note number; [`scale_level`] reconciles the two.
    break_point: u8,
    /// Level scaling depth below / above the breakpoint, 0-99.
    left_depth: u8,
    right_depth: u8,
    /// Level scaling curve below / above the breakpoint.
    left_curve: ScaleCurve,
    right_curve: ScaleCurve,
    /// Keyboard rate scaling, 0-7: how much faster the envelope runs as you play
    /// further up the keyboard.
    rate_scaling: u8,
    /// LFO amplitude modulation sensitivity, 0-3. This is the gate on the patch's
    /// amplitude modulation reaching this operator: at 0 the LFO cannot touch it
    /// no matter how high AMD is set. The steps are uneven — see
    /// [`AMP_MOD_SENS`].
    amp_mod_sens: u8,
}

impl OpPreset {
    /// The operator every preset is written as a delta from: ratio 1.0, centre
    /// detune, and no keyboard scaling, velocity response or amplitude
    /// modulation of any kind. All of that is deliberately inert — an operator
    /// built from this alone behaves exactly as one did before keyboard scaling
    /// existed.
    const fn neutral() -> Self {
        Self {
            coarse: 1,
            fine: 0,
            detune: 7,
            mode: OpFreqMode::Ratio,
            output_level: 99,
            rates: [99, 99, 99, 99],
            levels: [99, 99, 99, 0],
            vel_sens: 0,
            // `scale_level` hinges at `break_point + 17`, so 43 puts the hinge
            // on middle C. Inert while both depths are 0, but it means a patch
            // that sets only a depth gets scaling hinged where a player expects.
            break_point: 43,
            left_depth: 0,
            right_depth: 0,
            left_curve: ScaleCurve::LinNeg,
            right_curve: ScaleCurve::LinNeg,
            rate_scaling: 0,
            amp_mod_sens: 0,
        }
    }
}

impl Default for OpPreset {
    fn default() -> Self {
        Self::neutral()
    }
}

/// Full patch preset.
#[derive(Debug, Clone, Copy)]
struct PatchPreset {
    algorithm: u8,
    feedback: u8,  // 0-7
    ops: [OpPreset; NUM_OPERATORS],
    /// The one LFO, shared by every voice.
    lfo: LfoPreset,
    /// The one pitch envelope, applied to the whole voice.
    pitch_eg: PitchEgPreset,
    /// Keyboard transpose, 0-48 with **24 as the centre**, i.e. -24 to +24
    /// semitones on the played note. 94 of the 256 factory voices are off
    /// centre — mostly basses an octave down and mallets an octave up — so this
    /// is not an ornament: without it 37% of the bank plays in the wrong octave.
    transpose: u8,
    /// Oscillator key sync. When set, key-down resets every operator's phase, so
    /// each note is bit-for-bit the same waveform. When clear the operators keep
    /// running and the note starts wherever they happen to be, which is what 74
    /// of the factory voices ask for.
    osc_key_sync: bool,
    /// The voice's name as the cartridge stores it: 10 bytes, space padded.
    /// Held as bytes rather than a `&str` because a preset is `Copy` plain data
    /// and the ROM's own padding is not worth a second representation.
    name: [u8; NAME_LEN],
}

/// Length of a voice name in the packed format.
const NAME_LEN: usize = 10;

/// Transpose value that means "no transposition".
const TRANSPOSE_CENTRE: i32 = 24;

impl PatchPreset {
    /// The DX7's INIT VOICE, and the starting point a decoded voice overwrites.
    /// Both modulation sections are inert: LFO depths at zero and every pitch EG
    /// level at the neutral 50.
    const fn neutral() -> Self {
        Self {
            algorithm: 1,
            feedback: 0,
            ops: [OpPreset::neutral(); NUM_OPERATORS],
            lfo: LfoPreset::neutral(),
            pitch_eg: PitchEgPreset::neutral(),
            transpose: 24,
            osc_key_sync: true,
            name: *b"INIT VOICE",
        }
    }

    /// The voice's name, trailing padding removed.
    ///
    /// Cannot fail: [`decode_voice`] replaces anything outside printable ASCII
    /// as it copies, so the stored bytes are always valid UTF-8.
    fn name(&self) -> &str {
        std::str::from_utf8(&self.name).unwrap_or("").trim_end()
    }
}

// ── Decoding the ROM ──

/// Clamp a ROM byte into the 0-99 domain almost every DX7 patch parameter
/// shares.
///
/// Four values in the factory set are genuinely out of that range — `TIMPANI`
/// and `HORNS` carry an EG value of 127, `E.GRAND 1` an EG rate of 127, and
/// `60-S ORGAN` a frequency fine of 100. They are clamped here rather than
/// edited in the data, which is both what the hardware does with them and the
/// only way the file stays the bytes the cartridges shipped with.
fn param99(byte: u8) -> u8 {
    byte.min(99)
}

/// Decode one packed voice.
///
/// Bytes 0-101 are six operators of 17 bytes each stored **OP6 first**, so the
/// loop reads them backwards. Within an operator:
///
/// | byte | contents |
/// |------|----------|
/// | 0-3 | EG rates R1-R4 |
/// | 4-7 | EG levels L1-L4 |
/// | 8 | break point |
/// | 9-10 | left / right depth |
/// | 11 | bits 3-2 right curve, bits 1-0 left curve |
/// | 12 | bits 6-3 detune, bits 2-0 rate scaling |
/// | 13 | bits 4-2 velocity sensitivity, bits 1-0 amp mod sensitivity |
/// | 14 | output level |
/// | 15 | bits 5-1 frequency coarse, bit 0 oscillator mode |
/// | 16 | frequency fine |
///
/// and then, for the voice as a whole:
///
/// | byte | contents |
/// |------|----------|
/// | 102-109 | pitch EG rates 1-4, then levels 1-4 |
/// | 110 | bits 4-0 algorithm, 0-based |
/// | 111 | bit 3 oscillator key sync, bits 2-0 feedback |
/// | 112-115 | LFO speed, delay, pitch mod depth, amp mod depth |
/// | 116 | bits 6-4 pitch mod sensitivity, bits 3-1 waveform, bit 0 LFO key sync |
/// | 117 | transpose |
/// | 118-127 | name |
fn decode_voice(packed: &[u8]) -> PatchPreset {
    // Every caller hands this a `chunks_exact(PACKED_VOICE)` chunk, so the
    // indexing below cannot run off the end.
    debug_assert_eq!(packed.len(), PACKED_VOICE);

    let mut ops = [OpPreset::neutral(); NUM_OPERATORS];
    for (i, op) in ops.iter_mut().enumerate() {
        let b = &packed[(NUM_OPERATORS - 1 - i) * 17..];
        *op = OpPreset {
            rates: [param99(b[0]), param99(b[1]), param99(b[2]), param99(b[3])],
            levels: [param99(b[4]), param99(b[5]), param99(b[6]), param99(b[7])],
            break_point: param99(b[8]),
            left_depth: param99(b[9]),
            right_depth: param99(b[10]),
            right_curve: ScaleCurve::from_bits(b[11] >> 2),
            left_curve: ScaleCurve::from_bits(b[11]),
            detune: ((b[12] >> 3) & 15).min(14),
            rate_scaling: b[12] & 7,
            vel_sens: (b[13] >> 2) & 7,
            amp_mod_sens: b[13] & 3,
            output_level: param99(b[14]),
            coarse: (b[15] >> 1) & 31,
            mode: if b[15] & 1 == 0 { OpFreqMode::Ratio } else { OpFreqMode::Fixed },
            fine: param99(b[16]),
        };
    }

    let mut name = [b' '; NAME_LEN];
    for (dst, &src) in name.iter_mut().zip(&packed[118..128]) {
        // The DX7's character set has a handful of symbols outside ASCII that a
        // terminal cannot draw; none of the factory names use one, but a
        // cartridge that did must not put an unprintable byte on the screen.
        *dst = if (0x20..0x7F).contains(&src) { src } else { b'?' };
    }

    PatchPreset {
        // Stored 0-based, displayed 1-32.
        algorithm: (packed[110] & 31) + 1,
        feedback: packed[111] & 7,
        ops,
        lfo: LfoPreset {
            speed: param99(packed[112]),
            delay: param99(packed[113]),
            pmd: param99(packed[114]),
            amd: param99(packed[115]),
            pitch_mod_sens: (packed[116] >> 4) & 7,
            waveform: LfoWave::from_bits(packed[116] >> 1),
            sync: packed[116] & 1 != 0,
        },
        pitch_eg: PitchEgPreset {
            rates: [param99(packed[102]), param99(packed[103]),
                    param99(packed[104]), param99(packed[105])],
            levels: [param99(packed[106]), param99(packed[107]),
                     param99(packed[108]), param99(packed[109])],
        },
        transpose: packed[117].min(48),
        osc_key_sync: packed[111] & 0x08 != 0,
        name,
    }
}

/// The 256 factory voices, unpacked once for the whole process.
///
/// Decoding is 32 KB of bit-shifting — nothing, but it happens exactly once and
/// never on the audio thread. Every instance borrows the same table rather than
/// carrying its own 40 KB copy, and [`Dx7Synth::new`] touches it so the one-time
/// work lands on whichever thread built the instrument.
fn presets() -> &'static [PatchPreset; VOICE_COUNT] {
    static DECODED: OnceLock<Box<[PatchPreset; VOICE_COUNT]>> = OnceLock::new();
    DECODED.get_or_init(|| {
        let mut bank = Box::new([PatchPreset::neutral(); VOICE_COUNT]);
        for (slot, packed) in bank.iter_mut().zip(ROM.chunks_exact(PACKED_VOICE)) {
            *slot = decode_voice(packed);
        }
        bank
    })
}

// ── Voice selection ──

/// One knob into one of `count` equal steps.
///
/// Total by construction: `params` is public, so the knob can arrive as
/// anything at all. The float-to-int cast saturates in both directions and
/// turns NaN into zero, so every input lands on a real voice.
fn selector(value: f32, count: usize) -> usize {
    ((value * (count as f32 - 0.01)) as usize).min(count - 1)
}

/// The knob position in the middle of step `index` of `count` — the one
/// position in the step that no amount of float rounding can push into a
/// neighbouring step. The inverse of [`selector`].
fn knob_for(index: usize, count: usize) -> f32 {
    (index as f32 + 0.5) / count as f32
}

/// Which of the eight cartridges the bank knob is pointing at.
pub fn bank_index(value: f32) -> usize {
    selector(value, BANK_COUNT)
}

/// Which voice of the selected cartridge the patch knob is pointing at.
pub fn patch_index(value: f32) -> usize {
    selector(value, PATCH_COUNT)
}

/// The absolute voice number, 0-255, that the two knobs select together.
pub fn voice_index(bank: f32, patch: f32) -> usize {
    bank_index(bank) * PATCH_COUNT + patch_index(patch)
}

/// The `(bank, patch)` knob positions that select voice number `voice`.
///
/// The inverse of [`voice_index`], for anything that walks the factory set by
/// number — a level sweep, a headroom test — rather than by knob.
pub fn voice_knobs(voice: usize) -> (f32, f32) {
    let voice = voice.min(VOICE_COUNT - 1);
    (
        knob_for(voice / PATCH_COUNT, BANK_COUNT),
        knob_for(voice % PATCH_COUNT, PATCH_COUNT),
    )
}

/// A factory voice's ROM name, trailing padding removed.
///
/// 194 of the 256 names are distinct; the repeats are the cartridges' own —
/// several voices appear on more than one card — which is why the bank selector
/// is part of the display rather than a hidden index.
pub fn voice_name(voice: usize) -> &'static str {
    presets()[voice.min(VOICE_COUNT - 1)].name()
}

/// Which parameter indices are discrete selectors (rendered as labels, not bars).
pub fn is_discrete(index: usize) -> bool {
    matches!(index, P_PATCH | P_BANK)
}

/// The knob position one step up or down from `value`, for one of the two
/// selectors. Anything else is left alone.
///
/// Steps by *index* rather than by adding a fraction of the travel. Adding
/// 1/32 of the range 32 times does not arrive at 1.0 — the error is a few ulps
/// either way, and a step boundary missed by one ulp is a keypress that visibly
/// does nothing.
pub fn step_discrete(index: usize, value: f32, up: bool) -> f32 {
    let count = match index {
        P_BANK => BANK_COUNT,
        P_PATCH => PATCH_COUNT,
        _ => return value,
    };
    let current = selector(value, count);
    knob_for(
        if up { (current + 1).min(count - 1) } else { current.saturating_sub(1) },
        count,
    )
}

/// Label for a discrete selector.
///
/// Takes the whole parameter block rather than one value, unlike the other
/// instruments, because the two selectors are one control between them: the
/// voice name depends on the cartridge as much as on the voice button.
pub fn discrete_label(params: &[f32], index: usize) -> Option<&'static str> {
    let bank = params.get(P_BANK).copied().unwrap_or(0.0);
    let patch = params.get(P_PATCH).copied().unwrap_or(0.0);
    match index {
        P_PATCH => Some(voice_name(voice_index(bank, patch))),
        P_BANK => Some(BANK_NAMES[bank_index(bank)]),
        _ => None,
    }
}

// ── Algorithm routing ──

/// Defines which operators modulate which, and which are carriers.
/// For each operator (index 0-5 = op 1-6), lists indices of operators it modulates.
/// An empty target list means it's a carrier.
struct AlgorithmDef {
    /// For each op: which ops does it modulate? Empty = carrier.
    /// modulates[5] = vec![4,3,2] means op6 modulates ops 5,4,3.
    modulates: [&'static [usize]; NUM_OPERATORS],
    /// Which operators are carriers (output to audio).
    carriers: &'static [usize],
    /// Operator whose output is tapped for feedback.
    feedback_src: usize,
    /// Operator whose phase the feedback signal bends.
    /// Equal to `feedback_src` for self-feedback (30 of 32 algorithms).
    feedback_dst: usize,
}

/// Get the algorithm definition for a given algorithm number (1-32).
/// Decoded from the Yamaha DX7 algorithm table as encoded in
/// Dexed/msfa `FmCore::algorithms[32]` (fm_core.cc). Operator indices are
/// 0-based: index 0 = OP1 ... index 5 = OP6.
///
/// Algorithms 4 and 6 use a multi-operator feedback loop on the real
/// hardware (OP4→OP6 and OP5→OP6 respectively) rather than self-feedback;
/// `feedback_src` / `feedback_dst` model that directly.
fn algorithm(num: u8) -> AlgorithmDef {
    match num {
        // Alg 1: carriers 1,3 | 2→1 4→3 5→4 6→5 | fb 6→6
        1 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[3], &[4]],
            carriers: &[0, 2],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 2: carriers 1,3 | 2→1 4→3 5→4 6→5 | fb 2→2
        2 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[3], &[4]],
            carriers: &[0, 2],
            feedback_src: 1,
            feedback_dst: 1,
        },
        // Alg 3: carriers 1,4 | 2→1 3→2 5→4 6→5 | fb 6→6
        3 => AlgorithmDef {
            modulates: [&[], &[0], &[1], &[], &[3], &[4]],
            carriers: &[0, 3],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 4: carriers 1,4 | 2→1 3→2 5→4 6→5 | fb 4→6  [true HW loop]
        4 => AlgorithmDef {
            modulates: [&[], &[0], &[1], &[], &[3], &[4]],
            carriers: &[0, 3],
            feedback_src: 3,
            feedback_dst: 5,
        },
        // Alg 5: carriers 1,3,5 | 2→1 4→3 6→5 | fb 6→6  — THE classic
        5 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[], &[4]],
            carriers: &[0, 2, 4],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 6: carriers 1,3,5 | 2→1 4→3 6→5 | fb 5→6  [true HW loop]
        6 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[], &[4]],
            carriers: &[0, 2, 4],
            feedback_src: 4,
            feedback_dst: 5,
        },
        // Alg 7: carriers 1,3 | 2→1 4→3 5→3 6→5 | fb 6→6
        7 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[2], &[4]],
            carriers: &[0, 2],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 8: carriers 1,3 | 2→1 4→3 5→3 6→5 | fb 4→4
        8 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[2], &[4]],
            carriers: &[0, 2],
            feedback_src: 3,
            feedback_dst: 3,
        },
        // Alg 9: carriers 1,3 | 2→1 4→3 5→3 6→5 | fb 2→2
        9 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[2], &[4]],
            carriers: &[0, 2],
            feedback_src: 1,
            feedback_dst: 1,
        },
        // Alg 10: carriers 1,4 | 2→1 3→2 5→4 6→4 | fb 3→3
        10 => AlgorithmDef {
            modulates: [&[], &[0], &[1], &[], &[3], &[3]],
            carriers: &[0, 3],
            feedback_src: 2,
            feedback_dst: 2,
        },
        // Alg 11: carriers 1,4 | 2→1 3→2 5→4 6→4 | fb 6→6
        11 => AlgorithmDef {
            modulates: [&[], &[0], &[1], &[], &[3], &[3]],
            carriers: &[0, 3],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 12: carriers 1,3 | 2→1 4→3 5→3 6→3 | fb 2→2
        12 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[2], &[2]],
            carriers: &[0, 2],
            feedback_src: 1,
            feedback_dst: 1,
        },
        // Alg 13: carriers 1,3 | 2→1 4→3 5→3 6→3 | fb 6→6
        13 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[2], &[2]],
            carriers: &[0, 2],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 14: carriers 1,3 | 2→1 4→3 5→4 6→4 | fb 6→6
        14 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[3], &[3]],
            carriers: &[0, 2],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 15: carriers 1,3 | 2→1 4→3 5→4 6→4 | fb 2→2
        15 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[3], &[3]],
            carriers: &[0, 2],
            feedback_src: 1,
            feedback_dst: 1,
        },
        // Alg 16: carriers 1 | 2→1 3→1 4→3 5→1 6→5 | fb 6→6
        16 => AlgorithmDef {
            modulates: [&[], &[0], &[0], &[2], &[0], &[4]],
            carriers: &[0],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 17: carriers 1 | 2→1 3→1 4→3 5→1 6→5 | fb 2→2
        17 => AlgorithmDef {
            modulates: [&[], &[0], &[0], &[2], &[0], &[4]],
            carriers: &[0],
            feedback_src: 1,
            feedback_dst: 1,
        },
        // Alg 18: carriers 1 | 2→1 3→1 4→1 5→4 6→5 | fb 3→3
        18 => AlgorithmDef {
            modulates: [&[], &[0], &[0], &[0], &[3], &[4]],
            carriers: &[0],
            feedback_src: 2,
            feedback_dst: 2,
        },
        // Alg 19: carriers 1,4,5 | 2→1 3→2 6→5 6→4 | fb 6→6
        19 => AlgorithmDef {
            modulates: [&[], &[0], &[1], &[], &[], &[4, 3]],
            carriers: &[0, 3, 4],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 20: carriers 1,2,4 | 3→2 3→1 5→4 6→4 | fb 3→3
        20 => AlgorithmDef {
            modulates: [&[], &[], &[1, 0], &[], &[3], &[3]],
            carriers: &[0, 1, 3],
            feedback_src: 2,
            feedback_dst: 2,
        },
        // Alg 21: carriers 1,2,4,5 | 3→2 3→1 6→5 6→4 | fb 3→3
        21 => AlgorithmDef {
            modulates: [&[], &[], &[1, 0], &[], &[], &[4, 3]],
            carriers: &[0, 1, 3, 4],
            feedback_src: 2,
            feedback_dst: 2,
        },
        // Alg 22: carriers 1,3,4,5 | 2→1 6→5 6→4 6→3 | fb 6→6
        22 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[], &[], &[4, 3, 2]],
            carriers: &[0, 2, 3, 4],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 23: carriers 1,2,4,5 | 3→2 6→5 6→4 | fb 6→6
        23 => AlgorithmDef {
            modulates: [&[], &[], &[1], &[], &[], &[4, 3]],
            carriers: &[0, 1, 3, 4],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 24: carriers 1,2,3,4,5 | 6→5 6→4 6→3 | fb 6→6
        24 => AlgorithmDef {
            modulates: [&[], &[], &[], &[], &[], &[4, 3, 2]],
            carriers: &[0, 1, 2, 3, 4],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 25: carriers 1,2,3,4,5 | 6→5 6→4 | fb 6→6
        25 => AlgorithmDef {
            modulates: [&[], &[], &[], &[], &[], &[4, 3]],
            carriers: &[0, 1, 2, 3, 4],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 26: carriers 1,2,4 | 3→2 5→4 6→4 | fb 6→6
        26 => AlgorithmDef {
            modulates: [&[], &[], &[1], &[], &[3], &[3]],
            carriers: &[0, 1, 3],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 27: carriers 1,2,4 | 3→2 5→4 6→4 | fb 3→3
        27 => AlgorithmDef {
            modulates: [&[], &[], &[1], &[], &[3], &[3]],
            carriers: &[0, 1, 3],
            feedback_src: 2,
            feedback_dst: 2,
        },
        // Alg 28: carriers 1,3,6 | 2→1 4→3 5→4 | fb 5→5
        28 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[3], &[]],
            carriers: &[0, 2, 5],
            feedback_src: 4,
            feedback_dst: 4,
        },
        // Alg 29: carriers 1,2,3,5 | 4→3 6→5 | fb 6→6
        29 => AlgorithmDef {
            modulates: [&[], &[], &[], &[2], &[], &[4]],
            carriers: &[0, 1, 2, 4],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 30: carriers 1,2,3,6 | 4→3 5→4 | fb 5→5
        30 => AlgorithmDef {
            modulates: [&[], &[], &[], &[2], &[3], &[]],
            carriers: &[0, 1, 2, 5],
            feedback_src: 4,
            feedback_dst: 4,
        },
        // Alg 31: carriers 1,2,3,4,5 | 6→5 | fb 6→6
        31 => AlgorithmDef {
            modulates: [&[], &[], &[], &[], &[], &[4]],
            carriers: &[0, 1, 2, 3, 4],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Alg 32: carriers 1,2,3,4,5,6 | additive | fb 6→6
        32 => AlgorithmDef {
            modulates: [&[], &[], &[], &[], &[], &[]],
            carriers: &[0, 1, 2, 3, 4, 5],
            feedback_src: 5,
            feedback_dst: 5,
        },
        // Out-of-range algorithm numbers fall back to alg 1.
        _ => algorithm(1),
    }
}

// ── Operator frequency ──
//
// The hardware works in a log domain here too, with `1 << 24` counting one
// octave. Ratio mode is a pure multiply once you unpack it — the coarse table is
// `log2` of the ratio and the fine term is `log2(1 + fine/100)` — so it is done
// in the linear domain below, which keeps a dialled ratio exact instead of
// rounding it to the hardware's 7e-5 of a cent. Fixed mode has no such
// simplification and is built from the integer log-domain expression directly.

impl OpPreset {
    /// This operator's frequency in Hz for a given note.
    fn frequency(&self, note: u8) -> f64 {
        match self.mode {
            OpFreqMode::Ratio => note_to_freq(note) * self.detune_factor(note) * self.ratio(),
            OpFreqMode::Fixed => self.fixed_frequency(),
        }
    }

    /// The dialled ratio: coarse 0 is 0.5, coarse N is N, and fine multiplies by
    /// `1 + fine/100`. Nothing outside this grid is reachable on a real DX7.
    fn ratio(&self) -> f64 {
        let coarse = self.coarse & 31;
        let base = if coarse == 0 { 0.5 } else { f64::from(coarse) };
        base * (1.0 + f64::from(self.fine.min(99)) / 100.0)
    }

    /// Detune as a frequency multiplier, 1.0 at the centre setting of 7.
    ///
    /// Detune is applied to the note's log frequency *before* the coarse/fine
    /// multiply, and its size carries an `exp(-0.396 * octaves)` term, so it is
    /// not a fixed number of cents: 0.97 cents per step at A440, 2.46 down at
    /// C1. That frequency dependence is the whole character of DX7 detuning —
    /// two operators an octave apart with the same detune setting do not beat at
    /// the same rate.
    fn detune_factor(&self, note: u8) -> f64 {
        let steps = i32::from(self.detune.min(14)) - 7;
        if steps == 0 { return 1.0; }
        let octaves = note_to_freq(note).log2();
        let per_step = 0.0209 * (-0.396 * octaves).exp() / 7.0;
        f64::exp2(per_step * octaves * f64::from(steps))
    }

    /// Fixed mode: an absolute frequency with no relation to the played note,
    /// from the low two bits of coarse plus fine. That gives a four-decade sweep,
    /// `10^(x/100)` Hz for x in 0..=399, i.e. 1 Hz to 9.772 kHz. Detune only
    /// moves it *up* here, and only from the centre setting — a hardware quirk,
    /// not an oversight.
    fn fixed_frequency(&self) -> f64 {
        let x = i64::from(self.coarse & 3) * 100 + i64::from(self.fine.min(99));
        let mut logfreq = (4_458_616 * x) >> 3;
        let detune = i64::from(self.detune.min(14));
        if detune > 7 {
            logfreq += 13_457 * (detune - 7);
        }
        f64::exp2(logfreq as f64 / f64::from(1 << 24))
    }
}

// ── The level domain ──
//
// The DX7 does all of its amplitude work in a log domain. The EGS chip stores a
// 12-bit gain whose unit is 1/256 of an octave of amplitude, and every stage of
// the pipeline — operator output level, keyboard scaling, velocity, the envelope
// itself — is an *addition* in that domain. Only the final hand-off to the
// operator is linear. Everything below is expressed as attenuation in dB below
// full scale, so 0.0 is wide open and larger numbers are quieter.

/// One EGS level unit in dB: 1/256 of an octave of amplitude.
const DB_PER_UNIT: f64 = 6.020_599_913_279_624 / 256.0;

/// dB per step of the 0-127 `scaleoutlevel` domain. This is the `<< 5` that the
/// EGS applies to the coarse level before the fine (velocity) offset is added.
const LEVEL_STEP_DB: f64 = 32.0 * DB_PER_UNIT;

/// Attenuation of a fully closed level: `scaleoutlevel` 0 sits 127 steps below
/// `scaleoutlevel` 127, which is level 99.
const SILENCE_DB: f64 = 127.0 * LEVEL_STEP_DB;

/// Number of headroom bands the attack increment is scaled by. The hardware
/// divides its log range into 17 slices and multiplies the attack step by the
/// number of whole slices still to climb.
const HEADROOM_BANDS: f64 = 17.0;

/// Where key-down snaps the envelope before the attack ramp starts: 1716 out of
/// the 4352-unit log range, i.e. 39.4% of the way up from silence.
const ATTACK_JUMP_DB: f64 = SILENCE_DB * (1.0 - 1716.0 / 4352.0);

/// EGS output-level curve. Linear at 0.7526 dB per unit above 20, table-driven
/// (and steeper) below, so the bottom of the range collapses toward silence much
/// faster than a straight line would.
fn scaleoutlevel(level: u8) -> i32 {
    const LEVEL_LUT: [i32; 20] = [
        0, 5, 9, 13, 17, 20, 23, 25, 27, 29, 31, 33, 35, 37, 39, 41, 42, 43, 45, 46,
    ];
    let l = i32::from(level.min(99));
    if l >= 20 { 28 + l } else { LEVEL_LUT[l as usize] }
}

/// Convert DX7 level (0-99) to attenuation in dB below full scale.
fn dx_level_to_atten_db(level: u8) -> f64 {
    f64::from(127 - scaleoutlevel(level)) * LEVEL_STEP_DB
}

/// Convert an attenuation in dB to a linear gain. Negative attenuation (which a
/// velocity boost can produce) legitimately gives a gain above unity.
fn atten_to_gain(atten_db: f64) -> f64 {
    10.0f64.powf(-atten_db / 20.0)
}

/// Convert DX7 rate (0-99) to envelope slope in dB per second.
///
/// The patch rate is first quantised to the 0-63 `qrate` the EGS actually uses.
/// The slope is then a two-part number, exactly as the hardware builds it:
/// `(4 + qrate&3) << (qrate>>2)` — a 2-bit mantissa and a 4-bit exponent, so it
/// doubles every four qrate steps with three linear stops in between. The span is
/// 0.2819 dB/s to 16,165 dB/s, about 57,000:1.
///
/// This is the keyboard-scaled slope with no scaling applied; the two used to be
/// separate copies of the same arithmetic.
fn dx_rate_to_db_per_sec(rate: u8) -> f64 {
    dx_rate_to_db_per_sec_scaled(rate, 0)
}

/// Envelope slope with keyboard rate scaling folded in.
///
/// `scale_rate` returns a delta on the *quantised* rate, not on the 0-99 patch
/// rate, so it has to be added after the quantisation and before the mantissa /
/// exponent split — adding it to the patch rate instead would give a different
/// (and smaller) answer everywhere. `qrate_delta` of 0 is the unscaled slope,
/// which is all [`dx_rate_to_db_per_sec`] is.
fn dx_rate_to_db_per_sec_scaled(rate: u8, qrate_delta: i32) -> f64 {
    let qrate = ((i32::from(rate.min(99)) * 41) / 64 + qrate_delta).clamp(0, 63);
    let mantissa = 1.0 + 0.25 * f64::from(qrate & 3);
    0.2819 * mantissa * f64::exp2(f64::from(qrate >> 2))
}

/// Keyboard rate scaling: how much faster the envelope runs further up the
/// keyboard, as a delta on the quantised rate. Sensitivity is 0-7.
///
/// The note is bucketed into three-semitone steps and clamped, so the delta is
/// flat at or below MIDI 23 and saturates from MIDI 114 up. At full sensitivity
/// that span is 27 qrate steps, and since the slope doubles every four steps
/// that is roughly a 100x difference in envelope speed across the keyboard.
fn scale_rate(note: u8, sensitivity: u8) -> i32 {
    let x = (i32::from(note) / 3 - 7).clamp(0, 31);
    (i32::from(sensitivity.min(7)) * x) >> 3
}

/// Velocity curve, indexed by `velocity >> 1` — velocity resolution on the DX7 is
/// effectively 6-bit.
const VELOCITY_DATA: [u8; 64] = [
    0, 70, 86, 97, 106, 114, 121, 126, 132, 138, 142, 148, 152, 156, 160, 163,
    166, 170, 173, 174, 178, 181, 184, 186, 189, 190, 194, 196, 198, 200, 202,
    205, 206, 209, 211, 214, 216, 218, 220, 222, 224, 225, 227, 229, 230, 232,
    233, 235, 237, 238, 240, 241, 242, 243, 244, 246, 246, 248, 249, 250, 251,
    252, 253, 254,
];

/// Velocity as an additive offset in EGS level units (1 unit ≈ 0.0235 dB), not as
/// a gain multiplier. Sensitivity is 0-7; at 0 the result is always 0.
fn scale_velocity(velocity: u8, sensitivity: u8) -> i32 {
    let vel_value = i32::from(VELOCITY_DATA[usize::from(velocity.min(127) >> 1)]) - 239;
    ((i32::from(sensitivity.min(7)) * vel_value + 7) >> 3) << 4
}

/// Level-scaling curve for the two exponential curve settings. 33 entries, one
/// per three-semitone group, saturating at 250 — which is close enough to the
/// linear curve's 254 at the same distance that both curves arrive at the same
/// place at the far end of the keyboard and differ only in the shape between.
const EXP_SCALE_DATA: [u8; 33] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 14, 16, 19, 23, 27, 33, 39, 47, 56, 66, 80,
    94, 110, 126, 142, 158, 174, 190, 206, 222, 238, 250,
];

/// One side of the keyboard level scaling curve.
///
/// `group` counts three-semitone steps away from the breakpoint. The result is in
/// the coarse 0-127 `scaleoutlevel` domain, so one unit is 0.7526 dB — this is
/// applied *before* the shift into fine level units, which is why keyboard
/// scaling moves in visibly bigger steps than velocity does.
fn scale_curve(group: i32, depth: u8, curve: ScaleCurve) -> i32 {
    debug_assert!(group >= 0, "scale_curve group must be a distance, not a direction");
    let depth = i32::from(depth.min(99));
    let scale = if curve.is_linear() {
        (group * depth * 329) >> 12
    } else {
        let raw = i32::from(EXP_SCALE_DATA[group.clamp(0, 32) as usize]);
        (raw * depth * 329) >> 15
    };
    if curve.is_negative() { -scale } else { scale }
}

/// Keyboard level scaling: the level offset for one note, in `scaleoutlevel`
/// units. Zero at the breakpoint, and zero everywhere if both depths are 0.
///
/// The breakpoint parameter is the raw patch byte 0-99 rather than a MIDI note;
/// the `- 17` is what lines the two up, and it is deliberately the value the
/// hardware uses rather than the `- 21` a naive reading of "A-1 is MIDI 21"
/// would suggest.
fn scale_level(note: u8, op: &OpPreset) -> i32 {
    let offset = i32::from(note) - i32::from(op.break_point) - 17;
    if offset >= 0 {
        scale_curve((offset + 1) / 3, op.right_depth, op.right_curve)
    } else {
        scale_curve((1 - offset) / 3, op.left_depth, op.left_curve)
    }
}

/// An operator's static linear gain for one note: output level, keyboard level
/// scaling and velocity summed in the log domain and converted to linear exactly
/// once, in the order the EGS does it. Output level 0 is hard silence.
///
/// Note the asymmetry the hardware has here — level scaling is clamped to the top
/// of the coarse domain *before* the shift, so a boosted operator cannot exceed
/// level 99, but velocity is added afterwards and can push past it.
fn operator_gain(op: &OpPreset, note: u8, velocity: u8) -> f64 {
    if op.output_level == 0 { return 0.0; }
    let coarse = (scaleoutlevel(op.output_level) + scale_level(note, op)).min(127);
    let units = (coarse * 32 + scale_velocity(velocity, op.vel_sens)).max(0);
    atten_to_gain(f64::from(4064 - units) * DB_PER_UNIT)
}

// ── DX7 Envelope ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DxEnvStage {
    Idle,
    Attack,   // current → L1
    Decay1,   // L1 → L2
    Decay2,   // L2 → L3 (sustain)
    Release,  // current → L4
}

impl DxEnvStage {
    /// Which of the four rate/level slots this stage uses.
    fn slot(self) -> Option<usize> {
        match self {
            DxEnvStage::Idle => None,
            DxEnvStage::Attack => Some(0),
            DxEnvStage::Decay1 => Some(1),
            DxEnvStage::Decay2 => Some(2),
            DxEnvStage::Release => Some(3),
        }
    }
}

/// Four-rate / four-level EG running in the log domain.
///
/// Falling segments are straight lines in dB, which is what makes a DX7 decay
/// sound the way it does: the slope stays constant all the way to the target
/// instead of flattening out as a one-pole approach would. Rising segments use
/// the hardware's two attack quirks — the instant jump on key-down and the
/// headroom-scaled increment — so an attack is audibly not a mirrored decay.
#[derive(Debug, Clone)]
struct DxEnvelope {
    stage: DxEnvStage,
    /// Attenuation below full scale in dB. 0.0 is wide open, `SILENCE_DB` is shut.
    atten_db: f64,
    /// Linear gain matching `atten_db`. A straight line in dB is a geometric
    /// series in gain, so this is advanced by one multiply per sample rather than
    /// an exponential — exact, and cheap enough for the audio callback.
    gain: f64,
    /// Per-sample slope for each stage, in dB.
    step_db: [f64; 4],
    /// Target attenuation for each stage, in dB.
    target_db: [f64; 4],
    /// Per-sample gain multiplier for the segment in progress.
    ramp: f64,
    /// True when the segment in progress climbs toward full scale.
    rising: bool,
    /// Headroom multiplier currently applied to the attack step (1-17); 0 forces
    /// a recompute on the next tick.
    headroom: i32,
    /// Set once a segment has landed on a target it will not leave on its own.
    holding: bool,
    sample_rate: f64,
}

impl DxEnvelope {
    fn new(sr: f64) -> Self {
        let mut env = Self {
            stage: DxEnvStage::Idle,
            atten_db: SILENCE_DB,
            gain: 0.0,
            step_db: [0.0; 4],
            target_db: [SILENCE_DB; 4],
            ramp: 1.0,
            rising: false,
            headroom: 0,
            holding: false,
            sample_rate: sr,
        };
        env.set_from_preset([99, 60, 30, 60], [99, 90, 75, 0]);
        env
    }

    /// Configure from DX7 preset values (0-99 range), with no keyboard rate
    /// scaling. Identical to `set_from_preset_scaled(rates, levels, 0)`, which
    /// `rate_scaling_zero_is_the_unscaled_rate` pins.
    fn set_from_preset(&mut self, rates: [u8; 4], levels: [u8; 4]) {
        for (slot, (&rate, &level)) in rates.iter().zip(levels.iter()).enumerate() {
            self.step_db[slot] = dx_rate_to_db_per_sec(rate) / self.sample_rate;
            self.target_db[slot] = dx_level_to_atten_db(level);
        }
    }

    /// Configure from DX7 preset values with a keyboard rate scaling delta on the
    /// quantised rate. The delta applies to all four stages, as it does on the
    /// hardware — key scaling speeds up the release as well as the attack.
    fn set_from_preset_scaled(&mut self, rates: [u8; 4], levels: [u8; 4], qrate_delta: i32) {
        for (slot, (&rate, &level)) in rates.iter().zip(levels.iter()).enumerate() {
            self.step_db[slot] = dx_rate_to_db_per_sec_scaled(rate, qrate_delta) / self.sample_rate;
            self.target_db[slot] = dx_level_to_atten_db(level);
        }
    }

    /// Stretch or squeeze stage durations for the user-facing time knobs. A slope
    /// in dB/s scales inversely with the time taken to cover a fixed span, so a
    /// stage twice as long is a slope half as steep.
    fn scale_times(&mut self, attack_scale: f64, decay_scale: f64, release_scale: f64) {
        self.step_db[0] /= attack_scale.max(1e-4);
        self.step_db[1] /= decay_scale.max(1e-4);
        self.step_db[2] /= decay_scale.max(1e-4);
        self.step_db[3] /= release_scale.max(1e-4);
    }

    /// Push the sustain (decay 2) target down for the user sustain knob. Scaling a
    /// linear amplitude by `s` is adding `-20*log10(s)` dB of attenuation.
    fn scale_sustain(&mut self, sustain_scale: f64) {
        let offset = -20.0 * sustain_scale.max(1e-6).log10();
        self.target_db[2] = (self.target_db[2] + offset).min(SILENCE_DB);
    }

    /// Key-down. The level is *not* reset: retriggering part-way through a release
    /// picks up from wherever the envelope had got to, as the hardware does.
    fn trigger(&mut self) {
        self.enter(DxEnvStage::Attack);
    }

    fn release(&mut self) {
        if self.stage != DxEnvStage::Idle {
            self.enter(DxEnvStage::Release);
        }
    }

    fn kill(&mut self) {
        self.stage = DxEnvStage::Idle;
        self.atten_db = SILENCE_DB;
        self.gain = 0.0;
        self.holding = false;
    }

    fn is_active(&self) -> bool { self.stage != DxEnvStage::Idle }

    /// Begin a stage from wherever the level currently sits.
    fn enter(&mut self, stage: DxEnvStage) {
        self.stage = stage;
        self.holding = false;
        let Some(slot) = stage.slot() else {
            self.atten_db = SILENCE_DB;
            self.gain = 0.0;
            return;
        };
        // Rise or fall is decided by where this segment's target sits relative to
        // the level we are actually at, never by which stage it happens to be.
        self.rising = self.target_db[slot] < self.atten_db;
        if self.rising {
            if stage == DxEnvStage::Attack && self.atten_db > ATTACK_JUMP_DB {
                self.atten_db = ATTACK_JUMP_DB;
                self.gain = atten_to_gain(ATTACK_JUMP_DB);
            }
            self.headroom = 0;
        } else {
            self.ramp = atten_to_gain(self.step_db[slot]);
        }
    }

    /// The current segment has landed on its target; pick what happens next.
    fn advance(&mut self) {
        match self.stage {
            DxEnvStage::Attack => self.enter(DxEnvStage::Decay1),
            DxEnvStage::Decay1 => self.enter(DxEnvStage::Decay2),
            // Decay 2 is the sustain: hold at L3 for as long as the key is down.
            DxEnvStage::Decay2 => self.holding = true,
            DxEnvStage::Release => {
                // Only a release that reaches the floor frees the voice. A patch
                // with a nonzero L4 rings on, which is what the hardware does.
                if self.atten_db >= SILENCE_DB - 1e-9 {
                    self.stage = DxEnvStage::Idle;
                    self.gain = 0.0;
                } else {
                    self.holding = true;
                }
            }
            DxEnvStage::Idle => {}
        }
    }

    fn tick(&mut self) -> f64 {
        let Some(slot) = self.stage.slot() else { return 0.0 };
        if self.holding { return self.gain; }

        let target = self.target_db[slot];
        if self.rising {
            // Headroom-scaled step: the increment is multiplied by the number of
            // whole 1/17ths of the log range still to climb, so the attack starts
            // fast and decelerates. The multiplier is an integer, so the ramp only
            // has to be recomputed on the 17 crossings.
            let m = headroom_factor(self.atten_db);
            if m != self.headroom {
                self.headroom = m;
                self.ramp = atten_to_gain(-f64::from(m) * self.step_db[slot]);
            }
            self.atten_db -= f64::from(m) * self.step_db[slot];
            if self.atten_db <= target {
                self.atten_db = target;
                self.gain = atten_to_gain(target);
                self.advance();
            } else {
                self.gain *= self.ramp;
            }
        } else {
            self.atten_db += self.step_db[slot];
            if self.atten_db >= target {
                self.atten_db = target;
                self.gain = atten_to_gain(target);
                self.advance();
            } else {
                self.gain *= self.ramp;
            }
        }
        self.gain
    }
}

/// How many whole 1/17ths of the log range are left to climb, clamped to 1 so an
/// attack that is already at full scale still converges.
fn headroom_factor(atten_db: f64) -> i32 {
    ((HEADROOM_BANDS * atten_db / SILENCE_DB) as i32).clamp(1, 17)
}

// ── LFO ──
//
// One low-frequency oscillator for the whole instrument. Not one per voice: on a
// real DX7 every key reads the same oscillator, which is why a chord played with
// key sync off wobbles in lockstep rather than as six independent vibratos. It
// therefore lives on `Dx7Synth` and is ticked once per sample, whether or not
// anything is sounding — the phase has to keep running so a note started later
// picks it up part-way through, exactly as the hardware does.
//
// The phase is a 32-bit accumulator and the waveform generators are the
// hardware's bit twiddling rather than a smooth function of a float phase, which
// matters: the triangle's peak is one count wide, the square's high state is a
// hair above the range the other shapes reach, and sample-and-hold's step
// boundary is a wrap of the accumulator.

/// LFO speed 0-99 in Hz. Measured off hardware rather than derived — the values
/// are reciprocals of measured periods, which is why speed 75 is exactly 1/0.048 s
/// and speed 0 is 1/15.99 s. Not remotely linear: the bottom 64 steps cover 0.06
/// to 10 Hz and the top 35 cover 10 to 49 Hz.
///
/// Taken here as literal hertz. The reference implementation converts the same
/// table to a 32-bit phase increment via a constant of 4,437,500,000 where a
/// 32-bit accumulator wants 4,294,967,296, which runs every rate 3.32% fast. The
/// round reciprocals above are the reason to believe the table and not the
/// conversion: 1/0.048 s does not stay round after a 1.0332 multiply.
const LFO_RATE_HZ: [f64; 100] = [
    0.062_541, 0.125_031, 0.312_393, 0.437_120, 0.624_610,
    0.750_694, 0.936_330, 1.125_302, 1.249_609, 1.436_782,
    1.560_915, 1.752_081, 1.875_117, 2.062_494, 2.247_191,
    2.374_451, 2.560_492, 2.686_728, 2.873_976, 2.998_950,
    3.188_013, 3.369_840, 3.500_175, 3.682_224, 3.812_065,
    4.000_800, 4.186_202, 4.310_716, 4.501_260, 4.623_209,
    4.814_636, 4.930_480, 5.121_901, 5.315_191, 5.434_783,
    5.617_346, 5.750_431, 5.946_717, 6.062_811, 6.248_438,
    6.431_695, 6.564_264, 6.749_460, 6.868_132, 7.052_186,
    7.250_580, 7.375_719, 7.556_294, 7.687_577, 7.877_738,
    7.993_605, 8.181_967, 8.372_405, 8.504_848, 8.685_079,
    8.810_573, 8.986_341, 9.122_423, 9.300_595, 9.500_285,
    9.607_994, 9.798_158, 9.950_249, 10.117_361, 11.251_125,
    11.384_335, 12.562_814, 13.676_149, 13.904_338, 15.092_062,
    16.366_612, 16.638_935, 17.869_907, 19.193_858, 19.425_019,
    20.833_333, 21.034_918, 22.502_250, 24.003_841, 24.260_068,
    25.746_653, 27.173_913, 27.578_599, 29.052_876, 30.693_677,
    31.191_516, 32.658_393, 34.317_090, 34.674_064, 36.416_606,
    38.197_097, 38.550_501, 40.387_722, 40.749_796, 42.625_746,
    44.326_241, 44.883_303, 46.772_685, 48.590_865, 49.261_084,
];

/// Tick rate of the delay counter: `(1 << 32) / 15.5 s / 11`. The delay ramp is
/// a 32-bit accumulator like the phase, so this is what one second of delay costs
/// it.
const LFO_DELAY_UNIT: f64 = 25_190_424.0;

/// Half the phase accumulator — the point the delay ramp switches to its second,
/// faster increment, and the value key sync parks the phase just below.
const LFO_HALF: u32 = 1 << 31;

/// Full scale of an LFO waveform sample before normalising.
const LFO_FULL: u32 = 1 << 24;

/// One sample of the shared LFO.
#[derive(Debug, Clone, Copy)]
struct LfoFrame {
    /// Waveform output: 0.0 at the trough, 0.5 at the centre, 1.0 at the peak.
    value: f64,
    /// Delay ramp, 0.0 while the LFO is still held off and 1.0 once it is fully
    /// in. This gates *both* modulation destinations.
    delay: f64,
}

impl Default for LfoFrame {
    /// Centred and fully shut out, i.e. no modulation from either destination.
    fn default() -> Self {
        Self { value: 0.5, delay: 0.0 }
    }
}

#[derive(Debug, Clone)]
struct Lfo {
    /// 32-bit phase accumulator; one wrap is one cycle.
    phase: u32,
    /// Phase increment per sample.
    delta: u32,
    /// Delay ramp accumulator.
    delay_state: u32,
    /// The delay ramp's two increments. It runs at the first up to the halfway
    /// mark and at the second from there on. The ramp's output is pinned at zero
    /// for the whole of the first stage, so what the two rates actually split is
    /// "how long the LFO stays shut" against "how long it takes to arrive" — it
    /// is a delay, not a fade-in.
    delay_inc: u32,
    delay_inc2: u32,
    waveform: LfoWave,
    sync: bool,
    /// Sample-and-hold state: an 8-bit LCG, `x = 179x + 17`.
    rand_state: u8,
    sample_rate: f64,
}

impl Lfo {
    fn new(sample_rate: f64) -> Self {
        let mut lfo = Self {
            phase: 0,
            delta: 0,
            delay_state: 0,
            delay_inc: u32::MAX,
            delay_inc2: u32::MAX,
            waveform: LfoWave::Triangle,
            sync: true,
            rand_state: 0,
            sample_rate,
        };
        lfo.configure(&LfoPreset::neutral());
        lfo
    }

    /// Apply a patch's LFO settings. Deliberately does not touch the phase or the
    /// delay ramp, so switching patch mid-note does not restart the vibrato.
    fn configure(&mut self, p: &LfoPreset) {
        let hz = LFO_RATE_HZ[usize::from(p.speed.min(99))];
        self.delta = (hz / self.sample_rate * 4_294_967_296.0) as u32;

        // The delay parameter is inverted before use, and then split into a
        // 4-bit mantissa and a 3-bit exponent, so the increment (and hence the
        // delay time) doubles every sixteen steps.
        let a = 99 - i32::from(p.delay.min(99));
        if a == 99 {
            // Delay 0: the ramp saturates on its first tick, so there is no
            // delay at all rather than a very short one.
            self.delay_inc = u32::MAX;
            self.delay_inc2 = u32::MAX;
        } else {
            let first = (16 + (a & 15)) << (1 + (a >> 4));
            // The second increment is the first with its low seven bits cleared
            // and a floor of 128. That floor is the interesting part: it caps the
            // arrival stage at two thirds of a second no matter how long the
            // delay, so a very late vibrato still comes in at a usable speed.
            let second = (first & 0xff80).max(0x80);
            self.delay_inc = self.delay_increment(first);
            self.delay_inc2 = self.delay_increment(second);
        }
        self.waveform = p.waveform;
        self.sync = p.sync;
    }

    fn delay_increment(&self, a: i32) -> u32 {
        (LFO_DELAY_UNIT * f64::from(a) / self.sample_rate) as u32
    }

    /// Key-down. The phase only restarts when key sync is on; the delay ramp
    /// restarts either way, and because there is one LFO for the whole machine
    /// the newest key-down restarts it for every voice already sounding.
    fn keydown(&mut self) {
        if self.sync {
            // One count short of the halfway mark: the peak of a triangle, and
            // the descending zero crossing of a sine.
            self.phase = LFO_HALF - 1;
        }
        self.delay_state = 0;
    }

    fn tick(&mut self) -> LfoFrame {
        LfoFrame { value: self.next_value(), delay: self.next_delay() }
    }

    fn next_value(&mut self) -> f64 {
        self.phase = self.phase.wrapping_add(self.delta);
        let scale = 1.0 / f64::from(LFO_FULL);
        let raw = match self.waveform {
            LfoWave::Triangle => {
                // Up over the first half, down over the second, by inverting the
                // ramp's low 24 bits once the top phase bit sets.
                let x = self.phase >> 7;
                (x ^ 0u32.wrapping_sub(self.phase >> 31)) & (LFO_FULL - 1)
            }
            // Both sawtooths flip the top phase bit before shifting, so the ramp
            // starts at the centre of its travel rather than at an end.
            LfoWave::SawDown => ((!self.phase) ^ LFO_HALF) >> 8,
            LfoWave::SawUp => (self.phase ^ LFO_HALF) >> 8,
            // The high state is one count above what the other shapes reach at
            // their peak. That is the hardware's own off-by-one; it is inaudible
            // as a pitch, but it does mean a square LFO drives amplitude
            // modulation to exactly zero on the half cycle it is high.
            LfoWave::Square => ((!self.phase) >> 7) & LFO_FULL,
            LfoWave::Sine => {
                // The only shape done in the linear domain rather than by bit
                // twiddling — the hardware reads a 1024-entry table here, and a
                // real sine is simply a better version of the same curve.
                let turns = f64::from(self.phase) / 4_294_967_296.0;
                return 0.5 + 0.5 * (TWO_PI * turns).sin();
            }
            LfoWave::SampleHold => {
                // A wrap of the accumulator is one step. The generator is the
                // hardware's, so the sequence has the same character rather than
                // merely the same statistics.
                if self.phase < self.delta {
                    self.rand_state = self.rand_state.wrapping_mul(179).wrapping_add(17);
                }
                (u32::from(self.rand_state ^ 0x80) + 1) << 16
            }
        };
        f64::from(raw) * scale
    }

    fn next_delay(&mut self) -> f64 {
        let inc = if self.delay_state < LFO_HALF { self.delay_inc } else { self.delay_inc2 };
        let next = u64::from(self.delay_state) + u64::from(inc);
        if next > u64::from(u32::MAX) {
            // Saturated: the LFO is fully in and stays there until the next
            // key-down. Left pinned rather than wrapped.
            self.delay_state = u32::MAX;
            return 1.0;
        }
        self.delay_state = next as u32;
        if self.delay_state < LFO_HALF {
            0.0
        } else {
            f64::from((self.delay_state >> 7) & (LFO_FULL - 1)) / f64::from(LFO_FULL)
        }
    }
}

// ── Pitch envelope ──

/// Pitch EG level 0-99 to a pitch offset. **Level 50 is the neutral centre**, and
/// the curve either side of it is steep at the extremes and one table step per
/// patch step through the middle third.
///
/// The table is in units of 1/32 octave (0.375 semitone), which is where the pitch
/// EG's odd resolution comes from: you cannot dial a five-cent bend, the smallest
/// step off centre is 37.5 cents. The ends are -4 and +3.97 octaves.
fn pitch_env_offset(level: u8) -> f64 {
    const PITCH_ENV_TAB: [i8; 100] = [
        -128, -116, -104, -95, -85, -76, -68, -61, -56, -52,
        -49, -46, -43, -41, -39, -37, -35, -33, -32, -31,
        -30, -29, -28, -27, -26, -25, -24, -23, -22, -21,
        -20, -19, -18, -17, -16, -15, -14, -13, -12, -11,
        -10, -9, -8, -7, -6, -5, -4, -3, -2, -1,
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9,
        10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
        20, 21, 22, 23, 24, 25, 26, 27, 28, 29,
        30, 31, 32, 33, 34, 35, 38, 40, 43, 46,
        49, 53, 58, 65, 73, 82, 92, 103, 115, 127,
    ];
    f64::from(PITCH_ENV_TAB[usize::from(level.min(99))]) / 32.0
}

/// Pitch EG rate 0-99 to a slope, in table units. Divided by
/// [`PITCH_ENV_TIME_CONST`] and the sample rate this is octaves per sample, so the
/// span is 0.047 to 11.97 octaves per second. Unlike the amplitude EG the pitch EG
/// is linear in pitch, not exponential in it — a sweep at a fixed rate crosses
/// every octave in the same time.
const PITCH_ENV_RATE: [u8; 100] = [
    1, 2, 3, 3, 4, 4, 5, 5, 6, 6,
    7, 7, 8, 8, 9, 9, 10, 10, 11, 11,
    12, 12, 13, 13, 14, 14, 15, 16, 16, 17,
    18, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 30, 31, 33, 34, 36, 37, 38, 39,
    41, 42, 44, 46, 47, 49, 51, 53, 54, 56,
    58, 60, 62, 64, 66, 68, 70, 72, 74, 76,
    79, 82, 85, 88, 91, 94, 98, 102, 106, 110,
    115, 120, 125, 130, 135, 141, 147, 153, 159, 165,
    171, 178, 185, 193, 202, 211, 232, 243, 254, 255,
];

/// Seconds per octave at one table unit of rate.
const PITCH_ENV_TIME_CONST: f64 = 21.3;

/// The voice-global pitch envelope: one four-stage EG bending the pitch of every
/// ratio-mode operator at once.
///
/// Structurally it is the amplitude EG's sibling — same four rates, same four
/// levels, same "hold at L3 while the key is down" — but it runs in the linear
/// pitch domain rather than the log amplitude one, and it starts at L4 rather
/// than from silence, so a patch whose L4 is off centre begins the note detuned
/// and pulls itself into tune.
#[derive(Debug, Clone)]
struct DxPitchEnvelope {
    /// Stage targets in octaves, pre-converted from the 0-99 patch levels.
    targets: [f64; 4],
    /// Stage slopes in octaves per sample, always positive.
    incs: [f64; 4],
    level: f64,
    target: f64,
    inc: f64,
    rising: bool,
    stage: usize,
    down: bool,
    /// Set when every level is the neutral 50, so the envelope can never produce
    /// an offset at all. Lets a voice skip pitch modulation outright.
    flat: bool,
    sample_rate: f64,
}

impl DxPitchEnvelope {
    fn new(sample_rate: f64) -> Self {
        let mut env = Self {
            targets: [0.0; 4],
            incs: [0.0; 4],
            level: 0.0,
            target: 0.0,
            inc: 0.0,
            rising: false,
            stage: 4,
            down: false,
            flat: true,
            sample_rate,
        };
        let neutral = PitchEgPreset::neutral();
        env.set(neutral.rates, neutral.levels);
        env
    }

    /// Configure and key down in one go, which is what the hardware does — there
    /// is no way to load a pitch EG without also restarting it.
    fn set(&mut self, rates: [u8; 4], levels: [u8; 4]) {
        for slot in 0..4 {
            self.targets[slot] = pitch_env_offset(levels[slot]);
            let rate = PITCH_ENV_RATE[usize::from(rates[slot].min(99))];
            self.incs[slot] = f64::from(rate) / (PITCH_ENV_TIME_CONST * self.sample_rate);
        }
        self.flat = self.targets.iter().all(|&t| t == 0.0);
        // The note starts wherever L4 left it, not at centre.
        self.level = self.targets[3];
        self.down = true;
        self.enter(0);
    }

    fn enter(&mut self, stage: usize) {
        self.stage = stage;
        if stage < 4 {
            self.target = self.targets[stage];
            self.rising = self.target > self.level;
            self.inc = self.incs[stage];
        }
    }

    fn keyup(&mut self) {
        if self.down {
            self.down = false;
            self.enter(3);
        }
    }

    fn tick(&mut self) -> f64 {
        if self.flat {
            return 0.0;
        }
        // Stages 0-2 sweep to L1, L2 and L3 in turn and always run to completion.
        // Stage 3 targets L4 but is gated on the key being up, so reaching L3
        // parks the envelope there for as long as the note is held — the same
        // "decay 2 is the sustain" shape the amplitude EG has.
        if self.stage < 3 || (self.stage < 4 && !self.down) {
            if self.rising {
                self.level += self.inc;
                if self.level >= self.target {
                    self.level = self.target;
                    self.enter(self.stage + 1);
                }
            } else {
                self.level -= self.inc;
                if self.level <= self.target {
                    self.level = self.target;
                    self.enter(self.stage + 1);
                }
            }
        }
        self.level
    }
}

// ── FM Operator ──

#[derive(Debug, Clone)]
struct Operator {
    phase: f64,
    freq: f64,
    /// Static linear gain for this note: output level, velocity offset and (for
    /// modulators) brightness, all summed in the log domain and converted once.
    gain: f64,
    envelope: DxEnvelope,
    /// Previous two output samples for feedback averaging.
    prev: [f64; 2],
    /// This operator's LFO amplitude-mod sensitivity, 0-3, copied from the patch
    /// at key-down. Indexes the voice's per-sample gain table.
    amp_mod_sens: u8,
    /// False for a fixed-frequency operator. Neither the pitch EG nor the LFO's
    /// pitch modulation reaches one of those — a fixed operator's frequency is
    /// absolute, and the hardware routes only pitch bend to it.
    pitch_tracks: bool,
}

impl Operator {
    fn new(sr: f64) -> Self {
        Self {
            phase: 0.0,
            freq: 440.0,
            gain: 1.0,
            envelope: DxEnvelope::new(sr),
            prev: [0.0; 2],
            amp_mod_sens: 0,
            pitch_tracks: true,
        }
    }

    /// Process one sample. `modulation` is phase modulation input from other
    /// operators, **in cycles**; `freq_ratio` and `amp_gain` are this sample's
    /// pitch and amplitude modulation, both exactly 1.0 when nothing is
    /// modulating.
    ///
    /// Cycles, not radians, is the hardware's own unit. The EGS/OPS pair carries
    /// one full cycle as the same 24-bit quantity that carries full sine
    /// amplitude, and the OPS adds a modulator's output straight into the
    /// carrier's phase accumulator without rescaling it. A modulator at full
    /// output therefore swings its carrier through a whole cycle — 2π radians,
    /// not one radian. Treating the sum as radians was the single mistake that
    /// made every FM patch in the bank render as a near-pure sine.
    fn tick(&mut self, modulation: f64, sample_rate: f64, freq_ratio: f64, amp_gain: f64) -> f64 {
        let env = self.envelope.tick();
        let mut out = (self.phase + modulation * TWO_PI).sin() * env * self.gain;
        // Amplitude modulation attenuates the operator's whole output, so a
        // modulator loses depth as well as a carrier losing volume.
        if amp_gain != 1.0 { out *= amp_gain; }

        let freq = if freq_ratio == 1.0 { self.freq } else { self.freq * freq_ratio };
        self.phase += TWO_PI * freq / sample_rate;
        // Keep phase in bounds to prevent float drift
        if self.phase > TWO_PI { self.phase -= TWO_PI; }

        // Shift feedback history
        self.prev[1] = self.prev[0];
        self.prev[0] = out;

        out
    }

    /// Get feedback modulation, in cycles: the averaged previous two samples
    /// scaled by the patch's feedback depth. Averaging is the hardware's own
    /// one-pole smoothing of the loop, not a numerical dodge — without it the
    /// loop runs away into noise a full index earlier than the real machine.
    fn feedback(&self, amount: f64) -> f64 {
        (self.prev[0] + self.prev[1]) * 0.5 * amount
    }

    fn kill(&mut self) {
        self.envelope.kill();
        self.phase = 0.0;
        self.prev = [0.0; 2];
    }
}

// ── Feedback ──

/// The largest feedback index a patch can hold.
const FEEDBACK_MAX: u8 = 7;

/// Feedback index 0-7 as a phase-modulation depth **in cycles**, the same unit
/// [`Operator::tick`] takes its modulation input in.
///
/// The scale is exponential: every step up doubles the depth, and index 0 is
/// off rather than "very quiet", exactly as the patch parameter behaves.
///
/// The hardware shifts the averaged loop sample right by `8 - index`, so index 7
/// is a shift of one — half of full scale, i.e. half a cycle, i.e. π radians of
/// phase deviation once [`Operator::tick`] multiplies through. That is the same
/// depth this returned when it was written in radians and the modulation path
/// was 2π too shallow; the two errors cancelled and left feedback correct. Only
/// the unit moved here.
fn feedback_depth(index: u8) -> f64 {
    if index == 0 {
        0.0
    } else {
        f64::exp2(f64::from(index.min(FEEDBACK_MAX)) - 8.0)
    }
}

/// Resolve the feedback knob against a patch's authored feedback index.
///
/// The knob is a **trim**, not an absolute value: 0.5 is dead centre and returns
/// the index the patch was written with, full left subtracts the whole 7-step
/// range and full right adds it. An absolute knob cannot do better — it has no
/// idea what the patch asked for, so at any fixed position it flattens the whole
/// bank onto one index and puts the authored value out of reach.
///
/// Total by construction — `params` is public, so a knob can arrive as anything
/// at all. The float-to-int cast saturates, the add saturates rather than
/// overflowing on that saturated value, the sum is clamped, and a NaN knob casts
/// to zero steps and so lands on the patch's own index.
fn resolve_feedback(authored: u8, knob: f32) -> u8 {
    let steps = ((f64::from(knob) - 0.5) * 14.0).round() as i32;
    i32::from(authored).saturating_add(steps).clamp(0, i32::from(FEEDBACK_MAX)) as u8
}

// ── Voice ──

#[derive(Debug, Clone)]
struct DxVoice {
    ops: [Operator; NUM_OPERATORS],
    note: u8,
    velocity: f32,
    age: u64,
    sample_rate: f64,
    algorithm: u8,
    feedback_amount: f64,
    /// Per-voice, unlike the LFO: every key gets its own pitch envelope.
    pitch_env: DxPitchEnvelope,
    /// Octaves of pitch deviation per unit of (delay x bipolar LFO). Zero when
    /// either the patch's PMD or its pitch-mod sensitivity is zero, which is what
    /// makes a patch with no vibrato cost nothing.
    pitch_mod_depth: f64,
    /// LFO amplitude-mod depth, 0..1, before per-operator sensitivity.
    amp_mod_depth: f64,
    /// Which amp-mod sensitivity settings this patch's operators actually use, so
    /// the per-sample gain table only evaluates the entries that get read.
    amp_mod_used: [bool; 4],
}

impl DxVoice {
    fn new(sr: f64) -> Self {
        Self {
            ops: std::array::from_fn(|_| Operator::new(sr)),
            note: 255,
            velocity: 0.0,
            age: 0,
            sample_rate: sr,
            algorithm: 5,
            feedback_amount: 0.0,
            pitch_env: DxPitchEnvelope::new(sr),
            pitch_mod_depth: 0.0,
            amp_mod_depth: 0.0,
            amp_mod_used: [false; 4],
        }
    }

    fn note_on(
        &mut self,
        note: u8,
        vel: u8,
        preset: &PatchPreset,
        brightness: f64,
        attack_scale: f64,
        decay_scale: f64,
        sustain_scale: f64,
        release_scale: f64,
        age: u64,
    ) {
        self.note = note;
        self.velocity = vel as f32 / 127.0;
        self.age = age;
        self.algorithm = preset.algorithm;
        self.feedback_amount = feedback_depth(preset.feedback);

        // Transpose shifts the whole keyboard, so it is applied to the note once
        // here and everything downstream — operator frequency, keyboard level
        // scaling and keyboard rate scaling — reads the shifted note. The
        // *untransposed* note stays in `self.note`, because that is what a
        // note-off will name. Clamped rather than wrapped: a bass voice two
        // octaves down played at the bottom of the keyboard would otherwise ask
        // for a negative note number.
        let sounding = (i32::from(note) + i32::from(preset.transpose) - TRANSPOSE_CENTRE)
            .clamp(0, 127) as u8;

        let alg = algorithm(preset.algorithm);

        // Voice-global modulation. Both depths collapse to zero unless the patch
        // asks for them, and `amp_mod_used` records which sensitivity settings the
        // per-sample gain table has to bother computing.
        self.pitch_env.set(preset.pitch_eg.rates, preset.pitch_eg.levels);
        self.pitch_mod_depth = preset.lfo.pitch_mod_depth();
        self.amp_mod_depth = preset.lfo.amp_mod_depth();
        self.amp_mod_used = [false; 4];
        for op_preset in &preset.ops {
            self.amp_mod_used[usize::from(op_preset.amp_mod_sens.min(3))] = true;
        }

        for (i, op_preset) in preset.ops.iter().enumerate() {
            let op = &mut self.ops[i];
            // Oscillator key sync. Resetting the phase is what makes every note
            // of a patch identical; leaving it alone is what the other 74
            // factory voices ask for, and it is the whole running oscillator —
            // phase and the two samples the feedback loop averages — that has
            // to carry over, not just the phase.
            //
            // One difference from the hardware, and it is deliberate: the
            // OPS's 96 phase accumulators keep counting while a voice is idle,
            // where these stop. So a free-running voice picked up after a
            // silence starts where its last note left it rather than where it
            // would have got to, and the very first note after a reset starts
            // from zero either way. Modelling the difference means advancing
            // six accumulators per idle voice per sample, for a phase offset
            // that is arbitrary in both cases.
            if preset.osc_key_sync {
                op.phase = 0.0;
                op.prev = [0.0; 2];
            }
            op.freq = op_preset.frequency(sounding);
            op.amp_mod_sens = op_preset.amp_mod_sens.min(3);
            op.pitch_tracks = op_preset.mode == OpFreqMode::Ratio;

            // Output level, keyboard level scaling and velocity combine in the
            // log domain; modulators get the brightness knob on top, carriers
            // don't.
            let is_carrier = alg.carriers.contains(&i);
            let bright = if is_carrier { 1.0 } else { brightness };
            op.gain = operator_gain(op_preset, sounding, vel) * bright;

            // Configure envelope from preset, with keyboard rate scaling folded
            // into the quantised rate so higher notes decay sooner.
            op.envelope.set_from_preset_scaled(
                op_preset.rates,
                op_preset.levels,
                scale_rate(sounding, op_preset.rate_scaling),
            );

            // Apply user-facing envelope scaling
            op.envelope.scale_sustain(sustain_scale);
            op.envelope.scale_times(attack_scale, decay_scale, release_scale);

            op.envelope.trigger();
        }
    }

    fn note_off(&mut self) {
        for op in &mut self.ops {
            op.envelope.release();
        }
        self.pitch_env.keyup();
    }

    fn kill(&mut self) {
        self.note = 255;
        for op in &mut self.ops {
            op.kill();
        }
    }

    fn is_sounding(&self) -> bool {
        let alg = algorithm(self.algorithm);
        // Voice is sounding if ANY carrier envelope is active
        alg.carriers.iter().any(|&i| self.ops[i].envelope.is_active())
    }

    fn is_held(&self) -> bool {
        let alg = algorithm(self.algorithm);
        alg.carriers.iter().any(|&i| {
            matches!(
                self.ops[i].envelope.stage,
                DxEnvStage::Attack | DxEnvStage::Decay1 | DxEnvStage::Decay2
            )
        })
    }

    fn tick(&mut self, lfo: LfoFrame) -> f32 {
        if !self.is_sounding() { return 0.0; }

        let alg = algorithm(self.algorithm);
        let sr = self.sample_rate;

        // ── Pitch modulation ──
        // The pitch EG and the LFO both bend the whole voice, so this is one
        // multiplier shared by every ratio-mode operator. The LFO term is the
        // product of PMD, pitch-mod sensitivity, the delay ramp and the bipolar
        // waveform: any of the four at zero and the whole thing is zero, which is
        // what keeps an unmodulated patch bit-for-bit unchanged.
        let bend = self.pitch_env.tick()
            + self.pitch_mod_depth * lfo.delay * (lfo.value * 2.0 - 1.0);
        let freq_ratio = if bend == 0.0 { 1.0 } else { f64::exp2(bend) };

        // ── Amplitude modulation ──
        // The LFO only ever ducks an operator, never boosts it: the peak of the
        // waveform is full volume and the trough is the attenuated end. Depth is
        // a straight line in dB, so this is the log-domain offset the EGS adds to
        // the operator's level rather than a gain multiplier.
        let mut amp_gain = [1.0f64; 4];
        let amp_mod = self.amp_mod_depth * lfo.delay * (1.0 - lfo.value);
        if amp_mod > 0.0 {
            for (sens, gain) in amp_gain.iter_mut().enumerate().skip(1) {
                if self.amp_mod_used[sens] {
                    *gain = atten_to_gain(SILENCE_DB * AMP_MOD_SENS[sens] * amp_mod);
                }
            }
        }

        // Process operators in reverse order (6→1) so modulator outputs are ready
        let mut op_outputs = [0.0f64; NUM_OPERATORS];

        for i in (0..NUM_OPERATORS).rev() {
            // Modulation input from any operators that modulate this one, in
            // cycles. An operator's output is both its audio sample and its
            // phase-modulation contribution — the same number, in the same
            // units — so this is a plain sum, and a modulator at full output
            // contributes one whole cycle of deviation.
            let mut modulation = 0.0;
            for (j, targets) in alg.modulates.iter().enumerate() {
                if targets.contains(&i) {
                    modulation += op_outputs[j];
                }
            }

            // Add feedback if this operator is the feedback destination.
            // The tap reads the source operator's `prev` history, which for both
            // self-feedback and the 4→6 / 5→6 hardware loops still holds the
            // previous samples at this point in the descending scan (the source
            // op has an index <= the destination, so it has not ticked yet).
            if i == alg.feedback_dst {
                modulation += self.ops[alg.feedback_src].feedback(self.feedback_amount);
            }

            let op_ratio = if self.ops[i].pitch_tracks { freq_ratio } else { 1.0 };
            let op_amp = amp_gain[usize::from(self.ops[i].amp_mod_sens)];
            op_outputs[i] = self.ops[i].tick(modulation, sr, op_ratio, op_amp);
        }

        // Sum carrier outputs
        let mut out = 0.0f64;
        for &c in alg.carriers {
            out += op_outputs[c];
        }

        // Normalize by number of carriers
        let num_carriers = alg.carriers.len() as f64;
        (out / num_carriers) as f32
    }
}

// ── DX7 Synth ──

pub struct Dx7Synth {
    voices: Vec<DxVoice>,
    sample_rate: f64,
    pub params: [f32; PARAM_COUNT],
    voice_counter: u64,
    /// The whole factory set, borrowed rather than owned: 256 voices is 40 KB,
    /// and every DX7 track in a project would otherwise carry its own copy of
    /// the same constant table.
    presets: &'static [PatchPreset; VOICE_COUNT],
    /// One LFO for the whole instrument, as on the hardware — every voice reads
    /// the same phase.
    lfo: Lfo,
}

impl Dx7Synth {
    pub fn new() -> Self {
        Self {
            voices: Vec::new(),
            sample_rate: 44100.0,
            params: PARAM_DEFAULTS,
            voice_counter: 0,
            // Unpacks the ROM if this is the first instrument built. Deliberately
            // here, on whichever thread constructs the plugin, and never in
            // `process`.
            presets: presets(),
            lfo: Lfo::new(44100.0),
        }
    }

    /// The voice the bank and patch knobs select between them, 0-255.
    fn current_voice(&self) -> usize {
        voice_index(self.params[P_BANK], self.params[P_PATCH])
    }

    fn next_age(&mut self) -> u64 { self.voice_counter += 1; self.voice_counter }

    fn allocate_voice(&mut self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| !v.is_sounding()) { return i; }
        if let Some((i, _)) = self.voices.iter().enumerate()
            .filter(|(_, v)| !v.is_held()).min_by_key(|(_, v)| v.age) { return i; }
        self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap_or(0)
    }

    fn release_note(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.note == note && v.is_held() { v.note_off(); }
        }
    }

    fn kill_all_voices(&mut self) {
        for v in &mut self.voices { v.kill(); }
    }

    /// Resolve the brightness knob into a multiplier on every modulator's gain.
    ///
    /// A trim, like the feedback knob, and for the same reason: the presets are
    /// voiced, several of them decoded from ROM sysex, so the centre of the knob
    /// has to be the patch exactly as authored. It used to run 0.2 at hard left
    /// to 2.0 at hard right, which put the default at 1.1 — an extra 0.8 dB on
    /// every modulator, silently, on every patch in the bank. That was harmless
    /// while the modulation path was 2π too shallow and nothing sounded like FM
    /// anyway; with the index right it is 0.8 dB of somebody else's voicing.
    ///
    /// The travel is deliberately lopsided — 18 dB of cut, 6 dB of boost. Cut
    /// needs the room: a patch has to be pulled a long way down before its
    /// sidebands stop mattering, since amplitude falls off as `Jₙ(β)` and not as
    /// β. Boost does not: at the top of the bank a modulator already swings its
    /// carrier through 1.1 cycles, so 6 dB is the point where the deepest
    /// patches stop being instruments and start being noise, and there is no
    /// reason to hand the player travel that only reaches further into that.
    ///
    /// Total by construction — `params` is public, so the knob can arrive as
    /// anything. Out of range clamps and a NaN lands on the centre, which is the
    /// patch's own voicing, exactly as the feedback trim does.
    fn brightness(&self) -> f64 {
        const CUT_DB: f64 = 18.0;
        const BOOST_DB: f64 = 6.0;
        let knob = f64::from(self.params[P_BRIGHTNESS]);
        let knob = if knob.is_nan() { 0.5 } else { knob.clamp(0.0, 1.0) };
        let atten_db = if knob < 0.5 {
            CUT_DB * (0.5 - knob) * 2.0
        } else {
            -BOOST_DB * (knob - 0.5) * 2.0
        };
        atten_to_gain(atten_db)
    }

    fn attack_scale(&self) -> f64 {
        time_scale(self.params[P_ATTACK])
    }

    fn decay_scale(&self) -> f64 {
        time_scale(self.params[P_DECAY])
    }

    fn sustain_scale(&self) -> f64 {
        self.params[P_SUSTAIN] as f64
    }

    fn release_scale(&self) -> f64 {
        time_scale(self.params[P_RELEASE])
    }
}

impl Default for Dx7Synth {
    fn default() -> Self { Self::new() }
}

impl Plugin for Dx7Synth {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "DX7".into(),
            version: "0.1.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..MAX_VOICES).map(|_| DxVoice::new(sample_rate)).collect();
        self.lfo = Lfo::new(sample_rate);
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() { return; }

        let buf_len = outputs[0].len();
        let gain = self.params[P_GAIN] * OUTPUT_TRIM;
        let patch_idx = self.current_voice();
        // The knob trims the patch rather than replacing it, so it is folded into
        // this block's copy of the preset before any voice reads it. Doing it here
        // — once, up front — is what stops the authored value being silently
        // overwritten further down.
        let mut preset = self.presets[patch_idx];
        preset.feedback = resolve_feedback(preset.feedback, self.params[P_FEEDBACK]);
        let brightness = self.brightness();
        let attack_scale = self.attack_scale();
        let decay_scale = self.decay_scale();
        let sustain_scale = self.sustain_scale();
        let release_scale = self.release_scale();

        // Rate, shape, sync and delay come from the patch. Re-applying them every
        // block is what makes a patch change take effect; it deliberately leaves
        // the phase and the delay ramp alone so switching patch mid-note does not
        // restart the vibrato.
        self.lfo.configure(&preset.lfo);

        // Avoid allocation in audio thread — use fixed-size scratch buffer
        let mut event_indices: [usize; 256] = [0; 256];
        let event_count = midi_events.len().min(256);
        for i in 0..event_count { event_indices[i] = i; }
        // Simple insertion sort on sample_offset (usually already sorted, tiny N)
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
                            self.release_note(ev.data1);
                            let age = self.next_age();
                            let idx = self.allocate_voice();
                            self.voices[idx].note_on(
                                ev.data1, ev.data2, &preset,
                                brightness, attack_scale, decay_scale,
                                sustain_scale, release_scale, age,
                            );
                            // One LFO for the machine, so the newest key-down
                            // restarts the delay ramp — and the phase too, if the
                            // patch has key sync on — for everything sounding.
                            self.lfo.keydown();
                        } else {
                            self.release_note(ev.data1);
                        }
                    }
                    0x80 => self.release_note(ev.data1),
                    0xB0 => match ev.data1 {
                        120 => self.kill_all_voices(),
                        123 => {
                            for v in &mut self.voices { if v.is_held() { v.note_off(); } }
                        }
                        _ => {}
                    }
                    _ => {}
                }
                ei += 1;
            }

            // The LFO advances whether or not anything is sounding: its phase has
            // to keep running so a note started later picks it up part-way
            // through, which is exactly what makes key sync audible.
            let lfo = self.lfo.tick();

            let mut sample = 0.0f32;
            for v in &mut self.voices {
                sample += v.tick(lfo);
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
            min: 0.0,
            max: 1.0,
            default: PARAM_DEFAULTS[index],
            unit: match index {
                P_ATTACK | P_DECAY | P_RELEASE => "s".into(),
                // A trim on the patch's own index, so the useful unit is the
                // offset in feedback steps: -7 hard left, 0 centred, +7 hard right.
                P_FEEDBACK => "steps".into(),
                // Also a trim on the patch, in dB on the modulator levels:
                // -18 hard left, 0 centred, +6 hard right.
                P_BRIGHTNESS => "dB".into(),
                // The two selectors read as names, not numbers; see
                // [`discrete_label`].
                P_PATCH => "voice".into(),
                P_BANK => "cartridge".into(),
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
    }

    fn reset(&mut self) { self.kill_all_voices(); self.voice_counter = 0; }
}

fn note_to_freq(note: u8) -> f64 {
    440.0 * 2.0f64.powf((note as f64 - 69.0) / 12.0)
}

/// Map an attack/decay/release knob (0..1) to a stage-duration multiplier.
///
/// The knob is a rate offset in disguise: it shifts the effective `qrate` by
/// `TIME_KNOB_QRATE * (1 - knob)`, and because the EG slope doubles every four
/// qrate steps that comes out as a power-of-two multiplier on stage duration.
/// Turning the knob up lengthens the stage, as it always did; 1.0 now means
/// "exactly the rate written in the patch" and 0.0 is 64x quicker.
fn time_scale(knob: f32) -> f64 {
    const TIME_KNOB_QRATE: f64 = 24.0;
    f64::exp2(TIME_KNOB_QRATE * (f64::from(knob.clamp(0.0, 1.0)) - 1.0) / 4.0)
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

    fn process_buffers(synth: &mut Dx7Synth, events: &[MidiEvent], count: usize) -> Vec<f32> {
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

    #[test]
    fn silence_with_no_input() {
        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 4);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001, "Should produce sound, peak={peak}");
    }

    #[test]
    fn silent_after_release() {
        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[note_off(60, 0)], 3000);
        let out = process_buffers(&mut s, &[], 1);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak < 0.001, "Should be silent after release, peak={peak}");
    }

    #[test]
    fn output_is_finite() {
        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 1000);
        assert!(out.iter().all(|v| v.is_finite()), "Output must be finite");
    }

    #[test]
    fn polyphony() {
        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        let events = [note_on(60, 100, 0), note_on(64, 100, 0), note_on(67, 100, 0)];
        let out = process_buffers(&mut s, &events, 4);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001 && peak <= 2.0, "peak={peak}");
    }

    /// Point a synth at one of the 256 factory voices by number.
    fn select(s: &mut Dx7Synth, voice: usize) {
        let (bank, patch) = voice_knobs(voice);
        s.set_parameter(P_BANK, bank);
        s.set_parameter(P_PATCH, patch);
    }

    #[test]
    fn every_factory_voice_produces_sound() {
        // 1.5 s, which covers the slowest attack in the factory set: the
        // rendered peak of the quietest voice here arrives at 0.9 s.
        for voice in 0..VOICE_COUNT {
            let mut s = Dx7Synth::new();
            s.init(44100.0, 64);
            select(&mut s, voice);
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 1024);
            let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
            assert!(peak > 0.001, "voice {voice} ({}) should produce sound, peak={peak}",
                voice_name(voice));
        }
    }

    #[test]
    fn every_factory_voice_is_finite() {
        for voice in 0..VOICE_COUNT {
            let mut s = Dx7Synth::new();
            s.init(44100.0, 64);
            select(&mut s, voice);
            let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 200);
            assert!(out.iter().all(|v| v.is_finite()),
                "voice {voice} ({}) must produce finite output", voice_name(voice));
        }
    }

    #[test]
    fn cc120_kills_all() {
        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[cc(120, 0, 0)], 1);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn brightness_affects_sound() {
        let mut s1 = Dx7Synth::new();
        s1.init(44100.0, 64);
        s1.set_parameter(P_BRIGHTNESS, 0.1);
        let dark = process_buffers(&mut s1, &[note_on(60, 100, 0)], 8);
        let dark_energy: f32 = dark.iter().map(|v| v * v).sum();

        let mut s2 = Dx7Synth::new();
        s2.init(44100.0, 64);
        s2.set_parameter(P_BRIGHTNESS, 0.9);
        let bright = process_buffers(&mut s2, &[note_on(60, 100, 0)], 8);
        let bright_energy: f32 = bright.iter().map(|v| v * v).sum();

        // Different brightness should change the sound
        let diff: f32 = dark.iter().zip(bright.iter()).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 0.01, "Brightness should change sound, diff={diff}, dark_e={dark_energy}, bright_e={bright_energy}");
    }

    #[test]
    fn all_params_readable() {
        let s = Dx7Synth::new();
        assert_eq!(s.parameter_count(), PARAM_COUNT);
        for i in 0..PARAM_COUNT {
            assert!(s.parameter_info(i).is_some());
            let val = s.get_parameter(i);
            assert!((0.0..=1.0).contains(&val), "param {i} = {val}");
        }
    }

    #[test]
    fn the_two_selectors_span_the_whole_factory_set() {
        let mut s = Dx7Synth::new();
        s.set_parameter(P_BANK, 0.0);
        s.set_parameter(P_PATCH, 0.0);
        assert_eq!(s.current_voice(), 0);
        s.set_parameter(P_PATCH, 1.0);
        assert_eq!(s.current_voice(), PATCH_COUNT - 1, "the patch knob stops at the bank's edge");
        s.set_parameter(P_BANK, 1.0);
        assert_eq!(s.current_voice(), VOICE_COUNT - 1);
        s.set_parameter(P_PATCH, 0.0);
        assert_eq!(s.current_voice(), VOICE_COUNT - PATCH_COUNT, "first voice of the last bank");

        // Every voice is reachable, each from its own knob position, and the
        // round trip through `voice_knobs` is exact — a rounding error here
        // would silently make one voice unselectable and another appear twice.
        for voice in 0..VOICE_COUNT {
            let (bank, patch) = voice_knobs(voice);
            assert_eq!(voice_index(bank, patch), voice, "voice {voice} is not selectable");
        }

        // `params` is public, so the knobs can arrive as anything at all.
        for junk in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1e30, 1e30] {
            s.params[P_BANK] = junk;
            s.params[P_PATCH] = junk;
            assert!(s.current_voice() < VOICE_COUNT, "knob {junk} escaped the bank");
        }
    }

    #[test]
    fn stepping_a_selector_moves_exactly_one_voice() {
        // The defect this is here for: stepping by 1/32 of the knob's travel
        // accumulates a rounding error, and around the fourth or fifth press
        // the sum lands a few ulps below a step boundary — a keypress that
        // visibly does nothing. Stepping by index cannot do that.
        let mut knob = 0.0f32;
        for want in 1..PATCH_COUNT {
            knob = step_discrete(P_PATCH, knob, true);
            assert_eq!(patch_index(knob), want, "patch step {want}");
        }
        knob = step_discrete(P_PATCH, knob, true);
        assert_eq!(patch_index(knob), PATCH_COUNT - 1, "the top of the range holds");
        for want in (0..PATCH_COUNT - 1).rev() {
            knob = step_discrete(P_PATCH, knob, false);
            assert_eq!(patch_index(knob), want, "patch step down to {want}");
        }
        assert_eq!(patch_index(step_discrete(P_PATCH, knob, false)), 0, "the bottom holds");

        let mut knob = 0.0f32;
        for want in 1..BANK_COUNT {
            knob = step_discrete(P_BANK, knob, true);
            assert_eq!(bank_index(knob), want, "bank step {want}");
        }
        // Anything that is not a selector is left exactly where it was.
        for index in [P_FEEDBACK, P_BRIGHTNESS, P_GAIN] {
            assert_eq!(step_discrete(index, 0.42, true), 0.42);
        }
    }

    #[test]
    fn the_default_voice_is_the_electric_piano() {
        let s = Dx7Synth::new();
        assert_eq!(s.current_voice(), 10);
        assert_eq!(voice_name(s.current_voice()), "E.PIANO 1");
        assert_eq!(BANK_NAMES[bank_index(s.params[P_BANK])], "ROM1A");
    }

    /// Run one envelope, held down, and return `(atten_db, stage)` per sample.
    fn env_trace(
        rates: [u8; 4],
        levels: [u8; 4],
        samples: usize,
    ) -> Vec<(f64, DxEnvStage)> {
        let mut env = DxEnvelope::new(44100.0);
        env.set_from_preset(rates, levels);
        env.trigger();
        (0..samples)
            .map(|_| { env.tick(); (env.atten_db, env.stage) })
            .collect()
    }

    /// Largest minus smallest per-sample dB step while `stage` is running.
    fn slope_spread(trace: &[(f64, DxEnvStage)], stage: DxEnvStage) -> (f64, f64, usize) {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut n = 0;
        for w in trace.windows(2) {
            if w[0].1 != stage || w[1].1 != stage { continue; }
            let step = w[1].0 - w[0].0;
            lo = lo.min(step);
            hi = hi.max(step);
            n += 1;
        }
        (lo, hi, n)
    }

    #[test]
    fn dx_rate_conversion() {
        // The slope is a 2-bit mantissa times a power of two, straight off the EGS
        // rate word. Anchors are the qrate multiples of four, where a floating
        // reading of the exponent and this integer one agree.
        let anchors = [
            (0u8, 0.2819f64),
            (20, 2.2552),
            (50, 72.166),
            (70, 577.33),
        ];
        for (rate, expected) in anchors {
            let got = dx_rate_to_db_per_sec(rate);
            let err = (got - expected).abs() / expected;
            assert!(err < 0.01, "rate {rate}: {got} dB/s, expected {expected} ({:.2}%)", err * 100.0);
        }

        // Mantissa stops within an exponent step: qrate 60..63 is 1, 1.25, 1.5, 1.75
        // times the same power of two. qrate = rate*41/64, so rates 94..99 cover it.
        let base = dx_rate_to_db_per_sec(94);
        for (rate, mult) in [(94u8, 1.0f64), (96, 1.25), (97, 1.5), (99, 1.75)] {
            let got = dx_rate_to_db_per_sec(rate);
            assert!((got - base * mult).abs() < base * 1e-9, "rate {rate}: {got}, want {}", base * mult);
        }

        // The whole point of the qrate curve: the range is ~57,000:1, not ~100:1.
        let span = dx_rate_to_db_per_sec(99) / dx_rate_to_db_per_sec(0);
        assert!(span > 50_000.0, "rate range collapsed to {span}:1");

        // Rate 0 must take minutes to traverse the full level range, not seconds.
        let sweep_0 = SILENCE_DB / dx_rate_to_db_per_sec(0);
        assert!(sweep_0 > 200.0, "rate 0 full sweep is only {sweep_0} s");
        let sweep_99 = SILENCE_DB / dx_rate_to_db_per_sec(99);
        assert!(sweep_99 < 0.01, "rate 99 full sweep is {sweep_99} s");

        // Monotonic: turning a patch rate up must never make the stage slower.
        for r in 0..99u8 {
            assert!(dx_rate_to_db_per_sec(r + 1) >= dx_rate_to_db_per_sec(r), "rate {r} → {}", r + 1);
        }
    }

    #[test]
    fn dx_level_conversion() {
        // The lookup table below 20 is the part a flat 0.7526 dB/unit line gets
        // badly wrong: it is 5 dB out at level 10 and 18 dB out at level 1.
        assert_eq!(scaleoutlevel(0), 0);
        assert_eq!(scaleoutlevel(10), 31);
        assert_eq!(scaleoutlevel(19), 46);
        assert_eq!(scaleoutlevel(20), 48);
        assert_eq!(scaleoutlevel(99), 127);

        let anchor = |level: u8, db: f64| {
            let got = dx_level_to_atten_db(level);
            assert!((got - db).abs() < 0.01, "level {level}: {got} dB, expected {db}");
        };
        anchor(99, 0.0);
        anchor(20, 59.45);
        anchor(10, 72.25);
        anchor(0, 95.58);

        // Above 20 the curve is a straight 0.7526 dB per unit.
        for l in 20..99u8 {
            let step = dx_level_to_atten_db(l) - dx_level_to_atten_db(l + 1);
            assert!((step - LEVEL_STEP_DB).abs() < 1e-9, "level {l} step {step}");
        }
        // Below 20 the table pulls the curve away from that line, and never back
        // toward it — this is the part a flat 0.7526 dB/unit ramp gets wrong.
        let flat = |l: u8| f64::from(99 - i32::from(l)) * LEVEL_STEP_DB;
        let mut prev_gap = 0.0;
        for l in (0..20u8).rev() {
            let gap = dx_level_to_atten_db(l) - flat(l);
            assert!(gap >= prev_gap - 1e-9, "level {l}: table curve turned back toward the line");
            prev_gap = gap;
        }
        assert!((dx_level_to_atten_db(10) - flat(10) - 5.27).abs() < 0.02,
            "flat ramp should be ~5 dB out at level 10");
        assert!((dx_level_to_atten_db(1) - flat(1) - 18.06).abs() < 0.02,
            "flat ramp should be ~18 dB out at level 1");
    }

    #[test]
    fn velocity_is_a_log_domain_offset() {
        // Sensitivity 0 means no velocity response at all, at any velocity.
        for v in 0..=127u8 {
            assert_eq!(scale_velocity(v, 0), 0, "vel {v} at sens 0");
        }
        // Reference values from the EGS velocity table.
        assert_eq!(scale_velocity(127, 7), 224);
        assert_eq!(scale_velocity(1, 7), -3344);
        // Monotonic, and the offset really is additive in dB: doubling the level
        // step count doubles the dB, it does not scale a linear gain.
        for v in 0..127u8 {
            assert!(scale_velocity(v + 1, 7) >= scale_velocity(v, 7), "vel {v}");
        }
        let boost_db = f64::from(scale_velocity(127, 7)) * DB_PER_UNIT;
        assert!((boost_db - 5.27).abs() < 0.05, "max velocity boost {boost_db} dB");
        let cut_db = f64::from(scale_velocity(1, 7)) * DB_PER_UNIT;
        assert!((cut_db + 78.6).abs() < 0.1, "min velocity cut {cut_db} dB");
    }

    #[test]
    fn falling_segment_holds_a_constant_db_slope() {
        // This is the test the old one-pole implementation could not pass. A decay
        // to a *nonzero* sustain is where a one-pole gives itself away: its dB
        // slope flattens as it nears the target. The hardware never does that.
        let sr = 44100.0;
        for (label, levels) in [
            ("silent target", [99u8, 0, 0, 0]),
            ("sustain target", [99u8, 40, 40, 0]),
            ("high sustain target", [99u8, 85, 85, 0]),
        ] {
            let trace = env_trace([99, 40, 40, 40], levels, 300_000);
            let (lo, hi, n) = slope_spread(&trace, DxEnvStage::Decay1);
            assert!(n > 1000, "{label}: only {n} decay samples measured");
            let nominal = dx_rate_to_db_per_sec(40) / sr;
            let spread_db = (hi - lo) * sr;
            assert!(spread_db < 0.5,
                "{label}: dB/s spread {spread_db} over the segment (lo={lo}, hi={hi})");
            assert!((lo - nominal).abs() < 1e-9 && (hi - nominal).abs() < 1e-9,
                "{label}: slope {lo}..{hi} per sample, expected {nominal}");
        }
    }

    #[test]
    fn falling_segment_matches_the_rate_table() {
        // Time an unambiguous span — L1 99 (0 dB) down to L2 20 (59.45 dB) — and
        // check it against dB/s straight from the rate formula.
        let sr = 44100.0;
        for rate in [30u8, 50, 60, 70] {
            let mut env = DxEnvelope::new(sr);
            env.set_from_preset([99, rate, rate, rate], [99, 20, 20, 0]);
            env.trigger();
            let mut n = 0usize;
            for _ in 0..8_000_000 {
                env.tick();
                match env.stage {
                    DxEnvStage::Decay1 => n += 1,
                    DxEnvStage::Attack => {}
                    _ => break,
                }
            }
            let measured = dx_level_to_atten_db(20) / (n as f64 / sr);
            let expected = dx_rate_to_db_per_sec(rate);
            let err = (measured - expected).abs() / expected;
            assert!(err < 0.01, "rate {rate}: measured {measured} dB/s, formula {expected}");
        }
    }

    #[test]
    fn attack_is_not_the_mirror_of_the_decay() {
        // The attack has to be visibly different from a decay run backwards: it
        // starts part-way up, and its slope decelerates instead of holding.
        let trace = env_trace([50, 50, 50, 50], [99, 99, 99, 0], 400_000);

        // (a) The key-down jump: before a single sample is produced the level is
        // already 1716/4352 of the way up the log range, not at silence.
        let mut jumped = DxEnvelope::new(44100.0);
        jumped.set_from_preset([50, 50, 50, 50], [99, 99, 99, 0]);
        assert!((jumped.atten_db - SILENCE_DB).abs() < 1e-12, "should start shut");
        jumped.trigger();
        let jump_fraction = 1.0 - jumped.atten_db / SILENCE_DB;
        assert!((jump_fraction - 1716.0 / 4352.0).abs() < 1e-12,
            "jump landed {:.2}% up the log range, expected 39.43%", jump_fraction * 100.0);
        // The first produced sample sits just above it, one increment along.
        assert!(trace[0].0 < ATTACK_JUMP_DB && trace[0].0 > ATTACK_JUMP_DB - 2.0,
            "attack did not jump: first sample at {} dB, expected ≈{ATTACK_JUMP_DB}", trace[0].0);

        // (b) Decelerating: the first attack step is many times the last one, and
        // the spread is far outside what a constant-slope segment would show.
        let (lo, hi, n) = slope_spread(&trace, DxEnvStage::Attack);
        assert!(n > 1000, "only {n} attack samples");
        assert!(hi < 0.0, "attack must move toward full scale");
        assert!(lo / hi > 5.0,
            "attack slope barely varies ({lo}..{hi}); it is not headroom-scaled");

        // (c) And it is not a one-pole either: a one-pole's step is proportional to
        // the remaining distance, so step/distance would be constant. Here it is a
        // staircase of 17 integer bands, so that ratio changes by >2x.
        let ratios: Vec<f64> = trace.windows(2)
            .filter(|w| w[0].1 == DxEnvStage::Attack && w[1].1 == DxEnvStage::Attack)
            .map(|w| (w[0].0 - w[1].0) / w[0].0.max(1e-9))
            .collect();
        let rmin = ratios.iter().copied().fold(f64::INFINITY, f64::min);
        let rmax = ratios.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(rmax / rmin > 2.0,
            "attack step/distance ratio is nearly constant ({rmin}..{rmax}) — that is a one-pole");
    }

    #[test]
    fn segment_direction_is_decided_per_segment() {
        // TubBell-style levels: L2 is *below* L3, so decay 2 has to rise. An
        // implementation that hardcodes "only stage 1 rises" gets this backwards.
        let trace = env_trace([99, 70, 70, 70], [99, 88, 96, 0], 200_000);
        let d1: Vec<f64> = trace.iter()
            .filter(|(_, s)| *s == DxEnvStage::Decay1).map(|(a, _)| *a).collect();
        let d2: Vec<f64> = trace.iter()
            .filter(|(_, s)| *s == DxEnvStage::Decay2).map(|(a, _)| *a).collect();
        assert!(d1.len() > 10 && d2.len() > 10, "stages too short to measure");
        assert!(d1[d1.len() - 1] > d1[0], "decay 1 (L1 99 → L2 88) must fall");
        assert!(d2[d2.len() - 1] < d2[0], "decay 2 (L2 88 → L3 96) must rise");
        assert!((d2[d2.len() - 1] - dx_level_to_atten_db(96)).abs() < 0.01,
            "decay 2 did not land on L3");
    }

    #[test]
    fn retrigger_resumes_from_the_current_level() {
        let mut env = DxEnvelope::new(44100.0);
        env.set_from_preset([99, 99, 99, 20], [99, 99, 99, 0]);
        env.trigger();
        for _ in 0..2000 { env.tick(); }
        assert!(env.atten_db < 0.01, "should be wide open at the sustain");

        env.release();
        for _ in 0..20_000 { env.tick(); }
        let mid_release = env.atten_db;
        assert!(mid_release > 1.0 && mid_release < SILENCE_DB - 1.0,
            "release should be part-way down, got {mid_release}");

        env.trigger();
        assert!((env.atten_db - mid_release).abs() < 1e-9,
            "retrigger snapped from {mid_release} dB to {} dB instead of resuming", env.atten_db);
    }

    #[test]
    fn envelope_gain_tracks_its_own_db() {
        // The per-sample gain is advanced multiplicatively; it must not drift away
        // from the dB value it is supposed to represent.
        let mut env = DxEnvelope::new(44100.0);
        env.set_from_preset([70, 45, 45, 45], [99, 60, 30, 0]);
        env.trigger();
        for i in 0..400_000 {
            let gain = env.tick();
            if !env.is_active() { break; }
            let expected = atten_to_gain(env.atten_db);
            assert!((gain - expected).abs() <= expected * 1e-9 + 1e-12,
                "sample {i}: gain {gain} vs {expected} for {} dB", env.atten_db);
        }
    }

    /// An operator with nothing but an output level and a velocity sensitivity —
    /// no keyboard scaling of any kind.
    fn plain_op(output_level: u8, vel_sens: u8) -> OpPreset {
        OpPreset { output_level, vel_sens, ..OpPreset::neutral() }
    }

    #[test]
    fn operator_gain_pipeline() {
        // Full level, no velocity sensitivity: unity, by definition.
        let unity = operator_gain(&plain_op(99, 0), 60, 100);
        assert!((unity - 1.0).abs() < 1e-12, "level 99 sens 0 should be unity: {unity}");
        // Output level 0 is hard silence, so a muted operator really is inaudible.
        assert_eq!(operator_gain(&plain_op(0, 7), 60, 127), 0.0);
        // Velocity is an offset, so it moves the *dB*, not a fraction of the gain.
        let hard = 20.0 * operator_gain(&plain_op(99, 7), 60, 127).log10();
        let soft = 20.0 * operator_gain(&plain_op(99, 7), 60, 1).log10();
        assert!((hard - 5.27).abs() < 0.05, "velocity 127 at sens 7: {hard} dB");
        assert!((soft + 78.6).abs() < 0.1, "velocity 1 at sens 7: {soft} dB");
        // The same velocity applied to a quieter operator shifts it by the same dB.
        let ratio_at = |level: u8| {
            20.0 * (operator_gain(&plain_op(level, 7), 60, 127)
                / operator_gain(&plain_op(level, 7), 60, 64))
            .log10()
        };
        let (a, b) = (ratio_at(99), ratio_at(60));
        assert!((a - b).abs() < 1e-9, "velocity offset is not level-independent: {a} vs {b}");
    }

    #[test]
    fn time_knobs_lengthen_stages_monotonically() {
        let mut prev = 0.0;
        for step in 0..=20 {
            let s = time_scale(step as f32 / 20.0);
            assert!(s > prev, "knob {step} gave {s}, not longer than {prev}");
            prev = s;
        }
        assert!((time_scale(1.0) - 1.0).abs() < 1e-12, "knob 1.0 must be the patch rate");
        assert!(time_scale(0.0) < 0.02, "knob 0.0 should be much quicker: {}", time_scale(0.0));
    }

    #[test]
    fn algorithms_are_all_distinct() {
        // No two DX7 algorithms share a routing. Identical entries mean a
        // transcription error in the table.
        for a in 1..=32u8 {
            for b in (a + 1)..=32u8 {
                let (x, y) = (algorithm(a), algorithm(b));
                let same = x.modulates == y.modulates
                    && x.carriers == y.carriers
                    && x.feedback_src == y.feedback_src
                    && x.feedback_dst == y.feedback_dst;
                assert!(!same, "Algorithms {a} and {b} are identical");
            }
        }
    }

    #[test]
    fn algorithm_carriers_match_routing() {
        // A carrier is exactly an operator that modulates nothing, and every
        // index in the table must name a real operator.
        for n in 1..=32u8 {
            let alg = algorithm(n);
            let derived: Vec<usize> = (0..NUM_OPERATORS)
                .filter(|&i| alg.modulates[i].is_empty())
                .collect();
            assert_eq!(alg.carriers, derived.as_slice(),
                "Alg {n}: carrier list disagrees with modulation routing");
            assert!(!alg.carriers.is_empty(), "Alg {n} has no carriers");
            for targets in &alg.modulates {
                for &t in *targets {
                    assert!(t < NUM_OPERATORS, "Alg {n}: target {t} out of range");
                }
            }
            assert!(alg.feedback_src < NUM_OPERATORS, "Alg {n}: feedback_src out of range");
            assert!(alg.feedback_dst < NUM_OPERATORS, "Alg {n}: feedback_dst out of range");
        }
    }

    #[test]
    fn feedback_tap_reads_only_past_samples() {
        // DxVoice::tick scans operators from index 5 down to 0, so the feedback
        // source must have an index <= the destination; otherwise the tap would
        // read the source's current-sample output and close a zero-delay loop.
        for n in 1..=32u8 {
            let alg = algorithm(n);
            assert!(alg.feedback_src <= alg.feedback_dst,
                "Alg {n}: feedback src {} ticks before dst {}",
                alg.feedback_src, alg.feedback_dst);
        }
    }

    #[test]
    fn only_algs_4_and_6_use_a_multi_operator_loop() {
        for n in 1..=32u8 {
            let alg = algorithm(n);
            match n {
                4 => assert_eq!((alg.feedback_src, alg.feedback_dst), (3, 5), "Alg 4 is OP4→OP6"),
                6 => assert_eq!((alg.feedback_src, alg.feedback_dst), (4, 5), "Alg 6 is OP5→OP6"),
                _ => assert_eq!(alg.feedback_src, alg.feedback_dst,
                    "Alg {n} should be self-feedback"),
            }
        }
    }

    #[test]
    fn multi_operator_feedback_lands_on_op6() {
        // Algorithms 4 and 6 bend OP6's phase. Silencing OP6 must therefore make
        // feedback completely inaudible. If the loop were wired as self-feedback
        // on OP4 / OP5 — both carriers in these algorithms — muting OP6 would not
        // hide it, so this test fails for the self-feedback approximation.
        let op = |level: u8| OpPreset {
            output_level: level,
            rates: [99, 99, 99, 60],
            ..OpPreset::neutral()
        };
        for alg_num in [4u8, 6u8] {
            let mut patch = PatchPreset {
                algorithm: alg_num,
                feedback: 7,
                ops: [op(99), op(99), op(99), op(99), op(99), op(0)],
                ..PatchPreset::neutral()
            };
            let mut with_fb = DxVoice::new(44100.0);
            with_fb.note_on(60, 100, &patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
            patch.feedback = 0;
            let mut without_fb = DxVoice::new(44100.0);
            without_fb.note_on(60, 100, &patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);

            let mut peak = 0.0f32;
            for i in 0..512 {
                let a = with_fb.tick(LfoFrame::default());
                let b = without_fb.tick(LfoFrame::default());
                peak = peak.max(a.abs());
                assert_eq!(a, b,
                    "Alg {alg_num}: feedback must be inaudible with OP6 silent (sample {i})");
            }
            assert!(peak > 0.001, "Alg {alg_num}: test signal was silent, peak={peak}");
        }
    }

    #[test]
    fn sample_accurate_midi() {
        let mut s = Dx7Synth::new();
        s.init(44100.0, 128);
        let mut out = vec![0.0f32; 128];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 64)]);
        let pre_peak = out[..64].iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        let post_peak = out[64..].iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(pre_peak < 0.001, "Should be silent before note: {pre_peak}");
        assert!(post_peak > 0.001, "Should sound after note: {post_peak}");
    }

    // ── The factory ROM ──

    /// The 14 fully decoded reference voices, spread across all eight banks.
    ///
    /// Included verbatim rather than transcribed into Rust so that the fixture
    /// stays the artifact it was checked against, and so a decoder that agrees
    /// with itself cannot pass by agreeing with a typo.
    const REFERENCE: &str = include_str!("../tests/data/dx7_reference.json");

    fn u8_at(v: &serde_json::Value, key: &str) -> u8 {
        v[key].as_u64().unwrap_or_else(|| panic!("missing {key} in {v}")) as u8
    }

    fn quad(v: &serde_json::Value, key: &str) -> [u8; 4] {
        let a = v[key].as_array().unwrap_or_else(|| panic!("missing {key}"));
        std::array::from_fn(|i| a[i].as_u64().unwrap_or_default() as u8)
    }

    /// The parameter value a decoded scaling curve came from, written out
    /// here rather than read back through `ScaleCurve::from_bits` so that the
    /// documented mapping — 0 = -LIN, 1 = -EXP, 2 = +EXP, 3 = +LIN — is
    /// asserted rather than assumed.
    fn curve_bits(curve: ScaleCurve) -> u8 {
        match curve {
            ScaleCurve::LinNeg => 0,
            ScaleCurve::ExpNeg => 1,
            ScaleCurve::ExpPos => 2,
            ScaleCurve::LinPos => 3,
        }
    }

    /// The same, for the six LFO shapes.
    fn wave_bits(wave: LfoWave) -> u8 {
        match wave {
            LfoWave::Triangle => 0,
            LfoWave::SawDown => 1,
            LfoWave::SawUp => 2,
            LfoWave::Square => 3,
            LfoWave::Sine => 4,
            LfoWave::SampleHold => 5,
        }
    }

    /// Compare one decoded operator against its reference block.
    fn check_op(got: &OpPreset, want: &serde_json::Value, where_: &str) {
        assert_eq!(got.rates, quad(want, "rates"), "{where_}: EG rates");
        assert_eq!(got.levels, quad(want, "levels"), "{where_}: EG levels");
        assert_eq!(got.break_point, u8_at(want, "break_point"), "{where_}: break point");
        assert_eq!(got.left_depth, u8_at(want, "left_depth"), "{where_}: left depth");
        assert_eq!(got.right_depth, u8_at(want, "right_depth"), "{where_}: right depth");
        assert_eq!(curve_bits(got.left_curve), u8_at(want, "left_curve"), "{where_}: left curve");
        assert_eq!(curve_bits(got.right_curve), u8_at(want, "right_curve"), "{where_}: right curve");
        assert_eq!(got.rate_scaling, u8_at(want, "rate_scaling"), "{where_}: rate scaling");
        assert_eq!(got.detune, u8_at(want, "detune"), "{where_}: detune");
        assert_eq!(got.amp_mod_sens, u8_at(want, "amp_mod_sens"), "{where_}: amp mod sens");
        assert_eq!(got.vel_sens, u8_at(want, "vel_sens"), "{where_}: velocity sens");
        assert_eq!(got.output_level, u8_at(want, "output_level"), "{where_}: output level");
        assert_eq!(got.coarse, u8_at(want, "coarse"), "{where_}: coarse");
        assert_eq!(got.fine, u8_at(want, "fine"), "{where_}: fine");
        let mode = if u8_at(want, "mode") == 0 { OpFreqMode::Ratio } else { OpFreqMode::Fixed };
        assert_eq!(got.mode, mode, "{where_}: oscillator mode");
    }

    #[test]
    fn the_decoder_reproduces_the_reference_voices() {
        let reference: Vec<serde_json::Value> =
            serde_json::from_str(REFERENCE).expect("reference fixture is not valid JSON");
        assert_eq!(reference.len(), 14, "the fixture covers 14 voices");

        for want in &reference {
            let index = want["index"].as_u64().expect("index") as usize;
            let got = &presets()[index];
            let where_ = format!("{} voice {index}", want["bank"].as_str().unwrap_or("?"));

            assert_eq!(got.name(), want["name"].as_str().unwrap_or(""), "{where_}: name");
            assert_eq!(BANK_NAMES[index / PATCH_COUNT], want["bank"].as_str().unwrap_or(""),
                "{where_}: bank");
            assert_eq!(got.algorithm, u8_at(want, "algorithm"), "{where_}: algorithm");
            assert_eq!(got.feedback, u8_at(want, "feedback"), "{where_}: feedback");
            assert_eq!(got.transpose, u8_at(want, "transpose"), "{where_}: transpose");
            assert_eq!(got.osc_key_sync, u8_at(want, "osc_key_sync") != 0, "{where_}: key sync");

            let lfo = &want["lfo"];
            assert_eq!(got.lfo.speed, u8_at(lfo, "speed"), "{where_}: LFO speed");
            assert_eq!(got.lfo.delay, u8_at(lfo, "delay"), "{where_}: LFO delay");
            assert_eq!(got.lfo.pmd, u8_at(lfo, "pmd"), "{where_}: LFO PMD");
            assert_eq!(got.lfo.amd, u8_at(lfo, "amd"), "{where_}: LFO AMD");
            assert_eq!(got.lfo.pitch_mod_sens, u8_at(lfo, "pms"), "{where_}: LFO PMS");
            assert_eq!(got.lfo.sync, u8_at(lfo, "sync") != 0, "{where_}: LFO key sync");
            assert_eq!(wave_bits(got.lfo.waveform), u8_at(lfo, "wave"), "{where_}: LFO waveform");

            let peg = &want["pitch_eg"];
            assert_eq!(got.pitch_eg.rates, quad(peg, "rates"), "{where_}: pitch EG rates");
            assert_eq!(got.pitch_eg.levels, quad(peg, "levels"), "{where_}: pitch EG levels");

            // OP1 and OP6 are the two ends of the packed operator block, so a
            // decoder that reversed the order or lost its place would show up
            // in one or the other.
            check_op(&got.ops[0], &want["op1"], &format!("{where_} op1"));
            check_op(&got.ops[5], &want["op6"], &format!("{where_} op6"));
        }
    }

    #[test]
    fn every_factory_voice_decodes_in_range() {
        // Four values in the ROM are out of range — `TIMPANI` and `HORNS` carry
        // an EG level of 127, `E.GRAND 1` an EG rate of 127 and `60-S ORGAN` a
        // frequency fine of 100 — and they are clamped on the way in. After
        // that every field of every voice is inside the domain its parameter
        // is defined on, which is the property the rest of the engine assumes.
        for (voice, patch) in presets().iter().enumerate() {
            let name = patch.name();
            let at = |what: &str| format!("voice {voice} ({name}): {what}");
            assert!((1..=32).contains(&patch.algorithm), "{}", at("algorithm out of range"));
            assert!(patch.feedback <= 7, "{}", at("feedback out of range"));
            assert!(patch.transpose <= 48, "{}", at("transpose out of range"));
            assert!(patch.lfo.speed <= 99 && patch.lfo.delay <= 99, "{}", at("LFO time"));
            assert!(patch.lfo.pmd <= 99 && patch.lfo.amd <= 99, "{}", at("LFO depth"));
            assert!(patch.lfo.pitch_mod_sens <= 7, "{}", at("pitch mod sensitivity"));
            for (i, (&rate, &level)) in
                patch.pitch_eg.rates.iter().zip(&patch.pitch_eg.levels).enumerate()
            {
                assert!(rate <= 99, "{}", at(&format!("pitch EG rate {i}")));
                assert!(level <= 99, "{}", at(&format!("pitch EG level {i}")));
            }
            assert!(patch.name.iter().all(|c| (0x20..0x7F).contains(c)),
                "{}", at("name is not printable ASCII"));

            for (o, op) in patch.ops.iter().enumerate() {
                let at = |what: &str| format!("voice {voice} ({name}) op{}: {what}", o + 1);
                assert!(op.rates.iter().all(|&r| r <= 99), "{}", at("EG rate out of range"));
                assert!(op.levels.iter().all(|&l| l <= 99), "{}", at("EG level out of range"));
                assert!(op.output_level <= 99, "{}", at("output level out of range"));
                assert!(op.break_point <= 99, "{}", at("break point out of range"));
                assert!(op.left_depth <= 99 && op.right_depth <= 99, "{}", at("depth out of range"));
                assert!(op.rate_scaling <= 7, "{}", at("rate scaling out of range"));
                assert!(op.vel_sens <= 7, "{}", at("velocity sensitivity out of range"));
                assert!(op.amp_mod_sens <= 3, "{}", at("amp mod sensitivity out of range"));
                assert!(op.detune <= 14, "{}", at("detune out of range"));
                assert!(op.coarse <= 31, "{}", at("coarse out of range"));
                assert!(op.fine <= 99, "{}", at("fine out of range"));
                // The ratio is the coarse/fine grid expression and nothing else,
                // which is all a DX7 can dial.
                let base = if op.coarse == 0 { 0.5 } else { f64::from(op.coarse) };
                assert!((op.ratio() - base * (1.0 + f64::from(op.fine) / 100.0)).abs() < 1e-12,
                    "{}", at("ratio is off the hardware grid"));
            }
        }
    }

    #[test]
    fn the_four_out_of_spec_rom_values_are_clamped() {
        // Named individually because they are the only four, and because a
        // decoder that silently stopped clamping would otherwise only show up
        // as a wrong envelope somewhere in a 256-voice bank.
        let at = |bank: usize, voice: usize| &presets()[bank * PATCH_COUNT + voice];
        // ROM3A:19 TIMPANI, op2 EG level 3.
        let timpani = at(4, 19);
        assert_eq!(timpani.name(), "TIMPANI");
        assert_eq!(timpani.ops[1].levels[2], 99);
        // ROM3B:01 E.GRAND 1, op6 EG rate 1.
        let grand = at(5, 1);
        assert_eq!(grand.name(), "E.GRAND 1");
        assert_eq!(grand.ops[5].rates[0], 99);
        // ROM3B:14 60-S ORGAN, op1 frequency fine.
        let organ = at(5, 14);
        assert_eq!(organ.name(), "60-S ORGAN");
        assert_eq!(organ.ops[0].fine, 99);
        // ROM4A:07 HORNS, op5 EG level 4.
        let horns = at(6, 7);
        assert_eq!(horns.name(), "HORNS");
        assert_eq!(horns.ops[4].levels[3], 99);
    }

    #[test]
    fn the_factory_set_is_the_shape_the_cartridges_are() {
        // A census rather than a spot check: these counts are what the eight
        // cartridges contain, and every one of them is a feature of the engine
        // that the hand-authored bank this replaced never exercised. A decoder
        // that lost a bit somewhere would move at least one of them.
        let bank = presets();
        let count = |f: fn(&PatchPreset) -> bool| bank.iter().filter(|p| f(p)).count();

        assert_eq!(count(|p| p.transpose != 24), 94, "voices transposed off centre");
        assert_eq!(count(|p| !p.osc_key_sync), 74, "voices with free-running operators");
        assert_eq!(count(|p| p.ops.iter().any(|o| o.left_depth > 0 || o.right_depth > 0)),
            166, "voices using keyboard level scaling");
        assert_eq!(count(|p| p.ops.iter().any(|o| o.rate_scaling > 0)),
            200, "voices using keyboard rate scaling");
        assert_eq!(count(|p| p.lfo.pmd > 0 || p.lfo.amd > 0), 109, "voices using the LFO");
        assert_eq!(count(|p| p.pitch_eg.levels != [50; 4]), 37, "voices using the pitch EG");
        assert_eq!(count(|p| p.ops.iter().any(|o| o.amp_mod_sens > 0)),
            40, "voices using amplitude modulation");
        assert_eq!(
            bank.iter().flat_map(|p| p.ops.iter()).filter(|o| o.mode == OpFreqMode::Fixed).count(),
            37, "fixed-frequency operators");

        // All four scaling curves and all six LFO shapes are reached.
        for curve in [ScaleCurve::LinNeg, ScaleCurve::ExpNeg, ScaleCurve::ExpPos, ScaleCurve::LinPos] {
            assert!(bank.iter().any(|p| p.ops.iter().any(|o| o.left_curve == curve)),
                "no voice uses curve {curve:?}");
        }
        for wave in [LfoWave::Triangle, LfoWave::SawDown, LfoWave::SawUp, LfoWave::Square,
                     LfoWave::Sine, LfoWave::SampleHold] {
            assert!(bank.iter().any(|p| p.lfo.waveform == wave), "no voice uses {wave:?}");
        }

        // Transpose only ever appears in the seven positions the factory set
        // uses, all of them whole intervals: -24, -12, -7, -3, 0, +12, +24.
        let mut transposes: Vec<u8> = bank.iter().map(|p| p.transpose).collect();
        transposes.sort_unstable();
        transposes.dedup();
        assert_eq!(transposes, [0, 12, 17, 21, 24, 36, 48]);
    }

    #[test]
    fn voice_names_come_from_the_rom() {
        assert_eq!(voice_name(0), "BRASS   1");
        assert_eq!(voice_name(10), "E.PIANO 1");
        assert_eq!(voice_name(VOICE_COUNT - 1), "EXPLOSION");
        // Out of range saturates rather than panicking; `params` is public.
        assert_eq!(voice_name(VOICE_COUNT), voice_name(VOICE_COUNT - 1));

        // Names are padded in the ROM and trimmed here, never longer than the
        // 10 characters the display is sized for, and never blank.
        for voice in 0..VOICE_COUNT {
            let name = voice_name(voice);
            assert!(!name.is_empty(), "voice {voice} has no name");
            assert!(name.len() <= NAME_LEN, "voice {voice} name {name:?} is too long");
            assert!(!name.ends_with(' '), "voice {voice} name {name:?} is padded");
        }
        // 194 of the 256 are distinct: several voices appear on more than one
        // cartridge, which is the cartridges' doing and why the bank is part of
        // the display.
        let unique: std::collections::BTreeSet<&str> =
            (0..VOICE_COUNT).map(voice_name).collect();
        assert_eq!(unique.len(), 194);
    }

    #[test]
    fn the_selector_labels_name_the_voice_and_the_cartridge() {
        let mut params = PARAM_DEFAULTS;
        assert_eq!(discrete_label(&params, P_PATCH), Some("E.PIANO 1"));
        assert_eq!(discrete_label(&params, P_BANK), Some("ROM1A"));
        assert_eq!(discrete_label(&params, P_GAIN), None, "only the selectors are labelled");
        assert!(is_discrete(P_PATCH) && is_discrete(P_BANK));
        assert!(!is_discrete(P_GAIN) && !is_discrete(P_FEEDBACK));

        // The same patch knob reads a different voice on a different cartridge.
        let (bank, patch) = voice_knobs(4 * PATCH_COUNT + 19);
        params[P_BANK] = bank;
        params[P_PATCH] = patch;
        assert_eq!(discrete_label(&params, P_BANK), Some("ROM3A"));
        assert_eq!(discrete_label(&params, P_PATCH), Some("TIMPANI"));

        // A short parameter block — a session saved before the bank knob
        // existed — reads as the first cartridge rather than panicking.
        assert_eq!(discrete_label(&params[..P_BANK], P_BANK), Some("ROM1A"));
    }

    // ── Operator frequency ──

    fn cents(a: f64, b: f64) -> f64 {
        1200.0 * (a / b).log2()
    }

    #[test]
    fn ratio_grid_endpoints() {
        let at = |coarse, fine| OpPreset { coarse, fine, ..OpPreset::neutral() }.ratio();
        // Coarse 0 is the half-rate case, not silence or unity.
        assert_eq!(at(0, 0), 0.5);
        assert_eq!(at(1, 0), 1.0);
        assert_eq!(at(31, 0), 31.0);
        // Fine is a percentage on top of the coarse ratio.
        assert!((at(1, 50) - 1.5).abs() < 1e-12);
        assert!((at(0, 99) - 0.995).abs() < 1e-12);
        assert!((at(31, 99) - 61.69).abs() < 1e-9);
        // Coarse wraps at 32, as the hardware's `coarse & 31` does.
        assert_eq!(at(32, 0), 0.5);
    }

    #[test]
    fn ratio_mode_frequency_is_the_note_times_the_ratio() {
        for note in [24u8, 60, 96] {
            for (coarse, fine) in [(0u8, 0u8), (1, 0), (3, 0), (2, 71), (14, 0)] {
                let op = OpPreset { coarse, fine, ..OpPreset::neutral() };
                let want = note_to_freq(note) * op.ratio();
                assert_eq!(op.frequency(note), want,
                    "coarse {coarse} fine {fine} note {note}");
            }
        }
    }

    #[test]
    fn detune_is_a_frequency_dependent_offset() {
        let op = |detune| OpPreset { detune, ..OpPreset::neutral() };
        // Centre is exactly neutral — not approximately, exactly. Anything else
        // would detune every preset in the bank.
        for note in 0..=127u8 {
            assert_eq!(op(7).detune_factor(note), 1.0, "note {note} at centre detune");
        }
        // Above centre sharpens, below flattens, and it is monotonic.
        let mut prev = f64::NEG_INFINITY;
        for d in 0..=14u8 {
            let f = op(d).detune_factor(69);
            assert!(f > prev, "detune {d} did not raise pitch above {}", d.saturating_sub(1));
            prev = f;
        }
        assert!(op(8).detune_factor(69) > 1.0);
        assert!(op(6).detune_factor(69) < 1.0);
        // Size at A440: about one cent per step, so the full range is ~7 cents.
        let step_at_a440 = cents(op(8).detune_factor(69), 1.0);
        assert!((step_at_a440 - 0.97).abs() < 0.05, "one detune step at A440 is {step_at_a440} cents");
        // And it is *not* a fixed number of cents — down at C1 it is about two
        // and a half times bigger. This is the part a naive "detune = n cents"
        // model gets wrong, and it is why two operators an octave apart with the
        // same detune setting do not beat at the same rate.
        let step_at_c1 = cents(op(8).detune_factor(24), 1.0);
        assert!((step_at_c1 - 2.46).abs() < 0.05, "one detune step at C1 is {step_at_c1} cents");
        let ratio = step_at_c1 / step_at_a440;
        assert!(ratio > 2.4, "low-note detune is only {ratio:.2}x the A440 step");
    }

    #[test]
    fn fixed_mode_ignores_the_played_note() {
        let fixed = |coarse, fine| OpPreset {
            mode: OpFreqMode::Fixed, coarse, fine, ..OpPreset::neutral()
        };
        // Same frequency whatever is played — that is the whole point of the mode.
        let op = fixed(2, 50);
        let f = op.frequency(0);
        for note in 0..=127u8 {
            assert_eq!(op.frequency(note), f, "fixed operator moved at note {note}");
        }
        // Four decades from 1 Hz, using only the low two bits of coarse.
        assert!((fixed(0, 0).frequency(60) - 1.0).abs() < 1e-6);
        assert!((fixed(1, 0).frequency(60) - 10.0).abs() < 1e-3);
        assert!((fixed(3, 99).frequency(60) - 9772.0).abs() < 1.0);
        // Coarse above 3 wraps, so 4 is the same as 0.
        assert_eq!(fixed(4, 0).frequency(60), fixed(0, 0).frequency(60));
        // Detune only moves it up here, and only above centre.
        let base = fixed(2, 0).frequency(60);
        let up = OpPreset { detune: 14, ..fixed(2, 0) }.frequency(60);
        let down = OpPreset { detune: 0, ..fixed(2, 0) }.frequency(60);
        assert!(up > base, "detune above centre should sharpen a fixed operator");
        assert_eq!(down, base, "detune below centre does nothing in fixed mode");
    }

    // ── Keyboard level scaling ──

    #[test]
    fn scale_curve_signs_and_shapes() {
        // Depth 0 is silent in every curve — this is what makes the neutral
        // default genuinely neutral.
        for curve in [ScaleCurve::LinNeg, ScaleCurve::ExpNeg, ScaleCurve::ExpPos, ScaleCurve::LinPos] {
            for group in 0..40 {
                assert_eq!(scale_curve(group, 0, curve), 0, "curve {curve:?} group {group}");
            }
        }
        // Curves 0 and 1 cut, 2 and 3 boost.
        assert!(scale_curve(8, 99, ScaleCurve::LinNeg) < 0);
        assert!(scale_curve(8, 99, ScaleCurve::ExpNeg) < 0);
        assert!(scale_curve(8, 99, ScaleCurve::ExpPos) > 0);
        assert!(scale_curve(8, 99, ScaleCurve::LinPos) > 0);
        // Linear is a straight line in the level domain: one group at full depth
        // is 7 units, four groups (an octave) is 31.
        assert_eq!(scale_curve(1, 99, ScaleCurve::LinPos), 7);
        assert_eq!(scale_curve(4, 99, ScaleCurve::LinPos), 31);
        assert_eq!(scale_curve(8, 99, ScaleCurve::LinPos), 63);
        // The exponential table is normalised to arrive at roughly the same place
        // as the linear curve at the far end of the keyboard, so it is shallower
        // than linear everywhere in between rather than steeper — the difference
        // is the *shape*, not the endpoint.
        assert_eq!(scale_curve(4, 99, ScaleCurve::ExpPos), 3);   // linear is 31 here
        assert_eq!(scale_curve(32, 99, ScaleCurve::ExpPos), 248); // linear is 254
        for group in 1..32 {
            assert!(scale_curve(group, 99, ScaleCurve::ExpPos) < scale_curve(group, 99, ScaleCurve::LinPos),
                "exp curve overtook linear at group {group}");
        }
        // Convexity is what makes it worth having: doubling the distance from the
        // breakpoint roughly doubles the linear curve but more than quadruples
        // the exponential one.
        let grow = |curve| {
            f64::from(scale_curve(16, 99, curve)) / f64::from(scale_curve(8, 99, curve))
        };
        assert!((grow(ScaleCurve::LinPos) - 2.0).abs() < 0.05);
        assert!(grow(ScaleCurve::ExpPos) > 4.0, "exp curve is not convex: {}", grow(ScaleCurve::ExpPos));
        // Saturates once the table runs out at group 32.
        assert_eq!(scale_curve(32, 99, ScaleCurve::ExpPos), scale_curve(60, 99, ScaleCurve::ExpPos));
    }

    #[test]
    fn scale_level_hinges_at_the_breakpoint() {
        let op = OpPreset {
            break_point: 43, // hinge at middle C
            left_depth: 99,
            right_depth: 99,
            left_curve: ScaleCurve::LinPos,
            right_curve: ScaleCurve::LinNeg,
            ..OpPreset::neutral()
        };
        // One three-semitone group either side of the hinge is dead.
        for note in 59..=61u8 {
            assert_eq!(scale_level(note, &op), 0, "note {note} should sit in the hinge group");
        }
        // Right of the hinge uses the right depth and curve (cutting here)...
        assert!(scale_level(72, &op) < 0);
        assert!(scale_level(96, &op) < scale_level(72, &op));
        // ...and left uses the left ones (boosting here). The two sides are
        // genuinely independent, which is the whole reason there are four
        // parameters instead of two.
        assert!(scale_level(48, &op) > 0);
        assert!(scale_level(24, &op) > scale_level(48, &op));
        // An octave at full linear depth is 31 units either way.
        assert_eq!(scale_level(72, &op), -31);
        assert_eq!(scale_level(48, &op), 31);
    }

    #[test]
    fn key_level_scaling_changes_level_across_the_keyboard() {
        // The audible claim: with a depth set, the same operator is a different
        // number of dB at the top of the keyboard than at the bottom.
        let scaled = OpPreset {
            break_point: 43,
            right_depth: 60,
            right_curve: ScaleCurve::LinNeg,
            ..OpPreset::neutral()
        };
        let db = |op: &OpPreset, note: u8| 20.0 * operator_gain(op, note, 100).log10();
        // Breakpoint 43 hinges at MIDI 60, so note 72 is four three-semitone
        // groups up: 19 level units at depth 60, and it stacks linearly from
        // there — three octaves is 57 units.
        let octave = db(&scaled, 60) - db(&scaled, 72);
        assert!((octave - 19.0 * LEVEL_STEP_DB).abs() < 0.01,
            "an octave at depth 60 should cut 14.3 dB, got {octave:.2}");
        let three = db(&scaled, 60) - db(&scaled, 96);
        assert!((three - 57.0 * LEVEL_STEP_DB).abs() < 0.01,
            "three octaves at depth 60 should cut 42.9 dB, got {three:.2}");
        // Positive curves boost instead, and the boost is clamped at level 99 —
        // the level-scaling sum is capped before the shift into fine units.
        let boosted = OpPreset { right_curve: ScaleCurve::LinPos, ..scaled };
        assert_eq!(db(&boosted, 96), db(&boosted, 60),
            "an operator already at level 99 cannot be boosted further");
        let quiet = OpPreset { output_level: 50, ..boosted };
        assert!(db(&quiet, 96) > db(&quiet, 60) + 6.0,
            "a quieter operator has headroom and should be boosted");
        // Left and right are separate: with only a right depth set, everything
        // below the breakpoint is untouched.
        assert_eq!(db(&scaled, 36), db(&scaled, 60));
    }

    // ── Keyboard rate scaling ──

    #[test]
    fn scale_rate_reference_values() {
        // Sensitivity 0 is inert at every note.
        for note in 0..=127u8 {
            assert_eq!(scale_rate(note, 0), 0, "note {note}");
        }
        // Three-semitone buckets, clamped flat at the bottom and top.
        assert_eq!(scale_rate(0, 7), 0);
        assert_eq!(scale_rate(23, 7), 0);
        assert_eq!(scale_rate(24, 7), 0); // 24/3 - 7 = 1, (7*1)>>3 = 0
        assert_eq!(scale_rate(60, 7), 11);
        assert_eq!(scale_rate(114, 7), 27);
        assert_eq!(scale_rate(127, 7), 27); // saturated
        // Monotonic in both note and sensitivity.
        for note in 0..127u8 {
            assert!(scale_rate(note + 1, 7) >= scale_rate(note, 7), "note {note}");
        }
        for sens in 0..7u8 {
            assert!(scale_rate(96, sens + 1) >= scale_rate(96, sens), "sens {sens}");
        }
    }

    #[test]
    fn rate_scaling_zero_is_the_unscaled_rate() {
        // The scaled slope function is the one production calls; it must agree
        // bit for bit with the plain one whenever scaling is off, or every
        // existing patch changes. The plain one is now a delegation to it, so
        // this pins that delegation — and pinned it across all 100 rates while
        // the two really were separate copies of the arithmetic.
        for rate in 0..=99u8 {
            assert_eq!(dx_rate_to_db_per_sec_scaled(rate, 0), dx_rate_to_db_per_sec(rate),
                "rate {rate}");
        }
        // And the same at the envelope level.
        let (rates, levels) = ([96u8, 64, 30, 10], [99u8, 90, 75, 0]);
        let mut plain = DxEnvelope::new(44100.0);
        plain.set_from_preset(rates, levels);
        let mut scaled = DxEnvelope::new(44100.0);
        scaled.set_from_preset_scaled(rates, levels, 0);
        assert_eq!(plain.step_db, scaled.step_db);
        assert_eq!(plain.target_db, scaled.target_db);
    }

    #[test]
    fn rate_scaling_is_added_to_the_quantised_rate() {
        // A delta of four qrate steps is exactly one doubling of the slope. This
        // is the check that distinguishes adding the delta in the right domain
        // from adding it to the 0-99 patch rate, which would give a much smaller
        // and rate-dependent change.
        for rate in [20u8, 40, 60, 80] {
            let base = dx_rate_to_db_per_sec_scaled(rate, 0);
            for octaves in 1..=3 {
                let got = dx_rate_to_db_per_sec_scaled(rate, 4 * octaves);
                let want = base * f64::exp2(f64::from(octaves));
                assert!((got - want).abs() < want * 1e-9,
                    "rate {rate} + {} qrate: {got}, expected {want}", 4 * octaves);
            }
        }
        // The delta saturates with the rate word, it does not wrap.
        assert_eq!(dx_rate_to_db_per_sec_scaled(99, 40), dx_rate_to_db_per_sec_scaled(99, 0));
    }

    /// Samples until a held envelope reaches its sustain, for a given rate
    /// scaling delta.
    fn samples_to_sustain(rates: [u8; 4], levels: [u8; 4], qrate_delta: i32) -> usize {
        let mut env = DxEnvelope::new(44100.0);
        env.set_from_preset_scaled(rates, levels, qrate_delta);
        env.trigger();
        for n in 0..4_000_000usize {
            env.tick();
            if env.holding || !env.is_active() { return n; }
        }
        usize::MAX
    }

    #[test]
    fn rate_scaling_shortens_envelopes_on_higher_notes() {
        // Same patch, two notes three octaves apart, full rate scaling.
        let (rates, levels) = ([70u8, 50, 50, 50], [99u8, 40, 0, 0]);
        let low = samples_to_sustain(rates, levels, scale_rate(36, 7));
        let high = samples_to_sustain(rates, levels, scale_rate(96, 7));
        assert!(low > 100 && high > 10, "envelopes too short to measure: {low}, {high}");
        // 36 -> delta 4, 96 -> delta 21: 17 qrate steps, so about 2^(17/4) = 19x.
        let speedup = low as f64 / high as f64;
        assert!(speedup > 15.0 && speedup < 25.0,
            "expected ~19x faster at the top of the keyboard, got {speedup:.1}x");
        // With scaling off the two notes are identical, to the sample.
        let low0 = samples_to_sustain(rates, levels, scale_rate(36, 0));
        let high0 = samples_to_sustain(rates, levels, scale_rate(96, 0));
        assert_eq!(low0, high0, "rate scaling 0 must not depend on the note");
    }

    // ── The neutral-defaults contract ──

    /// Keyboard level scaling cannot make an operator louder than a
    /// full-level one, however deep the taper.
    ///
    /// The hand-authored bank this replaced only ever cut, and the test here
    /// asserted exactly that — no patch louder at the top of the keyboard than
    /// at the bottom. The factory set uses all four curves at depths up to 99,
    /// and the two positive ones deliberately boost as the keyboard climbs, so
    /// the property worth holding is the one the hardware guarantees instead:
    /// scaling is summed into the coarse level domain and clamped at its top
    /// *before* velocity is added, so the deepest +LIN curve in the bank can
    /// reach level 99 and no further.
    #[test]
    fn key_level_scaling_cannot_exceed_a_full_level_operator() {
        for (voice, patch) in presets().iter().enumerate() {
            for (o, op) in patch.ops.iter().enumerate() {
                // Same velocity sensitivity, so the only difference between the
                // two is the output level and the keyboard taper.
                let full = OpPreset { output_level: 99, vel_sens: op.vel_sens,
                                      ..OpPreset::neutral() };
                for note in (0..=127u8).step_by(3) {
                    let ceiling = operator_gain(&full, note, 127);
                    assert!(operator_gain(op, note, 127) <= ceiling,
                        "voice {voice} ({}) op{}: note {note} is louder than a full-level \
                         operator", patch.name(), o + 1);
                }
            }
        }
        // ...and the boost really is reached, so the clamp above is being tested
        // against something rather than against a bank that only ever cuts.
        let boosting = presets().iter().filter(|p| p.ops.iter().any(|op| {
            operator_gain(op, 96, 100) > operator_gain(op, 60, 100)
        })).count();
        assert!(boosting > 20, "only {boosting} voices boost up the keyboard");
    }

    #[test]
    fn neutral_defaults_are_inert_at_every_note() {
        let op = OpPreset::neutral();
        for note in 0..=127u8 {
            // No level offset, whatever the breakpoint happens to be...
            assert_eq!(scale_level(note, &op), 0, "note {note}");
            // ...no rate offset...
            assert_eq!(scale_rate(note, op.rate_scaling), 0, "note {note}");
            // ...no detune...
            assert_eq!(op.detune_factor(note), 1.0, "note {note}");
            // ...and therefore a gain that does not depend on the note at all.
            assert_eq!(operator_gain(&op, note, 100), operator_gain(&op, 60, 100), "note {note}");
        }
        // Belt and braces: a nonzero breakpoint cannot leak in either.
        let moved = OpPreset { break_point: 0, ..op };
        for note in 0..=127u8 {
            assert_eq!(scale_level(note, &moved), 0, "note {note} with breakpoint 0");
        }
    }

    /// A patch whose six operators are all identical carriers (algorithm 32 is
    /// fully additive), so anything measured at the output is that one operator.
    fn additive_patch(op: OpPreset) -> PatchPreset {
        PatchPreset {
            algorithm: 32,
            feedback: 0,
            ops: [op; NUM_OPERATORS],
            ..PatchPreset::neutral()
        }
    }

    /// Total energy of one note over `samples`, rendered through a voice.
    fn voice_energy(patch: &PatchPreset, note: u8, samples: usize) -> f64 {
        let mut v = DxVoice::new(44100.0);
        v.note_on(note, 100, patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
        (0..samples).map(|_| f64::from(v.tick(LfoFrame::default())).powi(2)).sum()
    }

    #[test]
    fn key_scaling_reaches_the_audio_output() {
        // Level scaling, end to end. Without a depth the keyboard is flat...
        let flat = additive_patch(OpPreset {
            rates: [99, 50, 50, 50], levels: [99, 99, 99, 0], ..OpPreset::neutral()
        });
        let (lo, hi) = (voice_energy(&flat, 48, 44100), voice_energy(&flat, 84, 44100));
        assert!((lo / hi - 1.0).abs() < 0.02, "flat keyboard is not flat: {lo} vs {hi}");

        // ...and with one, the top of the keyboard is quieter by exactly the
        // amount the curve asks for. Note 48 is below the breakpoint and the left
        // depth is 0, so it is untouched; note 84 is eight three-semitone groups
        // above it, which at depth 99 on a -LIN curve is 63 level units.
        let sloped = additive_patch(OpPreset {
            rates: [99, 50, 50, 50], levels: [99, 99, 99, 0],
            break_point: 43, right_depth: 99, right_curve: ScaleCurve::LinNeg,
            ..OpPreset::neutral()
        });
        let (lo, hi) = (voice_energy(&sloped, 48, 44100), voice_energy(&sloped, 84, 44100));
        let cut_db = 10.0 * (lo / hi).log10();
        assert!((cut_db - 63.0 * LEVEL_STEP_DB).abs() < 0.2,
            "level scaling should cut 47.4 dB at the top, measured {cut_db:.2}");
    }

    #[test]
    fn rate_scaling_reaches_the_audio_output() {
        // Rate scaling, end to end: a decaying patch, measured over a fixed
        // window, must carry visibly less energy at the top of the keyboard.
        let env = ([99u8, 55, 55, 55], [99u8, 0, 0, 0]);
        let flat = additive_patch(OpPreset {
            rates: env.0, levels: env.1, ..OpPreset::neutral()
        });
        let (lo, hi) = (voice_energy(&flat, 48, 44100), voice_energy(&flat, 84, 44100));
        assert!((lo / hi - 1.0).abs() < 0.02, "without rate scaling both notes decay alike");

        // Note 48 gets a qrate delta of 7 and note 84 one of 18. Eleven qrate
        // steps is 2^(11/4) = 6.73x the slope, and since energy under a straight
        // dB decay goes as 1/slope that is the ceiling on the energy ratio; the
        // attack and the finite window keep it a little under.
        let keyed = additive_patch(OpPreset {
            rates: env.0, levels: env.1, rate_scaling: 7, ..OpPreset::neutral()
        });
        assert_eq!((scale_rate(48, 7), scale_rate(84, 7)), (7, 18));
        let (lo, hi) = (voice_energy(&keyed, 48, 44100), voice_energy(&keyed, 84, 44100));
        let speedup = lo / hi;
        let ceiling = f64::exp2(11.0 / 4.0);
        assert!(speedup > 4.0 && speedup < ceiling * 1.05,
            "high notes should decay ~{ceiling:.1}x sooner, measured {speedup:.2}x ({lo} vs {hi})");
    }

    // ── LFO ──

    const SR: f64 = 44100.0;

    fn lfo_with(preset: LfoPreset) -> Lfo {
        let mut lfo = Lfo::new(SR);
        lfo.configure(&preset);
        lfo
    }

    /// Cycles the LFO completes in one second, counted off the phase accumulator
    /// wrapping rather than off the waveform, so it is shape-independent.
    fn measured_hz(preset: LfoPreset) -> f64 {
        let mut lfo = lfo_with(preset);
        let mut wraps = 0u32;
        let mut prev = lfo.phase;
        for _ in 0..SR as usize {
            lfo.next_value();
            if lfo.phase < prev { wraps += 1; }
            prev = lfo.phase;
        }
        f64::from(wraps)
    }

    #[test]
    fn lfo_runs_at_the_speed_the_table_asks_for() {
        // Whole-cycle counting, so the tolerance has to cover one cycle of
        // truncation at the ends of a one-second window.
        for speed in [10u8, 35, 50, 70, 90, 99] {
            let want = LFO_RATE_HZ[usize::from(speed)];
            let got = measured_hz(LfoPreset { speed, ..LfoPreset::neutral() });
            assert!((got - want).abs() <= 1.0,
                "speed {speed}: table says {want:.3} Hz, measured {got} cycles/s");
        }
        // The full span, so a wrong scale factor anywhere would show up.
        assert!((LFO_RATE_HZ[0] - 0.0625).abs() < 0.001);
        assert!((LFO_RATE_HZ[99] - 49.26).abs() < 0.01);
    }

    /// When the delay ramp first lets anything through, and when it is fully in.
    fn delay_stages(delay: u8) -> (f64, f64) {
        let mut lfo = lfo_with(LfoPreset { delay, ..LfoPreset::neutral() });
        let (mut opened, mut full) = (None, None);
        for i in 0..(8.0 * SR) as usize {
            let d = lfo.next_delay();
            if opened.is_none() && d > 0.0 { opened = Some(i); }
            if full.is_none() && d >= 1.0 { full = Some(i); break; }
        }
        (
            opened.expect("delay never opened") as f64 / SR,
            full.expect("delay never completed") as f64 / SR,
        )
    }

    #[test]
    fn lfo_delay_is_a_two_stage_ramp() {
        // The defining property: the ramp is *exactly zero* for the whole first
        // stage, so this is a delay and not a slow fade-in. Then it opens over
        // the second stage.
        let (opened, full) = delay_stages(99);
        assert!((opened - 2.664).abs() < 0.01, "first stage should last 2.66 s, was {opened:.3}");
        assert!((full - 3.330).abs() < 0.01, "ramp should complete at 3.33 s, was {full:.3}");

        // The second increment clears the low seven bits of the first and floors
        // at 128, which caps the arrival stage at two thirds of a second however
        // long the delay — so a very late vibrato still comes in briskly. It also
        // means the second stage is only the *faster* one for long delays.
        for delay in [55u8, 60, 75, 90, 99] {
            let (opened, full) = delay_stages(delay);
            assert!((full - opened - 0.666).abs() < 0.01,
                "delay {delay}: arrival should take 0.67 s, took {:.3}", full - opened);
        }
        // Total delay times, straight off the patch value.
        for (delay, want) in [(20u8, 0.181), (50, 0.646), (75, 1.554), (99, 3.330)] {
            let (_, full) = delay_stages(delay);
            assert!((full - want).abs() < 0.01, "delay {delay}: want {want:.3} s, got {full:.3}");
        }
    }

    #[test]
    fn lfo_delay_zero_means_no_delay_at_all() {
        // Not "a very short delay" — the accumulator saturates on its first tick.
        let mut lfo = lfo_with(LfoPreset { delay: 0, ..LfoPreset::neutral() });
        assert!(lfo.next_delay() > 0.999);
        assert_eq!(lfo.next_delay(), 1.0);
    }

    #[test]
    fn key_sync_restarts_the_phase_and_the_delay_always_restarts() {
        let mut synced = lfo_with(LfoPreset { sync: true, delay: 60, ..LfoPreset::neutral() });
        let mut free = lfo_with(LfoPreset { sync: false, delay: 60, ..LfoPreset::neutral() });
        for _ in 0..5000 {
            synced.next_value();
            synced.next_delay();
            free.next_value();
            free.next_delay();
        }
        let (synced_before, free_before) = (synced.phase, free.phase);
        assert!(synced.delay_state > 0 && free.delay_state > 0);

        synced.keydown();
        free.keydown();

        assert_eq!(synced.phase, LFO_HALF - 1, "key sync should restart the phase");
        assert_eq!(free.phase, free_before, "without key sync the phase keeps running");
        assert_ne!(synced_before, LFO_HALF - 1, "test would pass by accident");
        // The delay restarts either way — that part is not optional.
        assert_eq!(synced.delay_state, 0);
        assert_eq!(free.delay_state, 0);
    }

    /// One full cycle of a waveform, sampled evenly.
    fn one_cycle(waveform: LfoWave) -> Vec<f64> {
        // Speed 0 is slow enough that a cycle is thousands of samples wide.
        let mut lfo = lfo_with(LfoPreset { speed: 0, waveform, ..LfoPreset::neutral() });
        let period = (4_294_967_296.0 / f64::from(lfo.delta)) as usize;
        (0..period).map(|_| lfo.next_value()).collect()
    }

    #[test]
    fn every_waveform_spans_the_full_range_and_centres() {
        for waveform in [LfoWave::Triangle, LfoWave::SawDown, LfoWave::SawUp,
                         LfoWave::Square, LfoWave::Sine] {
            let cycle = one_cycle(waveform);
            let lo = cycle.iter().copied().fold(f64::MAX, f64::min);
            let hi = cycle.iter().copied().fold(f64::MIN, f64::max);
            let mean = cycle.iter().sum::<f64>() / cycle.len() as f64;
            assert!(lo < 0.001, "{waveform:?} never reaches its trough (min {lo})");
            assert!(hi > 0.999, "{waveform:?} never reaches its peak (max {hi})");
            assert!((mean - 0.5).abs() < 0.01, "{waveform:?} is not centred (mean {mean})");
        }
    }

    #[test]
    fn waveform_shapes_are_the_shapes_they_claim_to_be() {
        // Sawtooths are monotonic apart from the one wrap.
        for (waveform, want_rising) in [(LfoWave::SawUp, true), (LfoWave::SawDown, false)] {
            let cycle = one_cycle(waveform);
            let steps: Vec<f64> = cycle.windows(2).map(|w| w[1] - w[0]).collect();
            let wraps = steps.iter().filter(|s| s.abs() > 0.5).count();
            assert_eq!(wraps, 1, "{waveform:?} should wrap exactly once per cycle");
            let monotonic = steps.iter().filter(|s| s.abs() < 0.5)
                .all(|&s| if want_rising { s >= 0.0 } else { s <= 0.0 });
            assert!(monotonic, "{waveform:?} changes direction mid-cycle");
        }

        // Square only ever takes two values.
        let square = one_cycle(LfoWave::Square);
        assert!(square.iter().all(|&v| v == 0.0 || v == 1.0), "square is not two-valued");

        // Triangle rises for half the cycle and falls for the other half.
        let tri = one_cycle(LfoWave::Triangle);
        let rising = tri.windows(2).filter(|w| w[1] > w[0]).count();
        assert!((rising as f64 / tri.len() as f64 - 0.5).abs() < 0.01,
            "triangle should spend half its cycle rising, spent {rising} of {}", tri.len());
    }

    #[test]
    fn sample_and_hold_steps_once_per_cycle_from_the_hardware_generator() {
        let mut lfo = lfo_with(LfoPreset {
            speed: 0, waveform: LfoWave::SampleHold, ..LfoPreset::neutral()
        });
        let period = (4_294_967_296.0 / f64::from(lfo.delta)) as usize;

        // A step happens on the wrap of the phase accumulator and nowhere else,
        // and each new value is the next term of `x = 179x + 17` on eight bits —
        // the hardware's generator, so the sequence has the same character and
        // not merely the same statistics.
        let mut state = 0u8;
        let (mut prev_phase, mut prev_value) = (lfo.phase, f64::NAN);
        let (mut wraps, mut changes) = (0u32, 0u32);
        for _ in 0..period * 9 / 2 {
            let value = lfo.next_value();
            if lfo.phase < prev_phase {
                wraps += 1;
                state = state.wrapping_mul(179).wrapping_add(17);
                let want = f64::from((u32::from(state ^ 0x80) + 1) << 16) / f64::from(LFO_FULL);
                assert_eq!(value, want, "step {wraps} is not the hardware sequence");
            }
            if !prev_value.is_nan() && value != prev_value { changes += 1; }
            prev_phase = lfo.phase;
            prev_value = value;
        }
        assert!(wraps >= 4, "test window was too short: {wraps} cycles");
        assert_eq!(changes, wraps, "the value changed somewhere other than a cycle boundary");
    }

    // ── LFO depth gating ──

    #[test]
    fn pitch_modulation_needs_both_depth_and_sensitivity() {
        let base = LfoPreset::neutral();
        assert_eq!(LfoPreset { pmd: 0, pitch_mod_sens: 7, ..base }.pitch_mod_depth(), 0.0);
        assert_eq!(LfoPreset { pmd: 99, pitch_mod_sens: 0, ..base }.pitch_mod_depth(), 0.0);
        assert!(LfoPreset { pmd: 99, pitch_mod_sens: 7, ..base }.pitch_mod_depth() > 0.0);

        // Both at maximum is just short of an octave either side — 255 * 255 out
        // of 65536 — which is the hardware's ceiling on vibrato.
        let full = LfoPreset { pmd: 99, pitch_mod_sens: 7, ..base }.pitch_mod_depth();
        assert!((full - 255.0 * 255.0 / 65536.0).abs() < 1e-12);
        assert!(full < 1.0 && full > 0.99);

        // The sensitivity table is not a straight line: step 6 to 7 is worth more
        // than steps 0 to 5 put together.
        let depth = |s| LfoPreset { pmd: 99, pitch_mod_sens: s, ..base }.pitch_mod_depth();
        assert!(depth(7) - depth(6) > depth(5));
    }

    #[test]
    fn amplitude_modulation_needs_a_nonzero_operator_sensitivity() {
        // AMD alone is not enough: the LFO cannot reach an operator whose amp-mod
        // sensitivity is 0, which is what every preset but Vibraphone has.
        let loud = LfoPreset { amd: 99, ..LfoPreset::neutral() };
        assert!(loud.amp_mod_depth() > 0.99);
        assert_eq!(AMP_MOD_SENS[0], 0.0);

        let patch = additive_patch(OpPreset { amp_mod_sens: 0, ..OpPreset::neutral() });
        let patch = PatchPreset { lfo: loud, ..patch };
        let mut deaf = DxVoice::new(SR);
        deaf.note_on(60, 100, &patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
        let mut open = DxVoice::new(SR);
        open.note_on(60, 100, &patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
        // Fully modulated on one, fully unmodulated on the other: identical.
        let trough = LfoFrame { value: 0.0, delay: 1.0 };
        for _ in 0..512 {
            assert_eq!(deaf.tick(trough), open.tick(LfoFrame::default()));
        }
    }

    // ── Modulation reaching the audio output ──

    /// Estimated frequency of a voice's output, from upward zero crossings.
    fn voice_pitch(v: &mut DxVoice, lfo: LfoFrame, samples: usize) -> f64 {
        let mut prev = 0.0f32;
        let (mut first, mut last, mut crossings) = (None, 0usize, 0u32);
        for i in 0..samples {
            let s = v.tick(lfo);
            if prev <= 0.0 && s > 0.0 {
                if first.is_none() { first = Some(i); } else { crossings += 1; }
                last = i;
            }
            prev = s;
        }
        let first = first.expect("voice produced no output");
        f64::from(crossings) * SR / (last - first) as f64
    }

    /// A steady additive patch that holds at full level, so the only thing that
    /// can change its pitch or amplitude is modulation.
    fn steady_patch() -> PatchPreset {
        additive_patch(OpPreset {
            rates: [99, 99, 99, 99], levels: [99, 99, 99, 0], ..OpPreset::neutral()
        })
    }

    #[test]
    fn vibrato_bends_pitch_by_the_dialled_amount() {
        let depth = LfoPreset { pmd: 40, pitch_mod_sens: 5, ..LfoPreset::neutral() };
        let octaves = depth.pitch_mod_depth();
        let patch = PatchPreset { lfo: depth, ..steady_patch() };

        let mut v = DxVoice::new(SR);
        v.note_on(69, 100, &patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
        // Holding the LFO at its peak and at its trough pins the two extremes of
        // the vibrato, which is the cleanest way to measure its depth.
        let sharp = voice_pitch(&mut v, LfoFrame { value: 1.0, delay: 1.0 }, 22050);
        let mut v = DxVoice::new(SR);
        v.note_on(69, 100, &patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
        let flat = voice_pitch(&mut v, LfoFrame { value: 0.0, delay: 1.0 }, 22050);

        assert!(((sharp / 440.0).log2() - octaves).abs() < 0.002,
            "sharp end should be {:.2} Hz, measured {sharp:.2}", 440.0 * f64::exp2(octaves));
        assert!(((flat / 440.0).log2() + octaves).abs() < 0.002,
            "flat end should be {:.2} Hz, measured {flat:.2}", 440.0 * f64::exp2(-octaves));
        assert!(sharp > flat, "vibrato did not move the pitch at all");
        // ...and the delay ramp gates all of it.
        let mut v = DxVoice::new(SR);
        v.note_on(69, 100, &patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
        let held_off = voice_pitch(&mut v, LfoFrame { value: 1.0, delay: 0.0 }, 22050);
        assert!((held_off - 440.0).abs() < 0.5, "delay should hold the vibrato off, got {held_off:.2} Hz");
    }

    #[test]
    fn vibrato_arrives_at_the_lfo_rate_and_only_after_the_delay() {
        // End to end on a factory voice with a delayed vibrato: ROM1A's
        // STRINGS 2, which asks for a slow section wobble that a short bowed
        // note never reaches. Its own LFO settings, its own operators replaced
        // by six steady carriers so nothing but the modulation can move.
        let mut patch = presets()[4];
        assert_eq!(patch.name(), "STRINGS 2");
        assert!(patch.lfo.pmd > 0 && patch.lfo.delay > 0);
        patch.ops = [OpPreset { rates: [99, 99, 99, 99], levels: [99, 99, 99, 0],
                                ..OpPreset::neutral() }; NUM_OPERATORS];
        patch.algorithm = 32;
        patch.feedback = 0;

        let mut lfo = Lfo::new(SR);
        lfo.configure(&patch.lfo);
        lfo.keydown();
        let mut v = DxVoice::new(SR);
        v.note_on(69, 100, &patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);

        // Sample the applied bend directly out of the LFO, in windows.
        let mut spread = |seconds: f64| {
            let (mut lo, mut hi) = (f64::MAX, f64::MIN);
            for _ in 0..(seconds * SR) as usize {
                let frame = lfo.tick();
                v.tick(frame);
                let bend = patch.lfo.pitch_mod_depth() * frame.delay * (frame.value * 2.0 - 1.0);
                lo = lo.min(bend);
                hi = hi.max(bend);
            }
            (hi - lo) * 1200.0
        };
        // The delay is a two-stage ramp — shut, then opening — so nothing at
        // all reaches the pitch before the first stage ends. Both stages are
        // read from the model rather than written down, because they are a
        // property of the voice's delay setting rather than of this test.
        let (shut, open) = delay_stages(patch.lfo.delay);
        assert!(shut > 0.5, "this test needs a voice whose vibrato is held off");
        let early = spread(shut * 0.9);
        assert_eq!(early, 0.0, "vibrato should not have started yet, saw {early:.1} cents");
        spread(open - shut * 0.9 + 0.1); // the arrival ramp
        let late = spread(2.0);
        // Peak to peak is twice the depth the patch dials, in cents.
        let want = 2.0 * patch.lfo.pitch_mod_depth() * 1200.0;
        assert!((late - want).abs() < 0.08 * want,
            "vibrato should settle near {want:.1} cents peak to peak, saw {late:.1}");

        // And it wobbles at the speed the patch asks for.
        let want = LFO_RATE_HZ[usize::from(patch.lfo.speed)];
        let got = measured_hz(patch.lfo);
        assert!((got - want).abs() <= 1.0, "expected ~{want:.2} Hz, measured {got}");
    }

    #[test]
    fn tremolo_ducks_the_operators_that_asked_for_it() {
        let patch = PatchPreset {
            lfo: LfoPreset { amd: 60, ..LfoPreset::neutral() },
            ..additive_patch(OpPreset {
                rates: [99, 99, 99, 99], levels: [99, 99, 99, 0],
                amp_mod_sens: 3, ..OpPreset::neutral()
            })
        };
        let peak_rms = |frame: LfoFrame| {
            let mut v = DxVoice::new(SR);
            v.note_on(60, 100, &patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
            let energy: f64 = (0..8820).map(|_| f64::from(v.tick(frame)).powi(2)).sum();
            (energy / 8820.0).sqrt()
        };
        let loud = peak_rms(LfoFrame { value: 1.0, delay: 1.0 });
        let quiet = peak_rms(LfoFrame { value: 0.0, delay: 1.0 });
        let held_off = peak_rms(LfoFrame { value: 0.0, delay: 0.0 });

        // The LFO's peak is full volume — amplitude modulation only ever ducks.
        assert!((loud / held_off - 1.0).abs() < 1e-9, "the LFO peak should be unmodulated");
        // The trough is down by depth x sensitivity x the full level range, in dB.
        let want_db = SILENCE_DB * patch.lfo.amp_mod_depth() * AMP_MOD_SENS[3];
        let got_db = -20.0 * (quiet / loud).log10();
        assert!((got_db - want_db).abs() < 0.2,
            "trough should sit {want_db:.1} dB down, measured {got_db:.1}");
        assert!(want_db > 20.0, "this test is only meaningful with a deep tremolo");
    }

    // ── Pitch envelope ──

    #[test]
    fn pitch_env_level_50_is_the_neutral_centre() {
        assert_eq!(pitch_env_offset(50), 0.0);
        // Either side of centre the step is one table unit, which is a coarse
        // 37.5 cents — the pitch EG cannot do fine detuning.
        assert!((pitch_env_offset(51) * 1200.0 - 37.5).abs() < 1e-9);
        assert!((pitch_env_offset(49) * 1200.0 + 37.5).abs() < 1e-9);
        // The ends are the documented four octaves.
        assert_eq!(pitch_env_offset(0), -4.0);
        assert!((pitch_env_offset(99) - 127.0 / 32.0).abs() < 1e-12);
        // Monotonic all the way across, with no flat spots.
        for level in 0..99u8 {
            assert!(pitch_env_offset(level + 1) > pitch_env_offset(level), "level {level}");
        }
    }

    #[test]
    fn flat_pitch_env_never_leaves_centre() {
        let mut env = DxPitchEnvelope::new(SR);
        let neutral = PitchEgPreset::neutral();
        env.set(neutral.rates, neutral.levels);
        assert!(env.flat);
        for _ in 0..(3.0 * SR) as usize {
            assert_eq!(env.tick(), 0.0);
        }
        env.keyup();
        for _ in 0..(3.0 * SR) as usize {
            assert_eq!(env.tick(), 0.0);
        }
    }

    #[test]
    fn pitch_env_sweeps_to_each_level_in_turn_and_holds_at_l3() {
        // Start a fifth flat, sweep up to an octave sharp, settle back to centre,
        // and drop a fifth flat again on release.
        let mut env = DxPitchEnvelope::new(SR);
        env.set([80, 80, 80, 80], [82, 50, 50, 43]);
        assert!(!env.flat);
        assert!((env.level - pitch_env_offset(43)).abs() < 1e-12,
            "the envelope should start at L4, not at centre");

        /// Runs the envelope, returning the extremes it touched and where it
        /// finished. Nothing holds at L1 or L2, so a sweep has to be caught by
        /// its extent rather than sampled at a moment.
        fn run(env: &mut DxPitchEnvelope, seconds: f64) -> (f64, f64, f64) {
            let (mut lo, mut hi, mut last) = (f64::MAX, f64::MIN, 0.0);
            for _ in 0..(seconds * SR) as usize {
                last = env.tick();
                lo = lo.min(last);
                hi = hi.max(last);
            }
            (lo, hi, last)
        }

        // L1 is the top of the sweep and the envelope lands exactly on it.
        let (_, peak, sustain) = run(&mut env, 1.0);
        assert!((peak - pitch_env_offset(82)).abs() < 1e-9, "L1 not reached: {peak}");
        assert_eq!(sustain, 0.0, "L3 is level 50, so the sustain should be centre");
        // ...and having got there it stays until the key comes up.
        assert_eq!(run(&mut env, 2.0), (0.0, 0.0, 0.0), "the sustain should hold indefinitely");

        // Release runs to L4 and stops there.
        env.keyup();
        let (bottom, _, released) = run(&mut env, 1.0);
        assert!((released - pitch_env_offset(43)).abs() < 1e-9, "L4 not reached: {released}");
        assert_eq!(bottom, released, "the release should not overshoot L4");
    }

    #[test]
    fn pitch_env_sweep_bends_the_audio_in_the_right_direction() {
        // A held offset first, so the magnitude of the bend can be checked
        // exactly: every level at 43 is seven table steps below centre, which is
        // 7/32 of an octave flat.
        for level in [43u8, 57] {
            let patch = PatchPreset {
                pitch_eg: PitchEgPreset { rates: [99; 4], levels: [level; 4] },
                ..steady_patch()
            };
            let mut v = DxVoice::new(SR);
            v.note_on(69, 100, &patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
            let want = 440.0 * f64::exp2(pitch_env_offset(level));
            let got = voice_pitch(&mut v, LfoFrame::default(), 22050);
            assert!((got - want).abs() < 0.5,
                "level {level} should hold the note at {want:.2} Hz, measured {got:.2}");
        }

        // Then a slow rise from a fifth flat up to centre: the note opens
        // audibly flat and arrives in tune.
        let patch = PatchPreset {
            pitch_eg: PitchEgPreset { rates: [10, 99, 99, 99], levels: [50, 50, 50, 43] },
            ..steady_patch()
        };
        let mut v = DxVoice::new(SR);
        v.note_on(69, 100, &patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
        let start = voice_pitch(&mut v, LfoFrame::default(), 2205);
        let cents = (start / 440.0).log2() * 1200.0;
        assert!(cents < -200.0, "the note should open well flat, measured {cents:.0} cents");
        for _ in 0..(2.0 * SR) as usize { v.tick(LfoFrame::default()); }
        let settled = voice_pitch(&mut v, LfoFrame::default(), 22050);
        assert!((settled - 440.0).abs() < 0.5, "should settle on 440 Hz, measured {settled:.2}");

        // ...and the same envelope reflected about 50 bends the other way.
        let down = PatchPreset {
            pitch_eg: PitchEgPreset { rates: [10, 99, 99, 99], levels: [50, 50, 50, 57] },
            ..steady_patch()
        };
        let mut v = DxVoice::new(SR);
        v.note_on(69, 100, &down, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
        let start = voice_pitch(&mut v, LfoFrame::default(), 2205);
        let cents = (start / 440.0).log2() * 1200.0;
        assert!(cents > 200.0, "level 57 should open well sharp, measured {cents:.0} cents");
    }

    // ── Headroom ──

    /// Peak of one note, at the voice's own level: one voice at the default
    /// knob settings, times the default gain, but *not* times the headroom trim
    /// `process` applies. Driving the voice directly rather than the synth is
    /// what keeps a sweep over the factory set affordable — `process` would
    /// tick fifteen silent voices for every sounding one.
    fn note_peak(preset: &PatchPreset, note: u8, vel: u8, secs: f64) -> f32 {
        let defaults = Dx7Synth::new();
        let mut voice = DxVoice::new(44100.0);
        voice.note_on(note, vel, preset, defaults.brightness(), defaults.attack_scale(),
            defaults.decay_scale(), defaults.sustain_scale(), defaults.release_scale(), 1);
        let mut lfo = Lfo::new(44100.0);
        lfo.configure(&preset.lfo);
        lfo.keydown();
        let mut peak = 0.0f32;
        for _ in 0..(secs * 44100.0) as usize {
            peak = peak.max((voice.tick(lfo.tick()) * PARAM_DEFAULTS[P_GAIN]).abs());
        }
        peak
    }

    /// The most a patch could possibly reach on one note: every carrier at its
    /// loudest envelope level with their sines momentarily aligned.
    ///
    /// A true upper bound — an operator's output is `sin(...) * env * gain`, the
    /// envelope clamps at its target rather than overshooting it, amplitude
    /// modulation only ever attenuates, and the sustain knob only ever adds
    /// attenuation. It is loose for a patch whose carriers sit at different
    /// ratios, because those never actually line up.
    fn peak_bound(preset: &PatchPreset, note: u8, vel: u8) -> f32 {
        let alg = algorithm(preset.algorithm);
        let sum: f64 = alg.carriers.iter().map(|&c| {
            let op = &preset.ops[c];
            let loudest = op.levels[0].max(op.levels[1]).max(op.levels[2]);
            operator_gain(op, note, vel) * atten_to_gain(dx_level_to_atten_db(loudest))
        }).sum();
        (sum / alg.carriers.len() as f64) as f32 * PARAM_DEFAULTS[P_GAIN]
    }

    #[test]
    fn no_factory_voice_clips_on_one_note() {
        // One note, measured at the instrument's output: the analytic bound on
        // the voice, times the headroom trim `process` applies. The bound holds
        // at every instant of the note rather than at the instants a render
        // happened to sample, which is what makes a 256-voice sweep over the
        // whole playable keyboard affordable.
        //
        // A single note is no longer what sizes the trim — an eight-note chord
        // is, and by a wide margin — so this is the check that the wide margin
        // is really there. The loudest single note in the factory set is
        // ROM1B's HARP 1 at the bottom of the keyboard.
        let mut loudest = (0.0f32, "", 0u8);
        for preset in presets().iter() {
            for note in 36..=96u8 {
                let peak = peak_bound(preset, note, 127) * OUTPUT_TRIM;
                assert!(peak < 1.0,
                    "{} can clip at note {note}, velocity 127: bound {peak:.4}", preset.name());
                if peak > loudest.0 { loudest = (peak, preset.name(), note); }
            }
        }
        let headroom_db = -20.0 * loudest.0.log10();
        assert!((6.0..30.0).contains(&headroom_db),
            "the loudest single note in the bank ({loudest:?}) leaves {headroom_db:.1} dB \
             of headroom, which is either no margin at all or a trim that has collapsed");

        // The bound is only worth sweeping with if it really is an upper bound,
        // so the voice it picks out gets rendered and checked against it. Loose
        // is fine — carriers at different ratios never do line up — but under
        // is not.
        let hottest = presets().iter().find(|p| p.name() == loudest.1).expect("a named voice");
        let rendered = note_peak(hottest, loudest.2, 127, 0.4) * OUTPUT_TRIM;
        assert!(rendered <= loudest.0,
            "{} renders at {rendered:.4}, above its own bound of {:.4}", loudest.1, loudest.0);
    }

    // ── Transpose and oscillator key sync ──

    /// A voice built from `preset`, keyed on `note`, with everything else at
    /// the neutral settings that make a comparison exact.
    fn keyed(preset: &PatchPreset, note: u8) -> DxVoice {
        let mut v = DxVoice::new(SR);
        v.note_on(note, 100, preset, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
        v
    }

    #[test]
    fn transpose_shifts_the_note_the_whole_voice_is_built_from() {
        // Not just the pitch: the DX7 transposes the keyboard, so the level
        // scaling and the rate scaling see the shifted note too. The test for
        // that is exact equality between a transposed voice and an untransposed
        // one played at the shifted note — if any stage read the original note
        // the two renders would diverge.
        let scaled = OpPreset {
            rates: [80, 60, 40, 40], levels: [99, 80, 60, 0],
            break_point: 43, right_depth: 60, left_depth: 40,
            right_curve: ScaleCurve::ExpNeg, rate_scaling: 5, vel_sens: 4,
            ..OpPreset::neutral()
        };
        let base = additive_patch(scaled);

        for (transpose, semitones) in [(0u8, -24i32), (12, -12), (21, -3), (36, 12), (48, 24)] {
            let moved = PatchPreset { transpose, ..base };
            let mut shifted = keyed(&moved, 60);
            let mut plain = keyed(&base, (60 + semitones) as u8);
            for i in 0..2048 {
                assert_eq!(shifted.tick(LfoFrame::default()), plain.tick(LfoFrame::default()),
                    "transpose {transpose} diverges from note {} at sample {i}", 60 + semitones);
            }
        }

        // The centre really is inert.
        let centred = PatchPreset { transpose: 24, ..base };
        assert_eq!(keyed(&centred, 60).ops[0].freq, keyed(&base, 60).ops[0].freq);
    }

    #[test]
    fn transpose_cannot_run_off_either_end_of_the_keyboard() {
        // Two octaves down from MIDI 5, or two up from MIDI 120, is not a note.
        // Nothing here may wrap, panic or ask for a negative frequency.
        let low = PatchPreset { transpose: 0, ..additive_patch(OpPreset::neutral()) };
        let high = PatchPreset { transpose: 48, ..additive_patch(OpPreset::neutral()) };
        for note in 0..=127u8 {
            for preset in [&low, &high] {
                let v = keyed(preset, note);
                let freq = v.ops[0].freq;
                assert!(freq.is_finite() && freq > 0.0, "note {note} produced {freq} Hz");
                assert!(freq <= note_to_freq(127) * 1.001, "note {note} ran off the top");
            }
        }
        assert_eq!(keyed(&low, 5).ops[0].freq, note_to_freq(0), "clamped at the bottom");
        assert_eq!(keyed(&high, 120).ops[0].freq, note_to_freq(127), "clamped at the top");
    }

    #[test]
    fn a_transposed_voice_still_answers_to_the_key_that_was_pressed() {
        // The transposed note is what the voice is built from; the note the
        // player pressed is what a note-off names. Getting that backwards
        // leaves the key stuck down.
        let bass = presets().iter().position(|p| p.transpose == 12).expect("a transposed voice");
        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        select(&mut s, bass);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        assert!(s.voices.iter().any(|v| v.is_held()), "the note should be held");
        process_buffers(&mut s, &[note_off(60, 0)], 1);
        assert!(!s.voices.iter().any(|v| v.is_held()), "note-off did not reach the voice");
    }

    #[test]
    fn key_sync_decides_whether_a_note_starts_from_the_same_phase() {
        let patch = additive_patch(OpPreset {
            rates: [99, 99, 99, 99], levels: [99, 99, 99, 0], ..OpPreset::neutral()
        });
        let synced = PatchPreset { osc_key_sync: true, ..patch };
        let free = PatchPreset { osc_key_sync: false, ..patch };

        // Run a voice for an odd number of samples, then key it again. With
        // sync on the second note is the first note over again, sample for
        // sample; with it off the operators carry on from where they were.
        //
        // Both windows start 512 samples after their key-down, past the attack:
        // an envelope retriggers from wherever it currently sits, so comparing
        // from the key-down itself would be comparing envelopes rather than
        // phases. By 512 samples both are held at full level and the phase is
        // the only thing left that can differ.
        let restart = |preset: &PatchPreset| {
            let mut v = keyed(preset, 60);
            let window = |v: &mut DxVoice| {
                for _ in 0..512 { v.tick(LfoFrame::default()); }
                (0..256).map(|_| v.tick(LfoFrame::default())).collect::<Vec<f32>>()
            };
            let first = window(&mut v);
            for _ in 0..1017 { v.tick(LfoFrame::default()); }
            v.note_on(60, 100, preset, 1.0, 1.0, 1.0, 1.0, 1.0, 2);
            let second = window(&mut v);
            (first, second)
        };

        let (first, second) = restart(&synced);
        assert_eq!(first, second, "key sync on: every note must start from phase zero");

        let (first, second) = restart(&free);
        assert_ne!(first, second, "key sync off: the operators must keep running");
        // ...and it is the phase that moved, not the level: both renders swing
        // just as wide, they are simply not the same waveform.
        let swing = |xs: &[f32]| xs.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        let (a, b) = (swing(&first), swing(&second));
        assert!((a - b).abs() < 0.05 * a, "free-running note changed level: {a} -> {b}");
    }

    #[test]
    fn free_running_voices_decorrelate_a_repeated_chord() {
        // What key sync is worth at the output. Eight notes keyed on the same
        // sample sum coherently when every operator restarts from phase zero —
        // roughly 8x a single note rather than the sqrt(8) uncorrelated voices
        // would give. A free-running voice only starts coherently the first
        // time; after that its operators are wherever the last note left them.
        //
        // ROM2A's PIANO 4 is one of the 74 voices that asks for this.
        let voice = 32;
        assert!(!presets()[voice].osc_key_sync, "this test needs a free-running voice");

        let chord = |warm: bool| {
            let mut s = Dx7Synth::new();
            s.init(44100.0, 64);
            select(&mut s, voice);
            if warm {
                // A different chord, held and released, which leaves the
                // operators scattered.
                let down: Vec<MidiEvent> = [38u8, 45, 50, 57, 62, 66, 69, 74]
                    .iter().map(|&n| note_on(n, 100, 0)).collect();
                process_buffers(&mut s, &down, 400);
                let up: Vec<MidiEvent> = [38u8, 45, 50, 57, 62, 66, 69, 74]
                    .iter().map(|&n| note_off(n, 0)).collect();
                process_buffers(&mut s, &up, 400);
            }
            let down: Vec<MidiEvent> = [36u8, 43, 48, 55, 60, 64, 67, 72]
                .iter().map(|&n| note_on(n, 127, 0)).collect();
            process_buffers(&mut s, &down, 256).iter()
                .fold(0.0f32, |a, x| a.max(x.abs()))
        };

        let (fresh, warm) = (chord(false), chord(true));
        assert!(warm < fresh * 0.9,
            "the second chord should sum less coherently: {fresh:.4} -> {warm:.4}");
    }

    // ── The feedback trim ──

    #[test]
    fn centred_knob_is_the_patch_as_authored() {
        // The whole point of the trim: at the default the player hears the index
        // the patch was written with, for every patch in the bank.
        for patch in presets().iter() {
            let got = resolve_feedback(patch.feedback, PARAM_DEFAULTS[P_FEEDBACK]);
            assert_eq!(got, patch.feedback,
                "{}: centred knob changed feedback {} -> {got}", patch.name(), patch.feedback);
        }
        assert_eq!(PARAM_DEFAULTS[P_FEEDBACK], 0.5, "the trim must default to centred");
    }

    #[test]
    fn feedback_trim_offsets_and_clamps() {
        // Full travel is the whole 0-7 range either way, so every patch can reach
        // both ends — and neither end can run off it.
        for authored in 0..=7u8 {
            assert_eq!(resolve_feedback(authored, 0.0), 0, "authored {authored} at hard left");
            assert_eq!(resolve_feedback(authored, 1.0), 7, "authored {authored} at hard right");
            assert_eq!(resolve_feedback(authored, 0.5), authored);
        }
        // One step is 1/14 of the travel, and the mapping is monotonic across it.
        let step = 1.0 / 14.0;
        assert_eq!(resolve_feedback(4, 0.5 + step), 5);
        assert_eq!(resolve_feedback(4, 0.5 - step), 3);
        assert_eq!(resolve_feedback(4, 0.5 + 3.0 * step), 7);
        let mut prev = resolve_feedback(4, 0.0);
        for i in 1..=100 {
            let got = resolve_feedback(4, i as f32 / 100.0);
            assert!(got >= prev, "trim is not monotonic at {i}");
            prev = got;
        }
        // `params` is public, so a knob can arrive out of range or not a number
        // at all. Nothing here may panic or wrap: the float-to-int cast
        // saturates, and NaN casts to zero, which is the patch's own index.
        assert_eq!(resolve_feedback(4, f32::NAN), 4);
        assert_eq!(resolve_feedback(4, f32::INFINITY), 7);
        assert_eq!(resolve_feedback(4, f32::NEG_INFINITY), 0);
        assert_eq!(resolve_feedback(4, -1e30), 0);
    }

    #[test]
    fn feedback_depth_is_an_exponential_scale_with_a_hard_off() {
        assert_eq!(feedback_depth(0), 0.0, "index 0 is off, not merely quiet");
        for index in 1..7u8 {
            let ratio = feedback_depth(index + 1) / feedback_depth(index);
            assert!((ratio - 2.0).abs() < 1e-12, "index {index}: step is {ratio}, not a doubling");
        }
        // Depth is in cycles now, matching the modulation input `Operator::tick`
        // takes. Index 7 is half a cycle — the hardware's `>> 1` on the averaged
        // loop sample — which is the same π radians this asserted when the unit
        // was radians and the modulation path was 2π too shallow.
        assert!((feedback_depth(7) - 0.5).abs() < 1e-12);
        assert!((feedback_depth(7) * TWO_PI - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn the_knob_no_longer_discards_the_patch() {
        // The regression this replaces: `process` used to overwrite the voice's
        // feedback with an absolute reading of the knob, so a patch authored at 7
        // ran at index 4 and its real setting was unreachable at any knob
        // position. Render the same note at every knob position and require the
        // centred one to match a voice built straight from the preset.
        // ROM1B's HARP 1, which the cartridge authors wrote at feedback 7 —
        // the far end of the range from where the knob sits — and which asks
        // for no modulation of any kind, so the reference voice below can be
        // ticked with a dead LFO and still be the same render.
        let brass = PATCH_COUNT + 28;
        let preset = presets()[brass];
        assert_eq!(preset.name(), "HARP    1");
        assert_eq!(preset.feedback, 7, "this test needs a voice authored away from centre");
        assert_eq!(preset.lfo.pitch_mod_depth(), 0.0, "and one the LFO cannot reach");
        assert_eq!(preset.lfo.amp_mod_depth(), 0.0);

        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        select(&mut s, brass);
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 4);

        let mut voice = DxVoice::new(44100.0);
        voice.note_on(60, 100, &preset, s.brightness(), s.attack_scale(),
            s.decay_scale(), s.sustain_scale(), s.release_scale(), 1);
        assert_eq!(voice.feedback_amount, feedback_depth(7));
        // Sample for sample, not close to: the output stage is exactly the
        // voice times the gain knob times the headroom trim, with the
        // saturator sitting below its knee and therefore doing nothing.
        // Knob and trim are folded into one factor before the loop, so they
        // are bracketed the same way here — float multiplication does not
        // associate, and this comparison is exact.
        let out_gain = PARAM_DEFAULTS[P_GAIN] * OUTPUT_TRIM;
        let want: Vec<f32> = (0..out.len())
            .map(|_| voice.tick(LfoFrame::default()) * out_gain)
            .collect();
        assert_eq!(out, want, "the centred knob must render the patch as authored");

        // ...and the knob still does something in both directions.
        let energy = |knob: f32| {
            let mut s = Dx7Synth::new();
            s.init(44100.0, 64);
            select(&mut s, brass);
            s.set_parameter(P_FEEDBACK, knob);
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 4);
            out.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>()
        };
        assert!(energy(0.0) != energy(0.5), "hard left should not sound like centre");
        let centre: f64 = out.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
        assert!((energy(0.5) - centre).abs() < 1e-12);
    }

    #[test]
    fn brightness_is_a_trim_centred_on_the_patch() {
        // The knob that used to sit at 1.1 by default. With the modulation index
        // right, the centre has to be the modulator levels the patch was voiced
        // with — exactly 1.0, not "nearly".
        let mut s = Dx7Synth::new();
        assert_eq!(PARAM_DEFAULTS[P_BRIGHTNESS], 0.5, "the trim must default to centred");
        assert!((s.brightness() - 1.0).abs() < 1e-12,
            "the centred knob is {} of the authored level", s.brightness());

        // 18 dB of cut, 6 dB of boost, exponential across each half.
        s.set_parameter(P_BRIGHTNESS, 0.0);
        let cut = 20.0 * s.brightness().log10();
        assert!((cut + 18.0).abs() < 1e-9, "hard left is {cut:.3} dB, want -18");
        s.set_parameter(P_BRIGHTNESS, 1.0);
        let boost = 20.0 * s.brightness().log10();
        assert!((boost - 6.0).abs() < 1e-9, "hard right is {boost:.3} dB, want +6");
        s.set_parameter(P_BRIGHTNESS, 0.25);
        assert!((20.0 * s.brightness().log10() + 9.0).abs() < 1e-9, "half left is not -9 dB");
        s.set_parameter(P_BRIGHTNESS, 0.75);
        assert!((20.0 * s.brightness().log10() - 3.0).abs() < 1e-9, "half right is not +3 dB");

        // Monotonic across the whole travel, and never zero — hard left is dull,
        // not an operator that has been switched off.
        let mut prev = 0.0;
        for i in 0..=100 {
            s.set_parameter(P_BRIGHTNESS, i as f32 / 100.0);
            let got = s.brightness();
            assert!(got > prev, "trim is not monotonic at {i}");
            prev = got;
        }

        // `params` is public, so anything can land in it. Out of range clamps and
        // NaN falls back to the centre, i.e. the patch as authored.
        s.params[P_BRIGHTNESS] = f32::NAN;
        assert!((s.brightness() - 1.0).abs() < 1e-12, "a NaN knob must be the patch itself");
        s.params[P_BRIGHTNESS] = f32::INFINITY;
        assert!((20.0 * s.brightness().log10() - 6.0).abs() < 1e-9);
        s.params[P_BRIGHTNESS] = -1e30;
        assert!((20.0 * s.brightness().log10() + 18.0).abs() < 1e-9);
    }

    // ── The FM modulation index ──

    /// An operator parked at DC with a flat, fully open envelope and unity gain.
    /// Its phase never advances, so whatever it returns is `sin` of the phase
    /// deviation its modulation input asked for and nothing else.
    fn parked_operator() -> Operator {
        let mut op = Operator::new(SR);
        op.freq = 0.0;
        op.gain = 1.0;
        op.envelope.set_from_preset([99, 99, 99, 99], [99, 99, 99, 0]);
        op.envelope.trigger();
        for _ in 0..4096 {
            op.tick(0.0, SR, 1.0, 1.0);
        }
        assert!(op.envelope.atten_db.abs() < 1e-12,
            "the envelope has not opened: {} dB", op.envelope.atten_db);
        assert_eq!(op.phase, 0.0, "a DC operator must not accumulate phase");
        op
    }

    #[test]
    fn a_full_scale_modulator_bends_a_whole_cycle() {
        // The unit the modulation input is carried in. On the hardware one full
        // cycle of phase and full sine amplitude are the same 24-bit quantity,
        // and the OPS adds a modulator's output into the carrier's phase
        // accumulator with no rescaling, so an operator at full output swings
        // the operator it feeds through a whole cycle — 2π radians.
        //
        // Reading the sum as radians instead put every FM patch in the bank at
        // an index of 1 where the machine runs 2π, which is why they all
        // rendered as very nearly pure sines. A quarter-cycle of modulation is
        // the sharpest test of the two readings: 2π * 0.25 peaks the sine at
        // 1.0, while the radians reading returns sin(0.25) = 0.247.
        let mut op = parked_operator();
        for (cycles, want) in [
            (0.0, 0.0),
            (0.125, std::f64::consts::FRAC_1_SQRT_2),
            (0.25, 1.0),
            (0.5, 0.0),
            (0.75, -1.0),
            (1.0, 0.0),
        ] {
            let got = op.tick(cycles, SR, 1.0, 1.0);
            assert!((got - want).abs() < 1e-9,
                "{cycles} cycles of modulation gave {got}, want sin({} rad) = {want}",
                cycles * TWO_PI);
        }
        // ...and the deviation really is 2π radians per unit of input, read back
        // off a sample that is still on the rising quarter of the sine.
        let radians = parked_operator().tick(0.125, SR, 1.0, 1.0).asin();
        assert!((radians - 0.125 * TWO_PI).abs() < 1e-9,
            "one eighth of a cycle came out as {radians} rad");
    }

    #[test]
    fn feedback_at_index_seven_is_still_pi() {
        // Feedback was already right, because it was written in radians against
        // a modulation path that was 2π too shallow and the two errors cancelled.
        // Rescaling the path without rescaling this would have made feedback 2π
        // too strong, so pin it: the hardware shifts the averaged loop sample
        // right by `8 - index`, index 7 is therefore a `>> 1` — half of full
        // scale, half a cycle, π radians.
        let mut src = parked_operator();
        src.prev = [1.0, 1.0]; // the loop pinned at full output
        let cycles = src.feedback(feedback_depth(7));
        assert!((cycles - 0.5).abs() < 1e-12, "index 7 taps {cycles} cycles, want 0.5");
        assert!((cycles * TWO_PI - std::f64::consts::PI).abs() < 1e-12,
            "index 7 taps {} rad, want π", cycles * TWO_PI);

        // And that is what lands on the phase: π reads sin(π) = 0, half of it
        // reads sin(π/2) = 1.
        let mut dst = parked_operator();
        assert!(dst.tick(cycles, SR, 1.0, 1.0).abs() < 1e-9);
        assert!((dst.tick(cycles * 0.5, SR, 1.0, 1.0) - 1.0).abs() < 1e-9);

        // Every index below it is a halving of the same quantity.
        for index in 1..=FEEDBACK_MAX {
            let want = f64::exp2(f64::from(index) - 8.0);
            assert!((feedback_depth(index) - want).abs() < 1e-15,
                "index {index}: {} cycles, want {want}", feedback_depth(index));
        }
    }

    /// `J_order(x)`, from the integral `(1/π) ∫₀^π cos(nθ - x sin θ) dθ`. The
    /// integrand is smooth and periodic, so the midpoint rule converges faster
    /// than any power of the step and 4096 points is exact to double precision
    /// at the arguments used here.
    fn bessel_j(order: u32, x: f64) -> f64 {
        const N: usize = 4096;
        let mut sum = 0.0;
        for i in 0..N {
            let theta = std::f64::consts::PI * (i as f64 + 0.5) / N as f64;
            sum += (f64::from(order) * theta - x * theta.sin()).cos();
        }
        sum / N as f64
    }

    /// Peak amplitude of the component at `freq`, by direct correlation. Exact
    /// only when the window holds a whole number of cycles of `freq`, which is
    /// what the sample rate below is chosen to guarantee.
    fn partial_amplitude(x: &[f64], freq: f64, sr: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (n, v) in x.iter().enumerate() {
            let w = TWO_PI * freq * n as f64 / sr;
            re += v * w.cos();
            im += v * w.sin();
        }
        2.0 * re.hypot(im) / x.len() as f64
    }

    #[test]
    fn fm_index_follows_the_bessel_spectrum() {
        // The end-to-end statement of the same fact, in the shape the textbook
        // gives it: a carrier phase-modulated by one sine at index β keeps a
        // fundamental of J₀(β) and grows sidebands of Jₙ(β). One full-scale
        // modulator is β = 2π, which leaves the fundamental at J₀(2π) = 0.220 —
        // under a fifth of the unmodulated carrier, with most of the energy now
        // in the sidebands. The radians reading gave β = 1 and J₀(1) = 0.765.
        //
        // The modulator sits at ratio 4 so that nothing lands back on the
        // fundamental: the partials are at (1 + 4n)·f0, and the negative-order
        // ones fold onto 3·f0, 7·f0 and up.
        let f0 = note_to_freq(60);
        // 128 samples per cycle of f0 puts f0 and every harmonic of it exactly
        // on the analysis grid, so the correlation above has no leakage to
        // correct for.
        let sr = f0 * 128.0;

        let mut patch = PatchPreset { algorithm: 5, feedback: 0, ..PatchPreset::neutral() };
        patch.ops[1].coarse = 4;
        // Alg 5 is three 2-op stacks; mute the other two outright.
        for op in &mut patch.ops[2..] {
            op.output_level = 0;
        }

        let render = |patch: &PatchPreset| {
            let mut v = DxVoice::new(sr);
            v.note_on(60, 100, patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
            for _ in 0..8192 {
                v.tick(LfoFrame::default());
            }
            (0..8192).map(|_| f64::from(v.tick(LfoFrame::default()))).collect::<Vec<f64>>()
        };

        let modulated = render(&patch);
        let mut clean = patch;
        clean.ops[1].output_level = 0;
        let reference = partial_amplitude(&render(&clean), f0, sr);
        assert!(reference > 0.3, "the unmodulated carrier is missing: {reference}");

        for order in 0..=3u32 {
            let freq = f0 * (1.0 + 4.0 * f64::from(order));
            let got = partial_amplitude(&modulated, freq, sr) / reference;
            let want = bessel_j(order, TWO_PI).abs();
            assert!((got - want).abs() < 0.01,
                "partial at {freq:.1} Hz is {got:.4} of the unmodulated carrier, \
                 want |J{order}(2π)| = {want:.4}");
        }
    }

    // ── The modulation audit ──

    #[test]
    fn the_modulation_the_cartridges_ask_for_reaches_the_engine() {
        // The hand-authored bank this replaced was inert apart from six patches,
        // and the test here pinned that list by name. The factory set is the
        // other way round: 109 voices use the LFO and 37 bend pitch, so what
        // matters is that every one of those paths is actually wired through —
        // a depth that never reaches an operator is the failure mode, and it is
        // silent.
        let bank = presets();
        // Vibrato needs a depth *and* a sensitivity; either at zero and the LFO
        // cannot reach the pitch at all.
        let vibrato = bank.iter().filter(|p| p.lfo.pitch_mod_depth() > 0.0).count();
        assert_eq!(vibrato, 98, "voices whose LFO reaches pitch");

        // Amplitude modulation needs both halves too: a depth on the patch and
        // a sensitivity on an operator. The cartridges set one without the
        // other on 44 voices, which is authentic and silent — what has to be
        // right is the 22 where both are set, because those are the ones the
        // per-sample gain table is built for.
        let tremolo = bank.iter().filter(|p| {
            p.lfo.amp_mod_depth() > 0.0 && p.ops.iter().any(|op| op.amp_mod_sens > 0)
        }).count();
        assert_eq!(tremolo, 22, "voices where amplitude modulation is audible");
        assert_eq!(bank.iter().filter(|p| p.lfo.amp_mod_depth() > 0.0).count(), 48,
            "voices with an amplitude depth set at all");

        // Pitch envelopes: 37 voices leave the neutral 50, and every one of them
        // produces a real offset rather than a rounding artefact.
        for patch in bank.iter().filter(|p| p.pitch_eg.levels != [50; 4]) {
            let extreme = patch.pitch_eg.levels.iter()
                .map(|&l| pitch_env_offset(l).abs())
                .fold(0.0f64, f64::max);
            assert!(extreme > 0.0, "{}: pitch EG is set but bends nothing", patch.name());
        }
    }

    #[test]
    fn an_unmodulated_patch_is_untouched_by_a_running_lfo() {
        // The contract that keeps the 147 unmodulated voices bit-for-bit
        // identical to what they were before the LFO existed: with
        // both depths at zero, no LFO output can reach the audio, however hard the
        // oscillator is swinging.
        let patch = steady_patch();
        assert_eq!(patch.lfo.pitch_mod_depth(), 0.0);
        assert_eq!(patch.lfo.amp_mod_depth(), 0.0);

        let mut modulated = DxVoice::new(SR);
        modulated.note_on(60, 100, &patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);
        let mut still = DxVoice::new(SR);
        still.note_on(60, 100, &patch, 1.0, 1.0, 1.0, 1.0, 1.0, 1);

        let mut lfo = Lfo::new(SR);
        lfo.configure(&LfoPreset { speed: 60, delay: 0, ..LfoPreset::neutral() });
        for i in 0..(2.0 * SR) as usize {
            let frame = lfo.tick();
            assert_eq!(modulated.tick(frame), still.tick(LfoFrame::default()), "sample {i}");
        }
    }

    // ── The decay-shape audit ──

    /// How one carrier's amplitude envelope behaves at the knob settings the
    /// instrument loads with: the dB it has lost 50 ms after key-down, and the
    /// time it takes to fall 60 dB below its own peak with the key held.
    ///
    /// The knobs are in because they are what the player hears — the default
    /// decay knob is 0.5, which runs both decay stages eight times faster than
    /// the patch authored them, so a 50 ms window here is a 400 ms window in the
    /// preset data. That makes this the stricter of the two readings: a patch
    /// that passes at the knobs passes on the raw preset values as well, since
    /// a decay stage only ever loses more level over a longer window.
    ///
    /// Returns `f64::INFINITY` for the ring time of an envelope that is still
    /// above -60 dB when the render runs out, which is what a patch holding at a
    /// nonzero L3 does.
    fn carrier_decay(op: &OpPreset, note: u8, secs: f64) -> (f64, f64) {
        let defaults = Dx7Synth::new();
        let mut env = DxEnvelope::new(SR);
        env.set_from_preset_scaled(op.rates, op.levels, scale_rate(note, op.rate_scaling));
        env.scale_sustain(defaults.sustain_scale());
        env.scale_times(
            defaults.attack_scale(),
            defaults.decay_scale(),
            defaults.release_scale(),
        );
        env.trigger();
        // Attenuation in dB below full scale, so the arithmetic is subtraction
        // rather than a log of a ratio.
        let trace: Vec<f64> = (0..(secs * SR) as usize).map(|_| { env.tick(); env.atten_db }).collect();
        let peak = trace.iter().copied().fold(f64::INFINITY, f64::min);
        let lost = trace[(0.05 * SR) as usize] - peak;
        let ring = trace.iter().position(|&a| a >= peak + 60.0)
            .map_or(f64::INFINITY, |i| i as f64 / SR);
        (lost, ring)
    }

    /// Worst case over a patch's carriers: the largest 50 ms loss and the
    /// longest ring, which together are the shape of the note.
    fn patch_decay(preset: &PatchPreset, note: u8, secs: f64) -> (f64, f64) {
        algorithm(preset.algorithm).carriers.iter().fold((0.0f64, 0.0f64), |acc, &c| {
            let (lost, ring) = carrier_decay(&preset.ops[c], note, secs);
            (acc.0.max(lost), acc.1.max(ring))
        })
    }

    /// A factory voice by name, so the decay audit reads as the voices a
    /// player would name rather than as indices into a 256-entry bank.
    fn voice_by_name(name: &str) -> &'static PatchPreset {
        let index = (0..VOICE_COUNT).find(|&v| voice_name(v) == name)
            .unwrap_or_else(|| panic!("no factory voice called {name}"));
        &presets()[index]
    }

    #[test]
    fn plucked_factory_voices_decay_as_one_smooth_curve() {
        // A plucked or struck string is a single exponential: it loses a few dB
        // settling off the pluck and then rings down at one rate. What it must
        // not do is collapse 25-30 dB in the first few milliseconds and trickle
        // out afterwards, which is a click with a tail behind it — the failure
        // the hand-authored bank had and the reason this audit exists.
        //
        // The windows are measured from the cartridges themselves, at the knob
        // settings the instrument loads with. They are here to catch the
        // envelope engine drifting, not to grade Yamaha's voicing.
        const PLUCKED: [(&str, f64, f64); 4] = [
            // name,        ring at least, ring at most
            ("CLAV    1",   0.4, 0.8),
            ("SITAR",       0.7, 1.2),
            ("HARPSICH 1",  0.5, 0.9),
            ("GUITAR  1",   0.8, 1.3),
        ];
        for (name, ring_min, ring_max) in PLUCKED {
            let (lost, ring) = patch_decay(voice_by_name(name), 60, 5.0);
            assert!(lost <= 12.0,
                "{name} loses {lost:.1} dB in the first 50 ms — that is a click, not a pluck");
            assert!(ring >= ring_min && ring <= ring_max,
                "{name} rings for {ring:.2} s, want {ring_min}-{ring_max} s");
        }
    }

    #[test]
    fn struck_factory_voices_are_still_allowed_to_be_abrupt() {
        // The other side of the same coin. A xylophone bar, a marimba and a
        // pizzicato string are struck and damped, and their carriers are meant
        // to fall off a cliff — this is here so that the ceiling above never
        // gets applied to them by someone reading it as a rule for the bank.
        for name in ["XYLOPHONE", "MARIMBA", "PIZZ STGS", "KOTO", "BANJO"] {
            let (lost, _) = patch_decay(voice_by_name(name), 60, 1.0);
            assert!(lost >= 20.0,
                "{name} only loses {lost:.1} dB in the first 50 ms — it is supposed to be percussive");
        }
    }

    /// Magnitude of one DFT bin, by Goertzel. One multiply and two adds per
    /// sample per bin, which is what makes a whole spectrum affordable here
    /// without pulling a transform crate into the dependency list.
    fn goertzel(windowed: &[f64], bin: usize) -> f64 {
        let w = std::f64::consts::TAU * bin as f64 / windowed.len() as f64;
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in windowed {
            let s = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0).sqrt()
    }

    /// Spectral centroid of one 23 ms window of a rendered note, as a multiple
    /// of the note's own fundamental.
    ///
    /// Magnitude-weighted rather than power-weighted on purpose: the sidebands a
    /// surviving modulator puts into the ring are 20-30 dB down, and squaring
    /// the weights buries exactly the thing this is here to see. A pure sine
    /// reads 1.0 and nothing can read lower, so 1.0 means the modulators feeding
    /// this window may as well not be there.
    fn ring_brightness(samples: &[f32], f0: f64) -> f64 {
        let n = samples.len();
        let windowed: Vec<f64> = samples.iter().enumerate()
            .map(|(i, &x)| {
                let hann = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
                f64::from(x) * hann
            })
            .collect();
        let (mut num, mut den) = (0.0f64, 0.0f64);
        for bin in 1..n / 2 {
            let mag = goertzel(&windowed, bin);
            num += mag * (bin as f64 * SR / n as f64);
            den += mag;
        }
        if den <= 0.0 { return 0.0; }
        num / den / f0
    }

    /// One note held down, rendered through the whole instrument at the knob
    /// settings it loads with, and read at three points across the ring.
    fn brightness_through_the_ring(voice: usize, note: u8) -> [f64; 3] {
        const WINDOW: usize = 1024;
        let mut synth = Dx7Synth::new();
        synth.init(SR, 256);
        select(&mut synth, voice);
        let events = [note_on(note, 100, 0)];
        let mut block = vec![0.0f32; 256];
        let mut audio = Vec::new();
        for b in 0..200 {
            block.fill(0.0);
            let mut outs: [&mut [f32]; 1] = [&mut block];
            if b == 0 {
                synth.process(&[], &mut outs, &events);
            } else {
                synth.process(&[], &mut outs, &[]);
            }
            audio.extend_from_slice(&block);
        }
        let f0 = 440.0 * f64::exp2((f64::from(note) - 69.0) / 12.0);
        let mut out = [0.0; 3];
        for (slot, ms) in [50.0, 300.0, 800.0].iter().enumerate() {
            let start = (ms / 1000.0 * SR) as usize;
            out[slot] = ring_brightness(&audio[start..start + WINDOW], f0);
        }
        out
    }

    #[test]
    fn plucked_modulators_do_not_leave_the_ring_a_bare_sine() {
        // The other half of the plucked-patch audit. Carriers that ring for a
        // second achieve nothing on their own if the modulators run to L3 0 and
        // get there in the first few milliseconds: the ring the carriers have
        // just been handed is a bare sine, which measures 1.00x its own
        // fundamental at every point across the note — the reading of a patch
        // with no modulator at all. The factory clavinet does not do this; its
        // modulators hold at a nonzero L3 for as long as the key is down, and
        // that is where its bite is.
        //
        // Two assertions, both about shape rather than how bright a voice ought
        // to be:
        //
        // * a floor that only says "an operator is still modulating this";
        // * a fall from 50 ms to 800 ms, because a plucked string mellows as it
        //   rings. A modulator pinned at L1 would pass the floor and is just as
        //   wrong — it is a static, buzzy tone rather than a pluck.
        for name in ["CLAV    1", "SITAR"] {
            let voice = (0..VOICE_COUNT).find(|&v| voice_name(v) == name).expect("factory voice");
            let b = brightness_through_the_ring(voice, 60);
            for (reading, floor) in b.iter().zip([1.35, 1.20, 1.10]) {
                assert!(*reading >= floor,
                    "{name} rings at {reading:.2}x its fundamental, want at least {floor:.2}x \
                     — the modulators have died and left a bare sine: {b:.2?}");
            }
            assert!(b[0] > b[2] * 1.1,
                "{name} is as bright at 800 ms as at 50 ms ({b:.2?}); a plucked string \
                 mellows as it rings rather than holding one timbre");
        }
    }

}
