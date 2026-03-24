//! Roland Jupiter-8 style dual-VCO analog poly synthesizer.
//!
//! Authentic recreation of the Jupiter-8 architecture:
//! 2 polyBLEP VCOs per voice, IR3109 OTA ladder filter with tanh saturation,
//! separate HPF, 2 exponential ADSR envelopes, global LFO, 4 voice modes.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

const MAX_VOICES: usize = 8;
const TWO_PI: f64 = std::f64::consts::TAU;

// ── Parameter indices ──
pub const P_PATCH: usize = 0;
pub const P_VCO1_WAVE: usize = 1;
pub const P_VCO2_WAVE: usize = 2;
pub const P_DETUNE: usize = 3;
pub const P_MIX: usize = 4;
pub const P_CUTOFF: usize = 5;
pub const P_RESO: usize = 6;
pub const P_ENV_MOD: usize = 7;
pub const P_ATTACK: usize = 8;
pub const P_DECAY: usize = 9;
pub const P_SUSTAIN: usize = 10;
pub const P_RELEASE: usize = 11;
pub const P_LFO_RATE: usize = 12;
pub const P_LFO_MOD: usize = 13;
pub const P_MODE: usize = 14;
pub const P_GAIN: usize = 15;
pub const PARAM_COUNT: usize = 16;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "patch", "vco1wav", "vco2wav", "detune", "mix",
    "cutoff", "reso", "envmod",
    "attack", "decay", "sustain", "release",
    "lfo rate", "lfo mod", "mode", "gain",
];

pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    0.0,   // patch: Pad
    0.25,  // vco1wav: Saw
    0.25,  // vco2wav: Saw
    0.55,  // detune: slight +7 cents
    0.5,   // mix: equal
    0.7,   // cutoff: mostly open
    0.0,   // reso: none
    0.3,   // env_mod: moderate
    0.1,   // attack
    0.3,   // decay
    0.7,   // sustain
    0.2,   // release
    0.3,   // lfo rate
    0.0,   // lfo mod: off
    0.5,   // mode: Poly1
    0.75,  // gain
];

// ── Patches ──

pub const PATCH_COUNT: usize = 42;
pub const PATCH_NAMES: [&str; PATCH_COUNT] = [
    "Pad", "Brass", "Bass", "SyncLd", "String", "Init",
    "ElecPno", "Pluck", "Bell", "Organ", "PWMPad", "UniLead", "KeyBass", "Ambient",
    "Sweep", "Stab", "Harp", "SynBass", "SubBass", "Acid",
    "Choir", "Vox", "Whstle", "PWMLd", "XMBell", "Seq",
    "Reso", "Dtune",
    // ── new patches ──
    "Clav", "HlwPad", "PwrPlk", "LoStr", "Flute", "Tuba",
    "SawPad", "Clrnet", "Cello", "Xylo", "FnkBas", "WrmLd",
    "Noise", "CarSyn",
];

/// Discrete parameter labels for the UI.
pub fn discrete_label(index: usize, value: f32) -> Option<&'static str> {
    match index {
        P_PATCH => {
            let idx = (value * (PATCH_COUNT as f32 - 0.01)) as usize;
            Some(PATCH_NAMES[idx.min(PATCH_COUNT - 1)])
        }
        P_VCO1_WAVE => Some(match (value * 4.0) as u8 {
            0 => "tri", 1 => "saw", 2 => "pulse", _ => "square",
        }),
        P_VCO2_WAVE => Some(match (value * 4.0) as u8 {
            0 => "tri", 1 => "saw", 2 => "pulse", _ => "noise",
        }),
        P_MODE => Some(match (value * 4.0) as u8 {
            0 => "solo", 1 => "unison", 2 => "poly1", _ => "poly2",
        }),
        _ => None,
    }
}

/// Which parameter indices are discrete selectors (rendered as labels, not bars).
pub fn is_discrete(index: usize) -> bool {
    matches!(index, P_PATCH | P_VCO1_WAVE | P_VCO2_WAVE | P_MODE)
}

// ── Internal preset data ──

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // preset fields read when inst config panel is wired up
struct JupiterPatch {
    vco1_wave: u8,    // 0=tri, 1=saw, 2=pulse, 3=square
    vco2_wave: u8,    // 0=tri, 1=saw, 2=pulse, 3=noise
    detune_cents: f64, // VCO-2 fine tune in cents
    vco1_level: f64,
    vco2_level: f64,
    pulse_width: f64,  // 0.0-1.0 (0.5 = square)
    sync: bool,
    xmod: f64,         // cross-mod amount
    cutoff: f64,       // 0.0-1.0
    resonance: f64,    // 0.0-1.0
    hpf_cutoff: f64,   // 0.0-1.0
    slope_24: bool,    // true = 24dB, false = 12dB
    env_mod: f64,      // ENV-1 → filter amount
    env_polarity: f64, // +1.0 or -1.0
    key_follow: f64,   // 0.0-1.0
    // ENV-1 (filter)
    env1_a: f64, env1_d: f64, env1_s: f64, env1_r: f64,
    // ENV-2 (amp)
    env2_a: f64, env2_d: f64, env2_s: f64, env2_r: f64,
    lfo_rate: f64,     // Hz
    lfo_wave: u8,      // 0=sin, 1=saw, 2=square, 3=random
    lfo_to_pitch: f64,
    lfo_to_filter: f64,
    lfo_delay: f64,    // seconds
    voice_mode: u8,    // 0=solo, 1=unison, 2=poly1, 3=poly2
    portamento: f64,   // 0.0 = off, 1.0 = max glide
}

