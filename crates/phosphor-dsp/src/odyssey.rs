//! ARP Odyssey style duophonic synthesizer.
//!
//! Authentic recreation of the ARP Odyssey architecture:
//! 2 polyBLEP VCOs with duophonic voice split, 3 selectable filter types
//! (4023 SVF / 4035 Moog ladder / 4075 Norton), HPF→LPF signal chain,
//! ADSR + AR envelopes, XOR ring modulator, Sample & Hold, hard sync.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

const TWO_PI: f64 = std::f64::consts::TAU;

// ── Parameter indices ──
pub const P_PATCH: usize = 0;
pub const P_VCO1_WAVE: usize = 1;
pub const P_VCO2_WAVE: usize = 2;
pub const P_DETUNE: usize = 3;
pub const P_CUTOFF: usize = 4;
pub const P_RESO: usize = 5;
pub const P_FILTER_TYPE: usize = 6;
pub const P_ENV_MOD: usize = 7;
pub const P_ATTACK: usize = 8;
pub const P_DECAY: usize = 9;
pub const P_SUSTAIN: usize = 10;
pub const P_RELEASE: usize = 11;
pub const P_SYNC: usize = 12;
pub const P_RING_MOD: usize = 13;
pub const P_LFO_RATE: usize = 14;
pub const P_GAIN: usize = 15;
pub const PARAM_COUNT: usize = 16;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "patch", "vco1wav", "vco2wav", "detune",
    "cutoff", "reso", "filter", "envmod",
    "attack", "decay", "sustain", "release",
    "sync", "ringmod", "lfo rate", "gain",
];

pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    0.0,   // patch: Bass
    0.5,   // vco1wav: Saw
    0.5,   // vco2wav: Saw
    0.52,  // detune: slight sharp
    0.5,   // cutoff
    0.0,   // reso
    0.0,   // filter type: 4023 (Mk I)
    0.4,   // env_mod
    0.01,  // attack: fast
    0.3,   // decay
    0.5,   // sustain
    0.2,   // release
    0.0,   // sync: off
    0.0,   // ring mod: off
    0.3,   // lfo rate
    0.75,  // gain
];

// ── Patches ──

pub const PATCH_COUNT: usize = 15;
pub const PATCH_NAMES: [&str; PATCH_COUNT] = [
    "Bass", "Funk", "SyncLd", "Bells", "Pad", "S&H", "Zap",
    "HwkFunk", "Atmos", "Cars", "SciFi", "Pluck", "ThkLead", "FltSwp", "NoiseHt",
];

/// Discrete parameter labels for UI.
pub fn discrete_label(index: usize, value: f32) -> Option<&'static str> {
    match index {
        P_PATCH => {
            let idx = (value * (PATCH_COUNT as f32 - 0.01)) as usize;
            Some(PATCH_NAMES[idx.min(PATCH_COUNT - 1)])
        }
        P_VCO1_WAVE | P_VCO2_WAVE => Some(match (value * 2.0) as u8 {
            0 => "saw", _ => "pulse",
        }),
        P_FILTER_TYPE => Some(match (value * 3.0) as u8 {
            0 => "4023", 1 => "4035", _ => "4075",
        }),
        P_SYNC => Some(if value > 0.5 { "on" } else { "off" }),
        P_RING_MOD => Some(if value > 0.5 { "on" } else { "off" }),
        _ => None,
    }
}

pub fn is_discrete(index: usize) -> bool {
    matches!(index, P_PATCH | P_VCO1_WAVE | P_VCO2_WAVE | P_FILTER_TYPE | P_SYNC | P_RING_MOD)
}

// ── Internal preset data ──

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct OdysseyPatch {
    vco1_wave: u8,     // 0=saw, 1=pulse
    vco2_wave: u8,
    detune_cents: f64,
    vco1_level: f64,
    vco2_level: f64,
    pulse_width: f64,
    sync: bool,
    ring_mod_level: f64,
    noise_level: f64,
    filter_type: u8,   // 0=4023, 1=4035, 2=4075
    cutoff: f64,
    resonance: f64,
    hpf_cutoff: f64,
    env_mod: f64,
    key_follow: f64,
    // ADSR
    adsr_a: f64, adsr_d: f64, adsr_s: f64, adsr_r: f64,
    // AR (for VCA)
    ar_a: f64, ar_r: f64,
    use_adsr_for_vca: bool, // true = ADSR controls VCA, false = AR
    lfo_rate: f64,
    lfo_to_pitch: f64,
    lfo_to_filter: f64,
    lfo_to_pwm: f64,
    portamento: f64,
}

