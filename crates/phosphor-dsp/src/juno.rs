//! Roland Juno-60 style single-DCO poly synthesizer with BBD chorus.
//!
//! Authentic recreation: single DCO per voice (saw/pulse/sub), IR3109 4-pole
//! LPF with BA662 resonance, single ADSR envelope (shared VCF+VCA),
//! BBD stereo chorus (I/II/I+II modes), sub-oscillator, white noise.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

const MAX_VOICES: usize = 6;
const TWO_PI: f64 = std::f64::consts::TAU;

// ── Parameter indices ──
pub const P_PATCH: usize = 0;
pub const P_SAW: usize = 1;       // on/off
pub const P_PULSE: usize = 2;     // on/off
pub const P_SUB: usize = 3;       // on/off
pub const P_PW: usize = 4;        // pulse width
pub const P_CUTOFF: usize = 5;
pub const P_RESO: usize = 6;
pub const P_ENV_MOD: usize = 7;   // bipolar: 0.5=center, <0.5=negative, >0.5=positive
pub const P_ATTACK: usize = 8;
pub const P_DECAY: usize = 9;
pub const P_SUSTAIN: usize = 10;
pub const P_RELEASE: usize = 11;
pub const P_CHORUS: usize = 12;   // 0=off, 1=I, 2=II, 3=I+II
pub const P_LFO_RATE: usize = 13;
pub const P_LFO_MOD: usize = 14;  // LFO to pitch amount
pub const P_GAIN: usize = 15;
pub const PARAM_COUNT: usize = 16;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "patch", "saw", "pulse", "sub", "pw",
    "cutoff", "reso", "envmod",
    "attack", "decay", "sustain", "release",
    "chorus", "lfo rate", "lfo mod", "gain",
];

pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    0.0,   // patch
    0.0,   // saw: off
    1.0,   // pulse: on
    0.0,   // sub: off
    0.7,   // pw
    0.5,   // cutoff
    0.0,   // reso
    0.6,   // env_mod (0.5=center, >0.5=positive)
    0.1,   // attack
    0.4,   // decay
    0.7,   // sustain
    0.3,   // release
    0.33,  // chorus: I
    0.1,   // lfo rate
    0.0,   // lfo mod
    0.75,  // gain
];

// ── Patches ──

pub const PATCH_COUNT: usize = 18;
pub const PATCH_NAMES: [&str; PATCH_COUNT] = [
    "Pad", "PWMPad", "Bass", "Brass", "String", "Hoover",
    "Acid", "WrmLd", "Choir", "Pluck", "Organ", "SynBas",
    "GlsBel", "ResPad", "Wind", "Clav", "SubBas", "SawPad",
];

pub fn discrete_label(index: usize, value: f32) -> Option<&'static str> {
    match index {
        P_PATCH => {
            let idx = (value * (PATCH_COUNT as f32 - 0.01)) as usize;
            Some(PATCH_NAMES[idx.min(PATCH_COUNT - 1)])
        }
        P_SAW | P_PULSE | P_SUB => Some(if value > 0.5 { "on" } else { "off" }),
        P_CHORUS => Some(match (value * 4.0) as u8 {
            0 => "off", 1 => "I", 2 => "II", _ => "I+II",
        }),
        _ => None,
    }
}

pub fn is_discrete(index: usize) -> bool {
    matches!(index, P_PATCH | P_SAW | P_PULSE | P_SUB | P_CHORUS)
}

// ── Internal preset ──

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct JunoPatch {
    saw: bool,
    pulse: bool,
    sub: bool,
    sub_level: f64,
    noise_level: f64,
    pulse_width: f64,
    pwm_lfo: bool,      // true = LFO modulates PW
    pwm_depth: f64,
    hpf: u8,            // 0-3
    cutoff: f64,
    resonance: f64,
    env_mod: f64,        // -1.0 to +1.0 (bipolar)
    lfo_to_filter: f64,
    key_follow: f64,
    attack: f64, decay: f64, sustain: f64, release: f64,
    vca_gate: bool,      // true = gate mode
    chorus: u8,          // 0=off, 1=I, 2=II, 3=I+II
    lfo_rate: f64,
    lfo_delay: f64,
    lfo_to_pitch: f64,
}