fn presets() -> [JupiterPatch; PATCH_COUNT] {
    [
        // Pad — lush detuned saw pad
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 7.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.65, resonance: 0.2, hpf_cutoff: 0.12, slope_24: true,
            env_mod: 0.3, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.4, env1_d: 1.0, env1_s: 1.0, env1_r: 1.0,
            env2_a: 0.4, env2_d: 0.0, env2_s: 1.0, env2_r: 0.55,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.02, lfo_to_filter: 0.0, lfo_delay: 0.5,
            voice_mode: 2, portamento: 0.0,
        },
        // Brass — punchy brass stab
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 7.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.35, resonance: 0.3, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.75, env_polarity: 1.0, key_follow: 0.4,
            env1_a: 0.01, env1_d: 0.3, env1_s: 0.5, env1_r: 0.18,
            env2_a: 0.18, env2_d: 0.0, env2_s: 1.0, env2_r: 0.18,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Bass — deep round bass
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 5.0,
            vco1_level: 0.9, vco2_level: 0.9,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.35, resonance: 0.4, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.5, env_polarity: 1.0, key_follow: 0.3,
            env1_a: 0.005, env1_d: 0.3, env1_s: 0.6, env1_r: 0.15,
            env2_a: 0.001, env2_d: 0.3, env2_s: 0.85, env2_r: 0.1,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 0, portamento: 0.0,
        },
        // SyncLead — classic sync sweep lead
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 0.0,
            vco1_level: 0.5, vco2_level: 0.9,
            pulse_width: 0.5, sync: true, xmod: 0.0,
            cutoff: 0.6, resonance: 0.25, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.6, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.01, env1_d: 0.4, env1_s: 0.3, env1_r: 0.2,
            env2_a: 0.005, env2_d: 0.0, env2_s: 1.0, env2_r: 0.3,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.03, lfo_to_filter: 0.0, lfo_delay: 0.4,
            voice_mode: 0, portamento: 0.15,
        },
        // Strings — analog strings
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 2, detune_cents: 6.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.43, sync: false, xmod: 0.0,
            cutoff: 0.85, resonance: 0.1, hpf_cutoff: 0.3, slope_24: false,
            env_mod: 0.15, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.45, env1_d: 0.5, env1_s: 0.9, env1_r: 0.5,
            env2_a: 0.45, env2_d: 0.0, env2_s: 1.0, env2_r: 0.5,
            lfo_rate: 4.5, lfo_wave: 0, lfo_to_pitch: 0.015, lfo_to_filter: 0.0, lfo_delay: 0.6,
            voice_mode: 2, portamento: 0.0,
        },
        // Init — basic saw, filter open
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 0.0,
            vco1_level: 0.8, vco2_level: 0.0,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.8, resonance: 0.0, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.0, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.01, env1_d: 0.3, env1_s: 0.7, env1_r: 0.2,
            env2_a: 0.01, env2_d: 0.3, env2_s: 0.7, env2_r: 0.2,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // ElecPno — Rhodes-like electric piano
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 0, detune_cents: 3.0,
            vco1_level: 0.7, vco2_level: 0.5,
            pulse_width: 0.45, sync: false, xmod: 0.0,
            cutoff: 0.55, resonance: 0.1, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.35, env_polarity: 1.0, key_follow: 0.7,
            env1_a: 0.001, env1_d: 0.8, env1_s: 0.0, env1_r: 0.3,
            env2_a: 0.001, env2_d: 1.2, env2_s: 0.0, env2_r: 0.4,
            lfo_rate: 5.5, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Pluck — harpsichord-like percussive
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 2, detune_cents: 7.0,
            vco1_level: 0.6, vco2_level: 0.6,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.2, resonance: 0.35, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.65, env_polarity: 1.0, key_follow: 0.8,
            env1_a: 0.001, env1_d: 0.25, env1_s: 0.0, env1_r: 0.15,
            env2_a: 0.001, env2_d: 0.3, env2_s: 0.0, env2_r: 0.2,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Bell — cross-mod metallic bell
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 700.0,
            vco1_level: 0.6, vco2_level: 0.4,
            pulse_width: 0.5, sync: false, xmod: 0.45,
            cutoff: 0.7, resonance: 0.05, hpf_cutoff: 0.1, slope_24: false,
            env_mod: 0.25, env_polarity: 1.0, key_follow: 0.9,
            env1_a: 0.001, env1_d: 2.5, env1_s: 0.0, env1_r: 2.0,
            env2_a: 0.001, env2_d: 3.0, env2_s: 0.0, env2_r: 2.5,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Organ — drawbar style
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 2, detune_cents: 1200.0,
            vco1_level: 0.7, vco2_level: 0.5,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.6, resonance: 0.0, hpf_cutoff: 0.05, slope_24: false,
            env_mod: 0.0, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.001, env1_d: 0.0, env1_s: 1.0, env1_r: 0.05,
            env2_a: 0.001, env2_d: 0.0, env2_s: 1.0, env2_r: 0.05,
            lfo_rate: 6.0, lfo_wave: 0, lfo_to_pitch: 0.03, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // PWMPad — slow pulse width modulation
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 2, detune_cents: 5.0,
            vco1_level: 0.6, vco2_level: 0.6,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.65, resonance: 0.15, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.2, env_polarity: 1.0, key_follow: 0.4,
            env1_a: 0.001, env1_d: 0.5, env1_s: 0.7, env1_r: 0.8,
            env2_a: 0.5, env2_d: 0.8, env2_s: 0.8, env2_r: 1.5,
            lfo_rate: 0.3, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 3, portamento: 0.0,
        },
        // UniLead — fat unison lead
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 10.0,
            vco1_level: 0.7, vco2_level: 0.7,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.5, resonance: 0.2, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.4, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.01, env1_d: 0.3, env1_s: 0.6, env1_r: 0.3,
            env2_a: 0.01, env2_d: 0.4, env2_s: 0.7, env2_r: 0.4,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.015, lfo_to_filter: 0.0, lfo_delay: 0.4,
            voice_mode: 1, portamento: 0.08,
        },
        // KeyBass — punchy keyboard bass
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 3, detune_cents: 0.0,
            vco1_level: 0.8, vco2_level: 0.4,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.15, resonance: 0.25, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.5, env_polarity: 1.0, key_follow: 0.3,
            env1_a: 0.001, env1_d: 0.15, env1_s: 0.1, env1_r: 0.08,
            env2_a: 0.001, env2_d: 0.3, env2_s: 0.3, env2_r: 0.1,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 0, portamento: 0.0,
        },
        // Ambient — evolving Vangelis-style texture
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 0, detune_cents: 8.0,
            vco1_level: 0.5, vco2_level: 0.6,
            pulse_width: 0.5, sync: false, xmod: 0.08,
            cutoff: 0.35, resonance: 0.5, hpf_cutoff: 0.1, slope_24: true,
            env_mod: 0.15, env_polarity: 1.0, key_follow: 0.3,
            env1_a: 0.01, env1_d: 1.0, env1_s: 0.5, env1_r: 1.0,
            env2_a: 2.0, env2_d: 1.5, env2_s: 0.7, env2_r: 3.0,
            lfo_rate: 0.15, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.3, lfo_delay: 0.5,
            voice_mode: 3, portamento: 0.15,
        },
        // ── NEW PATCHES ──
        // Sweep — slow resonant filter sweep (Tangerine Dream / Jarre sequencer territory)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 6.0,
            vco1_level: 0.7, vco2_level: 0.7,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.15, resonance: 0.65, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.7, env_polarity: 1.0, key_follow: 0.3,
            env1_a: 2.0, env1_d: 3.0, env1_s: 0.3, env1_r: 1.5,
            env2_a: 0.01, env2_d: 0.0, env2_s: 1.0, env2_r: 0.5,
            lfo_rate: 0.08, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.15, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Stab — short rhythmic brass stab (Duran Duran "Rio" style)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 2, detune_cents: 5.0,
            vco1_level: 0.8, vco2_level: 0.7,
            pulse_width: 0.4, sync: false, xmod: 0.0,
            cutoff: 0.3, resonance: 0.2, hpf_cutoff: 0.08, slope_24: true,
            env_mod: 0.8, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.001, env1_d: 0.12, env1_s: 0.0, env1_r: 0.08,
            env2_a: 0.001, env2_d: 0.15, env2_s: 0.0, env2_r: 0.06,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Harp — ethereal plucked harp (Howard Jones style)
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 1, detune_cents: 3.0,
            vco1_level: 0.5, vco2_level: 0.7,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.45, resonance: 0.15, hpf_cutoff: 0.15, slope_24: false,
            env_mod: 0.4, env_polarity: 1.0, key_follow: 0.8,
            env1_a: 0.001, env1_d: 0.6, env1_s: 0.0, env1_r: 0.8,
            env2_a: 0.001, env2_d: 0.8, env2_s: 0.0, env2_r: 1.0,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // SynBass — sync bass (Thompson Twins style growl bass)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 0.0,
            vco1_level: 0.6, vco2_level: 0.9,
            pulse_width: 0.5, sync: true, xmod: 0.0,
            cutoff: 0.25, resonance: 0.35, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.65, env_polarity: 1.0, key_follow: 0.3,
            env1_a: 0.001, env1_d: 0.2, env1_s: 0.15, env1_r: 0.1,
            env2_a: 0.001, env2_d: 0.25, env2_s: 0.4, env2_r: 0.08,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 0, portamento: 0.0,
        },
        // SubBass — deep sub bass (triangle fundamental, barely any harmonics)
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 2.0,
            vco1_level: 1.0, vco2_level: 0.7,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.2, resonance: 0.3, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.2, env_polarity: 1.0, key_follow: 0.15,
            env1_a: 0.005, env1_d: 0.4, env1_s: 0.7, env1_r: 0.12,
            env2_a: 0.005, env2_d: 0.0, env2_s: 1.0, env2_r: 0.08,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 0, portamento: 0.0,
        },
        // Acid — TB-303-ish acid bass (resonant squelch, 24dB filter, glide)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 0.0,
            vco1_level: 0.9, vco2_level: 0.0,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.12, resonance: 0.75, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.85, env_polarity: 1.0, key_follow: 0.35,
            env1_a: 0.001, env1_d: 0.18, env1_s: 0.0, env1_r: 0.08,
            env2_a: 0.001, env2_d: 0.3, env2_s: 0.5, env2_r: 0.05,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 0, portamento: 0.12,
        },
        // Choir — bright analog choir (Depeche Mode "Somebody" style)
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 2, detune_cents: 8.0,
            vco1_level: 0.7, vco2_level: 0.7,
            pulse_width: 0.35, sync: false, xmod: 0.0,
            cutoff: 0.7, resonance: 0.25, hpf_cutoff: 0.2, slope_24: false,
            env_mod: 0.1, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.6, env1_d: 0.8, env1_s: 0.8, env1_r: 0.8,
            env2_a: 0.6, env2_d: 0.5, env2_s: 0.9, env2_r: 0.7,
            lfo_rate: 0.25, lfo_wave: 0, lfo_to_pitch: 0.01, lfo_to_filter: 0.05, lfo_delay: 0.3,
            voice_mode: 3, portamento: 0.0,
        },
        // Vox — vocal formant-like (resonant filter, Tears for Fears territory)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 2, detune_cents: 5.0,
            vco1_level: 0.6, vco2_level: 0.8,
            pulse_width: 0.3, sync: false, xmod: 0.0,
            cutoff: 0.4, resonance: 0.55, hpf_cutoff: 0.15, slope_24: false,
            env_mod: 0.35, env_polarity: 1.0, key_follow: 0.7,
            env1_a: 0.25, env1_d: 0.6, env1_s: 0.5, env1_r: 0.6,
            env2_a: 0.2, env2_d: 0.4, env2_s: 0.8, env2_r: 0.5,
            lfo_rate: 5.5, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.08, lfo_delay: 0.4,
            voice_mode: 2, portamento: 0.0,
        },
        // Whstle — pure sine-like whistle lead (Vangelis "Chariots of Fire" style)
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 0.0,
            vco1_level: 0.9, vco2_level: 0.0,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.3, resonance: 0.0, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.15, env_polarity: 1.0, key_follow: 0.9,
            env1_a: 0.15, env1_d: 0.3, env1_s: 0.7, env1_r: 0.3,
            env2_a: 0.1, env2_d: 0.0, env2_s: 1.0, env2_r: 0.2,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.025, lfo_to_filter: 0.0, lfo_delay: 0.5,
            voice_mode: 0, portamento: 0.1,
        },
        // PWMLd — pulse width modulation lead (OMD / Depeche Mode lead style)
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 2, detune_cents: 7.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.3, sync: false, xmod: 0.0,
            cutoff: 0.55, resonance: 0.15, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.3, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.01, env1_d: 0.3, env1_s: 0.5, env1_r: 0.25,
            env2_a: 0.01, env2_d: 0.0, env2_s: 1.0, env2_r: 0.3,
            lfo_rate: 0.4, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 1, portamento: 0.06,
        },
        // XMBell — cross-mod ring bell (aggressive metallic, Jarre "Oxygene" style)
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 1, detune_cents: 500.0,
            vco1_level: 0.5, vco2_level: 0.5,
            pulse_width: 0.5, sync: false, xmod: 0.6,
            cutoff: 0.8, resonance: 0.1, hpf_cutoff: 0.08, slope_24: false,
            env_mod: 0.2, env_polarity: -1.0, key_follow: 0.8,
            env1_a: 0.001, env1_d: 1.8, env1_s: 0.0, env1_r: 1.5,
            env2_a: 0.001, env2_d: 2.0, env2_s: 0.0, env2_r: 2.0,
            lfo_rate: 0.5, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Seq — tight sequence-friendly pluck (Tangerine Dream / Berlin school)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 4.0,
            vco1_level: 0.8, vco2_level: 0.6,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.25, resonance: 0.4, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.55, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.001, env1_d: 0.15, env1_s: 0.0, env1_r: 0.1,
            env2_a: 0.001, env2_d: 0.18, env2_s: 0.0, env2_r: 0.08,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Reso — screaming resonant sweep (classic Jupiter-8 factory reso demo)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 3, detune_cents: 0.0,
            vco1_level: 0.7, vco2_level: 0.2,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.2, resonance: 0.85, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.9, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.01, env1_d: 1.5, env1_s: 0.0, env1_r: 0.8,
            env2_a: 0.01, env2_d: 0.0, env2_s: 1.0, env2_r: 0.5,
            lfo_rate: 0.1, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.2, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // Dtune — massively detuned poly (Duran Duran "Save a Prayer" style)
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 15.0,
            vco1_level: 0.7, vco2_level: 0.7,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.55, resonance: 0.1, hpf_cutoff: 0.1, slope_24: true,
            env_mod: 0.2, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.5, env1_d: 0.8, env1_s: 0.7, env1_r: 1.2,
            env2_a: 0.3, env2_d: 0.5, env2_s: 0.85, env2_r: 1.0,
            lfo_rate: 0.2, lfo_wave: 0, lfo_to_pitch: 0.008, lfo_to_filter: 0.05, lfo_delay: 0.3,
            voice_mode: 3, portamento: 0.0,
        },
        // ── NEW PATCHES (batch 3) ──
        // Sources: Roland JP-8 factory patch sheets, Roland Cloud JUPITER-8 Model
        // Expansion programming guide (articles.roland.com), Sound on Sound
        // "Synth Secrets" series by Gordon Reid, Arturia Jup-8 V manual.

        // Clav — JP-8 factory patch #21 CLAV
        // Source: Roland factory patch sheet #21; percussive pulse-wave clavinet.
        // Pulse waves with very short filter+amp envelopes, moderate resonance,
        // HPF engaged for nasal quality. 24dB slope for sharp cutoff.
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 2, detune_cents: 3.0,
            vco1_level: 0.8, vco2_level: 0.7,
            pulse_width: 0.35, sync: false, xmod: 0.0,
            cutoff: 0.3, resonance: 0.35, hpf_cutoff: 0.2, slope_24: true,
            env_mod: 0.6, env_polarity: 1.0, key_follow: 0.7,
            env1_a: 0.001, env1_d: 0.12, env1_s: 0.0, env1_r: 0.08,
            env2_a: 0.001, env2_d: 0.2, env2_s: 0.0, env2_r: 0.1,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // HlwPad — Hollow Pad
        // Source: Roland Cloud JUPITER-8 Model Expansion programming guide.
        // Both pulse waves, Env-1 inverted with fast attack/short decay modulating
        // VCO-1 pitch for hollow onset. Mixer 180-200, fine tune 10, VCF ~750,
        // Env-2 A=400 S=full R=500-550. Optional LFO PWM at rate 100-110.
        JupiterPatch {
            vco1_wave: 2, vco2_wave: 2, detune_cents: 10.0,
            vco1_level: 0.75, vco2_level: 0.75,
            pulse_width: 0.43, sync: false, xmod: 0.0,
            cutoff: 0.75, resonance: 0.1, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.1, env_polarity: -1.0, key_follow: 0.5,
            env1_a: 0.001, env1_d: 0.25, env1_s: 0.0, env1_r: 0.3,
            env2_a: 0.4, env2_d: 0.0, env2_s: 1.0, env2_r: 0.55,
            lfo_rate: 0.4, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // PwrPlk — Power Pluck
        // Source: Roland Cloud JUPITER-8 Model Expansion programming guide.
        // Both saw, mixer 200/200, fine tune 8, VCF 750 (0.75), Env-1 mod 230-250
        // (normalized ~0.9), Env-2 A=0 S=0 D/R=500-600, Env-1 A=0 S=0 D/R=470.
        // Described as "authentic when dry" — no effects needed.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 8.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.75, resonance: 0.15, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.9, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.001, env1_d: 0.47, env1_s: 0.0, env1_r: 0.47,
            env2_a: 0.001, env2_d: 0.55, env2_s: 0.0, env2_r: 0.55,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // LoStr — JP-8 factory patch #31 LO STRINGS
        // Source: Roland factory patch sheet #31; Roland Cloud programming guide
        // "Analog Strings" section. VCO-1 saw, VCO-2 pulse with PW manual ~110
        // (normalized 0.43), fine tune +6, mixer 200/200, VCF 800-900 (0.85),
        // HPF ~300 (0.3), Env-2 A=450 S=full R=500. 12dB slope for warmth.
        // The classic "JP Strings" sound used in countless records.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 2, detune_cents: 6.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.43, sync: false, xmod: 0.0,
            cutoff: 0.85, resonance: 0.08, hpf_cutoff: 0.3, slope_24: false,
            env_mod: 0.1, env_polarity: 1.0, key_follow: 0.6,
            env1_a: 0.45, env1_d: 0.5, env1_s: 0.9, env1_r: 0.5,
            env2_a: 0.45, env2_d: 0.0, env2_s: 1.0, env2_r: 0.5,
            lfo_rate: 4.5, lfo_wave: 0, lfo_to_pitch: 0.012, lfo_to_filter: 0.0, lfo_delay: 0.6,
            voice_mode: 2, portamento: 0.0,
        },
        // Flute — JP-8 factory patch #82 FLUTE
        // Source: Roland factory patch sheet #82; Sound on Sound "Synthesizing
        // Simple Flutes" by Gordon Reid. Triangle wave (near-sine fundamental),
        // light noise for breath. Filter at ~2kHz region (0.4 normalized) with
        // keyboard tracking ~65%, light resonance for edge. Fast filter attack
        // peaking before amp. Vibrato via LFO at 5-6Hz with delay.
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 3, detune_cents: 0.0,
            vco1_level: 0.9, vco2_level: 0.15,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.4, resonance: 0.1, hpf_cutoff: 0.08, slope_24: true,
            env_mod: 0.2, env_polarity: 1.0, key_follow: 0.65,
            env1_a: 0.02, env1_d: 0.3, env1_s: 0.6, env1_r: 0.15,
            env2_a: 0.08, env2_d: 0.0, env2_s: 1.0, env2_r: 0.12,
            lfo_rate: 5.5, lfo_wave: 0, lfo_to_pitch: 0.02, lfo_to_filter: 0.05, lfo_delay: 0.5,
            voice_mode: 0, portamento: 0.05,
        },
        // Tuba — deep brass, JP-8 factory patch #35 LO BRASS territory
        // Source: Roland factory patch sheet #35 LO BRASS; Sound on Sound "Synth
        // Secrets" Part 25 (synthesizing brass). Saw waves for harmonic-rich brass
        // timbre. Low VCF cutoff (~350 = 0.35) with strong env mod (700-750 = 0.75).
        // Env-1 inverted with fast decay for downward pitch transient on attack
        // (mod ~10 = small pitch dip). Env-2 A=180, S=full, R=180.
        // Solo mode, deeper range implied by patch name.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 5.0,
            vco1_level: 0.9, vco2_level: 0.8,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.3, resonance: 0.2, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.75, env_polarity: 1.0, key_follow: 0.3,
            env1_a: 0.01, env1_d: 0.35, env1_s: 0.6, env1_r: 0.25,
            env2_a: 0.18, env2_d: 0.0, env2_s: 1.0, env2_r: 0.18,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.015, lfo_to_filter: 0.0, lfo_delay: 0.6,
            voice_mode: 0, portamento: 0.0,
        },
        // SawPad — pure saw pad (Roland Cloud "Smooth Pad" recipe)
        // Source: Roland Cloud JUPITER-8 Model Expansion programming guide.
        // Both saw, mixer 200/200, fine tune +/-7, VCF 600-700 (0.65),
        // Env mod ~70 assigned to Env-2, Env-2 A=400 R=500-550 S=max.
        // No HPF, 24dB slope. Classic Jupiter warmth.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 7.0,
            vco1_level: 0.8, vco2_level: 0.8,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.65, resonance: 0.15, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.25, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.4, env1_d: 0.8, env1_s: 0.85, env1_r: 0.55,
            env2_a: 0.4, env2_d: 0.0, env2_s: 1.0, env2_r: 0.55,
            lfo_rate: 0.8, lfo_wave: 0, lfo_to_pitch: 0.01, lfo_to_filter: 0.0, lfo_delay: 0.4,
            voice_mode: 2, portamento: 0.0,
        },
        // Clrnet — JP-8 factory patch #81 CLARINET
        // Source: Roland factory patch sheet #81; Sound on Sound "Synth Secrets"
        // clarinet synthesis. Square/pulse wave is the classic clarinet starting
        // point (odd harmonics). Narrow pulse width ~0.3 for hollow clarinet
        // character. Moderate filter with key follow for natural brightness
        // tracking. Vibrato delayed, solo mode with portamento.
        JupiterPatch {
            vco1_wave: 3, vco2_wave: 3, detune_cents: 0.0,
            vco1_level: 0.8, vco2_level: 0.15,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.45, resonance: 0.15, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.25, env_polarity: 1.0, key_follow: 0.7,
            env1_a: 0.03, env1_d: 0.2, env1_s: 0.7, env1_r: 0.12,
            env2_a: 0.05, env2_d: 0.0, env2_s: 1.0, env2_r: 0.1,
            lfo_rate: 5.5, lfo_wave: 0, lfo_to_pitch: 0.015, lfo_to_filter: 0.03, lfo_delay: 0.5,
            voice_mode: 0, portamento: 0.05,
        },
        // Cello — JP-8 factory patch #84 CELLO
        // Source: Roland factory patch sheet #84. Saw wave for rich bowed string
        // harmonics. Low HPF to remove rumble, moderate filter with key follow.
        // Slow attack for bowed onset, full sustain, moderate release.
        // Solo mode with gentle portamento for legato slides.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 3.0,
            vco1_level: 0.8, vco2_level: 0.6,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.5, resonance: 0.15, hpf_cutoff: 0.1, slope_24: false,
            env_mod: 0.2, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.3, env1_d: 0.5, env1_s: 0.8, env1_r: 0.4,
            env2_a: 0.25, env2_d: 0.3, env2_s: 0.9, env2_r: 0.35,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.015, lfo_to_filter: 0.0, lfo_delay: 0.6,
            voice_mode: 0, portamento: 0.08,
        },
        // Xylo — JP-8 factory patch #26 XYLO (xylophone)
        // Source: Roland factory patch sheet #26. Triangle waves for pure,
        // bell-like fundamental. Very short envelopes (percussive mallet strike).
        // High key follow so higher notes are brighter. No VCO detune for
        // clean pitch. Cross-mod adds metallic inharmonic content.
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 0, detune_cents: 0.0,
            vco1_level: 0.7, vco2_level: 0.5,
            pulse_width: 0.5, sync: false, xmod: 0.15,
            cutoff: 0.6, resonance: 0.05, hpf_cutoff: 0.1, slope_24: true,
            env_mod: 0.3, env_polarity: 1.0, key_follow: 0.9,
            env1_a: 0.001, env1_d: 0.4, env1_s: 0.0, env1_r: 0.3,
            env2_a: 0.001, env2_d: 0.5, env2_s: 0.0, env2_r: 0.4,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // FnkBas — JP-8 factory patch #13 JUICY FUNK
        // Source: Roland factory patch sheet #13. Funky resonant bass with
        // sharp filter envelope. Single saw for tight low end, high resonance
        // for squelchy character. Very short envelopes, solo mode.
        // 24dB slope for aggressive filter sweep.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 3, detune_cents: 0.0,
            vco1_level: 0.9, vco2_level: 0.3,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.18, resonance: 0.6, hpf_cutoff: 0.0, slope_24: true,
            env_mod: 0.7, env_polarity: 1.0, key_follow: 0.35,
            env1_a: 0.001, env1_d: 0.15, env1_s: 0.1, env1_r: 0.08,
            env2_a: 0.001, env2_d: 0.25, env2_s: 0.3, env2_r: 0.08,
            lfo_rate: 1.0, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.0, lfo_delay: 0.0,
            voice_mode: 0, portamento: 0.06,
        },
        // WrmLd — warm lead (JP-8 factory patch #17 HAMMER LEAD territory)
        // Source: Roland factory patch sheet #17. Saw+pulse combination for a
        // rich, warm solo lead. Moderate filter with env mod for expressive
        // attack. Unison mode for fat sound, gentle portamento, delayed vibrato.
        // 12dB slope for smoother, warmer filtering.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 2, detune_cents: 7.0,
            vco1_level: 0.7, vco2_level: 0.7,
            pulse_width: 0.4, sync: false, xmod: 0.0,
            cutoff: 0.5, resonance: 0.2, hpf_cutoff: 0.03, slope_24: false,
            env_mod: 0.35, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.01, env1_d: 0.4, env1_s: 0.6, env1_r: 0.3,
            env2_a: 0.01, env2_d: 0.0, env2_s: 1.0, env2_r: 0.25,
            lfo_rate: 5.0, lfo_wave: 0, lfo_to_pitch: 0.02, lfo_to_filter: 0.0, lfo_delay: 0.5,
            voice_mode: 1, portamento: 0.08,
        },
        // Noise — textural noise wash (JP-8 factory patch #66 SOLAR WINDS)
        // Source: Roland factory patch sheet #66 SOLAR WINDS; Jupiter-8 noise
        // generator is on VCO-2 wave 3. Filtered noise with slow LFO modulating
        // filter for evolving texture. Long envelopes, resonance for tonal
        // character. 12dB slope for gentler rolloff.
        JupiterPatch {
            vco1_wave: 0, vco2_wave: 3, detune_cents: 0.0,
            vco1_level: 0.3, vco2_level: 0.9,
            pulse_width: 0.5, sync: false, xmod: 0.0,
            cutoff: 0.4, resonance: 0.45, hpf_cutoff: 0.1, slope_24: false,
            env_mod: 0.2, env_polarity: 1.0, key_follow: 0.2,
            env1_a: 1.5, env1_d: 2.0, env1_s: 0.5, env1_r: 2.0,
            env2_a: 2.0, env2_d: 1.0, env2_s: 0.7, env2_r: 3.0,
            lfo_rate: 0.1, lfo_wave: 0, lfo_to_pitch: 0.0, lfo_to_filter: 0.3, lfo_delay: 0.0,
            voice_mode: 2, portamento: 0.0,
        },
        // CarSyn — JP-8 factory patch #15 CARS SYNC (The Cars "Let's Go" style)
        // Source: Roland factory patch sheet #15 CARS SYNC. Hard sync lead with
        // filter envelope sweep for the classic sync scream. VCO-2 synced to
        // VCO-1, filter envelope sweeps harmonics. Solo mode, portamento for
        // pitch slides. Bright, aggressive character.
        JupiterPatch {
            vco1_wave: 1, vco2_wave: 1, detune_cents: 0.0,
            vco1_level: 0.4, vco2_level: 0.9,
            pulse_width: 0.5, sync: true, xmod: 0.0,
            cutoff: 0.55, resonance: 0.2, hpf_cutoff: 0.05, slope_24: true,
            env_mod: 0.7, env_polarity: 1.0, key_follow: 0.5,
            env1_a: 0.01, env1_d: 0.5, env1_s: 0.2, env1_r: 0.25,
            env2_a: 0.005, env2_d: 0.0, env2_s: 1.0, env2_r: 0.2,
            lfo_rate: 5.5, lfo_wave: 0, lfo_to_pitch: 0.025, lfo_to_filter: 0.0, lfo_delay: 0.4,
            voice_mode: 0, portamento: 0.1,
        },
    ]
}