fn presets() -> [OdysseyPatch; PATCH_COUNT] {
    [
        // Bass — classic Odyssey bass
        OdysseyPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: -3.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.5, sync: false, ring_mod_level: 0.0, noise_level: 0.0,
            filter_type: 0, cutoff: 0.25, resonance: 0.3, hpf_cutoff: 0.0,
            env_mod: 0.6, key_follow: 0.3,
            adsr_a: 0.005, adsr_d: 0.3, adsr_s: 0.2, adsr_r: 0.15,
            ar_a: 0.005, ar_r: 0.3, use_adsr_for_vca: false,
            lfo_rate: 1.0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_to_pwm: 0.0,
            portamento: 0.0,
        },
        // Funk — Chameleon-style funky bass
        OdysseyPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: -5.0,
            vco1_level: 1.0, vco2_level: 1.0,
            pulse_width: 0.5, sync: false, ring_mod_level: 0.0, noise_level: 0.0,
            filter_type: 0, cutoff: 0.25, resonance: 0.8, hpf_cutoff: 0.0,
            env_mod: 0.75, key_follow: 0.4,
            adsr_a: 0.005, adsr_d: 0.25, adsr_s: 0.1, adsr_r: 0.2,
            ar_a: 0.005, ar_r: 0.25, use_adsr_for_vca: false,
            lfo_rate: 1.0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_to_pwm: 0.0,
            portamento: 0.0,
        },
        // SyncLd — aggressive sync sweep lead
        OdysseyPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 0.0,
            vco1_level: 0.0, vco2_level: 1.0,
            pulse_width: 0.5, sync: true, ring_mod_level: 0.0, noise_level: 0.0,
            filter_type: 0, cutoff: 0.65, resonance: 0.2, hpf_cutoff: 0.0,
            env_mod: 0.7, key_follow: 0.5,
            adsr_a: 0.005, adsr_d: 0.4, adsr_s: 0.5, adsr_r: 0.25,
            ar_a: 0.005, ar_r: 0.3, use_adsr_for_vca: true,
            lfo_rate: 5.0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_to_pwm: 0.0,
            portamento: 0.0,
        },
        // Bells — ring mod metallic bells
        OdysseyPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 700.0, // ~fifth interval
            vco1_level: 0.0, vco2_level: 0.0,
            pulse_width: 0.5, sync: false, ring_mod_level: 1.0, noise_level: 0.0,
            filter_type: 0, cutoff: 0.5, resonance: 0.15, hpf_cutoff: 0.0,
            env_mod: 0.5, key_follow: 0.6,
            adsr_a: 0.005, adsr_d: 0.5, adsr_s: 0.0, adsr_r: 0.4,
            ar_a: 0.005, ar_r: 0.5, use_adsr_for_vca: true,
            lfo_rate: 1.0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_to_pwm: 0.0,
            portamento: 0.0,
        },
        // Pad — strings/pad
        OdysseyPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 6.0,
            vco1_level: 0.7, vco2_level: 0.7,
            pulse_width: 0.3, sync: false, ring_mod_level: 0.0, noise_level: 0.0,
            filter_type: 0, cutoff: 0.45, resonance: 0.1, hpf_cutoff: 0.05,
            env_mod: 0.2, key_follow: 0.6,
            adsr_a: 0.4, adsr_d: 0.3, adsr_s: 0.8, adsr_r: 0.5,
            ar_a: 0.4, ar_r: 0.5, use_adsr_for_vca: true,
            lfo_rate: 4.0, lfo_to_pitch: 0.015, lfo_to_filter: 0.0, lfo_to_pwm: 0.3,
            portamento: 0.0,
        },
        // S&H — sample & hold random pattern
        OdysseyPatch {
            vco1_wave: 0, vco2_wave: 1, detune_cents: 0.0,
            vco1_level: 0.6, vco2_level: 0.6,
            pulse_width: 0.5, sync: false, ring_mod_level: 0.0, noise_level: 0.0,
            filter_type: 0, cutoff: 0.4, resonance: 0.3, hpf_cutoff: 0.0,
            env_mod: 0.3, key_follow: 0.4,
            adsr_a: 0.005, adsr_d: 0.2, adsr_s: 0.6, adsr_r: 0.2,
            ar_a: 0.005, ar_r: 0.2, use_adsr_for_vca: false,
            lfo_rate: 6.0, lfo_to_pitch: 0.4, lfo_to_filter: 0.3, lfo_to_pwm: 0.0,
            portamento: 0.0,
        },
        // Zap — sci-fi laser effect
        OdysseyPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 0.0,
            vco1_level: 0.0, vco2_level: 1.0,
            pulse_width: 0.5, sync: true, ring_mod_level: 0.0, noise_level: 0.0,
            filter_type: 0, cutoff: 0.8, resonance: 0.4, hpf_cutoff: 0.0,
            env_mod: 1.0, key_follow: 0.3,
            adsr_a: 0.005, adsr_d: 0.6, adsr_s: 0.0, adsr_r: 0.4,
            ar_a: 0.005, ar_r: 0.6, use_adsr_for_vca: true,
            lfo_rate: 1.0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_to_pwm: 0.0,
            portamento: 0.0,
        },
        // HwkFunk — Alan Hawkshaw funky sequence style
        OdysseyPatch {
            vco1_wave: 1, vco2_wave: 0, detune_cents: 0.0,
            vco1_level: 0.7, vco2_level: 0.5,
            pulse_width: 0.35, sync: false, ring_mod_level: 0.0, noise_level: 0.0,
            filter_type: 1, cutoff: 0.25, resonance: 0.3, hpf_cutoff: 0.05,
            env_mod: 0.55, key_follow: 0.6,
            adsr_a: 0.001, adsr_d: 0.2, adsr_s: 0.1, adsr_r: 0.08,
            ar_a: 0.001, ar_r: 0.15, use_adsr_for_vca: false,
            lfo_rate: 1.0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_to_pwm: 0.0,
            portamento: 0.0,
        },
        // Atmos — Brian Bennett atmospheric pad
        OdysseyPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 6.0,
            vco1_level: 0.6, vco2_level: 0.6,
            pulse_width: 0.5, sync: false, ring_mod_level: 0.0, noise_level: 0.05,
            filter_type: 0, cutoff: 0.4, resonance: 0.35, hpf_cutoff: 0.08,
            env_mod: 0.2, key_follow: 0.3,
            adsr_a: 1.5, adsr_d: 1.0, adsr_s: 0.7, adsr_r: 2.5,
            ar_a: 1.8, ar_r: 3.0, use_adsr_for_vca: true,
            lfo_rate: 0.2, lfo_to_pitch: 0.0, lfo_to_filter: 0.25, lfo_to_pwm: 0.0,
            portamento: 0.1,
        },
        // Cars — Gary Numan nasal lead
        OdysseyPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 5.0,
            vco1_level: 0.8, vco2_level: 0.6,
            pulse_width: 0.4, sync: false, ring_mod_level: 0.0, noise_level: 0.0,
            filter_type: 2, cutoff: 0.45, resonance: 0.25, hpf_cutoff: 0.05,
            env_mod: 0.3, key_follow: 0.5,
            adsr_a: 0.01, adsr_d: 0.4, adsr_s: 0.5, adsr_r: 0.2,
            ar_a: 0.01, ar_r: 0.25, use_adsr_for_vca: false,
            lfo_rate: 1.0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_to_pwm: 0.0,
            portamento: 0.05,
        },
        // SciFi — wobble effect
        OdysseyPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 12.0,
            vco1_level: 0.7, vco2_level: 0.5,
            pulse_width: 0.5, sync: false, ring_mod_level: 0.15, noise_level: 0.0,
            filter_type: 1, cutoff: 0.5, resonance: 0.6, hpf_cutoff: 0.0,
            env_mod: 0.3, key_follow: 0.4,
            adsr_a: 0.01, adsr_d: 0.5, adsr_s: 0.6, adsr_r: 0.5,
            ar_a: 0.01, ar_r: 0.4, use_adsr_for_vca: true,
            lfo_rate: 4.0, lfo_to_pitch: 0.08, lfo_to_filter: 0.4, lfo_to_pwm: 0.0,
            portamento: 0.0,
        },
        // Pluck — percussive pluck/clavinet
        OdysseyPatch {
            vco1_wave: 1, vco2_wave: 0, detune_cents: 0.0,
            vco1_level: 0.6, vco2_level: 0.7,
            pulse_width: 0.3, sync: false, ring_mod_level: 0.0, noise_level: 0.02,
            filter_type: 2, cutoff: 0.1, resonance: 0.2, hpf_cutoff: 0.05,
            env_mod: 0.7, key_follow: 0.8,
            adsr_a: 0.001, adsr_d: 0.12, adsr_s: 0.0, adsr_r: 0.08,
            ar_a: 0.001, ar_r: 0.1, use_adsr_for_vca: false,
            lfo_rate: 1.0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_to_pwm: 0.0,
            portamento: 0.0,
        },
        // ThkLead — fat Zawinul-style lead
        OdysseyPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 8.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.5, sync: false, ring_mod_level: 0.0, noise_level: 0.0,
            filter_type: 1, cutoff: 0.4, resonance: 0.15, hpf_cutoff: 0.0,
            env_mod: 0.35, key_follow: 0.5,
            adsr_a: 0.01, adsr_d: 0.3, adsr_s: 0.65, adsr_r: 0.25,
            ar_a: 0.01, ar_r: 0.3, use_adsr_for_vca: false,
            lfo_rate: 5.5, lfo_to_pitch: 0.02, lfo_to_filter: 0.0, lfo_to_pwm: 0.0,
            portamento: 0.1,
        },
        // FltSwp — Vince Clarke filter sweep pad
        OdysseyPatch {
            vco1_wave: 0, vco2_wave: 1, detune_cents: 4.0,
            vco1_level: 0.6, vco2_level: 0.6,
            pulse_width: 0.5, sync: false, ring_mod_level: 0.0, noise_level: 0.0,
            filter_type: 0, cutoff: 0.3, resonance: 0.45, hpf_cutoff: 0.05,
            env_mod: 0.1, key_follow: 0.3,
            adsr_a: 0.8, adsr_d: 0.5, adsr_s: 0.8, adsr_r: 1.5,
            ar_a: 1.0, ar_r: 2.0, use_adsr_for_vca: true,
            lfo_rate: 0.12, lfo_to_pitch: 0.0, lfo_to_filter: 0.45, lfo_to_pwm: 0.3,
            portamento: 0.08,
        },
        // NoiseHt — percussive noise burst
        OdysseyPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 0.0,
            vco1_level: 0.3, vco2_level: 0.0,
            pulse_width: 0.5, sync: false, ring_mod_level: 0.0, noise_level: 0.8,
            filter_type: 2, cutoff: 0.8, resonance: 0.1, hpf_cutoff: 0.15,
            env_mod: 0.6, key_follow: 0.0,
            adsr_a: 0.001, adsr_d: 0.08, adsr_s: 0.0, adsr_r: 0.05,
            ar_a: 0.001, ar_r: 0.06, use_adsr_for_vca: false,
            lfo_rate: 1.0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_to_pwm: 0.0,
            portamento: 0.0,
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

