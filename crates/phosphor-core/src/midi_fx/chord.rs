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

const P_ROOT: usize = 0;
const P_SCALE: usize = 1;
const P_COLOR: usize = 2;
const P_VOICING: usize = 3;
const P_SPLIT: usize = 4;
const P_BASS: usize = 5;
const P_STRUM: usize = 6;

/// The chord device's parameter table, exported for the panel.
pub const CHORD_PARAMS: [FxParamInfo; 7] = [
    FxParamInfo { name: "root", unit: "", min: 0.0, max: 11.0, default: 0.0 },
    FxParamInfo { name: "scale", unit: "", min: 0.0, max: 7.0, default: 0.0 },
    // Sevenths by default — the whole genre starts there.
    FxParamInfo { name: "color", unit: "", min: 0.0, max: 3.0, default: 1.0 },
    FxParamInfo { name: "voicing", unit: "", min: 0.0, max: 5.0, default: 0.0 },
    FxParamInfo { name: "split", unit: "", min: 24.0, max: 96.0, default: 60.0 },
    // 0 = none, 1 = root an octave down, 2 = two octaves down.
    FxParamInfo { name: "bass", unit: "", min: 0.0, max: 2.0, default: 1.0 },
    FxParamInfo { name: "strum", unit: "ms", min: 0.0, max: 60.0, default: 0.0 },
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
        let (degree, octave) = self.degree_of(key);
        let (stack, n) = self.stack(degree);
        let chord_root = self.root + stack[0] + 12 * octave;
        let (offsets, count) = self.voice(&stack[..n], stack[0]);

        // The register lock: slide the whole voicing by octaves toward
        // where the last chord sat, so consecutive chords walk.
        let target = self.last_center.unwrap_or(HOME_CENTER);
        let mut best_shift = 0i32;
        let mut best_dist = f32::MAX;
        for shift in -3..=3i32 {
            let mut sum = 0f32;
            for &o in &offsets[..count] {
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
            let bass_note = base - 12 * self.bass as i32;
            if (0..=127).contains(&bass_note) {
                notes[len] = bass_note as u8;
                len += 1;
            }
        }
        for &o in &offsets[..count] {
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
            _ => 0.0,
        }
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
}