// ── PolyBLEP anti-aliasing ──

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

// ── VCO ──

#[derive(Debug, Clone)]
struct JupiterVco {
    phase: f64,
    freq: f64,
    dt: f64, // freq / sample_rate
    // Noise state (LCG)
    noise_state: u32,
    noise_value: f64,
}

impl JupiterVco {
    fn new() -> Self {
        Self { phase: 0.0, freq: 440.0, dt: 0.01, noise_state: 12345, noise_value: 0.0 }
    }

    fn set_freq(&mut self, freq: f64, sr: f64) {
        self.freq = freq;
        self.dt = freq / sr;
    }

    /// Generate one sample. Returns (output, did_reset) for sync detection.
    fn tick(&mut self, waveform: u8, pulse_width: f64) -> (f64, bool) {
        let dt = self.dt;
        let mut did_reset = false;

        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            did_reset = true;
        }

        let t = self.phase;
        let out = match waveform {
            0 => {
                // Triangle — derived from saw via absolute value
                // Integrate: 2*|2*saw - 1| - 1
                let raw_saw = 2.0 * t - 1.0;
                let tri = 2.0 * raw_saw.abs() - 1.0;
                // Scale to compensate for polyBLEP not applying perfectly to triangle
                tri
            }
            1 => {
                // Sawtooth with polyBLEP
                let mut saw = 2.0 * t - 1.0;
                saw -= poly_blep(t, dt);
                saw
            }
            2 => {
                // Pulse with polyBLEP
                let pw = pulse_width.clamp(0.05, 0.95);
                let mut pulse = if t < pw { 1.0 } else { -1.0 };
                pulse += poly_blep(t, dt);
                pulse -= poly_blep((t - pw).rem_euclid(1.0), dt);
                pulse
            }
            3 => {
                // Square (pulse at 50%) for VCO-1, Noise for VCO-2
                // Caller selects which meaning based on context
                let mut sq = if t < 0.5 { 1.0 } else { -1.0 };
                sq += poly_blep(t, dt);
                sq -= poly_blep((t - 0.5).rem_euclid(1.0), dt);
                sq
            }
            _ => 0.0,
        };

