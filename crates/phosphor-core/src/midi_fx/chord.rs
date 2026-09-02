//! The chord device: one key becomes a voicing.
//!
//! The split keyboard is the posture: below the split, every key names a
//! scale degree and sounds that degree's chord — sevenths and up, because a
//! triad-based chord device is useless for the music this one was built
//! for; at and above the split, the keyboard is a keyboard, so the left
//! hand comps full voicings while the right plays melody.
//!
//! Chords are built by stacking diatonic thirds in the chosen scale, which
//! is what makes the qualities come out right by construction — the ii is
//! minor, the V is dominant, the vii is half-diminished — and the color
//! control decides how far up the stack to reach: triad, 7th, 9th, or the
//! full 9/11/13. A voicing then rearranges the stack (drop-2, the two
//! rootless forms, quartal, spread), and a register lock slides the result
//! toward where the last chord sat, so a progression walks instead of
//! leaping — the difference between output that sounds like a plugin and
//! output that sounds like hands.

use phosphor_plugin::MidiEvent;

use crate::fx::FxParamInfo;

use super::{MidiEffect, MidiFxContext};

/// The scales the device harmonizes in, as semitone steps from the root.
const SCALES: [[i32; 7]; 8] = [
    [0, 2, 4, 5, 7, 9, 11],  // major
    [0, 2, 3, 5, 7, 8, 10],  // natural minor
    [0, 2, 3, 5, 7, 9, 10],  // dorian
    [0, 2, 4, 5, 7, 9, 10],  // mixolydian
    [0, 2, 4, 6, 7, 9, 11],  // lydian
    [0, 1, 3, 5, 7, 8, 10],  // phrygian
    [0, 2, 3, 5, 7, 8, 11],  // harmonic minor
    [0, 2, 3, 5, 7, 9, 11],  // melodic minor
];

/// Panel names, indexed like [`SCALES`].
pub const SCALE_LABELS: [&str; 8] =
    ["major", "minor", "dorian", "mixo", "lydian", "phrygian", "harm min", "mel min"];

/// Panel names for the color steps.
pub const COLOR_LABELS: [&str; 4] = ["triad", "7th", "9th", "lush"];

/// Panel names for the voicings.
pub const VOICING_LABELS: [&str; 6] = ["close", "drop2", "rtless a", "rtless b", "quartal", "spread"];

/// Note names for the root and split readouts.
pub const NOTE_NAMES: [&str; 12] =
    ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// One chord of a factory progression: where its root sits relative to
/// the song root, the voicing above that root in semitones, and — for the
/// slash chords the idiom leans on — a bass pitch of its own.
struct ProgChord {
    root: i32,
    intervals: &'static [i32],
    bass: Option<i32>,
}

const fn pc(root: i32, intervals: &'static [i32]) -> ProgChord {
    ProgChord { root, intervals, bass: None }
}

const fn pcb(root: i32, intervals: &'static [i32], bass: i32) -> ProgChord {
    ProgChord { root, intervals, bass: Some(bass) }
}

/// The factory progressions, spelled in C and transposed by the root knob.
/// Straight from the repertoire: extended ii-V-I, the gospel sus-to-b9
/// ignition, the backdoor, the church IV-iv, a 6-2-5-1 turnaround, the
/// Thundercat half-step drop, a non-resolving neo-soul loop, and the
/// quality cycle — one root, four colors, the reason this device exists.
const PROGRESSIONS: [&[ProgChord]; 8] = [
    // 2-5-1: Dm9 → G13 → Cmaj9
    &[
        pc(2, &[0, 3, 7, 10, 14]),
        pc(7, &[0, 4, 10, 14, 21]),
        pc(0, &[0, 4, 7, 11, 14]),
    ],
    // gospel V: G9sus4 → G7b9 → Cmaj9 → C6/9
    &[
        pc(7, &[0, 5, 7, 10, 14]),
        pc(7, &[0, 4, 7, 10, 13]),
        pc(0, &[0, 4, 7, 11, 14]),
        pc(0, &[0, 4, 7, 9, 14]),
    ],
    // backdoor: Fm7 → Bb9 → Cmaj7
    &[
        pc(5, &[0, 3, 7, 10]),
        pc(10, &[0, 4, 7, 10, 14]),
        pc(0, &[0, 4, 7, 11]),
    ],
    // church IV-iv: Fmaj9 → Fm6/9 → Cmaj7/E
    &[
        pc(5, &[0, 4, 7, 11, 14]),
        pc(5, &[0, 3, 7, 9, 14]),
        pcb(0, &[0, 4, 7, 11], 4),
    ],
    // turnaround 6-2-5-1: A7b9 → Dm9 → G13sus4 → Cmaj9
    &[
        pc(9, &[0, 4, 7, 10, 13]),
        pc(2, &[0, 3, 7, 10, 14]),
        pc(7, &[0, 5, 10, 14, 21]),
        pc(0, &[0, 4, 7, 11, 14]),
    ],
    // half-step drop: Dm9 → Dbmaj7
    &[pc(2, &[0, 3, 7, 10, 14]), pc(1, &[0, 4, 7, 11])],
    // neo loop: Bbm11 → Gm9 → Gbmaj13 → Ebm11 — cycles, never cadences
    &[
        pc(10, &[0, 3, 7, 10, 14, 17]),
        pc(7, &[0, 3, 7, 10, 14]),
        pc(6, &[0, 4, 7, 11, 14, 21]),
        pc(3, &[0, 3, 7, 10, 14, 17]),
    ],
    // quality cycle: Cm9 → Cmaj9 → C9sus4 → C7b9 — park the root, move
    // the light
    &[
        pc(0, &[0, 3, 7, 10, 14]),
        pc(0, &[0, 4, 7, 11, 14]),
        pc(0, &[0, 5, 7, 10, 14]),
        pc(0, &[0, 4, 7, 10, 13]),
    ],
];

