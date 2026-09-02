//! The arpeggiator: a held chord becomes a run of short notes.
//!
//! Timing lives in two clock domains and the device is honest about which
//! one it is in. Rolling, every step lands on the transport's tick grid —
//! the same grid the clips play from, so an arp line and a drawn line on
//! the same beat land on the same sample. Stopped, it free-runs on a sample
//! clock at the session tempo, because playing a chord with the transport
//! parked and hearing it arpeggiate is how everyone meets an arp.
//!
//! The held-key pool is the *input* keys, not the generated notes — the
//! rule that keeps every parameter live under a latched chord.

use phosphor_plugin::MidiEvent;

use crate::fx::FxParamInfo;
use crate::transport::Transport;

use super::{MidiEffect, MidiFxContext};

/// The synced divisions the rate knob steps through, as ticks at PPQ 960.
const RATE_TICKS: [i64; 8] = [
    Transport::PPQ,         // 1/4
    Transport::PPQ * 2 / 3, // 1/4T
    Transport::PPQ / 2,     // 1/8
    Transport::PPQ * 3 / 4, // 1/8.
    Transport::PPQ / 3,     // 1/8T
    Transport::PPQ / 4,     // 1/16
    Transport::PPQ / 6,     // 1/16T
    Transport::PPQ / 8,     // 1/32
];

/// Panel names for the rate steps, indexed like [`RATE_TICKS`].
pub const RATE_LABELS: [&str; 8] = ["1/4", "1/4T", "1/8", "1/8.", "1/8T", "1/16", "1/16T", "1/32"];

/// Panel names for the styles.
pub const STYLE_LABELS: [&str; 6] = ["up", "down", "updown", "played", "chord", "random"];

const P_STYLE: usize = 0;
const P_RATE: usize = 1;
const P_GATE: usize = 2;
const P_OCTAVES: usize = 3;
const P_LATCH: usize = 4;
const P_VEL: usize = 5;
const P_SWING: usize = 6;
const P_HUMAN: usize = 7;

/// The arp's parameter table, exported so the front end can draw a panel
/// without holding an instance.
pub const ARP_PARAMS: [FxParamInfo; 8] = [
    FxParamInfo { name: "style", unit: "", min: 0.0, max: 5.0, default: 0.0 },
    FxParamInfo { name: "rate", unit: "", min: 0.0, max: 7.0, default: 5.0 },
    FxParamInfo { name: "gate", unit: "%", min: 1.0, max: 200.0, default: 60.0 },
    FxParamInfo { name: "octaves", unit: "", min: 1.0, max: 4.0, default: 1.0 },
    FxParamInfo { name: "latch", unit: "", min: 0.0, max: 1.0, default: 0.0 },
    // 0 = as played; anything above is a fixed velocity.
    FxParamInfo { name: "vel", unit: "", min: 0.0, max: 127.0, default: 0.0 },
    FxParamInfo { name: "swing", unit: "%", min: 50.0, max: 75.0, default: 50.0 },
    // Appended after v0.3.59; old sessions load with it at zero.
    FxParamInfo { name: "human", unit: "%", min: 0.0, max: 100.0, default: 0.0 },
];

/// All ten fingers, with room for a generated chord on top.
const MAX_HELD: usize = 16;

/// Note-offs that have not come due yet. Gate over 100% overlaps steps, so
/// more than one can be pending.
const MAX_PENDING: usize = 8;

pub struct Arpeggiator {
    // ── parameters ──
    style: usize,
    rate: usize,
    gate_pct: f32,
    octaves: usize,
    latch: bool,
    vel_fixed: u8,
    swing_pct: f32,
    human_pct: f32,

    // ── the key pool ──
    /// Held input keys in the order they were played.
    held: [(u8, u8); MAX_HELD],
    held_len: usize,
    /// Keys physically down right now — differs from `held` under latch.
    down: [u8; MAX_HELD],
    down_len: usize,

