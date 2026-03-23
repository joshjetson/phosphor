//! Drum rack — synthesized drum machines (808, 909, 707, 606).
//!
//! 88 sounds per kit across the full MIDI note range (24-111).
//! All sounds synthesized using 5 engines: sine body, noise burst,
//! click, metallic, and distorted sine. Each kit has a distinct
//! character table that parameterizes the engines differently.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

// ── Parameters ──
pub const P_KIT: usize = 0;
pub const P_DECAY: usize = 1;
pub const P_TONE: usize = 2;
pub const P_NOISE: usize = 3;
pub const P_DRIVE: usize = 4;
pub const P_GAIN: usize = 5;
pub const PARAM_COUNT: usize = 6;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "kit", "decay", "tone", "noise", "drive", "gain",
];

pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    0.0, 0.5, 0.5, 0.5, 0.0, 0.75,
];

// ── Kit definitions ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrumKit { Kit808, Kit909, Kit707, Kit606 }

impl DrumKit {
    pub fn from_param(val: f32) -> Self {
        match (val * 4.0) as u8 {
            0 => Self::Kit808, 1 => Self::Kit909,
            2 => Self::Kit707, _ => Self::Kit606,
        }
    }
    pub fn label(self) -> &'static str {
        match self { Self::Kit808 => "808", Self::Kit909 => "909",
                     Self::Kit707 => "707", Self::Kit606 => "606" }
    }
}

// ── Synthesis engine types ──

#[derive(Debug, Clone, Copy)]
enum Engine {
    /// Pitched sine with pitch sweep (kicks, toms, bass drums)
    SineBody,
    /// Band-passed noise burst (snares, hats, crashes, shakers)
    NoiseBurst,
    /// Very short impulse click (rims, claves, sticks, blocks)
    Click,
    /// Two detuned sines for metallic tone (cowbell, ride bell, agogo)
    Metallic,
    /// Waveshaped/clipped sine for gritty sounds (909 kick, dist perc)
    Distorted,
}

/// Parameters for one drum sound.
#[derive(Debug, Clone, Copy)]
struct SoundParams {
    engine: Engine,
    /// Secondary engine mixed in (optional)
    engine2: Option<Engine>,
    mix: f32,          // balance between engine1 and engine2 (0=all e1, 1=all e2)
    freq: f64,         // base frequency Hz
    freq2: f64,        // secondary frequency (metallic, or noise filter)
    sweep: f64,        // pitch sweep multiplier (0=none)
    sweep_speed: f64,  // how fast the sweep decays
    decay: f64,        // envelope decay time in seconds
    noise_freq: f64,   // noise bandpass center frequency
    drive: f64,        // distortion amount (0-1)
    attack: f64,       // attack click amount (0-1)
}

impl Default for SoundParams {
    fn default() -> Self {
        Self {
            engine: Engine::Click, engine2: None, mix: 0.0,
            freq: 1000.0, freq2: 0.0, sweep: 0.0, sweep_speed: 30.0,
            decay: 0.05, noise_freq: 8000.0, drive: 0.0, attack: 0.0,
        }
    }
}