// ── Fast tanh approximation ──

#[inline]
fn tanh_approx(x: f64) -> f64 {
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// Softer clipping for Norton op-amp filter (4075).
#[inline]
fn soft_clip(x: f64) -> f64 {
    x / (1.0 + x.abs())
}

// ── VCO ──

#[derive(Debug, Clone)]
struct OdysseyVco {
    phase: f64,
    freq: f64,
    dt: f64,
    // Track last output for ring mod
    last_pulse: f64,
}

impl OdysseyVco {
    fn new() -> Self {
        Self { phase: 0.0, freq: 440.0, dt: 0.01, last_pulse: -1.0 }
    }

    fn set_freq(&mut self, freq: f64, sr: f64) {
        self.freq = freq.clamp(0.1, sr * 0.45);
        self.dt = self.freq / sr;
    }

    /// Returns (saw_out, pulse_out, did_reset).
    fn tick(&mut self, pulse_width: f64) -> (f64, f64, bool) {
        let dt = self.dt;
        let mut did_reset = false;

        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            did_reset = true;
        }
        let t = self.phase;

        // Sawtooth with polyBLEP
        let saw = 2.0 * t - 1.0 - poly_blep(t, dt);

        // Pulse with polyBLEP
        let pw = pulse_width.clamp(0.05, 0.95);
        let mut pulse = if t < pw { 1.0 } else { -1.0 };
        pulse += poly_blep(t, dt);
        pulse -= poly_blep((t - pw).rem_euclid(1.0), dt);
        self.last_pulse = pulse;

        (saw, pulse, did_reset)
    }

    fn reset_phase(&mut self) {
        self.phase = 0.0;
    }
}

// ── Noise Generator ──

#[derive(Debug, Clone)]
struct NoiseGen {
    state: u32,
    #[allow(dead_code)]
    pink_b: [f64; 7], // Pink noise filter state (for future pink noise)
}

impl NoiseGen {
    fn new() -> Self {
        Self { state: 12345, pink_b: [0.0; 7] }
    }

    fn white(&mut self) -> f64 {
        self.state = self.state.wrapping_mul(1103515245).wrapping_add(12345);
        (self.state as i32) as f64 / i32::MAX as f64
    }
}

// ── Filters ──

/// ARP 4023 — 2-pole state-variable filter (12 dB/oct)
#[derive(Debug, Clone)]
struct Filter4023 {
    bp: f64,
    lp: f64,
}

impl Filter4023 {
    fn new() -> Self { Self { bp: 0.0, lp: 0.0 } }

