//! Polyphonic subtractive synth with vintage character.
//!
//! Features: dual oscillators with detune, sub oscillator, noise generator,
//! resonant low-pass filter, drive/saturation, and ADSR envelope.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

use crate::level::soft_saturate;
use crate::oscillator::{Oscillator, Waveform};

const MAX_VOICES: usize = 16;

/// Fixed headroom trim on the voice sum, applied after the gain knob.
///
/// Sized on ordinary playing, in step with the other four — see `OUTPUT_TRIM`
/// in dx7.rs, which carries the full reasoning. The trim lands this synth at
/// the same typical loudness as theirs.
///
/// Measured worst case at default parameters: 1.39 for an eight-note chord
/// at velocity 127, which used to leave the buffer — this synth had no
/// output bound of any kind, so it handed values above 1.0 straight to the
/// mixer. At this trim that worst case is well under the saturator knee.
///
/// The second factor is the GAIN knob's old default. Everything above was
/// measured with that knob at 0.75, so the quarter of its travel above 0.75
/// was a boost into headroom nothing had measured — an eight-note chord with
/// DRIVE at the top reached 0.9379 with GAIN at the top, past the master
/// limiter's ceiling. Folding it in here leaves the default render identical
/// sample for sample and makes GAIN a control that can only cut, which is the
/// same reasoning the drum rack's GAIN was pinned at the top of its travel
/// for.
const OUTPUT_TRIM: f32 = 0.2481 * 0.75;

// ── Parameter indices ──
pub const P_WAVEFORM: usize = 0;
pub const P_DETUNE: usize = 1;
pub const P_SUB_LEVEL: usize = 2;
pub const P_NOISE_LEVEL: usize = 3;
pub const P_FILTER_CUTOFF: usize = 4;
pub const P_FILTER_RESO: usize = 5;
pub const P_DRIVE: usize = 6;
pub const P_ATTACK: usize = 7;
pub const P_DECAY: usize = 8;
pub const P_SUSTAIN: usize = 9;
pub const P_RELEASE: usize = 10;
pub const P_GAIN: usize = 11;
pub const PARAM_COUNT: usize = 12;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "waveform", "detune", "sub", "noise",
    "cutoff", "reso", "drive",
    "attack", "decay", "sustain", "release", "gain",
];

pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    0.25,  // waveform: saw
    0.05,  // detune: slight
    0.0,   // sub level: off
    0.0,   // noise: off
    0.8,   // filter cutoff: mostly open
    0.0,   // filter resonance: none
    0.0,   // drive: clean
    0.01,  // attack: 20ms
    0.15,  // decay
    0.7,   // sustain
    0.1,   // release: 200ms
    1.0,   // gain: the top of its travel, so the knob can only cut — see OUTPUT_TRIM
];

// ── ADSR Envelope ──

#[derive(Debug, Clone, Copy, PartialEq)]
enum EnvStage { Idle, Attack, Decay, Sustain, Release }

#[derive(Debug, Clone)]
struct Envelope {
    stage: EnvStage,
    level: f64,
    attack: f64,
    decay: f64,
    sustain: f64,
    release: f64,
    sample_rate: f64,
}

impl Envelope {
    fn new(sr: f64) -> Self {
        Self { stage: EnvStage::Idle, level: 0.0, attack: 0.005, decay: 0.1, sustain: 0.7, release: 0.15, sample_rate: sr }
    }
    fn trigger(&mut self) { self.stage = EnvStage::Attack; }
    fn release(&mut self) { if self.stage != EnvStage::Idle { self.stage = EnvStage::Release; } }
    fn kill(&mut self) { self.stage = EnvStage::Idle; self.level = 0.0; }
    fn is_active(&self) -> bool { self.stage != EnvStage::Idle }

    fn tick(&mut self) -> f64 {
        match self.stage {
            EnvStage::Idle => 0.0,
            EnvStage::Attack => {
                self.level += 1.0 / (self.attack * self.sample_rate).max(1.0);
                if self.level >= 1.0 { self.level = 1.0; self.stage = EnvStage::Decay; }
                self.level
            }
            EnvStage::Decay => {
                self.level -= (1.0 - self.sustain) / (self.decay * self.sample_rate).max(1.0);
                if self.level <= self.sustain { self.level = self.sustain; self.stage = EnvStage::Sustain; }
                self.level
            }
            EnvStage::Sustain => self.sustain,
            EnvStage::Release => {
                self.level -= self.level / (self.release * self.sample_rate).max(1.0);
                if self.level <= 0.001 { self.level = 0.0; self.stage = EnvStage::Idle; }
                self.level
            }
        }
    }
}