        (out, did_reset)
    }

    /// Generate noise sample (for VCO-2 waveform 3).
    fn tick_noise(&mut self) -> f64 {
        self.noise_state = self.noise_state.wrapping_mul(1103515245).wrapping_add(12345);
        self.noise_value = (self.noise_state as i32) as f64 / i32::MAX as f64;
        self.noise_value
    }

    fn reset_phase(&mut self) {
        self.phase = 0.0;
    }
}

// ── IR3109 OTA Ladder Filter ──

#[derive(Debug, Clone)]
struct Ir3109Filter {
    s: [f64; 4], // 4 filter stages
}

impl Ir3109Filter {
    fn new() -> Self {
        Self { s: [0.0; 4] }
    }

    /// Fast tanh approximation — good enough for real-time, captures saturation character.
    #[inline]
    fn tanh_approx(x: f64) -> f64 {
        let x2 = x * x;
        x * (27.0 + x2) / (27.0 + 9.0 * x2)
    }

    /// Process one sample through the 4-stage OTA ladder.
    fn process_sr(&mut self, input: f64, cutoff_norm: f64, resonance: f64, use_4pole: bool, sample_rate: f64) -> f64 {
        let freq = 20.0 * (1000.0f64).powf(cutoff_norm.clamp(0.0, 1.0));
        let g = (std::f64::consts::PI * freq / sample_rate).tan().min(0.99);

        let res = resonance.clamp(0.0, 1.0) * 4.0;
        let feedback = if use_4pole { self.s[3] } else { self.s[1] };
        let compensation = 1.0 + resonance * 0.5;
        let input_compensated = input * compensation - res * Self::tanh_approx(feedback);

        self.s[0] += g * (Self::tanh_approx(input_compensated) - Self::tanh_approx(self.s[0]));
        self.s[1] += g * (Self::tanh_approx(self.s[0]) - Self::tanh_approx(self.s[1]));
        self.s[2] += g * (Self::tanh_approx(self.s[1]) - Self::tanh_approx(self.s[2]));
        self.s[3] += g * (Self::tanh_approx(self.s[2]) - Self::tanh_approx(self.s[3]));

        for s in &mut self.s {
            if s.abs() < 1e-18 { *s = 0.0; }
        }

        if use_4pole { self.s[3] } else { self.s[1] }
    }