    fn process(&mut self, input: f64, cutoff_norm: f64, resonance: f64, sr: f64) -> f64 {
        let freq = 20.0 * (1750.0f64).powf(cutoff_norm.clamp(0.0, 1.0)); // up to ~35kHz
        let f = (std::f64::consts::PI * freq / sr).sin().min(0.99) * 2.0;
        let q = (1.0 - resonance.clamp(0.0, 0.99)) * 2.0; // damping

        let hp = input - self.lp - q * self.bp;
        self.bp += f * tanh_approx(hp);
        self.lp += f * tanh_approx(self.bp);

        // Flush denormals
        if self.bp.abs() < 1e-18 { self.bp = 0.0; }
        if self.lp.abs() < 1e-18 { self.lp = 0.0; }

        self.lp
    }

    fn reset(&mut self) { self.bp = 0.0; self.lp = 0.0; }
}

/// ARP 4035 — 4-pole Moog-style transistor ladder (24 dB/oct)
#[derive(Debug, Clone)]
struct Filter4035 {
    s: [f64; 4],
}

impl Filter4035 {
    fn new() -> Self { Self { s: [0.0; 4] } }

    fn process(&mut self, input: f64, cutoff_norm: f64, resonance: f64, sr: f64) -> f64 {
        let freq = 20.0 * (1000.0f64).powf(cutoff_norm.clamp(0.0, 1.0));
        let g = (std::f64::consts::PI * freq / sr).tan().min(0.99);
        let res = resonance.clamp(0.0, 1.0) * 4.0;

        // No Q compensation — bass loss at resonance is authentic Moog behavior
        let fb = tanh_approx(self.s[3]);
        let input_comp = tanh_approx(input - res * fb);

        self.s[0] += g * (input_comp - tanh_approx(self.s[0]));
        self.s[1] += g * (tanh_approx(self.s[0]) - tanh_approx(self.s[1]));
        self.s[2] += g * (tanh_approx(self.s[1]) - tanh_approx(self.s[2]));
        self.s[3] += g * (tanh_approx(self.s[2]) - tanh_approx(self.s[3]));

        for s in &mut self.s { if s.abs() < 1e-18 { *s = 0.0; } }
        self.s[3]
    }

    fn reset(&mut self) { self.s = [0.0; 4]; }
}

/// ARP 4075 — 4-pole Norton op-amp cascaded integrator (24 dB/oct)
#[derive(Debug, Clone)]
struct Filter4075 {
    s: [f64; 4],
}

impl Filter4075 {
    fn new() -> Self { Self { s: [0.0; 4] } }

    fn process(&mut self, input: f64, cutoff_norm: f64, resonance: f64, sr: f64) -> f64 {
        // Max cutoff ~14kHz (authentic 4075 limitation)
        let freq = 20.0 * (700.0f64).powf(cutoff_norm.clamp(0.0, 1.0));
        let g = (std::f64::consts::PI * freq / sr).tan().min(0.99);
        let res = resonance.clamp(0.0, 1.0) * 4.0;

        let fb = self.s[3];
        let inp = soft_clip(input - res * fb);

        self.s[0] += g * soft_clip(inp - self.s[0]);
        self.s[1] += g * soft_clip(self.s[0] - self.s[1]);
        self.s[2] += g * soft_clip(self.s[1] - self.s[2]);
        self.s[3] += g * soft_clip(self.s[2] - self.s[3]);

        for s in &mut self.s { if s.abs() < 1e-18 { *s = 0.0; } }
        self.s[3]
    }

    fn reset(&mut self) { self.s = [0.0; 4]; }
}

/// HPF — 6 dB/oct non-resonant
#[derive(Debug, Clone)]
struct HpFilter {
    prev_in: f64,
    prev_out: f64,
}

impl HpFilter {
    fn new() -> Self { Self { prev_in: 0.0, prev_out: 0.0 } }

    fn process(&mut self, input: f64, cutoff_norm: f64, sr: f64) -> f64 {
        if cutoff_norm < 0.001 { return input; }
        let freq = 16.0 * (1000.0f64).powf(cutoff_norm.clamp(0.0, 1.0));
        let rc = 1.0 / (TWO_PI * freq);
        let dt = 1.0 / sr;
        let alpha = rc / (rc + dt);
        let out = alpha * (self.prev_out + input - self.prev_in);
        self.prev_in = input;
        self.prev_out = if out.abs() < 1e-18 { 0.0 } else { out };
        self.prev_out
    }

    fn reset(&mut self) { self.prev_in = 0.0; self.prev_out = 0.0; }
}

// ── Exponential ADSR Envelope ──

#[derive(Debug, Clone, Copy, PartialEq)]
enum EnvStage { Idle, Attack, Decay, Sustain, Release }

#[derive(Debug, Clone)]
struct AdsrEnvelope {
    stage: EnvStage,
    level: f64,
    attack: f64,
    decay: f64,
    sustain: f64,
    release: f64,
    sample_rate: f64,
}

impl AdsrEnvelope {
    fn new(sr: f64) -> Self {
        Self { stage: EnvStage::Idle, level: 0.0,
               attack: 0.01, decay: 0.3, sustain: 0.7, release: 0.2, sample_rate: sr }
    }

    fn trigger(&mut self) { self.stage = EnvStage::Attack; }
    fn release_env(&mut self) { if self.stage != EnvStage::Idle { self.stage = EnvStage::Release; } }
    fn kill(&mut self) { self.stage = EnvStage::Idle; self.level = 0.0; }
    fn is_active(&self) -> bool { self.stage != EnvStage::Idle }

    fn tick(&mut self) -> f64 {
        let sr = self.sample_rate;
        match self.stage {
            EnvStage::Idle => 0.0,
            EnvStage::Attack => {
                let rate = exp_rate(self.attack, sr);
                self.level += rate * (1.3 - self.level);
                if self.level >= 1.0 { self.level = 1.0; self.stage = EnvStage::Decay; }
                self.level
            }
            EnvStage::Decay => {
                let rate = exp_rate(self.decay, sr);
                self.level += rate * (self.sustain - self.level);
                if (self.level - self.sustain).abs() < 0.001 {
                    self.level = self.sustain;
                    self.stage = EnvStage::Sustain;
                }
                self.level
            }
            EnvStage::Sustain => self.sustain,
            EnvStage::Release => {
                let rate = exp_rate(self.release, sr);
                self.level += rate * (0.0 - self.level);
                if self.level < 0.001 { self.level = 0.0; self.stage = EnvStage::Idle; }
                self.level
            }
        }
    }
}