/// The chord qualities a user progression picks from — the working
/// vocabulary of the idiom, each spelled in semitones from its root.
pub const QUALITIES: [(&str, &[i32]); 16] = [
    ("maj7", &[0, 4, 7, 11]),
    ("maj9", &[0, 4, 7, 11, 14]),
    ("6/9", &[0, 4, 7, 9, 14]),
    ("maj13", &[0, 4, 7, 11, 14, 21]),
    ("m7", &[0, 3, 7, 10]),
    ("m9", &[0, 3, 7, 10, 14]),
    ("m11", &[0, 3, 7, 10, 14, 17]),
    ("m6/9", &[0, 3, 7, 9, 14]),
    ("7", &[0, 4, 7, 10]),
    ("9", &[0, 4, 7, 10, 14]),
    ("13", &[0, 4, 10, 14, 21]),
    ("9sus4", &[0, 5, 7, 10, 14]),
    ("7b9", &[0, 4, 7, 10, 13]),
    ("7#9", &[0, 4, 7, 10, 15]),
    ("mMaj7", &[0, 3, 7, 11]),
    ("dim7", &[0, 3, 6, 9]),
];

/// One chord of a user progression, in wire form: a root as semitones
/// above the song root, a quality as an index into [`QUALITIES`], and a
/// bass pitch class of its own for slash chords (`-1` = none, the chord
/// root carries the bass). Small and `Copy`, because it crosses to the
/// audio thread by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserChord {
    pub root: i8,
    pub quality: u8,
    pub bass: i8,
}

/// How many chords a user progression holds — one per white key.
pub const MAX_USER_CHORDS: usize = 7;

/// Panel names, indexed like [`PROGRESSIONS`].
pub const PROG_LABELS: [&str; 8] =
    ["2-5-1", "gospel v", "backdoor", "iv \u{2192} iv", "turnrnd", "halfstep", "neo loop", "quality"];

/// Panel names for the mode switch.
pub const MODE_LABELS: [&str; 2] = ["scale", "prog"];

const P_ROOT: usize = 0;
const P_SCALE: usize = 1;
const P_COLOR: usize = 2;
const P_VOICING: usize = 3;
const P_SPLIT: usize = 4;
const P_BASS: usize = 5;
const P_STRUM: usize = 6;
const P_MODE: usize = 7;
const P_PROG: usize = 8;

/// The chord device's parameter table, exported for the panel.
pub const CHORD_PARAMS: [FxParamInfo; 9] = [
    FxParamInfo { name: "root", unit: "", min: 0.0, max: 11.0, default: 0.0 },
    FxParamInfo { name: "scale", unit: "", min: 0.0, max: 7.0, default: 0.0 },
    // Sevenths by default — the whole genre starts there.
    FxParamInfo { name: "color", unit: "", min: 0.0, max: 3.0, default: 1.0 },
    FxParamInfo { name: "voicing", unit: "", min: 0.0, max: 5.0, default: 0.0 },
    FxParamInfo { name: "split", unit: "", min: 24.0, max: 96.0, default: 60.0 },
    // 0 = none, 1 = root an octave down, 2 = two octaves down.
    FxParamInfo { name: "bass", unit: "", min: 0.0, max: 2.0, default: 1.0 },
    FxParamInfo { name: "strum", unit: "ms", min: 0.0, max: 60.0, default: 0.0 },
    // Appended after v0.3.56 — sessions that stored seven parameters load
    // with these at their defaults, which is the old behaviour exactly.
    FxParamInfo { name: "mode", unit: "", min: 0.0, max: 1.0, default: 0.0 },
    FxParamInfo { name: "prog", unit: "", min: 0.0, max: 8.0, default: 0.0 },
];

/// The most notes one key can sound: seven stack tones, a doubled top,
/// and the bass.
const MAX_CHORD: usize = 9;

/// How many keys can be down at once in the chord zone.
const MAX_ACTIVE: usize = 16;

/// The register the lock steers voicings toward when there is no previous
/// chord to walk from — around F4, the middle of the warm zone.
const HOME_CENTER: f32 = 65.0;

/// One sounding chord: the key that asked for it and the notes it got.
#[derive(Clone, Copy)]
struct ActiveChord {
    key: u8,
    notes: [u8; MAX_CHORD],
    count: usize,
}

/// A note-on waiting out its strum delay: (samples until due, note, vel).
const MAX_STRUM_PENDING: usize = 32;

