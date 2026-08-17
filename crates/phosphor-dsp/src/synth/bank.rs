//! The patch bank: four sets of charts, in three different situations.
//!
//! A chart is a row of numbers, one per panel control, and `chart_params` in
//! the parent module turns it into the parameter block the engine reads. What
//! is worth saying here is where the numbers come from, because the three
//! large sets in this file are not the same kind of thing and the code should
//! not imply that they are.
//!
//! ## P.01 – P.11, the instrument's own
//!
//! The eleven patches this synthesizer shipped with, unchanged. Patch zero is
//! the panel the instrument loads with, and the two sequenced patches and the
//! starter kit are what the engine's own tests measure, so they stay at the
//! front and stay as they were.
//!
//! ## A.11 – B.88, the microKORG: authentic names, authored values
//!
//! The names, the slot numbers, the categories, the tempo column and the
//! arpeggiator column are the real factory list, read from Korg's own Voice
//! Name List. **The parameter values are authored** — the same rule the
//! Jupiter-8's 64 patches in this project follow — because a name is not a
//! patch. What makes them more than invention is that the name usually says
//! how the sound is made: `Acid Saw Bass` is a saw through a resonant
//! low-pass with an envelope on the corner, `Unison Ring Lead` is oscillators
//! in unison with one of them ring-modulating the rest, `X-Mod Perc.` is
//! cross modulation on a percussive envelope, and `MG Bass 1` and `MG Bass 2`
//! are Moog-style basses, MG being Korg's own shorthand for Moog.
//!
//! Where the hardware has something this instrument does not, the substitution
//! is named in the patch's own comment rather than left for someone to
//! discover. The four that recur:
//!
//! * **no ring modulator.** Oscillator D out of the mixer and onto the
//!   amplitude destination is amplitude modulation, which puts the same sum
//!   and difference sidebands either side of the carrier and leaves the
//!   carrier in the middle. Used by `Acid Ring Bass`, `Techstep Ring Bass`,
//!   `Unison Ring Lead`, `Ring Chord`, `Short Ring Perc.` and `RingSync Bass`.
//! * **no oscillator sync.** What a sync sweep is, heard rather than built, is
//!   a formant moving through a fixed pitch, so the sync patches walk the
//!   wavetable position with envelope 2 under a resonant peak. Used by `Sync
//!   Bass`, `Gated Sync Bass`, `Sweep Sync Lead`, `Reverse Sync Lead` and
//!   `RingSync Bass`.
//! * **no high-pass filter.** The ladder's own bass loss at resonance is the
//!   substitute — the same property the kits' hi-hats are built on. Used by
//!   `Unison HPF+LPF`, `HPF Sweep Bass`, `HPF m7 Chord` and `BPF 4th Pad`.
//! * **no effects.** There is no delay line in this instrument, so the
//!   `Phaser`, `Flanger` and `Chorus` programs are built from what a sweep
//!   leaves behind: two slow LFOs at unrelated rates on the corner and the
//!   wave position, and detuning for the chorus.
//!
//! ### The vocoder programs
//!
//! Sixteen of the 128 are vocoder programs, and this instrument has neither a
//! vocoder nor an audio input, so they cannot be what they are on the
//! hardware. They are voiced as **the carrier alone** — the two formant
//! wavetables through the ladder, which is what a vocoder patch sounds like
//! with nothing plugged into it — rather than as some unrelated sound wearing
//! a vocoder's name. The programming that survives the missing modulator is
//! the part that was always in the carrier: the register, the fifths, the
//! pulse and square carriers, the wah, and — on `Voice Changer` and `Vocoder
//! Vox Wave` — the vowel itself moving.
//!
//! ### The arpeggio programs
//!
//! Eighteen of them are arpeggio programs, and there is no arpeggiator here
//! either. They are voiced as the sound the arpeggiator would be playing, and
//! where the pattern is part of the sound they are given a **wave sequence**,
//! which is the closest thing the engine has: a step list with a pitch column
//! is an arpeggio that runs on one held note. The factory list gives every
//! program a tempo, so the sequence clock is set from it rather than picked —
//! a sixteenth note at 138 is 9.2 Hz, which is `seq_rate_at(5.20)`.
//!
//! Nine of the eighteen carry the riff, stab or bell-run sequences, whose
//! pitch columns make them arpeggios outright. Eight carry the gate, the
//! vowel drift or the eight-step morph, which are the rhythm and the movement
//! without the pitch. The eighteenth is `B.21 S&H Signal`, which has no step
//! list at all: its part is a sample-and-hold on *pitch* at a sixteenth of
//! 138, a random note per step, which is what that name asks for and what no
//! step list in the bank can give.
//!
//! One thing about the substitution is worth stating because it is a trap. An
//! arpeggiator retriggers the envelope on every step; a wave sequence plays
//! under one held note and cannot. So a percussive arpeggio has to take its
//! rhythm from the *rests in the step list* and let the amplifier hold —
//! `Bleeps Perc.` and four others were first written the other way round,
//! with a tenth-of-a-second envelope over a step list, and the sequence was
//! inaudible because the note had ended before the second step arrived.
//! `every_sequenced_patch_is_audibly_sequenced` is what found that and what
//! would find it again.
//!
//! ## M.01 – M.40, the Minimoog: authored, and honestly so
//!
//! **The Minimoog has no factory presets.** It is knob-per-function with no
//! patch memory at all, which is the instrument's design rather than a gap in
//! the research — so every Minimoog patch bank anywhere is authored, and this
//! one says so rather than implying a factory list that never existed. The
//! names say what each patch is and what the machine is known for.
//!
//! This is the set that leans on the three Minimoog ideas in the engine: the
//! ladder and its bass loss, the drive stage before the filter rather than
//! after it, and oscillator D given up to the modulation bus. `OSC3 VIBRATO`
//! and `OSC3 GROWL` are the same trade at two rates, `SINE DRONE` and `FILTER
//! SINE` are the filter used as the oscillator, and `KICK DRUM` is the
//! pitch-envelope trick the instrument is as famous for as its bass.
//!
//! ## W.01 – W.50, the Wavestation: authored, in its idiom
//!
//! The original factory performance names were not findable in
//! machine-readable form, so this set is authored too. What makes it a
//! Wavestation bank is the technique rather than the names: **wave sequencing
//! and vector movement**. W.01 to W.12 are pads whose timbre walks a step
//! list, W.13 to W.22 are parts played by the sequencer on one held note,
//! W.23 to W.30 are vector movement with no sequence at all, W.31 to W.38 are
//! struck tones from the additive tables, and W.39 to W.47 are choirs and
//! strings built by crossfading the formant tables.
//!
//! W.48 to W.50 are **drum kits**, which is the specific thing this set is
//! here for: this machine and the M1 put whole kits in the patch list beside
//! the pads, and a kit made from the same oscillators, ladder and envelopes as
//! the pads sits in the same world rather than sounding bolted on. Three of
//! them, not one — an analog-leaning kit, a wavetable kit and a hand
//! percussion kit — because they are three obviously different sounds out of
//! the same engine.
//!
//! ## Where the numbers came from
//!
//! Every value here is a knob position, 0..1, because that is what the engine
//! reads. They were authored in musical units and converted: a cutoff in
//! hertz through the slider's three-decade taper, an envelope segment in
//! seconds through its own, an LFO in hertz, a sequence clock as a note value
//! at the program's own tempo. So A.12's `filter: [0.35, 0.8, 0.75, 0.35]` is
//! a corner at 180 Hz, four fifths of the resonance travel, half of the
//! envelope's depth and a third of an octave of corner per octave of
//! keyboard — the numbers are only unreadable if they are read as numbers.

// Two lints this file switches off, both of them consequences of being data
// rather than code.
//
// A knob position that happens to round to 0.318 is a decay of 320 ms, not an
// approximation of 1/π, and there are three of them in 229 patches.
#![allow(clippy::approx_constant)]
// `large_const_arrays` asks for a `static`, which this cannot be: a static
// cannot be read during const evaluation, and `PARAM_DEFAULTS` is a `const`
// derived from row zero of this table. That derivation is what keeps the
// default panel and patch zero from drifting apart, and it is worth more than
// the duplication the lint is about.
#![allow(clippy::large_const_arrays)]

use super::{seq_rate_at, Chart, KeyChart, KeyMap, OscChart, MOD_SLOTS, NO_ROUTE};

/// How many patches the instrument carries.
pub const PATCH_COUNT: usize = 229;

/// How many sets the bank is divided into.
pub const BANK_COUNT: usize = 4;

/// The sets, in bank order.
///
/// Named rather than numbered because a player picking a patch wants to know
/// which instrument's world they are in, and because the sets are not the same
/// kind of data — see this module's documentation. Cut to the twelve columns
/// the editor's selector row leaves for a label, like everything else that has
/// to fit that column.
pub const BANK_NAMES: [&str; BANK_COUNT] = ["PHOSPHOR", "microKORG", "MINIMOOG", "WAVESTATN"];

/// Where each set starts, and where the last one ends: set `i` is
/// `BANK_FIRST[i]..BANK_FIRST[i + 1]`.
pub const BANK_FIRST: [usize; BANK_COUNT + 1] = [0, 11, 139, 179, 229];