/// AR envelope — attack then immediate release (no sustain).
#[derive(Debug, Clone)]
struct ArEnvelope {
    stage: EnvStage,
    level: f64,
    attack: f64,
    release: f64,
    sample_rate: f64,
}

impl ArEnvelope {
    fn new(sr: f64) -> Self {
        Self { stage: EnvStage::Idle, level: 0.0, attack: 0.005, release: 0.3, sample_rate: sr }
    }

    fn trigger(&mut self) { self.stage = EnvStage::Attack; }
    fn release_env(&mut self) { if self.stage != EnvStage::Idle { self.stage = EnvStage::Release; } }
    fn kill(&mut self) { self.stage = EnvStage::Idle; self.level = 0.0; }
    fn is_active(&self) -> bool { self.stage != EnvStage::Idle }

    fn tick(&mut self) -> f64 {
        let sr = self.sample_rate;
        match self.stage {
            EnvStage::Idle => 0.0,
            EnvStage::Attack => {
                let rate = exp_rate(self.attack, sr);
                self.level += rate * (1.3 - self.level);
                if self.level >= 1.0 { self.level = 1.0; self.stage = EnvStage::Release; }
                self.level
            }
            // AR has no decay/sustain — goes straight to release after peak
            EnvStage::Decay | EnvStage::Sustain => {
                self.stage = EnvStage::Release;
                self.level
            }
            EnvStage::Release => {
                let rate = exp_rate(self.release, sr);
                self.level += rate * (0.0 - self.level);
                if self.level < 0.001 { self.level = 0.0; self.stage = EnvStage::Idle; }
                self.level
            }
        }
    }
}

fn exp_rate(time_secs: f64, sr: f64) -> f64 {
    if time_secs < 0.001 { return 1.0; }
    1.0 - (-1.0 / (time_secs * sr)).exp()
}

// ── LFO ──

#[derive(Debug, Clone)]
struct OdysseyLfo {
    phase: f64,
    rate: f64,
}

impl OdysseyLfo {
    fn new() -> Self { Self { phase: 0.0, rate: 1.0 } }

    /// Returns (sine, square).
    fn tick(&mut self, sr: f64) -> (f64, f64) {
        self.phase += self.rate / sr;
        if self.phase >= 1.0 { self.phase -= 1.0; }
        let sine = (self.phase * TWO_PI).sin();
        let square = if self.phase < 0.5 { 1.0 } else { -1.0 };
        (sine, square)
    }
}

// ── Sample & Hold ──

#[derive(Debug, Clone)]
struct SampleAndHold {
    held_value: f64,
    output: f64,
    lag_coeff: f64,
    prev_trigger: bool,
}

impl SampleAndHold {
    fn new() -> Self {
        Self { held_value: 0.0, output: 0.0, lag_coeff: 0.5, prev_trigger: false }
    }

    fn process(&mut self, input: f64, trigger_high: bool) -> f64 {
        // Sample on rising edge
        if trigger_high && !self.prev_trigger {
            self.held_value = input;
        }
        self.prev_trigger = trigger_high;
        // Lag (slew)
        self.output += (self.held_value - self.output) * self.lag_coeff;
        self.output
    }
}

// ── Duophonic Voice ──

/// The Odyssey is fundamentally a single voice with duophonic keyboard split.
/// VCO-1 plays the lowest held note, VCO-2 plays the highest.
/// Single note = both in unison.
#[derive(Debug)]
struct OdysseyVoice {
    vco1: OdysseyVco,
    vco2: OdysseyVco,
    noise: NoiseGen,
    hpf: HpFilter,
    filter_4023: Filter4023,
    filter_4035: Filter4035,
    filter_4075: Filter4075,
    adsr: AdsrEnvelope,
    ar: ArEnvelope,
    lfo: OdysseyLfo,
    sh: SampleAndHold,
    // Portamento state
    vco1_current_freq: f64,
    vco1_target_freq: f64,
    vco2_current_freq: f64,
    vco2_target_freq: f64,
    glide_coeff: f64,
    // Held notes for duophonic split
    held_notes: Vec<u8>,
    velocity: f64,
    gate: bool,
    sample_rate: f64,
    // Per-voice drift
    drift_phase1: f64,
    drift_phase2: f64,
}

impl OdysseyVoice {
    fn new(sr: f64) -> Self {
        Self {
            vco1: OdysseyVco::new(),
            vco2: OdysseyVco::new(),
            noise: NoiseGen::new(),
            hpf: HpFilter::new(),
            filter_4023: Filter4023::new(),
            filter_4035: Filter4035::new(),
            filter_4075: Filter4075::new(),
            adsr: AdsrEnvelope::new(sr),
            ar: ArEnvelope::new(sr),
            lfo: OdysseyLfo::new(),
            sh: SampleAndHold::new(),
            vco1_current_freq: 440.0,
            vco1_target_freq: 440.0,
            vco2_current_freq: 440.0,
            vco2_target_freq: 440.0,
            glide_coeff: 1.0,
            held_notes: Vec::with_capacity(16),
            velocity: 1.0,
            gate: false,
            sample_rate: sr,
            drift_phase1: 0.0,
            drift_phase2: 0.37,
        }
    }

    fn note_on(&mut self, note: u8, vel: u8, patch: &OdysseyPatch) {
        self.velocity = vel as f64 / 127.0;
        if !self.held_notes.contains(&note) {
            self.held_notes.push(note);
        }
        self.update_frequencies(patch);

        if !self.gate {
            // New gate — trigger envelopes
            self.adsr.trigger();
            self.ar.trigger();
            self.gate = true;
        }
        // If already gating (legato), don't retrigger envelopes
    }

    fn note_off(&mut self, note: u8, patch: &OdysseyPatch) {
        self.held_notes.retain(|&n| n != note);
        if self.held_notes.is_empty() {
            self.gate = false;
            self.adsr.release_env();
            self.ar.release_env();
        } else {
            self.update_frequencies(patch);
        }
    }