pub struct ChordDevice {
    root: i32,
    scale: usize,
    color: usize,
    voicing: usize,
    split: u8,
    bass: usize,
    strum_ms: f32,
    /// 0 = scale mode (key names a degree), 1 = progression mode (white
    /// keys walk the stored progression).
    mode: usize,
    prog: usize,
    custom: [UserChord; MAX_USER_CHORDS],
    custom_len: usize,

    active: [ActiveChord; MAX_ACTIVE],
    active_len: usize,
    /// Where the last voicing sat, for the register lock's walk.
    last_center: Option<f32>,
    strum_pending: [(i64, u8, u8); MAX_STRUM_PENDING],
    strum_len: usize,
}

impl Default for ChordDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl ChordDevice {
    #[must_use]
    pub fn new() -> Self {
        Self {
            root: 0,
            scale: 0,
            color: 1,
            voicing: 0,
            split: 60,
            bass: 1,
            strum_ms: 0.0,
            mode: 0,
            prog: 0,
            custom: [UserChord { root: 0, quality: 0, bass: -1 }; MAX_USER_CHORDS],
            custom_len: 0,
            active: [ActiveChord { key: 0, notes: [0; MAX_CHORD], count: 0 }; MAX_ACTIVE],
            active_len: 0,
            last_center: None,
            strum_pending: [(0, 0, 0); MAX_STRUM_PENDING],
            strum_len: 0,
        }
    }

    /// The scale degree a played pitch class names, snapping a chromatic
    /// note to the degree below it — the nearest thing to what was asked.
    fn degree_of(&self, note: u8) -> (usize, i32) {
        let steps = &SCALES[self.scale.min(SCALES.len() - 1)];
        let rel = (i32::from(note) - self.root).rem_euclid(12);
        let octave = (i32::from(note) - self.root).div_euclid(12);
        let mut degree = 0;
        for (d, &s) in steps.iter().enumerate() {
            if s <= rel {
                degree = d;
            }
        }
        (degree, octave)
    }

    /// The stacked-thirds pitch set for a degree, in semitones from the
    /// scale root, reaching as far up as the color asks: 3 notes for a
    /// triad, 4 for a 7th, 5 for a 9th, 7 for the full 9/11/13.
    fn stack(&self, degree: usize) -> ([i32; 7], usize) {
        let steps = &SCALES[self.scale.min(SCALES.len() - 1)];
        let count = match self.color {
            0 => 3,
            1 => 4,
            2 => 5,
            _ => 7,
        };
        let mut out = [0i32; 7];
        for (k, slot) in out.iter_mut().enumerate().take(count) {
            let pos = degree + 2 * k;
            *slot = steps[pos % 7] + 12 * (pos / 7) as i32;
        }
        (out, count)
    }

    /// Rearrange the stack into the chosen voicing, as offsets from the
    /// chord's root pitch. Returns (offsets, count, include_root).
    fn voice(&self, stack: &[i32], base: i32) -> ([i32; MAX_CHORD], usize) {
        let mut out = [0i32; MAX_CHORD];
        let n = stack.len();
        // Fixed-size: this runs on the audio thread, where a Vec is a heap
        // allocation dressed as a convenience.
        let mut rel = [0i32; 7];
        for (slot, s) in rel.iter_mut().zip(stack) {
            *slot = s - base;
        }
        let rel = &rel[..n];
        match self.voicing {
            // close: the stack as stacked.
            0 => {
                out[..n].copy_from_slice(rel);
                (out, n)
            }
            // drop2: second voice from the top falls an octave.
            1 => {
                out[..n].copy_from_slice(rel);
                if n >= 2 {
                    out[n - 2] -= 12;
                }
                (out, n)
            }
            // rootless A: 3-5-7-9; rootless B: 7-9-3-5 (an octave up on the
            // back pair). Both lean on the bass note for the root; with the
            // bass off they still speak, just floatier — that is a sound
            // too. Below a 7th there is nothing to be rootless about.
            2 | 3 => {
                if n < 4 {
                    out[..n].copy_from_slice(rel);
                    return (out, n);
                }
                if self.voicing == 2 {
                    let m = n.min(5);
                    out[..m - 1].copy_from_slice(&rel[1..m]);
                    (out, m - 1)
                } else {
                    let hi = rel.get(4).copied().unwrap_or(rel[1] + 12);
                    out[0] = rel[3];
                    out[1] = hi;
                    out[2] = rel[1] + 12;
                    out[3] = rel[2] + 12;
                    (out, 4)
                }
            }
            // quartal: fourths from the played note — the modal stack.
            4 => {
                let count = if stack.len() >= 5 { 4 } else { 3 };
                for (k, slot) in out.iter_mut().enumerate().take(count) {
                    *slot = 5 * k as i32;
                }
                (out, count)
            }
            // spread: root, 5th, 10th, and the 9th on top when the color
            // reaches it — the ballad left hand.
            _ => {
                out[0] = rel[0];
                out[1] = rel.get(2).copied().unwrap_or(7);
                out[2] = rel.get(1).copied().unwrap_or(4) + 12;
                let mut count = 3;
                if n >= 5 {
                    out[3] = rel[4] + 12;
                    count = 4;
                }
                (out, count)
            }
        }
    }

