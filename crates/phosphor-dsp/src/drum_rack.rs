//! Drum rack — synthesized drum machines (808, 909, 707, 606).
//!
//! Each kit maps MIDI notes 36-51 to drum sounds. All sounds are
//! synthesized (no samples), using sine waves, noise, and envelopes
//! to recreate the character of classic drum machines.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

// ── Note mapping (General MIDI drum map subset) ──
const NOTE_KICK: u8 = 36;
const NOTE_SNARE: u8 = 38;
const NOTE_CLAP: u8 = 39;
const NOTE_CLOSED_HAT: u8 = 42;
const NOTE_OPEN_HAT: u8 = 46;
const NOTE_LOW_TOM: u8 = 41;
const NOTE_MID_TOM: u8 = 45;
const NOTE_HIGH_TOM: u8 = 48;
const NOTE_CRASH: u8 = 49;
const NOTE_RIDE: u8 = 51;
const NOTE_RIM: u8 = 37;
const NOTE_COWBELL: u8 = 56;

// ── Parameters ──
pub const P_KIT: usize = 0;
pub const P_KICK_DECAY: usize = 1;
pub const P_SNARE_DECAY: usize = 2;
pub const P_HAT_DECAY: usize = 3;
pub const P_TONE: usize = 4;
pub const P_GAIN: usize = 5;
pub const PARAM_COUNT: usize = 6;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "kit", "kick dcy", "snr dcy", "hat dcy", "tone", "gain",
];

pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    0.0,   // kit: 808
    0.5,   // kick decay
    0.4,   // snare decay
    0.3,   // hat decay
    0.5,   // tone
    0.75,  // gain
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrumKit {
    Kit808,
    Kit909,
    Kit707,
    Kit606,
}

impl DrumKit {
    pub fn from_param(val: f32) -> Self {
        match (val * 4.0) as u8 {
            0 => Self::Kit808,
            1 => Self::Kit909,
            2 => Self::Kit707,
            _ => Self::Kit606,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Kit808 => "808",
            Self::Kit909 => "909",
            Self::Kit707 => "707",
            Self::Kit606 => "606",
        }
    }
}

// ── Drum voice — one active sound ──

#[derive(Debug)]
struct DrumVoice {
    active: bool,
    phase: f64,
    time: f64,     // seconds since trigger
    note: u8,
    velocity: f32,
    // Per-voice parameters set at trigger time
    freq: f64,
    decay: f64,
    noise_mix: f64,
    pitch_sweep: f64,
}

const MAX_DRUM_VOICES: usize = 12;

impl DrumVoice {
    fn new() -> Self {
        Self {
            active: false, phase: 0.0, time: 0.0, note: 0,
            velocity: 0.0, freq: 200.0, decay: 0.1,
            noise_mix: 0.0, pitch_sweep: 0.0,
        }
    }