    fn update_frequencies(&mut self, patch: &OdysseyPatch) {
        if self.held_notes.is_empty() { return; }

        let lowest = *self.held_notes.iter().min().unwrap();
        let highest = *self.held_notes.iter().max().unwrap();

        self.vco1_target_freq = note_to_freq(lowest);
        self.vco2_target_freq = note_to_freq(highest);

        if patch.portamento > 0.01 {
            self.glide_coeff = exp_rate(patch.portamento * 1.5, self.sample_rate);
        } else {
            self.vco1_current_freq = self.vco1_target_freq;
            self.vco2_current_freq = self.vco2_target_freq;
            self.glide_coeff = 1.0;
        }
    }

    fn kill(&mut self) {
        self.held_notes.clear();
        self.gate = false;
        self.adsr.kill();
        self.ar.kill();
        self.hpf.reset();
        self.filter_4023.reset();
        self.filter_4035.reset();
        self.filter_4075.reset();
    }

    fn is_sounding(&self) -> bool {
        // Voice produces sound as long as the VCA envelope (AR or ADSR) is active
        self.ar.is_active() || self.adsr.is_active()
    }

    fn tick(&mut self, patch: &OdysseyPatch, user_cutoff: f64, user_reso: f64,
            user_env_mod: f64) -> f64 {
        if !self.is_sounding() { return 0.0; }

        let sr = self.sample_rate;

        // Portamento
        if self.glide_coeff < 1.0 {
            self.vco1_current_freq += self.glide_coeff * (self.vco1_target_freq - self.vco1_current_freq);
            self.vco2_current_freq += self.glide_coeff * (self.vco2_target_freq - self.vco2_current_freq);
        }

        // LFO
        self.lfo.rate = patch.lfo_rate;
        let (lfo_sin, lfo_sq) = self.lfo.tick(sr);

        // S&H — noise sampled at LFO rate
        let noise_val = self.noise.white();
        let sh_out = self.sh.process(noise_val, lfo_sq > 0.0);

        // Per-voice drift
        self.drift_phase1 += 0.23 / sr;
        self.drift_phase2 += 0.31 / sr;
        if self.drift_phase1 > 1.0 { self.drift_phase1 -= 1.0; }
        if self.drift_phase2 > 1.0 { self.drift_phase2 -= 1.0; }
        let drift1 = (self.drift_phase1 * TWO_PI).sin() * 1.5; // ±1.5 cents
        let drift2 = (self.drift_phase2 * TWO_PI).sin() * 1.5;

        // VCO frequencies with LFO vibrato, drift, detune
        let lfo_pitch_mod = lfo_sin * patch.lfo_to_pitch * 100.0;
        // S&H pitch mod (for S&H patch)
        let sh_pitch_mod = sh_out * patch.lfo_to_pitch * 200.0;
        let freq1 = self.vco1_current_freq * 2.0f64.powf((drift1 + lfo_pitch_mod + sh_pitch_mod) / 1200.0);
        let freq2 = self.vco2_current_freq * 2.0f64.powf((drift2 + lfo_pitch_mod + sh_pitch_mod + patch.detune_cents) / 1200.0);

        self.vco1.set_freq(freq1, sr);
        self.vco2.set_freq(freq2, sr);

        // PWM
        let pw = patch.pulse_width + lfo_sin * patch.lfo_to_pwm * 0.4;
        let pw = pw.clamp(0.05, 0.95);

        // Generate VCO-1
        let (saw1, pulse1, vco1_reset) = self.vco1.tick(pw);

        // Generate VCO-2 (with sync if enabled)
        if patch.sync && vco1_reset {
            self.vco2.reset_phase();
        }
        let (saw2, pulse2, _) = self.vco2.tick(pw);

        // Select waveforms
        let vco1_out = if patch.vco1_wave == 0 { saw1 } else { pulse1 };
        let vco2_out = if patch.vco2_wave == 0 { saw2 } else { pulse2 };

        // Ring mod (XOR of pulse waves)
        let ring_mod = -self.vco1.last_pulse * self.vco2.last_pulse;

        // Audio mixer
        let mixed = vco1_out * patch.vco1_level
            + vco2_out * patch.vco2_level
            + ring_mod * patch.ring_mod_level
            + noise_val * patch.noise_level;

        // HPF
        let hp_out = self.hpf.process(mixed, patch.hpf_cutoff, sr);

        // ADSR envelope
        let adsr_val = self.adsr.tick();
        let ar_val = self.ar.tick();

        // Filter cutoff modulation
        let note_center = if !self.held_notes.is_empty() {
            *self.held_notes.iter().min().unwrap() as f64
        } else { 60.0 };
        let key_follow = (note_center - 60.0) / 60.0 * patch.key_follow;
        let lfo_filter_mod = lfo_sin * patch.lfo_to_filter;
        let sh_filter_mod = sh_out * patch.lfo_to_filter * 0.5;
        let effective_cutoff = (user_cutoff
            + adsr_val * user_env_mod
            + key_follow
            + lfo_filter_mod
            + sh_filter_mod
        ).clamp(0.0, 1.0);

        // LPF (select filter type)
        let lp_out = match patch.filter_type {
            0 => self.filter_4023.process(hp_out, effective_cutoff, user_reso, sr),
            1 => self.filter_4035.process(hp_out, effective_cutoff, user_reso, sr),
            _ => self.filter_4075.process(hp_out, effective_cutoff, user_reso, sr),
        };

        // VCA — AR or ADSR
        let vca_env = if patch.use_adsr_for_vca { adsr_val } else { ar_val };
        let out = lp_out * vca_env * self.velocity;

        out
    }
}

// ── Odyssey Synth ──

pub struct OdysseySynth {
    voice: Option<OdysseyVoice>,
    sample_rate: f64,
    pub params: [f32; PARAM_COUNT],
    patches: [OdysseyPatch; PATCH_COUNT],
    last_patch_index: usize,
}

impl OdysseySynth {
    pub fn new() -> Self {
        let mut s = Self {
            voice: None,
            sample_rate: 44100.0,
            params: PARAM_DEFAULTS,
            patches: presets(),
            last_patch_index: usize::MAX, // force initial load
        };
        s.sync_params_from_patch();
        s
    }

    fn current_patch_index(&self) -> usize {
        let idx = (self.params[P_PATCH] * (PATCH_COUNT as f32 - 0.01)) as usize;
        idx.min(PATCH_COUNT - 1)
    }