/// Build the sound table for a kit. Returns params for MIDI notes 0-127.
fn build_kit(kit: DrumKit) -> [SoundParams; 128] {
    let mut table = [SoundParams::default(); 128];

    // Kit-specific base values
    let (kick_f, kick_sw, kick_dec, kick_drv) = match kit {
        DrumKit::Kit808 => (42.0, 9.0, 0.40, 0.0),
        DrumKit::Kit909 => (58.0, 5.0, 0.22, 0.35),
        DrumKit::Kit707 => (52.0, 6.5, 0.28, 0.1),
        DrumKit::Kit606 => (65.0, 3.5, 0.15, 0.2),
    };
    let (snr_f, snr_nz, snr_dec) = match kit {
        DrumKit::Kit808 => (175.0, 0.55, 0.18),
        DrumKit::Kit909 => (200.0, 0.70, 0.14),
        DrumKit::Kit707 => (185.0, 0.50, 0.12),
        DrumKit::Kit606 => (210.0, 0.65, 0.10),
    };
    let (hat_f, hat_nf) = match kit {
        DrumKit::Kit808 => (320.0, 9500.0),
        DrumKit::Kit909 => (400.0, 11000.0),
        DrumKit::Kit707 => (350.0, 8000.0),
        DrumKit::Kit606 => (420.0, 12000.0),
    };

    // ── Kicks (24-35) — 12 variations from deep sub to tight punch ──
    for i in 0..12 {
        let n = 24 + i;
        let variation = i as f64 / 11.0;
        table[n] = SoundParams {
            engine: if kit == DrumKit::Kit909 { Engine::Distorted } else { Engine::SineBody },
            engine2: Some(Engine::Click),
            mix: 0.05 + variation as f32 * 0.1,
            freq: kick_f * (0.6 + variation * 0.8),
            sweep: kick_sw * (1.2 - variation * 0.6),
            sweep_speed: 25.0 + variation * 20.0,
            decay: kick_dec * (1.5 - variation * 0.8),
            drive: kick_drv + variation * 0.15,
            attack: 0.3 + variation * 0.4,
            ..Default::default()
        };
    }

    // ── Snares (36-43) — 8 from tight to loose, varying noise ──
    for i in 0..8 {
        let n = 36 + i;
        let v = i as f64 / 7.0;
        table[n] = SoundParams {
            engine: Engine::SineBody,
            engine2: Some(Engine::NoiseBurst),
            mix: (snr_nz as f32) + v as f32 * 0.15,
            freq: snr_f + v * 40.0,
            sweep: 1.5 + v,
            sweep_speed: 20.0,
            decay: snr_dec * (0.7 + v * 0.6),
            noise_freq: 4000.0 + v * 4000.0,
            attack: 0.6,
            ..Default::default()
        };
    }

    // ── Claps & snaps (44-47) ──
    for i in 0..4 {
        let n = 44 + i;
        let v = i as f64 / 3.0;
        table[n] = SoundParams {
            engine: Engine::NoiseBurst,
            engine2: Some(Engine::Click),
            mix: 0.1,
            freq: 1000.0 + v * 500.0,
            noise_freq: 2500.0 + v * 2000.0,
            decay: 0.04 + v * 0.04,
            attack: 0.9,
            ..Default::default()
        };
    }

    // ── Closed hats (48-55) — 8 from tight to sizzle ──
    for i in 0..8 {
        let n = 48 + i;
        let v = i as f64 / 7.0;
        table[n] = SoundParams {
            engine: Engine::Metallic,
            engine2: Some(Engine::NoiseBurst),
            mix: 0.6 + v as f32 * 0.2,
            freq: hat_f + v * 100.0,
            freq2: hat_f * 1.414 + v * 80.0,
            noise_freq: hat_nf + v * 2000.0,
            decay: 0.015 + v * 0.035,
            ..Default::default()
        };
    }

    // ── Open hats (56-63) — 8 from short to washy ──
    for i in 0..8 {
        let n = 56 + i;
        let v = i as f64 / 7.0;
        table[n] = SoundParams {
            engine: Engine::Metallic,
            engine2: Some(Engine::NoiseBurst),
            mix: 0.65 + v as f32 * 0.15,
            freq: hat_f + v * 80.0,
            freq2: hat_f * 1.414 + v * 60.0,
            noise_freq: hat_nf + v * 1500.0,
            decay: 0.08 + v * 0.35,
            ..Default::default()
        };
    }

    // ── Toms (64-75) — 12 from low floor to high rack ──
    for i in 0..12 {
        let n = 64 + i;
        let v = i as f64 / 11.0;
        let base = match kit {
            DrumKit::Kit808 => 70.0,
            DrumKit::Kit909 => 85.0,
            DrumKit::Kit707 => 75.0,
            DrumKit::Kit606 => 90.0,
        };
        table[n] = SoundParams {
            engine: Engine::SineBody,
            engine2: Some(Engine::NoiseBurst),
            mix: 0.12 + v as f32 * 0.08,
            freq: base + v * 200.0,
            sweep: 2.5 - v * 1.0,
            sweep_speed: 18.0 + v * 10.0,
            decay: 0.20 - v * 0.08,
            noise_freq: 3000.0 + v * 2000.0,
            attack: 0.4,
            ..Default::default()
        };
    }

    // ── Cymbals & rides (76-83) — crash to ride to splash ──
    for i in 0..8 {
        let n = 76 + i;
        let v = i as f64 / 7.0;
        table[n] = SoundParams {
            engine: Engine::Metallic,
            engine2: Some(Engine::NoiseBurst),
            mix: 0.75,
            freq: 300.0 + v * 200.0,
            freq2: 420.0 + v * 280.0,
            noise_freq: 6000.0 + v * 4000.0,
            decay: 0.5 - v * 0.25,
            ..Default::default()
        };
    }

    // ── Percussion (84-95) — rims, blocks, claves, sticks, shakers ──
    for i in 0..12 {
        let n = 84 + i;
        let v = i as f64 / 11.0;
        let eng = if i < 4 { Engine::Click }
            else if i < 8 { Engine::Metallic }
            else { Engine::NoiseBurst };
        table[n] = SoundParams {
            engine: eng,
            freq: 400.0 + v * 3000.0,
            freq2: 600.0 + v * 2000.0,
            decay: 0.008 + v * 0.04,
            noise_freq: 5000.0 + v * 5000.0,
            attack: 0.8 - v * 0.3,
            ..Default::default()
        };
    }

    // ── FX sounds (96-111) — sweeps, zaps, noise textures, risers ──
    for i in 0..16 {
        let n = 96 + i;
        let v = i as f64 / 15.0;
        let eng = match i % 4 {
            0 => Engine::SineBody,
            1 => Engine::Distorted,
            2 => Engine::NoiseBurst,
            _ => Engine::Metallic,
        };
        table[n] = SoundParams {
            engine: eng,
            engine2: if i % 3 == 0 { Some(Engine::NoiseBurst) } else { None },
            mix: 0.3 + v as f32 * 0.3,
            freq: 100.0 + v * 2000.0,
            freq2: 200.0 + v * 1500.0,
            sweep: v * 15.0,
            sweep_speed: 5.0 + v * 40.0,
            decay: 0.05 + v * 0.4,
            noise_freq: 2000.0 + v * 8000.0,
            drive: v * 0.6,
            attack: v * 0.5,
        };
    }

    // Fill remaining notes (0-23, 112-127) with simple clicks
    for n in 0..24 {
        table[n] = SoundParams {
            engine: Engine::Click,
            freq: 200.0 + n as f64 * 50.0,
            decay: 0.005 + n as f64 * 0.002,
            ..Default::default()
        };
    }
    for n in 112..128 {
        table[n] = SoundParams {
            engine: Engine::NoiseBurst,
            freq: 1000.0 + (n - 112) as f64 * 300.0,
            noise_freq: 3000.0 + (n - 112) as f64 * 500.0,
            decay: 0.02 + (n - 112) as f64 * 0.01,
            ..Default::default()
        };
    }

    table
}

