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

pub const PATCH_COUNT: usize = 51;
pub const PATCH_NAMES: [&str; PATCH_COUNT] = [
    "E.Piano", "Bass", "Brass", "Bells", "Organ", "Strings",
    "Flute", "Harpsi", "Marimba", "Clav", "TubBell",
    "Vibes", "Koto", "SynLead", "Choir", "Harmnca", "Kalimba",
    "Sitar", "Oboe", "Clarnet", "Trumpet", "Glock",
    "Xylophn", "SteelPn", "SlapBas", "FrtlsBs", "Crystal",
    "IceRain", "SynPad", "DigiPad", "Cello", "Pizz",
    "LogDrum", "TnklBel", "Shakuha",
    // Verified factory ROM patches (source: DX7 ROM sysex via itsjoesullivan/dx7-patches):
    "SynBras", "Voices", "E.Pian2", "Accordn", "Harp",
    "Clav2", "Banjo", "Guitar1", "Piano1", "Celeste",
    "CowBell", "SynBas1", "Timpani", "PanFlut", "Horns",
    "ToyPian",
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
        // Flute — Algorithm 5, gentle breathy tone
        PatchPreset {
            algorithm: 5,
            feedback: 4,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [72,40,25,55], levels: [99,99,96,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0, output_level: 42, rates: [85,55,40,50], levels: [99,50,40,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 95, rates: [72,40,25,55], levels: [99,99,96,0], vel_sens: 1 },
                OpPreset { freq_ratio: 2.0, output_level: 35, rates: [88,60,45,55], levels: [99,45,30,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 70, rates: [72,40,25,55], levels: [99,95,90,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0, output_level: 30, rates: [90,65,50,55], levels: [99,40,25,0], vel_sens: 2 },
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
        // Vibraphone — Algorithm 5 (sustaining metallic tone, distinct from Marimba)
        PatchPreset {
            algorithm: 5,
            feedback: 4,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [72,76,99,71], levels: [99,88,96,0], vel_sens: 2 },
                OpPreset { freq_ratio: 3.5,  output_level: 65, rates: [95,82,65,50], levels: [99,45,10,0],  vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [72,76,99,71], levels: [99,88,96,0], vel_sens: 2 },
                OpPreset { freq_ratio: 5.19, output_level: 58, rates: [98,85,68,52], levels: [99,38,0,0],  vel_sens: 6 },
                OpPreset { freq_ratio: 1.0,  output_level: 92, rates: [72,76,99,71], levels: [99,85,93,0], vel_sens: 2 },
                OpPreset { freq_ratio: 8.5,  output_level: 45, rates: [99,90,72,55], levels: [99,30,0,0],  vel_sens: 6 },
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
        // ── New patches ──────────────────────────────────────────────
        // Sitar — Algorithm 5 (buzzy pluck with inharmonic overtones)
        PatchPreset {
            algorithm: 5,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,75,35,68], levels: [99,70,55,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.01, output_level: 88, rates: [99,82,40,60], levels: [99,75,60,0], vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,75,35,68], levels: [99,70,55,0], vel_sens: 2 },
                OpPreset { freq_ratio: 5.0,  output_level: 82, rates: [99,90,50,70], levels: [99,55,0,0],  vel_sens: 6 },
                OpPreset { freq_ratio: 1.0,  output_level: 90, rates: [99,70,30,65], levels: [99,65,50,0], vel_sens: 2 },
                OpPreset { freq_ratio: 7.0,  output_level: 76, rates: [99,92,55,72], levels: [99,50,0,0],  vel_sens: 7 },
            ],
        },
        // Oboe — Algorithm 5 (nasal, reedy tone with odd harmonics)
        PatchPreset {
            algorithm: 5,
            feedback: 5,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [70,40,25,50], levels: [99,99,96,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0, output_level: 72, rates: [88,62,52,50], levels: [99,75,70,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [70,40,25,50], levels: [99,99,96,0], vel_sens: 1 },
                OpPreset { freq_ratio: 3.0, output_level: 68, rates: [88,62,52,50], levels: [99,70,65,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 85, rates: [70,40,25,50], levels: [99,99,96,0], vel_sens: 1 },
                OpPreset { freq_ratio: 5.0, output_level: 55, rates: [88,68,58,55], levels: [99,60,50,0], vel_sens: 4 },
            ],
        },
        // Clarinet — Algorithm 5 (hollow tone, odd harmonics dominant)
        PatchPreset {
            algorithm: 5,
            feedback: 6,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [62,32,20,48], levels: [99,99,97,0], vel_sens: 1 },
                OpPreset { freq_ratio: 2.0, output_level: 64, rates: [82,58,48,48], levels: [99,68,62,0], vel_sens: 3 },
                OpPreset { freq_ratio: 3.0, output_level: 80, rates: [62,32,20,48], levels: [99,99,97,0], vel_sens: 1 },
                OpPreset { freq_ratio: 4.0, output_level: 52, rates: [82,58,48,48], levels: [99,55,45,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 88, rates: [65,35,22,50], levels: [99,99,96,0], vel_sens: 1 },
                OpPreset { freq_ratio: 6.0, output_level: 42, rates: [85,65,55,55], levels: [99,50,40,0], vel_sens: 4 },
            ],
        },
        // Trumpet — Algorithm 22 (bright brass, single carrier stack)
        PatchPreset {
            algorithm: 22,
            feedback: 5,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [72,48,40,30], levels: [99,96,92,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [72,48,40,30], levels: [99,96,92,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 95, rates: [68,44,38,28], levels: [99,94,90,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 78, rates: [80,55,35,20], levels: [99,60,30,0], vel_sens: 5 },
                OpPreset { freq_ratio: 1.0, output_level: 75, rates: [78,52,32,18], levels: [99,55,25,0], vel_sens: 5 },
                OpPreset { freq_ratio: 3.0, output_level: 68, rates: [85,60,40,15], levels: [99,50,20,0], vel_sens: 6 },
            ],
        },
        // Glockenspiel — Algorithm 5 (pure, bright, metallic bells)
        PatchPreset {
            algorithm: 5,
            feedback: 2,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,70,0,40], levels: [99,55,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 3.52, output_level: 72, rates: [99,80,0,50], levels: [99,40,0,0], vel_sens: 6 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,70,0,40], levels: [99,55,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 5.06, output_level: 68, rates: [99,82,0,52], levels: [99,35,0,0], vel_sens: 6 },
                OpPreset { freq_ratio: 1.0,  output_level: 92, rates: [99,68,0,38], levels: [99,50,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 8.21, output_level: 58, rates: [99,88,0,58], levels: [99,30,0,0], vel_sens: 7 },
            ],
        },
        // Xylophone — Algorithm 5 (woody attack, bright decay)
        PatchPreset {
            algorithm: 5,
            feedback: 4,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,88,0,62], levels: [99,45,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 4.0,  output_level: 78, rates: [99,94,0,72], levels: [99,32,0,0], vel_sens: 6 },
                OpPreset { freq_ratio: 3.0,  output_level: 92, rates: [99,86,0,60], levels: [99,42,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 6.0,  output_level: 70, rates: [99,92,0,70], levels: [99,28,0,0], vel_sens: 6 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,85,0,58], levels: [99,40,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 10.0, output_level: 62, rates: [99,96,0,75], levels: [99,25,0,0], vel_sens: 7 },
            ],
        },
        // Steel Pan — Algorithm 5 (warm metallic, detuned partials)
        PatchPreset {
            algorithm: 5,
            feedback: 3,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [95,65,25,45], levels: [99,72,50,0], vel_sens: 2 },
                OpPreset { freq_ratio: 2.76, output_level: 76, rates: [95,75,30,50], levels: [99,50,0,0],  vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [95,65,25,45], levels: [99,72,50,0], vel_sens: 2 },
                OpPreset { freq_ratio: 4.07, output_level: 70, rates: [95,78,32,52], levels: [99,45,0,0],  vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 92, rates: [95,62,22,42], levels: [99,68,45,0], vel_sens: 2 },
                OpPreset { freq_ratio: 6.14, output_level: 60, rates: [95,82,35,55], levels: [99,38,0,0],  vel_sens: 6 },
            ],
        },
        // Slap Bass — Algorithm 5 (punchy attack, quick decay)
        PatchPreset {
            algorithm: 5,
            feedback: 6,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [99,78,40,70], levels: [99,58,0,0],  vel_sens: 3 },
                OpPreset { freq_ratio: 3.0, output_level: 88, rates: [99,92,55,75], levels: [99,42,0,0],  vel_sens: 7 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [99,78,40,70], levels: [99,58,0,0],  vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 82, rates: [99,88,48,72], levels: [99,50,0,0],  vel_sens: 5 },
                OpPreset { freq_ratio: 1.0, output_level: 92, rates: [99,75,38,68], levels: [99,55,0,0],  vel_sens: 2 },
                OpPreset { freq_ratio: 5.0, output_level: 80, rates: [99,95,60,78], levels: [99,35,0,0],  vel_sens: 7 },
            ],
        },
        // Fretless Bass — Algorithm 5 (warm, round, singing mwah)
        PatchPreset {
            algorithm: 5,
            feedback: 2,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [80,50,35,40], levels: [99,95,90,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 45, rates: [82,55,40,42], levels: [99,55,40,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [80,50,35,40], levels: [99,95,90,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 40, rates: [82,55,40,42], levels: [99,50,35,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 88, rates: [78,48,32,38], levels: [99,92,85,0], vel_sens: 1 },
                OpPreset { freq_ratio: 2.0, output_level: 30, rates: [85,60,48,45], levels: [99,40,20,0], vel_sens: 4 },
            ],
        },
        // Crystal — Algorithm 5 (glassy, shimmering, inharmonic partials)
        PatchPreset {
            algorithm: 5,
            feedback: 2,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [85,50,20,35], levels: [99,80,60,0], vel_sens: 2 },
                OpPreset { freq_ratio: 7.07, output_level: 72, rates: [90,60,15,30], levels: [99,45,0,0],  vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 95, rates: [85,50,20,35], levels: [99,78,58,0], vel_sens: 2 },
                OpPreset { freq_ratio: 11.0, output_level: 65, rates: [92,65,12,28], levels: [99,38,0,0],  vel_sens: 6 },
                OpPreset { freq_ratio: 2.0,  output_level: 88, rates: [85,48,18,32], levels: [99,75,55,0], vel_sens: 2 },
                OpPreset { freq_ratio: 3.14, output_level: 68, rates: [88,55,10,25], levels: [99,42,0,0],  vel_sens: 5 },
            ],
        },
        // Ice Rain — Algorithm 5 (bright cascading tones, fast arpeggiated feel)
        PatchPreset {
            algorithm: 5,
            feedback: 3,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [92,55,15,30], levels: [99,72,45,0], vel_sens: 3 },
                OpPreset { freq_ratio: 5.19, output_level: 78, rates: [95,65,10,25], levels: [99,40,0,0],  vel_sens: 6 },
                OpPreset { freq_ratio: 2.0,  output_level: 95, rates: [90,52,12,28], levels: [99,68,40,0], vel_sens: 3 },
                OpPreset { freq_ratio: 9.0,  output_level: 70, rates: [96,70,8,22],  levels: [99,35,0,0],  vel_sens: 7 },
                OpPreset { freq_ratio: 1.0,  output_level: 88, rates: [88,48,18,32], levels: [99,65,38,0], vel_sens: 2 },
                OpPreset { freq_ratio: 13.0, output_level: 62, rates: [97,75,5,20],  levels: [99,30,0,0],  vel_sens: 7 },
            ],
        },
        // Synth Pad — Algorithm 1 (warm, evolving, lush pad)
        PatchPreset {
            algorithm: 1,
            feedback: 5,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [28,20,18,35], levels: [99,99,97,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0, output_level: 62, rates: [40,38,35,40], levels: [99,68,65,0], vel_sens: 2 },
                OpPreset { freq_ratio: 2.0, output_level: 55, rates: [45,42,40,42], levels: [99,60,55,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [30,22,18,38], levels: [99,99,97,0], vel_sens: 1 },
                OpPreset { freq_ratio: 2.0, output_level: 58, rates: [42,40,38,42], levels: [99,65,60,0], vel_sens: 2 },
                OpPreset { freq_ratio: 3.0, output_level: 48, rates: [48,45,42,45], levels: [99,55,48,0], vel_sens: 3 },
            ],
        },
        // Digital Pad — Algorithm 5 (shimmery, evolving digital texture)
        PatchPreset {
            algorithm: 5,
            feedback: 3,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [30,20,15,35], levels: [99,99,95,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.41, output_level: 55, rates: [35,25,18,40], levels: [99,60,45,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0,  output_level: 95, rates: [32,22,16,38], levels: [99,99,93,0], vel_sens: 1 },
                OpPreset { freq_ratio: 2.23, output_level: 50, rates: [38,28,20,42], levels: [99,55,35,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0,  output_level: 85, rates: [35,25,18,40], levels: [99,95,88,0], vel_sens: 1 },
                OpPreset { freq_ratio: 3.14, output_level: 42, rates: [42,32,22,45], levels: [99,48,28,0], vel_sens: 3 },
            ],
        },
        // Cello — Algorithm 2 (rich bowed string with rosin bite)
        PatchPreset {
            algorithm: 2,
            feedback: 4,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [42,50,48,22], levels: [99,96,94,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0, output_level: 65, rates: [48,55,52,22], levels: [99,72,68,0], vel_sens: 2 },
                OpPreset { freq_ratio: 2.0, output_level: 58, rates: [52,58,55,20], levels: [99,68,62,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 88, rates: [42,50,48,22], levels: [99,94,92,0], vel_sens: 1 },
                OpPreset { freq_ratio: 3.0, output_level: 52, rates: [50,55,52,20], levels: [99,62,55,0], vel_sens: 2 },
                OpPreset { freq_ratio: 4.0, output_level: 42, rates: [55,58,55,18], levels: [99,55,45,0], vel_sens: 3 },
            ],
        },
        // Pizzicato — Algorithm 5 (short plucked string)
        PatchPreset {
            algorithm: 5,
            feedback: 3,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [99,85,0,65], levels: [99,48,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 2.0, output_level: 72, rates: [99,90,0,70], levels: [99,35,0,0], vel_sens: 5 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [99,85,0,65], levels: [99,48,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 3.0, output_level: 65, rates: [99,92,0,72], levels: [99,30,0,0], vel_sens: 5 },
                OpPreset { freq_ratio: 1.0, output_level: 92, rates: [99,82,0,62], levels: [99,45,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 5.0, output_level: 55, rates: [99,95,0,75], levels: [99,25,0,0], vel_sens: 6 },
            ],
        },
        // Log Drum — Algorithm 5 (deep woody thud, pitch drop)
        PatchPreset {
            algorithm: 5,
            feedback: 4,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,90,0,50], levels: [99,40,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 2.0,  output_level: 85, rates: [99,95,0,60], levels: [99,30,0,0], vel_sens: 5 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,90,0,50], levels: [99,40,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.41, output_level: 80, rates: [99,94,0,58], levels: [99,28,0,0], vel_sens: 5 },
                OpPreset { freq_ratio: 0.5,  output_level: 99, rates: [99,88,0,48], levels: [99,45,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 3.0,  output_level: 72, rates: [99,96,0,65], levels: [99,22,0,0], vel_sens: 6 },
            ],
        },
        // Tinkle Bell — Algorithm 5 (small, bright, high bell)
        PatchPreset {
            algorithm: 5,
            feedback: 2,
            ops: [
                OpPreset { freq_ratio: 1.0,   output_level: 99, rates: [99,72,0,35], levels: [99,58,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 6.73,  output_level: 70, rates: [99,82,0,45], levels: [99,38,0,0], vel_sens: 6 },
                OpPreset { freq_ratio: 1.0,   output_level: 95, rates: [99,70,0,33], levels: [99,55,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 10.65, output_level: 62, rates: [99,85,0,48], levels: [99,32,0,0], vel_sens: 7 },
                OpPreset { freq_ratio: 2.0,   output_level: 88, rates: [99,68,0,30], levels: [99,52,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 14.0,  output_level: 55, rates: [99,90,0,52], levels: [99,28,0,0], vel_sens: 7 },
            ],
        },
        // Shakuhachi — Algorithm 5 (breathy Japanese flute, lots of air noise)
        PatchPreset {
            algorithm: 5,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [55,30,20,45], levels: [99,99,96,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0,  output_level: 72, rates: [80,55,45,45], levels: [99,75,70,0], vel_sens: 3 },
                OpPreset { freq_ratio: 2.0,  output_level: 82, rates: [58,32,22,48], levels: [99,99,95,0], vel_sens: 1 },
                OpPreset { freq_ratio: 3.0,  output_level: 60, rates: [82,58,48,48], levels: [99,65,55,0], vel_sens: 4 },
                OpPreset { freq_ratio: 1.0,  output_level: 78, rates: [52,28,18,42], levels: [99,99,94,0], vel_sens: 1 },
                OpPreset { freq_ratio: 11.0, output_level: 52, rates: [90,70,60,55], levels: [99,58,45,0], vel_sens: 5 },
            ],
        },
        // ── Verified factory ROM patches ──
        // Source: Yamaha DX7 ROM sysex decoded via github.com/itsjoesullivan/dx7-patches
        // Frequency mapping: JSON "frequency":0 = ratio 0.5, "frequency":N = ratio N.0

        // SYNBRASS 1 — ROM2B — Algorithm 22, fb 7
        PatchPreset {
            algorithm: 22,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,76,99,71], levels: [99,88,96,0], vel_sens: 0 },
                OpPreset { freq_ratio: 0.5,  output_level: 91, rates: [25,16,18,71], levels: [92,95,93,0], vel_sens: 0 },
                OpPreset { freq_ratio: 0.5,  output_level: 99, rates: [99,76,82,71], levels: [99,98,98,0], vel_sens: 0 },
                OpPreset { freq_ratio: 0.5,  output_level: 99, rates: [99,36,41,71], levels: [99,98,98,0], vel_sens: 0 },
                OpPreset { freq_ratio: 0.5,  output_level: 99, rates: [99,36,41,71], levels: [99,98,98,0], vel_sens: 0 },
                OpPreset { freq_ratio: 0.5,  output_level: 83, rates: [99,32,28,68], levels: [98,98,92,0], vel_sens: 2 },
            ],
        },
        // VOICES — ROM4A — Algorithm 5, fb 7
        PatchPreset {
            algorithm: 5,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [31,20,53,57], levels: [99,94,97,0], vel_sens: 0 },
                OpPreset { freq_ratio: 2.0, output_level: 54, rates: [19,26,53,25], levels: [46,56,71,46], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [31,20,53,39], levels: [99,94,97,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 92, rates: [10,19,41,12], levels: [48,58,20,9],  vel_sens: 3 },
                OpPreset { freq_ratio: 2.0, output_level: 99, rates: [31,21,36,63], levels: [99,90,85,0], vel_sens: 1 },
                OpPreset { freq_ratio: 2.0, output_level: 84, rates: [14,72,48,17], levels: [53,47,41,0], vel_sens: 2 },
            ],
        },
        // E.PIANO 2 — ROM1B — Algorithm 12, fb 7
        PatchPreset {
            algorithm: 12,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [90,85,20,54], levels: [99,93,0,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0,  output_level: 75, rates: [85,85,20,54], levels: [99,93,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [80,70,35,55], levels: [99,90,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 0.5,  output_level: 93, rates: [95,61,50,78], levels: [99,0,0,0],  vel_sens: 7 },
                OpPreset { freq_ratio: 11.0, output_level: 53, rates: [94,38,34,68], levels: [87,55,0,0], vel_sens: 6 },
                OpPreset { freq_ratio: 1.0,  output_level: 86, rates: [94,99,0,85],  levels: [99,99,0,0], vel_sens: 0 },
            ],
        },
        // ACCORDION — ROM1B — Algorithm 3, fb 7
        PatchPreset {
            algorithm: 3,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 87, rates: [55,15,10,76], levels: [99,92,82,0], vel_sens: 0 },
                OpPreset { freq_ratio: 0.5, output_level: 91, rates: [91,15,10,70], levels: [99,92,71,0], vel_sens: 0 },
                OpPreset { freq_ratio: 3.0, output_level: 56, rates: [87,15,10,46], levels: [99,92,71,0], vel_sens: 0 },
                OpPreset { freq_ratio: 2.0, output_level: 90, rates: [55,15,10,69], levels: [99,92,99,0], vel_sens: 0 },
                OpPreset { freq_ratio: 1.0, output_level: 75, rates: [63,15,10,46], levels: [99,92,68,0], vel_sens: 0 },
                OpPreset { freq_ratio: 6.0, output_level: 69, rates: [98,15,10,50], levels: [90,92,68,0], vel_sens: 0 },
            ],
        },
        // HARP 1 — ROM1B — Algorithm 3, fb 7
        PatchPreset {
            algorithm: 3,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [95,27,45,31], levels: [99,70,0,0], vel_sens: 7 },
                OpPreset { freq_ratio: 1.0, output_level: 75, rates: [95,30,99,30], levels: [99,70,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 87, rates: [95,42,44,35], levels: [99,70,0,0], vel_sens: 0 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [95,29,49,30], levels: [99,70,0,0], vel_sens: 7 },
                OpPreset { freq_ratio: 1.0, output_level: 55, rates: [95,38,99,17], levels: [99,70,0,0], vel_sens: 4 },
                OpPreset { freq_ratio: 2.0, output_level: 86, rates: [95,46,28,23], levels: [94,79,0,0], vel_sens: 1 },
            ],
        },
        // CLAV 2 — ROM1B — Algorithm 4, fb 5
        PatchPreset {
            algorithm: 4,
            feedback: 5,
            ops: [
                OpPreset { freq_ratio: 2.0,  output_level: 99, rates: [95,75,28,60], levels: [99,85,0,0], vel_sens: 0 },
                OpPreset { freq_ratio: 0.5,  output_level: 99, rates: [95,95,0,0],   levels: [99,96,89,0], vel_sens: 3 },
                OpPreset { freq_ratio: 6.0,  output_level: 80, rates: [98,50,0,0],   levels: [87,86,0,0], vel_sens: 4 },
                OpPreset { freq_ratio: 2.0,  output_level: 99, rates: [95,64,28,60], levels: [99,85,0,0], vel_sens: 0 },
                OpPreset { freq_ratio: 0.5,  output_level: 99, rates: [95,95,0,0],   levels: [99,78,89,0], vel_sens: 2 },
                OpPreset { freq_ratio: 8.0,  output_level: 78, rates: [98,87,0,0],   levels: [87,86,0,0], vel_sens: 5 },
            ],
        },
        // BANJO — ROM1B — Algorithm 8, fb 7
        PatchPreset {
            algorithm: 8,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [95,62,28,58], levels: [99,60,0,0],  vel_sens: 0 },
                OpPreset { freq_ratio: 1.0,  output_level: 80, rates: [99,20,0,0],   levels: [99,0,0,0],   vel_sens: 0 },
                OpPreset { freq_ratio: 1.0,  output_level: 91, rates: [98,36,44,56], levels: [99,99,0,0],  vel_sens: 0 },
                OpPreset { freq_ratio: 5.0,  output_level: 78, rates: [99,30,20,54], levels: [99,95,0,0],  vel_sens: 0 },
                OpPreset { freq_ratio: 1.0,  output_level: 75, rates: [99,77,26,48], levels: [99,98,0,0],  vel_sens: 0 },
                OpPreset { freq_ratio: 15.0, output_level: 87, rates: [99,85,43,71], levels: [99,77,0,0],  vel_sens: 0 },
            ],
        },
        // GUITAR 1 — ROM1A — Algorithm 8, fb 7
        PatchPreset {
            algorithm: 8,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [74,85,27,70], levels: [99,95,0,0], vel_sens: 5 },
                OpPreset { freq_ratio: 3.0,  output_level: 93, rates: [91,25,39,60], levels: [99,86,0,0], vel_sens: 7 },
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [78,87,22,75], levels: [99,92,0,0], vel_sens: 7 },
                OpPreset { freq_ratio: 3.0,  output_level: 89, rates: [81,87,22,75], levels: [99,92,0,0], vel_sens: 4 },
                OpPreset { freq_ratio: 3.0,  output_level: 99, rates: [81,87,22,75], levels: [99,92,0,0], vel_sens: 7 },
                OpPreset { freq_ratio: 12.0, output_level: 57, rates: [99,57,99,75], levels: [99,0,0,0],  vel_sens: 6 },
            ],
        },
        // PIANO 1 — ROM1A — Algorithm 19, fb 6
        PatchPreset {
            algorithm: 19,
            feedback: 6,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [81,25,20,48], levels: [99,82,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 87, rates: [99,0,25,0],   levels: [99,75,0,0], vel_sens: 0 },
                OpPreset { freq_ratio: 3.0, output_level: 57, rates: [81,25,25,14], levels: [99,99,99,0], vel_sens: 0 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [81,23,22,45], levels: [99,78,0,0], vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 93, rates: [81,58,36,39], levels: [99,14,0,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0, output_level: 82, rates: [99,0,25,0],   levels: [99,75,0,0], vel_sens: 0 },
            ],
        },
        // CELESTE — ROM1B — Algorithm 31, fb 5
        PatchPreset {
            algorithm: 31,
            feedback: 5,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [96,30,25,40], levels: [99,80,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 86, rates: [96,30,25,40], levels: [99,80,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 88, rates: [96,30,25,40], levels: [99,80,0,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 97, rates: [96,30,25,40], levels: [99,80,0,0], vel_sens: 0 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [96,82,25,40], levels: [99,80,0,0], vel_sens: 7 },
                OpPreset { freq_ratio: 5.0, output_level: 65, rates: [96,30,25,40], levels: [99,80,0,0], vel_sens: 7 },
            ],
        },
        // COW BELL — ROM2A — Algorithm 6, fb 0
        PatchPreset {
            algorithm: 6,
            feedback: 0,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [96,45,50,50], levels: [99,90,0,0], vel_sens: 1 },
                OpPreset { freq_ratio: 7.0, output_level: 99, rates: [96,80,50,33], levels: [78,75,0,0], vel_sens: 0 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [96,65,65,50], levels: [99,0,0,0],  vel_sens: 0 },
                OpPreset { freq_ratio: 4.0, output_level: 80, rates: [96,98,44,33], levels: [99,96,97,0], vel_sens: 0 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [96,76,50,46], levels: [99,90,0,0], vel_sens: 0 },
                OpPreset { freq_ratio: 8.0, output_level: 74, rates: [96,66,99,33], levels: [99,96,89,0], vel_sens: 0 },
            ],
        },
        // SYN-BASS 1 — ROM2B — Algorithm 3, fb 7
        PatchPreset {
            algorithm: 3,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 2.0, output_level: 99, rates: [99,76,99,99], levels: [99,88,96,0], vel_sens: 0 },
                OpPreset { freq_ratio: 1.0, output_level: 76, rates: [61,38,25,47], levels: [99,72,72,0], vel_sens: 0 },
                OpPreset { freq_ratio: 1.0, output_level: 83, rates: [99,39,25,35], levels: [99,71,64,0], vel_sens: 0 },
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [99,76,99,99], levels: [99,88,96,0], vel_sens: 0 },
                OpPreset { freq_ratio: 1.0, output_level: 78, rates: [99,39,25,71], levels: [99,71,64,0], vel_sens: 0 },
                OpPreset { freq_ratio: 1.0, output_level: 75, rates: [61,38,25,32], levels: [99,72,72,0], vel_sens: 0 },
            ],
        },
        // TIMPANI — ROM1A — Algorithm 16, fb 7
        PatchPreset {
            algorithm: 16,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 0.5, output_level: 99, rates: [99,36,98,33], levels: [99,0,0,0],  vel_sens: 1 },
                OpPreset { freq_ratio: 0.5, output_level: 86, rates: [99,74,0,0],   levels: [99,0,0,0],  vel_sens: 1 },
                OpPreset { freq_ratio: 0.5, output_level: 85, rates: [99,77,26,23], levels: [99,72,0,0], vel_sens: 0 },
                OpPreset { freq_ratio: 0.5, output_level: 87, rates: [99,31,17,30], levels: [99,75,0,0], vel_sens: 7 },
                OpPreset { freq_ratio: 0.5, output_level: 73, rates: [99,50,26,19], levels: [99,0,0,0],  vel_sens: 1 },
                OpPreset { freq_ratio: 0.5, output_level: 73, rates: [98,2,26,27],  levels: [98,0,0,0],  vel_sens: 1 },
            ],
        },
        // PAN FLUTE — ROM4A — Algorithm 3, fb 7
        PatchPreset {
            algorithm: 3,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 2.0, output_level: 97, rates: [67,41,28,75], levels: [99,99,80,0], vel_sens: 0 },
                OpPreset { freq_ratio: 8.0, output_level: 75, rates: [99,72,31,17], levels: [60,70,0,0],  vel_sens: 0 },
                OpPreset { freq_ratio: 7.0, output_level: 98, rates: [57,72,31,17], levels: [60,70,0,0],  vel_sens: 4 },
                OpPreset { freq_ratio: 2.0, output_level: 90, rates: [67,41,28,75], levels: [99,99,80,0], vel_sens: 0 },
                OpPreset { freq_ratio: 8.0, output_level: 94, rates: [99,72,31,17], levels: [60,70,0,0],  vel_sens: 0 },
                OpPreset { freq_ratio: 3.0, output_level: 88, rates: [57,72,31,17], levels: [60,70,0,0],  vel_sens: 0 },
            ],
        },
        // HORNS — ROM4A — Algorithm 18, fb 7
        PatchPreset {
            algorithm: 18,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0, output_level: 99, rates: [55,24,19,55], levels: [99,86,86,0], vel_sens: 3 },
                OpPreset { freq_ratio: 1.0, output_level: 70, rates: [37,34,15,70], levels: [85,0,0,0],   vel_sens: 2 },
                OpPreset { freq_ratio: 1.0, output_level: 77, rates: [39,35,22,50], levels: [99,86,86,0], vel_sens: 1 },
                OpPreset { freq_ratio: 1.0, output_level: 79, rates: [66,92,22,50], levels: [53,61,62,0], vel_sens: 2 },
                OpPreset { freq_ratio: 3.0, output_level: 70, rates: [48,55,22,50], levels: [98,61,62,0], vel_sens: 1 },
                OpPreset { freq_ratio: 8.0, output_level: 79, rates: [77,56,20,70], levels: [99,0,0,0],   vel_sens: 2 },
            ],
        },
        // TOY PIANO — ROM1B — Algorithm 30, fb 7
        PatchPreset {
            algorithm: 30,
            feedback: 7,
            ops: [
                OpPreset { freq_ratio: 1.0,  output_level: 99, rates: [99,99,36,41], levels: [99,99,0,0], vel_sens: 0 },
                OpPreset { freq_ratio: 2.0,  output_level: 98, rates: [99,71,30,42], levels: [99,70,0,0], vel_sens: 0 },
                OpPreset { freq_ratio: 4.0,  output_level: 99, rates: [99,71,39,42], levels: [99,70,0,0], vel_sens: 0 },
                OpPreset { freq_ratio: 12.0, output_level: 68, rates: [99,99,29,30], levels: [99,99,99,0], vel_sens: 0 },
                OpPreset { freq_ratio: 12.0, output_level: 66, rates: [99,99,99,42], levels: [99,99,99,0], vel_sens: 0 },
                OpPreset { freq_ratio: 1.0,  output_level: 85, rates: [99,99,36,41], levels: [99,99,0,0], vel_sens: 0 },
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
/// Routing decoded from Yamaha DX7 factory ROM and verified against Dexed source.
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
        // Alg 3: [6fb→5→4*] + [3→2→1*]
        3 => AlgorithmDef {
            modulates: [&[], &[0], &[1], &[], &[3], &[4]],
            carriers: &[0, 3],
            feedback_op: 5,
        },
        // Alg 4: [6→5→4fb*] + [3→2→1*]
        4 => AlgorithmDef {
            modulates: [&[], &[0], &[1], &[], &[3], &[4]],
            carriers: &[0, 3],
            feedback_op: 3,
        },
        // Alg 5: [6fb→5*] + [4→3*] + [2→1*]  — THE classic
        5 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[], &[4]],
            carriers: &[0, 2, 4],
            feedback_op: 5,
        },
        // Alg 6: [6→5fb*] + [4→3*] + [2→1*]
        6 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[], &[4]],
            carriers: &[0, 2, 4],
            feedback_op: 4,
        },
        // Alg 7: [6fb→5*] + [4→3+2→1*]  (3 sums with 4 to modulate 1)
        7 => AlgorithmDef {
            modulates: [&[], &[0], &[0], &[0], &[], &[4]],
            carriers: &[0, 4],
            feedback_op: 5,
        },
        // Alg 8: [6→5fb*] + [4→3+2→1*]
        8 => AlgorithmDef {
            modulates: [&[], &[0], &[0], &[0], &[], &[4]],
            carriers: &[0, 4],
            feedback_op: 4,
        },
        // Alg 9: [6→5→4→3*] + [2fb→1*]  (same topology as 2)
        9 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[3], &[4]],
            carriers: &[0, 2],
            feedback_op: 1,
        },
        // Alg 10: [6fb→5→4*] + [3→2→1*] (fb on 4 for this variant)
        // Actually: [6→5→4fb*] + [3*→(2→1)]  — 3 carriers
        10 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[3], &[4]],
            carriers: &[0, 2],
            feedback_op: 2,
        },
        // Alg 11: [6fb→5→4*] + [3→2*→1]
        11 => AlgorithmDef {
            modulates: [&[], &[0], &[1], &[], &[3], &[4]],
            carriers: &[0, 3],
            feedback_op: 5,
        },
        // Alg 12: [6→5→4→3*] + [2fb→1*]
        12 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[3], &[4]],
            carriers: &[0, 2],
            feedback_op: 1,
        },
        // Alg 13: [6fb→5→4*] + [3→2→1*]
        13 => AlgorithmDef {
            modulates: [&[], &[0], &[1], &[], &[3], &[4]],
            carriers: &[0, 3],
            feedback_op: 5,
        },
        // Alg 14: [6fb→5→4*] + [3→2→1*]  (slight variant)
        14 => AlgorithmDef {
            modulates: [&[], &[0], &[1], &[], &[3], &[4]],
            carriers: &[0, 3],
            feedback_op: 5,
        },
        // Alg 15: [6→5→4→3*] + [2fb→1*]
        15 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[3], &[4]],
            carriers: &[0, 2],
            feedback_op: 1,
        },
        // Alg 16: [6fb→5] + [4→3*] + [2→(1*,5)]
        // OP6fb, OP2 and OP5 modulate OP1, OP4→OP3
        16 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[0], &[4]],
            carriers: &[0, 2],
            feedback_op: 5,
        },
        // Alg 17: [2fb→1*] + [6→5→(3*+4→3*)]
        // OP6→OP5, OP5 and OP4 modulate OP3
        17 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[2], &[4]],
            carriers: &[0, 2],
            feedback_op: 1,
        },
        // Alg 18: [6→5→4fb→3*] + [2*] + [1*]
        // OP6→OP5→OP4, OP4(fb) modulates OP3, OP2 and OP1 are carriers
        18 => AlgorithmDef {
            modulates: [&[], &[], &[], &[2], &[3], &[4]],
            carriers: &[0, 1, 2],
            feedback_op: 3,
        },
        // Alg 19: [6fb→(5*,4*,3*)] + [2→1*]  (6 modulates 3 carriers)
        19 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[], &[], &[4, 3, 2]],
            carriers: &[0, 2, 3, 4],
            feedback_op: 5,
        },
        // Alg 20: [3fb→2→1*] + [6*,5*,4*]  (3 carriers + 3-op chain)
        20 => AlgorithmDef {
            modulates: [&[], &[0], &[1], &[], &[], &[]],
            carriers: &[0, 3, 4, 5],
            feedback_op: 2,
        },
        // Alg 21: [6*,5*,4*,3fb→2→1*]  (4 carriers)
        21 => AlgorithmDef {
            modulates: [&[], &[0], &[1], &[], &[], &[]],
            carriers: &[0, 3, 4, 5],
            feedback_op: 2,
        },
        // Alg 22: [6fb→(5*,4*,3*)] + [2*] + [1*]
        22 => AlgorithmDef {
            modulates: [&[], &[], &[], &[], &[], &[4, 3, 2]],
            carriers: &[0, 1, 2, 3, 4],
            feedback_op: 5,
        },
        // Alg 23: [6fb→(5*,4*)] + [3→2*] + [1*]
        23 => AlgorithmDef {
            modulates: [&[], &[], &[1], &[], &[], &[4, 3]],
            carriers: &[0, 1, 3, 4],
            feedback_op: 5,
        },
        // Alg 24: [6fb→(5*,4*,3*,2*,1*)]
        24 => AlgorithmDef {
            modulates: [&[], &[], &[], &[], &[], &[4, 3, 2, 1, 0]],
            carriers: &[0, 1, 2, 3, 4],
            feedback_op: 5,
        },
        // Alg 25: [6fb→(5*,4*,3*)] + [2*] + [1*]  (variant)
        25 => AlgorithmDef {
            modulates: [&[], &[], &[], &[], &[], &[4, 3, 2]],
            carriers: &[0, 1, 2, 3, 4],
            feedback_op: 5,
        },
        // Alg 26: [6fb→5→4*] + [3→2*] + [1*]
        26 => AlgorithmDef {
            modulates: [&[], &[], &[1], &[], &[3], &[4]],
            carriers: &[0, 1, 3],
            feedback_op: 5,
        },
        // Alg 27: [3fb→2→1*] + [6→5*] + [4*]
        27 => AlgorithmDef {
            modulates: [&[], &[0], &[1], &[], &[], &[4]],
            carriers: &[0, 3, 4],
            feedback_op: 2,
        },
        // Alg 28: [5fb→4→3*] + [6*] + [2→1*]
        28 => AlgorithmDef {
            modulates: [&[], &[0], &[], &[2], &[3], &[]],
            carriers: &[0, 2, 5],
            feedback_op: 4,
        },
        // Alg 29: [6fb→5*] + [4→3*] + [2*] + [1*]
        29 => AlgorithmDef {
            modulates: [&[], &[], &[], &[2], &[], &[4]],
            carriers: &[0, 1, 2, 4],
            feedback_op: 5,
        },
        // Alg 30: [5fb→4→3*] + [6*] + [2*] + [1*]
        30 => AlgorithmDef {
            modulates: [&[], &[], &[], &[2], &[3], &[]],
            carriers: &[0, 1, 2, 5],
            feedback_op: 4,
        },
        // Alg 31: [6fb→5*] + [4*] + [3*] + [2*] + [1*]
        31 => AlgorithmDef {
            modulates: [&[], &[], &[], &[], &[], &[4]],
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
            let out = process_buffers(&mut s, &[note_on(60, 100, 0)], 2000);
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