    // ── the run ──
    step_index: usize,
    /// Pending note-offs: (samples until due, note).
    pending_off: [(i64, u8); MAX_PENDING],
    pending_len: usize,
    /// Free-run clock: samples until the next step, when stopped.
    free_next: i64,
    /// Whether the free clock is armed (a chord is held).
    running: bool,
    /// The grid step the last rolling fire landed on, to fire each step once.
    last_grid_step: i64,
    /// Where the last rolling block started, to see the transport jump
    /// backward — a loop wrap, a rewind, a fresh play — and let the grid
    /// tracker follow instead of starving.
    last_block_tick: i64,
    /// xorshift state for the random style. Reseeded on reset, so a
    /// committed render reproduces what was heard.
    rng: u32,
}

impl Default for Arpeggiator {
    fn default() -> Self {
        Self::new()
    }
}

impl Arpeggiator {
    #[must_use]
    pub fn new() -> Self {
        Self {
            style: 0,
            rate: 5,
            gate_pct: 60.0,
            octaves: 1,
            latch: false,
            vel_fixed: 0,
            swing_pct: 50.0,
            human_pct: 0.0,
            held: [(0, 0); MAX_HELD],
            held_len: 0,
            down: [0; MAX_HELD],
            down_len: 0,
            step_index: 0,
            pending_off: [(0, 0); MAX_PENDING],
            pending_len: 0,
            free_next: 0,
            running: false,
            last_grid_step: i64::MIN,
            last_block_tick: i64::MIN,
            rng: 0x9E37_79B9,
        }
    }

    fn step_ticks(&self) -> i64 {
        RATE_TICKS[self.rate.min(RATE_TICKS.len() - 1)]
    }

    /// The step length in samples at this block's tempo.
    fn step_samples(&self, ctx: &MidiFxContext) -> i64 {
        let beats = self.step_ticks() as f64 / Transport::PPQ as f64;
        let secs = beats * 60.0 / ctx.tempo_bpm.max(1.0);
        (secs * ctx.sample_rate as f64).max(1.0) as i64
    }

    /// How many notes one pass of the pattern visits.
    fn pattern_len(&self) -> usize {
        let base = self.held_len.max(1);
        match self.style {
            2 => (base * self.octaves * 2).saturating_sub(2).max(1), // updown, exclusive
            _ => base * self.octaves,
        }
    }

    /// The pool note the pattern's position `i` names: (note, velocity).
    fn pattern_note(&mut self, i: usize) -> (u8, u8) {
        let n = self.held_len;
        debug_assert!(n > 0);
        // The pool sorted ascending, as (note, vel) picked out of `held`.
        let mut order: [usize; MAX_HELD] = [0; MAX_HELD];
        for (k, slot) in order.iter_mut().enumerate().take(n) {
            *slot = k;
        }
        if self.style != 3 {
            // Everything except played-order works from ascending pitch.
            order[..n].sort_unstable_by_key(|&k| self.held[k].0);
        }

        let span = n * self.octaves;
        let pos = match self.style {
            1 => span - 1 - (i % span),                    // down
            2 => {
                // updown, endpoints not repeated: 0 1 2 3 2 1 | 0 1 ...
                let cycle = (span * 2).saturating_sub(2).max(1);
                let p = i % cycle;
                if p < span { p } else { cycle - p }
            }
            5 => {
                // xorshift32 — deterministic from the last reset.
                self.rng ^= self.rng << 13;
                self.rng ^= self.rng >> 17;
                self.rng ^= self.rng << 5;
                (self.rng as usize) % span
            }
            _ => i % span, // up, played
        };
        let (note, vel) = self.held[order[pos % n]];
        let octave = (pos / n) as i32;
        let shifted = (note as i32 + octave * 12).clamp(0, 127) as u8;
        (shifted, vel)
    }

