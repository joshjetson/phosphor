//! DX7-style 6-operator FM synthesizer.
//!
//! Authentic recreation of the Yamaha DX7's FM synthesis engine:
//! 6 sine-wave operators, 32 algorithms, 4-rate/4-level envelopes,
//! operator feedback, and classic preset patches.

use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

const MAX_VOICES: usize = 16;
const NUM_OPERATORS: usize = 6;
const TWO_PI: f64 = std::f64::consts::TAU;

// ── Parameter indices ──
pub const P_PATCH: usize = 0;
pub const P_FEEDBACK: usize = 1;
pub const P_BRIGHTNESS: usize = 2;
pub const P_ATTACK: usize = 3;
pub const P_DECAY: usize = 4;
pub const P_SUSTAIN: usize = 5;
pub const P_RELEASE: usize = 6;
pub const P_GAIN: usize = 7;
pub const PARAM_COUNT: usize = 8;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = [
    "patch", "feedback", "bright", "attack", "decay", "sustain", "release", "gain",
];

pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [
    0.0,   // patch: E.Piano
    0.5,   // feedback
    0.5,   // brightness (scales modulator output levels)
    0.3,   // attack time scale
    0.5,   // decay time scale
    0.7,   // sustain level scale
    0.3,   // release time scale
    0.75,  // gain
];

// ── Patches ──

pub const PATCH_COUNT: usize = 17;
pub const PATCH_NAMES: [&str; PATCH_COUNT] = [
    "E.Piano", "Bass", "Brass", "Bells", "Organ", "Strings",
    "Flute", "Harpsi", "Marimba", "Clav", "TubBell",
    "Vibes", "Koto", "SynLead", "Choir", "Harmnca", "Kalimba",
];

/// Per-operator preset data.
#[derive(Debug, Clone, Copy)]
struct OpPreset {
    freq_ratio: f64,
    output_level: u8,  // 0-99
    rates: [u8; 4],    // R1-R4
    levels: [u8; 4],   // L1-L4
    vel_sens: u8,      // 0-7
}

/// Full patch preset.
#[derive(Debug, Clone, Copy)]
struct PatchPreset {
    algorithm: u8,
    feedback: u8,  // 0-7
    ops: [OpPreset; NUM_OPERATORS],
}