    /// Build the chord a key asks for. Returns the notes, lowest first.
    fn build(&mut self, key: u8) -> ([u8; MAX_CHORD], usize) {
        if self.mode == 1 {
            return self.build_prog(key);
        }
        let (degree, octave) = self.degree_of(key);
        let (stack, n) = self.stack(degree);
        let chord_root = self.root + stack[0] + 12 * octave;
        let (offsets, count) = self.voice(&stack[..n], stack[0]);
        self.place(chord_root, &offsets[..count], None)
    }

    /// Progression mode: the white keys walk the stored progression — C is
    /// its first chord, D its second, and so on, wrapping past the end; a
    /// black key plays the same slot as the white key below it, so a hand
    /// cannot land on nothing. The octave played does not pick the octave
    /// heard — the register lock does, exactly as in scale mode.
    fn build_prog(&mut self, key: u8) -> ([u8; MAX_CHORD], usize) {
        const WHITE_MAP: [usize; 12] = [0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6];
        // Past the factory bank sits the user's own progression, supplied
        // over a command and held here in wire form.
        if self.prog >= PROGRESSIONS.len() {
            if self.custom_len == 0 {
                // Nothing loaded: an honest silence beats a wrong chord.
                return ([0; MAX_CHORD], 0);
            }
            let slot = WHITE_MAP[usize::from(key % 12)] % self.custom_len;
            let chord = self.custom[slot];
            let (_, intervals) = QUALITIES[usize::from(chord.quality).min(QUALITIES.len() - 1)];
            let chord_root = 48 + self.root + i32::from(chord.root);
            let mut offsets = [0i32; MAX_CHORD];
            let count = intervals.len().min(MAX_CHORD);
            offsets[..count].copy_from_slice(&intervals[..count]);
            let bass_override = (chord.bass >= 0)
                .then(|| 48 + self.root + i32::from(chord.bass));
            return self.place(chord_root, &offsets[..count], bass_override);
        }
        let prog = PROGRESSIONS[self.prog.min(PROGRESSIONS.len() - 1)];
        // Pitch class → white-key index, black keys borrowing the white
        // below: C C# → 0, D D# → 1, E → 2, F F# → 3, G G# → 4, A A# → 5,
        // B → 6.
        let slot = WHITE_MAP[usize::from(key % 12)] % prog.len();
        let chord = &prog[slot];
        // Build near the middle of the keyboard; the register lock places
        // it properly from there.
        let chord_root = 48 + self.root + chord.root;
        let mut offsets = [0i32; MAX_CHORD];
        let count = chord.intervals.len().min(MAX_CHORD);
        offsets[..count].copy_from_slice(&chord.intervals[..count]);
        let bass_override = chord.bass.map(|b| 48 + self.root + b);
        self.place(chord_root, &offsets[..count], bass_override)
    }

    /// The shared back half of chord building: the register lock, the bass,
    /// the clamps, the dedup, and the memory of where the voicing sat.
    /// `bass_override` is a slash chord's own bass pitch (pre-transposed,
    /// near the build octave); without one the bass is the chord root.
    fn place(
        &mut self,
        chord_root: i32,
        offsets: &[i32],
        bass_override: Option<i32>,
    ) -> ([u8; MAX_CHORD], usize) {
        let count = offsets.len().max(1);
        // The register lock: slide the whole voicing by octaves toward
        // where the last chord sat, so consecutive chords walk.
        let target = self.last_center.unwrap_or(HOME_CENTER);
        let mut best_shift = 0i32;
        let mut best_dist = f32::MAX;
        for shift in -3..=3i32 {
            let mut sum = 0f32;
            for &o in offsets {
                sum += (chord_root + o + 12 * shift) as f32;
            }
            let center = sum / count as f32;
            let dist = (center - target).abs();
            if dist < best_dist {
                best_dist = dist;
                best_shift = shift;
            }
        }

        let mut notes = [0u8; MAX_CHORD];
        let mut len = 0;
        let base = chord_root + 12 * best_shift;
        if self.bass > 0 {
            let anchor = bass_override.map_or(base, |b| b + 12 * best_shift);
            let bass_note = anchor - 12 * self.bass as i32;
            if (0..=127).contains(&bass_note) {
                notes[len] = bass_note as u8;
                len += 1;
            }
        }
        for &o in offsets {
            let p = base + o;
            if (0..=127).contains(&p) && !notes[..len].contains(&(p as u8)) {
                notes[len] = p as u8;
                len += 1;
            }
        }
        notes[..len].sort_unstable();

        // Remember where this voicing sat (bass excluded — it is an anchor,
        // not a hand position).
        let body = &notes[usize::from(self.bass > 0).min(len)..len];
        if !body.is_empty() {
            let sum: f32 = body.iter().map(|&p| f32::from(p)).sum();
            self.last_center = Some(sum / body.len() as f32);
        }
        (notes, len)
    }

    fn drain_strum(&mut self, num_frames: u32, out: &mut Vec<MidiEvent>) {
        let mut k = 0;
        while k < self.strum_len {
            let (due, note, vel) = self.strum_pending[k];
            if due < i64::from(num_frames) {
                if out.len() < out.capacity() {
                    out.push(MidiEvent {
                        sample_offset: due.max(0) as u32,
                        status: 0x90,
                        data1: note,
                        data2: vel,
                    });
                }
                self.strum_len -= 1;
                self.strum_pending[k] = self.strum_pending[self.strum_len];
            } else {
                self.strum_pending[k].0 = due - i64::from(num_frames);
                k += 1;
            }
        }
    }
}