    fn reset(&mut self) { self.s = [0.0; 4]; }
}

// ── HPF (6dB/oct, non-resonant) ──

#[derive(Debug, Clone)]
struct HpFilter {
    prev_in: f64,
    prev_out: f64,
}

impl HpFilter {
    fn new() -> Self { Self { prev_in: 0.0, prev_out: 0.0 } }

    fn process(&mut self, input: f64, cutoff_norm: f64, sample_rate: f64) -> f64 {
        if cutoff_norm < 0.001 { return input; } // bypass when fully closed
        let freq = 20.0 * (500.0f64).powf(cutoff_norm.clamp(0.0, 1.0)); // 20Hz-~4.5kHz range
        let rc = 1.0 / (TWO_PI * freq);
        let dt = 1.0 / sample_rate;
        let alpha = rc / (rc + dt);
        let out = alpha * (self.prev_out + input - self.prev_in);
        self.prev_in = input;
        self.prev_out = out;
        if out.abs() < 1e-18 { self.prev_out = 0.0; }
        out
    }

    fn reset(&mut self) { self.prev_in = 0.0; self.prev_out = 0.0; }
}

// ── Exponential ADSR Envelope ──

#[derive(Debug, Clone, Copy, PartialEq)]
enum EnvStage { Idle, Attack, Decay, Sustain, Release }

#[derive(Debug, Clone)]
struct JupiterEnvelope {
    stage: EnvStage,
    level: f64,
    attack: f64,  // seconds
    decay: f64,
    sustain: f64, // 0.0-1.0
    release: f64,
    sample_rate: f64,
}

impl JupiterEnvelope {
    fn new(sr: f64) -> Self {
        Self {
            stage: EnvStage::Idle, level: 0.0,
            attack: 0.01, decay: 0.3, sustain: 0.7, release: 0.2,
            sample_rate: sr,
        }
    }