// ── Synthesis functions ──

fn synth_sine_body(phase: &mut f64, time: f64, p: &SoundParams, sr: f64) -> f64 {
    let sweep = p.sweep * (-time * p.sweep_speed).exp();
    let freq = p.freq * (1.0 + sweep);
    *phase += freq / sr;
    if *phase > 1.0 { *phase -= 1.0; }
    (*phase * std::f64::consts::TAU).sin()
}

fn synth_noise_burst(time: f64, p: &SoundParams) -> f64 {
    // Deterministic pseudo-noise with bandpass character
    let t = time * p.noise_freq;
    let raw = (t * 1.0).sin() * (t * 0.731).cos() + (t * 2.173).sin() * 0.5;
    raw * 0.6
}

fn synth_click(time: f64, p: &SoundParams) -> f64 {
    // Sharp impulse with fast decay
    let click_env = (-time * 800.0).exp();
    let t = time * p.freq;
    (t * std::f64::consts::TAU).sin() * click_env * p.attack
}

fn synth_metallic(phase: &mut f64, phase2: &mut f64, _time: f64, p: &SoundParams, sr: f64) -> f64 {
    *phase += p.freq / sr;
    *phase2 += p.freq2 / sr;
    if *phase > 1.0 { *phase -= 1.0; }
    if *phase2 > 1.0 { *phase2 -= 1.0; }
    let s1 = (*phase * std::f64::consts::TAU).sin();
    let s2 = (*phase2 * std::f64::consts::TAU).sin();
    // Square-ish combination for metallic character
    (s1 * s2 + s1 * 0.3) * 0.7
}

fn synth_distorted(phase: &mut f64, time: f64, p: &SoundParams, sr: f64) -> f64 {
    let sweep = p.sweep * (-time * p.sweep_speed).exp();
    let freq = p.freq * (1.0 + sweep);
    *phase += freq / sr;
    if *phase > 1.0 { *phase -= 1.0; }
    let sine = (*phase * std::f64::consts::TAU).sin();
    // Soft clip / waveshape
    let gain = 1.0 + p.drive * 8.0;
    let driven = sine * gain;
    driven / (1.0 + driven.abs()).sqrt()
}