// ── Simple resonant low-pass filter (State Variable Filter) ──

#[derive(Debug, Clone)]
struct SvfFilter {
    low: f64,
    band: f64,
    cutoff: f64,  // 0..1 normalized
    reso: f64,    // 0..1
    sample_rate: f64,
}

impl SvfFilter {
    fn new(sr: f64) -> Self {
        Self { low: 0.0, band: 0.0, cutoff: 1.0, reso: 0.0, sample_rate: sr }
    }

    fn set(&mut self, cutoff: f64, reso: f64) {
        self.cutoff = cutoff.clamp(0.0, 1.0);
        self.reso = reso.clamp(0.0, 0.95); // cap to avoid self-oscillation blowup
    }

    fn process(&mut self, input: f64) -> f64 {
        // Map normalized cutoff (0..1) to frequency (20Hz..20kHz), exponential
        let freq = 20.0 * (1000.0f64).powf(self.cutoff);
        let f = (2.0 * std::f64::consts::PI * freq / self.sample_rate).sin();
        let f = f.clamp(0.0, 0.99); // stability
        let q = 1.0 - self.reso;

        self.low += f * self.band;
        let high = input - self.low - q * self.band;
        self.band += f * high;

        // Flush denormals
        if self.low.abs() < 1e-18 { self.low = 0.0; }
        if self.band.abs() < 1e-18 { self.band = 0.0; }

        self.low
    }

    fn reset(&mut self) {
        self.low = 0.0;
        self.band = 0.0;
    }
}

// ── Noise generator (simple LCG, deterministic) ──

#[derive(Debug, Clone)]
struct NoiseGen {
    state: u32,
}

impl NoiseGen {
    fn new() -> Self { Self { state: 12345 } }

    fn tick(&mut self) -> f32 {
        // Linear congruential generator → white noise
        self.state = self.state.wrapping_mul(1103515245).wrapping_add(12345);
        // Map to -1..1
        (self.state as i32) as f32 / i32::MAX as f32
    }
}

// ── Drive ──

/// The signal level the DRIVE knob holds still, in the units the filter hands
/// it: one voice, before the envelope, the velocity and the per-voice trim.
///
/// The knob preserves the level of a signal *at* this amplitude exactly, lifts
/// what is quieter towards it and holds down what is louder — which is what a
/// compressor does, and what makes a synth sound driven rather than just loud.
///
/// Measured against this synth rather than picked. The four waveform settings
/// put a voice at 1.01 (sine), 1.09 (triangle), 1.17 (saw) and 1.25 (square)
/// out of the filter, so a voice here *is* about 1 — and the reference sits at
/// the bottom of that band rather than in the middle of it, because holding one
/// voice's peak still is not the same as holding a chord still: everything
/// under that peak comes up towards it, and sixteen voices summed carry the
/// rise. At 1.17 an eight-note chord's peak rose 1.7 dB across the knob; here
/// it rises 0.5 dB, from 0.346 to 0.366, while its RMS gains 3.6 dB.
///
/// What it costs is at the other end: a patch whose resonance puts it far above
/// the reference is held down rather than driven. The panel at its extremes —
/// sub, noise, cutoff and resonance all at the top — leaves the filter at 8.1,
/// 18 dB above this, and the knob takes 4.6 dB of RMS off it across its travel.
/// That patch is 0.947 out of the instrument at DRIVE zero, already past the
/// master limiter's ceiling, so what the knob does to it is the direction that
/// helps.
const DRIVE_REFERENCE: f64 = 1.0;

/// How hard the knob drives at the top of its travel.
///
/// The curve lifts a signal far below the reference by `1 + DRIVE_DEPTH *
/// DRIVE_REFERENCE`, which at a reference of 1 is exactly the 11x the
/// uncompensated knob this replaced applied to everything. Same low-level
/// gain, so a patch saved with DRIVE up keeps its character; what is gone is
/// the level that came with it.
const DRIVE_DEPTH: f64 = 10.0;