    /// Get the param values for a given patch (for TUI sync).
    pub fn params_for_patch(patch_value: f32) -> [f32; PARAM_COUNT] {
        let idx = (patch_value * (PATCH_COUNT as f32 - 0.01)) as usize;
        let idx = idx.min(PATCH_COUNT - 1);
        let p = &presets()[idx];
        let mut params = PARAM_DEFAULTS;
        params[P_PATCH] = patch_value;
        params[P_VCO1_WAVE] = p.vco1_wave as f32 * 0.5;
        params[P_VCO2_WAVE] = p.vco2_wave as f32 * 0.5;
        params[P_DETUNE] = (p.detune_cents / 100.0 + 0.5) as f32;
        params[P_CUTOFF] = p.cutoff as f32;
        params[P_RESO] = p.resonance as f32;
        params[P_FILTER_TYPE] = p.filter_type as f32 / 2.99;
        params[P_ENV_MOD] = p.env_mod as f32;
        params[P_ATTACK] = ((p.adsr_a - 0.005) / 4.995).clamp(0.0, 1.0) as f32;
        params[P_DECAY] = ((p.adsr_d - 0.01) / 7.99).clamp(0.0, 1.0) as f32;
        params[P_SUSTAIN] = p.adsr_s as f32;
        params[P_RELEASE] = ((p.adsr_r - 0.015) / 9.985).clamp(0.0, 1.0) as f32;
        params[P_SYNC] = if p.sync { 1.0 } else { 0.0 };
        params[P_RING_MOD] = if p.ring_mod_level > 0.01 { 1.0 } else { 0.0 };
        params[P_LFO_RATE] = ((p.lfo_rate - 0.2) / 19.8).clamp(0.0, 1.0) as f32;
        params[P_GAIN] = PARAM_DEFAULTS[P_GAIN];
        params
    }

    /// When the patch selector changes, load preset values into user params
    /// so sliders reflect the patch character and user can tweak from there.
    fn sync_params_from_patch(&mut self) {
        let idx = self.current_patch_index();
        if idx == self.last_patch_index { return; }
        self.last_patch_index = idx;
        let p = &self.patches[idx];

        self.params[P_VCO1_WAVE] = p.vco1_wave as f32 * 0.5;
        self.params[P_VCO2_WAVE] = p.vco2_wave as f32 * 0.5;
        self.params[P_DETUNE] = (p.detune_cents / 100.0 + 0.5) as f32;
        self.params[P_CUTOFF] = p.cutoff as f32;
        self.params[P_RESO] = p.resonance as f32;
        self.params[P_FILTER_TYPE] = p.filter_type as f32 / 2.99;
        self.params[P_ENV_MOD] = p.env_mod as f32;
        self.params[P_ATTACK] = ((p.adsr_a - 0.005) / 4.995).clamp(0.0, 1.0) as f32;
        self.params[P_DECAY] = ((p.adsr_d - 0.01) / 7.99).clamp(0.0, 1.0) as f32;
        self.params[P_SUSTAIN] = p.adsr_s as f32;
        self.params[P_RELEASE] = ((p.adsr_r - 0.015) / 9.985).clamp(0.0, 1.0) as f32;
        self.params[P_SYNC] = if p.sync { 1.0 } else { 0.0 };
        self.params[P_RING_MOD] = if p.ring_mod_level > 0.01 { 1.0 } else { 0.0 };
        self.params[P_LFO_RATE] = ((p.lfo_rate - 0.2) / 19.8).clamp(0.0, 1.0) as f32;
    }

    fn active_patch(&self) -> OdysseyPatch {
        let mut p = self.patches[self.current_patch_index()];
        // All params driven by sliders (which were loaded from preset on patch change)
        p.vco1_wave = (self.params[P_VCO1_WAVE] * 2.0).min(1.0) as u8;
        p.vco2_wave = (self.params[P_VCO2_WAVE] * 2.0).min(1.0) as u8;
        p.detune_cents = (self.params[P_DETUNE] as f64 - 0.5) * 100.0;
        p.filter_type = (self.params[P_FILTER_TYPE] * 3.0).min(2.0) as u8;
        p.cutoff = self.params[P_CUTOFF] as f64;
        p.resonance = self.params[P_RESO] as f64;
        p.sync = self.params[P_SYNC] > 0.5;
        if self.params[P_RING_MOD] > 0.5 { p.ring_mod_level = p.ring_mod_level.max(0.8); }
        else { p.ring_mod_level = 0.0; }
        p.env_mod = self.params[P_ENV_MOD] as f64;
        p.adsr_a = 0.005 + self.params[P_ATTACK] as f64 * 4.995;
        p.adsr_d = 0.01 + self.params[P_DECAY] as f64 * 7.99;
        p.adsr_s = self.params[P_SUSTAIN] as f64;
        p.adsr_r = 0.015 + self.params[P_RELEASE] as f64 * 9.985;
        p.ar_a = p.adsr_a;
        p.ar_r = p.adsr_r;
        p.lfo_rate = 0.2 + self.params[P_LFO_RATE] as f64 * 19.8;
        p
    }
}

impl Default for OdysseySynth {
    fn default() -> Self { Self::new() }
}

impl Plugin for OdysseySynth {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "Odyssey".into(),
            version: "0.1.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voice = Some(OdysseyVoice::new(sample_rate));
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() || self.voice.is_none() { return; }

        let buf_len = outputs[0].len();
        let gain = self.params[P_GAIN];
        let patch = self.active_patch();
        let voice = self.voice.as_mut().unwrap();
        let user_cutoff = self.params[P_CUTOFF] as f64;
        let user_reso = self.params[P_RESO] as f64;
        let user_env_mod = self.params[P_ENV_MOD] as f64;

        // Configure envelopes
        voice.adsr.attack = patch.adsr_a;
        voice.adsr.decay = patch.adsr_d;
        voice.adsr.sustain = patch.adsr_s;
        voice.adsr.release = patch.adsr_r;
        voice.ar.attack = patch.ar_a;
        voice.ar.release = patch.ar_r;

        // Sort MIDI events (allocation-free)
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