fn presets() -> [PatchPreset; PATCH_COUNT] {
    [
        // E.Piano 1 — Algorithm 5
        PatchPreset {
            algorithm: 5,
            feedback: 6,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [96,64,30,10], levels: [99,90,75,0], vel_sens: 3 },
                OpPreset { freq_ratio: 14.0, output_level: 75, rates: [95,50,20,10], levels: [99,60,0,0],  vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [96,64,30,10], levels: [99,90,75,0], vel_sens: 3 },
                OpPreset { freq_ratio: 14.0, output_level: 70, rates: [95,50,20,10], levels: [99,55,0,0],  vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 86, rates: [96,60,28,10], levels: [99,85,70,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0,  output_level: 65, rates: [95,55,25,10], levels: [99,50,0,0],  vel_sens: 4 },
            ],
        },
        // Bass 1 — Algorithm 5
        PatchPreset {
            algorithm: 5,
            feedback: 5,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [90,50,35,15], levels: [99,80,70,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 82, rates: [95,60,25,10], levels: [99,45,0,0],  vel_sens: 4 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [90,50,35,15], levels: [99,80,70,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 78, rates: [95,55,20,10], levels: [99,40,0,0],  vel_sens: 4 },
                OpPreset { freq_ratio: 1.0, output_level: 90, rates: [85,45,30,15], levels: [99,75,65,0], vel_sens: 2 },
                OpPreset { freq_ratio: 2.0, output_level: 72, rates: [95,60,25,10], levels: [99,50,0,0],  vel_sens: 5 },
            ],
        },
        // Brass 1 — Algorithm 22
        PatchPreset {
            algorithm: 22,
            feedback: 4,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [60,45,40,20], levels: [99,95,90,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 95, rates: [60,45,40,20], levels: [99,92,87,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 92, rates: [55,40,35,20], levels: [99,90,85,0], vel_sens: 2 },
                OpPreset { freq_ratio: 3.0, output_level: 60, rates: [65,50,30,15], levels: [99,50,20,0], vel_sens: 5 },
                OpPreset { freq_ratio: 1.0, output_level: 88, rates: [55,40,35,20], levels: [99,88,83,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 76, rates: [70,55,45,15], levels: [99,70,40,0], vel_sens: 4 },
            ],
        },
        // Bells — Algorithm 5
        PatchPreset {
            algorithm: 5,
            feedback: 3,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [80,40,20,8], levels: [99,75,50,0], vel_sens: 3 },
                OpPreset { freq_ratio: 4.23, output_level: 80, rates: [75,35,15,8], levels: [99,55,0,0],  vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 95, rates: [80,40,20,8], levels: [99,70,45,0], vel_sens: 3 },
                OpPreset { freq_ratio: 5.37, output_level: 78, rates: [70,30,12,8], levels: [99,50,0,0],  vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 88, rates: [85,45,22,8], levels: [99,60,30,0], vel_sens: 2 },
                OpPreset { freq_ratio: 13.0, output_level: 70, rates: [65,25,10,8], levels: [99,40,0,0],  vel_sens: 6 },
            ],
        },
        // Organ 1 — Algorithm 32
        PatchPreset {
            algorithm: 32,
            feedback: 0,
            ops: [
                OpPreset { freq_ratio: 0.5, output_level: 90, rates: [99,90,90,30], levels: [99,99,99,0], vel_sens: 0 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [99,90,90,30], levels: [99,99,99,0], vel_sens: 0 },
                OpPreset { freq_ratio: 2.0, output_level: 92, rates: [99,90,90,30], levels: [99,99,99,0], vel_sens: 0 },
                OpPreset { freq_ratio: 3.0, output_level: 85, rates: [99,90,90,30], levels: [99,99,99,0], vel_sens: 0 },
                OpPreset { freq_ratio: 4.0, output_level: 78, rates: [99,90,90,30], levels: [99,99,99,0], vel_sens: 0 },
                OpPreset { freq_ratio: 6.0, output_level: 70, rates: [99,90,90,30], levels: [99,99,99,0], vel_sens: 1 },
            ],
        },
        // Strings — Algorithm 2
        PatchPreset {
            algorithm: 2,
            feedback: 3,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [45,55,50,25], levels: [99,95,92,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0, output_level: 60, rates: [50,60,55,25], levels: [99,70,60,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 55, rates: [55,60,55,20], levels: [99,65,55,0], vel_sens: 2 },
                OpPreset { freq_ratio: 2.0, output_level: 85, rates: [45,55,50,25], levels: [99,92,88,0], vel_sens: 1 },
                OpPreset { freq_ratio: 2.0, output_level: 50, rates: [50,55,50,20], levels: [99,60,50,0], vel_sens: 2 },
                OpPreset { freq_ratio: 3.0, output_level: 45, rates: [55,55,50,20], levels: [99,55,45,0], vel_sens: 3 },
            ],
        },
        // Flute — Algorithm 5
        PatchPreset {
            algorithm: 5,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [65,35,22,50], levels: [99,99,95,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0, output_level: 56, rates: [90,68,50,50], levels: [99,62,50,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [65,35,22,50], levels: [99,99,95,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0, output_level: 56, rates: [90,68,50,50], levels: [99,62,50,0], vel_sens: 3 },
                OpPreset { freq_ratio: 2.0, output_level: 78, rates: [65,35,22,50], levels: [99,99,95,0], vel_sens: 1 },
                OpPreset { freq_ratio: 3.0, output_level: 45, rates: [90,68,50,50], levels: [99,62,50,0], vel_sens: 3 },
            ],
        },
        // Harpsichord — Algorithm 5
        PatchPreset {
            algorithm: 5,
            feedback: 3,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [99,40,30,60], levels: [99,70,0,0],  vel_sens: 1 },
                OpPreset { freq_ratio: 5.0, output_level: 80, rates: [99,75,60,60], levels: [99,56,0,0],  vel_sens: 5 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [99,40,30,60], levels: [99,70,0,0],  vel_sens: 1 },
                OpPreset { freq_ratio: 5.0, output_level: 80, rates: [99,75,60,60], levels: [99,56,0,0],  vel_sens: 5 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [99,70,35,90], levels: [99,60,0,0],  vel_sens: 2 },
                OpPreset { freq_ratio: 3.0, output_level: 70, rates: [99,85,50,85], levels: [99,50,0,0],  vel_sens: 4 },
            ],
        },
        // Marimba — Algorithm 5
        PatchPreset {
            algorithm: 5,
            feedback: 4,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,85,0,60], levels: [99,50,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 4.0,  output_level: 72, rates: [99,92,0,70], levels: [99,36,0,0], vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,85,0,60], levels: [99,50,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 4.0,  output_level: 72, rates: [99,92,0,70], levels: [99,36,0,0], vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,80,0,60], levels: [99,50,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 10.0, output_level: 60, rates: [99,90,0,70], levels: [99,30,0,0], vel_sens: 6 },
            ],
        },
        // Clavinet — Algorithm 5
        PatchPreset {
            algorithm: 5,
            feedback: 6,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,86,56,76], levels: [99,60,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 3.41, output_level: 86, rates: [99,95,70,80], levels: [99,52,0,0], vel_sens: 7 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,86,56,76], levels: [99,60,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 3.41, output_level: 86, rates: [99,95,70,80], levels: [99,52,0,0], vel_sens: 7 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,86,56,76], levels: [99,55,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 2.0,  output_level: 78, rates: [99,92,66,82], levels: [99,50,0,0], vel_sens: 7 },
            ],
        },
        // Tubular Bells — Algorithm 5
        PatchPreset {
            algorithm: 5,
            feedback: 4,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [72,76,99,71], levels: [99,88,96,0], vel_sens: 2 },
                OpPreset { freq_ratio: 3.50, output_level: 78, rates: [99,88,96,60], levels: [95,60,50,0], vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [72,76,99,71], levels: [99,88,96,0], vel_sens: 2 },
                OpPreset { freq_ratio: 3.50, output_level: 78, rates: [99,88,96,60], levels: [95,60,50,0], vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [72,76,99,71], levels: [99,88,96,0], vel_sens: 2 },
                OpPreset { freq_ratio: 7.12, output_level: 62, rates: [99,88,96,60], levels: [95,60,50,0], vel_sens: 5 },
            ],
        },
        // Vibraphone — Algorithm 5
        PatchPreset {
            algorithm: 5,
            feedback: 6,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [72,76,99,71], levels: [99,88,96,0], vel_sens: 2 },
                OpPreset { freq_ratio: 4.0,  output_level: 72, rates: [99,88,70,50], levels: [99,40,0,0],  vel_sens: 6 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [72,76,99,71], levels: [99,88,96,0], vel_sens: 2 },
                OpPreset { freq_ratio: 4.0,  output_level: 72, rates: [99,88,70,50], levels: [99,40,0,0],  vel_sens: 6 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [72,76,99,71], levels: [99,88,96,0], vel_sens: 2 },
                OpPreset { freq_ratio: 10.0, output_level: 55, rates: [99,88,70,50], levels: [99,40,0,0],  vel_sens: 6 },
            ],
        },
        // Koto — Algorithm 5
        PatchPreset {
            algorithm: 5,
            feedback: 5,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [99,80,40,72], levels: [99,65,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 2.0, output_level: 78, rates: [99,90,55,65], levels: [99,55,0,0], vel_sens: 7 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [99,80,40,72], levels: [99,65,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 2.0, output_level: 78, rates: [99,90,55,65], levels: [99,55,0,0], vel_sens: 7 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [99,90,40,85], levels: [99,55,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 5.0, output_level: 70, rates: [99,95,60,75], levels: [99,48,0,0], vel_sens: 6 },
            ],
        },
        // Synth Lead — Algorithm 22
        PatchPreset {
            algorithm: 22,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [80,50,28,55], levels: [99,99,92,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [80,50,28,55], levels: [99,99,92,0], vel_sens: 2 },
                OpPreset { freq_ratio: 2.0, output_level: 84, rates: [80,50,28,55], levels: [99,99,92,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 86, rates: [90,82,88,50], levels: [99,90,94,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 86, rates: [90,82,88,50], levels: [99,90,94,0], vel_sens: 3 },
                OpPreset { freq_ratio: 3.0, output_level: 82, rates: [90,82,88,50], levels: [99,90,94,0], vel_sens: 3 },
            ],
        },
        // Choir/Pad — Algorithm 1
        PatchPreset {
            algorithm: 1,
            feedback: 6,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [35,25,20,40], levels: [99,99,96,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0, output_level: 68, rates: [50,45,45,45], levels: [99,72,70,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 82, rates: [38,22,20,42], levels: [99,99,96,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [35,25,20,40], levels: [99,99,96,0], vel_sens: 1 },
                OpPreset { freq_ratio: 2.0, output_level: 60, rates: [50,45,45,45], levels: [99,72,70,0], vel_sens: 2 },
                OpPreset { freq_ratio: 3.0, output_level: 50, rates: [55,48,48,48], levels: [99,65,60,0], vel_sens: 3 },
            ],
        },
        // Harmonica — Algorithm 5
        PatchPreset {
            algorithm: 5,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [72,35,25,50], levels: [99,99,96,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0, output_level: 75, rates: [85,60,50,50], levels: [99,72,65,0], vel_sens: 4 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [72,35,25,50], levels: [99,99,96,0], vel_sens: 1 },
                OpPreset { freq_ratio: 2.0, output_level: 70, rates: [85,60,50,50], levels: [99,72,65,0], vel_sens: 4 },
                OpPreset { freq_ratio: 3.0, output_level: 82, rates: [72,35,25,50], levels: [99,99,96,0], vel_sens: 1 },
                OpPreset { freq_ratio: 4.0, output_level: 58, rates: [85,65,55,55], levels: [99,68,60,0], vel_sens: 4 },
            ],
        },
        // Kalimba — Algorithm 5
        PatchPreset {
            algorithm: 5,
            feedback: 3,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,82,0,55], levels: [99,55,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 5.19, output_level: 68, rates: [99,90,0,65], levels: [99,35,0,0], vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,82,0,55], levels: [99,55,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 3.0,  output_level: 62, rates: [99,88,0,60], levels: [99,30,0,0], vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,78,0,50], levels: [99,50,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 9.0,  output_level: 50, rates: [99,95,0,70], levels: [99,25,0,0], vel_sens: 6 },
            ],
        },
    ]
}