/// The DRIVE knob's waveshaper: harmonics without level.
///
/// ```text
/// a = amount * DRIVE_DEPTH
/// y = x * (1 + a*r) / (1 + a*|x|),   r = DRIVE_REFERENCE
/// ```
///
/// The same curve the drum rack's DRIVE uses, and for the same four reasons:
///
/// * **Identity at zero.** `a = 0` gives `y = x` exactly, in f64 and in f32,
///   so the bottom of the knob is the patch as voiced. The curve this replaced
///   needed a `drive < 0.01` branch to manage that, and stepped quiet signal by
///   0.83 dB the moment the knob left it.
/// * **Level-preserving at the reference.** `|x| = r` gives `|y| = r` for
///   every `a`. The old curve multiplied by up to 11 before its own
///   denominator, so the knob was a level control as much as a tone control:
///   an eight-note chord at velocity 127 went from 0.346 to 0.901 across its
///   travel, +8.3 dB, and past the master limiter's ceiling with the rest of
///   the panel untouched.
/// * **Monotonic and odd.** `dy/dx = (1 + a*r)/(1 + a|x|)^2 > 0`, so the curve
///   never folds back, and `y(-x) = -y(x)`, so it makes odd harmonics rather
///   than a DC offset. The old curve was not even monotonic *in the knob*: the
///   panel at its extremes measured 0.947 at drive 0 and 0.888 at drive 0.1,
///   because past its own knee it took more level off than the knob put on.
/// * **Bounded.** `|y| < r + 1/a` for `a > 0`, where the old curve grew
///   without limit as `sqrt(x)`.
#[inline]
fn drive_stage(x: f64, amount: f64) -> f64 {
    let a = amount * DRIVE_DEPTH;
    x * (1.0 + a * DRIVE_REFERENCE) / (1.0 + a * x.abs())
}

// ── Voice ──

#[derive(Debug)]
struct Voice {
    osc1: Oscillator,
    osc2: Oscillator,   // detuned copy
    sub_osc: Oscillator, // sub oscillator (one octave down, sine)
    env: Envelope,
    filter: SvfFilter,
    noise: NoiseGen,
    note: u8,
    velocity: f32,
    age: u64,
}

impl Voice {
    fn new(sr: f64) -> Self {
        Self {
            osc1: Oscillator::new(Waveform::Saw, 440.0, sr),
            osc2: Oscillator::new(Waveform::Saw, 440.0, sr),
            sub_osc: Oscillator::new(Waveform::Sine, 220.0, sr),
            env: Envelope::new(sr),
            filter: SvfFilter::new(sr),
            noise: NoiseGen::new(),
            note: 255,
            velocity: 0.0,
            age: 0,
        }
    }

    fn note_on(&mut self, note: u8, vel: u8, waveform: Waveform, detune_cents: f64, age: u64) {
        self.note = note;
        self.velocity = vel as f32 / 127.0;
        self.age = age;

        let freq = note_to_freq(note);
        let detune_ratio = 2.0f64.powf(detune_cents / 1200.0);

        self.osc1.set_frequency(freq);
        self.osc1.set_waveform(waveform);
        self.osc1.reset();

        self.osc2.set_frequency(freq * detune_ratio);
        self.osc2.set_waveform(waveform);
        self.osc2.reset();

        self.sub_osc.set_frequency(freq * 0.5); // one octave down
        self.sub_osc.set_waveform(Waveform::Sine);
        self.sub_osc.reset();

        self.filter.reset();
        self.env.trigger();
    }

    fn note_off(&mut self) { self.env.release(); }

    fn kill(&mut self) { self.env.kill(); self.note = 255; self.filter.reset(); }

    fn is_sounding(&self) -> bool { self.env.is_active() }
    fn is_held(&self) -> bool {
        matches!(self.env.stage, EnvStage::Attack | EnvStage::Decay | EnvStage::Sustain)
    }

    fn tick(&mut self, sub_level: f32, noise_level: f32, cutoff: f64, reso: f64, drive: f64) -> f32 {
        if !self.is_sounding() { return 0.0; }

        let env = self.env.tick() as f32;

        // Oscillator mix
        let mut s1 = [0.0f32; 1];
        let mut s2 = [0.0f32; 1];
        let mut ss = [0.0f32; 1];
        self.osc1.process(&mut s1);
        self.osc2.process(&mut s2);
        self.sub_osc.process(&mut ss);

        let osc_mix = (s1[0] + s2[0]) * 0.5; // blend both oscillators
        let sub = ss[0] * sub_level;
        let noise = self.noise.tick() * noise_level;

        let raw = osc_mix + sub + noise;

        // Filter (envelope modulates cutoff slightly)
        let env_cutoff = cutoff + (env as f64 - 0.5) * 0.2; // subtle envelope → filter
        self.filter.set(env_cutoff.clamp(0.0, 1.0), reso);
        let filtered = self.filter.process(raw as f64);

        // Drive / saturation
        let driven = drive_stage(filtered, drive);

        (driven as f32) * env * self.velocity * 0.25
    }
}