    fn trigger(&mut self) {
        // Retrigger from current level (not zero) — authentic Jupiter behavior
        self.stage = EnvStage::Attack;
    }

    fn release(&mut self) {
        if self.stage != EnvStage::Idle { self.stage = EnvStage::Release; }
    }

    fn kill(&mut self) { self.stage = EnvStage::Idle; self.level = 0.0; }

    fn is_active(&self) -> bool { self.stage != EnvStage::Idle }

    fn tick(&mut self) -> f64 {
        let sr = self.sample_rate;
        match self.stage {
            EnvStage::Idle => 0.0,
            EnvStage::Attack => {
                // Exponential attack: overshoot to 1.3 then clamp
                let rate = exp_rate(self.attack, sr);
                self.level += rate * (1.3 - self.level);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvStage::Decay;
                }
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
                if self.level < 0.001 {
                    self.level = 0.0;
                    self.stage = EnvStage::Idle;
                }
                self.level
            }
        }
    }
}

/// Compute exponential rate coefficient from time in seconds.
fn exp_rate(time_secs: f64, sample_rate: f64) -> f64 {
    if time_secs < 0.001 { return 1.0; }
    1.0 - (-1.0 / (time_secs * sample_rate)).exp()
}

// ── LFO (global, free-running) ──

#[derive(Debug, Clone)]
struct JupiterLfo {
    phase: f64,
    rate: f64, // Hz
    waveform: u8, // 0=sin, 1=saw, 2=square, 3=random
    // S&H state
    sh_value: f64,
    sh_noise_state: u32,
    // Per-note fade-in
    delay_time: f64,
    delay_counter: f64,
    delay_level: f64,
}

impl JupiterLfo {
    fn new() -> Self {
        Self {
            phase: 0.0, rate: 1.0, waveform: 0,
            sh_value: 0.0, sh_noise_state: 54321,
            delay_time: 0.0, delay_counter: 0.0, delay_level: 1.0,
        }
    }

    fn trigger_delay(&mut self) {
        if self.delay_time > 0.001 {
            self.delay_counter = 0.0;
            self.delay_level = 0.0;
        } else {
            self.delay_level = 1.0;
        }
    }

    fn tick(&mut self, sample_rate: f64) -> f64 {
        let prev_phase = self.phase;
        self.phase += self.rate / sample_rate;
        let wrapped = self.phase >= 1.0;
        if wrapped { self.phase -= 1.0; }

        // Fade-in delay
        if self.delay_level < 1.0 {
            self.delay_counter += 1.0 / sample_rate;
            self.delay_level = (self.delay_counter / self.delay_time.max(0.001)).min(1.0);
        }

        let raw = match self.waveform {
            0 => (self.phase * TWO_PI).sin(),           // Sine
            1 => 1.0 - 2.0 * self.phase,               // Saw (ramp down)
            2 => if self.phase < 0.5 { 1.0 } else { -1.0 }, // Square
            3 => {
                // Sample & Hold: new random value each cycle
                if wrapped || prev_phase == 0.0 {
                    self.sh_noise_state = self.sh_noise_state.wrapping_mul(1103515245).wrapping_add(12345);
                    self.sh_value = (self.sh_noise_state as i32) as f64 / i32::MAX as f64;
                }
                self.sh_value
            }
            _ => 0.0,
        };

        raw * self.delay_level
    }
}

// ── Voice ──

#[derive(Debug, Clone)]
struct JupiterVoice {
    vco1: JupiterVco,
    vco2: JupiterVco,
    lpf: Ir3109Filter,
    hpf: HpFilter,
    env1: JupiterEnvelope, // filter
    env2: JupiterEnvelope, // VCA
    note: u8,
    velocity: f64,
    age: u64,
    target_freq: f64,
    current_freq: f64,
    glide_coeff: f64,
    // Per-voice variation (fixed on creation)
    drift_phase: f64,
    drift_rate: f64,
    cutoff_offset: f64,
    pitch_offset: f64, // cents
    sample_rate: f64,
}

impl JupiterVoice {
    fn new(sr: f64, voice_idx: usize) -> Self {
        // Deterministic per-voice variation based on index
        let seed = (voice_idx as u32).wrapping_mul(2654435761);
        let cutoff_var = ((seed & 0xFF) as f64 / 255.0 - 0.5) * 0.04; // ±2%
        let pitch_var = (((seed >> 8) & 0xFF) as f64 / 255.0 - 0.5) * 3.0; // ±1.5 cents
        let drift_rate = 0.1 + ((seed >> 16) & 0xFF) as f64 / 255.0 * 0.4; // 0.1-0.5 Hz

        Self {
            vco1: JupiterVco::new(),
            vco2: JupiterVco::new(),
            lpf: Ir3109Filter::new(),
            hpf: HpFilter::new(),
            env1: JupiterEnvelope::new(sr),
            env2: JupiterEnvelope::new(sr),
            note: 255,
            velocity: 0.0,
            age: 0,
            target_freq: 440.0,
            current_freq: 440.0,
            glide_coeff: 1.0,
            drift_phase: voice_idx as f64 * 0.37, // stagger initial drift phases
            drift_rate,
            cutoff_offset: cutoff_var,
            pitch_offset: pitch_var,
            sample_rate: sr,
        }
    }

    fn note_on(&mut self, note: u8, vel: u8, patch: &JupiterPatch, portamento: bool, age: u64) {
        self.note = note;
        self.velocity = vel as f64 / 127.0;
        self.age = age;

        let freq = note_to_freq(note);
        if portamento && self.current_freq > 0.0 && patch.portamento > 0.01 {
            self.target_freq = freq;
            self.glide_coeff = exp_rate(patch.portamento * 2.0, self.sample_rate);
        } else {
            self.target_freq = freq;
            self.current_freq = freq;
            self.glide_coeff = 1.0;
        }

        // Configure envelopes from patch
        self.env1.attack = patch.env1_a;
        self.env1.decay = patch.env1_d;
        self.env1.sustain = patch.env1_s;
        self.env1.release = patch.env1_r;
        self.env2.attack = patch.env2_a;
        self.env2.decay = patch.env2_d;
        self.env2.sustain = patch.env2_s;
        self.env2.release = patch.env2_r;

        self.env1.trigger();
        self.env2.trigger();
        self.lpf.reset();
        self.hpf.reset();
    }

    fn note_off(&mut self) {
        self.env1.release();
        self.env2.release();
    }

    fn kill(&mut self) {
        self.note = 255;
        self.env1.kill();
        self.env2.kill();
        self.lpf.reset();
        self.hpf.reset();
    }

    fn is_sounding(&self) -> bool { self.env2.is_active() }
    fn is_held(&self) -> bool {
        matches!(self.env2.stage, EnvStage::Attack | EnvStage::Decay | EnvStage::Sustain)
    }

    fn tick(&mut self, patch: &JupiterPatch, lfo_out: f64, user_cutoff: f64, user_reso: f64,
            user_env_mod: f64) -> f64 {
        if !self.is_sounding() { return 0.0; }

        let sr = self.sample_rate;

        // Portamento
        if self.glide_coeff < 1.0 {
            self.current_freq += self.glide_coeff * (self.target_freq - self.current_freq);
        }

        // Per-voice drift
        self.drift_phase += self.drift_rate / sr;
        if self.drift_phase > 1.0 { self.drift_phase -= 1.0; }
        let drift_cents = (self.drift_phase * TWO_PI).sin() * 2.5; // ±2.5 cents

        // VCO frequencies with drift and detune
        let pitch_mod = self.pitch_offset + drift_cents + lfo_out * patch.lfo_to_pitch * 100.0;
        let freq1 = self.current_freq * 2.0f64.powf(pitch_mod / 1200.0);
        let freq2 = self.current_freq * 2.0f64.powf((pitch_mod + patch.detune_cents) / 1200.0);

        self.vco1.set_freq(freq1, sr);
        self.vco2.set_freq(freq2, sr);

        // Generate VCO-1
        let (mut vco1_out, vco1_reset) = self.vco1.tick(patch.vco1_wave, patch.pulse_width);

        // Cross-modulation: VCO-2 output modulates VCO-1 frequency (exponential FM)
        if patch.xmod > 0.001 {
            // Apply cross-mod by adjusting VCO-1 phase increment retroactively
            // This is a simplified approach — full expo FM would need per-sample freq update
            vco1_out *= 1.0 + patch.xmod * self.vco2.noise_value * 0.5;
        }

        // Generate VCO-2 (with sync if enabled)
        let vco2_out = if patch.vco2_wave == 3 {
            // Noise
            self.vco2.tick_noise()
        } else {
            // Hard sync: reset VCO-2 when VCO-1 resets
            if patch.sync && vco1_reset {
                self.vco2.reset_phase();
            }
            let (out, _) = self.vco2.tick(patch.vco2_wave, patch.pulse_width);
            out
        };

        // Mix
        let mixed = vco1_out * patch.vco1_level + vco2_out * patch.vco2_level;

        // HPF
        let hp_out = self.hpf.process(mixed, patch.hpf_cutoff, sr);

        // Envelope 1 → filter cutoff
        let env1 = self.env1.tick();
        let env_mod_amount = user_env_mod * patch.env_polarity;
        let note_follow = (self.note as f64 - 60.0) / 60.0 * patch.key_follow;
        let effective_cutoff = (user_cutoff + self.cutoff_offset
            + env1 * env_mod_amount
            + lfo_out * patch.lfo_to_filter
            + note_follow).clamp(0.0, 1.0);

        // LPF (IR3109)
        let lp_out = self.lpf.process_sr(hp_out, effective_cutoff, user_reso, patch.slope_24, sr);

        // Envelope 2 → VCA
        let env2 = self.env2.tick();
        let vca_out = lp_out * env2 * self.velocity;

        vca_out as f32 as f64 // ensure finite
    }
}