// ── Algorithm routing ──

/// Defines which operators modulate which, and which are carriers.
/// For each operator (index 0-5 = op 1-6), lists indices of operators it modulates.
/// An empty target list means it's a carrier.
struct AlgorithmDef {
    /// For each op: which ops does it modulate? Empty = carrier.
    /// modulates[5] = vec![4,3,2] means op6 modulates ops 5,4,3.
    modulates: [&'static [usize]; NUM_OPERATORS],
    /// Which operators are carriers (output to audio).
    carriers: &'static [usize],
    /// Which operator has feedback (0-indexed). None if no feedback in this alg.
    feedback_op: usize,
}

/// Get the algorithm definition for a given algorithm number (1-32).
fn algorithm(num: u8) -> AlgorithmDef {
    match num {
        // Alg 1: [6fb→5→4→3*] + [2→1*]
        1 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[3], &[4]],
            carriers: &[0, 2],
            feedback_op: 5,
        },
        // Alg 2: [6→5→4→3*] + [2fb→1*]
        2 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[3], &[4]],
            carriers: &[0, 2],
            feedback_op: 1,
        },
        // Alg 5: [6fb→5*] + [4→3*] + [2→1*]  — THE classic
        5 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[], &[4]],
            carriers: &[0, 2, 4],
            feedback_op: 5,
        },
        // Alg 22: [6fb→(5*,4*,3*)] + [2*] + [1*]
        22 => AlgorithmDef {
            modulates: [&[], &[], &[], &[], &[], &[4, 3, 2]],
            carriers: &[0, 1, 2, 3, 4],
            feedback_op: 5,
        },
        // Alg 32: All carriers, fb on 6
        32 => AlgorithmDef {
            modulates: [&[], &[], &[], &[], &[], &[]],
            carriers: &[0, 1, 2, 3, 4, 5],
            feedback_op: 5,
        },
        // Default to alg 5 for unimplemented algorithms
        _ => algorithm(5),
    }
}