fn run_engine(engine: Engine, phase: &mut f64, phase2: &mut f64, time: f64, p: &SoundParams, sr: f64) -> f64 {
    match engine {
        Engine::SineBody => synth_sine_body(phase, time, p, sr),
        Engine::NoiseBurst => synth_noise_burst(time, p),
        Engine::Click => synth_click(time, p),
        Engine::Metallic => synth_metallic(phase, phase2, time, p, sr),
        Engine::Distorted => synth_distorted(phase, time, p, sr),
    }
}

// ── Voice ──

const MAX_VOICES: usize = 16;

#[derive(Debug)]
struct DrumVoice {
    active: bool,
    time: f64,
    phase: f64,
    phase2: f64,
    note: u8,
    velocity: f32,
    params: SoundParams,
}

impl DrumVoice {
    fn new() -> Self {
        Self {
            active: false, time: 0.0, phase: 0.0, phase2: 0.0,
            note: 0, velocity: 0.0, params: SoundParams::default(),
        }
    }

    fn trigger(&mut self, note: u8, velocity: u8, sound: SoundParams) {
        self.active = true;
        self.time = 0.0;
        self.phase = 0.0;
        self.phase2 = 0.0;
        self.note = note;
        self.velocity = velocity as f32 / 127.0;
        self.params = sound;
    }

    fn tick(&mut self, sr: f64, user_decay: f64, user_tone: f64, user_noise: f64, user_drive: f64) -> f32 {
        if !self.active { return 0.0; }
        let dt = 1.0 / sr;
        self.time += dt;

        let mut p = self.params;
        // Apply user parameters
        p.decay *= 0.3 + user_decay * 1.4;
        p.freq *= 0.8 + user_tone * 0.4;
        p.noise_freq *= 0.7 + user_noise * 0.6;
        p.drive = (p.drive + user_drive * 0.5).min(1.0);

        let env = (-self.time / p.decay).exp();
        if env < 0.001 { self.active = false; return 0.0; }

        let e1 = run_engine(p.engine, &mut self.phase, &mut self.phase2, self.time, &p, sr);
        let e2 = match p.engine2 {
            Some(eng) => run_engine(eng, &mut self.phase2, &mut self.phase, self.time, &p, sr),
            None => 0.0,
        };
        let mixed = e1 * (1.0 - p.mix as f64) + e2 * p.mix as f64;

        (mixed * env * self.velocity as f64 * 0.4) as f32
    }
}

// ── DrumRack Plugin ──

pub struct DrumRack {
    voices: Vec<DrumVoice>,
    sample_rate: f64,
    pub kit: DrumKit,
    pub params: [f32; PARAM_COUNT],
    sound_table: [SoundParams; 128],
}

impl DrumRack {
    pub fn new() -> Self {
        let kit = DrumKit::Kit808;
        Self {
            voices: Vec::new(),
            sample_rate: 44100.0,
            kit,
            params: PARAM_DEFAULTS,
            sound_table: build_kit(kit),
        }
    }

    fn find_voice(&mut self, note: u8) -> &mut DrumVoice {
        if let Some(i) = self.voices.iter().position(|v| v.note == note) { return &mut self.voices[i]; }
        if let Some(i) = self.voices.iter().position(|v| !v.active) { return &mut self.voices[i]; }
        &mut self.voices[0]
    }
}

impl Default for DrumRack { fn default() -> Self { Self::new() } }

impl Plugin for DrumRack {
    fn info(&self) -> PluginInfo {
        PluginInfo { name: "Phosphor Drums".into(), version: "0.2.0".into(),
                     author: "Phosphor".into(), category: PluginCategory::Instrument }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..MAX_VOICES).map(|_| DrumVoice::new()).collect();
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() { return; }
        let buf_len = outputs[0].len();
        let gain = self.params[P_GAIN];
        let sr = self.sample_rate;
        let decay = self.params[P_DECAY] as f64;
        let tone = self.params[P_TONE] as f64;
        let noise = self.params[P_NOISE] as f64;
        let drive = self.params[P_DRIVE] as f64;
        let sound_table = self.sound_table;