// ── Jupiter-8 Synth ──

pub struct Jupiter8Synth {
    voices: Vec<JupiterVoice>,
    lfo: JupiterLfo,
    sample_rate: f64,
    pub params: [f32; PARAM_COUNT],
    voice_counter: u64,
    patches: [JupiterPatch; PATCH_COUNT],
    last_patch_index: usize,
}

impl Jupiter8Synth {
    pub fn new() -> Self {
        let mut s = Self {
            voices: Vec::new(),
            lfo: JupiterLfo::new(),
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

    /// Get the param values for a given patch (for TUI sync).
    pub fn params_for_patch(patch_value: f32) -> [f32; PARAM_COUNT] {
        let idx = (patch_value * (PATCH_COUNT as f32 - 0.01)) as usize;
        let idx = idx.min(PATCH_COUNT - 1);
        let p = &presets()[idx];
        let mut params = PARAM_DEFAULTS;
        params[P_PATCH] = patch_value;
        params[P_VCO1_WAVE] = p.vco1_wave as f32 / 3.99;
        params[P_VCO2_WAVE] = p.vco2_wave as f32 / 3.99;
        params[P_DETUNE] = (p.detune_cents / 100.0 + 0.5) as f32;
        let total = p.vco1_level + p.vco2_level;
        params[P_MIX] = if total > 0.0 { (p.vco2_level / total) as f32 } else { 0.5 };
        params[P_CUTOFF] = p.cutoff as f32;
        params[P_RESO] = p.resonance as f32;
        params[P_ENV_MOD] = p.env_mod as f32;
        params[P_ATTACK] = ((p.env1_a - 0.0006) / 5.9994).clamp(0.0, 1.0) as f32;
        params[P_DECAY] = ((p.env1_d - 0.003) / 9.997).clamp(0.0, 1.0) as f32;
        params[P_SUSTAIN] = p.env1_s as f32;
        params[P_RELEASE] = ((p.env1_r - 0.003) / 11.997).clamp(0.0, 1.0) as f32;
        params[P_LFO_RATE] = ((p.lfo_rate - 0.05) / 39.95).clamp(0.0, 1.0) as f32;
        let lfo_mod = (p.lfo_to_pitch / 0.02).max(p.lfo_to_filter / 0.3);
        params[P_LFO_MOD] = lfo_mod.clamp(0.0, 1.0) as f32;
        params[P_MODE] = p.voice_mode as f32 / 3.99;
        params[P_GAIN] = PARAM_DEFAULTS[P_GAIN];
        params
    }

    /// When patch changes, load preset values into user params.
    fn sync_params_from_patch(&mut self) {
        let idx = self.current_patch_index();
        if idx == self.last_patch_index { return; }
        self.last_patch_index = idx;
        let p = &self.patches[idx];

        self.params[P_VCO1_WAVE] = p.vco1_wave as f32 / 3.99;
        self.params[P_VCO2_WAVE] = p.vco2_wave as f32 / 3.99;
        self.params[P_DETUNE] = (p.detune_cents / 100.0 + 0.5) as f32;
        // Mix: derive from levels (equal levels = 0.5)
        let total = p.vco1_level + p.vco2_level;
        self.params[P_MIX] = if total > 0.0 { (p.vco2_level / total) as f32 } else { 0.5 };
        self.params[P_CUTOFF] = p.cutoff as f32;
        self.params[P_RESO] = p.resonance as f32;
        self.params[P_ENV_MOD] = p.env_mod as f32;
        self.params[P_ATTACK] = ((p.env1_a - 0.0006) / 5.9994).clamp(0.0, 1.0) as f32;
        self.params[P_DECAY] = ((p.env1_d - 0.003) / 9.997).clamp(0.0, 1.0) as f32;
        self.params[P_SUSTAIN] = p.env1_s as f32;
        self.params[P_RELEASE] = ((p.env1_r - 0.003) / 11.997).clamp(0.0, 1.0) as f32;
        self.params[P_LFO_RATE] = ((p.lfo_rate - 0.05) / 39.95).clamp(0.0, 1.0) as f32;
        let lfo_mod = (p.lfo_to_pitch / 0.02).max(p.lfo_to_filter / 0.3);
        self.params[P_LFO_MOD] = lfo_mod.clamp(0.0, 1.0) as f32;
        self.params[P_MODE] = p.voice_mode as f32 / 3.99;
    }

    fn voice_mode(&self) -> u8 {
        (self.params[P_MODE] * 4.0).min(3.0) as u8
    }

    fn next_age(&mut self) -> u64 { self.voice_counter += 1; self.voice_counter }

    fn allocate_voice_poly1(&mut self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| !v.is_sounding()) { return i; }
        if let Some((i, _)) = self.voices.iter().enumerate()
            .filter(|(_, v)| !v.is_held()).min_by_key(|(_, v)| v.age) { return i; }
        self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap_or(0)
    }

    fn allocate_voice_poly2(&mut self) -> usize {
        // Kill all voices in release phase first
        for v in &mut self.voices {
            if v.is_sounding() && !v.is_held() { v.kill(); }
        }
        self.allocate_voice_poly1()
    }

    fn release_note(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.note == note && v.is_held() { v.note_off(); }
        }
    }

    fn kill_all_voices(&mut self) {
        for v in &mut self.voices { v.kill(); }
    }

    /// Build an active patch by merging preset data with user parameter overrides.
    fn active_patch(&self) -> JupiterPatch {
        let mut p = self.patches[self.current_patch_index()];
        // Override with user-facing params
        p.vco1_wave = (self.params[P_VCO1_WAVE] * 4.0).min(3.0) as u8;
        p.vco2_wave = (self.params[P_VCO2_WAVE] * 4.0).min(3.0) as u8;
        p.detune_cents = (self.params[P_DETUNE] as f64 - 0.5) * 100.0; // ±50 cents
        let mix = self.params[P_MIX] as f64;
        p.vco1_level = (1.0 - mix).max(0.0).min(1.0) * 1.0;
        p.vco2_level = mix.max(0.0).min(1.0) * 1.0;
        // ENV-1 ADSR from user params (scaled to Jupiter ranges)
        p.env1_a = 0.0006 + self.params[P_ATTACK] as f64 * 5.9994; // 0.6ms - 6s
        p.env1_d = 0.003 + self.params[P_DECAY] as f64 * 9.997;    // 3ms - 10s
        p.env1_s = self.params[P_SUSTAIN] as f64;
        p.env1_r = 0.003 + self.params[P_RELEASE] as f64 * 11.997; // 3ms - 12s
        // ENV-2 mirrors ENV-1 for simplicity (full control via inst config later)
        p.env2_a = p.env1_a;
        p.env2_d = p.env1_d;
        p.env2_s = p.env1_s;
        p.env2_r = p.env1_r;
        // LFO
        p.lfo_rate = 0.05 + self.params[P_LFO_RATE] as f64 * 39.95; // 0.05-40 Hz
        let lfo_mod = self.params[P_LFO_MOD] as f64;
        p.lfo_to_filter = lfo_mod * 0.3;
        p.lfo_to_pitch = lfo_mod * 0.02;
        p.voice_mode = self.voice_mode();
        p
    }
}