    /// Fire one step at `offset` samples into the block.
    fn fire(&mut self, offset: u32, step_len: i64, sample_rate: f32, out: &mut Vec<MidiEvent>) {
        if self.held_len == 0 {
            return;
        }
        let gate = ((step_len as f64) * f64::from(self.gate_pct) / 100.0).max(1.0) as i64;
        if self.style == 4 {
            // chord: every held note at once
            for k in 0..self.held_len {
                let (note, vel) = self.held[k];
                self.emit_on(note, vel, offset, gate, sample_rate, out);
            }
        } else {
            let i = self.step_index;
            let (note, vel) = self.pattern_note(i);
            self.emit_on(note, vel, offset, gate, sample_rate, out);
        }
        self.step_index = (self.step_index + 1) % self.pattern_len();
    }

    /// The next humanize draw, -1.0..1.0.
    fn jitter(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    fn emit_on(
        &mut self,
        note: u8,
        vel: u8,
        offset: u32,
        gate: i64,
        sample_rate: f32,
        out: &mut Vec<MidiEvent>,
    ) {
        let vel = if self.vel_fixed > 0 { self.vel_fixed } else { vel };
        // Humanize: a hand drags a few milliseconds and no two hits land
        // at the same weight. Delay-only, so a step never fires before its
        // grid line; the off rides the same delay through the gate book.
        let (offset, vel) = if self.human_pct > 0.0 {
            let amt = self.human_pct / 100.0;
            let drag =
                ((self.jitter().abs() * amt * 0.010) * sample_rate).max(0.0) as u32;
            let dv = (self.jitter() * amt * 12.0).round() as i32;
            (offset + drag, (i32::from(vel) + dv).clamp(1, 127) as u8)
        } else {
            (offset, vel)
        };
        if out.len() >= out.capacity() {
            return;
        }
        out.push(MidiEvent { sample_offset: offset, status: 0x90, data1: note, data2: vel });
        if self.pending_len < MAX_PENDING {
            self.pending_off[self.pending_len] = (i64::from(offset) + gate, note);
            self.pending_len += 1;
        } else {
            // The book is full: close this note at the step's end instead of
            // letting it hang.
            if out.len() < out.capacity() {
                out.push(MidiEvent {
                    sample_offset: offset,
                    status: 0x80,
                    data1: note,
                    data2: 0,
                });
            }
        }
    }

    /// Emit any offs that come due inside this block; age the rest.
    fn drain_offs(&mut self, num_frames: u32, out: &mut Vec<MidiEvent>) {
        let mut k = 0;
        while k < self.pending_len {
            let (due, note) = self.pending_off[k];
            if due < i64::from(num_frames) {
                if out.len() < out.capacity() {
                    out.push(MidiEvent {
                        sample_offset: due.max(0) as u32,
                        status: 0x80,
                        data1: note,
                        data2: 0,
                    });
                }
                self.pending_len -= 1;
                self.pending_off[k] = self.pending_off[self.pending_len];
            } else {
                self.pending_off[k].0 = due - i64::from(num_frames);
                k += 1;
            }
        }
    }

    fn fire_steps(&mut self, _input: &[MidiEvent], out: &mut Vec<MidiEvent>, ctx: &MidiFxContext) {
        if self.held_len == 0 {
            return;
        }
        let step_len = self.step_samples(ctx);
        let swing = f64::from(self.swing_pct);
        if ctx.playing && ctx.ticks_per_sample > 0.0 {
            // The transport moved backward — a loop wrap, a rewind, play
            // pressed again from the top. The tracker resets so the steps
            // of the new pass fire; without this, one trip round a loop
            // reads as "everything already played" and the arp starves
            // while the keys are still down.
            if ctx.block_start_tick < self.last_block_tick {
                self.last_grid_step = i64::MIN;
            }
            self.last_block_tick = ctx.block_start_tick;
            // Grid-locked: fire every multiple of the step division the
            // block covers. Swing delays every second division.
            let step_ticks = self.step_ticks();
            let block_ticks = (f64::from(ctx.num_frames) * ctx.ticks_per_sample).ceil() as i64;
            let first = ctx.block_start_tick.div_euclid(step_ticks);
            let last = (ctx.block_start_tick + block_ticks).div_euclid(step_ticks);
            for g in first..=last {
                if g <= self.last_grid_step {
                    continue;
                }
                let mut tick = g * step_ticks;
                if g.rem_euclid(2) == 1 {
                    let delay = (swing / 100.0 * 2.0 - 1.0) * step_ticks as f64;
                    tick += delay as i64;
                }
                let rel = tick - ctx.block_start_tick;
                if rel < 0 {
                    // A swung step scheduled before this block began.
                    self.last_grid_step = g;
                    continue;
                }
                let offset = (rel as f64 / ctx.ticks_per_sample) as i64;
                if offset >= i64::from(ctx.num_frames) {
                    break;
                }
                self.fire(offset as u32, step_len, ctx.sample_rate, out);
                self.last_grid_step = g;
            }
        } else {
            // Free-run on the sample clock.
            if !self.running {
                return;
            }
            while self.free_next < i64::from(ctx.num_frames) {
                let offset = self.free_next.max(0) as u32;
                self.fire(offset, step_len, ctx.sample_rate, out);
                let mut advance = step_len;
                // Swing on the free clock: odd steps late, even steps early
                // by the same amount, so pairs keep their combined length.
                let toward_odd = self.step_index % 2 == 1;
                let shift = ((swing / 100.0 * 2.0 - 1.0) * step_len as f64 / 2.0) as i64;
                advance += if toward_odd { shift } else { -shift };
                self.free_next += advance.max(1);
            }
            self.free_next -= i64::from(ctx.num_frames);
        }
    }

    fn note_on(&mut self, note: u8, vel: u8) {
        // Latch with nothing physically down: a new chord replaces the old.
        if self.down_len == 0 && self.held_len > 0 && self.latch {
            self.held_len = 0;
        }
        let starting = self.held_len == 0;
        if self.down_len < MAX_HELD {
            self.down[self.down_len] = note;
            self.down_len += 1;
        }
        if self.held_len < MAX_HELD && !self.held[..self.held_len].iter().any(|h| h.0 == note) {
            self.held[self.held_len] = (note, vel);
            self.held_len += 1;
        }
        if starting {
            // Retrigger: the pattern starts over with the new chord.
            self.step_index = 0;
            self.free_next = 0;
            self.running = true;
            self.last_grid_step = i64::MIN;
            self.last_block_tick = i64::MIN;
        }
    }

    fn note_off(&mut self, note: u8) {
        if let Some(p) = self.down[..self.down_len].iter().position(|&d| d == note) {
            self.down_len -= 1;
            self.down[p] = self.down[self.down_len];
        }
        if !self.latch {
            if let Some(p) = self.held[..self.held_len].iter().position(|h| h.0 == note) {
                self.held_len -= 1;
                self.held[p] = self.held[self.held_len];
            }
            if self.held_len == 0 {
                self.running = false;
            }
        }
    }
}

impl MidiEffect for Arpeggiator {
    fn name(&self) -> &'static str {
        "arp"
    }