// ── DX7 Envelope ──

#[derive(Debug, Clone, Copy, PartialEq)]
enum DxEnvStage {
    Idle,
    Attack,   // L4 → L1
    Decay1,   // L1 → L2
    Decay2,   // L2 → L3 (sustain)
    Release,  // L3 → L4
}

#[derive(Debug, Clone)]
struct DxEnvelope {
    stage: DxEnvStage,
    level: f64,        // current level (0..1 amplitude)
    rates: [f64; 4],   // time in seconds for each stage
    levels: [f64; 4],  // target amplitude for each stage
    sample_rate: f64,
}

impl DxEnvelope {
    fn new(sr: f64) -> Self {
        Self {
            stage: DxEnvStage::Idle,
            level: 0.0,
            rates: [0.01, 0.1, 0.5, 0.1],
            levels: [1.0, 0.7, 0.5, 0.0],
            sample_rate: sr,
        }
    }

    /// Configure from DX7 preset values (0-99 range).
    fn set_from_preset(&mut self, rates: [u8; 4], levels: [u8; 4]) {
        for i in 0..4 {
            self.rates[i] = dx_rate_to_seconds(rates[i]);
            self.levels[i] = dx_level_to_amplitude(levels[i]);
        }
    }

    /// Scale envelope times by a factor (for the user-facing attack/decay/release knobs).
    fn scale_times(&mut self, attack_scale: f64, decay_scale: f64, release_scale: f64) {
        self.rates[0] *= attack_scale;
        self.rates[1] *= decay_scale;
        self.rates[2] *= decay_scale;
        self.rates[3] *= release_scale;
    }