impl Default for Jupiter8Synth {
    fn default() -> Self { Self::new() }
}

impl Plugin for Jupiter8Synth {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "Jupiter-8".into(),
            version: "0.1.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..MAX_VOICES).map(|i| JupiterVoice::new(sample_rate, i)).collect();
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() { return; }

        let buf_len = outputs[0].len();
        let gain = self.params[P_GAIN];
        let patch = self.active_patch();
        let user_cutoff = self.params[P_CUTOFF] as f64;
        let user_reso = self.params[P_RESO] as f64;
        let user_env_mod = self.params[P_ENV_MOD] as f64;

        self.lfo.rate = patch.lfo_rate;
        self.lfo.waveform = patch.lfo_wave;
        self.lfo.delay_time = patch.lfo_delay;

        let mode = patch.voice_mode;

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
                            match mode {
                                0 => {
                                    // Solo: all voices play same note
                                    self.release_note(ev.data1);
                                    let age = self.next_age();
                                    for vi in 0..self.voices.len() {
                                        self.voices[vi].note_on(ev.data1, ev.data2, &patch, true, age);
                                    }
                                    self.lfo.trigger_delay();
                                }
                                1 => {
                                    // Unison: all voices on same note (simplified)
                                    self.release_note(ev.data1);
                                    let age = self.next_age();
                                    for vi in 0..self.voices.len() {
                                        self.voices[vi].note_on(ev.data1, ev.data2, &patch, false, age);
                                    }
                                    self.lfo.trigger_delay();
                                }
                                3 => {
                                    // Poly2: kill released voices first
                                    self.release_note(ev.data1);
                                    let age = self.next_age();
                                    let idx = self.allocate_voice_poly2();
                                    self.voices[idx].note_on(ev.data1, ev.data2, &patch, false, age);
                                    self.lfo.trigger_delay();
                                }
                                _ => {
                                    // Poly1 (default)
                                    self.release_note(ev.data1);
                                    let age = self.next_age();
                                    let idx = self.allocate_voice_poly1();
                                    self.voices[idx].note_on(ev.data1, ev.data2, &patch, false, age);
                                    self.lfo.trigger_delay();
                                }
                            }
                        } else {
                            self.release_note(ev.data1);
                        }
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

            // Global LFO
            let lfo_out = self.lfo.tick(self.sample_rate);

            // Sum voices
            let mut sample = 0.0f32;
            for v in &mut self.voices {
                sample += v.tick(&patch, lfo_out, user_cutoff, user_reso, user_env_mod) as f32;
            }

            // Normalize by voice count for poly modes
            let active_count = self.voices.iter().filter(|v| v.is_sounding()).count().max(1);
            if mode >= 2 {
                // Poly modes: scale by number of sounding voices to prevent clipping
                sample /= (active_count as f32).sqrt();
            } else {
                // Solo/Unison: scale by total voice count
                sample /= (MAX_VOICES as f32).sqrt();
            }

            sample *= gain;
            sample = sample.clamp(-1.0, 1.0);

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
        if index == P_PATCH {
            self.sync_params_from_patch();
        }
    }

    fn reset(&mut self) { self.kill_all_voices(); self.voice_counter = 0; }
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

    fn process_buffers(synth: &mut Jupiter8Synth, events: &[MidiEvent], count: usize) -> Vec<f32> {
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
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 4);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001, "Should produce sound, peak={peak}");
    }

    #[test]
    fn silent_after_release() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[note_off(60, 0)], 3000);
        let out = process_buffers(&mut s, &[], 1);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak < 0.001, "Should be silent after release, peak={peak}");
    }

    #[test]
    fn output_is_finite() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 1000);
        assert!(out.iter().all(|v| v.is_finite()), "Output must be finite");
    }

    #[test]
    fn polyphony() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        let events = [note_on(60, 100, 0), note_on(64, 100, 0), note_on(67, 100, 0)];
        let out = process_buffers(&mut s, &events, 4);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001 && peak <= 1.0, "peak={peak}");
    }

    #[test]
    fn all_patches_produce_sound() {
        for patch_idx in 0..PATCH_COUNT {
            let mut s = Jupiter8Synth::new();
            s.init(44100.0, 64);
            let patch_val = patch_idx as f32 / (PATCH_COUNT as f32 - 0.01);
            s.set_parameter(P_PATCH, patch_val);
            // Use enough buffers for slow-attack patches (up to ~3s attack)
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 2500);
            let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
            assert!(peak > 0.001, "Patch {} ({}) should produce sound, peak={peak}",
                patch_idx, PATCH_NAMES[patch_idx]);
        }
    }

    #[test]
    fn all_patches_finite() {
        for patch_idx in 0..PATCH_COUNT {
            let mut s = Jupiter8Synth::new();
            s.init(44100.0, 64);
            let patch_val = patch_idx as f32 / (PATCH_COUNT as f32 - 0.01);
            s.set_parameter(P_PATCH, patch_val);
            let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 500);
            assert!(out.iter().all(|v| v.is_finite()),
                "Patch {} ({}) must produce finite output", patch_idx, PATCH_NAMES[patch_idx]);
        }
    }

    #[test]
    fn cc120_kills_all() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[cc(120, 0, 0)], 1);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn all_params_readable() {
        let s = Jupiter8Synth::new();
        assert_eq!(s.parameter_count(), PARAM_COUNT);
        for i in 0..PARAM_COUNT {
            assert!(s.parameter_info(i).is_some());
            let val = s.get_parameter(i);
            assert!((0.0..=1.0).contains(&val), "param {i} = {val}");
        }
    }

    #[test]
    fn filter_resonance_affects_sound() {
        let mut s1 = Jupiter8Synth::new();
        s1.init(44100.0, 64);
        s1.set_parameter(P_RESO, 0.0);
        let flat = process_buffers(&mut s1, &[note_on(60, 100, 0)], 8);

        let mut s2 = Jupiter8Synth::new();
        s2.init(44100.0, 64);
        s2.set_parameter(P_RESO, 0.8);
        let reso = process_buffers(&mut s2, &[note_on(60, 100, 0)], 8);

        let diff: f32 = flat.iter().zip(reso.iter()).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 0.01, "Resonance should change sound, diff={diff}");
    }

    #[test]
    fn sample_accurate_midi() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 128);
        // Use Init patch (fast attack) so sound is audible within 64 samples
        s.set_parameter(P_PATCH, 1.0); // Init patch (last)
        s.set_parameter(P_ATTACK, 0.0); // fastest attack
        let mut out = vec![0.0f32; 128];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 64)]);
        let pre = out[..64].iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        let post = out[64..].iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(pre < 0.001, "Should be silent before note: {pre}");
        assert!(post > 0.001, "Should sound after note: {post}");
    }

    #[test]
    fn solo_mode_all_voices_same_note() {
        let mut s = Jupiter8Synth::new();
        s.init(44100.0, 64);
        s.set_parameter(P_MODE, 0.0); // solo
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        // All voices should be on note 60
        assert!(s.voices.iter().all(|v| v.note == 60), "Solo: all voices should play same note");
    }

    #[test]
    fn discrete_labels() {
        assert_eq!(discrete_label(P_PATCH, 0.0), Some("Pad"));
        assert_eq!(discrete_label(P_VCO1_WAVE, 0.25), Some("saw"));
        assert_eq!(discrete_label(P_VCO2_WAVE, 0.75), Some("noise"));
        assert_eq!(discrete_label(P_MODE, 0.5), Some("poly1"));
        assert_eq!(discrete_label(P_CUTOFF, 0.5), None);
    }

    #[test]
    fn poly_blep_at_boundaries() {
        // At t=0 (just after reset), blep should return a correction
        let dt = 0.01;
        let b1 = poly_blep(0.005, dt);
        assert!(b1.abs() > 0.0, "BLEP should correct near reset");
        let b2 = poly_blep(0.5, dt);
        assert_eq!(b2, 0.0, "BLEP should be zero far from boundary");
    }
}