    fn init(&mut self, _sample_rate: f64, _max_block: usize) {}

    fn process(&mut self, input: &[MidiEvent], out: &mut Vec<MidiEvent>, ctx: &MidiFxContext) {
        // 1. The key pool follows the input; controllers pass through.
        for ev in input {
            match ev.status & 0xF0 {
                0x90 if ev.data2 > 0 => self.note_on(ev.data1, ev.data2),
                0x90 | 0x80 => self.note_off(ev.data1),
                _ => {
                    if out.len() < out.capacity() {
                        out.push(*ev);
                    }
                }
            }
        }

        // 2. Steps in this block. (The off book is drained *after* firing,
        // so a step scheduled now is aged by this block like every other —
        // drain first and every gate lands one block late.)
        self.fire_steps(input, out, ctx);

        // 3. Offs owed — from earlier steps and from this block's own.
        self.drain_offs(ctx.num_frames, out);
    }

    fn flush(&mut self, out: &mut Vec<MidiEvent>) {
        for k in 0..self.pending_len {
            let (_, note) = self.pending_off[k];
            if out.len() < out.capacity() {
                out.push(MidiEvent { sample_offset: 0, status: 0x80, data1: note, data2: 0 });
            }
        }
        self.pending_len = 0;
        self.held_len = 0;
        self.down_len = 0;
        self.running = false;
        self.step_index = 0;
    }