fn presets() -> [JunoPatch; PATCH_COUNT] {
    [
        // Pad — classic Juno pad
        JunoPatch {
            saw: false, pulse: true, sub: false, sub_level: 0.0, noise_level: 0.0,
            pulse_width: 0.5, pwm_lfo: true, pwm_depth: 0.7,
            hpf: 0, cutoff: 0.5, resonance: 0.0, env_mod: 0.2, lfo_to_filter: 0.0, key_follow: 0.5,
            attack: 0.3, decay: 0.5, sustain: 0.7, release: 0.5,
            vca_gate: false, chorus: 1,
            lfo_rate: 1.0, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
        // PWMPad — thick PWM pad
        JunoPatch {
            saw: true, pulse: true, sub: true, sub_level: 0.4, noise_level: 0.05,
            pulse_width: 0.5, pwm_lfo: true, pwm_depth: 0.8,
            hpf: 0, cutoff: 0.6, resonance: 0.1, env_mod: 0.2, lfo_to_filter: 0.0, key_follow: 0.5,
            attack: 0.4, decay: 0.6, sustain: 0.8, release: 0.5,
            vca_gate: false, chorus: 3,
            lfo_rate: 1.5, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
        // Bass — Juno bass
        JunoPatch {
            saw: true, pulse: true, sub: true, sub_level: 0.7, noise_level: 0.0,
            pulse_width: 0.5, pwm_lfo: false, pwm_depth: 0.0,
            hpf: 0, cutoff: 0.3, resonance: 0.2, env_mod: 0.6, lfo_to_filter: 0.0, key_follow: 0.3,
            attack: 0.001, decay: 0.4, sustain: 0.0, release: 0.2,
            vca_gate: false, chorus: 0,
            lfo_rate: 1.0, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
        // Brass — Juno brass
        JunoPatch {
            saw: true, pulse: true, sub: false, sub_level: 0.0, noise_level: 0.0,
            pulse_width: 0.5, pwm_lfo: false, pwm_depth: 0.0,
            hpf: 1, cutoff: 0.3, resonance: 0.0, env_mod: 0.7, lfo_to_filter: 0.0, key_follow: 0.5,
            attack: 0.2, decay: 0.4, sustain: 0.6, release: 0.3,
            vca_gate: false, chorus: 2,
            lfo_rate: 2.5, lfo_delay: 0.5, lfo_to_pitch: 0.02,
        },
        // String — Juno strings
        JunoPatch {
            saw: true, pulse: true, sub: false, sub_level: 0.0, noise_level: 0.05,
            pulse_width: 0.5, pwm_lfo: true, pwm_depth: 0.6,
            hpf: 1, cutoff: 0.55, resonance: 0.0, env_mod: 0.3, lfo_to_filter: 0.0, key_follow: 0.7,
            attack: 0.4, decay: 0.5, sustain: 0.7, release: 0.4,
            vca_gate: false, chorus: 3,
            lfo_rate: 2.0, lfo_delay: 0.3, lfo_to_pitch: 0.01,
        },
        // Hoover — rave stab
        JunoPatch {
            saw: true, pulse: true, sub: true, sub_level: 1.0, noise_level: 0.0,
            pulse_width: 0.5, pwm_lfo: false, pwm_depth: 0.0,
            hpf: 0, cutoff: 0.7, resonance: 0.4, env_mod: 0.5, lfo_to_filter: 0.0, key_follow: 0.5,
            attack: 0.001, decay: 0.3, sustain: 0.5, release: 0.3,
            vca_gate: false, chorus: 3,
            lfo_rate: 1.0, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
        // Acid — acid bass
        JunoPatch {
            saw: false, pulse: true, sub: false, sub_level: 0.0, noise_level: 0.0,
            pulse_width: 0.5, pwm_lfo: false, pwm_depth: 0.0,
            hpf: 0, cutoff: 0.2, resonance: 0.8, env_mod: 0.8, lfo_to_filter: 0.0, key_follow: 0.5,
            attack: 0.001, decay: 0.4, sustain: 0.0, release: 0.1,
            vca_gate: false, chorus: 0,
            lfo_rate: 1.0, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
        // WrmLd — warm lead
        JunoPatch {
            saw: true, pulse: false, sub: true, sub_level: 0.5, noise_level: 0.0,
            pulse_width: 0.5, pwm_lfo: false, pwm_depth: 0.0,
            hpf: 0, cutoff: 0.5, resonance: 0.1, env_mod: 0.4, lfo_to_filter: 0.0, key_follow: 0.7,
            attack: 0.1, decay: 0.5, sustain: 0.7, release: 0.3,
            vca_gate: false, chorus: 2,
            lfo_rate: 2.5, lfo_delay: 0.4, lfo_to_pitch: 0.03,
        },
        // Choir — choir pad
        JunoPatch {
            saw: false, pulse: true, sub: true, sub_level: 0.3, noise_level: 0.1,
            pulse_width: 0.5, pwm_lfo: true, pwm_depth: 0.8,
            hpf: 1, cutoff: 0.4, resonance: 0.2, env_mod: 0.3, lfo_to_filter: 0.1, key_follow: 0.7,
            attack: 0.5, decay: 0.5, sustain: 0.8, release: 0.5,
            vca_gate: false, chorus: 1,
            lfo_rate: 1.5, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
        // Pluck
        JunoPatch {
            saw: true, pulse: false, sub: false, sub_level: 0.0, noise_level: 0.0,
            pulse_width: 0.5, pwm_lfo: false, pwm_depth: 0.0,
            hpf: 0, cutoff: 0.2, resonance: 0.3, env_mod: 0.7, lfo_to_filter: 0.0, key_follow: 0.7,
            attack: 0.001, decay: 0.3, sustain: 0.0, release: 0.2,
            vca_gate: false, chorus: 1,
            lfo_rate: 1.0, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
        // Organ
        JunoPatch {
            saw: false, pulse: true, sub: true, sub_level: 0.5, noise_level: 0.0,
            pulse_width: 0.5, pwm_lfo: false, pwm_depth: 0.0,
            hpf: 1, cutoff: 0.7, resonance: 0.0, env_mod: 0.0, lfo_to_filter: 0.0, key_follow: 0.5,
            attack: 0.001, decay: 0.001, sustain: 1.0, release: 0.01,
            vca_gate: true, chorus: 3,
            lfo_rate: 2.5, lfo_delay: 0.0, lfo_to_pitch: 0.01,
        },
        // SynBas — synthwave bass
        JunoPatch {
            saw: true, pulse: true, sub: true, sub_level: 0.6, noise_level: 0.0,
            pulse_width: 0.5, pwm_lfo: false, pwm_depth: 0.0,
            hpf: 0, cutoff: 0.2, resonance: 0.1, env_mod: 0.5, lfo_to_filter: 0.0, key_follow: 0.3,
            attack: 0.001, decay: 0.5, sustain: 0.0, release: 0.2,
            vca_gate: false, chorus: 0,
            lfo_rate: 1.0, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
        // GlsBel — glass bells
        JunoPatch {
            saw: false, pulse: true, sub: false, sub_level: 0.0, noise_level: 0.0,
            pulse_width: 0.3, pwm_lfo: false, pwm_depth: 0.0,
            hpf: 2, cutoff: 0.4, resonance: 0.6, env_mod: 0.6, lfo_to_filter: 0.0, key_follow: 1.0,
            attack: 0.001, decay: 0.6, sustain: 0.0, release: 0.4,
            vca_gate: false, chorus: 1,
            lfo_rate: 1.0, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
        // ResPad — resonant sweep pad
        JunoPatch {
            saw: true, pulse: true, sub: false, sub_level: 0.0, noise_level: 0.0,
            pulse_width: 0.5, pwm_lfo: true, pwm_depth: 0.7,
            hpf: 0, cutoff: 0.2, resonance: 0.5, env_mod: 0.6, lfo_to_filter: 0.3, key_follow: 0.5,
            attack: 0.3, decay: 0.7, sustain: 0.4, release: 0.5,
            vca_gate: false, chorus: 2,
            lfo_rate: 1.0, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
        // Wind — noise wash
        JunoPatch {
            saw: false, pulse: false, sub: false, sub_level: 0.0, noise_level: 1.0,
            pulse_width: 0.5, pwm_lfo: false, pwm_depth: 0.0,
            hpf: 0, cutoff: 0.3, resonance: 0.3, env_mod: 0.5, lfo_to_filter: 0.4, key_follow: 0.5,
            attack: 0.5, decay: 0.7, sustain: 0.5, release: 0.7,
            vca_gate: false, chorus: 1,
            lfo_rate: 1.0, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
        // Clav — funky clavinet
        JunoPatch {
            saw: false, pulse: true, sub: false, sub_level: 0.0, noise_level: 0.0,
            pulse_width: 0.3, pwm_lfo: false, pwm_depth: 0.0,
            hpf: 2, cutoff: 0.5, resonance: 0.4, env_mod: 0.5, lfo_to_filter: 0.0, key_follow: 0.8,
            attack: 0.001, decay: 0.2, sustain: 0.2, release: 0.1,
            vca_gate: false, chorus: 0,
            lfo_rate: 1.0, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
        // SubBas — 808-style sub bass
        JunoPatch {
            saw: false, pulse: false, sub: true, sub_level: 1.0, noise_level: 0.0,
            pulse_width: 0.5, pwm_lfo: false, pwm_depth: 0.0,
            hpf: 0, cutoff: 0.2, resonance: 0.0, env_mod: 0.3, lfo_to_filter: 0.0, key_follow: 0.3,
            attack: 0.001, decay: 0.6, sustain: 0.5, release: 0.3,
            vca_gate: false, chorus: 0,
            lfo_rate: 1.0, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
        // SawPad — detuned saw pad (chorus magic)
        JunoPatch {
            saw: true, pulse: false, sub: false, sub_level: 0.0, noise_level: 0.0,
            pulse_width: 0.5, pwm_lfo: false, pwm_depth: 0.0,
            hpf: 0, cutoff: 0.7, resonance: 0.0, env_mod: 0.0, lfo_to_filter: 0.0, key_follow: 0.5,
            attack: 0.3, decay: 0.5, sustain: 0.8, release: 0.5,
            vca_gate: false, chorus: 3,
            lfo_rate: 1.0, lfo_delay: 0.0, lfo_to_pitch: 0.0,
        },
    ]
}

// ── PolyBLEP ──

#[inline]
fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let t = t / dt;
        2.0 * t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

#[inline]
fn tanh_approx(x: f64) -> f64 {
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

// ── DCO ──

#[derive(Debug, Clone)]
struct JunoDco {
    phase: f64,
    sub_phase: bool, // flip-flop for sub-oscillator
    freq: f64,
    dt: f64,
}

impl JunoDco {
    fn new() -> Self {
        Self { phase: 0.0, sub_phase: false, freq: 440.0, dt: 0.01 }
    }

    fn set_freq(&mut self, freq: f64, sr: f64) {
        self.freq = freq.clamp(0.1, sr * 0.45);
        self.dt = self.freq / sr;
    }

    /// Returns (saw, pulse, sub_osc).
    fn tick(&mut self, pulse_width: f64) -> (f64, f64, f64) {
        let dt = self.dt;
        self.phase += dt;
        let did_reset = self.phase >= 1.0;
        if did_reset {
            self.phase -= 1.0;
            self.sub_phase = !self.sub_phase; // divide by 2
        }
        let t = self.phase;

        // Sawtooth (falling, like the real Juno)
        let saw = 1.0 - 2.0 * t + poly_blep(t, dt);

        // Pulse with polyBLEP
        let pw = pulse_width.clamp(0.05, 0.95);
        let mut pulse = if t < pw { 1.0 } else { -1.0 };
        pulse += poly_blep(t, dt);
        pulse -= poly_blep((t - pw).rem_euclid(1.0), dt);

        // Sub-oscillator: square wave, one octave down
        let sub = if self.sub_phase { 1.0 } else { -1.0 };

        (saw, pulse, sub)
    }
}

// ── IR3109 Filter (same topology as Jupiter but Juno-specific tuning) ──

#[derive(Debug, Clone)]
struct JunoFilter {
    s: [f64; 4],
}

impl JunoFilter {
    fn new() -> Self { Self { s: [0.0; 4] } }

    fn process(&mut self, input: f64, cutoff_norm: f64, resonance: f64, sr: f64) -> f64 {
        let freq = 10.0 * (1000.0f64).powf(cutoff_norm.clamp(0.0, 1.0));
        let g = (std::f64::consts::PI * freq / sr).tan().min(0.99);
        let res = resonance.clamp(0.0, 1.0) * 4.0;
        let compensation = 1.0 + resonance * 0.5;
        let fb = tanh_approx(self.s[3]);
        let inp = tanh_approx(input * compensation - res * fb);

        self.s[0] += g * (inp - tanh_approx(self.s[0]));
        self.s[1] += g * (tanh_approx(self.s[0]) - tanh_approx(self.s[1]));
        self.s[2] += g * (tanh_approx(self.s[1]) - tanh_approx(self.s[2]));
        self.s[3] += g * (tanh_approx(self.s[2]) - tanh_approx(self.s[3]));

        for s in &mut self.s { if s.abs() < 1e-18 { *s = 0.0; } }
        self.s[3]
    }

    fn reset(&mut self) { self.s = [0.0; 4]; }
}

// ── HPF (4-position switched) ──

#[derive(Debug, Clone)]
struct JunoHpf {
    prev_in: f64,
    prev_out: f64,
}

impl JunoHpf {
    fn new() -> Self { Self { prev_in: 0.0, prev_out: 0.0 } }

    fn process(&mut self, input: f64, position: u8, sr: f64) -> f64 {
        let freq = match position {
            0 => return input, // bypass
            1 => 225.0,
            2 => 339.0,
            _ => 720.0,
        };
        let rc = 1.0 / (TWO_PI * freq);
        let dt = 1.0 / sr;
        let alpha = rc / (rc + dt);
        let out = alpha * (self.prev_out + input - self.prev_in);
        self.prev_in = input;
        self.prev_out = if out.abs() < 1e-18 { 0.0 } else { out };
        self.prev_out
    }
}

// ── ADSR Envelope ──

#[derive(Debug, Clone, Copy, PartialEq)]
enum EnvStage { Idle, Attack, Decay, Sustain, Release }

#[derive(Debug, Clone)]
struct JunoEnvelope {
    stage: EnvStage,
    level: f64,
    attack: f64, decay: f64, sustain: f64, release: f64,
    sample_rate: f64,
}

impl JunoEnvelope {
    fn new(sr: f64) -> Self {
        Self { stage: EnvStage::Idle, level: 0.0,
               attack: 0.01, decay: 0.3, sustain: 0.7, release: 0.2, sample_rate: sr }
    }

    fn trigger(&mut self) { self.stage = EnvStage::Attack; }
    fn release_env(&mut self) { if self.stage != EnvStage::Idle { self.stage = EnvStage::Release; } }
    fn kill(&mut self) { self.stage = EnvStage::Idle; self.level = 0.0; }
    fn is_active(&self) -> bool { self.stage != EnvStage::Idle }
    fn is_held(&self) -> bool {
        matches!(self.stage, EnvStage::Attack | EnvStage::Decay | EnvStage::Sustain)
    }

    fn tick(&mut self) -> f64 {
        let sr = self.sample_rate;
        match self.stage {
            EnvStage::Idle => 0.0,
            EnvStage::Attack => {
                // Juno uses LINEAR attack
                let rate = 1.0 / (self.attack * sr).max(1.0);
                self.level += rate;
                if self.level >= 1.0 { self.level = 1.0; self.stage = EnvStage::Decay; }
                self.level
            }
            EnvStage::Decay => {
                let rate = if self.decay < 0.001 { 1.0 }
                    else { 1.0 - (-1.0 / (self.decay * sr)).exp() };
                self.level += rate * (self.sustain - self.level);
                if (self.level - self.sustain).abs() < 0.001 {
                    self.level = self.sustain;
                    self.stage = EnvStage::Sustain;
                }
                self.level
            }
            EnvStage::Sustain => self.sustain,
            EnvStage::Release => {
                let rate = if self.release < 0.001 { 1.0 }
                    else { 1.0 - (-1.0 / (self.release * sr)).exp() };
                self.level += rate * (0.0 - self.level);
                if self.level < 0.001 { self.level = 0.0; self.stage = EnvStage::Idle; }
                self.level
            }
        }
    }
}

// ── LFO ──

#[derive(Debug, Clone)]
struct JunoLfo {
    phase: f64,
    rate: f64,
    delay_time: f64,
    delay_counter: f64,
    delay_level: f64,
}

impl JunoLfo {
    fn new() -> Self {
        Self { phase: 0.0, rate: 1.0, delay_time: 0.0, delay_counter: 0.0, delay_level: 1.0 }
    }

    fn trigger_delay(&mut self) {
        if self.delay_time > 0.001 {
            self.delay_counter = 0.0;
            self.delay_level = 0.0;
        }
    }

    /// Returns triangle LFO output (-1..1).
    fn tick(&mut self, sr: f64) -> f64 {
        self.phase += self.rate / sr;
        if self.phase >= 1.0 { self.phase -= 1.0; }
        if self.delay_level < 1.0 {
            self.delay_counter += 1.0 / sr;
            self.delay_level = (self.delay_counter / self.delay_time.max(0.001)).min(1.0);
        }
        // Triangle wave
        let tri = if self.phase < 0.5 { 4.0 * self.phase - 1.0 } else { 3.0 - 4.0 * self.phase };
        tri * self.delay_level
    }
}

// ── BBD Chorus ──

#[derive(Debug, Clone)]
struct BbdChorus {
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    write_pos: usize,
    lfo_phase: f64,
}

impl BbdChorus {
    fn new(sr: f64) -> Self {
        let max_samples = (sr * 0.01) as usize + 2; // ~10ms max
        Self {
            delay_l: vec![0.0; max_samples],
            delay_r: vec![0.0; max_samples],
            write_pos: 0,
            lfo_phase: 0.0,
        }
    }

    /// Process mono input, return (left, right) stereo output.
    fn process(&mut self, input: f32, mode: u8, sr: f64) -> (f32, f32) {
        if mode == 0 { return (input, input); } // bypass

        let (rate, depth_frac, min_delay, max_delay, invert_r) = match mode {
            1 => (0.5, 1.0, 0.00166, 0.00535, true),  // Chorus I
            2 => (0.8, 1.0, 0.00166, 0.00535, true),  // Chorus II
            _ => (9.75, 0.08, 0.0033, 0.0037, false),  // I+II
        };

        // LFO
        self.lfo_phase += rate / sr;
        if self.lfo_phase >= 1.0 { self.lfo_phase -= 1.0; }
        let lfo = if mode <= 2 {
            // Triangle
            if self.lfo_phase < 0.5 { 4.0 * self.lfo_phase - 1.0 } else { 3.0 - 4.0 * self.lfo_phase }
        } else {
            // Sine-like for I+II
            (self.lfo_phase * TWO_PI).sin()
        };

        let center = (min_delay + max_delay) / 2.0;
        let sweep = (max_delay - min_delay) / 2.0 * depth_frac;

        let delay_l_time = center + lfo * sweep;
        let delay_r_time = if invert_r { center - lfo * sweep } else { center + lfo * sweep };

        // Write input (with subtle BBD saturation)
        let saturated = tanh_approx(input as f64 * 0.8) as f32;
        let len = self.delay_l.len();
        self.delay_l[self.write_pos] = saturated;
        self.delay_r[self.write_pos] = saturated;
        self.write_pos = (self.write_pos + 1) % len;

        // Read with linear interpolation
        let read_l = self.read_delay(&self.delay_l, delay_l_time, sr);
        let read_r = self.read_delay(&self.delay_r, delay_r_time, sr);

        // 50/50 wet/dry mix
        let out_l = input * 0.5 + read_l * 0.5;
        let out_r = input * 0.5 + read_r * 0.5;

        (out_l, out_r)
    }

    fn read_delay(&self, buf: &[f32], delay_secs: f64, sr: f64) -> f32 {
        let delay_samples = delay_secs * sr;
        let len = buf.len() as f64;
        let read_pos = self.write_pos as f64 - delay_samples - 1.0;
        let read_pos = ((read_pos % len) + len) % len;
        let idx0 = read_pos as usize % buf.len();
        let idx1 = (idx0 + 1) % buf.len();
        let frac = (read_pos - read_pos.floor()) as f32;
        buf[idx0] * (1.0 - frac) + buf[idx1] * frac
    }
}

// ── Noise Generator ──

#[derive(Debug, Clone)]
struct NoiseGen {
    state: u32,
}

impl NoiseGen {
    fn new() -> Self { Self { state: 12345 } }
    fn tick(&mut self) -> f64 {
        self.state = self.state.wrapping_mul(1103515245).wrapping_add(12345);
        (self.state as i32) as f64 / i32::MAX as f64
    }
}

// ── Voice ──

#[derive(Debug, Clone)]
struct JunoVoice {
    dco: JunoDco,
    filter: JunoFilter,
    env: JunoEnvelope,
    note: u8,
    age: u64,
    sample_rate: f64,
}

impl JunoVoice {
    fn new(sr: f64) -> Self {
        Self {
            dco: JunoDco::new(),
            filter: JunoFilter::new(),
            env: JunoEnvelope::new(sr),
            note: 255,
            age: 0,
            sample_rate: sr,
        }
    }

    fn note_on(&mut self, note: u8, patch: &JunoPatch, age: u64) {
        self.note = note;
        self.age = age;
        self.env.attack = patch.attack;
        self.env.decay = patch.decay;
        self.env.sustain = patch.sustain;
        self.env.release = patch.release;
        self.env.trigger();
        self.filter.reset();
    }

    fn note_off(&mut self) { self.env.release_env(); }
    fn kill(&mut self) { self.note = 255; self.env.kill(); self.filter.reset(); }
    fn is_sounding(&self) -> bool { self.env.is_active() }
    fn is_held(&self) -> bool { self.env.is_held() }

    /// Process one sample. Returns mono output (pre-chorus).
    fn tick(&mut self, patch: &JunoPatch, lfo_out: f64, noise: f64, pw_mod: f64) -> f64 {
        if !self.is_sounding() { return 0.0; }

        let sr = self.sample_rate;
        let freq = note_to_freq(self.note);

        // LFO pitch modulation
        let pitch_mod = lfo_out * patch.lfo_to_pitch * 100.0;
        let modulated_freq = freq * 2.0f64.powf(pitch_mod / 1200.0);
        self.dco.set_freq(modulated_freq, sr);

        // Pulse width with PWM
        let effective_pw = if patch.pwm_lfo {
            (patch.pulse_width + lfo_out * patch.pwm_depth * 0.45).clamp(0.05, 0.95)
        } else {
            (patch.pulse_width + pw_mod).clamp(0.05, 0.95)
        };

        let (saw, pulse, sub) = self.dco.tick(effective_pw);

        // Mix waveforms (switches, not faders)
        let mut mixed = 0.0;
        if patch.saw { mixed += saw; }
        if patch.pulse { mixed += pulse; }
        if patch.sub { mixed += sub * patch.sub_level; }
        mixed += noise * patch.noise_level;

        // Scale to prevent clipping when multiple sources are on
        let source_count = patch.saw as u8 + patch.pulse as u8 +
            (patch.sub && patch.sub_level > 0.01) as u8 +
            (patch.noise_level > 0.01) as u8;
        if source_count > 1 { mixed /= (source_count as f64).sqrt(); }

        // Envelope
        let env_val = self.env.tick();

        // Filter cutoff: base + envelope + key follow + LFO
        let key_follow = (self.note as f64 - 60.0) / 60.0 * patch.key_follow;
        let env_mod_amount = patch.env_mod; // already bipolar -1..1
        let effective_cutoff = (patch.cutoff
            + env_val * env_mod_amount
            + key_follow
            + lfo_out * patch.lfo_to_filter
        ).clamp(0.0, 1.0);

        let filtered = self.filter.process(mixed, effective_cutoff, patch.resonance, sr);

        // VCA
        let vca = if patch.vca_gate { 1.0 } else { env_val };
        filtered * vca * 0.5
    }
}

// ── Juno-60 Synth ──

pub struct Juno60Synth {
    voices: Vec<JunoVoice>,
    lfo: JunoLfo,
    chorus: Option<BbdChorus>,
    noise: NoiseGen,
    hpf: JunoHpf,
    sample_rate: f64,
    pub params: [f32; PARAM_COUNT],
    voice_counter: u64,
    patches: [JunoPatch; PATCH_COUNT],
    last_patch_index: usize,
}

impl Juno60Synth {
    pub fn new() -> Self {
        let mut s = Self {
            voices: Vec::new(),
            lfo: JunoLfo::new(),
            chorus: None,
            noise: NoiseGen::new(),
            hpf: JunoHpf::new(),
            sample_rate: 44100.0,
            params: PARAM_DEFAULTS,
            voice_counter: 0,
            patches: presets(),
            last_patch_index: usize::MAX,
        };
        s.sync_params_from_patch();
        s
    }

    fn current_patch_index(&self) -> usize {
        let idx = (self.params[P_PATCH] * (PATCH_COUNT as f32 - 0.01)) as usize;
        idx.min(PATCH_COUNT - 1)
    }

    pub fn params_for_patch(patch_value: f32) -> [f32; PARAM_COUNT] {
        let idx = (patch_value * (PATCH_COUNT as f32 - 0.01)) as usize;
        let idx = idx.min(PATCH_COUNT - 1);
        let p = &presets()[idx];
        let mut params = PARAM_DEFAULTS;
        params[P_PATCH] = patch_value;
        params[P_SAW] = if p.saw { 1.0 } else { 0.0 };
        params[P_PULSE] = if p.pulse { 1.0 } else { 0.0 };
        params[P_SUB] = if p.sub { 1.0 } else { 0.0 };
        params[P_PW] = p.pulse_width as f32;
        params[P_CUTOFF] = p.cutoff as f32;
        params[P_RESO] = p.resonance as f32;
        params[P_ENV_MOD] = ((p.env_mod + 1.0) / 2.0) as f32; // -1..1 → 0..1
        params[P_ATTACK] = ((p.attack - 0.001) / 2.999).clamp(0.0, 1.0) as f32;
        params[P_DECAY] = ((p.decay - 0.002) / 11.998).clamp(0.0, 1.0) as f32;
        params[P_SUSTAIN] = p.sustain as f32;
        params[P_RELEASE] = ((p.release - 0.002) / 11.998).clamp(0.0, 1.0) as f32;
        params[P_CHORUS] = p.chorus as f32 / 3.99;
        params[P_LFO_RATE] = ((p.lfo_rate - 0.3) / 19.7).clamp(0.0, 1.0) as f32;
        params[P_LFO_MOD] = p.lfo_to_pitch.clamp(0.0, 1.0) as f32;
        params[P_GAIN] = PARAM_DEFAULTS[P_GAIN];
        params
    }

    fn sync_params_from_patch(&mut self) {
        let idx = self.current_patch_index();
        if idx == self.last_patch_index { return; }
        self.last_patch_index = idx;
        let new_params = Self::params_for_patch(self.params[P_PATCH]);
        for (i, &v) in new_params.iter().enumerate() {
            if i != P_PATCH { self.params[i] = v; }
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

    fn kill_all(&mut self) {
        for v in &mut self.voices { v.kill(); }
        if let Some(ref mut chorus) = self.chorus {
            for s in &mut chorus.delay_l { *s = 0.0; }
            for s in &mut chorus.delay_r { *s = 0.0; }
        }
    }

    fn active_patch(&self) -> JunoPatch {
        let mut p = self.patches[self.current_patch_index()];
        p.saw = self.params[P_SAW] > 0.5;
        p.pulse = self.params[P_PULSE] > 0.5;
        p.sub = self.params[P_SUB] > 0.5;
        p.pulse_width = self.params[P_PW] as f64;
        p.cutoff = self.params[P_CUTOFF] as f64;
        p.resonance = self.params[P_RESO] as f64;
        p.env_mod = (self.params[P_ENV_MOD] as f64) * 2.0 - 1.0; // 0..1 → -1..1
        p.attack = 0.001 + self.params[P_ATTACK] as f64 * 2.999;
        p.decay = 0.002 + self.params[P_DECAY] as f64 * 11.998;
        p.sustain = self.params[P_SUSTAIN] as f64;
        p.release = 0.002 + self.params[P_RELEASE] as f64 * 11.998;
        p.chorus = (self.params[P_CHORUS] * 4.0).min(3.0) as u8;
        p.lfo_rate = 0.3 + self.params[P_LFO_RATE] as f64 * 19.7;
        p.lfo_to_pitch = self.params[P_LFO_MOD] as f64;
        p
    }
}

impl Default for Juno60Synth {
    fn default() -> Self { Self::new() }
}

impl Plugin for Juno60Synth {
    fn info(&self) -> PluginInfo {
        PluginInfo { name: "Juno-60".into(), version: "0.1.0".into(),
                     author: "Phosphor".into(), category: PluginCategory::Instrument }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..MAX_VOICES).map(|_| JunoVoice::new(sample_rate)).collect();
        self.chorus = Some(BbdChorus::new(sample_rate));
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() { return; }

        let buf_len = outputs[0].len();
        let gain = self.params[P_GAIN];
        let patch = self.active_patch();
        let sr = self.sample_rate;

        self.lfo.rate = patch.lfo_rate;
        self.lfo.delay_time = patch.lfo_delay;

        // PWM from envelope (when not using LFO)
        let pwm_from_env = !patch.pwm_lfo;

        // MIDI event sorting (allocation-free)
        let mut event_indices: [usize; 256] = [0; 256];
        let event_count = midi_events.len().min(256);
        for i in 0..event_count { event_indices[i] = i; }
        for i in 1..event_count {
            let mut j = i;
            while j > 0 && midi_events[event_indices[j]].sample_offset < midi_events[event_indices[j-1]].sample_offset {
                event_indices.swap(j, j - 1);
                j -= 1;
            }
        }
        let mut ei = 0;

        let stereo = outputs.len() >= 2;

        for i in 0..buf_len {
            while ei < event_count && midi_events[event_indices[ei]].sample_offset as usize <= i {
                let ev = &midi_events[event_indices[ei]];
                match ev.status & 0xF0 {
                    0x90 => {
                        if ev.data2 > 0 {
                            self.release_note(ev.data1);
                            let age = self.next_age();
                            let idx = self.allocate_voice();
                            self.voices[idx].note_on(ev.data1, &patch, age);
                            self.lfo.trigger_delay();
                        } else {
                            self.release_note(ev.data1);
                        }
                    }
                    0x80 => self.release_note(ev.data1),
                    0xB0 => match ev.data1 {
                        120 => self.kill_all(),
                        123 => { for v in &mut self.voices { if v.is_held() { v.note_off(); } } }
                        _ => {}
                    }
                    _ => {}
                }
                ei += 1;
            }

            // Global LFO
            let lfo_out = self.lfo.tick(sr);
            let noise_val = self.noise.tick();

            // PWM from envelope (use first active voice's envelope for simplicity)
            let pw_mod = if pwm_from_env {
                self.voices.iter().find(|v| v.is_sounding())
                    .map(|v| v.env.level * patch.pwm_depth * 0.45)
                    .unwrap_or(0.0)
            } else { 0.0 };

            // Sum all voices (mono)
            let mut mono = 0.0f32;
            for v in &mut self.voices {
                mono += v.tick(&patch, lfo_out, noise_val, pw_mod) as f32;
            }

            // HPF (post voice-sum)
            mono = self.hpf.process(mono as f64, patch.hpf, sr) as f32;

            // BBD Chorus
            let (left, right) = if let Some(ref mut chorus) = self.chorus {
                chorus.process(mono, patch.chorus, sr)
            } else {
                (mono, mono)
            };

            let left = (left * gain).clamp(-1.0, 1.0);
            let right = (right * gain).clamp(-1.0, 1.0);

            if stereo {
                outputs[0][i] = left;
                outputs[1][i] = right;
            } else {
                outputs[0][i] = (left + right) * 0.5;
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
        }
        if index == P_PATCH { self.sync_params_from_patch(); }
    }

    fn reset(&mut self) {
        self.kill_all();
        self.voice_counter = 0;
        if let Some(ref mut chorus) = self.chorus {
            for s in &mut chorus.delay_l { *s = 0.0; }
            for s in &mut chorus.delay_r { *s = 0.0; }
        }
    }
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
    fn cc(cc_num: u8, val: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0xB0, data1: cc_num, data2: val }
    }

    fn process_buffers(synth: &mut Juno60Synth, events: &[MidiEvent], count: usize) -> Vec<f32> {
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
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 4);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001, "peak={peak}");
    }

    #[test]
    fn silent_after_release() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        s.set_parameter(P_RELEASE, 0.05);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[note_off(60, 0)], 3000);
        let out = process_buffers(&mut s, &[], 1);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak < 0.001, "peak={peak}");
    }

    #[test]
    fn output_is_finite() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 1000);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn polyphony() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        let events = [note_on(60, 100, 0), note_on(64, 100, 0), note_on(67, 100, 0)];
        let out = process_buffers(&mut s, &events, 4);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001 && peak <= 1.0, "peak={peak}");
    }

    #[test]
    fn all_patches_produce_sound() {
        for pi in 0..PATCH_COUNT {
            let mut s = Juno60Synth::new();
            s.init(44100.0, 64);
            s.set_parameter(P_PATCH, pi as f32 / (PATCH_COUNT as f32 - 0.01));
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 2000);
            let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
            assert!(peak > 0.001, "Patch {} ({}) peak={peak}", pi, PATCH_NAMES[pi]);
        }
    }

    #[test]
    fn all_patches_finite() {
        for pi in 0..PATCH_COUNT {
            let mut s = Juno60Synth::new();
            s.init(44100.0, 64);
            s.set_parameter(P_PATCH, pi as f32 / (PATCH_COUNT as f32 - 0.01));
            let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 500);
            assert!(out.iter().all(|v| v.is_finite()), "Patch {} finite", pi);
        }
    }

    #[test]
    fn chorus_changes_sound() {
        let mut s1 = Juno60Synth::new();
        s1.init(44100.0, 64);
        s1.set_parameter(P_CHORUS, 0.0); // off
        let dry = process_buffers(&mut s1, &[note_on(60, 100, 0)], 8);

        let mut s2 = Juno60Synth::new();
        s2.init(44100.0, 64);
        s2.set_parameter(P_CHORUS, 0.75); // I+II
        let wet = process_buffers(&mut s2, &[note_on(60, 100, 0)], 8);

        let diff: f32 = dry.iter().zip(wet.iter()).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 0.01, "Chorus should change sound, diff={diff}");
    }

    #[test]
    fn cc120_kills() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[cc(120, 0, 0)], 1);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn all_params_readable() {
        let s = Juno60Synth::new();
        assert_eq!(s.parameter_count(), PARAM_COUNT);
        for i in 0..PARAM_COUNT {
            assert!(s.parameter_info(i).is_some());
            let val = s.get_parameter(i);
            assert!((0.0..=1.0).contains(&val), "param {i} = {val}");
        }
    }

    #[test]
    fn sample_accurate_midi() {
        let mut s = Juno60Synth::new();
        s.init(44100.0, 128);
        s.set_parameter(P_ATTACK, 0.0);
        let mut out = vec![0.0f32; 128];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 64)]);
        let pre = out[..64].iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        let post = out[64..].iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(pre < 0.001, "pre={pre}");
        assert!(post > 0.001, "post={post}");
    }
}