// ── PhosphorSynth ──

pub struct PhosphorSynth {
    voices: Vec<Voice>,
    sample_rate: f64,
    pub waveform: Waveform,
    pub params: [f32; PARAM_COUNT],
    voice_counter: u64,
}

impl PhosphorSynth {
    pub fn new() -> Self {
        Self {
            voices: Vec::new(),
            sample_rate: 44100.0,
            waveform: Waveform::Saw,
            params: PARAM_DEFAULTS,
            voice_counter: 0,
        }
    }

    fn next_age(&mut self) -> u64 { self.voice_counter += 1; self.voice_counter }

    fn allocate_voice(&mut self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| !v.is_sounding()) { return i; }
        if let Some((i, _)) = self.voices.iter().enumerate()
            .filter(|(_, v)| !v.is_held()).min_by_key(|(_, v)| v.age) { return i; }
        self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap_or(0)
    }

    fn release_note(&mut self, note: u8) {
        for v in &mut self.voices { if v.note == note && v.is_held() { v.note_off(); } }
    }

    fn kill_all_voices(&mut self) {
        for v in &mut self.voices { v.kill(); }
    }

    fn waveform_from_param(val: f32) -> Waveform {
        match (val * 4.0) as u8 {
            0 => Waveform::Sine,
            1 => Waveform::Saw,
            2 => Waveform::Square,
            _ => Waveform::Triangle,
        }
    }

    fn apply_params(&mut self) {
        self.waveform = Self::waveform_from_param(self.params[P_WAVEFORM]);
        for v in &mut self.voices {
            v.env.attack = self.params[P_ATTACK] as f64 * 2.0;
            v.env.decay = self.params[P_DECAY] as f64 * 2.0;
            v.env.sustain = self.params[P_SUSTAIN] as f64;
            v.env.release = self.params[P_RELEASE] as f64 * 2.0;
        }
    }

    /// Detune in cents (0..1 → 0..50 cents).
    fn detune_cents(&self) -> f64 { self.params[P_DETUNE] as f64 * 50.0 }
}

impl Default for PhosphorSynth {
    fn default() -> Self { Self::new() }
}

impl Plugin for PhosphorSynth {
    fn info(&self) -> PluginInfo {
        PluginInfo { name: "Phosphor Synth".into(), version: "0.2.0".into(), author: "Phosphor".into(), category: PluginCategory::Instrument }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..MAX_VOICES).map(|_| Voice::new(sample_rate)).collect();
        self.apply_params();
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() { return; }

        let buf_len = outputs[0].len();
        let gain = self.params[P_GAIN] * OUTPUT_TRIM;
        let waveform = self.waveform;
        let detune = self.detune_cents();
        let sub_level = self.params[P_SUB_LEVEL];
        let noise_level = self.params[P_NOISE_LEVEL];
        let cutoff = self.params[P_FILTER_CUTOFF] as f64;
        let reso = self.params[P_FILTER_RESO] as f64;
        let drive = self.params[P_DRIVE] as f64;

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
                match ev.status & 0xF0 {
                    0x90 => {
                        if ev.data2 > 0 {
                            self.release_note(ev.data1);
                            let age = self.next_age();
                            let idx = self.allocate_voice();
                            self.voices[idx].note_on(ev.data1, ev.data2, waveform, detune, age);
                        } else { self.release_note(ev.data1); }
                    }
                    0x80 => self.release_note(ev.data1),
                    0xB0 => match ev.data1 {
                        120 => self.kill_all_voices(),
                        123 => { for v in &mut self.voices { if v.is_held() { v.note_off(); } } }
                        _ => {}
                    }
                    _ => {}
                }
                ei += 1;
            }

            let mut sample = 0.0f32;
            for v in &mut self.voices {
                sample += v.tick(sub_level, noise_level, cutoff, reso, drive);
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
            min: 0.0, max: 1.0,
            default: PARAM_DEFAULTS[index],
            unit: match index {
                P_ATTACK | P_DECAY | P_RELEASE => "s".into(),
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
            self.apply_params();
        }
    }

    fn reset(&mut self) { self.kill_all_voices(); self.voice_counter = 0; }
}