    fn reset(&mut self) {
        self.pending_len = 0;
        self.held_len = 0;
        self.down_len = 0;
        self.running = false;
        self.step_index = 0;
        self.free_next = 0;
        self.last_grid_step = i64::MIN;
        self.last_block_tick = i64::MIN;
        self.rng = 0x9E37_79B9;
    }

    fn parameter_count(&self) -> usize {
        ARP_PARAMS.len()
    }

    fn parameter_info(&self, index: usize) -> Option<FxParamInfo> {
        ARP_PARAMS.get(index).copied()
    }

    fn get_parameter(&self, index: usize) -> f32 {
        match index {
            P_STYLE => self.style as f32,
            P_RATE => self.rate as f32,
            P_GATE => self.gate_pct,
            P_OCTAVES => self.octaves as f32,
            P_LATCH => if self.latch { 1.0 } else { 0.0 },
            P_VEL => f32::from(self.vel_fixed),
            P_SWING => self.swing_pct,
            P_HUMAN => self.human_pct,
            _ => 0.0,
        }
    }

    fn set_parameter(&mut self, index: usize, value: f32) {
        match index {
            P_STYLE => self.style = (value.round() as usize).min(5),
            P_RATE => self.rate = (value.round() as usize).min(7),
            P_GATE => self.gate_pct = value.clamp(1.0, 200.0),
            P_OCTAVES => self.octaves = (value.round() as usize).clamp(1, 4),
            P_LATCH => {
                let on = value >= 0.5;
                if self.latch && !on && self.down_len == 0 {
                    // Turning latch off with no keys down releases the chord.
                    self.held_len = 0;
                    self.running = false;
                }
                self.latch = on;
            }
            P_VEL => self.vel_fixed = (value.round() as i32).clamp(0, 127) as u8,
            P_SWING => self.swing_pct = value.clamp(50.0, 75.0),
            P_HUMAN => self.human_pct = value.clamp(0.0, 100.0),
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

    /// Run `blocks` stopped-transport blocks, feeding `input` on the first.
    fn run_free(arp: &mut Arpeggiator, input: &[MidiEvent], blocks: usize) -> Vec<(usize, MidiEvent)> {
        let mut all = Vec::new();
        let mut out = Vec::with_capacity(super::super::MIDI_FX_EVENT_CAPACITY);
        for b in 0..blocks {
            out.clear();
            let ctx = MidiFxContext::bare(SR, BLOCK);
            arp.process(if b == 0 { input } else { &[] }, &mut out, &ctx);
            for ev in &out {
                all.push((b, *ev));
            }
        }
        all
    }

    fn ons(events: &[(usize, MidiEvent)]) -> Vec<u8> {
        events.iter().filter(|(_, e)| e.status == 0x90).map(|(_, e)| e.data1).collect()
    }

    /// A held chord arpeggiates with the transport parked, at the session
    /// tempo — the first thing anyone does with an arp.
    #[test]
    fn a_stopped_arp_still_runs() {
        let mut arp = Arpeggiator::new();
        // 1/16 at 120bpm = 0.125s = 6000 samples; 40 blocks = 20480 samples
        // = 3.41 steps, so 4 fires (one at t=0).
        let events = run_free(&mut arp, &[on(60, 100), on(64, 100), on(67, 100)], 40);
        let fired = ons(&events);
        assert_eq!(fired, vec![60, 64, 67, 60], "up order broke: {fired:?}");
        // And every on eventually gets its off.
        let offs = events.iter().filter(|(_, e)| e.status == 0x80).count();
        assert!(offs >= 3, "note-offs missing: {offs}");
    }

    /// The gate places the off at gate% of the step, not at the next step.
    #[test]
    fn the_gate_shapes_the_note() {
        let mut arp = Arpeggiator::new();
        arp.set_parameter(P_GATE, 50.0);
        let events = run_free(&mut arp, &[on(60, 100)], 12);
        // step = 6000 samples, gate 50% = 3000. The off lands in block 5
        // (samples 2560..3072) at offset 3000-2560=440.
        let the_off: Vec<_> = events.iter().filter(|(_, e)| e.status == 0x80).collect();
        assert!(!the_off.is_empty(), "no off arrived");
        let (b, e) = the_off[0];
        let abs = *b as i64 * i64::from(BLOCK) + i64::from(e.sample_offset);
        assert!((abs - 3000).abs() <= 1, "off landed at {abs}, wanted 3000");
    }

    /// updown visits the ends once per turn — 60 64 67 64 | 60 64 67 64.
    #[test]
    fn updown_does_not_stutter_at_the_ends() {
        let mut arp = Arpeggiator::new();
        arp.set_parameter(P_STYLE, 2.0);
        let events = run_free(&mut arp, &[on(60, 100), on(64, 100), on(67, 100)], 200);
        let fired = ons(&events);
        assert!(fired.len() >= 8, "not enough steps fired: {}", fired.len());
        assert_eq!(&fired[..8], &[60, 64, 67, 64, 60, 64, 67, 64], "updown order: {fired:?}");
    }

    /// Two octaves walk the pool then the pool an octave up.
    #[test]
    fn octaves_extend_the_pool() {
        let mut arp = Arpeggiator::new();
        arp.set_parameter(P_OCTAVES, 2.0);
        let events = run_free(&mut arp, &[on(60, 100), on(64, 100)], 200);
        let fired = ons(&events);
        assert!(fired.len() >= 5);
        assert_eq!(&fired[..5], &[60, 64, 72, 76, 60], "octave walk: {fired:?}");
    }

    /// Chord style pulses every held note at once.
    #[test]
    fn chord_style_pulses_the_whole_chord() {
        let mut arp = Arpeggiator::new();
        arp.set_parameter(P_STYLE, 4.0);
        let events = run_free(&mut arp, &[on(60, 100), on(64, 100), on(67, 100)], 12);
        let first_block: Vec<u8> = events
            .iter()
            .filter(|(b, e)| *b == 0 && e.status == 0x90)
            .map(|(_, e)| e.data1)
            .collect();
        assert_eq!(first_block, vec![60, 64, 67], "the chord did not pulse together");
    }

    /// Latch keeps the run going after the hands leave, and the next chord
    /// replaces the old one instead of piling onto it.
    #[test]
    fn latch_holds_and_the_next_chord_replaces() {
        let mut arp = Arpeggiator::new();
        arp.set_parameter(P_LATCH, 1.0);
        let mut all = run_free(&mut arp, &[on(60, 100), on(64, 100)], 4);
        // Lift both keys; the run must continue.
        let mut out = Vec::with_capacity(64);
        let ctx = MidiFxContext::bare(SR, BLOCK);
        arp.process(&[off(60), off(64)], &mut out, &ctx);
        for _ in 0..40 {
            out.clear();
            arp.process(&[], &mut out, &ctx);
            for e in &out {
                all.push((99, *e));
            }
        }
        let after_lift: Vec<u8> =
            all.iter().filter(|(b, e)| *b == 99 && e.status == 0x90).map(|(_, e)| e.data1).collect();
        assert!(!after_lift.is_empty(), "latch did not hold the chord");
        assert!(after_lift.iter().all(|&n| n == 60 || n == 64));

        // A new chord replaces the latched one.
        out.clear();
        arp.process(&[on(48, 90)], &mut out, &ctx);
        let mut fired = Vec::new();
        for _ in 0..60 {
            out.clear();
            arp.process(&[], &mut out, &ctx);
            fired.extend(out.iter().filter(|e| e.status == 0x90).map(|e| e.data1));
        }
        assert!(fired.iter().all(|&n| n == 48), "the old chord leaked through latch: {fired:?}");
    }

    /// Rolling, the steps land on the transport's grid — the same ticks a
    /// drawn clip line would land on.
    #[test]
    fn rolling_steps_lock_to_the_tick_grid() {
        let mut arp = Arpeggiator::new();
        let tps = 120.0 * Transport::PPQ as f64 / (60.0 * f64::from(SR));
        let mut out = Vec::with_capacity(256);
        let mut tick_of: Vec<i64> = Vec::new();
        let mut start_tick = 0i64;
        for b in 0..60 {
            out.clear();
            let ctx = MidiFxContext {
                sample_rate: SR,
                tempo_bpm: 120.0,
                playing: true,
                num_frames: BLOCK,
                block_start_tick: start_tick,
                ticks_per_sample: tps,
            };
            let input = if b == 0 { vec![on(60, 100)] } else { vec![] };
            arp.process(&input, &mut out, &ctx);
            for e in out.iter().filter(|e| e.status == 0x90) {
                tick_of.push(start_tick + (f64::from(e.sample_offset) * tps) as i64);
            }
            start_tick += (f64::from(BLOCK) * tps).round() as i64;
        }
        assert!(tick_of.len() >= 3, "not enough grid steps: {tick_of:?}");
        for t in &tick_of {
            let step = Transport::PPQ / 4; // 1/16 default
            let miss = t.rem_euclid(step).min(step - t.rem_euclid(step));
            assert!(miss <= 2, "a step missed the grid by {miss} ticks: {tick_of:?}");
        }
    }

    /// Controllers pass through; the keys themselves are consumed.
    #[test]
    fn controllers_pass_and_keys_are_eaten() {
        let mut arp = Arpeggiator::new();
        let wheel = MidiEvent { sample_offset: 7, status: 0xB0, data1: 1, data2: 88 };
        let mut out = Vec::with_capacity(64);
        let ctx = MidiFxContext::bare(SR, BLOCK);
        arp.process(&[on(60, 100), wheel], &mut out, &ctx);
        assert!(out.iter().any(|e| e.status == 0xB0 && e.data2 == 88), "the wheel was eaten");
        // The only note-on is the arp's own step at offset 0 — the raw key
        // is not doubled through.
        let raw_ons = out.iter().filter(|e| e.status == 0x90).count();
        assert_eq!(raw_ons, 1, "the held key leaked past the arp");
    }

    /// flush closes what is sounding — bypassing mid-run must not hang a
    /// note under the instrument.
    #[test]
    fn flush_closes_the_sounding_note() {
        let mut arp = Arpeggiator::new();
        let mut out = Vec::with_capacity(64);
        let ctx = MidiFxContext::bare(SR, BLOCK);
        arp.process(&[on(60, 100)], &mut out, &ctx);
        assert!(out.iter().any(|e| e.status == 0x90));
        out.clear();
        arp.flush(&mut out);
        assert!(out.iter().any(|e| e.status == 0x80 && e.data1 == 60), "flush owed an off");
        // And the run is over.
        out.clear();
        arp.process(&[], &mut out, &ctx);
        assert!(out.is_empty(), "flush did not stop the run");
    }

    /// The output buffer never grows: a full buffer drops events instead of
    /// allocating on the audio thread.
    #[test]
    fn a_full_buffer_never_grows() {
        let mut arp = Arpeggiator::new();
        arp.set_parameter(P_STYLE, 4.0); // chord: max events per step
        let mut out: Vec<MidiEvent> = Vec::with_capacity(4);
        let ctx = MidiFxContext::bare(SR, BLOCK);
        let chord: Vec<MidiEvent> = (0..10).map(|k| on(48 + k * 3, 100)).collect();
        arp.process(&chord, &mut out, &ctx);
        assert!(out.capacity() == 4, "the buffer grew on the audio thread");
        assert!(out.len() <= 4);
    }

    /// Humanize drags steps a few milliseconds and varies their weight —
    /// but never fires a step early, and never at full jitter leaves the
    /// 10ms window.
    #[test]
    fn humanize_drags_late_and_varies_weight() {
        let mut arp = Arpeggiator::new();
        arp.set_parameter(P_HUMAN, 100.0);
        let events = run_free(&mut arp, &[on(60, 100)], 200);
        let ons: Vec<(usize, u32, u8)> = events
            .iter()
            .filter(|(_, e)| e.status == 0x90)
            .map(|(b, e)| (*b, e.sample_offset, e.data2))
            .collect();
        assert!(ons.len() >= 6, "not enough steps: {}", ons.len());
        // Step k's grid line is at k*6000 absolute samples; every on must
        // land on or after it, within 10ms (480 samples at 48k).
        let mut dragged = 0;
        for (k, &(b, off, _)) in ons.iter().enumerate() {
            let abs = b as i64 * i64::from(BLOCK) + i64::from(off);
            let grid = k as i64 * 6000;
            assert!(abs >= grid, "step {k} fired early: {abs} vs {grid}");
            assert!(abs - grid <= 480, "step {k} dragged past 10ms: {}", abs - grid);
            if abs > grid {
                dragged += 1;
            }
        }
        assert!(dragged >= 3, "humanize never moved a step: {ons:?}");
        let vels: Vec<u8> = ons.iter().map(|&(_, _, v)| v).collect();
        assert!(vels.iter().any(|&v| v != vels[0]), "every hit weighed the same: {vels:?}");
        assert!(vels.iter().all(|&v| (88..=112).contains(&v)), "weight left the band: {vels:?}");
    }

    /// The field report: hold keys, and the arp stops "at a certain point".
    /// The point is the loop wrap — the grid tracker only ever moves
    /// forward, so when the transport's ticks jump back to the loop start,
    /// every step reads as already fired and the arp starves for the rest
    /// of the session.
    #[test]
    fn the_arp_survives_a_loop_wrap() {
        let mut arp = Arpeggiator::new();
        let tps = 120.0 * Transport::PPQ as f64 / (60.0 * f64::from(SR));
        let loop_len = Transport::PPQ * 4; // one bar
        let mut tick = 0i64;
        let mut fires_before = 0usize;
        let mut fires_after = 0usize;
        let mut wrapped = false;
        let mut out = Vec::with_capacity(256);
        for b in 0..400 {
            out.clear();
            let ctx = MidiFxContext {
                sample_rate: SR,
                tempo_bpm: 120.0,
                playing: true,
                num_frames: BLOCK,
                block_start_tick: tick,
                ticks_per_sample: tps,
            };
            let input = if b == 0 { vec![on(60, 100)] } else { vec![] };
            arp.process(&input, &mut out, &ctx);
            let fired = out.iter().filter(|e| e.status == 0x90).count();
            if wrapped {
                fires_after += fired;
            } else {
                fires_before += fired;
            }
            tick += (f64::from(BLOCK) * tps).round() as i64;
            if tick >= loop_len {
                tick -= loop_len;
                wrapped = true;
            }
        }
        assert!(fires_before > 4, "the arp never ran at all: {fires_before}");
        assert!(
            fires_after > 8,
            "the arp starved after the loop wrapped: {fires_after} fires in three-plus passes"
        );
    }
}