    fn trigger(&mut self) {
        self.stage = DxEnvStage::Attack;
        // Start from L4 (the release target / start level)
        self.level = self.levels[3];
    }

    fn release(&mut self) {
        if self.stage != DxEnvStage::Idle {
            self.stage = DxEnvStage::Release;
        }
    }

    fn kill(&mut self) {
        self.stage = DxEnvStage::Idle;
        self.level = 0.0;
    }

    fn is_active(&self) -> bool { self.stage != DxEnvStage::Idle }

    fn tick(&mut self) -> f64 {
        match self.stage {
            DxEnvStage::Idle => 0.0,
            DxEnvStage::Attack => {
                let target = self.levels[0];
                self.level = move_toward(self.level, target, self.rates[0], self.sample_rate);
                if (self.level - target).abs() < 0.001 {
                    self.level = target;
                    self.stage = DxEnvStage::Decay1;
                }
                self.level
            }
            DxEnvStage::Decay1 => {
                let target = self.levels[1];
                self.level = move_toward(self.level, target, self.rates[1], self.sample_rate);
                if (self.level - target).abs() < 0.001 {
                    self.level = target;
                    self.stage = DxEnvStage::Decay2;
                }
                self.level
            }
            DxEnvStage::Decay2 => {
                let target = self.levels[2];
                self.level = move_toward(self.level, target, self.rates[2], self.sample_rate);
                if (self.level - target).abs() < 0.001 {
                    self.level = target;
                    // Stay in Decay2 (sustain) until release
                }
                self.level
            }
            DxEnvStage::Release => {
                let target = self.levels[3];
                self.level = move_toward(self.level, target, self.rates[3], self.sample_rate);
                if self.level <= 0.001 {
                    self.level = 0.0;
                    self.stage = DxEnvStage::Idle;
                }
                self.level
            }
        }
    }
}

/// Move level toward target over a given time period.
fn move_toward(current: f64, target: f64, time_secs: f64, sample_rate: f64) -> f64 {
    let samples = (time_secs * sample_rate).max(1.0);
    let step = (target - current) / samples;
    current + step
}

/// Convert DX7 rate (0-99) to time in seconds.
fn dx_rate_to_seconds(rate: u8) -> f64 {
    // Rate 99 ≈ 1ms (instant), rate 0 ≈ 40 seconds
    let r = rate.min(99) as f64;
    2.0f64.powf((99.0 - r) * 0.07) * 0.01
}

/// Convert DX7 level (0-99) to linear amplitude.
fn dx_level_to_amplitude(level: u8) -> f64 {
    if level == 0 { return 0.0; }
    let l = level.min(99) as f64;
    // Each unit ≈ 0.75 dB, level 99 = 0dB
    10.0f64.powf((l - 99.0) * 0.075 / 2.0)
}

// ── FM Operator ──

#[derive(Debug, Clone)]
struct Operator {
    phase: f64,
    freq: f64,
    output_level: f64,  // linear amplitude from preset
    envelope: DxEnvelope,
    vel_scale: f64,      // velocity scaling factor for this note
    /// Previous two output samples for feedback averaging.
    prev: [f64; 2],
}

impl Operator {
    fn new(sr: f64) -> Self {
        Self {
            phase: 0.0,
            freq: 440.0,
            output_level: 1.0,
            envelope: DxEnvelope::new(sr),
            vel_scale: 1.0,
            prev: [0.0; 2],
        }
    }

    /// Process one sample. `modulation` is phase modulation input from other operators.
    fn tick(&mut self, modulation: f64, sample_rate: f64) -> f64 {
        let env = self.envelope.tick();
        let out = (self.phase + modulation).sin() * env * self.output_level * self.vel_scale;

        self.phase += TWO_PI * self.freq / sample_rate;
        // Keep phase in bounds to prevent float drift
        if self.phase > TWO_PI { self.phase -= TWO_PI; }

        // Shift feedback history
        self.prev[1] = self.prev[0];
        self.prev[0] = out;

        out
    }

    /// Get feedback modulation (averaged previous two samples).
    fn feedback(&self, amount: f64) -> f64 {
        (self.prev[0] + self.prev[1]) * 0.5 * amount
    }

    fn kill(&mut self) {
        self.envelope.kill();
        self.phase = 0.0;
        self.prev = [0.0; 2];
    }
}

// ── Voice ──

#[derive(Debug, Clone)]
struct DxVoice {
    ops: [Operator; NUM_OPERATORS],
    note: u8,
    velocity: f32,
    age: u64,
    sample_rate: f64,
    algorithm: u8,
    feedback_amount: f64,
}