fn note_to_freq(note: u8) -> f64 {
    440.0 * 2.0f64.powf((note as f64 - 69.0) / 12.0)
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
    fn cc(cc: u8, val: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0xB0, data1: cc, data2: val }
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

    #[test]
    fn silence_with_no_input() {
        let mut s = PhosphorSynth::new(); s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = PhosphorSynth::new(); s.init(44100.0, 64);
        // 200 buffers, not 4: the output carries a headroom trim, so five
        // milliseconds of attack is not enough signal to tell "sounding" from
        // "silent" with any margin.
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 200);
        assert!(out.iter().map(|v| v.abs()).fold(0.0f32, f32::max) > 0.005);
    }

    #[test]
    fn silent_after_release() {
        let mut s = PhosphorSynth::new(); s.init(44100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[note_off(60, 0)], 500);
        let out = process_buffers(&mut s, &[], 1);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak < 0.001, "peak={peak}");
    }

    #[test]
    fn output_is_finite() {
        let mut s = PhosphorSynth::new(); s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 1000);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn polyphony() {
        let mut s = PhosphorSynth::new(); s.init(44100.0, 64);
        let events = [note_on(60, 100, 0), note_on(64, 100, 0), note_on(67, 100, 0)];
        let out = process_buffers(&mut s, &events, 200);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.005 && peak < 1.0, "peak={peak}");
    }

    #[test]
    fn sample_accurate_midi() {
        let mut s = PhosphorSynth::new(); s.init(44100.0, 128);
        let mut out = vec![0.0f32; 128];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 64)]);
        // Before the offset no voice is sounding at all, so the sum is exactly
        // zero — a stronger claim than "quiet", and one that does not move
        // when the output trim does.
        assert!(out[..64].iter().all(|&v| v == 0.0));
        assert!(out[64..].iter().map(|v| v.abs()).fold(0.0f32, f32::max) > 0.0001);
    }

    #[test]
    fn waveform_change() {
        let mut s = PhosphorSynth::new(); s.init(44100.0, 64);
        s.set_parameter(P_WAVEFORM, 0.0);
        assert_eq!(s.waveform, Waveform::Sine);
        s.set_parameter(P_WAVEFORM, 0.5);
        assert_eq!(s.waveform, Waveform::Square);
    }

    #[test]
    fn filter_cutoff_affects_sound() {
        let mut s = PhosphorSynth::new(); s.init(44100.0, 64);
        // Full cutoff
        s.set_parameter(P_FILTER_CUTOFF, 1.0);
        let bright = process_buffers(&mut s, &[note_on(60, 100, 0)], 4);
        let bright_energy: f32 = bright.iter().map(|v| v * v).sum();

        s.reset(); s.init(44100.0, 64);
        // Low cutoff
        s.set_parameter(P_FILTER_CUTOFF, 0.1);
        let dark = process_buffers(&mut s, &[note_on(60, 100, 0)], 4);
        let dark_energy: f32 = dark.iter().map(|v| v * v).sum();

        assert!(bright_energy > dark_energy * 1.5, "Filter should reduce energy: bright={bright_energy} dark={dark_energy}");
    }

    #[test]
    fn drive_affects_sound() {
        let mut s = PhosphorSynth::new(); s.init(44100.0, 64);
        s.set_parameter(P_DRIVE, 0.0);
        let clean = process_buffers(&mut s, &[note_on(60, 100, 0)], 4);

        s.reset(); s.init(44100.0, 64);
        s.set_parameter(P_DRIVE, 0.8);
        let dirty = process_buffers(&mut s, &[note_on(60, 100, 0)], 4);

        // Driven signal should differ from clean
        let diff: f32 = clean.iter().zip(dirty.iter()).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 0.1, "Drive should change the signal, diff={diff}");
    }

    // ── DRIVE ──

    /// The property that makes the knob safe to leave alone: at the bottom of
    /// its travel it is not in the signal path at all, bit for bit. The panel
    /// the instrument loads with has it there, and every render of that panel
    /// has to stay what it was.
    #[test]
    fn the_drive_stage_is_the_identity_at_zero() {
        assert_eq!(PARAM_DEFAULTS[P_DRIVE], 0.0, "the default panel now drives");
        let mut x = -8.0f64;
        while x <= 8.0 {
            assert_eq!(drive_stage(x, 0.0).to_bits(), x.to_bits(), "drive 0 altered {x}");
            x += 0.001;
        }
        // Including the values a bit pattern comparison is fussy about.
        for x in [0.0f64, -0.0, f64::MIN_POSITIVE, -f64::MIN_POSITIVE, 1e-300, -1e-300] {
            assert_eq!(drive_stage(x, 0.0).to_bits(), x.to_bits(), "drive 0 altered {x:e}");
        }
        assert_eq!(drive_stage(0.0, 1.0), 0.0);
    }

    /// The property that makes it a tone control rather than a fader: a signal
    /// at the reference comes out at the reference, whatever the knob says.
    /// Before this the curve multiplied by up to 11 first, so the knob added
    /// 8.3 dB to a chord on its way past the master limiter's ceiling.
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

    /// Monotonic, so the curve never folds back, and bounded, where the curve
    /// it replaced grew as `sqrt(x)` without limit.
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
            // Nothing, however loud, gets past the asymptote.
            assert!(drive_stage(1e9, amount) <= bound);
        }
    }

    /// The knob has to be monotonic *in the knob* as well as in the signal.
    /// The curve it replaced was not: it multiplied before its own denominator,
    /// so on a signal past that denominator's knee turning DRIVE up from zero
    /// made the patch quieter, then louder again.
    #[test]
    fn turning_the_drive_knob_up_never_steps_the_level_down() {
        // A signal well above the reference, which is where the old curve
        // reversed, and one below it, which is where a drive is supposed to add.
        for x in [0.25f64, 1.0, 2.0, 8.0] {
            let mut previous = drive_stage(x, 0.0);
            let mut amount = 0.0f64;
            while amount <= 1.0 {
                amount += 0.005;
                let y = drive_stage(x, amount);
                // Above the reference the knob compresses, below it the knob
                // lifts; either way it moves one way and keeps going.
                let moving_up = x <= DRIVE_REFERENCE;
                assert!(
                    if moving_up { y >= previous } else { y <= previous },
                    "the knob reversed on {x} at amount {amount}: {y} against {previous}"
                );
                previous = y;
            }
        }
    }

    // ── Headroom ──

    /// The GAIN knob's travel above its default was headroom nothing had
    /// measured, so the default was folded into [`OUTPUT_TRIM`] and the knob
    /// pinned at the top. What makes that fold free is that the output stage
    /// multiplies knob by trim, and the two ways of bracketing the same product
    /// land on the same f32 — so the render at the new default is the old one
    /// sample for sample rather than merely close to it.
    #[test]
    fn folding_the_gain_default_into_the_trim_did_not_move_the_default_render() {
        /// The trim as it stood when GAIN defaulted to 0.75.
        const TRIM_BEFORE_THE_FOLD: f32 = 0.2481;

        assert_eq!(PARAM_DEFAULTS[P_GAIN], 1.0, "GAIN has travel above its default again");
        assert_eq!(
            (PARAM_DEFAULTS[P_GAIN] * OUTPUT_TRIM).to_bits(),
            (0.75f32 * TRIM_BEFORE_THE_FOLD).to_bits(),
            "the fold moved the output stage: {} against {}",
            PARAM_DEFAULTS[P_GAIN] * OUTPUT_TRIM,
            0.75f32 * TRIM_BEFORE_THE_FOLD
        );
    }

    #[test]
    fn retrigger_doesnt_leak() {
        let mut s = PhosphorSynth::new(); s.init(44100.0, 64);
        for _ in 0..100 { process_buffers(&mut s, &[note_on(60, 100, 0)], 1); }
        process_buffers(&mut s, &[note_off(60, 0)], 800);
        let out = process_buffers(&mut s, &[], 1);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak < 0.001, "peak={peak}");
    }

    #[test]
    fn cc120_kills_all() {
        let mut s = PhosphorSynth::new(); s.init(44100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[cc(120, 0, 0)], 1);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn all_params_readable() {
        let s = PhosphorSynth::new();
        assert_eq!(s.parameter_count(), PARAM_COUNT);
        for i in 0..PARAM_COUNT {
            assert!(s.parameter_info(i).is_some());
            let val = s.get_parameter(i);
            assert!(val >= 0.0 && val <= 1.0, "param {i} = {val}");
        }
    }
}