    fn trigger(&mut self, note: u8, velocity: u8, kit: DrumKit, params: &[f32; PARAM_COUNT]) {
        self.active = true;
        self.phase = 0.0;
        self.time = 0.0;
        self.note = note;
        self.velocity = velocity as f32 / 127.0;

        let tone = params[P_TONE] as f64;

        match note {
            NOTE_KICK => {
                let base_freq = match kit {
                    DrumKit::Kit808 => 45.0,
                    DrumKit::Kit909 => 55.0,
                    DrumKit::Kit707 => 50.0,
                    DrumKit::Kit606 => 48.0,
                };
                self.freq = base_freq + tone * 20.0;
                self.decay = 0.15 + params[P_KICK_DECAY] as f64 * 0.5;
                self.noise_mix = 0.05;
                self.pitch_sweep = match kit {
                    DrumKit::Kit808 => 8.0,
                    DrumKit::Kit909 => 5.0,
                    DrumKit::Kit707 => 6.0,
                    DrumKit::Kit606 => 4.0,
                };
            }
            NOTE_SNARE => {
                self.freq = 180.0 + tone * 40.0;
                self.decay = 0.08 + params[P_SNARE_DECAY] as f64 * 0.25;
                self.noise_mix = match kit {
                    DrumKit::Kit808 => 0.6,
                    DrumKit::Kit909 => 0.7,
                    DrumKit::Kit707 => 0.5,
                    DrumKit::Kit606 => 0.65,
                };
                self.pitch_sweep = 2.0;
            }
            NOTE_CLAP => {
                self.freq = 1200.0;
                self.decay = 0.06 + params[P_SNARE_DECAY] as f64 * 0.1;
                self.noise_mix = 0.95;
                self.pitch_sweep = 0.0;
            }
            NOTE_CLOSED_HAT => {
                self.freq = 6000.0 + tone * 2000.0;
                self.decay = 0.02 + params[P_HAT_DECAY] as f64 * 0.06;
                self.noise_mix = 0.9;
                self.pitch_sweep = 0.0;
            }
            NOTE_OPEN_HAT => {
                self.freq = 6000.0 + tone * 2000.0;
                self.decay = 0.1 + params[P_HAT_DECAY] as f64 * 0.4;
                self.noise_mix = 0.9;
                self.pitch_sweep = 0.0;
            }
            NOTE_LOW_TOM | NOTE_MID_TOM | NOTE_HIGH_TOM => {
                let tom_base = match note {
                    NOTE_LOW_TOM => 80.0,
                    NOTE_MID_TOM => 120.0,
                    _ => 160.0,
                };
                self.freq = tom_base + tone * 30.0;
                self.decay = 0.12 + params[P_KICK_DECAY] as f64 * 0.2;
                self.noise_mix = 0.15;
                self.pitch_sweep = 3.0;
            }
            NOTE_RIM => {
                self.freq = 800.0 + tone * 200.0;
                self.decay = 0.015;
                self.noise_mix = 0.3;
                self.pitch_sweep = 0.0;
            }
            NOTE_COWBELL => {
                self.freq = 560.0;
                self.decay = 0.05;
                self.noise_mix = 0.0;
                self.pitch_sweep = 0.0;
            }
            NOTE_CRASH | NOTE_RIDE => {
                self.freq = 4000.0 + tone * 3000.0;
                self.decay = if note == NOTE_CRASH { 0.4 } else { 0.2 };
                self.noise_mix = 0.85;
                self.pitch_sweep = 0.0;
            }
            _ => {
                // Unknown note — short click
                self.freq = 1000.0;
                self.decay = 0.01;
                self.noise_mix = 0.5;
                self.pitch_sweep = 0.0;
            }
        }
    }

    fn tick(&mut self, sample_rate: f64) -> f32 {
        if !self.active { return 0.0; }

        let dt = 1.0 / sample_rate;
        self.time += dt;

        // Exponential decay envelope
        let env = (-self.time / self.decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }

        // Pitch sweep (kick/tom character)
        let sweep = self.pitch_sweep * (-self.time * 30.0).exp();
        let freq = self.freq * (1.0 + sweep);

        // Oscillator
        self.phase += freq * dt;
        if self.phase > 1.0 { self.phase -= 1.0; }
        let osc = (self.phase * std::f64::consts::TAU).sin();

        // Noise (deterministic)
        let noise_phase = self.time * 17389.0;
        let noise = (noise_phase.sin() * (noise_phase * 1.731).cos()) as f64;

        // Mix
        let mix = osc * (1.0 - self.noise_mix) + noise * self.noise_mix;

        (mix * env * self.velocity as f64 * 0.5) as f32
    }
}

// ── DrumRack Plugin ──

pub struct DrumRack {
    voices: Vec<DrumVoice>,
    sample_rate: f64,
    pub kit: DrumKit,
    pub params: [f32; PARAM_COUNT],
}

impl DrumRack {
    pub fn new() -> Self {
        Self {
            voices: Vec::new(),
            sample_rate: 44100.0,
            kit: DrumKit::Kit808,
            params: PARAM_DEFAULTS,
        }
    }

    fn find_voice(&mut self, note: u8) -> &mut DrumVoice {
        // Reuse voice with same note, or find inactive, or steal oldest
        if let Some(i) = self.voices.iter().position(|v| v.note == note) {
            return &mut self.voices[i];
        }
        if let Some(i) = self.voices.iter().position(|v| !v.active) {
            return &mut self.voices[i];
        }
        &mut self.voices[0]
    }
}