impl DxVoice {
    fn new(sr: f64) -> Self {
        Self {
            ops: std::array::from_fn(|_| Operator::new(sr)),
            note: 255,
            velocity: 0.0,
            age: 0,
            sample_rate: sr,
            algorithm: 5,
            feedback_amount: 0.0,
        }
    }

    fn note_on(
        &mut self,
        note: u8,
        vel: u8,
        preset: &PatchPreset,
        brightness: f64,
        attack_scale: f64,
        decay_scale: f64,
        sustain_scale: f64,
        release_scale: f64,
        age: u64,
    ) {
        self.note = note;
        self.velocity = vel as f32 / 127.0;
        self.age = age;
        self.algorithm = preset.algorithm;
        self.feedback_amount = if preset.feedback == 0 {
            0.0
        } else {
            std::f64::consts::PI * 2.0f64.powi(preset.feedback as i32 - 7)
        };

        let base_freq = note_to_freq(note);
        let alg = algorithm(preset.algorithm);

        for (i, op_preset) in preset.ops.iter().enumerate() {
            let op = &mut self.ops[i];
            op.phase = 0.0;
            op.prev = [0.0; 2];
            op.freq = base_freq * op_preset.freq_ratio;

            // Scale output level: modulators get brightness scaling, carriers don't
            let is_carrier = alg.carriers.contains(&i);
            let raw_level = dx_level_to_amplitude(op_preset.output_level);
            op.output_level = if is_carrier {
                raw_level
            } else {
                raw_level * brightness
            };

            // Velocity sensitivity
            let vel_f = vel as f64 / 127.0;
            let sens = op_preset.vel_sens as f64 / 7.0;
            op.vel_scale = 1.0 - (1.0 - vel_f) * sens;

            // Configure envelope from preset
            op.envelope.set_from_preset(op_preset.rates, op_preset.levels);

            // Apply user-facing envelope scaling
            // For sustain: scale the decay2 target level
            op.envelope.levels[2] *= sustain_scale;
            op.envelope.scale_times(attack_scale, decay_scale, release_scale);

            op.envelope.trigger();
        }
    }

    fn note_off(&mut self) {
        for op in &mut self.ops {
            op.envelope.release();
        }
    }

    fn kill(&mut self) {
        self.note = 255;
        for op in &mut self.ops {
            op.kill();
        }
    }

    fn is_sounding(&self) -> bool {
        let alg = algorithm(self.algorithm);
        // Voice is sounding if ANY carrier envelope is active
        alg.carriers.iter().any(|&i| self.ops[i].envelope.is_active())
    }

    fn is_held(&self) -> bool {
        let alg = algorithm(self.algorithm);
        alg.carriers.iter().any(|&i| {
            matches!(
                self.ops[i].envelope.stage,
                DxEnvStage::Attack | DxEnvStage::Decay1 | DxEnvStage::Decay2
            )
        })
    }

    fn tick(&mut self) -> f32 {
        if !self.is_sounding() { return 0.0; }

        let alg = algorithm(self.algorithm);
        let sr = self.sample_rate;

        // Process operators in reverse order (6→1) so modulator outputs are ready
        let mut op_outputs = [0.0f64; NUM_OPERATORS];

        for i in (0..NUM_OPERATORS).rev() {
            // Calculate modulation input from any operators that modulate this one
            let mut modulation = 0.0;
            for (j, targets) in alg.modulates.iter().enumerate() {
                if targets.contains(&i) {
                    modulation += op_outputs[j];
                }
            }

            // Add feedback if this is the feedback operator
            if i == alg.feedback_op {
                modulation += self.ops[i].feedback(self.feedback_amount);
            }

            op_outputs[i] = self.ops[i].tick(modulation, sr);
        }

        // Sum carrier outputs
        let mut out = 0.0f64;
        for &c in alg.carriers {
            out += op_outputs[c];
        }

        // Normalize by number of carriers
        let num_carriers = alg.carriers.len() as f64;
        (out / num_carriers) as f32
    }
}

// ── DX7 Synth ──

pub struct Dx7Synth {
    voices: Vec<DxVoice>,
    sample_rate: f64,
    pub params: [f32; PARAM_COUNT],
    voice_counter: u64,
    presets: [PatchPreset; PATCH_COUNT],
}

impl Dx7Synth {
    pub fn new() -> Self {
        Self {
            voices: Vec::new(),
            sample_rate: 44100.0,
            params: PARAM_DEFAULTS,
            voice_counter: 0,
            presets: presets(),
        }
    }

    fn current_patch_index(&self) -> usize {
        let idx = (self.params[P_PATCH] * (PATCH_COUNT as f32 - 0.01)) as usize;
        idx.min(PATCH_COUNT - 1)
    }