/// Every patch in the instrument, in selector order.
pub const BANK: [Chart; PATCH_COUNT] = [
    // ── P.01 – P.11 · the instrument's own ──
    // Four sawtooths, one an octave down, spread a few cents apart. The panel
    // the instrument loads with, and the one every headroom figure quoted at
    // OUTPUT_TRIM is measured against.
    Chart {
        name: "INIT SAW", label: "P01 INIT SAW", slot: "P.01",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 7.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -7.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.0,
        filter: [0.66, 0.12, 0.58, 0.35], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.35), (0, 0.15)],
        env: [[0.02, 0.35, 0.75, 0.22], [0.0, 0.40, 0.30, 0.30]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The Minimoog end of the instrument: everything low, the drive well up,
    // the ladder resonant and swept by envelope 2, and velocity opening it
    // further through the matrix.
    Chart {
        name: "LADDER BASS", label: "P02 LADDRBAS", slot: "P.02",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 4.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.40], pulse_width: 0.35, drive: 0.55,
        filter: [0.28, 0.55, 0.72, 0.30], velocity: 0.75, gain: 1.0,
        lfo: [(0, 0.30), (4, 0.55)],
        env: [[0.004, 0.30, 0.35, 0.12], [0.0, 0.22, 0.0, 0.15]],
        matrix: [
            (5, 4, 0.30),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The other Minimoog trade: oscillator D is out of the mixer and running
    // at 5 Hz as a vibrato source, with the wheel opening the filter.
    Chart {
        name: "FAT LEAD", label: "P03 FAT LEAD", slot: "P.03",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 12.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -12.0, level: 0.95 },
            OscChart { shape: 3, table: 0.0, semitones: 10, cents: 0.0, level: 1.0 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 2, vector: [0.5, 0.35], pulse_width: 0.25, drive: 0.40,
        filter: [0.55, 0.35, 0.60, 0.40], velocity: 0.5, gain: 1.0,
        lfo: [(0, 0.45), (1, 0.20)],
        env: [[0.01, 0.45, 0.70, 0.20], [0.02, 0.35, 0.25, 0.25]],
        matrix: [
            (10, 1, 0.03),  // oscillator D → pitch: a light vibrato
            (7, 4, 0.25),   // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The Wavestation end: four wavetable positions with the vector walked
    // round them by two slow LFOs at different rates, so the mix never repeats
    // on any period a listener can count.
    Chart {
        name: "WAVE PAD", label: "P04 WAVE PAD", slot: "P.04",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.30, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.55, semitones: 0, cents: 5.0, level: 1.0 },
            OscChart { shape: 4, table: 0.72, semitones: 0, cents: -5.0, level: 1.0 },
            OscChart { shape: 4, table: 0.90, semitones: -12, cents: 0.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.15,
        filter: [0.62, 0.20, 0.55, 0.30], velocity: 0.4, gain: 1.0,
        lfo: [(1, 0.16), (1, 0.10)],
        env: [[0.45, 0.50, 0.85, 0.60], [0.30, 0.60, 0.50, 0.55]],
        matrix: [
            (1, 7, 0.45),   // LFO 1 → vector x
            (2, 8, 0.45),   // LFO 2 → vector y
            (4, 3, 0.12),   // envelope 2 → wavetable position
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The sparse-partial end of the bank, struck rather than blown: bell and
    // electric piano tables an octave and a twelfth apart, no sustain.
    Chart {
        name: "GLASS BELL", label: "P05 GLASBELL", slot: "P.05",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.80, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.867, semitones: 12, cents: 2.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.5 },
            OscChart { shape: 4, table: 1.0, semitones: 24, cents: 0.0, level: 0.35 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.0,
        filter: [0.80, 0.10, 0.58, 0.50], velocity: 0.85, gain: 1.0,
        lfo: [(1, 0.55), (4, 0.65)],
        env: [[0.0, 0.55, 0.0, 0.50], [0.0, 0.30, 0.0, 0.28]],
        matrix: [
            (4, 3, 0.10),   // envelope 2 → wavetable position
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The vector as the whole point: four unrelated tables, a square LFO
    // stepping x and a triangle sweeping y, so the timbre moves rhythmically
    // without an envelope doing it.
    Chart {
        name: "VECTOR SWEEP", label: "P06 VECSWEEP", slot: "P.06",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.07, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.40, semitones: 0, cents: 6.0, level: 1.0 },
            OscChart { shape: 4, table: 0.60, semitones: 7, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: -6.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.25,
        filter: [0.66, 0.30, 0.55, 0.30], velocity: 0.5, gain: 1.0,
        lfo: [(3, 0.55), (0, 0.38)],
        env: [[0.06, 0.50, 0.80, 0.35], [0.10, 0.45, 0.60, 0.35]],
        matrix: [
            (1, 7, 0.50),   // LFO 1 → vector x
            (2, 8, 0.50),   // LFO 2 → vector y
            (6, 4, 0.15),   // keyboard → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The filter as the sound source, which is what the top of the resonance
    // travel is for: a whisper of noise into a ladder that is already
    // oscillating, swept by envelope 2 and by an LFO.
    Chart {
        name: "RESO DRONE", label: "P07 RESODRON", slot: "P.07",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.35 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 0.30 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.35 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.30 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.0,
        filter: [0.30, 0.98, 0.66, 0.60], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.22), (1, 0.12)],
        env: [[0.25, 0.70, 0.65, 0.55], [0.35, 0.70, 0.35, 0.50]],
        matrix: [
            (1, 4, 0.18),   // LFO 1 → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Short, bright and digital: the pseudo-random table at the top of the
    // bank, a clavinet under it, the drive up and nothing sustaining.
    Chart {
        name: "DIGI PLUCK", label: "P08 DIGIPLCK", slot: "P.08",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.933, semitones: 0, cents: 8.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.60, semitones: 12, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.45,
        filter: [0.62, 0.35, 0.70, 0.45], velocity: 0.8, gain: 1.0,
        lfo: [(2, 0.60), (4, 0.70)],
        env: [[0.0, 0.28, 0.10, 0.18], [0.0, 0.18, 0.0, 0.15]],
        matrix: [
            (5, 3, 0.15),   // velocity → wavetable position
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The wave sequencer as a pad: three of the four oscillators walking step
    // lists of 8, 10 and 6 ticks, which do not come back into step for 120 of
    // them — two minutes at this clock — while the fourth holds a steady bed
    // underneath so the patch has something that does not move. The vector
    // walks between them under two slow LFOs, so which sequence is loudest is
    // itself changing. Nothing about this sound repeats on a period a listener
    // can count, and no envelope in the instrument could produce any of it.
    Chart {
        name: "SEQ PAD", label: "P09 SEQ PAD", slot: "P.09",
        osc: [
            // The three sequenced oscillators sit at TABLE 0.5. Any value
            // would play the sequence as written — the knob is an offset from
            // whatever the patch left it at — and the middle is chosen so that
            // a player has travel in both directions.
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 6.0, level: 1.0 },
            OscChart { shape: 4, table: 0.35, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: -6.0, level: 0.9 },
        ],
        keys: KeyMap::Melodic,
        // morph 8, vox 4, — and organ 3: 8, 10 and 6 ticks.
        seq: [Some(1), Some(2), None, Some(6)],
        seq_rate: seq_rate_at(3.0),      // 2 Hz, a step every half second
        // The vector rests towards the A corner rather than in the middle, so
        // that one sequence is in front and the other two are behind it. Dead
        // centre is four things at a quarter each, and four timbres all moving
        // at once average out into one that does not.
        d_mode: 0, vector: [0.35, 0.35], pulse_width: 0.0, drive: 0.15,
        // The cutoff is well up, and it has to be: what a wave sequence
        // changes is the harmonics above the fourth, and a corner down where a
        // pad's usually sits takes the sequence off with them.
        filter: [0.75, 0.15, 0.50, 0.30], velocity: 0.4, gain: 1.0,
        lfo: [(1, 0.14), (1, 0.09)],
        env: [[0.35, 0.55, 0.85, 0.60], [0.20, 0.60, 0.40, 0.50]],
        matrix: [
            (1, 7, 0.20),   // LFO 1 → vector x
            (2, 8, 0.20),   // LFO 2 → vector y: a slow drift behind the
                            // sequences rather than over the top of them
            (4, 9, 0.20),   // envelope 2 → sequence clock: the pattern comes
                            // in fast and settles as the note does
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The other thing a step list is for: a part rather than a texture. The
    // clavinet riff on A is four steps of pitch, the pulse under it on B is
    // four steps of *rhythm* — the same sequencer with the waveform column
    // doing nothing, because a pulse oscillator has no table to read — and C
    // plays its bell-and-clav attack once and holds, so the note has a strike
    // on it that no envelope shape could give. D is the bass and is not
    // sequenced at all.
    Chart {
        name: "SEQ RIFF", label: "P10 SEQ RIFF", slot: "P.10",
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 4.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: -4.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -24, cents: 0.0, level: 0.7 },
        ],
        keys: KeyMap::Melodic,
        // riff 5th, gate 4, attack (played once), —
        seq: [Some(3), Some(0), Some(4), None],
        seq_rate: seq_rate_at(5.0),      // 8 Hz, a sixteenth at 120
        d_mode: 0, vector: [0.40, 0.40], pulse_width: 0.30, drive: 0.35,
        filter: [0.68, 0.35, 0.55, 0.30], velocity: 0.7, gain: 1.0,
        lfo: [(3, 0.42), (0, 0.20)],
        env: [[0.004, 0.45, 0.70, 0.18], [0.0, 0.30, 0.20, 0.20]],
        matrix: [
            // Nothing here is pointed at the vector, which is unusual for this
            // instrument and is the point: on this patch the thing that moves
            // is the step list, and an LFO stepping the mix underneath it
            // would only be a second answer to the same question.
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The keymapped end: one patch where every note is a different sound,
    // built out of the same four oscillators, ladder and envelopes as the
    // pads. The panel row below is what stays live on a kit — the drive, the
    // velocity depth, the LFOs and the matrix — plus the CUTOFF value the
    // knob's offset is measured from. Its oscillator and envelope columns are
    // the kick's, so that a player who opens the panel sees something
    // plausible rather than whatever the last patch left.
    Chart {
        name: "SYNTH KIT", label: "P11 SYNTHKIT", slot: "P.11",
        keys: KeyMap::Keymapped(&SYNTH_KIT),
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.15, 0.15], pulse_width: 0.0, drive: 0.30,
        filter: [0.60, 0.30, 0.50, 0.0], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.35), (0, 0.20)],
        env: [[0.0, 0.30, 0.0, 0.10], [0.0, 0.05, 0.0, 0.05]],
        matrix: [
            // The three routings that turn four oscillators into percussion,
            // and every one of them lands differently on each note because
            // every note brings its own envelope 2 and its own vector.
            (4, 1, 0.35),   // envelope 2 → pitch: the drop a struck drum has
            (4, 7, 0.60),   // envelope 2 → vector x
            (4, 8, 0.60),   // envelope 2 → vector y: together, the noise
                            // transient, which decays into the body
            (5, 4, 0.20),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE,
        ],
    },

    // ── A.11 – A.88 · microKORG bank A ──
    // Korg's own eight rows — TRANCE, TECHNO/HOUSE, ELECTRONICA, D'n'B/BREAKS,
    // HIPHOP/VINTAGE, RETRO, S.E./HIT and VOCODER, in that order, eight
    // programs to a row. The row is not stored anywhere; it is the first digit
    // of the slot, and it is worth knowing because it is why A.51 Dirty Bass
    // and A.58 Tape Choir have more in common than either has with A.11.
    //
    // The Single/Layer column is read as a hint rather than as a rule. Most of
    // the Layer programs here carry two distinguishable timbres — the A/B pair
    // one sound and the C/D pair another, an octave or a register apart — but
    // where the name says unison or names a chord, the name wins: `Unison
    // HPF+LPF` is four pulses on one pitch and `4 OSC m7 Chord` is four notes,
    // whatever the column says.
    // The arpeggio the name is about is a step list: the riff on A walks root,
    // fifth, octave and minor third on a sixteenth of the 138 the factory list
    // gives this program, and the gate on B is the same clock with the pitch
    // column left flat. Under them an organ table and a sub hold the layer
    // still.
    Chart {
        name: "Trancey Arpeg.", label: "A11 TrancArp", slot: "A.11",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 9.0, level: 0.95 },
            OscChart { shape: 4, table: 0.3333, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
        ],
        seq: [Some(3), Some(0), None, None], seq_rate: seq_rate_at(5.2),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.35,
        filter: [0.647, 0.55, 0.7, 0.4], velocity: 0.55, gain: 1.0,
        lfo: [(0, 0.252), (1, 0.735)],
        env: [[0.04, 0.393, 0.6, 0.28], [0.0, 0.247, 0.0, 0.247]],
        matrix: [
            (1, 7, 0.15),   // lfo 1 → vector x
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // One saw and its octave into a ladder at four fifths of its resonance,
    // with envelope 2 shutting the corner behind every note and the wheel
    // opening it again. The acid line, which is a filter part played on a bass
    // rather than a bass part.
    Chart {
        name: "Acid Saw Bass", label: "A12 AcidSaw", slot: "A.12",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 6.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.3], pulse_width: 0.0, drive: 0.5,
        filter: [0.35, 0.8, 0.75, 0.35], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.424, 0.4, 0.179], [0.0, 0.297, 0.0, 0.207]],
        matrix: [
            (5, 4, 0.3),    // velocity → cutoff
            (7, 4, 0.35),   // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Four sawtooths on one pitch, spread fourteen cents either side. Nothing
    // else: no sub, no second register, no filter movement worth the name —
    // the whole sound is the beating between them.
    Chart {
        name: "Unison Saw Lead", label: "A13 UnisnSaw", slot: "A.13",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -14.0, level: 0.95 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -5.0, level: 0.95 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 6.0, level: 0.95 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 15.0, level: 0.95 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.35,
        filter: [0.699, 0.3, 0.625, 0.5], velocity: 0.5, gain: 1.0,
        lfo: [(1, 0.732), (0, 0.28)],
        env: [[0.032, 0.393, 0.8, 0.247], [0.077, 0.333, 0.3, 0.28]],
        matrix: [
            (1, 1, 0.02),   // lfo 1 → pitch
            (7, 4, 0.3),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // There is no high-pass filter in this instrument. What stands in for it
    // is the ladder's own bass loss at resonance — nine tenths of the travel
    // takes better than 12 dB off the bottom — so a corner in the middle of
    // the band reads as the two filters the name names.
    Chart {
        name: "Unison HPF+LPF", label: "A14 UniHPLPF", slot: "A.14",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -12.0, level: 0.9 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -4.0, level: 0.9 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 5.0, level: 0.9 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 13.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.3, drive: 0.4,
        filter: [0.583, 0.88, 0.65, 0.5], velocity: 0.5, gain: 1.0,
        lfo: [(0, 0.413), (0, 0.217)],
        env: [[0.019, 0.355, 0.75, 0.232], [0.0, 0.308, 0.2, 0.247]],
        matrix: [
            (1, 2, 0.3),    // lfo 1 → pulse width
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The cry is two things at once: envelope 2 pointed at pitch with a
    // negative amount, so every note scoops up into tune over its first fifth
    // of a second, and a sine LFO on top of it once it arrives.
    Chart {
        name: "Weepy Lead", label: "A15 WeepyLd", slot: "A.15",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 0.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.3,
        filter: [0.625, 0.65, 0.675, 0.5], velocity: 0.6, gain: 1.0,
        lfo: [(1, 0.743), (0, 0.28)],
        env: [[0.077, 0.424, 0.75, 0.308], [0.0, 0.232, 0.0, 0.247]],
        matrix: [
            (4, 1, -0.06),  // env 2 → pitch
            (1, 1, 0.035),  // lfo 1 → pitch
            (7, 4, 0.3),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Slippery is a sample-and-hold on the wavetable position: the four
    // oscillators sit on four neighbouring tables and the whole set slides a
    // fraction of a table every six tenths of a second, so the timbre never
    // settles where it was left.
    Chart {
        name: "Slippy Pad", label: "A16 SlipyPad", slot: "A.16",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.6, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: -6.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.15,
        filter: [0.612, 0.35, 0.65, 0.35], velocity: 0.4, gain: 1.0,
        lfo: [(4, 0.542), (0, 0.2)],
        env: [[0.393, 0.476, 0.8, 0.393], [0.308, 0.452, 0.4, 0.355]],
        matrix: [
            (1, 3, 0.12),   // lfo 1 → wavetable
            (2, 7, 0.3),    // lfo 2 → vector x
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Envelope 2 with a six-tenths attack, pointed at a corner that starts at
    // 300 Hz: the sweep is the note arriving rather than anything the player
    // does.
    Chart {
        name: "Sweep Poly Pad", label: "A17 SwepPoly", slot: "A.17",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 8.0, level: 0.95 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -7.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.25, drive: 0.2,
        filter: [0.424, 0.5, 0.775, 0.3], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.217), (0, 0.149)],
        env: [[0.308, 0.498, 0.85, 0.393], [0.551, 0.593, 0.5, 0.424]],
        matrix: [
            (1, 7, 0.2),    // lfo 1 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Four saws across two registers with the corner low enough to be heard as
    // a bowing, and an LFO on it slow enough not to be heard as an effect.
    Chart {
        name: "Filter Strings", label: "A18 FiltStrg", slot: "A.18",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -12.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -4.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 5.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 12.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.45], pulse_width: 0.0, drive: 0.1,
        filter: [0.547, 0.45, 0.675, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.304), (1, 0.137)],
        env: [[0.452, 0.551, 0.85, 0.452], [0.424, 0.517, 0.5, 0.424]],
        matrix: [
            (1, 4, 0.12),   // lfo 1 → cutoff
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The house organ stab, played by the sequencer rather than by a hand: the
    // gate on three of the four oscillators so that the rests are really
    // rests, and the three drawbar registrations walking on the fourth so the
    // stab is not the same chord every time. Six ticks against four, at a
    // sixteenth of 140.
    Chart {
        name: "Auto House", label: "A21 AutoHous", slot: "A.21",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.4, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 7.0, level: 0.85 },
            OscChart { shape: 4, table: 0.3333, semitones: 12, cents: 0.0, level: 0.7 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
        ],
        seq: [Some(0), Some(6), Some(0), Some(0)], seq_rate: seq_rate_at(5.22),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.2, drive: 0.45,
        filter: [0.667, 0.5, 0.675, 0.4], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.28), (0, 0.217)],
        env: [[0.014, 0.308, 0.5, 0.207], [0.0, 0.207, 0.0, 0.179]],
        matrix: [
            (5, 4, 0.25),   // velocity → cutoff
            (1, 7, 0.15),   // lfo 1 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The rave stab: saws through a driven ladder with the stab sequence on
    // all four oscillators, so the rests in its step list are the whole voice
    // stopping rather than a quarter of it. Its pitch column moves the stab an
    // octave down and a fourth up as it goes, at a sixteenth of 143.
    Chart {
        name: "Burnin' Rave", label: "A22 BurninRv", slot: "A.22",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 12.0, level: 0.95 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 7, cents: 0.0, level: 0.7 },
        ],
        seq: [Some(7), Some(7), Some(7), Some(7)], seq_rate: seq_rate_at(5.25),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.35, drive: 0.65,
        filter: [0.637, 0.62, 0.725, 0.4], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.664), (0, 0.252)],
        env: [[0.019, 0.393, 0.7, 0.247], [0.0, 0.28, 0.1, 0.247]],
        matrix: [
            (1, 4, 0.15),   // lfo 1 → cutoff
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Cross modulation, which this instrument has in the one place it could:
    // oscillator D out of the mixer, tracking the keyboard a twelfth and a
    // fifth up, and pointed at pitch. At this depth the sidebands are what is
    // heard rather than either oscillator. The percussion is the gate rather
    // than the envelope — the amplifier holds, and the step list is what cuts
    // it into notes, because an envelope that ends inside the first step would
    // leave the sequence inaudible.
    Chart {
        name: "X-Mod Perc.", label: "A23 XModPerc", slot: "A.23",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 0.8 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 1.0 },
        ],
        seq: [Some(0), Some(0), None, None], seq_rate: seq_rate_at(5.05),
        d_mode: 1, vector: [0.45, 0.25], pulse_width: 0.0, drive: 0.4,
        filter: [0.684, 0.5, 0.75, 0.5], velocity: 0.75, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.308, 0.8, 0.179], [0.0, 0.158, 0.0, 0.134]],
        matrix: [
            (10, 1, 0.28),  // oscillator D → pitch
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Saw, pulse and two octaves of sine, the corner at 280 Hz and a third of
    // a second of decay. The bass that is under the record rather than on it.
    Chart {
        name: "House Bass", label: "A24 HousBass", slot: "A.24",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 4.0, level: 0.8 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.3], pulse_width: 0.25, drive: 0.35,
        filter: [0.414, 0.45, 0.7, 0.3], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.374, 0.35, 0.179], [0.0, 0.26, 0.0, 0.179]],
        matrix: [
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The drive stage at 0.95, which on this instrument is before the filter
    // rather than after it — so what distorts is the mixer and the ladder
    // cleans up after it, which is the way round that sounds like an amplifier
    // being pushed.
    Chart {
        name: "Distorted Bass", label: "A25 DistBass", slot: "A.25",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.95 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -7.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.3, drive: 0.95,
        filter: [0.473, 0.5, 0.675, 0.3], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.393, 0.5, 0.198], [0.0, 0.28, 0.0, 0.198]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The same acid trick as A.12 on a square rather than a saw: the pulse
    // knob at the top of its travel, resonance past four fifths, and only odd
    // harmonics for the peak to find.
    Chart {
        name: "Acid Square Bass", label: "A26 AcidSqu", slot: "A.26",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 5.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.3], pulse_width: 0.0, drive: 0.45,
        filter: [0.366, 0.82, 0.775, 0.35], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.393, 0.35, 0.158], [0.0, 0.308, 0.0, 0.207]],
        matrix: [
            (7, 4, 0.35),   // wheel → cutoff
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // There is no oscillator sync here. What a sync sweep is, heard rather
    // than built, is a formant moving through a fixed pitch — so this is
    // envelope 2 walking the wavetable position from brass through reed to
    // clavinet under a resonant peak, which moves the same edge the same way.
    Chart {
        name: "Sync Bass", label: "A27 SyncBass", slot: "A.27",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 6.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.5,
        filter: [0.498, 0.6, 0.65, 0.3], velocity: 0.75, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.393, 0.45, 0.179], [0.0, 0.26, 0.0, 0.179]],
        matrix: [
            (4, 3, 0.35),   // env 2 → wavetable
            (5, 3, 0.15),   // velocity → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Three saws and a pulse with the drive well up and the corner at 2.4 kHz:
    // a lead that has to cut through a mix that is already full.
    Chart {
        name: "Hard House Lead", label: "A28 HardHous", slot: "A.28",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 10.0, level: 0.95 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -9.0, level: 0.95 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.3, drive: 0.6,
        filter: [0.725, 0.45, 0.65, 0.55], velocity: 0.5, gain: 1.0,
        lfo: [(1, 0.748), (0, 0.28)],
        env: [[0.014, 0.355, 0.7, 0.207], [0.0, 0.28, 0.2, 0.207]],
        matrix: [
            (7, 4, 0.3),    // wheel → cutoff
            (1, 1, 0.02),   // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Two sequences of unequal length under a pad envelope: the eight-step
    // morph on A and the ten-step vowel drift on C, at an eighth of 130, which
    // do not come back into step for forty ticks.
    Chart {
        name: "Sequence Pad", label: "A31 SeqncePd", slot: "A.31",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 7.0, level: 0.85 },
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: -7.0, level: 0.8 },
        ],
        seq: [Some(1), None, Some(2), None], seq_rate: seq_rate_at(4.12),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.2,
        filter: [0.684, 0.3, 0.65, 0.3], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.191), (0, 0.123)],
        env: [[0.424, 0.517, 0.85, 0.424], [0.355, 0.476, 0.5, 0.393]],
        matrix: [
            (1, 7, 0.25),   // lfo 1 → vector x
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Bleeps on the stab sequence at a sixteenth of 94, which is the one step
    // list in the bank with both rests and pitches: the rests are what makes
    // each step a bleep rather than a change of note, and the amplifier holds
    // underneath so that the pattern is heard at all.
    Chart {
        name: "Bleeps Perc.", label: "A32 BleepPrc", slot: "A.32",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 0.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.5 },
        ],
        seq: [Some(7), None, None, None], seq_rate: seq_rate_at(4.65),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.25,
        filter: [0.758, 0.4, 0.7, 0.6], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.26, 0.85, 0.134], [0.0, 0.093, 0.0, 0.093]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Both halves of the name where the engine has them: the gate sequence on
    // A at a sixteenth of 102, and the sync-shaped formant from envelope 2 on
    // the wavetable position.
    Chart {
        name: "Gated Sync Bass", label: "A33 GateSync", slot: "A.33",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 5.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [Some(0), None, None, None], seq_rate: seq_rate_at(4.77),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.0, drive: 0.5,
        filter: [0.483, 0.6, 0.675, 0.3], velocity: 0.75, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.424, 0.6, 0.179], [0.0, 0.28, 0.0, 0.179]],
        matrix: [
            (4, 3, 0.3),    // env 2 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The flap is a square LFO at 7.5 Hz on the cutoff, fast enough to be
    // heard as a rattle rather than a rhythm; the sweep is the second LFO
    // under it at a twentieth of that rate.
    Chart {
        name: "Flap & Sweep", label: "A34 FlapSwep", slot: "A.34",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 0.0, level: 0.7 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
        ],
        seq: [Some(0), Some(0), None, None], seq_rate: seq_rate_at(5.17),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.4,
        filter: [0.583, 0.7, 0.75, 0.4], velocity: 0.6, gain: 1.0,
        lfo: [(3, 0.783), (0, 0.325)],
        env: [[0.014, 0.355, 0.5, 0.247], [0.0, 0.308, 0.1, 0.247]],
        matrix: [
            (1, 4, 0.2),    // lfo 1 → cutoff
            (2, 4, 0.15),   // lfo 2 → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Tape played backwards is a swell with no attack at the end of it: both
    // envelopes take four tenths of a second to arrive and fifty milliseconds
    // to go.
    Chart {
        name: "Reverse Lead", label: "A35 RevLead", slot: "A.35",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 8.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.3,
        filter: [0.525, 0.55, 0.75, 0.4], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.498, 0.476, 0.9, 0.093], [0.517, 0.452, 0.6, 0.093]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The four digital tables at the top of the bank, with a sample-and-hold
    // nudging the position and both LFOs walking the vector: a pad whose grain
    // changes on a clock nobody set.
    Chart {
        name: "IDM Pad", label: "A36 IDM Pad", slot: "A.36",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 6.0, level: 0.8 },
            OscChart { shape: 4, table: 0.8667, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: -6.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.1,
        filter: [0.657, 0.4, 0.625, 0.3], velocity: 0.4, gain: 1.0,
        lfo: [(4, 0.592), (0, 0.172)],
        env: [[0.424, 0.551, 0.8, 0.424], [0.355, 0.498, 0.4, 0.393]],
        matrix: [
            (1, 3, 0.1),    // lfo 1 → wavetable
            (1, 7, 0.25),   // lfo 1 → vector x
            (2, 8, 0.25),   // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // No flanger — there is no delay line in this instrument. What is left of
    // one is the sweep: two pairs a fifth apart detuned against each other,
    // with a slow LFO on the corner and another on the vector at a different
    // rate, so the two never line up.
    Chart {
        name: "Flanger 5th Pad", label: "A37 Flang5th", slot: "A.37",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 7, cents: 5.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -9.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 7, cents: -14.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.15,
        filter: [0.583, 0.5, 0.65, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.232), (1, 0.182)],
        env: [[0.476, 0.551, 0.85, 0.452], [0.452, 0.517, 0.5, 0.424]],
        matrix: [
            (1, 4, 0.18),   // lfo 1 → cutoff
            (2, 7, 0.3),    // lfo 2 → vector x
            (1, 8, 0.2),    // lfo 1 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The vowel table on its own, four times over and barely detuned, with the
    // corner just above the second formant. The vocoder programs at A.81 are
    // this sound with a job.
    Chart {
        name: "Voice / A/", label: "A38 Voice/A/", slot: "A.38",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 4, table: 0.7333, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: -8.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.1,
        filter: [0.599, 0.4, 0.6, 0.35], velocity: 0.4, gain: 1.0,
        lfo: [(1, 0.714), (0, 0.217)],
        env: [[0.424, 0.517, 0.9, 0.393], [0.393, 0.476, 0.5, 0.355]],
        matrix: [
            (1, 1, 0.015),  // lfo 1 → pitch
            (2, 3, 0.06),   // lfo 2 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Sub sine in front, pulse behind it, and a 3 Hz triangle on the corner:
    // the wobble is slow enough to be a groove rather than a timbre.
    Chart {
        name: "2 Step Bass", label: "A41 2StepBas", slot: "A.41",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 6.0, level: 0.8 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.3], pulse_width: 0.3, drive: 0.4,
        filter: [0.434, 0.55, 0.7, 0.3], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.64), (0, 0.36)],
        env: [[0.01, 0.355, 0.4, 0.179], [0.0, 0.247, 0.0, 0.179]],
        matrix: [
            (1, 4, 0.12),   // lfo 1 → cutoff
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // There is no ring modulator either. Oscillator D on the amplitude
    // destination is amplitude modulation, which gives the same sum and
    // difference sidebands with the carrier still in the middle — at a twelfth
    // and a fifth above the note, and inharmonic, which is the part that makes
    // it sound rung rather than tuned.
    Chart {
        name: "Techstep Ring Bass", label: "A42 TechRing", slot: "A.42",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 1.0 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 1, vector: [0.45, 0.25], pulse_width: 0.35, drive: 0.7,
        filter: [0.447, 0.6, 0.675, 0.3], velocity: 0.75, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.393, 0.45, 0.179], [0.0, 0.247, 0.0, 0.179]],
        matrix: [
            (10, 6, 0.6),   // oscillator D → amplitude
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The kick is envelope 2 on pitch with a fifty-millisecond decay, which
    // drops the whole voice most of an octave in the time it takes to say it;
    // the valve is the drive stage at 0.85 under it.
    Chart {
        name: "Valve Kick Bass", label: "A43 ValvKick", slot: "A.43",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.4667, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.3], pulse_width: 0.0, drive: 0.85,
        filter: [0.466, 0.4, 0.65, 0.25], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.374, 0.4, 0.179], [0.0, 0.093, 0.0, 0.093]],
        matrix: [
            (4, 1, 0.3),    // env 2 → pitch
            (5, 4, 0.2),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The drive knob at the top of its travel and nothing else unusual — three
    // saws and a sub, so that what is heard is the stage rather than the
    // programming.
    Chart {
        name: "Drive Bass", label: "A44 DrivBass", slot: "A.44",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 8.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.95 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.0, drive: 1.0,
        filter: [0.459, 0.45, 0.65, 0.3], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.393, 0.5, 0.179], [0.0, 0.26, 0.0, 0.179]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The clavinet table's rasp — everything up to the twenty-fourth harmonic,
    // hardly falling — through a resonant corner at 600 Hz, with a tenth of a
    // second of decay. Edge rather than weight.
    Chart {
        name: "Blade Bass", label: "A45 BladBass", slot: "A.45",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.55,
        filter: [0.525, 0.68, 0.7, 0.35], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.333, 0.3, 0.158], [0.0, 0.207, 0.0, 0.158]],
        matrix: [
            (5, 4, 0.3),    // velocity → cutoff
            (4, 3, 0.15),   // env 2 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The A.27 formant sweep given a whole second to travel and a resonance of
    // 0.72 to travel under, with the wheel able to push it further by hand.
    Chart {
        name: "Sweep Sync Lead", label: "A46 SwepSync", slot: "A.46",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: -7.0, level: 0.8 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.45,
        filter: [0.625, 0.72, 0.675, 0.45], velocity: 0.55, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.019, 0.424, 0.75, 0.247], [0.452, 0.498, 0.2, 0.355]],
        matrix: [
            (4, 3, 0.4),    // env 2 → wavetable
            (7, 3, 0.2),    // wheel → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The pseudo-random table at the top of the bank with a bell above it and
    // a sample-and-hold at 6.5 Hz on the position: a lead that keeps changing
    // its mind about which harmonics it has.
    Chart {
        name: "Science Lead", label: "A47 SciLead", slot: "A.47",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 6.0, level: 0.8 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.35,
        filter: [0.713, 0.5, 0.65, 0.5], velocity: 0.6, gain: 1.0,
        lfo: [(4, 0.761), (0, 0.325)],
        env: [[0.014, 0.393, 0.65, 0.247], [0.0, 0.308, 0.15, 0.247]],
        matrix: [
            (1, 3, 0.08),   // lfo 1 → wavetable
            (7, 4, 0.3),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A minor seventh built by tuning the four oscillators to it — 0, 3, 7 and
    // 10 — with the gate sequence on all four at once, so what is chopped is
    // the chord rather than a part of it.
    Chart {
        name: "Gated Chord", label: "A48 GatChord", slot: "A.48",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 3, cents: 0.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 7, cents: 0.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 10, cents: 0.0, level: 0.85 },
        ],
        seq: [Some(0), Some(0), Some(0), Some(0)], seq_rate: seq_rate_at(5.12),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.35,
        filter: [0.667, 0.45, 0.65, 0.35], velocity: 0.5, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.019, 0.424, 0.8, 0.247], [0.0, 0.308, 0.2, 0.247]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // A third of an oscillator's worth of noise in the mix and the drive at
    // 0.9: dirt here is broadband rather than harmonic, which is what
    // separates it from A.25.
    Chart {
        name: "Dirty Bass", label: "A51 DirtBass", slot: "A.51",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -8.0, level: 0.9 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.35 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.3], pulse_width: 0.4, drive: 0.9,
        filter: [0.404, 0.5, 0.65, 0.25], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.014, 0.424, 0.5, 0.207], [0.0, 0.28, 0.0, 0.207]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // MG is Korg's shorthand for Moog, and this is the bass that machine is
    // known for: two saws at 8' beating seven cents apart, a third at 16', the
    // ladder a quarter open with envelope 2 half over it, and the mixer pushed
    // past unity into it.
    Chart {
        name: "MG Bass 1", label: "A52 MG Bass1", slot: "A.52",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -7.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.95 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.0, drive: 0.45,
        filter: [0.392, 0.4, 0.725, 0.35], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.409, 0.45, 0.207], [0.0, 0.308, 0.0, 0.207]],
        matrix: [
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Triangle and sine with the corner well up and the resonance at nothing:
    // the one lead in the bank with no edge on it at all, which is what makes
    // it sit under a voice.
    Chart {
        name: "R&B Lead", label: "A53 R&B Lead", slot: "A.53",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 5.0, level: 0.9 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 0.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.2,
        filter: [0.647, 0.3, 0.6, 0.5], velocity: 0.5, gain: 1.0,
        lfo: [(1, 0.72), (0, 0.28)],
        env: [[0.108, 0.452, 0.85, 0.308], [0.158, 0.355, 0.3, 0.308]],
        matrix: [
            (1, 1, 0.03),   // lfo 1 → pitch
            (7, 4, 0.25),   // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Four pulses with two triangle LFOs at 0.45 and 0.28 Hz on the width,
    // which is the string machine's whole trick: the duty cycle moving is what
    // a chorus does to a chord, done in the oscillator instead.
    Chart {
        name: "PWM Strings", label: "A54 PWM Strg", slot: "A.54",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 6.0, level: 0.95 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -8.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.45], pulse_width: 0.25, drive: 0.15,
        filter: [0.583, 0.35, 0.625, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.343), (1, 0.269)],
        env: [[0.424, 0.551, 0.9, 0.424], [0.393, 0.498, 0.5, 0.393]],
        matrix: [
            (1, 2, 0.35),   // lfo 1 → pulse width
            (2, 2, 0.2),    // lfo 2 → pulse width
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The electric piano table with a bell three cents off it, no sustain to
    // speak of and a second and a half of decay — and velocity on both the
    // wavetable position and the corner, so playing harder finds the bell
    // rather than just more of the same note.
    Chart {
        name: "Reed Piano", label: "A55 ReedPno", slot: "A.55",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.8667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 3.0, level: 0.6 },
            OscChart { shape: 4, table: 0.8667, semitones: 12, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.1,
        filter: [0.667, 0.2, 0.65, 0.55], velocity: 0.85, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.58, 0.15, 0.333], [0.0, 0.355, 0.0, 0.308]],
        matrix: [
            (5, 3, 0.12),   // velocity → wavetable
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Two drawbar registrations, a twelfth above them, and the drive at a
    // third: the organ in this bank that is a rock organ rather than a church
    // one. Sustain at unity and a sixty-millisecond release, because an organ
    // has no envelope.
    Chart {
        name: "British Organ", label: "A56 BritOrgn", slot: "A.56",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.4, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.3333, semitones: 12, cents: 0.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.5 },
            OscChart { shape: 4, table: 0.4, semitones: -12, cents: 0.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.35,
        filter: [0.725, 0.2, 0.55, 0.3], velocity: 0.3, gain: 1.0,
        lfo: [(1, 0.761), (0, 0.217)],
        env: [[0.019, 0.308, 1.0, 0.108], [0.0, 0.158, 0.5, 0.093]],
        matrix: [
            (7, 4, 0.2),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Clavinet table, keyboard follow at 0.7 so the band tracks the note, and
    // velocity a third of the way onto the corner. Half a second of decay to a
    // fifth of its level.
    Chart {
        name: "Synth Clav", label: "A57 SynClav", slot: "A.57",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 5.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.6 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.4 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.3,
        filter: [0.583, 0.55, 0.7, 0.7], velocity: 0.85, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.393, 0.2, 0.179], [0.0, 0.247, 0.0, 0.179]],
        matrix: [
            (5, 4, 0.35),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Both vowel tables across two octaves with a 0.6 Hz triangle putting
    // twelve cents of wow on the pitch — the tape part of the name, which is a
    // defect of the machine rather than a feature of the choir.
    Chart {
        name: "Tape Choir", label: "A58 TapeChor", slot: "A.58",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: -9.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.1,
        filter: [0.612, 0.35, 0.575, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.388), (1, 0.191)],
        env: [[0.476, 0.58, 0.9, 0.452], [0.424, 0.517, 0.5, 0.424]],
        matrix: [
            (1, 1, 0.012),  // lfo 1 → pitch
            (2, 7, 0.25),   // lfo 2 → vector x
            (2, 3, 0.05),   // lfo 2 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The riff sequence on a clavinet at a sixteenth of 118: the same step
    // list as A.11 on a different oscillator and a different clock, which is a
    // different part rather than the same one re-skinned.
    Chart {
        name: "Elektric Arpeg.", label: "A61 ElektArp", slot: "A.61",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 6.0, level: 0.7 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.5 },
        ],
        seq: [Some(3), None, None, None], seq_rate: seq_rate_at(4.98),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.4,
        filter: [0.684, 0.5, 0.7, 0.5], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.333, 0.35, 0.179], [0.0, 0.207, 0.0, 0.158]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The vowel drift and the eight-step morph running against each other with
    // a 4 Hz sample-and-hold on the corner: ten ticks against eight, so the
    // two patterns meet every forty.
    Chart {
        name: "Water Edge", label: "A62 WaterEdg", slot: "A.62",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 8.0, level: 0.85 },
            OscChart { shape: 4, table: 0.7333, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.5 },
        ],
        seq: [Some(2), Some(1), None, None], seq_rate: seq_rate_at(5.22),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.2,
        filter: [0.657, 0.6, 0.65, 0.4], velocity: 0.5, gain: 1.0,
        lfo: [(4, 0.685), (0, 0.252)],
        env: [[0.077, 0.424, 0.6, 0.355], [0.0, 0.308, 0.2, 0.308]],
        matrix: [
            (1, 4, 0.15),   // lfo 1 → cutoff
            (2, 7, 0.25),   // lfo 2 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Pulse in front, saw behind, corner at 500 Hz and keyboard follow at
    // nearly half — the bass that tracks up the keyboard instead of going
    // dull, which is what dates it.
    Chart {
        name: "80's Synth Bass", label: "A63 80s Bass", slot: "A.63",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.3, drive: 0.4,
        filter: [0.498, 0.5, 0.7, 0.45], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.393, 0.4, 0.179], [0.0, 0.28, 0.0, 0.179]],
        matrix: [
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A.35's backwards envelope and A.27's formant sweep on one patch, which
    // is what the name asks for: the wavetable walks while the swell arrives,
    // and both stop dead at the key.
    Chart {
        name: "Reverse Sync Lead", label: "A64 RevSync", slot: "A.64",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: -7.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.35,
        filter: [0.547, 0.6, 0.725, 0.4], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.476, 0.498, 0.85, 0.108], [0.498, 0.476, 0.4, 0.108]],
        matrix: [
            (4, 3, 0.35),   // env 2 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The corner at 2.6 kHz and the resonance at a quarter: a polysynth with
    // nothing taken off the top, which is the setting every other pad in this
    // bank is a departure from.
    Chart {
        name: "Bright Poly Synth", label: "A65 BritPoly", slot: "A.65",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 7.0, level: 0.95 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -6.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.45], pulse_width: 0.25, drive: 0.25,
        filter: [0.737, 0.25, 0.625, 0.5], velocity: 0.5, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.577)],
        env: [[0.023, 0.424, 0.7, 0.28], [0.0, 0.333, 0.25, 0.28]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The same instrument with the corner an octave down and the envelope
    // longer: the workhorse setting rather than the bright one.
    Chart {
        name: "Poly Synth", label: "A66 Poly Syn", slot: "A.66",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 5.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: -6.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.35, drive: 0.2,
        filter: [0.637, 0.35, 0.675, 0.45], velocity: 0.55, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.577)],
        env: [[0.032, 0.452, 0.6, 0.308], [0.0, 0.355, 0.2, 0.308]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Stacked fourths — 0, 5, 12 and 17 — which is the interval that makes a
    // pad sound open rather than major or minor. Half a second of attack and
    // the corner at 700 Hz.
    Chart {
        name: "Warm 4th Pad", label: "A67 Warm4th", slot: "A.67",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.4667, semitones: 5, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 12, cents: -6.0, level: 0.8 },
            OscChart { shape: 4, table: 0.3333, semitones: 17, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.1,
        filter: [0.547, 0.35, 0.625, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.2), (1, 0.123)],
        env: [[0.517, 0.593, 0.9, 0.476], [0.476, 0.551, 0.5, 0.452]],
        matrix: [
            (1, 7, 0.25),   // lfo 1 → vector x
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The same saw at three octaves, each a few cents off the others: the
    // string sound that comes from the register rather than from the filter.
    Chart {
        name: "Octave Strings", label: "A68 OctStrgs", slot: "A.68",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 12, cents: 6.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: -6.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 12.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.45], pulse_width: 0.0, drive: 0.12,
        filter: [0.599, 0.3, 0.625, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(1, 0.703), (0, 0.217)],
        env: [[0.452, 0.566, 0.9, 0.464], [0.424, 0.517, 0.5, 0.424]],
        matrix: [
            (1, 1, 0.01),   // lfo 1 → pitch
            (2, 4, 0.1),    // lfo 2 → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A swarm: a 17 Hz sample-and-hold on the pitch, a 6 Hz triangle on the
    // corner under it, and a pulse at its narrowest so there is plenty for
    // both to chew on.
    Chart {
        name: "Killa Beez", label: "A71 KillaBez", slot: "A.71",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 14.0, level: 0.8 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.4 },
            OscChart { shape: 0, table: 0.0, semitones: 12, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.5, drive: 0.5,
        filter: [0.713, 0.6, 0.6, 0.4], velocity: 0.4, gain: 1.0,
        lfo: [(4, 0.911), (0, 0.748)],
        env: [[0.158, 0.476, 0.9, 0.308], [0.0, 0.355, 0.3, 0.308]],
        matrix: [
            (1, 1, 0.06),   // lfo 1 → pitch
            (2, 4, 0.2),    // lfo 2 → cutoff
            (2, 1, 0.02),   // lfo 2 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The factory list gives this one 240 BPM, which at a sixteenth is 16 Hz —
    // the top half of the sequence clock's travel, where the crossfades start
    // to be heard as a waveform rather than a rhythm. That is the effect this
    // patch is for.
    Chart {
        name: "Diginator", label: "A72 Diginatr", slot: "A.72",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 9.0, level: 0.8 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.6 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
        ],
        seq: [Some(7), Some(1), None, None], seq_rate: seq_rate_at(6.0),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.55,
        filter: [0.699, 0.55, 0.675, 0.4], velocity: 0.5, gain: 1.0,
        lfo: [(0, 0.28), (0, 0.217)],
        env: [[0.005, 0.393, 0.7, 0.207], [0.0, 0.247, 0.2, 0.207]],
        matrix: [
            (1, 7, 0.2),    // lfo 1 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The gate on all four oscillators at 16 Hz, which is fast enough that the
    // rests are heard as a broken sound rather than as a rhythm. Noise on B so
    // that what is being chopped has no pitch of its own.
    Chart {
        name: "Stutter", label: "A73 Stutter", slot: "A.73",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.6 },
        ],
        seq: [Some(0), Some(0), Some(0), None], seq_rate: seq_rate_at(6.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.4,
        filter: [0.667, 0.5, 0.65, 0.35], velocity: 0.5, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.577)],
        env: [[0.005, 0.476, 0.85, 0.207], [0.0, 0.308, 0.3, 0.247]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The arcade descent: envelope 2 at half depth on pitch, negative, with a
    // half-second decay, so every note falls an octave and stops. A square LFO
    // on the pulse width does the rest.
    Chart {
        name: "Invaders", label: "A74 Invaders", slot: "A.74",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.6 },
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.6, drive: 0.3,
        filter: [0.699, 0.45, 0.6, 0.3], velocity: 0.4, gain: 1.0,
        lfo: [(3, 0.793), (0, 0.28)],
        env: [[0.005, 0.424, 0.6, 0.207], [0.0, 0.393, 0.0, 0.308]],
        matrix: [
            (4, 1, -0.5),   // env 2 → pitch
            (1, 2, 0.2),    // lfo 1 → pulse width
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A major triad on three oscillators with the fourth taken out of the
    // mixer and put on the amplitude — so the whole chord is being modulated
    // by one inharmonic tone rather than each note carrying its own.
    Chart {
        name: "Ring Chord", label: "A75 RingChrd", slot: "A.75",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 4, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 7, cents: 0.0, level: 0.85 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 1.0 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 1, vector: [0.5, 0.25], pulse_width: 0.0, drive: 0.35,
        filter: [0.657, 0.45, 0.675, 0.35], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.577)],
        env: [[0.01, 0.452, 0.3, 0.28], [0.0, 0.308, 0.0, 0.247]],
        matrix: [
            (10, 6, 0.75),  // oscillator D → amplitude
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A minor triad and its octave, with the corner starting at 300 Hz and
    // envelope 2 taking a second to open it. The hit that arrives after the
    // beat it was played on.
    Chart {
        name: "Sweep min Chord", label: "A76 SweepMin", slot: "A.76",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 3, cents: 0.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 7, cents: 0.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 12, cents: 0.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.3,
        filter: [0.424, 0.6, 0.8, 0.3], velocity: 0.55, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.577)],
        env: [[0.04, 0.551, 0.5, 0.355], [0.628, 0.517, 0.2, 0.355]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Two noise sources, a saw and a digital table, all of it over in three
    // tenths of a second with the drive at 0.6. The only patch in the bank
    // whose loudest component has no pitch.
    Chart {
        name: "Noisy Hit", label: "A77 NoisyHit", slot: "A.77",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 0.8 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.6,
        filter: [0.684, 0.5, 0.75, 0.2], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.577)],
        env: [[0.0, 0.308, 0.0, 0.179], [0.0, 0.158, 0.0, 0.134]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The name is the programming: four oscillators tuned 0, 3, 7 and 10,
    // which is a minor seventh, one note to an oscillator.
    Chart {
        name: "4 OSC m7 Chord", label: "A78 4OSC m7", slot: "A.78",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 3, cents: 0.0, level: 0.95 },
            OscChart { shape: 0, table: 0.0, semitones: 7, cents: 0.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 10, cents: 0.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.4,
        filter: [0.675, 0.4, 0.675, 0.35], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.577)],
        env: [[0.014, 0.476, 0.35, 0.308], [0.0, 0.333, 0.0, 0.28]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The vowel table an octave down with the corner just above its second
    // formant, and nothing else: a male ah as the carrier alone would sound
    // it.
    Chart {
        name: "Male-Ahhh", label: "A81 MaleAhhh", slot: "A.81",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.7333, semitones: -12, cents: -6.0, level: 0.8 },
            OscChart { shape: 4, table: 0.6667, semitones: -24, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.15,
        filter: [0.547, 0.45, 0.575, 0.25], velocity: 0.4, gain: 1.0,
        lfo: [(1, 0.726), (0, 0.28)],
        env: [[0.158, 0.476, 0.9, 0.28], [0.158, 0.393, 0.5, 0.28]],
        matrix: [
            (1, 1, 0.012),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // An /i/ is an /a/ with its second formant moved up, so this is the same
    // table shifted towards the bell and the corner opened to 1.8 kHz to let
    // the peak through.
    Chart {
        name: "Male-Eeee", label: "A82 MaleEeee", slot: "A.82",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.75, semitones: -12, cents: 5.0, level: 0.85 },
            OscChart { shape: 4, table: 0.8, semitones: -12, cents: 0.0, level: 0.5 },
            OscChart { shape: 4, table: 0.6667, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.15,
        filter: [0.684, 0.55, 0.575, 0.3], velocity: 0.4, gain: 1.0,
        lfo: [(1, 0.726), (0, 0.28)],
        env: [[0.158, 0.476, 0.9, 0.28], [0.158, 0.393, 0.5, 0.28]],
        matrix: [
            (1, 1, 0.012),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A.81 with the second pair a fifth up, which is what the hardware's 5th
    // program does to the carrier before it reaches the filter bank.
    Chart {
        name: "Male-Ahhh 5th", label: "A83 MaleAh5", slot: "A.83",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6667, semitones: -5, cents: 5.0, level: 0.85 },
            OscChart { shape: 4, table: 0.7333, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.6667, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.15,
        filter: [0.566, 0.45, 0.575, 0.25], velocity: 0.4, gain: 1.0,
        lfo: [(1, 0.726), (0, 0.28)],
        env: [[0.158, 0.476, 0.9, 0.28], [0.158, 0.393, 0.5, 0.28]],
        matrix: [
            (1, 1, 0.012),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The ensemble is four voices detuned across two octaves with both LFOs
    // walking the vector between them — no chorus, because there is no delay
    // line; what is left of one is the beating and the moving balance.
    Chart {
        name: "Vocoder Ensemble", label: "A84 VocEnsmb", slot: "A.84",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: -14.0, level: 1.0 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: -5.0, level: 0.9 },
            OscChart { shape: 4, table: 0.7333, semitones: -12, cents: 13.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.45], pulse_width: 0.0, drive: 0.12,
        filter: [0.625, 0.4, 0.575, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.295), (1, 0.224)],
        env: [[0.308, 0.517, 0.9, 0.374], [0.247, 0.452, 0.5, 0.355]],
        matrix: [
            (1, 7, 0.3),    // lfo 1 → vector x
            (2, 8, 0.25),   // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The other half of that idea: the detuning left alone and a pair of slow
    // LFOs put on the pitch instead, at 0.31 and 0.52 Hz, which is the rate a
    // chorus runs its delay at.
    Chart {
        name: "Vocoder Chorus", label: "A85 VocChors", slot: "A.85",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 9.0, level: 0.95 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: -9.0, level: 0.9 },
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: 0.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.45], pulse_width: 0.0, drive: 0.12,
        filter: [0.612, 0.4, 0.575, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(1, 0.285), (0, 0.366)],
        env: [[0.308, 0.517, 0.9, 0.374], [0.247, 0.452, 0.5, 0.355]],
        matrix: [
            (1, 1, 0.014),  // lfo 1 → pitch
            (2, 1, 0.009),  // lfo 2 → pitch
            (1, 7, 0.15),   // lfo 1 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Fifths on every pair, at pitch rather than an octave down: the brightest
    // of the eight and the one that reads as an instrument rather than as a
    // voice.
    Chart {
        name: "Vocoder 5th", label: "A86 Voc 5th", slot: "A.86",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6667, semitones: 7, cents: 5.0, level: 0.85 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.7333, semitones: 7, cents: -6.0, level: 0.75 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.45], pulse_width: 0.0, drive: 0.12,
        filter: [0.647, 0.45, 0.575, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(1, 0.72), (0, 0.28)],
        env: [[0.216, 0.498, 0.9, 0.333], [0.158, 0.424, 0.5, 0.308]],
        matrix: [
            (1, 1, 0.01),   // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Both vowels at 16' and 32' with the drive at half: the carrier a bass
    // vocoder patch is built on, which is a different sound from a vocal one
    // an octave down because the drive puts harmonics back where the tables
    // have none.
    Chart {
        name: "Bass Vocoder", label: "A87 BassVoc", slot: "A.87",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.7333, semitones: -12, cents: 7.0, level: 0.9 },
            OscChart { shape: 4, table: 0.6667, semitones: -24, cents: 0.0, level: 0.85 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.5,
        filter: [0.483, 0.5, 0.625, 0.25], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.04, 0.452, 0.7, 0.247], [0.0, 0.333, 0.2, 0.247]],
        matrix: [
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The one vocoder program where the vowel itself moves: envelope 2 and a
    // slow sample-and-hold both on the wavetable position, which walks the
    // carrier between ah and oo and past them into the bell.
    Chart {
        name: "Voice Changer", label: "A88 VoiceChg", slot: "A.88",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: -7.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.2,
        filter: [0.637, 0.5, 0.6, 0.3], velocity: 0.45, gain: 1.0,
        lfo: [(4, 0.497), (0, 0.252)],
        env: [[0.108, 0.498, 0.85, 0.333], [0.355, 0.476, 0.3, 0.333]],
        matrix: [
            (4, 3, 0.18),   // env 2 → wavetable
            (1, 3, 0.1),    // lfo 1 → wavetable
            (7, 3, 0.15),   // wheel → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },

    // ── B.11 – B.88 · microKORG bank B ──
    // The same eight rows again. Where a B program is an obvious relative of
    // an A one — B.52 MG Bass 2 against A.52 MG Bass 1, B.55 Rock Organ
    // against A.56 British Organ — the pair is voiced as two settings of one
    // instrument rather than as two unrelated sounds, which is what the
    // factory pairing means.
    // A harp is a run of plucks, so this is the one-shot sequence — four bells
    // falling two octaves into a held table — at a sixteenth of 138. The run
    // takes half a beat and then stops, which is what a one-shot step list is
    // for and what no envelope could shape.
    Chart {
        name: "Synth Harp", label: "B11 SynHarp", slot: "B.11",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8667, semitones: 0, cents: 5.0, level: 0.7 },
            OscChart { shape: 4, table: 0.9333, semitones: 12, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.6 },
        ],
        seq: [Some(5), None, None, None], seq_rate: seq_rate_at(5.2),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.15,
        filter: [0.737, 0.25, 0.675, 0.6], velocity: 0.75, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.498, 0.45, 0.308], [0.0, 0.308, 0.0, 0.247]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // A.12's squelch with oscillator D taken out of the mixer and put on the
    // amplitude at a twelfth up — amplitude modulation rather than ring
    // modulation, which keeps the carrier in the middle but puts the same
    // sidebands either side of it.
    Chart {
        name: "Acid Ring Bass", label: "B12 AcidRing", slot: "B.12",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 6.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 1.0 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 1, vector: [0.4, 0.25], pulse_width: 0.3, drive: 0.45,
        filter: [0.379, 0.8, 0.75, 0.35], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.409, 0.35, 0.179], [0.0, 0.297, 0.0, 0.207]],
        matrix: [
            (10, 6, 0.45),  // oscillator D → amplitude
            (7, 4, 0.3),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Three saws in unison with the fourth oscillator ringing them at a major
    // tenth — an interval chosen because it is nearly harmonic, so the
    // sidebands land near the chord rather than beside it.
    Chart {
        name: "Unison Ring Lead", label: "B13 UniRing", slot: "B.13",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -10.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 4.0, level: 0.95 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 12.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: 16, cents: 0.0, level: 1.0 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 1, vector: [0.5, 0.25], pulse_width: 0.0, drive: 0.4,
        filter: [0.684, 0.4, 0.65, 0.5], velocity: 0.55, gain: 1.0,
        lfo: [(1, 0.735), (0, 0.28)],
        env: [[0.023, 0.393, 0.75, 0.247], [0.0, 0.308, 0.25, 0.247]],
        matrix: [
            (10, 6, 0.5),   // oscillator D → amplitude
            (7, 4, 0.3),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A phaser is a notch that moves. There is no all-pass chain here, so this
    // is the nearest thing the panel can build: two LFOs at 0.4 and 0.27 Hz,
    // one on the corner and one on the wavetable position, which move a peak
    // and a spectral hole past each other.
    Chart {
        name: "Phaser Lead", label: "B14 PhaserLd", slot: "B.14",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -8.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.3,
        filter: [0.637, 0.62, 0.625, 0.45], velocity: 0.5, gain: 1.0,
        lfo: [(0, 0.325), (1, 0.264)],
        env: [[0.04, 0.424, 0.8, 0.28], [0.0, 0.333, 0.3, 0.28]],
        matrix: [
            (1, 4, 0.22),   // lfo 1 → cutoff
            (2, 3, 0.12),   // lfo 2 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Pizzicato is the shortest note a string can make: a hundred and twenty
    // milliseconds, no sustain, and the resonance high enough to put a body on
    // something that has no time to have one.
    Chart {
        name: "Synth Pizz", label: "B15 SynPizz", slot: "B.15",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 6.0, level: 0.8 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.2,
        filter: [0.612, 0.55, 0.725, 0.55], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.179, 0.0, 0.158], [0.0, 0.121, 0.0, 0.108]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Everything wide and nothing dark: four saws across two octaves, the
    // corner at 3 kHz, sustain at nine tenths and a slow LFO swinging the
    // whole mix from side to side.
    Chart {
        name: "Euphoric Synth", label: "B16 Euphoric", slot: "B.16",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -11.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 6.0, level: 0.95 },
            OscChart { shape: 0, table: 0.0, semitones: 12, cents: -5.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.45], pulse_width: 0.0, drive: 0.3,
        filter: [0.758, 0.3, 0.625, 0.5], velocity: 0.45, gain: 1.0,
        lfo: [(0, 0.245), (1, 0.172)],
        env: [[0.077, 0.476, 0.9, 0.333], [0.04, 0.393, 0.4, 0.308]],
        matrix: [
            (1, 7, 0.3),    // lfo 1 → vector x
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The flash is a square LFO at 4.6 Hz on the amplitude, which on this
    // instrument can only take level away — so the pad is at full level
    // between the flashes rather than pumped up by them.
    Chart {
        name: "Flashin' Pad", label: "B17 FlashPad", slot: "B.17",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.3333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 8.0, level: 0.85 },
            OscChart { shape: 4, table: 0.4, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: -8.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.15,
        filter: [0.667, 0.35, 0.625, 0.3], velocity: 0.4, gain: 1.0,
        lfo: [(3, 0.707), (0, 0.209)],
        env: [[0.308, 0.498, 0.85, 0.393], [0.247, 0.424, 0.4, 0.355]],
        matrix: [
            (1, 6, 0.45),   // lfo 1 → amplitude
            (2, 7, 0.25),   // lfo 2 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Nothing in this patch stops: two slow triangles on the vector and a
    // third of a table of drift on the wave position, with a second and a half
    // of release so notes overlap into each other.
    Chart {
        name: "Stream Pad", label: "B18 StreamPd", slot: "B.18",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 4, table: 0.3333, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: -7.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.1,
        filter: [0.625, 0.35, 0.6, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.149), (1, 0.092)],
        env: [[0.551, 0.605, 0.9, 0.593], [0.517, 0.551, 0.6, 0.517]],
        matrix: [
            (1, 7, 0.3),    // lfo 1 → vector x
            (2, 8, 0.3),    // lfo 2 → vector y
            (1, 3, 0.08),   // lfo 1 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The sample-and-hold as the part rather than as an effect: a 9.2 Hz hold
    // — a sixteenth of 138 — at nearly half depth on the pitch, which is a
    // random note per step and is what the name means.
    Chart {
        name: "S&H Signal", label: "B21 S&H Sig", slot: "B.21",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 8.0, level: 0.8 },
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.35, drive: 0.35,
        filter: [0.699, 0.55, 0.65, 0.4], velocity: 0.55, gain: 1.0,
        lfo: [(4, 0.815), (0, 0.325)],
        env: [[0.005, 0.355, 0.6, 0.207], [0.0, 0.247, 0.2, 0.207]],
        matrix: [
            (1, 1, 0.4),    // lfo 1 → pitch
            (2, 4, 0.15),   // lfo 2 → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The stab sequence at a sixteenth of 124 with the drive at 0.8 and a
    // sample-and-hold on the corner at a quarter of the step rate, so the
    // filter changes on some steps and not others.
    Chart {
        name: "Dirty Motion", label: "B22 DirtyMot", slot: "B.22",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -9.0, level: 0.9 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.3 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
        ],
        seq: [Some(7), Some(7), Some(0), Some(7)], seq_rate: seq_rate_at(5.05),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.4, drive: 0.8,
        filter: [0.583, 0.65, 0.675, 0.35], velocity: 0.6, gain: 1.0,
        lfo: [(4, 0.584), (0, 0.36)],
        env: [[0.01, 0.393, 0.65, 0.247], [0.0, 0.28, 0.15, 0.247]],
        matrix: [
            (1, 4, 0.2),    // lfo 1 → cutoff
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Amplitude modulation gated at a sixteenth of 140, with a fifty-
    // millisecond filter envelope on every step: the short in the name is the
    // gate's step and the corner shutting behind it, not the amplifier, which
    // has to hold for the pattern to be heard.
    Chart {
        name: "Short Ring Perc.", label: "B23 ShrtRing", slot: "B.23",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 6.0, level: 0.7 },
            OscChart { shape: 2, table: 0.0, semitones: 12, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: 17, cents: 0.0, level: 1.0 },
        ],
        seq: [Some(0), Some(0), None, None], seq_rate: seq_rate_at(5.22),
        d_mode: 1, vector: [0.45, 0.25], pulse_width: 0.0, drive: 0.35,
        filter: [0.725, 0.45, 0.7, 0.55], velocity: 0.75, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.28, 0.8, 0.134], [0.0, 0.093, 0.0, 0.093]],
        matrix: [
            (10, 6, 0.6),   // oscillator D → amplitude
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The drawbar table at 16' with a sine under it and no envelope on either:
    // an organ pedal, which is a bass that starts and stops rather than one
    // that decays.
    Chart {
        name: "Organ Bass", label: "B24 OrganBas", slot: "B.24",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.4, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.3333, semitones: -12, cents: 4.0, level: 0.85 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.3333, semitones: 0, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.35,
        filter: [0.525, 0.3, 0.575, 0.25], velocity: 0.5, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.014, 0.355, 0.95, 0.134], [0.0, 0.247, 0.5, 0.134]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Four squares — the pulse knob at the bottom of its travel, where the
    // duty is exactly a half — detuned eight cents apart. Odd harmonics only,
    // four times over.
    Chart {
        name: "Unison SQU Bass", label: "B25 UniSQU", slot: "B.25",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -8.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -3.0, level: 0.95 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 4.0, level: 0.95 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.35], pulse_width: 0.0, drive: 0.45,
        filter: [0.442, 0.5, 0.7, 0.35], velocity: 0.75, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.393, 0.45, 0.179], [0.0, 0.28, 0.0, 0.179]],
        matrix: [
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Twenty-two cents between the two saws, which is past chorus and into a
    // beat you can count: about three a second at the bottom of the keyboard.
    Chart {
        name: "Detune Bass", label: "B26 DetunBas", slot: "B.26",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -11.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 11.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.35], pulse_width: 0.0, drive: 0.4,
        filter: [0.466, 0.45, 0.675, 0.3], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.409, 0.45, 0.198], [0.0, 0.28, 0.0, 0.198]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // A hundred and eighty milliseconds and no sustain: the bass that leaves
    // room for the kick rather than fighting it.
    Chart {
        name: "Short Synth Bass", label: "B27 ShrtBass", slot: "B.27",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 5.0, level: 0.8 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 0.4 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.25, drive: 0.4,
        filter: [0.483, 0.55, 0.725, 0.35], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.232, 0.0, 0.158], [0.0, 0.158, 0.0, 0.134]],
        matrix: [
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A stab is a chord with an envelope on it: a fifth and an octave on the
    // second pair, three tenths of a second, and the drive at half so the
    // attack has some grit on it.
    Chart {
        name: "NRG Stab", label: "B28 NRG Stab", slot: "B.28",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 9.0, level: 0.95 },
            OscChart { shape: 0, table: 0.0, semitones: 7, cents: 0.0, level: 0.85 },
            OscChart { shape: 1, table: 0.0, semitones: 12, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.3, drive: 0.5,
        filter: [0.684, 0.45, 0.7, 0.4], velocity: 0.65, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.308, 0.25, 0.207], [0.0, 0.232, 0.0, 0.179]],
        matrix: [
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Two noise sources and two saws, all four gated together at a sixteenth
    // of 140 with envelope 2 opening the corner behind the note: a rhythm made
    // mostly of the one thing in the oscillator bank that has no pitch. All
    // four, because an oscillator left out of the sequence fills in the rests
    // and there is no blast left.
    Chart {
        name: "Noize Blasts", label: "B31 NoizBlst", slot: "B.31",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 0.7 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
        ],
        seq: [Some(0), Some(0), Some(0), Some(0)], seq_rate: seq_rate_at(5.22),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.55,
        filter: [0.625, 0.6, 0.775, 0.25], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.355, 0.5, 0.207], [0.0, 0.207, 0.0, 0.179]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The bell and the digital table at a sixteenth of 97 on the stab
    // sequence, which has rests in it — so the pattern is 8 ticks long and
    // only four of them sound.
    Chart {
        name: "Future Perc.", label: "B32 FuturPrc", slot: "B.32",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 7.0, level: 0.8 },
            OscChart { shape: 4, table: 0.9333, semitones: 12, cents: 0.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.5 },
        ],
        seq: [Some(7), None, None, None], seq_rate: seq_rate_at(4.69),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.3,
        filter: [0.748, 0.45, 0.7, 0.55], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.28, 0.8, 0.158], [0.0, 0.134, 0.0, 0.121]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // A pad with one oscillator gated under it at a sixteenth of 130: the
    // other three hold and the fourth pulses, which is a rhythm inside a chord
    // rather than a chord being chopped.
    Chart {
        name: "Rhythmic Pad", label: "B33 RhythmPd", slot: "B.33",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.3333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 7.0, level: 0.85 },
            OscChart { shape: 4, table: 0.4667, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: -6.0, level: 0.8 },
        ],
        seq: [None, None, None, Some(0)], seq_rate: seq_rate_at(5.12),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.2,
        filter: [0.647, 0.4, 0.65, 0.35], velocity: 0.45, gain: 1.0,
        lfo: [(0, 0.224), (1, 0.161)],
        env: [[0.355, 0.517, 0.85, 0.393], [0.247, 0.452, 0.4, 0.355]],
        matrix: [
            (1, 7, 0.25),   // lfo 1 → vector x
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The pump is a 2.7 Hz triangle on the amplitude — a reed organ's bellows,
    // which is where the name comes from — over two drawbar registrations with
    // no envelope.
    Chart {
        name: "Pump Organ", label: "B34 PumpOrgn", slot: "B.34",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.4, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.3333, semitones: 0, cents: 5.0, level: 0.85 },
            OscChart { shape: 4, table: 0.4667, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 0.5333, semitones: 12, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.25,
        filter: [0.647, 0.3, 0.575, 0.3], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.624), (1, 0.28)],
        env: [[0.108, 0.393, 0.95, 0.207], [0.0, 0.308, 0.5, 0.207]],
        matrix: [
            (1, 6, 0.3),    // lfo 1 → amplitude
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Two slow modulations of pitch that never agree: a 0.3 Hz triangle at six
    // cents, and envelope 2 pulling the note a quarter tone flat and letting
    // it back over a second and a half.
    Chart {
        name: "Lazy Pitch", label: "B35 LazyPtch", slot: "B.35",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 6.0, level: 0.85 },
            OscChart { shape: 2, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: -7.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.2,
        filter: [0.625, 0.4, 0.65, 0.4], velocity: 0.45, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.2)],
        env: [[0.158, 0.498, 0.8, 0.355], [0.0, 0.593, 0.0, 0.393]],
        matrix: [
            (1, 1, 0.06),   // lfo 1 → pitch
            (4, 1, -0.04),  // env 2 → pitch
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A band-pass out of a low-pass, the way the hi-hats in the kit are made:
    // resonance at 0.85 takes the bottom off, the corner at 900 Hz takes the
    // top, and what is left is a band. Fourths on the oscillators, as the name
    // asks.
    Chart {
        name: "BPF 4th Pad", label: "B36 BPF 4th", slot: "B.36",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 5, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.4667, semitones: 10, cents: 0.0, level: 0.8 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.25,
        filter: [0.583, 0.85, 0.65, 0.35], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.217), (1, 0.149)],
        env: [[0.424, 0.535, 0.85, 0.424], [0.393, 0.498, 0.5, 0.393]],
        matrix: [
            (1, 4, 0.15),   // lfo 1 → cutoff
            (2, 7, 0.25),   // lfo 2 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The four digital tables with the vector under two LFOs and envelope 2 on
    // the wave position as well, so the timbre moves on two clocks: one that
    // repeats and one that only happens once per note.
    Chart {
        name: "Future Pad", label: "B37 FuturPad", slot: "B.37",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 6.0, level: 0.85 },
            OscChart { shape: 4, table: 0.8667, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: -6.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.15,
        filter: [0.675, 0.35, 0.65, 0.3], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.182), (1, 0.123)],
        env: [[0.476, 0.566, 0.85, 0.452], [0.424, 0.517, 0.4, 0.424]],
        matrix: [
            (1, 7, 0.3),    // lfo 1 → vector x
            (2, 8, 0.25),   // lfo 2 → vector y
            (4, 3, 0.12),   // env 2 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The darkest pad in the bank: the corner at 420 Hz, the vowel tables
    // underneath, and envelope 2 taking it *down* rather than up — a negative
    // filter envelope, which is the setting most patches never use.
    Chart {
        name: "Shadow Pad", label: "B38 ShadowPd", slot: "B.38",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: -7.0, level: 0.9 },
            OscChart { shape: 4, table: 0.4667, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 8.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.15,
        filter: [0.473, 0.45, 0.375, 0.3], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.172), (1, 0.108)],
        env: [[0.517, 0.58, 0.85, 0.476], [0.424, 0.551, 0.3, 0.424]],
        matrix: [
            (1, 7, 0.25),   // lfo 1 → vector x
            (2, 3, 0.06),   // lfo 2 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A.23's cross modulation moved down two octaves and taken off the
    // percussion envelope: oscillator D a fourth above the note, on pitch at a
    // quarter depth, which at bass frequencies reads as a growl rather than as
    // a bell.
    Chart {
        name: "X-Mod Bass", label: "B41 XModBass", slot: "B.41",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.95 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -6.0, level: 0.8 },
            OscChart { shape: 3, table: 0.0, semitones: 5, cents: 0.0, level: 1.0 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 1, vector: [0.45, 0.25], pulse_width: 0.35, drive: 0.6,
        filter: [0.424, 0.55, 0.675, 0.3], velocity: 0.75, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.409, 0.5, 0.198], [0.0, 0.28, 0.0, 0.198]],
        matrix: [
            (10, 1, 0.22),  // oscillator D → pitch
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The hollow table is a stopped pipe — odd harmonics falling fast — which
    // is exactly what an organ's bourdon is, so the pipe in the name is a
    // waveform choice rather than a filter setting.
    Chart {
        name: "Pipe Bass", label: "B42 PipeBass", slot: "B.42",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.4667, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 6.0, level: 0.7 },
            OscChart { shape: 2, table: 0.0, semitones: -12, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.25,
        filter: [0.498, 0.35, 0.65, 0.3], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.032, 0.424, 0.7, 0.207], [0.0, 0.308, 0.2, 0.207]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // A bass with a swell on it: three tenths of a second of attack on both
    // envelopes, which is long enough to hear as backwards and short enough to
    // still play in time.
    Chart {
        name: "Reverse Bass", label: "B43 RevBass", slot: "B.43",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -8.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.45,
        filter: [0.451, 0.55, 0.75, 0.3], velocity: 0.5, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.424, 0.424, 0.8, 0.108], [0.424, 0.393, 0.5, 0.108]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Both substitutions on one patch, which is what the name asks for:
    // amplitude modulation from oscillator D for the ring, and envelope 2
    // walking the wavetable position for the sync.
    Chart {
        name: "RingSync Bass", label: "B44 RingSync", slot: "B.44",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 7.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 1.0 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 1, vector: [0.45, 0.25], pulse_width: 0.0, drive: 0.55,
        filter: [0.473, 0.6, 0.675, 0.3], velocity: 0.75, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.393, 0.45, 0.179], [0.0, 0.26, 0.0, 0.179]],
        matrix: [
            (10, 6, 0.4),   // oscillator D → amplitude
            (4, 3, 0.3),    // env 2 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The high-pass sweep, again out of the ladder's bass loss: resonance at
    // 0.88 and envelope 2 taking the corner up a decade and a half over half a
    // second, so the bottom disappears before the top does.
    Chart {
        name: "HPF Sweep Bass", label: "B45 HPFSweep", slot: "B.45",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.25, drive: 0.45,
        filter: [0.379, 0.88, 0.775, 0.3], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.424, 0.55, 0.207], [0.0, 0.393, 0.0, 0.28]],
        matrix: [
            (7, 4, 0.3),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The drop is envelope 2 at a third of its depth on pitch, negative, over
    // a second — so the note starts where it was played and slides a fourth
    // below it while it decays.
    Chart {
        name: "Nu Skool Drop", label: "B46 NuSkool", slot: "B.46",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -9.0, level: 0.85 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.4, drive: 0.7,
        filter: [0.442, 0.6, 0.65, 0.3], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.014, 0.498, 0.6, 0.247], [0.0, 0.517, 0.0, 0.308]],
        matrix: [
            (4, 1, -0.35),  // env 2 → pitch
            (5, 4, 0.2),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Every kind of modulation the panel has, at once and at moderate depth:
    // an LFO on the width, a second on the wavetable position, oscillator D in
    // its low range on pitch, and the wheel on the corner. The patch that is a
    // tour of the matrix.
    Chart {
        name: "Modulation Lead", label: "B47 ModLead", slot: "B.47",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 7.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 3, table: 0.0, semitones: 8, cents: 0.0, level: 1.0 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 2, vector: [0.45, 0.25], pulse_width: 0.3, drive: 0.35,
        filter: [0.657, 0.5, 0.65, 0.45], velocity: 0.55, gain: 1.0,
        lfo: [(0, 0.433), (4, 0.65)],
        env: [[0.032, 0.424, 0.75, 0.28], [0.0, 0.333, 0.25, 0.28]],
        matrix: [
            (1, 2, 0.3),    // lfo 1 → pulse width
            (2, 3, 0.1),    // lfo 2 → wavetable
            (10, 1, 0.03),  // oscillator D → pitch
            (7, 4, 0.35),   // wheel → cutoff
            NO_ROUTE, NO_ROUTE,
        ],
    },
    // Noise and saw through the drive at 0.9, gated at a sixteenth of 136,
    // with a sample-and-hold on the corner: a storm is a rhythm you cannot
    // quite hear the edges of.
    Chart {
        name: "Grimey Storm", label: "B48 GrimStrm", slot: "B.48",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.55 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -11.0, level: 0.8 },
        ],
        seq: [Some(0), Some(0), Some(0), Some(0)], seq_rate: seq_rate_at(5.18),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.45, drive: 0.9,
        filter: [0.566, 0.65, 0.7, 0.3], velocity: 0.6, gain: 1.0,
        lfo: [(4, 0.732), (0, 0.325)],
        env: [[0.01, 0.409, 0.65, 0.247], [0.0, 0.308, 0.2, 0.247]],
        matrix: [
            (1, 4, 0.2),    // lfo 1 → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The corner at 190 Hz, which on a bass note is under the third harmonic:
    // everything above the fundamental and its octave is gone before the
    // amplifier sees it.
    Chart {
        name: "Dark Bass", label: "B51 DarkBass", slot: "B.51",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.95 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -6.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.0, drive: 0.3,
        filter: [0.358, 0.4, 0.65, 0.25], velocity: 0.65, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.014, 0.424, 0.55, 0.207], [0.0, 0.308, 0.0, 0.207]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The other Moog bass: pulse rather than saw on the front pair, more
    // resonance, and a faster envelope on both the amplifier and the filter —
    // the setting that reads as funk where A.52 reads as dub.
    Chart {
        name: "MG Bass 2", label: "B52 MG Bass2", slot: "B.52",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -6.0, level: 0.95 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.35, drive: 0.5,
        filter: [0.424, 0.62, 0.75, 0.35], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.333, 0.3, 0.158], [0.0, 0.232, 0.0, 0.158]],
        matrix: [
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Two sines an octave apart and nothing else in the mix. The corner is at
    // 160 Hz not because there is anything above it to remove but because the
    // ladder's bass loss is part of the sound at any setting.
    Chart {
        name: "Sub Bass", label: "B53 Sub Bass", slot: "B.53",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.9 },
            OscChart { shape: 2, table: 0.0, semitones: -12, cents: 5.0, level: 0.4 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 0.3 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.3], pulse_width: 0.0, drive: 0.3,
        filter: [0.333, 0.25, 0.625, 0.2], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.023, 0.452, 0.75, 0.232], [0.0, 0.333, 0.2, 0.232]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Velocity three quarters of the way onto the corner and keyboard follow
    // at 0.65: the whole patch is about how hard and how high it is played,
    // which is what a funk lead is for.
    Chart {
        name: "70's Funk Lead", label: "B54 70s Funk", slot: "B.54",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.3, drive: 0.45,
        filter: [0.547, 0.65, 0.675, 0.65], velocity: 0.7, gain: 1.0,
        lfo: [(1, 0.738), (0, 0.28)],
        env: [[0.014, 0.374, 0.65, 0.232], [0.0, 0.28, 0.1, 0.207]],
        matrix: [
            (5, 4, 0.45),   // velocity → cutoff
            (1, 1, 0.025),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The drawbar table with the drive at 0.75, which is the whole difference
    // between this and A.56: an organ through an amplifier that is being asked
    // for more than it has.
    Chart {
        name: "Rock Organ", label: "B55 RockOrgn", slot: "B.55",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.4, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.3333, semitones: 12, cents: 0.0, level: 0.75 },
            OscChart { shape: 4, table: 0.4, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.75,
        filter: [0.699, 0.25, 0.55, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(1, 0.768), (0, 0.217)],
        env: [[0.014, 0.308, 1.0, 0.093], [0.0, 0.158, 0.5, 0.093]],
        matrix: [
            (7, 4, 0.2),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The percussion drawbar: a twelfth above the note with a hundred-and-
    // fifty-millisecond decay of its own, which the vector does rather than a
    // second envelope — envelope 2 pulls the mix towards the organ and lets it
    // fall back to the percussion.
    Chart {
        name: "Perc. Organ", label: "B56 PercOrgn", slot: "B.56",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.3333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.4, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.3, 0.3], pulse_width: 0.0, drive: 0.35,
        filter: [0.713, 0.25, 0.575, 0.3], velocity: 0.55, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.333, 0.95, 0.108], [0.0, 0.207, 0.0, 0.108]],
        matrix: [
            (4, 7, 0.45),   // env 2 → vector x
            (4, 8, 0.35),   // env 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // B.14's pair of slow LFOs on a clavinet instead of a lead, which is where
    // a phaser was actually used: the sweep is audible on a sound that has
    // harmonics all the way up for it to move through.
    Chart {
        name: "Phaser Clav", label: "B57 PhasClav", slot: "B.57",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.35,
        filter: [0.612, 0.6, 0.65, 0.65], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.343), (1, 0.285)],
        env: [[0.0, 0.409, 0.25, 0.207], [0.0, 0.28, 0.0, 0.179]],
        matrix: [
            (1, 4, 0.2),    // lfo 1 → cutoff
            (2, 3, 0.1),    // lfo 2 → wavetable
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // What a string machine is: one waveform per note, divided down from one
    // oscillator, with an ensemble over the top. Here that is four saws across
    // three registers and a pair of slow LFOs on pitch at different rates.
    Chart {
        name: "String Machine", label: "B58 StrgMach", slot: "B.58",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -9.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 7.0, level: 0.95 },
            OscChart { shape: 0, table: 0.0, semitones: 12, cents: -4.0, level: 0.8 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 10.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.45], pulse_width: 0.0, drive: 0.12,
        filter: [0.637, 0.3, 0.6, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(1, 0.333), (0, 0.396)],
        env: [[0.424, 0.566, 0.9, 0.452], [0.393, 0.517, 0.5, 0.424]],
        matrix: [
            (1, 1, 0.012),  // lfo 1 → pitch
            (2, 1, 0.008),  // lfo 2 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The bell run sequence, played once at a sixteenth of 139 and then held:
    // four bells falling two octaves into an organ that stays. The attack of
    // an instrument that does not exist, which is the Wavestation's whole
    // argument.
    Chart {
        name: "Analog Bell", label: "B61 AnlogBel", slot: "B.61",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 4.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.4 },
            OscChart { shape: 4, table: 0.8667, semitones: -12, cents: 0.0, level: 0.6 },
        ],
        seq: [Some(5), None, None, None], seq_rate: seq_rate_at(5.21),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.2,
        filter: [0.737, 0.3, 0.675, 0.55], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.517, 0.2, 0.355], [0.0, 0.308, 0.0, 0.28]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The stairs are the riff sequence's pitch column — root, fifth, octave,
    // minor third — at an eighth of 140 under a pad envelope, so the steps
    // arrive slowly enough to be heard as a melody rather than as an arpeggio.
    Chart {
        name: "Stairs Pad", label: "B62 StairsPd", slot: "B.62",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.3333, semitones: 0, cents: 6.0, level: 0.85 },
            OscChart { shape: 4, table: 0.4667, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -7.0, level: 0.7 },
        ],
        seq: [Some(3), None, None, None], seq_rate: seq_rate_at(4.22),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.2,
        filter: [0.667, 0.4, 0.65, 0.35], velocity: 0.45, gain: 1.0,
        lfo: [(0, 0.2), (1, 0.137)],
        env: [[0.355, 0.535, 0.85, 0.409], [0.308, 0.476, 0.45, 0.374]],
        matrix: [
            (1, 7, 0.25),   // lfo 1 → vector x
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Two triangles and a sine, the corner wide open and the resonance at
    // nothing: a triangle's harmonics fall as the square of their number, so
    // there is almost nothing for a filter to do.
    Chart {
        name: "Triangle Lead", label: "B63 TriLead", slot: "B.63",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 2, table: 0.0, semitones: 12, cents: -5.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.25,
        filter: [0.799, 0.2, 0.575, 0.5], velocity: 0.5, gain: 1.0,
        lfo: [(1, 0.726), (0, 0.28)],
        env: [[0.04, 0.409, 0.85, 0.28], [0.0, 0.308, 0.3, 0.28]],
        matrix: [
            (1, 1, 0.03),   // lfo 1 → pitch
            (7, 4, 0.25),   // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A comping part that is different every bar: the gate at a sixteenth of
    // 144 for the rhythm, and a sample-and-hold on the wavetable position at a
    // fifth of that rate for the timbre.
    Chart {
        name: "Random Comp", label: "B64 RandComp", slot: "B.64",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 7.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 0.6 },
        ],
        seq: [Some(0), None, None, None], seq_rate: seq_rate_at(5.26),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.35,
        filter: [0.667, 0.5, 0.675, 0.45], velocity: 0.65, gain: 1.0,
        lfo: [(4, 0.569), (0, 0.325)],
        env: [[0.005, 0.355, 0.55, 0.207], [0.0, 0.247, 0.1, 0.179]],
        matrix: [
            (1, 3, 0.14),   // lfo 1 → wavetable
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Saws, a quarter-second decay and no sustain at all: the shortest chord
    // in the bank that is still a chord rather than a hit.
    Chart {
        name: "Stab Saw", label: "B65 StabSaw", slot: "B.65",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 8.0, level: 0.95 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -8.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.4,
        filter: [0.657, 0.45, 0.725, 0.4], velocity: 0.65, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.28, 0.0, 0.198], [0.0, 0.207, 0.0, 0.158]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // B.65's job on squares and with the gate under it: odd harmonics only,
    // chopped at a sixteenth of 144, which is the hollow comping sound the
    // pulse wave is for.
    Chart {
        name: "Square Comp", label: "B66 SquComp", slot: "B.66",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 2, table: 0.0, semitones: 12, cents: 0.0, level: 0.5 },
        ],
        seq: [Some(0), Some(0), None, None], seq_rate: seq_rate_at(5.26),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.1, drive: 0.35,
        filter: [0.647, 0.45, 0.675, 0.45], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.333, 0.45, 0.198], [0.0, 0.232, 0.1, 0.179]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Twenty-eight cents between the outer pair, which is nearly a quarter
    // tone: past chorus, past beating, and into a chord that is audibly out of
    // tune with itself. The 178 the factory list gives it is fast enough that
    // nobody has time to mind.
    Chart {
        name: "Detuned Comp", label: "B67 DetComp", slot: "B.67",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -14.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 14.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 9.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -4.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.45], pulse_width: 0.3, drive: 0.4,
        filter: [0.637, 0.45, 0.675, 0.4], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.355, 0.5, 0.216], [0.0, 0.247, 0.15, 0.198]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The corner at 800 Hz and the attack at four tenths: strings from a
    // machine that could not do better, which is a different sound from
    // strings from one that chose not to.
    Chart {
        name: "Old Strings", label: "B68 OldStrgs", slot: "B.68",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -10.0, level: 1.0 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 5.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 11.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.45], pulse_width: 0.0, drive: 0.15,
        filter: [0.566, 0.35, 0.6, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(1, 0.317), (0, 0.245)],
        env: [[0.476, 0.58, 0.9, 0.476], [0.452, 0.535, 0.5, 0.452]],
        matrix: [
            (1, 1, 0.014),  // lfo 1 → pitch
            (2, 4, 0.1),    // lfo 2 → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A four-second release and a sample-and-hold at 0.9 Hz on both pitch and
    // the wavetable position: notes that go on changing long after the key is
    // up.
    Chart {
        name: "Time Zone SFX", label: "B71 TimeZone", slot: "B.71",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: 7, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.25,
        filter: [0.699, 0.55, 0.65, 0.4], velocity: 0.4, gain: 1.0,
        lfo: [(4, 0.452), (0, 0.172)],
        env: [[0.308, 0.648, 0.6, 0.783], [0.247, 0.593, 0.4, 0.648]],
        matrix: [
            (1, 1, 0.15),   // lfo 1 → pitch
            (1, 3, 0.15),   // lfo 1 → wavetable
            (2, 7, 0.3),    // lfo 2 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The drive at the top, cross modulation from oscillator D, and a third of
    // an oscillator of noise: three ways of adding harmonics that were not in
    // the waveform, all at once.
    Chart {
        name: "Domin8or", label: "B72 Domin8or", slot: "B.72",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -10.0, level: 0.9 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.35 },
            OscChart { shape: 3, table: 0.0, semitones: 13, cents: 0.0, level: 1.0 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 1, vector: [0.45, 0.25], pulse_width: 0.35, drive: 1.0,
        filter: [0.583, 0.6, 0.675, 0.35], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.424, 0.7, 0.247], [0.0, 0.308, 0.2, 0.247]],
        matrix: [
            (10, 1, 0.2),   // oscillator D → pitch
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Noise with the corner at 120 Hz and a 0.35 Hz triangle rolling it either
    // side of that — the factory list gives this program 34 BPM, which is the
    // slowest tempo in the whole bank and is why.
    Chart {
        name: "Thunder", label: "B73 Thunder", slot: "B.73",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
            OscChart { shape: 2, table: 0.0, semitones: -24, cents: 7.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.6,
        filter: [0.292, 0.55, 0.675, 0.1], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.304), (1, 0.123)],
        env: [[0.424, 0.691, 0.6, 0.648], [0.517, 0.648, 0.3, 0.593]],
        matrix: [
            (1, 4, 0.3),    // lfo 1 → cutoff
            (2, 7, 0.25),   // lfo 2 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Envelope 2 on pitch, positive and deep, with a slow attack and a long
    // decay: the note bends up into a wail and falls back, with the resonance
    // high enough to make a formant of it.
    Chart {
        name: "Cry", label: "B74 Cry", slot: "B.74",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 7.0, level: 0.85 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 0.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.35,
        filter: [0.583, 0.75, 0.65, 0.45], velocity: 0.45, gain: 1.0,
        lfo: [(1, 0.732), (0, 0.28)],
        env: [[0.216, 0.551, 0.7, 0.393], [0.355, 0.551, 0.2, 0.393]],
        matrix: [
            (4, 1, 0.25),   // env 2 → pitch
            (1, 1, 0.03),   // lfo 1 → pitch
            (4, 4, 0.2),    // env 2 → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The minor seventh of A.78 with the resonance at 0.86 and the corner at
    // 1.1 kHz: the same chord with its bottom removed, which is what a hit
    // needs when there is already a bass part.
    Chart {
        name: "HPF m7 Chord", label: "B75 HPF m7", slot: "B.75",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 3, cents: 0.0, level: 0.95 },
            OscChart { shape: 0, table: 0.0, semitones: 7, cents: 0.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 10, cents: 0.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.35,
        filter: [0.612, 0.86, 0.65, 0.35], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.014, 0.424, 0.3, 0.28], [0.0, 0.308, 0.0, 0.247]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Major thirds only — 0, 4, 12 and 16 — which is a chord with no fifth in
    // it and reads as bright rather than as major.
    Chart {
        name: "M3rd Chord", label: "B76 M3rdChrd", slot: "B.76",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 4, cents: 0.0, level: 0.95 },
            OscChart { shape: 4, table: 0.6, semitones: 12, cents: 0.0, level: 0.8 },
            OscChart { shape: 0, table: 0.0, semitones: 16, cents: 0.0, level: 0.75 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.4,
        filter: [0.667, 0.4, 0.7, 0.35], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.452, 0.3, 0.297], [0.0, 0.308, 0.0, 0.26]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Noise, saw and the drive at the top with a two-hundred-millisecond
    // envelope: the loudest thing in the bank for the shortest time.
    Chart {
        name: "Hardcore Hit", label: "B77 HardcorH", slot: "B.77",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -12.0, level: 0.9 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.3, drive: 1.0,
        filter: [0.647, 0.55, 0.75, 0.25], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.247, 0.0, 0.158], [0.0, 0.134, 0.0, 0.108]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // A major seventh — 0, 4, 7 and 11 — with a second of decay and the corner
    // well up, so the semitone between the seventh and the octave is audible
    // rather than buried.
    Chart {
        name: "Artcore M7 Chord", label: "B78 ArtcorM7", slot: "B.78",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 4, cents: 0.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 7, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.6, semitones: 11, cents: 0.0, level: 0.75 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.3,
        filter: [0.699, 0.35, 0.675, 0.4], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.019, 0.517, 0.4, 0.333], [0.0, 0.374, 0.1, 0.308]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // A.81 at pitch rather than an octave down, with the corner up where a
    // female first formant sits. Same table, different register, which is what
    // the difference is on the hardware too.
    Chart {
        name: "Female-Ahhh", label: "B81 FemAhhh", slot: "B.81",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: -6.0, level: 0.75 },
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.12,
        filter: [0.637, 0.45, 0.575, 0.3], velocity: 0.4, gain: 1.0,
        lfo: [(1, 0.732), (0, 0.28)],
        env: [[0.158, 0.476, 0.9, 0.28], [0.158, 0.393, 0.5, 0.28]],
        matrix: [
            (1, 1, 0.012),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // An octave above the male programs with the corner at 2.6 kHz and the
    // table pushed towards the bell: a small vocal tract has its formants
    // further up, and the bell's partials are the only thing in the wave bank
    // that sits that high.
    Chart {
        name: "Kid-Eeey", label: "B82 Kid-Eeey", slot: "B.82",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: 12, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.74, semitones: 12, cents: 6.0, level: 0.85 },
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 0.0, level: 0.45 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.12,
        filter: [0.737, 0.5, 0.575, 0.35], velocity: 0.4, gain: 1.0,
        lfo: [(1, 0.743), (0, 0.28)],
        env: [[0.134, 0.452, 0.9, 0.26], [0.134, 0.374, 0.5, 0.26]],
        matrix: [
            (1, 1, 0.014),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The wow is a 1.4 Hz triangle on the corner, deep enough to be heard as a
    // vowel changing rather than as a tremolo — which is what a wah is, and
    // what the bracket in the factory name is telling you.
    Chart {
        name: "Kid-Ahhh (Wow)", label: "B83 KidAhWow", slot: "B.83",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: 12, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.7333, semitones: 12, cents: -7.0, level: 0.85 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.15,
        filter: [0.647, 0.62, 0.6, 0.35], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.521), (1, 0.28)],
        env: [[0.134, 0.476, 0.9, 0.28], [0.158, 0.393, 0.5, 0.28]],
        matrix: [
            (1, 4, 0.3),    // lfo 1 → cutoff
            (1, 3, 0.08),   // lfo 1 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A pulse carrier rather than a vocal one, which is what most vocoder
    // programs actually use: the width knob near its narrowest, where a pulse
    // has the flattest spectrum and therefore the most for a filter bank to
    // find.
    Chart {
        name: "Vocoder Pulse", label: "B84 VocPulse", slot: "B.84",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 0.0, level: 0.6 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.7, drive: 0.2,
        filter: [0.667, 0.4, 0.575, 0.35], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.36), (1, 0.28)],
        env: [[0.077, 0.452, 0.9, 0.247], [0.0, 0.374, 0.5, 0.247]],
        matrix: [
            (1, 2, 0.15),   // lfo 1 → pulse width
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The same idea at exactly half duty, where the even harmonics vanish: a
    // squarer carrier, which reads as hollow where B.84 reads as buzzy.
    Chart {
        name: "Vocoder SQU", label: "B85 Voc SQU", slot: "B.85",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 0.0, level: 0.6 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.2,
        filter: [0.657, 0.4, 0.575, 0.35], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.36), (1, 0.28)],
        env: [[0.077, 0.452, 0.9, 0.247], [0.0, 0.374, 0.5, 0.247]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The factory list gives this one 200 BPM, and 3.3 Hz is a quarter note at
    // that tempo — so the wah is on the beat rather than at whatever rate
    // sounded good. A triangle on the corner at 0.35 depth.
    Chart {
        name: "Vocoder Wah", label: "B86 Voc Wah", slot: "B.86",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 6.0, level: 0.85 },
            OscChart { shape: 4, table: 0.7333, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -8.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.4, drive: 0.25,
        filter: [0.599, 0.7, 0.6, 0.35], velocity: 0.45, gain: 1.0,
        lfo: [(0, 0.655), (1, 0.325)],
        env: [[0.04, 0.452, 0.9, 0.247], [0.0, 0.355, 0.4, 0.247]],
        matrix: [
            (1, 4, 0.35),   // lfo 1 → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The vowel sequence — ah, oo, hollow, reed on unequal steps — is the
    // closest thing in the instrument to a vocoder's formants moving, so this
    // program is that step list on two oscillators at a quarter of 120.
    Chart {
        name: "Vocoder Vox Wave", label: "B87 VocVoxWv", slot: "B.87",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: -7.0, level: 0.8 },
        ],
        seq: [Some(2), None, None, Some(2)], seq_rate: seq_rate_at(3.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.2,
        filter: [0.657, 0.45, 0.6, 0.3], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.325), (1, 0.252)],
        env: [[0.108, 0.498, 0.9, 0.308], [0.0, 0.424, 0.5, 0.308]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // DWGS is Korg's Digital Waveform Generator System — the additive single-
    // cycle bank on the DW-8000, which is the same idea as this instrument's
    // sixteen tables and the reason they are here. So this program is the
    // digital end of the bank as the carrier: clav, digital and bell, with the
    // morph sequence walking between them.
    Chart {
        name: "Vocoder DWGS", label: "B88 Voc DWGS", slot: "B.88",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 6.0, level: 0.85 },
            OscChart { shape: 4, table: 1.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 0.0, level: 0.6 },
        ],
        seq: [Some(1), None, None, None], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.25,
        filter: [0.684, 0.45, 0.6, 0.35], velocity: 0.45, gain: 1.0,
        lfo: [(0, 0.325), (1, 0.252)],
        env: [[0.077, 0.498, 0.85, 0.308], [0.0, 0.424, 0.45, 0.308]],
        matrix: [
            (1, 7, 0.2),    // lfo 1 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },

    // ── M.01 – M.40 · the Minimoog set ──
    // Authored. There was never a factory bank to transcribe, because the
    // instrument has no patch memory — so these are named for what they are:
    // ten basses, seven leads, three brass, seven winds and strings, four
    // struck sounds, five effects, three that exist to show what oscillator D
    // on the modulation bus does, and a drone.
    // Two saws at 8' beating seven cents apart and a third at 16', the ladder
    // a quarter open with envelope 2 half over it, and the mixer pushed past
    // unity into it. The setting the instrument is bought for.
    Chart {
        name: "FAT BASS", label: "M01 FAT BASS", slot: "M.01",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -7.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.95 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.55 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.0, drive: 0.5,
        filter: [0.386, 0.42, 0.725, 0.35], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.409, 0.45, 0.207], [0.0, 0.308, 0.0, 0.207]],
        matrix: [
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The bass pedal sound: one triangle at 16', one sine at 32', the corner
    // under the third harmonic and no envelope on the filter at all. Nothing
    // to hear but weight.
    Chart {
        name: "PEDAL BASS", label: "M02 PEDALBAS", slot: "M.02",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.95 },
            OscChart { shape: 2, table: 0.0, semitones: -24, cents: 5.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: -5.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.3], pulse_width: 0.0, drive: 0.35,
        filter: [0.324, 0.3, 0.55, 0.2], velocity: 0.5, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.04, 0.498, 0.85, 0.28], [0.0, 0.355, 0.3, 0.247]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The drive stage at 0.9 into a corner at 350 Hz with the resonance at two
    // thirds — the growl is the mixer clipping and the ladder then emphasising
    // exactly the band the clipping filled.
    Chart {
        name: "GROWL BASS", label: "M03 GROWLBAS", slot: "M.03",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -9.0, level: 0.95 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.35, drive: 0.9,
        filter: [0.447, 0.66, 0.675, 0.3], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.424, 0.55, 0.207], [0.0, 0.297, 0.0, 0.207]],
        matrix: [
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Three pulses at exactly half duty, where the even harmonics vanish, plus
    // a sine at 32' to put a fundamental back under them.
    Chart {
        name: "SQUARE BASS", label: "M04 SQU BASS", slot: "M.04",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.95 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.0, drive: 0.45,
        filter: [0.434, 0.45, 0.7, 0.35], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.393, 0.45, 0.189], [0.0, 0.28, 0.0, 0.189]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The bass where the envelope is on the filter rather than the amplifier:
    // sustain at four fifths, and envelope 2 sweeping two decades of corner in
    // a third of a second on every note.
    Chart {
        name: "FILTER BASS", label: "M05 FILT BAS", slot: "M.05",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 8.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.0, drive: 0.45,
        filter: [0.292, 0.7, 0.825, 0.3], velocity: 0.75, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.01, 0.498, 0.8, 0.207], [0.0, 0.333, 0.0, 0.232]],
        matrix: [
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A twenty-millisecond envelope 2 on the corner and nothing else: the
    // click is the filter opening and shutting faster than the note can
    // establish a pitch.
    Chart {
        name: "CLICK BASS", label: "M06 CLICKBAS", slot: "M.06",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -6.0, level: 0.85 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.25, drive: 0.55,
        filter: [0.373, 0.6, 0.8, 0.3], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.374, 0.35, 0.179], [0.0, 0.04, 0.0, 0.077]],
        matrix: [
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Velocity most of the way onto the corner, which is the only way a bass
    // part gets its accents from the player rather than from the sequencer.
    Chart {
        name: "FUNK BASS", label: "M07 FUNK BAS", slot: "M.07",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.55 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.3, drive: 0.5,
        filter: [0.404, 0.62, 0.675, 0.4], velocity: 0.85, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.333, 0.25, 0.158], [0.0, 0.232, 0.0, 0.158]],
        matrix: [
            (5, 4, 0.5),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Everything at 32' and 16', the corner at 130 Hz: a bass for a record
    // that has something else carrying the tune.
    Chart {
        name: "SUB BASS 32", label: "M08 SUB 32", slot: "M.08",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: -24, cents: 4.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 0, table: 0.0, semitones: -24, cents: 0.0, level: 0.4 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.3], pulse_width: 0.0, drive: 0.4,
        filter: [0.303, 0.3, 0.625, 0.2], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.023, 0.476, 0.8, 0.247], [0.0, 0.355, 0.3, 0.247]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Resonance at 0.9, which on this ladder is the last tenth before it
    // starts producing rather than losing: the peak is nearly a tone of its
    // own, and the bass loss that comes with it is most of the sound.
    Chart {
        name: "RESO BASS", label: "M09 RESO BAS", slot: "M.09",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 6.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.3], pulse_width: 0.3, drive: 0.4,
        filter: [0.414, 0.9, 0.725, 0.35], velocity: 0.75, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.005, 0.409, 0.4, 0.189], [0.0, 0.308, 0.0, 0.207]],
        matrix: [
            (7, 4, 0.3),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The corner at 110 Hz and the drive at half: a bass with no harmonics
    // left and a lot of level, which is what a sound system wants.
    Chart {
        name: "DUB BASS", label: "M10 DUB BASS", slot: "M.10",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: -6.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.3], pulse_width: 0.0, drive: 0.5,
        filter: [0.279, 0.45, 0.65, 0.2], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.032, 0.535, 0.7, 0.308], [0.0, 0.393, 0.2, 0.28]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Three oscillators at 8', 8' and 4' with the outer two detuned either way
    // — the lead that made the instrument famous, and the reason it has three
    // oscillators rather than two.
    Chart {
        name: "SOLO LEAD", label: "M11 SOLO LD", slot: "M.11",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -8.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 7.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 12, cents: 0.0, level: 0.8 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.3, drive: 0.5,
        filter: [0.657, 0.5, 0.65, 0.5], velocity: 0.5, gain: 1.0,
        lfo: [(1, 0.735), (0, 0.28)],
        env: [[0.032, 0.424, 0.8, 0.247], [0.0, 0.333, 0.25, 0.247]],
        matrix: [
            (7, 4, 0.3),    // wheel → cutoff
            (1, 1, 0.02),   // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Resonance at 0.88 and the drive at 0.8, with the wheel able to push the
    // corner two decades: the sound of the filter being asked for more than it
    // has.
    Chart {
        name: "SCREAM LEAD", label: "M12 SCREAM", slot: "M.12",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -11.0, level: 0.95 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 9.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.4, drive: 0.8,
        filter: [0.612, 0.88, 0.675, 0.5], velocity: 0.55, gain: 1.0,
        lfo: [(1, 0.754), (0, 0.28)],
        env: [[0.019, 0.393, 0.75, 0.232], [0.0, 0.308, 0.2, 0.232]],
        matrix: [
            (7, 4, 0.4),    // wheel → cutoff
            (1, 1, 0.03),   // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // There is no portamento in this instrument. What a glide up into the note
    // leaves behind, if you only hear it, is a pitch envelope: envelope 2
    // negative on pitch with a hundred-millisecond decay, which scoops every
    // note the same way rather than only the ones after another.
    Chart {
        name: "SCOOP LEAD", label: "M13 SCOOP LD", slot: "M.13",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 9.0, level: 0.95 },
            OscChart { shape: 2, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 1, table: 0.0, semitones: 12, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.35, drive: 0.45,
        filter: [0.647, 0.55, 0.65, 0.5], velocity: 0.5, gain: 1.0,
        lfo: [(1, 0.743), (0, 0.28)],
        env: [[0.023, 0.424, 0.8, 0.247], [0.0, 0.158, 0.0, 0.179]],
        matrix: [
            (4, 1, -0.12),  // env 2 → pitch
            (7, 4, 0.3),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Root, fifth and octave on three oscillators, which is the registration
    // the patch books call a horn and everyone else calls a power chord.
    Chart {
        name: "FIFTH LEAD", label: "M14 FIFTH LD", slot: "M.14",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 7, cents: 5.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 12, cents: 0.0, level: 0.8 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.3, drive: 0.55,
        filter: [0.637, 0.5, 0.65, 0.5], velocity: 0.55, gain: 1.0,
        lfo: [(1, 0.735), (0, 0.28)],
        env: [[0.023, 0.409, 0.8, 0.247], [0.0, 0.308, 0.2, 0.247]],
        matrix: [
            (7, 4, 0.3),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Triangles and a sine with the corner at 1.2 kHz and the drive off: the
    // one lead in this bank with no edge on it, which is what the instrument
    // sounds like when nothing is being pushed.
    Chart {
        name: "SOFT LEAD", label: "M15 SOFT LD", slot: "M.15",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 2, table: 0.0, semitones: 12, cents: -6.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.15,
        filter: [0.625, 0.25, 0.6, 0.5], velocity: 0.45, gain: 1.0,
        lfo: [(1, 0.72), (0, 0.28)],
        env: [[0.108, 0.452, 0.85, 0.308], [0.077, 0.355, 0.3, 0.308]],
        matrix: [
            (1, 1, 0.025),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A sine with a whisper of noise beside it and the corner just above the
    // fundamental: everything a whistle is, which is not much.
    Chart {
        name: "WHISTLE", label: "M16 WHISTLE", slot: "M.16",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.12 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 4.0, level: 0.35 },
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 0.3 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.3, 0.3], pulse_width: 0.0, drive: 0.1,
        filter: [0.657, 0.35, 0.625, 0.6], velocity: 0.45, gain: 1.0,
        lfo: [(1, 0.738), (0, 0.28)],
        env: [[0.179, 0.424, 0.9, 0.247], [0.134, 0.333, 0.5, 0.247]],
        matrix: [
            (1, 1, 0.02),   // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The brass trick is in the envelope rather than the waveform: a hundred
    // and fifty milliseconds of attack on envelope 2 so the corner arrives
    // after the note does, which is what a section sounds like tonguing
    // together.
    Chart {
        name: "BRASS SECT", label: "M17 BRASSSEC", slot: "M.17",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -9.0, level: 0.95 },
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 6.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.4,
        filter: [0.525, 0.45, 0.775, 0.4], velocity: 0.6, gain: 1.0,
        lfo: [(1, 0.714), (0, 0.28)],
        env: [[0.134, 0.476, 0.8, 0.28], [0.308, 0.424, 0.55, 0.308]],
        matrix: [
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // One player rather than a section: less detuning, a faster attack on the
    // filter and a vibrato that arrives with the note rather than after it.
    Chart {
        name: "SOLO BRASS", label: "M18 SOLOBRSS", slot: "M.18",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 5.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 0.0, level: 0.7 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.45,
        filter: [0.566, 0.5, 0.725, 0.45], velocity: 0.65, gain: 1.0,
        lfo: [(1, 0.732), (0, 0.28)],
        env: [[0.077, 0.452, 0.8, 0.26], [0.179, 0.393, 0.5, 0.28]],
        matrix: [
            (1, 1, 0.025),  // lfo 1 → pitch
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Six tenths of a second of attack on both envelopes and a three-quarter-
    // second release: the note is over before it is played, which is the only
    // way a synthesizer plays a French horn.
    Chart {
        name: "HORN SWELL", label: "M19 HORNSWEL", slot: "M.19",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: -7.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.4667, semitones: 7, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.3,
        filter: [0.498, 0.4, 0.75, 0.4], velocity: 0.45, gain: 1.0,
        lfo: [(1, 0.707), (0, 0.217)],
        env: [[0.551, 0.551, 0.85, 0.464], [0.517, 0.498, 0.6, 0.424]],
        matrix: [
            (1, 1, 0.015),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A sine, a fifth of an oscillator of noise for the breath, and a vibrato
    // that is nearly the whole character: without the noise this is a test
    // tone.
    Chart {
        name: "FLUTE", label: "M20 FLUTE", slot: "M.20",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.2 },
            OscChart { shape: 2, table: 0.0, semitones: 12, cents: 5.0, level: 0.3 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.25 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.3, 0.3], pulse_width: 0.0, drive: 0.1,
        filter: [0.637, 0.3, 0.65, 0.55], velocity: 0.5, gain: 1.0,
        lfo: [(1, 0.726), (0, 0.28)],
        env: [[0.216, 0.452, 0.9, 0.247], [0.179, 0.355, 0.6, 0.247]],
        matrix: [
            (1, 1, 0.022),  // lfo 1 → pitch
            (1, 6, 0.1),    // lfo 1 → amplitude
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The hollow table is odd harmonics falling fast, which is what a stopped
    // pipe and a clarinet have in common. The corner is low enough to keep the
    // reed out of it.
    Chart {
        name: "CLARINET", label: "M21 CLARINET", slot: "M.21",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 4.0, level: 0.6 },
            OscChart { shape: 4, table: 0.4667, semitones: -12, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 0.4 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.35], pulse_width: 0.0, drive: 0.2,
        filter: [0.583, 0.35, 0.65, 0.5], velocity: 0.55, gain: 1.0,
        lfo: [(1, 0.714), (0, 0.28)],
        env: [[0.158, 0.424, 0.9, 0.207], [0.108, 0.333, 0.6, 0.207]],
        matrix: [
            (1, 1, 0.012),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The reed table — odd harmonics that barely fall at all — through a
    // corner at 1.6 kHz with a little resonance on it, which puts the formant
    // an oboe has around its third harmonic.
    Chart {
        name: "OBOE", label: "M22 OBOE", slot: "M.22",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 5.0, level: 0.6 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 0.4 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.35], pulse_width: 0.0, drive: 0.25,
        filter: [0.667, 0.55, 0.65, 0.5], velocity: 0.55, gain: 1.0,
        lfo: [(1, 0.726), (0, 0.28)],
        env: [[0.134, 0.409, 0.9, 0.207], [0.108, 0.308, 0.6, 0.207]],
        matrix: [
            (1, 1, 0.018),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The same reed an octave and a half down with the corner at 500 Hz: a
    // bassoon is an oboe that has run out of top end.
    Chart {
        name: "BASSOON", label: "M23 BASSOON", slot: "M.23",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5333, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.4667, semitones: -12, cents: 6.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.4 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.35], pulse_width: 0.0, drive: 0.25,
        filter: [0.498, 0.45, 0.65, 0.45], velocity: 0.55, gain: 1.0,
        lfo: [(1, 0.707), (0, 0.28)],
        env: [[0.158, 0.452, 0.9, 0.232], [0.134, 0.355, 0.6, 0.232]],
        matrix: [
            (1, 1, 0.015),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Half an oscillator of noise under a sine, with envelope 2 shutting the
    // corner over the first tenth of a second: the chiff is the noise being
    // let through and then taken away.
    Chart {
        name: "PAN PIPE", label: "M24 PAN PIPE", slot: "M.24",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 2, table: 0.0, semitones: 12, cents: 0.0, level: 0.3 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 6.0, level: 0.3 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.3, 0.35], pulse_width: 0.0, drive: 0.15,
        filter: [0.725, 0.3, 0.325, 0.55], velocity: 0.6, gain: 1.0,
        lfo: [(1, 0.72), (0, 0.28)],
        env: [[0.108, 0.424, 0.85, 0.247], [0.0, 0.158, 0.0, 0.158]],
        matrix: [
            (1, 1, 0.02),   // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A saw and a reed table with an eighth of a second of attack and a 5 Hz
    // vibrato: the bow is the attack shape, and there is nothing else in a
    // synthesizer's violin.
    Chart {
        name: "VIOLIN", label: "M25 VIOLIN", slot: "M.25",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 6.0, level: 0.7 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -8.0, level: 0.7 },
            OscChart { shape: 0, table: 0.0, semitones: 12, cents: 0.0, level: 0.4 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.25,
        filter: [0.647, 0.5, 0.675, 0.55], velocity: 0.55, gain: 1.0,
        lfo: [(1, 0.732), (0, 0.28)],
        env: [[0.273, 0.476, 0.9, 0.308], [0.216, 0.393, 0.6, 0.308]],
        matrix: [
            (1, 1, 0.028),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The same bowing an octave and a fifth down with the corner at 600 Hz and
    // the resonance up a little, which is where a cello's body resonance sits.
    Chart {
        name: "CELLO", label: "M26 CELLO", slot: "M.26",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5333, semitones: -12, cents: 7.0, level: 0.7 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: -9.0, level: 0.8 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.25,
        filter: [0.525, 0.55, 0.675, 0.5], velocity: 0.55, gain: 1.0,
        lfo: [(1, 0.714), (0, 0.28)],
        env: [[0.297, 0.498, 0.9, 0.333], [0.247, 0.409, 0.6, 0.333]],
        matrix: [
            (1, 1, 0.024),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Two hundred milliseconds, no sustain, and envelope 2 a little faster
    // than envelope 1 so the sound goes dull before it goes quiet — which is
    // the whole difference between a pluck and a gate.
    Chart {
        name: "PLUCK", label: "M27 PLUCK", slot: "M.27",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 6.0, level: 0.8 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.35, drive: 0.35,
        filter: [0.599, 0.55, 0.725, 0.55], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.247, 0.0, 0.198], [0.0, 0.179, 0.0, 0.158]],
        matrix: [
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A triangle with a bell table a twelfth above it, both over in three
    // tenths of a second: a struck bar is a fundamental and one very high
    // partial, and almost nothing in between.
    Chart {
        name: "MARIMBA", label: "M28 MARIMBA", slot: "M.28",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 19, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.6 },
            OscChart { shape: 4, table: 0.8667, semitones: 12, cents: 0.0, level: 0.4 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.35], pulse_width: 0.0, drive: 0.15,
        filter: [0.684, 0.3, 0.7, 0.6], velocity: 0.85, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.308, 0.0, 0.247], [0.0, 0.179, 0.0, 0.158]],
        matrix: [
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The other thing this instrument is famous for. Envelope 2 on pitch at a
    // third of its depth with a forty-millisecond decay drops the oscillator
    // most of an octave in the time it takes to hear it, and the drive does
    // the rest. Melodic rather than keymapped, so the note played is the pitch
    // it lands on.
    Chart {
        name: "KICK DRUM", label: "M29 KICK", slot: "M.29",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: -24, cents: 0.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 5.0, level: 0.6 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.15 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.3, 0.3], pulse_width: 0.0, drive: 0.7,
        filter: [0.424, 0.35, 0.65, 0.1], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.355, 0.0, 0.158], [0.0, 0.077, 0.0, 0.093]],
        matrix: [
            (4, 1, 0.35),   // env 2 → pitch
            (5, 4, 0.2),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The same pitch envelope over a longer decay and higher up, with a little
    // noise in the mix for the head: a tom is a kick that is allowed to ring.
    Chart {
        name: "TOM DRUM", label: "M30 TOM", slot: "M.30",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 6.0, level: 0.6 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.3 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.35], pulse_width: 0.0, drive: 0.45,
        filter: [0.547, 0.45, 0.7, 0.2], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.0, 0.409, 0.0, 0.247], [0.0, 0.147, 0.0, 0.121]],
        matrix: [
            (4, 1, 0.25),   // env 2 → pitch
            (4, 8, 0.35),   // env 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The filter as the oscillator: resonance at 0.98, which is past the point
    // where four poles come back round in phase with more than unity gain, and
    // a whisper of noise to start it. What comes out is a sine at the corner
    // frequency.
    Chart {
        name: "SINE DRONE", label: "M31 SINEDRON", slot: "M.31",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.1 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.1 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 0.1 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.1 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.0,
        filter: [0.379, 0.98, 0.65, 0.85], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.217), (1, 0.149)],
        env: [[0.247, 0.551, 0.9, 0.393], [0.247, 0.476, 0.6, 0.355]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The same self-oscillation with the corner swept: envelope 2 takes it up
    // two decades and a slow triangle keeps moving it, so the pitch of the
    // tone is the filter's rather than the keyboard's.
    Chart {
        name: "FILTER SINE", label: "M32 FILTSINE", slot: "M.32",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.12 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 0.1 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.12 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.1 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.0,
        filter: [0.25, 0.97, 0.8, 0.6], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.304), (1, 0.2)],
        env: [[0.158, 0.593, 0.8, 0.424], [0.355, 0.551, 0.4, 0.393]],
        matrix: [
            (1, 4, 0.2),    // lfo 1 → cutoff
            (7, 4, 0.3),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Two noise sources, the corner at 600 Hz with the resonance well up, and
    // two slow LFOs moving it at rates that do not divide into each other.
    // Nothing here has a pitch.
    Chart {
        name: "WIND", label: "M33 WIND", slot: "M.33",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.7 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.15 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.2,
        filter: [0.525, 0.75, 0.65, 0.1], velocity: 0.3, gain: 1.0,
        lfo: [(0, 0.239), (1, 0.161)],
        env: [[0.683, 0.648, 0.85, 0.593], [0.605, 0.593, 0.5, 0.517]],
        matrix: [
            (1, 4, 0.3),    // lfo 1 → cutoff
            (2, 4, 0.2),    // lfo 2 → cutoff
            (2, 7, 0.3),    // lfo 2 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The wind patch with the corner lower and a sample-and-hold on it: what
    // turns a hiss into a sea is that the band moves in steps rather than
    // smoothly.
    Chart {
        name: "SURF", label: "M34 SURF", slot: "M.34",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.85 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.8 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.7 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.25,
        filter: [0.466, 0.7, 0.65, 0.1], velocity: 0.3, gain: 1.0,
        lfo: [(4, 0.483), (0, 0.191)],
        env: [[0.628, 0.691, 0.85, 0.648], [0.551, 0.648, 0.5, 0.593]],
        matrix: [
            (1, 4, 0.25),   // lfo 1 → cutoff
            (2, 4, 0.25),   // lfo 2 → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Noise and a saw with envelope 2 at half depth on pitch — and the
    // envelope's *attack* is the three seconds, not its decay, which is the
    // difference between a rise and a jump. Everything climbs an octave over
    // the first three seconds of a held note and stays there.
    Chart {
        name: "ROCKET", label: "M35 ROCKET", slot: "M.35",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.8 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 0.8 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.7 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 9.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.45,
        filter: [0.547, 0.65, 0.7, 0.3], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.247, 0.727, 0.8, 0.424], [0.863, 0.424, 0.9, 0.424]],
        matrix: [
            (4, 1, 0.5),    // env 2 → pitch
            (4, 4, 0.25),   // env 2 → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The sample-and-hold on the corner rather than on the pitch, at 5.5 Hz
    // and nearly half depth: the wobble that every modular patch sheet from
    // the period starts with.
    Chart {
        name: "S&H WOBBLE", label: "M36 S&H WOBL", slot: "M.36",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: -7.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.35, drive: 0.5,
        filter: [0.498, 0.72, 0.65, 0.35], velocity: 0.55, gain: 1.0,
        lfo: [(4, 0.735), (0, 0.325)],
        env: [[0.019, 0.452, 0.8, 0.247], [0.0, 0.333, 0.3, 0.247]],
        matrix: [
            (1, 4, 0.45),   // lfo 1 → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The trade this instrument is built around: oscillator D out of the mixer
    // and into its low range at 5 Hz, pointed at pitch. Three oscillators
    // become two and a modulation source, and the patch is quieter for it —
    // exactly as pulling a fader down would be.
    Chart {
        name: "OSC3 VIBRATO", label: "M37 OSC3 VIB", slot: "M.37",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -8.0, level: 0.95 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 3, table: 0.0, semitones: 6, cents: 0.0, level: 1.0 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 2, vector: [0.45, 0.25], pulse_width: 0.3, drive: 0.45,
        filter: [0.625, 0.5, 0.65, 0.5], velocity: 0.5, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.04, 0.424, 0.85, 0.26], [0.0, 0.333, 0.3, 0.26]],
        matrix: [
            (10, 1, 0.03),  // oscillator D → pitch
            (7, 4, 0.3),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The same oscillator left in its audio range instead: at a fourth above
    // the note and a fifth of the pitch depth it is not vibrato any more but
    // cross modulation, and what comes out is a growl rather than a wobble.
    Chart {
        name: "OSC3 GROWL", label: "M38 OSC3 GRW", slot: "M.38",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 3, table: 0.0, semitones: 5, cents: 0.0, level: 1.0 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 1, vector: [0.45, 0.25], pulse_width: 0.0, drive: 0.6,
        filter: [0.566, 0.55, 0.675, 0.4], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.685), (0, 0.28)],
        env: [[0.023, 0.424, 0.75, 0.247], [0.0, 0.308, 0.2, 0.247]],
        matrix: [
            (10, 1, 0.2),   // oscillator D → pitch
            (7, 4, 0.25),   // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // One waveform at 4', 8' and 16' with nothing detuned: the registration
    // rather than the beating, which is the other way to make three
    // oscillators sound like more than one.
    Chart {
        name: "OCTAVE LEAD", label: "M39 OCTAVELD", slot: "M.39",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 12, cents: 0.0, level: 0.8 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 1, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.3, drive: 0.45,
        filter: [0.667, 0.45, 0.65, 0.5], velocity: 0.5, gain: 1.0,
        lfo: [(1, 0.726), (0, 0.28)],
        env: [[0.032, 0.424, 0.8, 0.247], [0.0, 0.333, 0.25, 0.247]],
        matrix: [
            (7, 4, 0.3),    // wheel → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Everything at once and nothing in a hurry: four oscillators across three
    // octaves, the drive at two thirds, two slow LFOs on the vector and the
    // corner, and a four-second release. The patch you leave running while you
    // make the tea.
    Chart {
        name: "MODULAR DRONE", label: "M40 MODULAR", slot: "M.40",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 1, table: 0.0, semitones: -24, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.5333, semitones: 7, cents: -9.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.4, drive: 0.65,
        filter: [0.483, 0.7, 0.65, 0.3], velocity: 0.3, gain: 1.0,
        lfo: [(0, 0.123), (1, 0.053)],
        env: [[0.605, 0.727, 0.9, 0.783], [0.551, 0.691, 0.6, 0.727]],
        matrix: [
            (1, 4, 0.25),   // lfo 1 → cutoff
            (2, 7, 0.35),   // lfo 2 → vector x
            (1, 8, 0.3),    // lfo 1 → vector y
            (7, 4, 0.25),   // wheel → cutoff
            NO_ROUTE, NO_ROUTE,
        ],
    },

    // ── W.01 – W.47 · the Wavestation set ──
    // Authored, in the idiom rather than from a list. Sequencing and vector
    // movement, and one idea per patch: which sequences, at what clock, on
    // which oscillators, against what the vector is doing.
    // Three sequences of 8, 10 and 6 ticks on three oscillators at a step
    // every 0.7 seconds: their lengths share no factor, so the combination
    // takes 120 ticks — a minute and a half — to come back to where it
    // started.
    Chart {
        name: "WAVE VOYAGE", label: "W01 VOYAGE", slot: "W.01",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 6.0, level: 0.95 },
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: -7.0, level: 0.85 },
        ],
        seq: [Some(1), Some(2), None, Some(6)], seq_rate: seq_rate_at(2.49),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.15,
        filter: [0.713, 0.25, 0.625, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.149), (1, 0.092)],
        env: [[0.517, 0.605, 0.9, 0.498], [0.452, 0.551, 0.55, 0.452]],
        matrix: [
            (1, 7, 0.25),   // lfo 1 → vector x
            (2, 8, 0.25),   // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The eight-step morph on its own at a step every four seconds, which is
    // the bottom of the clock's travel. At that rate the crossfade is longer
    // than most notes, so what is heard is one waveform slowly becoming
    // another.
    Chart {
        name: "SLOW MORPH", label: "W02 SLOWMORF", slot: "W.02",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: -6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.3333, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 8.0, level: 0.6 },
        ],
        seq: [Some(1), Some(1), None, None], seq_rate: seq_rate_at(0.0),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.1,
        filter: [0.684, 0.3, 0.6, 0.3], velocity: 0.3, gain: 1.0,
        lfo: [(0, 0.108), (1, 0.053)],
        env: [[0.605, 0.648, 0.9, 0.551], [0.551, 0.605, 0.6, 0.498]],
        matrix: [
            (1, 7, 0.3),    // lfo 1 → vector x
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The vowel sequence on two oscillators an octave apart, and because its
    // steps are 3, 2, 3 and 2 ticks long the two do not sit on the same vowel
    // for more than a step at a time.
    Chart {
        name: "VOWEL DRIFT", label: "W03 VOWLDRFT", slot: "W.03",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 5.0, level: 0.9 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 0.0, level: 0.7 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: -8.0, level: 0.6 },
        ],
        seq: [Some(2), Some(2), None, None], seq_rate: seq_rate_at(1.85),
        d_mode: 0, vector: [0.4, 0.45], pulse_width: 0.0, drive: 0.15,
        filter: [0.657, 0.35, 0.6, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.137), (1, 0.073)],
        env: [[0.551, 0.628, 0.9, 0.517], [0.476, 0.58, 0.6, 0.476]],
        matrix: [
            (1, 7, 0.3),    // lfo 1 → vector x
            (2, 3, 0.06),   // lfo 2 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The bell run played once per note and then held, under a pad that takes
    // a second to arrive: the sequence is the attack and the oscillators
    // behind it are the sustain.
    Chart {
        name: "GLASS FIELD", label: "W04 GLASFELD", slot: "W.04",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 6.0, level: 0.7 },
            OscChart { shape: 4, table: 0.8667, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: -6.0, level: 0.7 },
        ],
        seq: [Some(5), None, None, None], seq_rate: seq_rate_at(3.58),
        d_mode: 0, vector: [0.35, 0.45], pulse_width: 0.0, drive: 0.1,
        filter: [0.737, 0.25, 0.625, 0.4], velocity: 0.45, gain: 1.0,
        lfo: [(0, 0.161), (1, 0.108)],
        env: [[0.58, 0.648, 0.85, 0.535], [0.355, 0.551, 0.5, 0.476]],
        matrix: [
            (1, 7, 0.25),   // lfo 1 → vector x
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The same three sequences as W.01 an octave and a half down with the
    // corner at 700 Hz: a sequence heard as a bass texture rather than as a
    // timbre, which is what the low end of this technique sounds like.
    Chart {
        name: "DEEP SEQUENCE", label: "W05 DEEPSEQ", slot: "W.05",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 7.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5, semitones: -24, cents: 0.0, level: 0.85 },
            OscChart { shape: 3, table: 0.0, semitones: -24, cents: 0.0, level: 0.6 },
        ],
        seq: [Some(6), Some(2), Some(1), None], seq_rate: seq_rate_at(2.14),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.3,
        filter: [0.547, 0.4, 0.65, 0.25], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.123), (1, 0.073)],
        env: [[0.424, 0.605, 0.85, 0.476], [0.355, 0.551, 0.5, 0.424]],
        matrix: [
            (1, 7, 0.2),    // lfo 1 → vector x
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Envelope 2 on the sequence clock at a fifth of its depth: the pattern
    // comes in nearly twice as fast as it settles to, so the movement slows
    // down as the note establishes itself.
    Chart {
        name: "NORTH LIGHT", label: "W06 NORTHLGT", slot: "W.06",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 7, cents: -5.0, level: 0.85 },
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.5, semitones: 12, cents: 0.0, level: 0.7 },
        ],
        seq: [Some(1), None, None, Some(2)], seq_rate: seq_rate_at(3.0),
        d_mode: 0, vector: [0.4, 0.45], pulse_width: 0.0, drive: 0.15,
        filter: [0.699, 0.3, 0.625, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.137), (1, 0.092)],
        env: [[0.551, 0.628, 0.9, 0.517], [0.247, 0.593, 0.3, 0.452]],
        matrix: [
            (4, 9, 0.2),    // env 2 → sequence clock
            (1, 7, 0.25),   // lfo 1 → vector x
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // One sequence and one LFO pulling in opposite directions: the organ
    // sequence walks its three registrations while a 0.08 Hz triangle — one
    // cycle every twelve seconds — moves the vector past it.
    Chart {
        name: "TIDE PAD", label: "W07 TIDE PAD", slot: "W.07",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.7333, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -9.0, level: 0.7 },
        ],
        seq: [Some(6), None, None, None], seq_rate: seq_rate_at(1.26),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.12,
        filter: [0.647, 0.35, 0.625, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.073), (1, 0.029)],
        env: [[0.628, 0.666, 0.9, 0.58], [0.551, 0.605, 0.6, 0.517]],
        matrix: [
            (1, 7, 0.35),   // lfo 1 → vector x
            (2, 8, 0.3),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The morph sequence's crossfade is the whole step, so there is no step to
    // hear at all — this patch is that sequence on all four oscillators at
    // four different rates of vector movement, which is the most continuous
    // sound the instrument can make.
    Chart {
        name: "CROSSFADE", label: "W08 CROSSFAD", slot: "W.08",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 5.0, level: 0.95 },
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: -5.0, level: 0.9 },
        ],
        seq: [Some(1), Some(1), Some(1), Some(1)], seq_rate: seq_rate_at(1.49),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.1,
        filter: [0.725, 0.25, 0.6, 0.3], velocity: 0.3, gain: 1.0,
        lfo: [(0, 0.092), (1, 0.149)],
        env: [[0.58, 0.648, 0.9, 0.551], [0.517, 0.593, 0.6, 0.498]],
        matrix: [
            (1, 7, 0.35),   // lfo 1 → vector x
            (2, 8, 0.35),   // lfo 2 → vector y
            (1, 3, 0.05),   // lfo 1 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The wave position swept by envelope 2 as well as by a sequence, which
    // are two different kinds of movement on the same control: one lands on
    // waveforms and the other slides between them.
    Chart {
        name: "SPECTRAL PAD", label: "W09 SPECTRAL", slot: "W.09",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: -7.0, level: 0.85 },
            OscChart { shape: 4, table: 0.8, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 8.0, level: 0.85 },
        ],
        seq: [Some(2), None, None, Some(6)], seq_rate: seq_rate_at(2.68),
        d_mode: 0, vector: [0.45, 0.4], pulse_width: 0.0, drive: 0.15,
        filter: [0.699, 0.3, 0.625, 0.3], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.123), (1, 0.172)],
        env: [[0.517, 0.605, 0.85, 0.498], [0.476, 0.58, 0.4, 0.452]],
        matrix: [
            (4, 3, 0.2),    // env 2 → wavetable
            (1, 7, 0.25),   // lfo 1 → vector x
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Three drawbar registrations walking on 3, 2 and 1 ticks, doubled an
    // octave up and a fifth over: the sound of a registration being changed
    // continuously, which no organ can do.
    Chart {
        name: "ORGAN CLOUD", label: "W10 ORGNCLUD", slot: "W.10",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 12, cents: 5.0, level: 0.7 },
            OscChart { shape: 4, table: 0.5, semitones: 7, cents: 0.0, level: 0.75 },
            OscChart { shape: 4, table: 0.4, semitones: -12, cents: 0.0, level: 0.85 },
        ],
        seq: [Some(6), Some(6), Some(6), None], seq_rate: seq_rate_at(3.26),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.25,
        filter: [0.684, 0.3, 0.6, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.182), (1, 0.123)],
        env: [[0.355, 0.551, 0.9, 0.424], [0.308, 0.498, 0.6, 0.393]],
        matrix: [
            (1, 7, 0.25),   // lfo 1 → vector x
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A sample-and-hold on the sequence clock: every step is a different
    // length because the rate itself is being stepped, which is the one way
    // this instrument can make a pattern that is not periodic.
    Chart {
        name: "STEP AURORA", label: "W11 AURORA", slot: "W.11",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 0.0, level: 0.85 },
        ],
        seq: [Some(1), None, None, Some(2)], seq_rate: seq_rate_at(2.85),
        d_mode: 0, vector: [0.4, 0.45], pulse_width: 0.0, drive: 0.15,
        filter: [0.692, 0.35, 0.625, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(4, 0.304), (1, 0.108)],
        env: [[0.551, 0.628, 0.9, 0.517], [0.476, 0.58, 0.6, 0.476]],
        matrix: [
            (1, 9, 0.3),    // lfo 1 → sequence clock
            (2, 7, 0.3),    // lfo 2 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Four seconds a step, two and a half of attack and four of release: the
    // slowest patch in the instrument, and the one that shows what a sequence
    // is when it is slower than the music.
    Chart {
        name: "LONG WAVE", label: "W12 LONG WAV", slot: "W.12",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 0.5, semitones: 7, cents: 0.0, level: 0.8 },
        ],
        seq: [Some(2), None, None, Some(1)], seq_rate: seq_rate_at(0.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.1,
        filter: [0.667, 0.3, 0.6, 0.3], velocity: 0.25, gain: 1.0,
        lfo: [(0, 0.029), (1, 0.0)],
        env: [[0.827, 0.727, 0.95, 0.783], [0.783, 0.691, 0.7, 0.727]],
        matrix: [
            (1, 7, 0.35),   // lfo 1 → vector x
            (2, 8, 0.3),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The riff sequence at a sixteenth of 120 on two oscillators a fourth
    // apart, so one part is played twice at two pitches and the interval
    // between them is fixed.
    Chart {
        name: "RIFF ENGINE", label: "W13 RIFFENGN", slot: "W.13",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 5, cents: 6.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 0.6 },
        ],
        seq: [Some(3), Some(3), None, None], seq_rate: seq_rate_at(5.0),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.35,
        filter: [0.684, 0.45, 0.675, 0.4], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.014, 0.424, 0.75, 0.247], [0.0, 0.308, 0.2, 0.247]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The gate on all four oscillators, so what is chopped is the whole voice
    // rather than half of it: a rhythm made of rests has to take the level to
    // nothing, and an oscillator left out of the sequence fills the rests in.
    // The drive is low for the same reason — a compressor pulls a rest back
    // up.
    Chart {
        name: "STEP CLAV", label: "W14 STEPCLAV", slot: "W.14",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.5 },
        ],
        seq: [Some(0), Some(0), Some(0), Some(0)], seq_rate: seq_rate_at(5.26),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.25,
        filter: [0.657, 0.55, 0.675, 0.55], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.005, 0.393, 0.7, 0.207], [0.0, 0.28, 0.15, 0.179]],
        matrix: [
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Two step lists on one clock at 6 Hz, and the clock is the only one there
    // is — what makes them fall apart and back together is their length rather
    // than their rate: the gate is four ticks and the stab is six, so the pair
    // lines up every third bar.
    Chart {
        name: "GATE PULSE", label: "W15 GATEPULS", slot: "W.15",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: -12, cents: 7.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -8.0, level: 0.8 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 0.7 },
        ],
        seq: [Some(0), Some(7), None, None], seq_rate: seq_rate_at(4.58),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.3, drive: 0.45,
        filter: [0.637, 0.55, 0.7, 0.4], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.005, 0.424, 0.8, 0.232], [0.0, 0.308, 0.2, 0.207]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Four sequences at once on four oscillators, all on the same clock: 4, 8,
    // 10 and 6 ticks, which is a hundred and twenty before the combination
    // repeats and is the densest thing in this bank.
    Chart {
        name: "SIXTEEN STEP", label: "W16 16 STEP", slot: "W.16",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.5, semitones: 7, cents: 0.0, level: 0.8 },
        ],
        seq: [Some(0), Some(1), Some(2), Some(6)], seq_rate: seq_rate_at(5.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.3,
        filter: [0.699, 0.4, 0.65, 0.35], velocity: 0.5, gain: 1.0,
        lfo: [(0, 0.252), (1, 0.191)],
        env: [[0.019, 0.476, 0.85, 0.28], [0.0, 0.355, 0.3, 0.28]],
        matrix: [
            (1, 7, 0.3),    // lfo 1 → vector x
            (2, 8, 0.25),   // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The stab sequence, whose steps are 1, 1, 2, 1, 1 and 2 ticks with rests
    // in half of them, against a four-tick gate: neither of them lands on the
    // beat the other does.
    Chart {
        name: "BROKEN TIME", label: "W17 BROKNTIM", slot: "W.17",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.5, semitones: 12, cents: -6.0, level: 0.7 },
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 8.0, level: 0.7 },
        ],
        seq: [Some(7), None, Some(0), None], seq_rate: seq_rate_at(4.81),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.35, drive: 0.4,
        filter: [0.675, 0.5, 0.7, 0.4], velocity: 0.6, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.01, 0.452, 0.8, 0.247], [0.0, 0.333, 0.25, 0.247]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // Three against two, done with tick counts rather than with tempo: the
    // six-tick organ sequence and the four-tick gate on one clock are a
    // hemiola that never drifts.
    Chart {
        name: "POLY RHYTHM", label: "W18 POLYRHYM", slot: "W.18",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 7, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.75 },
            OscChart { shape: 4, table: 0.3333, semitones: 0, cents: 0.0, level: 0.7 },
        ],
        seq: [Some(6), Some(0), None, None], seq_rate: seq_rate_at(4.58),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.35,
        filter: [0.667, 0.45, 0.675, 0.4], velocity: 0.55, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.014, 0.452, 0.8, 0.26], [0.0, 0.333, 0.25, 0.26]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The stab sequence on its own at a sixteenth of 140 with a short envelope
    // over it: the rests in the step list are what makes this a part rather
    // than a chord.
    Chart {
        name: "STAB LOOP", label: "W19 STABLOOP", slot: "W.19",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 7.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.8 },
            OscChart { shape: 1, table: 0.0, semitones: 12, cents: 0.0, level: 0.5 },
        ],
        seq: [Some(7), None, None, None], seq_rate: seq_rate_at(5.22),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.3, drive: 0.45,
        filter: [0.692, 0.5, 0.7, 0.4], velocity: 0.65, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.005, 0.374, 0.6, 0.216], [0.0, 0.26, 0.15, 0.198]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The one-shot run at a sixteenth of 120, so the four bells and the held
    // organ arrive inside the first beat: an attack transient built out of
    // four different waveforms, which no envelope can imitate because what
    // changes across it is the waveform.
    Chart {
        name: "BELL RUN", label: "W20 BELLRUN", slot: "W.20",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8667, semitones: 0, cents: 5.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.6 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.45 },
        ],
        seq: [Some(5), None, None, None], seq_rate: seq_rate_at(5.0),
        d_mode: 0, vector: [0.4, 0.35], pulse_width: 0.0, drive: 0.2,
        filter: [0.737, 0.3, 0.675, 0.55], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.0, 0.551, 0.3, 0.393], [0.0, 0.333, 0.0, 0.28]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The three-step attack sequence — bell, clavinet, drawbar, played once
    // and then held — on two oscillators an octave apart, so the strike
    // arrives twice in two registers. The clock is shared, which it has to be:
    // there is one sequence rate on this panel, and what a patch can vary per
    // oscillator is the step list rather than the tempo.
    Chart {
        name: "ATTACK STACK", label: "W21 ATKSTACK", slot: "W.21",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 6.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 0.9333, semitones: 12, cents: 0.0, level: 0.5 },
        ],
        seq: [Some(4), Some(4), None, None], seq_rate: seq_rate_at(5.32),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.35,
        filter: [0.699, 0.4, 0.7, 0.45], velocity: 0.7, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.0, 0.498, 0.55, 0.308], [0.0, 0.308, 0.1, 0.247]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // A gate fast enough to be a timbre rather than a rhythm: 24 Hz, which is
    // past the bottom of hearing and into the range where the crossfades
    // themselves are a waveform.
    Chart {
        name: "PULSE FIELD", label: "W22 PULSFELD", slot: "W.22",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 7, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -8.0, level: 0.7 },
        ],
        seq: [Some(0), Some(1), None, None], seq_rate: seq_rate_at(6.58),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.3,
        filter: [0.713, 0.4, 0.625, 0.35], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.217), (1, 0.161)],
        env: [[0.247, 0.551, 0.85, 0.393], [0.158, 0.452, 0.5, 0.355]],
        matrix: [
            (1, 7, 0.25),   // lfo 1 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // No sequences at all: four unrelated tables and two LFOs at different
    // rates on the two vector axes, which walks the mix round an ellipse that
    // never closes.
    Chart {
        name: "VECTOR ARC", label: "W23 VECTORAC", slot: "W.23",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.3333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 6.0, level: 0.95 },
            OscChart { shape: 4, table: 0.8, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: -6.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.15,
        filter: [0.684, 0.3, 0.625, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.161), (1, 0.092)],
        env: [[0.476, 0.593, 0.9, 0.476], [0.424, 0.535, 0.6, 0.424]],
        matrix: [
            (1, 7, 0.45),   // lfo 1 → vector x
            (2, 8, 0.45),   // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Square LFOs on both axes, at rates a third apart: the mix jumps between
    // the four corners rather than sliding between them, which is the vector
    // used as a switch.
    Chart {
        name: "SQUARE WALK", label: "W24 SQU WALK", slot: "W.24",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.4, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.7333, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.25,
        filter: [0.692, 0.4, 0.65, 0.35], velocity: 0.45, gain: 1.0,
        lfo: [(3, 0.605), (3, 0.669)],
        env: [[0.158, 0.517, 0.85, 0.355], [0.108, 0.424, 0.5, 0.333]],
        matrix: [
            (1, 7, 0.5),    // lfo 1 → vector x
            (2, 8, 0.5),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Envelope 2 on both axes at full depth: every note starts in the A corner
    // and travels to the C one over a second and a half, so the timbre is a
    // function of how long the key has been down.
    Chart {
        name: "CORNER SWEEP", label: "W25 CORNSWEP", slot: "W.25",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 5.0, level: 0.9 },
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.8667, semitones: -12, cents: 0.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.1, 0.1], pulse_width: 0.0, drive: 0.2,
        filter: [0.675, 0.35, 0.625, 0.35], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.217), (1, 0.149)],
        env: [[0.355, 0.605, 0.85, 0.452], [0.424, 0.593, 0.9, 0.424]],
        matrix: [
            (4, 7, 0.8),    // env 2 → vector x
            (4, 8, 0.8),    // env 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The two axes on LFOs whose rates are nearly but not quite equal — 0.11
    // and 0.13 Hz — so the path through the square precesses instead of
    // repeating, and the pad takes two minutes to say everything it has.
    Chart {
        name: "X-Y DRIFT", label: "W26 X-YDRIFT", slot: "W.26",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.3333, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: -7.0, level: 0.85 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.12,
        filter: [0.667, 0.3, 0.6, 0.3], velocity: 0.3, gain: 1.0,
        lfo: [(0, 0.123), (1, 0.149)],
        env: [[0.628, 0.666, 0.9, 0.566], [0.58, 0.628, 0.6, 0.517]],
        matrix: [
            (1, 7, 0.4),    // lfo 1 → vector x
            (2, 8, 0.4),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The wheel on the vector, which is what a Wavestation's stick actually
    // is: one hand moving the balance of four oscillators while the other
    // plays.
    Chart {
        name: "JOYSTICK PAD", label: "W27 JOYSTICK", slot: "W.27",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.8, semitones: -12, cents: 0.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.2, 0.2], pulse_width: 0.0, drive: 0.15,
        filter: [0.684, 0.35, 0.625, 0.35], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.172), (1, 0.108)],
        env: [[0.424, 0.58, 0.9, 0.452], [0.355, 0.517, 0.6, 0.424]],
        matrix: [
            (7, 7, 0.6),    // wheel → vector x
            (7, 8, 0.6),    // wheel → vector y
            (1, 7, 0.15),   // lfo 1 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // One waveform per corner and nothing shared between them: a saw, a vowel,
    // a bell and noise. Whatever the vector does here is audible, which is the
    // point of the patch.
    Chart {
        name: "FOUR CORNERS", label: "W28 4CORNERS", slot: "W.28",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.75 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.45 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.2,
        filter: [0.692, 0.4, 0.65, 0.35], velocity: 0.4, gain: 1.0,
        lfo: [(0, 0.209), (1, 0.137)],
        env: [[0.308, 0.551, 0.85, 0.424], [0.247, 0.476, 0.5, 0.393]],
        matrix: [
            (1, 7, 0.4),    // lfo 1 → vector x
            (2, 8, 0.4),    // lfo 2 → vector y
            (5, 8, -0.2),   // velocity → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The slowest vector movement the LFOs can make — 0.05 Hz, a cycle every
    // twenty seconds — with a four-second release, so a held chord moves
    // further after the keys are up than while they are down.
    Chart {
        name: "SLOW ORBIT", label: "W29 SLOWORBT", slot: "W.29",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 5.0, level: 0.9 },
            OscChart { shape: 4, table: 0.3333, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.8667, semitones: 0, cents: -6.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.5, 0.5], pulse_width: 0.0, drive: 0.1,
        filter: [0.647, 0.3, 0.6, 0.3], velocity: 0.25, gain: 1.0,
        lfo: [(0, 0.0), (1, 0.029)],
        env: [[0.727, 0.727, 0.95, 0.783], [0.648, 0.691, 0.7, 0.727]],
        matrix: [
            (1, 7, 0.45),   // lfo 1 → vector x
            (2, 8, 0.45),   // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The vector movement compressed into a stab: envelope 2 has an eighty-
    // millisecond decay and full depth on both axes, so the mix crosses the
    // whole square inside the attack.
    Chart {
        name: "VECTOR STAB", label: "W30 VEC STAB", slot: "W.30",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: -12, cents: 0.0, level: 0.85 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.15, 0.15], pulse_width: 0.0, drive: 0.4,
        filter: [0.699, 0.45, 0.675, 0.4], velocity: 0.65, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.005, 0.355, 0.3, 0.216], [0.0, 0.134, 0.0, 0.134]],
        matrix: [
            (4, 7, 0.75),   // env 2 → vector x
            (4, 8, 0.75),   // env 2 → vector y
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The bell table is five partials at 1, 5, 9, 13 and 17 — no octave, no
    // fifth, nothing that makes a pitch feel settled — which is why it reads
    // as struck glass rather than as a note.
    Chart {
        name: "CRYSTAL BELL", label: "W31 CRYSTAL", slot: "W.31",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 4.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.45 },
            OscChart { shape: 4, table: 1.0, semitones: 24, cents: 0.0, level: 0.3 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.35], pulse_width: 0.0, drive: 0.05,
        filter: [0.78, 0.2, 0.675, 0.55], velocity: 0.85, gain: 1.0,
        lfo: [(1, 0.36), (0, 0.28)],
        env: [[0.0, 0.605, 0.0, 0.551], [0.0, 0.393, 0.0, 0.355]],
        matrix: [
            (5, 3, 0.1),    // velocity → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A triangle body with the bell an octave and a fifth over it, both gone
    // in half a second: the ratio is what makes it a mallet rather than a
    // bell, because the partial is high enough to be heard as a strike.
    Chart {
        name: "MALLET", label: "W32 MALLET", slot: "W.32",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 19, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.55 },
            OscChart { shape: 4, table: 0.8667, semitones: 12, cents: 0.0, level: 0.4 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.35], pulse_width: 0.0, drive: 0.1,
        filter: [0.713, 0.25, 0.7, 0.6], velocity: 0.85, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.0, 0.393, 0.0, 0.308], [0.0, 0.216, 0.0, 0.179]],
        matrix: [
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Four seconds of decay and a fifth above the fundamental, which is the
    // partial a tube actually has: a tubular bell is the one struck instrument
    // whose overtones are nearly harmonic.
    Chart {
        name: "TUBULAR", label: "W33 TUBULAR", slot: "W.33",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: 7, cents: 0.0, level: 0.6 },
            OscChart { shape: 4, table: 0.8667, semitones: 12, cents: 5.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.4], pulse_width: 0.0, drive: 0.08,
        filter: [0.748, 0.25, 0.65, 0.5], velocity: 0.8, gain: 1.0,
        lfo: [(1, 0.325), (0, 0.252)],
        env: [[0.0, 0.783, 0.0, 0.727], [0.0, 0.551, 0.0, 0.476]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // A sine and a bell an octave and a half up, both very short, with
    // velocity most of the way onto the wave position: played softly it is
    // nearly a sine, played hard it is nearly all partial.
    Chart {
        name: "MUSIC BOX", label: "W34 MUSICBOX", slot: "W.34",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 19, cents: 0.0, level: 0.5 },
            OscChart { shape: 4, table: 1.0, semitones: 24, cents: 0.0, level: 0.25 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.4 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.3, 0.35], pulse_width: 0.0, drive: 0.05,
        filter: [0.758, 0.2, 0.65, 0.6], velocity: 0.9, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.0, 0.452, 0.0, 0.393], [0.0, 0.247, 0.0, 0.207]],
        matrix: [
            (5, 3, 0.25),   // velocity → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Two bells thirteen cents apart, which is the beating a pair of gamelan
    // keys are deliberately tuned to, over a low gong from the same table two
    // octaves down.
    Chart {
        name: "GAMELAN", label: "W35 GAMELAN", slot: "W.35",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: -13.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 13.0, level: 0.95 },
            OscChart { shape: 4, table: 0.8, semitones: -24, cents: 0.0, level: 0.6 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.35 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.35], pulse_width: 0.0, drive: 0.1,
        filter: [0.737, 0.25, 0.675, 0.5], velocity: 0.85, gain: 1.0,
        lfo: [(1, 0.304), (0, 0.232)],
        env: [[0.0, 0.628, 0.05, 0.551], [0.0, 0.424, 0.0, 0.355]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // The attack sequence played once — bell, clavinet, drawbar — into a held
    // table, which is a strike made of three waveforms in a twentieth of a
    // second each.
    Chart {
        name: "STRUCK GLASS", label: "W36 STRKGLAS", slot: "W.36",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 4, table: 1.0, semitones: 19, cents: 0.0, level: 0.3 },
        ],
        seq: [Some(4), None, None, None], seq_rate: seq_rate_at(6.32),
        d_mode: 0, vector: [0.35, 0.35], pulse_width: 0.0, drive: 0.1,
        filter: [0.758, 0.25, 0.675, 0.55], velocity: 0.85, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.0, 0.551, 0.0, 0.476], [0.0, 0.333, 0.0, 0.28]],
        matrix: [NO_ROUTE; MOD_SLOTS],
    },
    // A triangle and a clavinet table with a two-hundred-millisecond decay and
    // the corner tracking the keyboard fully: a thumb piano is a plucked bar,
    // so it is brighter at the top of its range and there is nothing in it
    // that sustains.
    Chart {
        name: "KALIMBA", label: "W37 KALIMBA", slot: "W.37",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 6.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.4 },
            OscChart { shape: 4, table: 0.8, semitones: 24, cents: 0.0, level: 0.25 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.35], pulse_width: 0.0, drive: 0.15,
        filter: [0.684, 0.35, 0.7, 0.85], velocity: 0.85, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.0, 0.273, 0.0, 0.216], [0.0, 0.158, 0.0, 0.134]],
        matrix: [
            (5, 4, 0.3),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The electric piano table with a bell two octaves up: a celesta is a
    // struck bar with a resonator, and the resonator is why it decays in a
    // second rather than in a tenth of one.
    Chart {
        name: "CELESTA", label: "W38 CELESTA", slot: "W.38",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.8667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 24, cents: 0.0, level: 0.35 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.5 },
            OscChart { shape: 4, table: 0.8667, semitones: 12, cents: 4.0, level: 0.45 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.35], pulse_width: 0.0, drive: 0.05,
        filter: [0.767, 0.2, 0.65, 0.6], velocity: 0.85, gain: 1.0,
        lfo: [(0, 0.28), (1, 0.217)],
        env: [[0.0, 0.517, 0.0, 0.452], [0.0, 0.308, 0.0, 0.26]],
        matrix: [
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Four voices on the ah table across two octaves, detuned nine cents apart
    // and with the vector drifting between them: a choir is a detune and a
    // slow change of balance, which is exactly what this instrument is built
    // out of.
    Chart {
        name: "CHOIR AAH", label: "W39 ChoirAAH", slot: "W.39",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: -9.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 7.0, level: 0.95 },
            OscChart { shape: 4, table: 0.6667, semitones: -12, cents: -4.0, level: 0.9 },
            OscChart { shape: 4, table: 0.7333, semitones: 12, cents: 0.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.1,
        filter: [0.647, 0.35, 0.6, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.137), (1, 0.191)],
        env: [[0.517, 0.605, 0.9, 0.498], [0.452, 0.551, 0.6, 0.452]],
        matrix: [
            (1, 7, 0.3),    // lfo 1 → vector x
            (2, 8, 0.25),   // lfo 2 → vector y
            (1, 1, 0.01),   // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The other vowel, which has its energy at the fundamental and the fourth
    // harmonic rather than the third and the seventh — so the same arrangement
    // comes out rounder and needs the corner lower to stay that way.
    Chart {
        name: "CHOIR OOH", label: "W40 ChoirOOH", slot: "W.40",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: -8.0, level: 1.0 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 6.0, level: 0.95 },
            OscChart { shape: 4, table: 0.7333, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: -5.0, level: 0.6 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.1,
        filter: [0.599, 0.35, 0.6, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.108), (1, 0.172)],
        env: [[0.535, 0.617, 0.9, 0.517], [0.476, 0.566, 0.6, 0.464]],
        matrix: [
            (1, 7, 0.3),    // lfo 1 → vector x
            (2, 8, 0.25),   // lfo 2 → vector y
            (1, 1, 0.009),  // lfo 1 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The vowel sequence and a held vowel on the same patch: two oscillators
    // walk ah, oo, hollow and reed while the other two stay put, so the choir
    // keeps changing its mind about one half of itself.
    Chart {
        name: "VOX PAD", label: "W41 VOX PAD", slot: "W.41",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: -7.0, level: 0.85 },
        ],
        seq: [Some(2), None, Some(2), None], seq_rate: seq_rate_at(2.26),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.12,
        filter: [0.657, 0.35, 0.6, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.123), (1, 0.073)],
        env: [[0.551, 0.628, 0.9, 0.517], [0.476, 0.58, 0.6, 0.476]],
        matrix: [
            (1, 7, 0.25),   // lfo 1 → vector x
            (2, 8, 0.2),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Strings whose bow pressure changes: saws with the morph sequence on one
    // of them, so one voice of the section is walking through the wave bank
    // while the others hold.
    Chart {
        name: "STRING WAVE", label: "W42 STRINGWV", slot: "W.42",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -10.0, level: 1.0 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 8.0, level: 0.95 },
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 12, cents: -5.0, level: 0.7 },
        ],
        seq: [None, None, Some(1), None], seq_rate: seq_rate_at(1.68),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.12,
        filter: [0.637, 0.35, 0.625, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(1, 0.325), (0, 0.258)],
        env: [[0.476, 0.593, 0.9, 0.487], [0.424, 0.551, 0.55, 0.452]],
        matrix: [
            (1, 1, 0.012),  // lfo 1 → pitch
            (2, 7, 0.25),   // lfo 2 → vector x
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // One voice rather than a section: no detuning worth the name, a 5 Hz
    // vibrato that arrives with the note, and envelope 2 moving the wave
    // position from oo towards ah as the note opens out.
    Chart {
        name: "SOLO VOICE", label: "W43 SOLOVOIC", slot: "W.43",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 3.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.4 },
            OscChart { shape: 4, table: 0.4667, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.35, 0.4], pulse_width: 0.0, drive: 0.1,
        filter: [0.637, 0.45, 0.6, 0.4], velocity: 0.45, gain: 1.0,
        lfo: [(1, 0.72), (0, 0.28)],
        env: [[0.355, 0.551, 0.9, 0.374], [0.424, 0.517, 0.6, 0.355]],
        matrix: [
            (1, 1, 0.02),   // lfo 1 → pitch
            (4, 3, 0.1),    // env 2 → wavetable
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // A fifth of an oscillator of noise in a vowel pad, with the vector moving
    // towards it and back: breath is the part of a voice that has no pitch,
    // and it belongs in the mix rather than in the attack.
    Chart {
        name: "BREATH PAD", label: "W44 BREATHPD", slot: "W.44",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 7.0, level: 0.9 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.22 },
            OscChart { shape: 4, table: 0.4667, semitones: -12, cents: 0.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.4], pulse_width: 0.0, drive: 0.1,
        filter: [0.657, 0.4, 0.6, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.149), (1, 0.092)],
        env: [[0.551, 0.628, 0.9, 0.517], [0.476, 0.58, 0.6, 0.476]],
        matrix: [
            (1, 7, 0.35),   // lfo 1 → vector x
            (2, 8, 0.3),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // Half analog and half wavetable: two saws for the body, the reed and
    // hollow tables for the bow, and a slow crossfade between the two halves
    // on the vector.
    Chart {
        name: "HYBRID STRINGS", label: "W45 HYBRDSTR", slot: "W.45",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: -9.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 6.0, level: 0.85 },
            OscChart { shape: 4, table: 0.4667, semitones: -12, cents: 0.0, level: 0.85 },
            OscChart { shape: 0, table: 0.0, semitones: 0, cents: 9.0, level: 0.9 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.15,
        filter: [0.647, 0.35, 0.625, 0.3], velocity: 0.35, gain: 1.0,
        lfo: [(0, 0.092), (1, 0.161)],
        env: [[0.498, 0.593, 0.9, 0.487], [0.452, 0.551, 0.55, 0.452]],
        matrix: [
            (1, 7, 0.35),   // lfo 1 → vector x
            (2, 8, 0.3),    // lfo 2 → vector y
            (2, 1, 0.01),   // lfo 2 → pitch
            NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The morph sequence on the two outer voices and the vowel sequence on the
    // two inner ones, at a step every second and a half: a choir singing
    // something that is not a vowel and not a word.
    Chart {
        name: "WAVE CHOIR", label: "W46 WAVECHOR", slot: "W.46",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: 6.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.5, semitones: 0, cents: -6.0, level: 0.9 },
        ],
        seq: [Some(1), Some(2), Some(2), Some(1)], seq_rate: seq_rate_at(1.4),
        d_mode: 0, vector: [0.45, 0.45], pulse_width: 0.0, drive: 0.12,
        filter: [0.667, 0.3, 0.6, 0.3], velocity: 0.3, gain: 1.0,
        lfo: [(0, 0.108), (1, 0.053)],
        env: [[0.58, 0.648, 0.9, 0.551], [0.517, 0.605, 0.6, 0.498]],
        matrix: [
            (1, 7, 0.3),    // lfo 1 → vector x
            (2, 8, 0.3),    // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },
    // The quietest patch in the bank: velocity depth at a quarter, the corner
    // at 900 Hz, and a two-second attack. What it is for is the background of
    // something else.
    Chart {
        name: "AIR CHOIR", label: "W47 AIRCHOIR", slot: "W.47",
        keys: KeyMap::Melodic,
        osc: [
            OscChart { shape: 4, table: 0.7333, semitones: 0, cents: 0.0, level: 0.85 },
            OscChart { shape: 4, table: 0.6667, semitones: 0, cents: -6.0, level: 0.8 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.14 },
            OscChart { shape: 4, table: 0.7333, semitones: -12, cents: 7.0, level: 0.8 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.4, 0.45], pulse_width: 0.0, drive: 0.05,
        filter: [0.583, 0.3, 0.575, 0.3], velocity: 0.25, gain: 1.0,
        lfo: [(0, 0.053), (1, 0.0)],
        env: [[0.783, 0.691, 0.9, 0.648], [0.727, 0.648, 0.6, 0.593]],
        matrix: [
            (1, 7, 0.3),    // lfo 1 → vector x
            (2, 8, 0.25),   // lfo 2 → vector y
            NO_ROUTE, NO_ROUTE, NO_ROUTE, NO_ROUTE,
        ],
    },

    // ── W.48 – W.50 · the kits ──
    // Keymapped patches: the note picks a whole voice recipe rather than a
    // pitch. The panel row on each of these is the kit's first recipe, so a
    // player who opens the panel sees something plausible; the matrix under it
    // is shared by every note of the kit, which is why each kit states its own
    // convention for what the four oscillators are.
    // Eleven notes, eleven recipes, one instrument: sine bodies with a pitch
    // drop for the drums and the ladder's bass loss at resonance for the
    // cymbals, which is the same trick the starter kit uses and the only high-
    // pass this instrument has. The convention every recipe follows here —
    // because the matrix below is shared across the kit — is that oscillator A
    // is the body, B the second pitched element and C and D the noise, with
    // the vector resting near the A corner and envelope 2 pushing it towards C
    // at the strike.
    Chart {
        name: "ANALOG KIT", label: "W48 ANLOGKIT", slot: "W.48",
        keys: KeyMap::Keymapped(&ANALOG_KIT),
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.5 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.15, 0.15], pulse_width: 0.0, drive: 0.35,
        filter: [0.599, 0.3, 0.5, 0.0], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.304), (0, 0.217)],
        env: [[0.0, 0.333, 0.0, 0.158], [0.0, 0.085, 0.0, 0.093]],
        matrix: [
            (4, 1, 0.35),   // env 2 → pitch
            (4, 7, 0.6),    // env 2 → vector x
            (4, 8, 0.6),    // env 2 → vector y
            (5, 4, 0.2),    // velocity → cutoff
            NO_ROUTE, NO_ROUTE,
        ],
    },
    // The same idea with the bodies taken from the wavetables instead of from
    // the analog shapes: the clavinet is the snare, the reed and the electric
    // piano are two of the toms, and the digital table is the hats. No two of
    // the twelve share a waveform, which is what keeps this from being one
    // drum transposed. Envelope 2 walks the wave position as well as the
    // vector, so every strike moves through the bank on its way out.
    Chart {
        name: "WAVE KIT", label: "W49 WAVE KIT", slot: "W.49",
        keys: KeyMap::Keymapped(&WAVE_KIT),
        osc: [
            OscChart { shape: 4, table: 1.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 0.8, semitones: 19, cents: 0.0, level: 0.4 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.2, 0.15], pulse_width: 0.0, drive: 0.3,
        filter: [0.625, 0.3, 0.5, 0.0], velocity: 0.8, gain: 1.0,
        lfo: [(0, 0.304), (0, 0.217)],
        env: [[0.0, 0.318, 0.0, 0.158], [0.0, 0.093, 0.0, 0.093]],
        matrix: [
            (4, 1, 0.25),   // env 2 → pitch
            (4, 3, 0.2),    // env 2 → wavetable
            (4, 7, 0.55),   // env 2 → vector x
            (4, 8, 0.55),   // env 2 → vector y
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE,
        ],
    },
    // Hand percussion on the General MIDI notes it belongs on: bongos, congas,
    // claves, woodblocks, a cowbell made of two pulses a flat fifth apart, and
    // shakers that are noise through a corner at 10 kHz. Nothing here has a
    // pitch drop — a struck skin has one and a struck stick does not — so this
    // kit's matrix puts envelope 2 on the vector and the corner only.
    Chart {
        name: "PERC KIT", label: "W50 PERC KIT", slot: "W.50",
        keys: KeyMap::Keymapped(&PERC_KIT),
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: 7, cents: 0.0, level: 0.5 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 4, table: 0.4667, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        seq: [None; 4], seq_rate: seq_rate_at(4.0),
        d_mode: 0, vector: [0.3, 0.3], pulse_width: 0.0, drive: 0.25,
        filter: [0.699, 0.35, 0.5, 0.0], velocity: 0.75, gain: 1.0,
        lfo: [(0, 0.304), (0, 0.217)],
        env: [[0.0, 0.216, 0.0, 0.134], [0.0, 0.093, 0.0, 0.077]],
        matrix: [
            (4, 7, 0.5),    // env 2 → vector x
            (4, 8, 0.5),    // env 2 → vector y
            (4, 4, 0.2),    // env 2 → cutoff
            (5, 4, 0.25),   // velocity → cutoff
            NO_ROUTE, NO_ROUTE,
        ],
    },
];

/// The starter kit.
///
/// Eight recipes on the General MIDI notes they belong on: a kick, a snare,
/// two toms, two hi-hats, a clap and a crash, each a different arrangement of
/// the same four oscillators. Deliberately the smallest of the four kits — it
/// is what proved the mechanism in phase one, and the three at W.48 are what
/// the mechanism was for.
///
/// The convention every recipe follows, because the matrix is shared across
/// the kit and has to mean the same thing on each of them: **oscillator A is
/// the body, B the second pitched element, C and D the noise**, with the
/// vector resting near the A corner. Envelope 2 pushes the vector towards C at
/// the strike and lets it fall back, so each recipe's own envelope 2 decay is
/// how long its noise transient lasts — 25 ms on the kick, 285 ms on the
/// snare.
///
/// The hi-hats are worth a note. There is no high-pass filter on this
/// instrument, and a hi-hat made of noise through a low-pass would be a hiss
/// with all its bottom end still attached. What makes them work is the
/// ladder's bass loss at resonance — the thing the design brief was firm about
/// not compensating away: at 0.72 of the resonance travel the filter is better
/// than 12 dB down at the bottom, so noise through it comes out as a bright
/// band. The characteristic that makes the filter sound right is also the one
/// that makes a cymbal possible.
const SYNTH_KIT: [KeyChart; 8] = [
    KeyChart {
        note: 36, name: "kick", hz: 55.0,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.6 },
        ],
        vector: [0.10, 0.10], filter: [0.45, 0.05, 0.60],
        env: [[0.0, 0.30, 0.0, 0.10], [0.0, 0.05, 0.0, 0.05]],
        level: 1.6,
    },
    KeyChart {
        note: 38, name: "snare", hz: 190.0,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 0.8 },
            OscChart { shape: 2, table: 0.0, semitones: 7, cents: 0.0, level: 0.6 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
        ],
        vector: [0.50, 0.55], filter: [0.72, 0.30, 0.60],
        env: [[0.0, 0.22, 0.0, 0.08], [0.0, 0.12, 0.0, 0.05]],
        level: 1.8,
    },
    KeyChart {
        note: 39, name: "clap", hz: 400.0,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
        ],
        vector: [0.5, 0.5], filter: [0.70, 0.62, 0.55],
        env: [[0.0, 0.20, 0.0, 0.08], [0.0, 0.06, 0.0, 0.05]],
        level: 1.7,
    },
    KeyChart {
        note: 41, name: "low tom", hz: 100.0,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 0.4 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        vector: [0.20, 0.20], filter: [0.50, 0.05, 0.58],
        env: [[0.0, 0.38, 0.0, 0.12], [0.0, 0.10, 0.0, 0.05]],
        level: 1.6,
    },
    KeyChart {
        note: 42, name: "closed hat", hz: 800.0,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 24, cents: 0.0, level: 0.5 },
        ],
        vector: [0.5, 0.5], filter: [0.92, 0.72, 0.52],
        env: [[0.0, 0.07, 0.0, 0.04], [0.0, 0.03, 0.0, 0.03]],
        level: 1.7,
    },
    KeyChart {
        note: 45, name: "mid tom", hz: 150.0,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 0.4 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        vector: [0.20, 0.20], filter: [0.54, 0.05, 0.58],
        env: [[0.0, 0.34, 0.0, 0.12], [0.0, 0.09, 0.0, 0.05]],
        level: 1.6,
    },
    KeyChart {
        note: 46, name: "open hat", hz: 800.0,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 24, cents: 0.0, level: 0.5 },
        ],
        vector: [0.5, 0.5], filter: [0.90, 0.70, 0.52],
        env: [[0.0, 0.28, 0.0, 0.10], [0.0, 0.05, 0.0, 0.04]],
        level: 1.6,
    },
    KeyChart {
        note: 49, name: "crash", hz: 1200.0,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.6 },
        ],
        vector: [0.5, 0.5], filter: [0.95, 0.60, 0.52],
        env: [[0.0, 0.50, 0.0, 0.30], [0.0, 0.20, 0.0, 0.10]],
        level: 1.3,
    },
];

/// W.48 ANALOG KIT: sine bodies, noise cymbals.
///
/// Eleven recipes following the starter kit's convention — A the body, B the
/// second pitched element, C and D the noise — but voiced further apart: the
/// kick is a 50 Hz sine with a pitch drop, the three toms are one recipe at
/// three frequencies and three decays, and the cymbals are noise through a
/// corner at 9 kHz with the resonance high enough to take the bottom off. The
/// ride is the one cymbal with a pitched component in it, which is what
/// separates it from the crash.
const ANALOG_KIT: [KeyChart; 11] = [
    KeyChart {
        note: 36, name: "kick", hz: 50.0,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: -12, cents: 0.0, level: 0.7 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.5 },
        ],
        vector: [0.1, 0.1], filter: [0.473, 0.12, 0.65],
        env: [[0.0, 0.333, 0.0, 0.158], [0.0, 0.085, 0.0, 0.093]],
        level: 1.6,
    },
    KeyChart {
        note: 37, name: "rim", hz: 330.0,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 7, cents: 0.0, level: 0.6 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.8 },
        ],
        vector: [0.4, 0.45], filter: [0.725, 0.55, 0.65],
        env: [[0.0, 0.1, 0.0, 0.077], [0.0, 0.05, 0.0, 0.059]],
        level: 1.5,
    },
    KeyChart {
        note: 38, name: "snare", hz: 185.0,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 0.85 },
            OscChart { shape: 2, table: 0.0, semitones: 7, cents: 0.0, level: 0.55 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
        ],
        vector: [0.45, 0.5], filter: [0.692, 0.35, 0.675],
        env: [[0.0, 0.26, 0.0, 0.134], [0.0, 0.179, 0.0, 0.093]],
        level: 1.7,
    },
    KeyChart {
        note: 39, name: "clap", hz: 380.0,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
        ],
        vector: [0.5, 0.5], filter: [0.657, 0.65, 0.7],
        env: [[0.0, 0.247, 0.0, 0.134], [0.0, 0.108, 0.0, 0.093]],
        level: 1.6,
    },
    KeyChart {
        note: 41, name: "low tom", hz: 95.0,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 0.45 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        vector: [0.2, 0.2], filter: [0.547, 0.1, 0.65],
        env: [[0.0, 0.363, 0.0, 0.179], [0.0, 0.158, 0.0, 0.093]],
        level: 1.6,
    },
    KeyChart {
        note: 42, name: "closed hat", hz: 900.0,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 24, cents: 0.0, level: 0.5 },
        ],
        vector: [0.5, 0.5], filter: [0.917, 0.72, 0.625],
        env: [[0.0, 0.108, 0.0, 0.077], [0.0, 0.059, 0.0, 0.059]],
        level: 1.6,
    },
    KeyChart {
        note: 45, name: "mid tom", hz: 140.0,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 0.45 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        vector: [0.2, 0.2], filter: [0.583, 0.1, 0.65],
        env: [[0.0, 0.337, 0.0, 0.179], [0.0, 0.147, 0.0, 0.093]],
        level: 1.6,
    },
    KeyChart {
        note: 46, name: "open hat", hz: 900.0,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 24, cents: 0.0, level: 0.5 },
        ],
        vector: [0.5, 0.5], filter: [0.9, 0.7, 0.625],
        env: [[0.0, 0.308, 0.0, 0.158], [0.0, 0.093, 0.0, 0.077]],
        level: 1.5,
    },
    KeyChart {
        note: 48, name: "high tom", hz: 200.0,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 0.45 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        vector: [0.2, 0.2], filter: [0.612, 0.1, 0.65],
        env: [[0.0, 0.308, 0.0, 0.169], [0.0, 0.134, 0.0, 0.093]],
        level: 1.6,
    },
    KeyChart {
        note: 49, name: "crash", hz: 1200.0,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.6 },
        ],
        vector: [0.5, 0.5], filter: [0.946, 0.6, 0.625],
        env: [[0.0, 0.409, 0.0, 0.333], [0.0, 0.26, 0.0, 0.158]],
        level: 1.3,
    },
    KeyChart {
        note: 51, name: "ride", hz: 1600.0,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.8 },
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 0.0, level: 0.8 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.5 },
        ],
        vector: [0.45, 0.4], filter: [0.917, 0.55, 0.625],
        env: [[0.0, 0.374, 0.0, 0.28], [0.0, 0.158, 0.0, 0.134]],
        level: 1.3,
    },
];

/// W.49 WAVE KIT: the same drums with wavetables for bodies.
///
/// Twelve recipes, and no two of them share a waveform: the digital table is
/// the kick and the hats, the clavinet is the snare, the reed and the electric
/// piano are two of the toms, the hollow table is the third, and the bell is
/// the chime and the clap. That is the point of the kit — a drum out of a
/// wavetable oscillator is a different instrument from one out of a sine, not
/// the same one re-tuned.
const WAVE_KIT: [KeyChart; 12] = [
    KeyChart {
        note: 36, name: "digi kick", hz: 55.0,
        osc: [
            OscChart { shape: 4, table: 1.0, semitones: -12, cents: 0.0, level: 0.9 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.7 },
            OscChart { shape: 4, table: 0.8, semitones: 19, cents: 0.0, level: 0.4 },
        ],
        vector: [0.2, 0.15], filter: [0.498, 0.15, 0.65],
        env: [[0.0, 0.318, 0.0, 0.158], [0.0, 0.093, 0.0, 0.093]],
        level: 1.5,
    },
    KeyChart {
        note: 38, name: "digi snare", hz: 210.0,
        osc: [
            OscChart { shape: 4, table: 0.9333, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 1.0, semitones: 7, cents: 0.0, level: 0.7 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
        ],
        vector: [0.45, 0.5], filter: [0.713, 0.4, 0.675],
        env: [[0.0, 0.247, 0.0, 0.134], [0.0, 0.158, 0.0, 0.093]],
        level: 1.6,
    },
    KeyChart {
        note: 39, name: "glass clap", hz: 520.0,
        osc: [
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 0.0, level: 0.8 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.5 },
        ],
        vector: [0.5, 0.45], filter: [0.737, 0.55, 0.7],
        env: [[0.0, 0.232, 0.0, 0.147], [0.0, 0.108, 0.0, 0.093]],
        level: 1.5,
    },
    KeyChart {
        note: 40, name: "wave snare", hz: 240.0,
        osc: [
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.5 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.8 },
        ],
        vector: [0.45, 0.45], filter: [0.699, 0.45, 0.675],
        env: [[0.0, 0.216, 0.0, 0.121], [0.0, 0.134, 0.0, 0.093]],
        level: 1.6,
    },
    KeyChart {
        note: 41, name: "hollow tom", hz: 110.0,
        osc: [
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.4 },
            OscChart { shape: 4, table: 0.3333, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        vector: [0.25, 0.2], filter: [0.566, 0.2, 0.65],
        env: [[0.0, 0.346, 0.0, 0.179], [0.0, 0.147, 0.0, 0.093]],
        level: 1.5,
    },
    KeyChart {
        note: 42, name: "digi hat", hz: 1000.0,
        osc: [
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.6 },
        ],
        vector: [0.5, 0.5], filter: [0.917, 0.68, 0.625],
        env: [[0.0, 0.1, 0.0, 0.077], [0.0, 0.059, 0.0, 0.059]],
        level: 1.5,
    },
    KeyChart {
        note: 45, name: "reed tom", hz: 165.0,
        osc: [
            OscChart { shape: 4, table: 0.5333, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.4 },
            OscChart { shape: 4, table: 0.6, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        vector: [0.25, 0.2], filter: [0.599, 0.2, 0.65],
        env: [[0.0, 0.318, 0.0, 0.179], [0.0, 0.134, 0.0, 0.093]],
        level: 1.5,
    },
    KeyChart {
        note: 46, name: "open digi hat", hz: 1000.0,
        osc: [
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.6 },
        ],
        vector: [0.5, 0.5], filter: [0.9, 0.66, 0.625],
        env: [[0.0, 0.286, 0.0, 0.158], [0.0, 0.093, 0.0, 0.077]],
        level: 1.4,
    },
    KeyChart {
        note: 48, name: "epiano tom", hz: 230.0,
        osc: [
            OscChart { shape: 4, table: 0.8667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.35 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.35 },
        ],
        vector: [0.25, 0.2], filter: [0.637, 0.2, 0.65],
        env: [[0.0, 0.297, 0.0, 0.169], [0.0, 0.121, 0.0, 0.093]],
        level: 1.5,
    },
    KeyChart {
        note: 49, name: "wave crash", hz: 1400.0,
        osc: [
            OscChart { shape: 4, table: 1.0, semitones: 0, cents: 0.0, level: 0.9 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.7 },
        ],
        vector: [0.5, 0.5], filter: [0.946, 0.58, 0.625],
        env: [[0.0, 0.424, 0.0, 0.355], [0.0, 0.28, 0.0, 0.179]],
        level: 1.2,
    },
    KeyChart {
        note: 51, name: "chime", hz: 2100.0,
        osc: [
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.5 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.3 },
            OscChart { shape: 4, table: 1.0, semitones: 24, cents: 0.0, level: 0.25 },
        ],
        vector: [0.35, 0.35], filter: [0.88, 0.3, 0.65],
        env: [[0.0, 0.551, 0.0, 0.476], [0.0, 0.28, 0.0, 0.247]],
        level: 1.2,
    },
    KeyChart {
        note: 53, name: "bell tone", hz: 2800.0,
        osc: [
            OscChart { shape: 4, table: 0.8667, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.8, semitones: 12, cents: 0.0, level: 0.6 },
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 0.4 },
            OscChart { shape: 4, table: 1.0, semitones: 19, cents: 0.0, level: 0.25 },
        ],
        vector: [0.35, 0.35], filter: [0.9, 0.28, 0.65],
        env: [[0.0, 0.476, 0.0, 0.409], [0.0, 0.247, 0.0, 0.207]],
        level: 1.2,
    },
];

/// W.50 PERC KIT: hand percussion, on the General MIDI notes.
///
/// Thirteen recipes with no pitch drop on any of them, which is the difference
/// between this kit and the other two: a struck skin bends and a struck stick
/// does not. The cowbell is two pulses a flat fifth apart, which is how every
/// analog cowbell since 1980 has been made; the shakers are noise through a
/// corner at 10 kHz and last forty milliseconds.
const PERC_KIT: [KeyChart; 13] = [
    KeyChart {
        note: 54, name: "tambourine", hz: 1500.0,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.6 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.9 },
        ],
        vector: [0.45, 0.5], filter: [0.88, 0.65, 0.65],
        env: [[0.0, 0.198, 0.0, 0.134], [0.0, 0.093, 0.0, 0.077]],
        level: 1.4,
    },
    KeyChart {
        note: 56, name: "cowbell", hz: 560.0,
        osc: [
            OscChart { shape: 1, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 1, table: 0.0, semitones: 7, cents: -30.0, level: 0.9 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.3 },
            OscChart { shape: 2, table: 0.0, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        vector: [0.35, 0.35], filter: [0.78, 0.35, 0.625],
        env: [[0.0, 0.308, 0.0, 0.198], [0.0, 0.108, 0.0, 0.093]],
        level: 1.4,
    },
    KeyChart {
        note: 60, name: "hi bongo", hz: 330.0,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: 7, cents: 0.0, level: 0.5 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 4, table: 0.4667, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        vector: [0.3, 0.3], filter: [0.699, 0.35, 0.65],
        env: [[0.0, 0.216, 0.0, 0.134], [0.0, 0.093, 0.0, 0.077]],
        level: 1.5,
    },
    KeyChart {
        note: 61, name: "low bongo", hz: 250.0,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: 7, cents: 0.0, level: 0.5 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.5 },
            OscChart { shape: 4, table: 0.4667, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        vector: [0.3, 0.3], filter: [0.667, 0.35, 0.65],
        env: [[0.0, 0.247, 0.0, 0.147], [0.0, 0.108, 0.0, 0.077]],
        level: 1.5,
    },
    KeyChart {
        note: 62, name: "mute conga", hz: 210.0,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: 5, cents: 0.0, level: 0.5 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.45 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 0.0, level: 0.4 },
        ],
        vector: [0.3, 0.3], filter: [0.625, 0.3, 0.65],
        env: [[0.0, 0.169, 0.0, 0.108], [0.0, 0.077, 0.0, 0.077]],
        level: 1.5,
    },
    KeyChart {
        note: 63, name: "open conga", hz: 190.0,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: 5, cents: 0.0, level: 0.5 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.45 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 0.0, level: 0.4 },
        ],
        vector: [0.3, 0.3], filter: [0.647, 0.3, 0.65],
        env: [[0.0, 0.328, 0.0, 0.198], [0.0, 0.134, 0.0, 0.093]],
        level: 1.5,
    },
    KeyChart {
        note: 64, name: "low conga", hz: 135.0,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: 5, cents: 0.0, level: 0.5 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.4 },
            OscChart { shape: 4, table: 0.4667, semitones: 0, cents: 0.0, level: 0.35 },
        ],
        vector: [0.28, 0.28], filter: [0.599, 0.28, 0.65],
        env: [[0.0, 0.355, 0.0, 0.216], [0.0, 0.147, 0.0, 0.093]],
        level: 1.5,
    },
    KeyChart {
        note: 69, name: "cabasa", hz: 2600.0,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
        ],
        vector: [0.5, 0.5], filter: [0.932, 0.7, 0.625],
        env: [[0.0, 0.085, 0.0, 0.059], [0.0, 0.05, 0.0, 0.059]],
        level: 1.4,
    },
    KeyChart {
        note: 70, name: "maracas", hz: 3200.0,
        osc: [
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
        ],
        vector: [0.5, 0.5], filter: [0.958, 0.62, 0.625],
        env: [[0.0, 0.108, 0.0, 0.077], [0.0, 0.059, 0.0, 0.059]],
        level: 1.4,
    },
    KeyChart {
        note: 75, name: "claves", hz: 2500.0,
        osc: [
            OscChart { shape: 3, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 2, table: 0.0, semitones: 12, cents: 0.0, level: 0.4 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.3 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.3 },
        ],
        vector: [0.3, 0.3], filter: [0.858, 0.3, 0.625],
        env: [[0.0, 0.121, 0.0, 0.093], [0.0, 0.059, 0.0, 0.059]],
        level: 1.4,
    },
    KeyChart {
        note: 76, name: "hi woodblock", hz: 1300.0,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.4667, semitones: 12, cents: 0.0, level: 0.4 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.3 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.3 },
        ],
        vector: [0.3, 0.3], filter: [0.832, 0.35, 0.625],
        env: [[0.0, 0.147, 0.0, 0.093], [0.0, 0.068, 0.0, 0.068]],
        level: 1.4,
    },
    KeyChart {
        note: 77, name: "lo woodblock", hz: 950.0,
        osc: [
            OscChart { shape: 2, table: 0.0, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 4, table: 0.4667, semitones: 12, cents: 0.0, level: 0.4 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.3 },
            OscChart { shape: 3, table: 0.0, semitones: 19, cents: 0.0, level: 0.3 },
        ],
        vector: [0.3, 0.3], filter: [0.799, 0.35, 0.625],
        env: [[0.0, 0.169, 0.0, 0.108], [0.0, 0.077, 0.0, 0.068]],
        level: 1.4,
    },
    KeyChart {
        note: 81, name: "triangle", hz: 4200.0,
        osc: [
            OscChart { shape: 4, table: 0.8, semitones: 0, cents: 0.0, level: 1.0 },
            OscChart { shape: 3, table: 0.0, semitones: 12, cents: 0.0, level: 0.4 },
            OscChart { shape: 5, table: 0.0, semitones: 0, cents: 0.0, level: 0.2 },
            OscChart { shape: 4, table: 1.0, semitones: 12, cents: 0.0, level: 0.3 },
        ],
        vector: [0.35, 0.35], filter: [0.958, 0.35, 0.625],
        env: [[0.0, 0.58, 0.0, 0.498], [0.0, 0.308, 0.0, 0.247]],
        level: 1.1,
    },
];