impl Default for DrumRack {
    fn default() -> Self { Self::new() }
}

impl Plugin for DrumRack {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "Phosphor Drums".into(),
            version: "0.1.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..MAX_DRUM_VOICES).map(|_| DrumVoice::new()).collect();
    }

    fn process(
        &mut self,
        _inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        midi_events: &[MidiEvent],
    ) {
        if outputs.is_empty() { return; }
        let buf_len = outputs[0].len();
        let gain = self.params[P_GAIN];
        let kit = self.kit;
        let params = self.params; // copy to avoid borrow conflict

        let mut events: Vec<&MidiEvent> = midi_events.iter().collect();
        events.sort_by_key(|e| e.sample_offset);
        let mut ei = 0;

        for i in 0..buf_len {
            while ei < events.len() && events[ei].sample_offset as usize <= i {
                let ev = events[ei];
                if ev.status & 0xF0 == 0x90 && ev.data2 > 0 {
                    let voice = self.find_voice(ev.data1);
                    voice.trigger(ev.data1, ev.data2, kit, &params);
                }
                ei += 1;
            }

            let mut sample = 0.0f32;
            for voice in &mut self.voices {
                sample += voice.tick(self.sample_rate);
            }
            sample *= gain;

            for ch in outputs.iter_mut() {
                ch[i] = sample;
            }
        }
    }

    fn parameter_count(&self) -> usize { PARAM_COUNT }

    fn parameter_info(&self, index: usize) -> Option<ParameterInfo> {
        if index >= PARAM_COUNT { return None; }
        Some(ParameterInfo {
            name: PARAM_NAMES[index].into(),
            min: 0.0, max: 1.0,
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
        for v in &mut self.voices { v.active = false; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_on(note: u8, vel: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x90, data1: note, data2: vel }
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
    fn kick_produces_sound() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 256);
        let mut out = vec![0.0f32; 256];
        dr.process(&[], &mut [&mut out], &[note_on(NOTE_KICK, 100, 0)]);
        let peak = out.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.01, "Kick should produce sound, peak={peak}");
    }

    #[test]
    fn snare_produces_sound() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 256);
        let mut out = vec![0.0f32; 256];
        dr.process(&[], &mut [&mut out], &[note_on(NOTE_SNARE, 100, 0)]);
        let peak = out.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.01, "Snare should produce sound, peak={peak}");
    }

    #[test]
    fn hat_produces_sound() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 256);
        let mut out = vec![0.0f32; 256];
        dr.process(&[], &mut [&mut out], &[note_on(NOTE_CLOSED_HAT, 100, 0)]);
        let peak = out.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.01, "Hat should produce sound, peak={peak}");
    }

    #[test]
    fn output_is_finite() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 64);
        let events = [
            note_on(NOTE_KICK, 127, 0),
            note_on(NOTE_SNARE, 127, 10),
            note_on(NOTE_CLOSED_HAT, 127, 20),
        ];
        for _ in 0..1000 {
            let mut out = vec![0.0f32; 64];
            dr.process(&[], &mut [&mut out], &events);
            assert!(out.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn kit_switch() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 64);
        assert_eq!(dr.kit, DrumKit::Kit808);
        dr.set_parameter(P_KIT, 0.25);
        assert_eq!(dr.kit, DrumKit::Kit909);
        dr.set_parameter(P_KIT, 0.5);
        assert_eq!(dr.kit, DrumKit::Kit707);
        dr.set_parameter(P_KIT, 0.75);
        assert_eq!(dr.kit, DrumKit::Kit606);
    }

    #[test]
    fn different_kits_sound_different() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 256);

        dr.set_parameter(P_KIT, 0.0); // 808
        let mut out808 = vec![0.0f32; 256];
        dr.process(&[], &mut [&mut out808], &[note_on(NOTE_KICK, 100, 0)]);

        dr.reset();
        dr.set_parameter(P_KIT, 0.25); // 909
        let mut out909 = vec![0.0f32; 256];
        dr.process(&[], &mut [&mut out909], &[note_on(NOTE_KICK, 100, 0)]);

        let diff: f32 = out808.iter().zip(out909.iter())
            .map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 0.1, "808 and 909 kicks should sound different, diff={diff}");
    }
}