    fn next_age(&mut self) -> u64 { self.voice_counter += 1; self.voice_counter }

    fn allocate_voice(&mut self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| !v.is_sounding()) { return i; }
        if let Some((i, _)) = self.voices.iter().enumerate()
            .filter(|(_, v)| !v.is_held()).min_by_key(|(_, v)| v.age) { return i; }
        self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap_or(0)
    }

    fn release_note(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.note == note && v.is_held() { v.note_off(); }
        }
    }

    fn kill_all_voices(&mut self) {
        for v in &mut self.voices { v.kill(); }
    }

    fn brightness(&self) -> f64 {
        // 0..1 → 0.2..2.0 (brightness scales modulator output levels)
        0.2 + self.params[P_BRIGHTNESS] as f64 * 1.8
    }

    fn attack_scale(&self) -> f64 {
        // 0..1 → 0.1..3.0
        0.1 + self.params[P_ATTACK] as f64 * 2.9
    }

    fn decay_scale(&self) -> f64 {
        0.1 + self.params[P_DECAY] as f64 * 2.9
    }

    fn sustain_scale(&self) -> f64 {
        self.params[P_SUSTAIN] as f64
    }

    fn release_scale(&self) -> f64 {
        0.1 + self.params[P_RELEASE] as f64 * 2.9
    }
}

impl Default for Dx7Synth {
    fn default() -> Self { Self::new() }
}

impl Plugin for Dx7Synth {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "DX7".into(),
            version: "0.1.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..MAX_VOICES).map(|_| DxVoice::new(sample_rate)).collect();
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() { return; }

        let buf_len = outputs[0].len();
        let gain = self.params[P_GAIN];
        let patch_idx = self.current_patch_index();
        let preset = self.presets[patch_idx];
        let brightness = self.brightness();
        let attack_scale = self.attack_scale();
        let decay_scale = self.decay_scale();
        let sustain_scale = self.sustain_scale();
        let release_scale = self.release_scale();

        // Override feedback from user param if they've adjusted it
        let user_feedback = self.params[P_FEEDBACK];