impl MidiEffect for ChordDevice {
    fn name(&self) -> &'static str {
        "chord"
    }

    fn init(&mut self, _sample_rate: f64, _max_block: usize) {}

    fn process(&mut self, input: &[MidiEvent], out: &mut Vec<MidiEvent>, ctx: &MidiFxContext) {
        self.drain_strum(ctx.num_frames, out);
        for ev in input {
            match ev.status & 0xF0 {
                0x90 if ev.data2 > 0 && ev.data1 < self.split => {
                    if self.active_len >= MAX_ACTIVE {
                        continue;
                    }
                    let (notes, count) = self.build(ev.data1);
                    self.active[self.active_len] =
                        ActiveChord { key: ev.data1, notes, count };
                    self.active_len += 1;
                    let strum_samples =
                        (f64::from(self.strum_ms) / 1000.0 * f64::from(ctx.sample_rate)) as i64;
                    for (k, &note) in notes[..count].iter().enumerate() {
                        let delay = strum_samples * k as i64;
                        let at = i64::from(ev.sample_offset) + delay;
                        if at < i64::from(ctx.num_frames) {
                            if out.len() < out.capacity() {
                                out.push(MidiEvent {
                                    sample_offset: at as u32,
                                    status: 0x90,
                                    data1: note,
                                    data2: ev.data2,
                                });
                            }
                        } else if self.strum_len < MAX_STRUM_PENDING {
                            self.strum_pending[self.strum_len] =
                                (at - i64::from(ctx.num_frames), note, ev.data2);
                            self.strum_len += 1;
                        }
                    }
                }
                0x90 | 0x80 if ev.data1 < self.split => {
                    // The key's off releases exactly the notes its on made.
                    if let Some(p) =
                        self.active[..self.active_len].iter().position(|a| a.key == ev.data1)
                    {
                        let chord = self.active[p];
                        self.active_len -= 1;
                        self.active[p] = self.active[self.active_len];
                        // Any of its notes still waiting in the strum book
                        // must not fire after the release.
                        let mut k = 0;
                        while k < self.strum_len {
                            if chord.notes[..chord.count].contains(&self.strum_pending[k].1) {
                                self.strum_len -= 1;
                                self.strum_pending[k] = self.strum_pending[self.strum_len];
                            } else {
                                k += 1;
                            }
                        }
                        for &note in &chord.notes[..chord.count] {
                            // Another held key may share this note; if so it
                            // keeps sounding.
                            let shared = self.active[..self.active_len]
                                .iter()
                                .any(|a| a.notes[..a.count].contains(&note));
                            if !shared && out.len() < out.capacity() {
                                out.push(MidiEvent {
                                    sample_offset: ev.sample_offset,
                                    status: 0x80,
                                    data1: note,
                                    data2: 0,
                                });
                            }
                        }
                    }
                }
                _ => {
                    // Above the split, and everything that is not a note:
                    // straight through.
                    if out.len() < out.capacity() {
                        out.push(*ev);
                    }
                }
            }
        }
    }

    fn flush(&mut self, out: &mut Vec<MidiEvent>) {
        for a in &self.active[..self.active_len] {
            for &note in &a.notes[..a.count] {
                if out.len() < out.capacity() {
                    out.push(MidiEvent { sample_offset: 0, status: 0x80, data1: note, data2: 0 });
                }
            }
        }
        self.active_len = 0;
        self.strum_len = 0;
    }

    fn reset(&mut self) {
        self.active_len = 0;
        self.strum_len = 0;
        self.last_center = None;
    }

    fn parameter_count(&self) -> usize {
        CHORD_PARAMS.len()
    }

    fn parameter_info(&self, index: usize) -> Option<FxParamInfo> {
        CHORD_PARAMS.get(index).copied()
    }

    fn get_parameter(&self, index: usize) -> f32 {
        match index {
            P_ROOT => self.root as f32,
            P_SCALE => self.scale as f32,
            P_COLOR => self.color as f32,
            P_VOICING => self.voicing as f32,
            P_SPLIT => f32::from(self.split),
            P_BASS => self.bass as f32,
            P_STRUM => self.strum_ms,
            P_MODE => self.mode as f32,
            P_PROG => self.prog as f32,
            _ => 0.0,
        }
    }

    fn set_progression(&mut self, chords: &[UserChord]) {
        let n = chords.len().min(MAX_USER_CHORDS);
        self.custom[..n].copy_from_slice(&chords[..n]);
        self.custom_len = n;
    }

    fn set_parameter(&mut self, index: usize, value: f32) {
        match index {
            P_ROOT => self.root = (value.round() as i32).clamp(0, 11),
            P_SCALE => self.scale = (value.round() as usize).min(7),
            P_COLOR => self.color = (value.round() as usize).min(3),
            P_VOICING => self.voicing = (value.round() as usize).min(5),
            P_SPLIT => self.split = (value.round() as i32).clamp(24, 96) as u8,
            P_BASS => self.bass = (value.round() as usize).min(2),
            P_STRUM => self.strum_ms = value.clamp(0.0, 60.0),
            P_MODE => self.mode = (value.round() as usize).min(1),
            P_PROG => self.prog = (value.round() as usize).min(8),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    const BLOCK: u32 = 512;

    fn on(note: u8, vel: u8) -> MidiEvent {
        MidiEvent { sample_offset: 0, status: 0x90, data1: note, data2: vel }
    }
    fn off(note: u8) -> MidiEvent {
        MidiEvent { sample_offset: 0, status: 0x80, data1: note, data2: 0 }
    }

    fn chord_for(dev: &mut ChordDevice, key: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        let ctx = MidiFxContext::bare(SR, BLOCK);
        dev.process(&[on(key, 100)], &mut out, &ctx);
        let notes: Vec<u8> =
            out.iter().filter(|e| e.status == 0x90).map(|e| e.data1).collect();
        dev.process(&[off(key)], &mut Vec::with_capacity(64), &ctx);
        notes
    }

    fn pitch_classes(notes: &[u8]) -> Vec<u8> {
        let mut pc: Vec<u8> = notes.iter().map(|n| n % 12).collect();
        pc.sort_unstable();
        pc.dedup();
        pc
    }

    /// Stacking thirds in the scale makes the qualities right by
    /// construction: in C major with 7ths, the I is maj7, the ii is m7,
    /// the V is a dominant 7, the vii is half-diminished.
    #[test]
    fn diatonic_sevenths_come_out_correct() {
        let mut dev = ChordDevice::new();
        dev.set_parameter(P_BASS, 0.0); // pitch-class checks, no doubled root
        // C, D, G, B below the split: degrees I, ii, V, vii.
        let cases: [(u8, [u8; 4]); 4] = [
            (48, [0, 4, 7, 11]),  // Cmaj7
            (50, [0, 2, 5, 9]),   // Dm7 = D F A C
            (55, [2, 5, 7, 11]),  // G7 = G B D F
            (59, [2, 5, 9, 11]),  // Bm7b5 = B D F A
        ];
        for (key, want) in cases {
            let got = pitch_classes(&chord_for(&mut dev, key));
            assert_eq!(got, want.to_vec(), "wrong quality for key {key}");
        }
    }

    /// The color knob reaches up the stack: lush on the I touches all
    /// seven tones of the scale.
    #[test]
    fn lush_reaches_the_whole_scale() {
        let mut dev = ChordDevice::new();
        dev.set_parameter(P_BASS, 0.0);
        dev.set_parameter(P_COLOR, 3.0);
        let got = pitch_classes(&chord_for(&mut dev, 48));
        assert_eq!(got, vec![0, 2, 4, 5, 7, 9, 11], "the lush stack fell short: {got:?}");
    }

    /// At and above the split the keyboard is a keyboard.
    #[test]
    fn above_the_split_notes_pass_untouched() {
        let mut dev = ChordDevice::new();
        let mut out = Vec::with_capacity(64);
        let ctx = MidiFxContext::bare(SR, BLOCK);
        dev.process(&[on(72, 90)], &mut out, &ctx);
        let ons: Vec<(u8, u8)> =
            out.iter().filter(|e| e.status == 0x90).map(|e| (e.data1, e.data2)).collect();
        assert_eq!(ons, vec![(72, 90)], "melody range was harmonized: {ons:?}");
    }

    /// A key's release takes down exactly the notes its press put up — and
    /// a note shared with another held chord keeps sounding.
    #[test]
    fn release_is_exact_and_shared_notes_survive() {
        let mut dev = ChordDevice::new();
        dev.set_parameter(P_BASS, 0.0);
        let mut out = Vec::with_capacity(64);
        let ctx = MidiFxContext::bare(SR, BLOCK);
        // C and E chords in C major share pitches (Cmaj7 = CEGB, Em7 = EGBD).
        dev.process(&[on(48, 100)], &mut out, &ctx);
        let c_notes: Vec<u8> = out.iter().filter(|e| e.status == 0x90).map(|e| e.data1).collect();
        out.clear();
        dev.process(&[on(52, 100)], &mut out, &ctx);
        let e_notes: Vec<u8> = out.iter().filter(|e| e.status == 0x90).map(|e| e.data1).collect();
        out.clear();
        // Release C: only the notes E does not also hold may go silent.
        dev.process(&[off(48)], &mut out, &ctx);
        let released: Vec<u8> = out.iter().filter(|e| e.status == 0x80).map(|e| e.data1).collect();
        for n in &released {
            assert!(c_notes.contains(n), "released a note C never made: {n}");
            assert!(!e_notes.contains(n), "released {n}, which E still holds");
        }
        out.clear();
        // Release E: everything left goes, nothing twice.
        dev.process(&[off(52)], &mut out, &ctx);
        let mut rest: Vec<u8> = out.iter().filter(|e| e.status == 0x80).map(|e| e.data1).collect();
        rest.sort_unstable();
        let mut want: Vec<u8> = e_notes.clone();
        want.sort_unstable();
        assert_eq!(rest, want, "the second release did not close the rest");
    }

    /// The register lock walks: chords played an octave apart land in the
    /// same neighbourhood, not an octave apart.
    #[test]
    fn the_register_lock_keeps_chords_in_the_pocket() {
        let mut dev = ChordDevice::new();
        dev.set_parameter(P_BASS, 0.0);
        let a = chord_for(&mut dev, 48);
        let b = chord_for(&mut dev, 36); // same degree, an octave down
        let center = |v: &[u8]| v.iter().map(|&n| f32::from(n)).sum::<f32>() / v.len() as f32;
        assert!(
            (center(&a) - center(&b)).abs() < 6.0,
            "the voicings leapt: {a:?} vs {b:?}"
        );
    }

    /// Strum rolls the chord and an early release cancels what has not
    /// fired yet — no orphan note-on after the hands are up.
    #[test]
    fn strum_rolls_and_release_cancels_the_tail() {
        let mut dev = ChordDevice::new();
        dev.set_parameter(P_BASS, 0.0);
        dev.set_parameter(P_STRUM, 40.0); // 40ms per step at 48k = 1920 samples
        let mut out = Vec::with_capacity(64);
        let ctx = MidiFxContext::bare(SR, BLOCK);
        dev.process(&[on(48, 100)], &mut out, &ctx);
        let first_block_ons = out.iter().filter(|e| e.status == 0x90).count();
        assert_eq!(first_block_ons, 1, "the strum fired more than the first note at once");
        // Release before the rest arrive.
        out.clear();
        dev.process(&[off(48)], &mut out, &ctx);
        for _ in 0..20 {
            out.clear();
            dev.process(&[], &mut out, &ctx);
            assert!(
                out.iter().all(|e| e.status != 0x90),
                "a strummed note fired after its key was released"
            );
        }
    }

    /// The wheel and the pedal ride through untouched.
    #[test]
    fn controllers_pass_through() {
        let mut dev = ChordDevice::new();
        let wheel = MidiEvent { sample_offset: 3, status: 0xB0, data1: 1, data2: 77 };
        let mut out = Vec::with_capacity(64);
        let ctx = MidiFxContext::bare(SR, BLOCK);
        dev.process(&[wheel], &mut out, &ctx);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].data2, 77);
    }

    /// flush ends every sounding chord.
    #[test]
    fn flush_closes_all_chords() {
        let mut dev = ChordDevice::new();
        let mut out = Vec::with_capacity(64);
        let ctx = MidiFxContext::bare(SR, BLOCK);
        dev.process(&[on(48, 100), on(53, 100)], &mut out, &ctx);
        let ons = out.iter().filter(|e| e.status == 0x90).count();
        out.clear();
        dev.flush(&mut out);
        assert_eq!(out.len(), ons, "flush owed one off per sounding note");
        assert!(out.iter().all(|e| e.status == 0x80));
    }

    /// The canonical chain: chord into arp. One key becomes a chord, the
    /// chord becomes a run through its tones.
    #[test]
    fn chord_into_arp_arpeggiates_the_voicing() {
        use super::super::{Arpeggiator, MidiEffect as _};
        let mut chord = ChordDevice::new();
        chord.set_parameter(P_BASS, 0.0);
        let mut arp = Arpeggiator::new();
        let ctx = MidiFxContext::bare(SR, BLOCK);
        let mut mid = Vec::with_capacity(128);
        let mut out = Vec::with_capacity(128);
        chord.process(&[on(48, 100)], &mut mid, &ctx);
        arp.process(&mid, &mut out, &ctx);
        let mut fired: Vec<u8> = Vec::new();
        for _ in 0..80 {
            mid.clear();
            out.clear();
            chord.process(&[], &mut mid, &ctx);
            arp.process(&mid, &mut out, &ctx);
            fired.extend(out.iter().filter(|e| e.status == 0x90).map(|e| e.data1));
        }
        let pc = pitch_classes(&fired);
        assert_eq!(pc, vec![0, 4, 7, 11], "the arp is not walking the chord: {pc:?}");
    }

    /// Progression mode: white keys walk the stored chords — C is the ii,
    /// D the V, E the I of the 2-5-1 — with the qualities spelled from the
    /// repertoire, not stacked from one scale.
    #[test]
    fn white_keys_walk_the_progression() {
        let mut dev = ChordDevice::new();
        dev.set_parameter(P_BASS, 0.0);
        dev.set_parameter(P_MODE, 1.0);
        let cases: [(u8, &[u8]); 3] = [
            (48, &[0, 2, 4, 5, 9]),      // Dm9: D F A C E
            (50, &[4, 5, 7, 9, 11]),     // G13, fifth omitted: G B F A E
            (52, &[0, 2, 4, 7, 11]),     // Cmaj9: C E G B D
        ];
        for (key, want) in cases {
            let got = pitch_classes(&chord_for(&mut dev, key));
            assert_eq!(got, want.to_vec(), "wrong chord under key {key}");
        }
    }

    /// A black key plays the same slot as the white key below it, and keys
    /// past the progression's end wrap to its start.
    #[test]
    fn black_keys_borrow_and_the_walk_wraps() {
        let mut dev = ChordDevice::new();
        dev.set_parameter(P_BASS, 0.0);
        dev.set_parameter(P_MODE, 1.0); // 2-5-1: three chords
        let c = pitch_classes(&chord_for(&mut dev, 48));
        let c_sharp = pitch_classes(&chord_for(&mut dev, 49));
        assert_eq!(c, c_sharp, "C# should land on C's chord");
        let f = pitch_classes(&chord_for(&mut dev, 53)); // white index 3 wraps to slot 0
        assert_eq!(f, c, "the walk should wrap past the progression's end");
    }

    /// The root knob transposes the whole progression.
    #[test]
    fn the_progression_transposes_with_the_root() {
        let mut dev = ChordDevice::new();
        dev.set_parameter(P_BASS, 0.0);
        dev.set_parameter(P_MODE, 1.0);
        let in_c = pitch_classes(&chord_for(&mut dev, 48));
        dev.reset();
        dev.set_parameter(P_ROOT, 2.0); // D
        let in_d = pitch_classes(&chord_for(&mut dev, 48));
        let mut shifted: Vec<u8> = in_c.iter().map(|p| (p + 2) % 12).collect();
        shifted.sort_unstable();
        assert_eq!(in_d, shifted, "the progression did not transpose");
    }

    /// The slash chord in the church IV-iv puts its own bass under the
    /// voicing: Cmaj7 over E.
    #[test]
    fn slash_chords_carry_their_own_bass() {
        let mut dev = ChordDevice::new();
        dev.set_parameter(P_MODE, 1.0);
        dev.set_parameter(P_PROG, 3.0); // church IV-iv, third chord = Cmaj7/E
        let notes = chord_for(&mut dev, 52); // white index 2 → slot 2
        assert!(!notes.is_empty());
        assert_eq!(notes[0] % 12, 4, "the bass should be E, got {notes:?}");
    }

    /// The quality cycle keeps one root while the color changes — every
    /// chord in it is built on C.
    #[test]
    fn the_quality_cycle_parks_the_root() {
        let mut dev = ChordDevice::new();
        dev.set_parameter(P_BASS, 0.0);
        dev.set_parameter(P_MODE, 1.0);
        dev.set_parameter(P_PROG, 7.0);
        for key in [48u8, 50, 52, 53] {
            let pcs = pitch_classes(&chord_for(&mut dev, key));
            assert!(pcs.contains(&0), "a cycle chord lost its C root: {pcs:?}");
        }
        // And the qualities genuinely differ: the ons of the first two keys
        // are not the same set.
        let a = pitch_classes(&chord_for(&mut dev, 48));
        let b = pitch_classes(&chord_for(&mut dev, 50));
        assert_ne!(a, b, "minor and major came out identical");
    }

    /// The register lock holds in progression mode: walking the 2-5-1 up
    /// the keyboard keeps every voicing in the pocket.
    #[test]
    fn progression_voicings_walk_not_leap() {
        let mut dev = ChordDevice::new();
        dev.set_parameter(P_BASS, 0.0);
        dev.set_parameter(P_MODE, 1.0);
        let center = |v: &[u8]| v.iter().map(|&n| f32::from(n)).sum::<f32>() / v.len() as f32;
        let mut last: Option<f32> = None;
        for key in [48u8, 50, 52] {
            let notes = chord_for(&mut dev, key);
            let c = center(&notes);
            if let Some(prev) = last {
                assert!((c - prev).abs() < 8.0, "the progression leapt: {c} vs {prev}");
            }
            last = Some(c);
        }
    }

    /// A user progression: loaded over the wire, walked by the white keys,
    /// with its qualities looked up from the shared dictionary.
    #[test]
    fn a_user_progression_walks_like_a_factory_one() {
        let mut dev = ChordDevice::new();
        dev.set_parameter(P_BASS, 0.0);
        dev.set_parameter(P_MODE, 1.0);
        dev.set_parameter(P_PROG, 8.0);
        // Nothing loaded yet: an honest silence.
        assert!(chord_for(&mut dev, 48).is_empty(), "an empty user slot sounded");

        // Am9 -> Fmaj9 -> G13 (quality indices 5, 1, 10), F over its 3rd.
        dev.set_progression(&[
            UserChord { root: 9, quality: 5, bass: -1 },
            UserChord { root: 5, quality: 1, bass: 9 },
            UserChord { root: 7, quality: 10, bass: -1 },
        ]);
        let am9 = pitch_classes(&chord_for(&mut dev, 48));
        assert_eq!(am9, vec![0, 4, 7, 9, 11], "Am9 = A C E G B, got {am9:?}");
        let g13 = pitch_classes(&chord_for(&mut dev, 52));
        assert_eq!(g13, vec![4, 5, 7, 9, 11], "G13 no fifth, got {g13:?}");
    }

    /// The user slash bass lands under the voicing when the bass knob is on.
    #[test]
    fn a_user_slash_bass_lands_underneath() {
        let mut dev = ChordDevice::new();
        dev.set_parameter(P_MODE, 1.0);
        dev.set_parameter(P_PROG, 8.0);
        dev.set_progression(&[UserChord { root: 5, quality: 1, bass: 9 }]); // Fmaj9/A
        let notes = chord_for(&mut dev, 48);
        assert!(!notes.is_empty());
        assert_eq!(notes[0] % 12, 9, "the bass should be A, got {notes:?}");
    }
}