        let mut events: Vec<&MidiEvent> = midi_events.iter().collect();
        events.sort_by_key(|e| e.sample_offset);
        let mut ei = 0;

        for i in 0..buf_len {
            while ei < events.len() && events[ei].sample_offset as usize <= i {
                let ev = events[ei];
                if ev.status & 0xF0 == 0x90 && ev.data2 > 0 {
                    let sound = sound_table[ev.data1 as usize];
                    let voice = self.find_voice(ev.data1);
                    voice.trigger(ev.data1, ev.data2, sound);
                }
                ei += 1;
            }

            let mut sample = 0.0f32;
            for voice in &mut self.voices {
                sample += voice.tick(sr, decay, tone, noise, drive);
            }
            sample *= gain;
            for ch in outputs.iter_mut() { ch[i] = sample; }
        }
    }

    fn parameter_count(&self) -> usize { PARAM_COUNT }

    fn parameter_info(&self, index: usize) -> Option<ParameterInfo> {
        if index >= PARAM_COUNT { return None; }
        Some(ParameterInfo {
            name: PARAM_NAMES[index].into(), min: 0.0, max: 1.0,
            default: PARAM_DEFAULTS[index], unit: "".into(),
        })
    }

    fn get_parameter(&self, index: usize) -> f32 { self.params.get(index).copied().unwrap_or(0.0) }

    fn set_parameter(&mut self, index: usize, value: f32) {
        if let Some(p) = self.params.get_mut(index) {
            *p = phosphor_plugin::clamp_parameter(value);
            if index == P_KIT {
                self.kit = DrumKit::from_param(*p);
                self.sound_table = build_kit(self.kit);
            }
        }
    }

    fn reset(&mut self) { for v in &mut self.voices { v.active = false; } }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_on(note: u8, vel: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x90, data1: note, data2: vel }
    }

    #[test]
    fn silent_without_input() {
        let mut dr = DrumRack::new(); dr.init(44100.0, 64);
        let mut out = vec![0.0f32; 64];
        dr.process(&[], &mut [&mut out], &[]);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn all_note_ranges_produce_sound() {
        let mut dr = DrumRack::new(); dr.init(44100.0, 512);
        // Test every 8th note across the range
        for note in (24..112).step_by(8) {
            let mut out = vec![0.0f32; 512];
            dr.process(&[], &mut [&mut out], &[note_on(note, 100, 0)]);
            let peak = out.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!(peak > 0.001, "Note {note} should produce sound, peak={peak}");
            dr.reset();
        }
    }

    #[test]
    fn kits_sound_different() {
        let mut dr = DrumRack::new(); dr.init(44100.0, 512);
        dr.set_parameter(P_KIT, 0.0);
        let mut out808 = vec![0.0f32; 512];
        dr.process(&[], &mut [&mut out808], &[note_on(24, 100, 0)]);

        dr.reset();
        dr.set_parameter(P_KIT, 0.25);
        let mut out909 = vec![0.0f32; 512];
        dr.process(&[], &mut [&mut out909], &[note_on(24, 100, 0)]);

        let diff: f32 = out808.iter().zip(out909.iter()).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 0.5, "808 and 909 should differ, diff={diff}");
    }

    #[test]
    fn output_is_finite() {
        let mut dr = DrumRack::new(); dr.init(44100.0, 64);
        for note in 0..128u8 {
            let mut out = vec![0.0f32; 64];
            dr.process(&[], &mut [&mut out], &[note_on(note, 127, 0)]);
            assert!(out.iter().all(|s| s.is_finite()), "Note {note} output not finite");
        }
    }

    #[test]
    fn kit_switch_rebuilds_table() {
        let mut dr = DrumRack::new(); dr.init(44100.0, 64);
        let freq_808 = dr.sound_table[24].freq;
        dr.set_parameter(P_KIT, 0.25);
        let freq_909 = dr.sound_table[24].freq;
        assert!((freq_808 - freq_909).abs() > 1.0, "Kit switch should change frequencies");
    }

    #[test]
    fn varied_sounds_across_range() {
        let dr = DrumRack::new();
        // Kicks should be different from hats
        assert!(dr.sound_table[24].freq < 100.0); // kick = low freq
        assert!(dr.sound_table[48].noise_freq > 5000.0); // hat = high noise
        assert!(dr.sound_table[84].decay < 0.05); // percussion = short
    }
}