        // Avoid allocation in audio thread — use fixed-size scratch buffer
        let mut event_indices: [usize; 256] = [0; 256];
        let event_count = midi_events.len().min(256);
        for i in 0..event_count { event_indices[i] = i; }
        // Simple insertion sort on sample_offset (usually already sorted, tiny N)
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
                            self.release_note(ev.data1);
                            let age = self.next_age();
                            let idx = self.allocate_voice();
                            self.voices[idx].note_on(
                                ev.data1, ev.data2, &preset,
                                brightness, attack_scale, decay_scale,
                                sustain_scale, release_scale, age,
                            );
                            // Apply user feedback override
                            let fb_amount = if preset.feedback == 0 && user_feedback < 0.1 {
                                0.0
                            } else {
                                let fb_level = (user_feedback as f64 * 7.0).round() as i32;
                                if fb_level == 0 { 0.0 }
                                else { std::f64::consts::PI * 2.0f64.powi(fb_level - 7) }
                            };
                            self.voices[idx].feedback_amount = fb_amount;
                        } else {
                            self.release_note(ev.data1);
                        }
                    }
                    0x80 => self.release_note(ev.data1),
                    0xB0 => match ev.data1 {
                        120 => self.kill_all_voices(),
                        123 => {
                            for v in &mut self.voices { if v.is_held() { v.note_off(); } }
                        }
                        _ => {}
                    }
                    _ => {}
                }
                ei += 1;
            }

            let mut sample = 0.0f32;
            for v in &mut self.voices {
                sample += v.tick();
            }
            sample *= gain;
            // Soft-clip to prevent hot output that crashes VU rendering
            sample = sample.clamp(-1.0, 1.0);

            for ch in outputs.iter_mut() { ch[i] = sample; }
        }
    }

    fn parameter_count(&self) -> usize { PARAM_COUNT }

    fn parameter_info(&self, index: usize) -> Option<ParameterInfo> {
        if index >= PARAM_COUNT { return None; }
        Some(ParameterInfo {
            name: PARAM_NAMES[index].into(),
            min: 0.0,
            max: 1.0,
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
    fn cc(cc_num: u8, val: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0xB0, data1: cc_num, data2: val }
    }

    fn process_buffers(synth: &mut Dx7Synth, events: &[MidiEvent], count: usize) -> Vec<f32> {
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
        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sound_on_note_on() {
        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 4);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001, "Should produce sound, peak={peak}");
    }

    #[test]
    fn silent_after_release() {
        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[note_off(60, 0)], 3000);
        let out = process_buffers(&mut s, &[], 1);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak < 0.001, "Should be silent after release, peak={peak}");
    }

    #[test]
    fn output_is_finite() {
        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        let out = process_buffers(&mut s, &[note_on(60, 127, 0)], 1000);
        assert!(out.iter().all(|v| v.is_finite()), "Output must be finite");
    }

    #[test]
    fn polyphony() {
        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        let events = [note_on(60, 100, 0), note_on(64, 100, 0), note_on(67, 100, 0)];
        let out = process_buffers(&mut s, &events, 4);
        let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001 && peak <= 2.0, "peak={peak}");
    }

    #[test]
    fn all_patches_produce_sound() {
        for patch_idx in 0..PATCH_COUNT {
            let mut s = Dx7Synth::new();
            s.init(44100.0, 64);
            let patch_val = patch_idx as f32 / (PATCH_COUNT as f32 - 0.01);
            s.set_parameter(P_PATCH, patch_val);
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 8);
            let peak = out.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
            assert!(peak > 0.001, "Patch {} ({}) should produce sound, peak={peak}",
                patch_idx, PATCH_NAMES[patch_idx]);
        }
    }

    #[test]
    fn all_patches_finite() {
        for patch_idx in 0..PATCH_COUNT {
            let mut s = Dx7Synth::new();
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
        let mut s = Dx7Synth::new();
        s.init(44100.0, 64);
        process_buffers(&mut s, &[note_on(60, 100, 0)], 2);
        process_buffers(&mut s, &[cc(120, 0, 0)], 1);
        let out = process_buffers(&mut s, &[], 1);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn brightness_affects_sound() {
        let mut s1 = Dx7Synth::new();
        s1.init(44100.0, 64);
        s1.set_parameter(P_BRIGHTNESS, 0.1);
        let dark = process_buffers(&mut s1, &[note_on(60, 100, 0)], 8);
        let dark_energy: f32 = dark.iter().map(|v| v * v).sum();

        let mut s2 = Dx7Synth::new();
        s2.init(44100.0, 64);
        s2.set_parameter(P_BRIGHTNESS, 0.9);
        let bright = process_buffers(&mut s2, &[note_on(60, 100, 0)], 8);
        let bright_energy: f32 = bright.iter().map(|v| v * v).sum();

        // Different brightness should change the sound
        let diff: f32 = dark.iter().zip(bright.iter()).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 0.01, "Brightness should change sound, diff={diff}, dark_e={dark_energy}, bright_e={bright_energy}");
    }

    #[test]
    fn all_params_readable() {
        let s = Dx7Synth::new();
        assert_eq!(s.parameter_count(), PARAM_COUNT);
        for i in 0..PARAM_COUNT {
            assert!(s.parameter_info(i).is_some());
            let val = s.get_parameter(i);
            assert!((0.0..=1.0).contains(&val), "param {i} = {val}");
        }
    }

    #[test]
    fn patch_selector_range() {
        let s = Dx7Synth::new();
        // Min
        assert_eq!(s.current_patch_index(), 0);

        let mut s2 = Dx7Synth::new();
        s2.set_parameter(P_PATCH, 1.0);
        assert_eq!(s2.current_patch_index(), PATCH_COUNT - 1);
    }

    #[test]
    fn dx_rate_conversion() {
        let fast = dx_rate_to_seconds(99);
        let slow = dx_rate_to_seconds(0);
        assert!(fast < 0.02, "Rate 99 should be very fast: {fast}");
        assert!(slow > 1.0, "Rate 0 should be slow: {slow}");
        assert!(slow > fast * 100.0, "Rate 0 should be much slower than 99");
    }

    #[test]
    fn dx_level_conversion() {
        let full = dx_level_to_amplitude(99);
        let zero = dx_level_to_amplitude(0);
        let mid = dx_level_to_amplitude(50);
        assert!((full - 1.0).abs() < 0.01, "Level 99 should be ~1.0: {full}");
        assert_eq!(zero, 0.0, "Level 0 should be 0");
        assert!(mid > 0.0 && mid < 1.0, "Level 50 should be between 0 and 1: {mid}");
    }

    #[test]
    fn sample_accurate_midi() {
        let mut s = Dx7Synth::new();
        s.init(44100.0, 128);
        let mut out = vec![0.0f32; 128];
        s.process(&[], &mut [&mut out], &[note_on(60, 100, 64)]);
        let pre_peak = out[..64].iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        let post_peak = out[64..].iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        assert!(pre_peak < 0.001, "Should be silent before note: {pre_peak}");
        assert!(post_peak > 0.001, "Should sound after note: {post_peak}");
    }
}