        for i in 0..buf_len {
            while ei < event_count && midi_events[event_indices[ei]].sample_offset as usize <= i {
                let ev = &midi_events[event_indices[ei]];
                match ev.status & 0xF0 {
                    0x90 => {
                        if ev.data2 > 0 {
                            voice.note_on(ev.data1, ev.data2, &patch);
                        } else {
                            voice.note_off(ev.data1, &patch);
                        }
                    }
                    0x80 => voice.note_off(ev.data1, &patch),
                    0xB0 => match ev.data1 {
                        120 | 123 => voice.kill(),
                        _ => {}
                    }
                    _ => {}
                }
                ei += 1;
            }

            let sample = voice.tick(&patch, user_cutoff, user_reso, user_env_mod) as f32;
            let sample = (sample * gain).clamp(-1.0, 1.0);

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
        }
        // When patch changes, load preset values into all other params
        if index == P_PATCH {
            self.sync_params_from_patch();
        }
    }

    fn reset(&mut self) {
        if let Some(v) = self.voice.as_mut() { v.kill(); }
    }
}

fn note_to_freq(note: u8) -> f64 {
    440.0 * 2.0f64.powf((note as f64 - 69.0) / 12.0)
}

// ── Tests ──

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

    fn process_buffers(synth: &mut OdysseySynth, events: &[MidiEvent], count: usize) -> Vec<f32> {
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
        let mut s = OdysseySynth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = OdysseySynth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 4);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001, "Should produce sound, peak={peak}");
    }

    #[test]
    fn silent_after_release() {
        let mut s = OdysseySynth::new();
        s.init(44100.0, 64);
        s.set_parameter(P_RELEASE, 0.05); // short release BEFORE playing
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[note_off(60, 0)], 3000);
        let out = process_buffers(&mut s, &[], 1);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak < 0.001, "Should be silent after release, peak={peak}");
    }

    #[test]
    fn output_is_finite() {
        let mut s = OdysseySynth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 1000);
        assert!(out.iter().all(|v| v.is_finite()), "Output must be finite");
    }

    #[test]
    fn duophonic_split() {
        let mut s = OdysseySynth::new();
        s.init(44100.0, 64);
        // Play two notes — duophonic split
        let events = [note_on(48, 100, 0), note_on(72, 100, 0)];
        let out = process_buffers(&mut s, &events, 4);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001, "Duophonic should produce sound, peak={peak}");

        // Check voice state
        let voice = s.voice.as_ref().unwrap();
        assert_eq!(voice.held_notes.len(), 2);
    }

    #[test]
    fn all_patches_produce_sound() {
        for patch_idx in 0..PATCH_COUNT {
            let mut s = OdysseySynth::new();
            s.init(44100.0, 64);
            let patch_val = patch_idx as f32 / (PATCH_COUNT as f32 - 0.01);
            s.set_parameter(P_PATCH, patch_val);
            // Use enough buffers for slow-attack patches (up to ~2s attack)
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 2000);
            let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
            assert!(peak > 0.001, "Patch {} ({}) should produce sound, peak={peak}",
                patch_idx, PATCH_NAMES[patch_idx]);
        }
    }

    #[test]
    fn all_patches_finite() {
        for patch_idx in 0..PATCH_COUNT {
            let mut s = OdysseySynth::new();
            s.init(44100.0, 64);
            let patch_val = patch_idx as f32 / (PATCH_COUNT as f32 - 0.01);
            s.set_parameter(P_PATCH, patch_val);
            let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 500);
            assert!(out.iter().all(|v| v.is_finite()),
                "Patch {} ({}) must produce finite output", patch_idx, PATCH_NAMES[patch_idx]);
        }
    }

    #[test]
    fn all_filter_types_work() {
        for ft in 0..3 {
            let mut s = OdysseySynth::new();
            s.init(44100.0, 64);
            s.set_parameter(P_FILTER_TYPE, ft as f32 / 2.99);
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 8);
            let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
            assert!(peak > 0.001, "Filter type {ft} should produce sound, peak={peak}");
            assert!(out.iter().all(|v| v.is_finite()), "Filter type {ft} must be finite");
        }
    }

    #[test]
    fn cc120_kills() {
        let mut s = OdysseySynth::new();
        s.init(44100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[cc(120, 0, 0)], 1);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn all_params_readable() {
        let s = OdysseySynth::new();
        assert_eq!(s.parameter_count(), PARAM_COUNT);
        for i in 0..PARAM_COUNT {
            assert!(s.parameter_info(i).is_some());
            let val = s.get_parameter(i);
            assert!((0.0..=1.0).contains(&val), "param {i} = {val}");
        }
    }

    #[test]
    fn sync_changes_sound() {
        let mut s1 = OdysseySynth::new();
        s1.init(44100.0, 64);
        s1.set_parameter(P_SYNC, 0.0);
        let no_sync = process_buffers(&mut s1, &[note_on(60, 100, 0)], 8);

        let mut s2 = OdysseySynth::new();
        s2.init(44100.0, 64);
        s2.set_parameter(P_SYNC, 1.0);
        let with_sync = process_buffers(&mut s2, &[note_on(60, 100, 0)], 8);

        let diff: f32 = no_sync.iter().zip(with_sync.iter()).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 0.01, "Sync should change sound, diff={diff}");
    }

    #[test]
    fn sample_accurate_midi() {
        let mut s = OdysseySynth::new();
        s.init(44100.0, 128);
        s.set_parameter(P_ATTACK, 0.0); // fastest attack
        let mut out = vec![0.0f32; 128];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 64)]);
        let pre = out[..64].iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        let post = out[64..].iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(pre < 0.001, "Should be silent before note: {pre}");
        assert!(post > 0.001, "Should sound after note: {post}");
    }

    #[test]
    fn discrete_labels_correct() {
        assert_eq!(discrete_label(P_PATCH, 0.0), Some("Bass"));
        assert_eq!(discrete_label(P_VCO1_WAVE, 0.0), Some("saw"));
        assert_eq!(discrete_label(P_VCO1_WAVE, 0.9), Some("pulse"));
        assert_eq!(discrete_label(P_FILTER_TYPE, 0.0), Some("4023"));
        assert_eq!(discrete_label(P_FILTER_TYPE, 0.5), Some("4035"));
        assert_eq!(discrete_label(P_FILTER_TYPE, 1.0), Some("4075"));
        assert_eq!(discrete_label(P_SYNC, 0.0), Some("off"));
        assert_eq!(discrete_label(P_SYNC, 1.0), Some("on"));
    }
}
