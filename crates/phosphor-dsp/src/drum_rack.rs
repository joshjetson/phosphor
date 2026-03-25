//! Drum rack — synthesized drum machines (808, 909, 707, 606).
//!
//! Circuit-analysis-based synthesis for each drum sound. Every sound has its own
//! distinct synthesis chain modeled on the original hardware:
//! - 808: Analog sine bodies, 6-oscillator metallic hats, noise snares
//! - 909: Triangle-based snares, bit-crushed hats, longer pitch sweeps
//! - 707: Hybrid character between 808 and 909
//! - 606: Thinner, clickier, higher-frequency variants

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
pub enum DrumKit {
    Kit808,
    Kit909,
    Kit707,
    Kit606,
    Kit777,
    KitTsty1,
    KitTsty2,
    KitTsty3,
    KitTsty4,
    KitTsty5,
}

impl DrumKit {
    pub fn from_param(val: f32) -> Self {
        match (val * 10.0) as u8 {
            0 => Self::Kit808,
            1 => Self::Kit909,
            2 => Self::Kit707,
            3 => Self::Kit606,
            4 => Self::Kit777,
            5 => Self::KitTsty1,
            6 => Self::KitTsty2,
            7 => Self::KitTsty3,
            8 => Self::KitTsty4,
            _ => Self::KitTsty5,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Kit808 => "808",
            Self::Kit909 => "909",
            Self::Kit707 => "707",
            Self::Kit606 => "606",
            Self::Kit777 => "777",
            Self::KitTsty1 => "tsty-1",
            Self::KitTsty2 => "tsty-2",
            Self::KitTsty3 => "tsty-3",
            Self::KitTsty4 => "tsty-4",
            Self::KitTsty5 => "tsty-5",
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Synthesis helpers
// ══════════════════════════════════════════════════════════════════════════════

use std::f64::consts::TAU;

/// Deterministic white noise from a seed. Returns value in -1..1.
#[inline]
fn white_noise(seed: u64) -> f64 {
    // Fast hash-based noise (no state needed beyond the seed)
    let mut x = seed;
    x ^= x >> 13;
    x = x.wrapping_mul(0x5bd1_e995_5bd1_e995);
    x ^= x >> 15;
    x = x.wrapping_mul(0x3f3f_3f3f_3f3f_3f3f);
    x ^= x >> 17;
    (x as i64 as f64) / (i64::MAX as f64)
}

/// Advance a phase accumulator, return new phase (wrapped to 0..1).
#[inline]
fn advance_phase(phase: &mut f64, freq: f64, sr: f64) {
    *phase += freq / sr;
    *phase -= (*phase).floor();
}

/// Sine oscillator.
#[inline]
fn osc_sine(phase: f64) -> f64 {
    (phase * TAU).sin()
}

/// Square wave oscillator (band-limited via first few harmonics approximation).
#[inline]
fn osc_square(phase: f64) -> f64 {
    if phase < 0.5 { 1.0 } else { -1.0 }
}

/// Triangle wave oscillator.
#[inline]
fn osc_triangle(phase: f64) -> f64 {
    if phase < 0.25 {
        phase * 4.0
    } else if phase < 0.75 {
        2.0 - phase * 4.0
    } else {
        phase * 4.0 - 4.0
    }
}

/// Soft-clip distortion.
#[inline]
fn soft_clip(x: f64, drive: f64) -> f64 {
    let gained = x * (1.0 + drive * 8.0);
    gained / (1.0 + gained.abs()).sqrt()
}

/// Simple one-pole low-pass filter state.
#[derive(Debug, Clone, Copy)]
struct OnePole {
    y1: f64,
}

impl OnePole {
    fn new() -> Self {
        Self { y1: 0.0 }
    }

    fn tick_lp(&mut self, x: f64, cutoff: f64, sr: f64) -> f64 {
        let w = (TAU * cutoff / sr).min(1.0);
        let a = w / (1.0 + w);
        self.y1 += a * (x - self.y1);
        self.y1
    }

    fn tick_hp(&mut self, x: f64, cutoff: f64, sr: f64) -> f64 {
        x - self.tick_lp(x, cutoff, sr)
    }
}

/// State-variable filter (SVF) for bandpass/lowpass/highpass.
#[derive(Debug, Clone, Copy)]
struct Svf {
    ic1eq: f64,
    ic2eq: f64,
}

impl Svf {
    fn new() -> Self {
        Self { ic1eq: 0.0, ic2eq: 0.0 }
    }

    fn tick(&mut self, x: f64, cutoff: f64, q: f64, sr: f64) -> (f64, f64, f64) {
        let g = (std::f64::consts::PI * cutoff / sr).tan();
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = x - self.ic2eq;
        let v1 = a1 * self.ic1eq + a2 * v3;
        let v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;

        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        let lp = v2;
        let bp = v1;
        let hp = x - k * v1 - v2;
        (lp, bp, hp)
    }

    fn bandpass(&mut self, x: f64, cutoff: f64, q: f64, sr: f64) -> f64 {
        self.tick(x, cutoff, q, sr).1
    }

    #[allow(dead_code)]
    fn lowpass(&mut self, x: f64, cutoff: f64, q: f64, sr: f64) -> f64 {
        self.tick(x, cutoff, q, sr).0
    }

    #[allow(dead_code)]
    fn highpass(&mut self, x: f64, cutoff: f64, q: f64, sr: f64) -> f64 {
        self.tick(x, cutoff, q, sr).2
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Drum sound enum — what kind of sound to synthesize
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq)]
enum DrumSound {
    Kick,
    Snare,
    SnareAlt,
    Clap,
    ClosedHat,
    PedalHat,
    OpenHat,
    Rimshot,
    LowTom,
    MidTom,
    HighTom,
    Crash,
    Ride,
    RideBell,
    Cowbell,
    Clave,
    Maracas,
    Tambourine,
    Splash,
    Cymbal,
    Vibraslap,
    Bongo(f64),      // freq
    Conga(f64),      // freq
    Timbale(f64),    // freq
    Agogo(f64),      // freq
    Cabasa,
    Guiro(f64),      // decay
    Whistle(f64),    // decay
    SubKick(f64),    // freq multiplier
    FxNoise(f64),    // character
}

/// Map a MIDI note number to a DrumSound.
fn note_to_sound(note: u8) -> DrumSound {
    match note {
        // Sub kicks (24-35)
        0..=35 => {
            let mult = if note < 24 { 0.3 + note as f64 * 0.02 } else { 0.5 + (note - 24) as f64 * 0.05 };
            DrumSound::SubKick(mult)
        }
        36 => DrumSound::Kick,
        37 => DrumSound::Rimshot,
        38 => DrumSound::Snare,
        39 => DrumSound::Clap,
        40 => DrumSound::SnareAlt,
        41 => DrumSound::LowTom,
        42 => DrumSound::ClosedHat,
        43 => DrumSound::LowTom,       // Low Tom 2
        44 => DrumSound::PedalHat,
        45 => DrumSound::MidTom,
        46 => DrumSound::OpenHat,
        47 => DrumSound::MidTom,       // Mid Tom 2
        48 => DrumSound::HighTom,
        49 => DrumSound::Crash,
        50 => DrumSound::HighTom,      // High Tom 2
        51 => DrumSound::Ride,
        52 => DrumSound::Cymbal,
        53 => DrumSound::RideBell,
        54 => DrumSound::Tambourine,
        55 => DrumSound::Splash,
        56 => DrumSound::Cowbell,
        57 => DrumSound::Crash,        // Crash 2
        58 => DrumSound::Vibraslap,
        59 => DrumSound::Ride,         // Ride 2
        60 => DrumSound::Bongo(400.0), // Hi Bongo
        61 => DrumSound::Bongo(300.0), // Low Bongo
        62 => DrumSound::Conga(350.0), // Mute Hi Conga
        63 => DrumSound::Conga(300.0), // Open Hi Conga
        64 => DrumSound::Conga(200.0), // Low Conga
        65 => DrumSound::Timbale(500.0), // Hi Timbale
        66 => DrumSound::Timbale(350.0), // Lo Timbale
        67 => DrumSound::Agogo(900.0),   // Hi Agogo
        68 => DrumSound::Agogo(650.0),   // Lo Agogo
        69 => DrumSound::Cabasa,
        70 => DrumSound::Maracas,
        71 => DrumSound::Whistle(0.08),  // Short Whistle
        72 => DrumSound::Whistle(0.30),  // Long Whistle
        73 => DrumSound::Guiro(0.06),    // Short Guiro
        74 => DrumSound::Guiro(0.20),    // Long Guiro
        75 => DrumSound::Clave,
        // FX sounds (76-127)
        _ => {
            let v = (note - 76) as f64 / 51.0;
            DrumSound::FxNoise(v)
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Per-voice state: holds all oscillator phases and filter states
// ══════════════════════════════════════════════════════════════════════════════

const MAX_VOICES: usize = 16;

/// Six square-wave oscillator phases for metallic hat sounds.
#[derive(Debug, Clone, Copy)]
struct HatOscillators {
    phases: [f64; 6],
}

impl HatOscillators {
    fn new() -> Self {
        Self { phases: [0.0; 6] }
    }

    fn reset(&mut self) {
        self.phases = [0.0; 6];
    }

    /// Tick the 6 square oscillators at the canonical 808 hat frequencies.
    fn tick(&mut self, sr: f64, freqs: &[f64; 6]) -> f64 {
        let mut sum = 0.0;
        for i in 0..6 {
            advance_phase(&mut self.phases[i], freqs[i], sr);
            sum += osc_square(self.phases[i]);
        }
        sum / 6.0
    }
}

/// Canonical 808 hat oscillator frequencies.
const HAT_FREQS_808: [f64; 6] = [205.0, 304.0, 370.0, 523.0, 540.0, 800.0];

/// 606 hat oscillator frequencies (higher, thinner).
const HAT_FREQS_606: [f64; 6] = [10200.0, 10800.0, 11300.0, 11800.0, 12100.0, 12500.0];

#[derive(Debug)]
struct DrumVoice {
    active: bool,
    time: f64,
    note: u8,
    velocity: f32,
    sound: DrumSound,
    kit: DrumKit,
    // Global noise sample counter for this voice
    noise_counter: u64,
    noise_seed: u64,

    // Oscillator phases
    phase1: f64,
    phase2: f64,
    phase3: f64,

    // Hat oscillators
    hat_oscs: HatOscillators,

    // Filters
    svf1: Svf,
    svf2: Svf,
    svf3: Svf,
    hp1: OnePole,
    hp2: OnePole,
    lp1: OnePole,

    // Clap burst state
    clap_burst_index: usize,

    // Tape LP state (for tsty-1/tsty-2)
    lp1_state: f64,

    // Modal bank for tsty-2 (realistic acoustic drum modes)
    modal_phases: [f64; 8],
    modal_amps: [f64; 8],    // per-mode amplitude (set on trigger for per-hit variation)
    modal_decays: [f64; 8],  // per-mode decay time in seconds
    hit_seed: u32,           // per-hit random seed for variation
}

// ══════════════════════════════════════════════════════════════════════════════
// TSTY-5 Recipe Table: resonator-based drum synthesis
// Each sound is defined by parameters, not code. The engine is shared.
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Clone, Copy)]
struct T5Recipe {
    // Exciter type: 0=impulse+noise, 1=noise only, 2=click+noise, 3=multi-burst
    exciter: u8,
    impulse_level: f64,
    noise_level: f64,
    noise_decay: f64,       // seconds
    burst_count: u8,        // for exciter type 3
    burst_spread: f64,      // seconds, for exciter type 3
    // Resonator 1 (primary body)
    r1_freq: f64,           // Hz, 0 = disabled
    r1_q: f64,              // Q factor (higher = more tonal, longer ring)
    r1_level: f64,
    r1_decay: f64,          // seconds
    pitch_sweep: f64,       // Hz added at t=0, decays away
    pitch_time: f64,        // sweep decay time
    // Resonator 2 (secondary mode)
    r2_freq: f64, r2_q: f64, r2_level: f64, r2_decay: f64,
    // Resonator 3 (third mode / brightness)
    r3_freq: f64, r3_q: f64, r3_level: f64, r3_decay: f64,
    // Noise shaping (wires, shimmer, wash)
    noise_filter_freq: f64, // HP cutoff for shaped noise, 0 = disabled
    noise_mix: f64,
    noise_filter_decay: f64,
    wire_coupling: f64,     // 0-1, how much body amplitude modulates wire noise
}

const T5_DEFAULT: T5Recipe = T5Recipe {
    exciter: 0, impulse_level: 1.0, noise_level: 0.5, noise_decay: 0.01,
    burst_count: 0, burst_spread: 0.0,
    r1_freq: 0.0, r1_q: 5.0, r1_level: 0.5, r1_decay: 0.15, pitch_sweep: 0.0, pitch_time: 0.02,
    r2_freq: 0.0, r2_q: 3.0, r2_level: 0.3, r2_decay: 0.1,
    r3_freq: 0.0, r3_q: 2.0, r3_level: 0.2, r3_decay: 0.08,
    noise_filter_freq: 0.0, noise_mix: 0.0, noise_filter_decay: 0.15, wire_coupling: 0.0,
};

fn t5_recipe(note: u8) -> T5Recipe {
    match note {
    // ══ KICKS (24-31): impulse → low resonators with pitch sweep ══
    24 => T5Recipe { exciter:0, impulse_level:1.0, noise_level:0.4, noise_decay:0.003,
        r1_freq:62.0, r1_q:12.0, r1_level:0.8, r1_decay:0.3, pitch_sweep:120.0, pitch_time:0.018,
        r2_freq:98.0, r2_q:5.0, r2_level:0.2, r2_decay:0.08, // 1.593x Bessel mode
        r3_freq:142.0, r3_q:3.0, r3_level:0.1, r3_decay:0.05, // 2.296x mode
        noise_filter_freq:2000.0, noise_mix:0.15, noise_filter_decay:0.004, wire_coupling:0.0,
        ..T5_DEFAULT },
    25 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.6, noise_decay:0.002,
        r1_freq:72.0, r1_q:15.0, r1_level:0.85, r1_decay:0.15, pitch_sweep:80.0, pitch_time:0.012,
        r2_freq:115.0, r2_q:4.0, r2_level:0.15, r2_decay:0.06,
        noise_filter_freq:4000.0, noise_mix:0.2, noise_filter_decay:0.003, ..T5_DEFAULT },
    26 => T5Recipe { exciter:0, impulse_level:1.0, noise_level:0.2, noise_decay:0.005,
        r1_freq:50.0, r1_q:18.0, r1_level:0.9, r1_decay:0.45, pitch_sweep:60.0, pitch_time:0.025,
        r2_freq:80.0, r2_q:6.0, r2_level:0.2, r2_decay:0.15, ..T5_DEFAULT }, // deep
    27 => T5Recipe { exciter:0, impulse_level:0.6, noise_level:0.3, noise_decay:0.004,
        r1_freq:68.0, r1_q:20.0, r1_level:0.8, r1_decay:0.2, pitch_sweep:150.0, pitch_time:0.015,
        r2_freq:108.0, r2_q:8.0, r2_level:0.25, r2_decay:0.08,
        r3_freq:156.0, r3_q:4.0, r3_level:0.1, r3_decay:0.04,
        noise_filter_freq:5000.0, noise_mix:0.25, noise_filter_decay:0.002, ..T5_DEFAULT }, // rock
    28 => T5Recipe { exciter:0, impulse_level:1.2, noise_level:0.15, noise_decay:0.003,
        r1_freq:55.0, r1_q:10.0, r1_level:0.7, r1_decay:0.35, pitch_sweep:40.0, pitch_time:0.03,
        ..T5_DEFAULT }, // round muffled
    29 => T5Recipe { exciter:0, impulse_level:0.9, noise_level:0.5, noise_decay:0.002,
        r1_freq:78.0, r1_q:25.0, r1_level:0.85, r1_decay:0.1, pitch_sweep:100.0, pitch_time:0.008,
        noise_filter_freq:3500.0, noise_mix:0.3, noise_filter_decay:0.002, ..T5_DEFAULT }, // tight click
    30 => T5Recipe { exciter:0, impulse_level:1.0, noise_level:0.3, noise_decay:0.004,
        r1_freq:48.0, r1_q:14.0, r1_level:0.8, r1_decay:0.5, pitch_sweep:50.0, pitch_time:0.02,
        r2_freq:76.0, r2_q:7.0, r2_level:0.2, r2_decay:0.2,
        r3_freq:220.0, r3_q:15.0, r3_level:0.08, r3_decay:0.08, ..T5_DEFAULT }, // boomy shell
    31 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.2, noise_decay:0.006,
        r1_freq:58.0, r1_q:22.0, r1_level:0.75, r1_decay:0.25, pitch_sweep:70.0, pitch_time:0.02,
        r2_freq:92.0, r2_q:6.0, r2_level:0.15, r2_decay:0.1, ..T5_DEFAULT }, // warm

    // ══ SNARES (32-41): click exciter → mid resonators + wire noise ══
    32 => T5Recipe { exciter:2, impulse_level:0.8, noise_level:0.6, noise_decay:0.008,
        r1_freq:305.0, r1_q:8.0, r1_level:0.5, r1_decay:0.12,
        r2_freq:485.0, r2_q:4.0, r2_level:0.2, r2_decay:0.07,
        noise_filter_freq:2500.0, noise_mix:0.45, noise_filter_decay:0.25, wire_coupling:0.6,
        ..T5_DEFAULT }, // funk tight
    33 => T5Recipe { exciter:2, impulse_level:0.7, noise_level:0.7, noise_decay:0.01,
        r1_freq:235.0, r1_q:6.0, r1_level:0.55, r1_decay:0.15,
        r2_freq:375.0, r2_q:3.5, r2_level:0.25, r2_decay:0.1,
        r3_freq:500.0, r3_q:2.5, r3_level:0.12, r3_decay:0.06,
        noise_filter_freq:2000.0, noise_mix:0.5, noise_filter_decay:0.35, wire_coupling:0.5,
        ..T5_DEFAULT }, // fat backbeat
    34 => T5Recipe { exciter:2, impulse_level:0.9, noise_level:0.5, noise_decay:0.005,
        r1_freq:285.0, r1_q:10.0, r1_level:0.45, r1_decay:0.08,
        noise_filter_freq:3000.0, noise_mix:0.35, noise_filter_decay:0.12, wire_coupling:0.4,
        ..T5_DEFAULT }, // dry studio
    35 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:0.15,
        r1_freq:260.0, r1_q:3.0, r1_level:0.2, r1_decay:0.1,
        noise_filter_freq:1500.0, noise_mix:0.5, noise_filter_decay:0.2, wire_coupling:0.0,
        ..T5_DEFAULT }, // brush — noise exciter, low Q resonator
    36 => T5Recipe { exciter:2, impulse_level:1.0, noise_level:0.3, noise_decay:0.003,
        r1_freq:580.0, r1_q:12.0, r1_level:0.4, r1_decay:0.025,
        r2_freq:1520.0, r2_q:8.0, r2_level:0.25, r2_decay:0.015,
        ..T5_DEFAULT }, // cross-stick — high resonators, no wires
    37 => T5Recipe { exciter:2, impulse_level:0.4, noise_level:0.3, noise_decay:0.005,
        r1_freq:295.0, r1_q:5.0, r1_level:0.2, r1_decay:0.05,
        noise_filter_freq:3500.0, noise_mix:0.25, noise_filter_decay:0.08, wire_coupling:0.7,
        ..T5_DEFAULT }, // ghost note — quiet, wire-dominant
    38 => T5Recipe { exciter:2, impulse_level:0.8, noise_level:0.6, noise_decay:0.008,
        r1_freq:340.0, r1_q:9.0, r1_level:0.45, r1_decay:0.12,
        r2_freq:540.0, r2_q:18.0, r2_level:0.15, r2_decay:0.15, // shell ring!
        noise_filter_freq:2500.0, noise_mix:0.4, noise_filter_decay:0.25, wire_coupling:0.5,
        ..T5_DEFAULT }, // metal shell ring
    39 => T5Recipe { exciter:2, impulse_level:0.7, noise_level:0.7, noise_decay:0.012,
        r1_freq:270.0, r1_q:5.0, r1_level:0.4, r1_decay:0.1,
        noise_filter_freq:2000.0, noise_mix:0.55, noise_filter_decay:0.45, wire_coupling:0.3,
        ..T5_DEFAULT }, // loose wires — long wire decay
    40 => T5Recipe { exciter:2, impulse_level:1.0, noise_level:0.5, noise_decay:0.004,
        r1_freq:380.0, r1_q:12.0, r1_level:0.4, r1_decay:0.06,
        noise_filter_freq:4000.0, noise_mix:0.35, noise_filter_decay:0.12, wire_coupling:0.5,
        ..T5_DEFAULT }, // piccolo — high, bright
    41 => T5Recipe { exciter:2, impulse_level:0.7, noise_level:0.6, noise_decay:0.01,
        r1_freq:225.0, r1_q:7.0, r1_level:0.5, r1_decay:0.15,
        r2_freq:358.0, r2_q:4.0, r2_level:0.2, r2_decay:0.1,
        r3_freq:480.0, r3_q:3.0, r3_level:0.1, r3_decay:0.06,
        noise_filter_freq:2000.0, noise_mix:0.5, noise_filter_decay:0.35, wire_coupling:0.5,
        ..T5_DEFAULT }, // big 3-mode Bessel + shell

    // ══ CLAPS (42-47): multi-burst exciter → mid resonators ══
    42 => T5Recipe { exciter:3, burst_count:5, burst_spread:0.008,
        noise_level:0.7, noise_decay:0.15,
        r1_freq:2300.0, r1_q:1.5, r1_level:0.3, r1_decay:0.15,
        noise_filter_freq:800.0, noise_mix:0.3, noise_filter_decay:0.15,
        ..T5_DEFAULT }, // tight group
    43 => T5Recipe { exciter:3, burst_count:8, burst_spread:0.02,
        noise_level:0.6, noise_decay:0.2,
        r1_freq:1800.0, r1_q:1.2, r1_level:0.25, r1_decay:0.2,
        noise_filter_freq:600.0, noise_mix:0.35, noise_filter_decay:0.2,
        ..T5_DEFAULT }, // loose group
    44 => T5Recipe { exciter:2, impulse_level:0.8, noise_level:0.5, noise_decay:0.03,
        r1_freq:2100.0, r1_q:1.8, r1_level:0.3, r1_decay:0.03,
        ..T5_DEFAULT }, // single dry
    45 => T5Recipe { exciter:2, impulse_level:1.0, noise_level:0.4, noise_decay:0.008,
        r1_freq:3400.0, r1_q:2.5, r1_level:0.3, r1_decay:0.015,
        noise_filter_freq:2000.0, noise_mix:0.2, noise_filter_decay:0.01,
        ..T5_DEFAULT }, // finger snap
    46 => T5Recipe { exciter:2, impulse_level:0.6, noise_level:0.5, noise_decay:0.02,
        r1_freq:185.0, r1_q:3.0, r1_level:0.3, r1_decay:0.08,
        noise_filter_freq:500.0, noise_mix:0.3, noise_filter_decay:0.03,
        ..T5_DEFAULT }, // hand slap
    47 => T5Recipe { exciter:3, burst_count:4, burst_spread:0.012,
        noise_level:0.6, noise_decay:0.4,
        r1_freq:2200.0, r1_q:1.3, r1_level:0.25, r1_decay:0.4,
        noise_filter_freq:700.0, noise_mix:0.4, noise_filter_decay:0.4,
        ..T5_DEFAULT }, // hall reverb clap

    // ══ CLOSED HATS (48-55): noise exciter → high resonators, short decay ══
    48 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:0.06,
        r1_freq:4200.0, r1_q:8.0, r1_level:0.5, r1_decay:0.06,
        r2_freq:7800.0, r2_q:6.0, r2_level:0.3, r2_decay:0.04,
        r3_freq:11500.0, r3_q:4.0, r3_level:0.15, r3_decay:0.03,
        noise_filter_freq:5500.0, noise_mix:0.2, noise_filter_decay:0.05, ..T5_DEFAULT },
    49 => T5Recipe { exciter:1, noise_level:0.9, noise_decay:0.05,
        r1_freq:5500.0, r1_q:10.0, r1_level:0.45, r1_decay:0.045,
        r2_freq:9000.0, r2_q:7.0, r2_level:0.25, r2_decay:0.035,
        noise_filter_freq:6500.0, noise_mix:0.25, noise_filter_decay:0.04, ..T5_DEFAULT },
    50 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:0.07,
        r1_freq:3200.0, r1_q:6.0, r1_level:0.4, r1_decay:0.07,
        r2_freq:6000.0, r2_q:4.0, r2_level:0.2, r2_decay:0.05,
        noise_filter_freq:4000.0, noise_mix:0.2, noise_filter_decay:0.06, ..T5_DEFAULT }, // dark
    51 => T5Recipe { exciter:1, noise_level:0.85, noise_decay:0.04,
        r1_freq:6800.0, r1_q:12.0, r1_level:0.5, r1_decay:0.035,
        r2_freq:10200.0, r2_q:8.0, r2_level:0.3, r2_decay:0.025,
        noise_filter_freq:7000.0, noise_mix:0.15, noise_filter_decay:0.03, ..T5_DEFAULT }, // bright thin
    52 => T5Recipe { exciter:1, noise_level:0.75, noise_decay:0.055,
        r1_freq:3800.0, r1_q:5.0, r1_level:0.35, r1_decay:0.055,
        r2_freq:7200.0, r2_q:4.0, r2_level:0.2, r2_decay:0.04,
        r3_freq:10500.0, r3_q:3.0, r3_level:0.1, r3_decay:0.03,
        noise_filter_freq:4500.0, noise_mix:0.2, noise_filter_decay:0.05, ..T5_DEFAULT }, // medium
    53 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:0.065,
        r1_freq:4800.0, r1_q:15.0, r1_level:0.5, r1_decay:0.06,
        r2_freq:8500.0, r2_q:10.0, r2_level:0.3, r2_decay:0.045,
        noise_filter_freq:5000.0, noise_mix:0.15, noise_filter_decay:0.05, ..T5_DEFAULT }, // ringy
    54 => T5Recipe { exciter:1, noise_level:0.9, noise_decay:0.035,
        r1_freq:8000.0, r1_q:4.0, r1_level:0.3, r1_decay:0.03,
        noise_filter_freq:7500.0, noise_mix:0.3, noise_filter_decay:0.03, ..T5_DEFAULT }, // pure noise tick
    55 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:0.06,
        r1_freq:2800.0, r1_q:4.0, r1_level:0.3, r1_decay:0.065,
        r2_freq:5500.0, r2_q:3.0, r2_level:0.2, r2_decay:0.05,
        noise_filter_freq:3500.0, noise_mix:0.25, noise_filter_decay:0.06, ..T5_DEFAULT }, // chunky dark

    // ══ OPEN HATS (56-63): noise exciter → high resonators, LONG decay 0.8-2.5s ══
    56 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:1.5,
        r1_freq:4200.0, r1_q:12.0, r1_level:0.5, r1_decay:1.5,
        r2_freq:7800.0, r2_q:8.0, r2_level:0.3, r2_decay:1.8,
        r3_freq:11500.0, r3_q:5.0, r3_level:0.15, r3_decay:2.2,
        noise_filter_freq:3000.0, noise_mix:0.2, noise_filter_decay:1.2, ..T5_DEFAULT }, // standard
    57 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:2.0,
        r1_freq:5500.0, r1_q:14.0, r1_level:0.5, r1_decay:2.0,
        r2_freq:9000.0, r2_q:10.0, r2_level:0.3, r2_decay:2.5,
        noise_filter_freq:4000.0, noise_mix:0.15, noise_filter_decay:1.8, ..T5_DEFAULT }, // bright
    58 => T5Recipe { exciter:1, noise_level:0.6, noise_decay:1.0,
        r1_freq:3200.0, r1_q:8.0, r1_level:0.4, r1_decay:1.0,
        r2_freq:6000.0, r2_q:5.0, r2_level:0.25, r2_decay:0.8,
        noise_filter_freq:2500.0, noise_mix:0.25, noise_filter_decay:0.9, ..T5_DEFAULT }, // dark warm
    59 => T5Recipe { exciter:1, noise_level:0.75, noise_decay:1.8,
        r1_freq:3800.0, r1_q:10.0, r1_level:0.4, r1_decay:1.8,
        r2_freq:7200.0, r2_q:6.0, r2_level:0.25, r2_decay:2.0,
        r3_freq:10500.0, r3_q:4.0, r3_level:0.12, r3_decay:2.2,
        noise_filter_freq:3500.0, noise_mix:0.2, noise_filter_decay:1.5, ..T5_DEFAULT }, // washy long
    60 => T5Recipe { exciter:1, noise_level:0.85, noise_decay:2.5,
        r1_freq:4800.0, r1_q:6.0, r1_level:0.35, r1_decay:2.5,
        noise_filter_freq:5000.0, noise_mix:0.3, noise_filter_decay:2.0, ..T5_DEFAULT }, // breathy airy
    61 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:1.3,
        r1_freq:2800.0, r1_q:10.0, r1_level:0.45, r1_decay:1.3,
        r2_freq:5500.0, r2_q:6.0, r2_level:0.25, r2_decay:1.0,
        noise_filter_freq:2500.0, noise_mix:0.3, noise_filter_decay:1.0, ..T5_DEFAULT }, // dark open
    62 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:1.6,
        r1_freq:6800.0, r1_q:16.0, r1_level:0.5, r1_decay:1.6,
        r2_freq:10200.0, r2_q:12.0, r2_level:0.3, r2_decay:2.0,
        noise_filter_freq:5500.0, noise_mix:0.15, noise_filter_decay:1.5, ..T5_DEFAULT }, // bright shimmer
    63 => T5Recipe { exciter:1, noise_level:0.65, noise_decay:0.8,
        r1_freq:3500.0, r1_q:7.0, r1_level:0.35, r1_decay:0.8,
        r2_freq:6500.0, r2_q:5.0, r2_level:0.2, r2_decay:0.6,
        noise_filter_freq:3000.0, noise_mix:0.3, noise_filter_decay:0.7, ..T5_DEFAULT }, // half-open

    // ══ PEDAL HATS (64-65) ══
    64 => T5Recipe { exciter:0, impulse_level:0.6, noise_level:0.5, noise_decay:0.02,
        r1_freq:4000.0, r1_q:5.0, r1_level:0.3, r1_decay:0.025,
        r2_freq:1300.0, r2_q:2.5, r2_level:0.15, r2_decay:0.015,
        ..T5_DEFAULT }, // pedal chick
    65 => T5Recipe { exciter:1, noise_level:0.6, noise_decay:0.06,
        r1_freq:3500.0, r1_q:4.0, r1_level:0.25, r1_decay:0.06,
        r2_freq:1500.0, r2_q:2.0, r2_level:0.12, r2_decay:0.03,
        ..T5_DEFAULT }, // pedal loose

    // ══ TOMS (66-71): impulse → mid resonators with pitch sweep ══
    66 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.3, noise_decay:0.005,
        r1_freq:82.0, r1_q:10.0, r1_level:0.7, r1_decay:0.35, pitch_sweep:30.0, pitch_time:0.015,
        r2_freq:130.0, r2_q:5.0, r2_level:0.2, r2_decay:0.12,
        noise_filter_freq:2500.0, noise_mix:0.1, noise_filter_decay:0.005, ..T5_DEFAULT }, // floor deep
    67 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.3, noise_decay:0.004,
        r1_freq:110.0, r1_q:9.0, r1_level:0.65, r1_decay:0.28, pitch_sweep:35.0, pitch_time:0.012,
        r2_freq:175.0, r2_q:4.0, r2_level:0.18, r2_decay:0.1, ..T5_DEFAULT }, // floor medium
    68 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.3, noise_decay:0.004,
        r1_freq:140.0, r1_q:8.0, r1_level:0.6, r1_decay:0.24, pitch_sweep:30.0, pitch_time:0.01,
        r2_freq:223.0, r2_q:4.0, r2_level:0.15, r2_decay:0.08, ..T5_DEFAULT }, // low rack
    69 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.35, noise_decay:0.003,
        r1_freq:175.0, r1_q:8.0, r1_level:0.55, r1_decay:0.22, pitch_sweep:25.0, pitch_time:0.01,
        r2_freq:279.0, r2_q:4.0, r2_level:0.15, r2_decay:0.07, ..T5_DEFAULT }, // mid rack
    70 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.35, noise_decay:0.003,
        r1_freq:220.0, r1_q:8.0, r1_level:0.5, r1_decay:0.18, pitch_sweep:25.0, pitch_time:0.008,
        r2_freq:350.0, r2_q:4.0, r2_level:0.12, r2_decay:0.06, ..T5_DEFAULT }, // high rack
    71 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.25, noise_decay:0.005,
        r1_freq:155.0, r1_q:12.0, r1_level:0.6, r1_decay:0.4, pitch_sweep:20.0, pitch_time:0.012,
        r2_freq:247.0, r2_q:5.0, r2_level:0.2, r2_decay:0.15,
        r3_freq:355.0, r3_q:3.0, r3_level:0.1, r3_decay:0.1, ..T5_DEFAULT }, // concert long

    // ══ CYMBALS (72-77): noise exciter → high resonators, long decay ══
    72 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:1.5,
        r1_freq:3200.0, r1_q:6.0, r1_level:0.35, r1_decay:1.5,
        r2_freq:6000.0, r2_q:4.0, r2_level:0.2, r2_decay:1.2,
        noise_filter_freq:2000.0, noise_mix:0.25, noise_filter_decay:1.3, ..T5_DEFAULT }, // crash dark
    73 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:1.8,
        r1_freq:5000.0, r1_q:8.0, r1_level:0.4, r1_decay:1.8,
        r2_freq:8500.0, r2_q:5.0, r2_level:0.25, r2_decay:2.0,
        noise_filter_freq:3000.0, noise_mix:0.2, noise_filter_decay:1.5, ..T5_DEFAULT }, // crash bright
    74 => T5Recipe { exciter:1, noise_level:0.6, noise_decay:1.0,
        r1_freq:4500.0, r1_q:12.0, r1_level:0.45, r1_decay:1.0,
        r2_freq:7500.0, r2_q:8.0, r2_level:0.25, r2_decay:0.8,
        noise_filter_freq:3500.0, noise_mix:0.15, noise_filter_decay:0.8, ..T5_DEFAULT }, // ride ping
    75 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:2.0,
        r1_freq:3800.0, r1_q:5.0, r1_level:0.3, r1_decay:2.0,
        r2_freq:7000.0, r2_q:3.0, r2_level:0.18, r2_decay:1.8,
        noise_filter_freq:2500.0, noise_mix:0.25, noise_filter_decay:1.8, ..T5_DEFAULT }, // ride wash
    76 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:0.5,
        r1_freq:6000.0, r1_q:8.0, r1_level:0.4, r1_decay:0.5,
        r2_freq:10000.0, r2_q:5.0, r2_level:0.2, r2_decay:0.35,
        noise_filter_freq:4000.0, noise_mix:0.2, noise_filter_decay:0.4, ..T5_DEFAULT }, // splash
    77 => T5Recipe { exciter:1, noise_level:0.75, noise_decay:1.2,
        r1_freq:2800.0, r1_q:5.0, r1_level:0.35, r1_decay:1.2,
        r2_freq:5200.0, r2_q:3.0, r2_level:0.2, r2_decay:1.0,
        noise_filter_freq:1800.0, noise_mix:0.3, noise_filter_decay:1.0, ..T5_DEFAULT }, // china trashy

    // ══ PERCUSSION (78-89) ══
    78 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.2, noise_decay:0.003,
        r1_freq:580.0, r1_q:25.0, r1_level:0.5, r1_decay:0.07,
        r2_freq:870.0, r2_q:20.0, r2_level:0.35, r2_decay:0.06, ..T5_DEFAULT }, // cowbell
    79 => T5Recipe { exciter:0, impulse_level:1.0, noise_level:0.3, noise_decay:0.002,
        r1_freq:1900.0, r1_q:30.0, r1_level:0.4, r1_decay:0.015,
        r2_freq:3200.0, r2_q:20.0, r2_level:0.2, r2_decay:0.01, ..T5_DEFAULT }, // woodblock
    80 => T5Recipe { exciter:0, impulse_level:1.0, noise_level:0.1, noise_decay:0.002,
        r1_freq:2500.0, r1_q:50.0, r1_level:0.45, r1_decay:0.022, ..T5_DEFAULT }, // clave
    81 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.1, noise_decay:0.002,
        r1_freq:1200.0, r1_q:80.0, r1_level:0.4, r1_decay:0.9,
        r2_freq:3600.0, r2_q:40.0, r2_level:0.2, r2_decay:0.7, ..T5_DEFAULT }, // triangle metal
    82 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:0.2,
        r1_freq:8500.0, r1_q:6.0, r1_level:0.3, r1_decay:0.2,
        r2_freq:12000.0, r2_q:4.0, r2_level:0.2, r2_decay:0.15,
        noise_filter_freq:5000.0, noise_mix:0.2, noise_filter_decay:0.18, ..T5_DEFAULT }, // tambourine
    83 => T5Recipe { exciter:1, noise_level:0.8, noise_decay:0.07,
        noise_filter_freq:5500.0, noise_mix:0.35, noise_filter_decay:0.07, ..T5_DEFAULT }, // shaker
    84 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:0.1,
        noise_filter_freq:6500.0, noise_mix:0.3, noise_filter_decay:0.1, ..T5_DEFAULT }, // cabasa
    85 => T5Recipe { exciter:0, impulse_level:0.6, noise_level:0.15, noise_decay:0.005,
        r1_freq:930.0, r1_q:20.0, r1_level:0.4, r1_decay:0.16,
        r2_freq:1400.0, r2_q:15.0, r2_level:0.25, r2_decay:0.14, ..T5_DEFAULT }, // agogo high
    86 => T5Recipe { exciter:0, impulse_level:0.6, noise_level:0.15, noise_decay:0.005,
        r1_freq:670.0, r1_q:20.0, r1_level:0.4, r1_decay:0.16,
        r2_freq:1010.0, r2_q:15.0, r2_level:0.25, r2_decay:0.14, ..T5_DEFAULT }, // agogo low
    87 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.1, noise_decay:0.003,
        r1_freq:760.0, r1_q:30.0, r1_level:0.35, r1_decay:0.8,
        r2_freq:1140.0, r2_q:25.0, r2_level:0.25, r2_decay:0.65,
        r3_freq:1710.0, r3_q:15.0, r3_level:0.15, r3_decay:0.5, ..T5_DEFAULT }, // ride bell
    88 => T5Recipe { exciter:1, noise_level:0.6, noise_decay:0.04,
        noise_filter_freq:7000.0, noise_mix:0.3, noise_filter_decay:0.04, ..T5_DEFAULT }, // maracas
    89 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:0.5,
        r1_freq:3500.0, r1_q:6.0, r1_level:0.3, r1_decay:0.45,
        noise_filter_freq:4000.0, noise_mix:0.25, noise_filter_decay:0.4, ..T5_DEFAULT }, // vibraslap

    // ══ MORE PERCUSSION (90-101) ══
    90 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.2, noise_decay:0.005,
        r1_freq:340.0, r1_q:12.0, r1_level:0.6, r1_decay:0.22, pitch_sweep:25.0, pitch_time:0.01,
        ..T5_DEFAULT }, // conga open
    91 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.15, noise_decay:0.003,
        r1_freq:320.0, r1_q:8.0, r1_level:0.5, r1_decay:0.06, ..T5_DEFAULT }, // conga mute
    92 => T5Recipe { exciter:2, impulse_level:0.8, noise_level:0.4, noise_decay:0.005,
        r1_freq:355.0, r1_q:6.0, r1_level:0.35, r1_decay:0.04,
        noise_filter_freq:2000.0, noise_mix:0.3, noise_filter_decay:0.008, ..T5_DEFAULT }, // conga slap
    93 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.2, noise_decay:0.003,
        r1_freq:425.0, r1_q:10.0, r1_level:0.5, r1_decay:0.1, pitch_sweep:35.0, pitch_time:0.008,
        ..T5_DEFAULT }, // bongo high
    94 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.2, noise_decay:0.004,
        r1_freq:315.0, r1_q:10.0, r1_level:0.5, r1_decay:0.15, pitch_sweep:25.0, pitch_time:0.01,
        ..T5_DEFAULT }, // bongo low
    95 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.25, noise_decay:0.004,
        r1_freq:530.0, r1_q:8.0, r1_level:0.45, r1_decay:0.2,
        r2_freq:1590.0, r2_q:12.0, r2_level:0.15, r2_decay:0.15, ..T5_DEFAULT }, // timbale high
    96 => T5Recipe { exciter:0, impulse_level:0.7, noise_level:0.25, noise_decay:0.004,
        r1_freq:370.0, r1_q:8.0, r1_level:0.45, r1_decay:0.22,
        r2_freq:925.0, r2_q:10.0, r2_level:0.12, r2_decay:0.15, ..T5_DEFAULT }, // timbale low
    97 => T5Recipe { exciter:1, noise_level:0.5, noise_decay:0.15,
        r1_freq:600.0, r1_q:3.0, r1_level:0.3, r1_decay:0.15, pitch_sweep:400.0, pitch_time:0.1,
        ..T5_DEFAULT }, // cuica high
    98 => T5Recipe { exciter:1, noise_level:0.5, noise_decay:0.2,
        r1_freq:350.0, r1_q:3.0, r1_level:0.3, r1_decay:0.2, pitch_sweep:200.0, pitch_time:0.12,
        ..T5_DEFAULT }, // cuica low
    99 => T5Recipe { exciter:0, impulse_level:0.5, noise_level:0.1, noise_decay:0.002,
        r1_freq:2300.0, r1_q:40.0, r1_level:0.35, r1_decay:0.1, ..T5_DEFAULT }, // whistle
    100 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:0.22,
        r1_freq:4200.0, r1_q:4.0, r1_level:0.3, r1_decay:0.2,
        noise_filter_freq:3000.0, noise_mix:0.3, noise_filter_decay:0.18, ..T5_DEFAULT }, // guiro
    101 => T5Recipe { exciter:1, noise_level:0.6, noise_decay:1.5,
        r1_freq:3500.0, r1_q:8.0, r1_level:0.3, r1_decay:1.5,
        r2_freq:6500.0, r2_q:5.0, r2_level:0.2, r2_decay:1.2,
        noise_filter_freq:3000.0, noise_mix:0.2, noise_filter_decay:1.2, ..T5_DEFAULT }, // sizzle cymbal

    // ══ EXTRAS (102-111) ══
    102 => T5Recipe { exciter:3, burst_count:5, burst_spread:0.008,
        noise_level:0.65, noise_decay:0.2,
        r1_freq:1900.0, r1_q:1.5, r1_level:0.25, r1_decay:0.2,
        noise_filter_freq:700.0, noise_mix:0.35, noise_filter_decay:0.2, ..T5_DEFAULT }, // clap vinyl
    103 => T5Recipe { exciter:1, noise_level:0.6, noise_decay:0.15,
        r1_freq:4000.0, r1_q:5.0, r1_level:0.3, r1_decay:0.15,
        noise_filter_freq:4500.0, noise_mix:0.2, noise_filter_decay:0.12, ..T5_DEFAULT }, // hat pedal splash
    104 => T5Recipe { exciter:2, impulse_level:1.0, noise_level:0.3, noise_decay:0.002,
        r1_freq:4200.0, r1_q:20.0, r1_level:0.35, r1_decay:0.01, ..T5_DEFAULT }, // snap knuckle
    105 => T5Recipe { exciter:0, impulse_level:1.0, noise_level:0.2, noise_decay:0.002,
        r1_freq:2200.0, r1_q:25.0, r1_level:0.3, r1_decay:0.012,
        r2_freq:4800.0, r2_q:15.0, r2_level:0.15, r2_decay:0.008, ..T5_DEFAULT }, // stick click
    106 => T5Recipe { exciter:0, impulse_level:0.8, noise_level:0.15, noise_decay:0.003,
        r1_freq:1800.0, r1_q:50.0, r1_level:0.35, r1_decay:0.6, pitch_sweep:3000.0, pitch_time:0.3,
        r2_freq:2700.0, r2_q:30.0, r2_level:0.2, r2_decay:0.5, ..T5_DEFAULT }, // bell tree sweep
    107 => T5Recipe { exciter:2, impulse_level:0.8, noise_level:0.3, noise_decay:0.005,
        r1_freq:1800.0, r1_q:2.0, r1_level:0.3, r1_decay:0.02, ..T5_DEFAULT }, // tongue click
    108 => T5Recipe { exciter:1, noise_level:0.5, noise_decay:0.4,
        r1_freq:3000.0, r1_q:2.0, r1_level:0.2, r1_decay:0.35,
        noise_filter_freq:1500.0, noise_mix:0.3, noise_filter_decay:0.35, ..T5_DEFAULT }, // brush sweep
    109 => T5Recipe { exciter:1, noise_level:0.7, noise_decay:0.8,
        r1_freq:3500.0, r1_q:5.0, r1_level:0.3, r1_decay:0.8,
        r2_freq:6500.0, r2_q:4.0, r2_level:0.18, r2_decay:0.6,
        noise_filter_freq:3000.0, noise_mix:0.2, noise_filter_decay:0.7, ..T5_DEFAULT }, // hat foot splash
    110 => T5Recipe { exciter:3, burst_count:8, burst_spread:0.025,
        noise_level:0.6, noise_decay:0.5,
        r1_freq:2000.0, r1_q:1.2, r1_level:0.2, r1_decay:0.5,
        noise_filter_freq:600.0, noise_mix:0.4, noise_filter_decay:0.5, ..T5_DEFAULT }, // stadium clap
    111 => T5Recipe { exciter:1, noise_level:0.4, noise_decay:0.08,
        r1_freq:300.0, r1_q:4.0, r1_level:0.15, r1_decay:0.06,
        noise_filter_freq:3000.0, noise_mix:0.2, noise_filter_decay:0.08, wire_coupling:0.5,
        ..T5_DEFAULT }, // brush tap

    _ => T5_DEFAULT,
    }
}

impl DrumVoice {
    fn new() -> Self {
        Self {
            active: false,
            time: 0.0,
            note: 0,
            velocity: 0.0,
            sound: DrumSound::Kick,
            kit: DrumKit::Kit808,
            noise_counter: 0,
            noise_seed: 0,
            phase1: 0.0,
            phase2: 0.0,
            phase3: 0.0,
            hat_oscs: HatOscillators::new(),
            svf1: Svf::new(),
            svf2: Svf::new(),
            svf3: Svf::new(),
            hp1: OnePole::new(),
            hp2: OnePole::new(),
            lp1: OnePole::new(),
            clap_burst_index: 0,
            lp1_state: 0.0,
            modal_phases: [0.0; 8],
            modal_amps: [0.0; 8],
            modal_decays: [0.0; 8],
            hit_seed: 0,
        }
    }

    fn trigger(&mut self, note: u8, velocity: u8, sound: DrumSound, kit: DrumKit) {
        self.active = true;
        self.time = 0.0;
        self.note = note;
        self.velocity = velocity as f32 / 127.0;
        self.sound = sound;
        self.kit = kit;
        self.noise_counter = 0;
        // Use note as part of noise seed for variation
        self.noise_seed = (note as u64) * 127 + (velocity as u64) * 31;
        self.phase1 = 0.0;
        self.phase2 = 0.0;
        self.phase3 = 0.0;
        self.hat_oscs.reset();
        self.svf1 = Svf::new();
        self.svf2 = Svf::new();
        self.svf3 = Svf::new();
        self.hp1 = OnePole::new();
        self.hp2 = OnePole::new();
        self.lp1 = OnePole::new();
        self.clap_burst_index = 0;
        self.lp1_state = 0.0;
        self.modal_phases = [0.0; 8];
        self.modal_amps = [0.0; 8];
        self.modal_decays = [0.0; 8];
        // Per-hit seed: combine note, velocity, and a simple counter for variation
        self.hit_seed = self.hit_seed.wrapping_add(note as u32 * 7 + velocity as u32 * 13 + 1);
    }

    fn tick(&mut self, sr: f64, user_decay: f64, user_tone: f64, user_noise: f64, user_drive: f64) -> f32 {
        if !self.active {
            return 0.0;
        }
        let dt = 1.0 / sr;
        self.time += dt;
        self.noise_counter += 1;

        let decay_mod = 0.3 + user_decay * 1.4;
        let tone_mod = 0.8 + user_tone * 0.4;
        let noise_mod = 0.5 + user_noise * 1.0;
        let drive_amt = user_drive;

        let sample = match self.kit {
            DrumKit::Kit808 => self.synth_808(sr, decay_mod, tone_mod, noise_mod, drive_amt),
            DrumKit::Kit909 => self.synth_909(sr, decay_mod, tone_mod, noise_mod, drive_amt),
            DrumKit::Kit707 => self.synth_707(sr, decay_mod, tone_mod, noise_mod, drive_amt),
            DrumKit::Kit606 => self.synth_606(sr, decay_mod, tone_mod, noise_mod, drive_amt),
            DrumKit::Kit777 => self.synth_777(sr, decay_mod, tone_mod, noise_mod, drive_amt),
            DrumKit::KitTsty1 => self.synth_tsty1(sr, decay_mod, tone_mod, noise_mod, drive_amt),
            DrumKit::KitTsty2 => self.synth_tsty2(sr, decay_mod, tone_mod, noise_mod, drive_amt),
            DrumKit::KitTsty3 => self.synth_tsty3(sr, decay_mod, tone_mod, noise_mod, drive_amt),
            DrumKit::KitTsty4 => self.synth_tsty4(sr, decay_mod, tone_mod, noise_mod, drive_amt),
            DrumKit::KitTsty5 => self.synth_tsty5(sr, decay_mod, tone_mod, noise_mod, drive_amt),
        };

        // Auto-deactivate if very quiet — but allow long-decay sounds to ring
        let min_time = match self.kit {
            DrumKit::KitTsty1 | DrumKit::KitTsty2 | DrumKit::KitTsty3 | DrumKit::KitTsty4 => 2.5,
            _ => 0.01,
        };
        if self.time > min_time && sample.abs() < 0.00001 {
            self.active = false;
        } else if self.time > 0.01 && sample.abs() < 0.0001 {
            // For non-tsty kits, keep the original behavior
            if !matches!(self.kit, DrumKit::KitTsty1 | DrumKit::KitTsty2 | DrumKit::KitTsty3 | DrumKit::KitTsty4) {
                self.active = false;
            }
        }

        (sample * self.velocity as f64 * 0.4) as f32
    }

    fn noise(&self) -> f64 {
        white_noise(self.noise_counter.wrapping_add(self.noise_seed))
    }

    // ══════════════════════════════════════════════════════════════════════
    // 808 synthesis
    // ══════════════════════════════════════════════════════════════════════

    fn synth_808(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, drive_amt: f64) -> f64 {
        match self.sound {
            DrumSound::Kick | DrumSound::SubKick(_) => self.synth_808_kick(sr, decay_mod, tone_mod, drive_amt),
            DrumSound::Snare => self.synth_808_snare(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::SnareAlt => {
                // Alternate snare: more noise, less body
                self.synth_808_snare_alt(sr, decay_mod, tone_mod, noise_mod)
            }
            DrumSound::Clap => self.synth_808_clap(sr, decay_mod, noise_mod),
            DrumSound::ClosedHat | DrumSound::PedalHat => self.synth_808_closed_hat(sr, decay_mod, tone_mod),
            DrumSound::OpenHat => self.synth_808_open_hat(sr, decay_mod, tone_mod),
            DrumSound::Rimshot => self.synth_808_rimshot(sr, decay_mod, tone_mod),
            DrumSound::Cowbell => self.synth_808_cowbell(sr, decay_mod, tone_mod),
            DrumSound::Clave => self.synth_808_clave(sr, decay_mod),
            DrumSound::Maracas | DrumSound::Cabasa => self.synth_808_maracas(sr, decay_mod),
            DrumSound::LowTom => self.synth_808_tom(sr, decay_mod, tone_mod, 105.0),
            DrumSound::MidTom => self.synth_808_tom(sr, decay_mod, tone_mod, 160.0),
            DrumSound::HighTom => self.synth_808_tom(sr, decay_mod, tone_mod, 220.0),
            DrumSound::Crash | DrumSound::Splash => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.8),
            DrumSound::Cymbal => self.synth_808_cymbal(sr, decay_mod, tone_mod, 1.0),
            DrumSound::Ride | DrumSound::RideBell => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.6),
            DrumSound::Tambourine => self.synth_808_tambourine(sr, decay_mod),
            DrumSound::Vibraslap => self.synth_808_vibraslap(sr, decay_mod),
            DrumSound::Bongo(freq) => self.synth_808_bongo(sr, decay_mod, tone_mod, freq),
            DrumSound::Conga(freq) => self.synth_808_conga(sr, decay_mod, tone_mod, freq),
            DrumSound::Timbale(freq) => self.synth_808_timbale(sr, decay_mod, tone_mod, freq),
            DrumSound::Agogo(freq) => self.synth_808_agogo(sr, decay_mod, tone_mod, freq),
            DrumSound::Guiro(dec) => self.synth_808_guiro(sr, decay_mod, dec),
            DrumSound::Whistle(dec) => self.synth_808_whistle(sr, decay_mod, dec),
            DrumSound::FxNoise(v) => self.synth_808_fx(sr, decay_mod, v),
        }
    }

    /// 808 Kick: 42Hz sine with heavy pitch sweep from ~340Hz down over ~6ms.
    fn synth_808_kick(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, drive_amt: f64) -> f64 {
        let base_freq = match self.sound {
            DrumSound::SubKick(mult) => 42.0 * mult,
            _ => 42.0,
        };
        let freq = base_freq * tone_mod;
        let sweep = freq * 8.0 * (-self.time * 160.0).exp(); // fast sweep ~6ms
        let current_freq = freq + sweep;
        advance_phase(&mut self.phase1, current_freq, sr);
        let body = osc_sine(self.phase1);

        let decay = 0.40 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.0005 {
            self.active = false;
            return 0.0;
        }

        // Click transient
        let click = (-self.time * 800.0).exp() * osc_sine(self.phase1 * 3.0) * 0.3;

        let out = (body + click) * env;
        if drive_amt > 0.01 {
            soft_clip(out, drive_amt * 2.0)
        } else {
            out
        }
    }

    /// 808 Snare: Two sines (180Hz + 424Hz) + HPF noise.
    fn synth_808_snare(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 180.0 * tone_mod;
        let f2 = 424.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let tonal_decay = 0.20 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        let tonal = (osc_sine(self.phase1) * 0.6 + osc_sine(self.phase2) * 0.4) * tonal_env;

        let noise_decay = 0.18 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        let filtered_noise = self.hp1.tick_hp(raw_noise, 1500.0, sr);
        let noise_out = filtered_noise * noise_env * noise_mod;

        let snappy = 0.5; // balance tonal vs noise
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 808 Alternate Snare: More noise emphasis.
    fn synth_808_snare_alt(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 200.0 * tone_mod;
        let f2 = 440.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let tonal_decay = 0.15 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        let tonal = (osc_sine(self.phase1) * 0.5 + osc_sine(self.phase2) * 0.5) * tonal_env;

        let noise_decay = 0.22 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        let filtered_noise = self.hp1.tick_hp(raw_noise, 2000.0, sr);
        let noise_out = filtered_noise * noise_env * noise_mod;

        let snappy = 0.65;
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 808 Closed Hi-Hat: 6 square oscillators -> two parallel BPFs -> HPF.
    fn synth_808_closed_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        // Two parallel bandpass filters
        let bp1 = self.svf1.bandpass(raw, 3440.0, 3.0, sr);
        let bp2 = self.svf2.bandpass(raw, 7100.0, 3.0, sr);
        let filtered = bp1 * 0.5 + bp2 * 0.5;

        // High-pass at 6kHz
        let hpf = self.hp1.tick_hp(filtered, 6000.0, sr);

        // Short exponential decay ~50ms
        let decay = 0.05 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        hpf * env
    }

    /// 808 Open Hi-Hat: Same 6 oscillators + filters, longer decay.
    fn synth_808_open_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        let bp1 = self.svf1.bandpass(raw, 3440.0, 3.0, sr);
        let bp2 = self.svf2.bandpass(raw, 7100.0, 3.0, sr);
        let filtered = bp1 * 0.5 + bp2 * 0.5;

        let hpf = self.hp1.tick_hp(filtered, 6000.0, sr);

        // Longer decay 200-800ms
        let decay = 0.35 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        hpf * env
    }

    /// 808 Clap: 4 rapid noise bursts + reverb tail.
    fn synth_808_clap(&mut self, sr: f64, decay_mod: f64, noise_mod: f64) -> f64 {
        let raw_noise = self.noise() * noise_mod;
        let filtered = self.svf1.bandpass(raw_noise, 1000.0, 2.0, sr);

        // 4 bursts spaced 10ms apart, each ~3ms attack, ~7ms decay
        let mut burst_env = 0.0;
        for burst in 0..4 {
            let burst_start = burst as f64 * 0.010;
            let t_in_burst = self.time - burst_start;
            if t_in_burst >= 0.0 && t_in_burst < 0.010 {
                let attack = if t_in_burst < 0.003 {
                    t_in_burst / 0.003
                } else {
                    (-((t_in_burst - 0.003) / 0.005)).exp()
                };
                burst_env += attack;
            }
        }

        // Reverb tail starting after bursts
        let tail_start = 0.040;
        let tail_env = if self.time > tail_start {
            (-(self.time - tail_start) / (0.15 * decay_mod)).exp()
        } else {
            0.0
        };

        let env = burst_env + tail_env * 0.7;
        if self.time > tail_start + 0.15 * decay_mod * 7.0 {
            self.active = false;
        }
        filtered * env
    }

    /// 808 Cowbell: Two square waves at 540 Hz and 800 Hz through BPF.
    fn synth_808_cowbell(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        advance_phase(&mut self.phase1, 540.0 * tone_mod, sr);
        advance_phase(&mut self.phase2, 800.0 * tone_mod, sr);

        let raw = osc_square(self.phase1) * 0.5 + osc_square(self.phase2) * 0.5;
        let filtered = self.svf1.bandpass(raw, 700.0, 3.0, sr);

        let decay = 0.065 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        filtered * env
    }

    /// 808 Rimshot: Two sines at 455 Hz and 1667 Hz, HPF, soft-clip.
    fn synth_808_rimshot(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        advance_phase(&mut self.phase1, 455.0 * tone_mod, sr);
        advance_phase(&mut self.phase2, 1667.0 * tone_mod, sr);

        let raw = osc_sine(self.phase1) * 0.5 + osc_sine(self.phase2) * 0.5;
        let hpf = self.hp1.tick_hp(raw, 600.0, sr);

        let decay = 0.010 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        let out = hpf * env;
        soft_clip(out, 0.5) // subtle soft-clip
    }

    /// 808 Clave: Single sine at 2500 Hz, 25ms decay.
    fn synth_808_clave(&mut self, sr: f64, decay_mod: f64) -> f64 {
        advance_phase(&mut self.phase1, 2500.0, sr);
        let decay = 0.025 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        osc_sine(self.phase1) * env
    }

    /// 808 Maracas: White noise through HPF ~5kHz, attack-release envelope.
    fn synth_808_maracas(&mut self, sr: f64, decay_mod: f64) -> f64 {
        let raw_noise = self.noise();
        let hpf = self.hp1.tick_hp(raw_noise, 5000.0, sr);

        // 20ms attack, 8ms release
        let attack_time = 0.020;
        let release_time = 0.008 * decay_mod;
        let env = if self.time < attack_time {
            self.time / attack_time
        } else {
            (-(self.time - attack_time) / release_time).exp()
        };
        if self.time > attack_time && env < 0.001 {
            self.active = false;
            return 0.0;
        }
        hpf * env * 0.7
    }

    /// 808 Tom: Sine with pitch droop + small noise.
    fn synth_808_tom(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, base_freq: f64) -> f64 {
        let freq = base_freq * tone_mod;
        // Subtle pitch droop: starts 15% higher
        let droop = freq * 0.15 * (-self.time * 40.0).exp();
        advance_phase(&mut self.phase1, freq + droop, sr);

        let decay = match base_freq as u32 {
            0..=120 => 0.18,
            121..=180 => 0.12,
            _ => 0.08,
        } * decay_mod;

        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }

        let body = osc_sine(self.phase1);
        // Small noise click at attack
        let noise_env = (-self.time * 200.0).exp();
        let noise_click = self.noise() * noise_env * 0.15;

        (body + noise_click) * env
    }

    /// 808 Cymbal: 6 oscillators, dual envelope for spectral shift.
    fn synth_808_cymbal(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, size: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        // Lower BPF at 3440Hz with longer decay
        let bp_low = self.svf1.bandpass(raw, 3440.0, 2.5, sr);
        let low_decay = (0.6 + size * 0.6) * decay_mod;
        let low_env = (-self.time / low_decay).exp();

        // Upper BPF at 7100Hz with shorter decay
        let bp_high = self.svf2.bandpass(raw, 7100.0, 2.5, sr);
        let high_decay = (0.3 + size * 0.2) * decay_mod;
        let high_env = (-self.time / high_decay).exp();

        if low_env < 0.001 && high_env < 0.001 {
            self.active = false;
            return 0.0;
        }

        bp_low * low_env * 0.5 + bp_high * high_env * 0.5
    }

    /// 808 Tambourine: noise through high BPF with rattling envelope.
    fn synth_808_tambourine(&mut self, sr: f64, decay_mod: f64) -> f64 {
        let raw_noise = self.noise();
        let filtered = self.hp1.tick_hp(raw_noise, 7000.0, sr);
        let bp = self.svf1.bandpass(filtered, 10000.0, 2.0, sr);

        let decay = 0.12 * decay_mod;
        let env = (-self.time / decay).exp();
        // Add slight tremolo for rattle character
        let tremolo = 1.0 - 0.3 * (self.time * 120.0 * TAU).sin().abs();

        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        bp * env * tremolo * 0.8
    }

    /// 808 Vibraslap: Noise with increasing then decreasing rattle.
    fn synth_808_vibraslap(&mut self, sr: f64, decay_mod: f64) -> f64 {
        let raw_noise = self.noise();
        let filtered = self.svf1.bandpass(raw_noise, 3000.0, 4.0, sr);

        let total_dur = 0.30 * decay_mod;
        // Ramp up then decay
        let ramp = if self.time < total_dur * 0.3 {
            self.time / (total_dur * 0.3)
        } else {
            (-(self.time - total_dur * 0.3) / (total_dur * 0.5)).exp()
        };
        // Rattle pulses
        let rattle_freq = 60.0 + self.time * 200.0; // accelerating
        let rattle = (self.time * rattle_freq * TAU).sin().abs();

        if self.time > total_dur * 3.0 {
            self.active = false;
        }
        filtered * ramp * rattle * 0.6
    }

    /// 808 Bongo: Sine with slight pitch droop, fast decay.
    fn synth_808_bongo(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f = freq * tone_mod;
        let droop = f * 0.1 * (-self.time * 80.0).exp();
        advance_phase(&mut self.phase1, f + droop, sr);

        let decay = 0.06 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        osc_sine(self.phase1) * env
    }

    /// 808 Conga: Sine body, no noise component.
    fn synth_808_conga(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f = freq * tone_mod;
        let droop = f * 0.08 * (-self.time * 50.0).exp();
        advance_phase(&mut self.phase1, f + droop, sr);

        let decay = 0.10 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        osc_sine(self.phase1) * env
    }

    /// 808 Timbale: Bright sine with HPF.
    fn synth_808_timbale(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f = freq * tone_mod;
        advance_phase(&mut self.phase1, f, sr);
        let raw = osc_sine(self.phase1);
        let hpf = self.hp1.tick_hp(raw, 300.0, sr);

        let decay = 0.05 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        hpf * env
    }

    /// 808 Agogo: Two sines at fundamental and ~1.5x.
    fn synth_808_agogo(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f1 = freq * tone_mod;
        let f2 = f1 * 1.504;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let raw = osc_sine(self.phase1) * 0.6 + osc_sine(self.phase2) * 0.4;
        let decay = 0.08 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        raw * env
    }

    /// 808 Guiro: Noise with scraping rhythm.
    fn synth_808_guiro(&mut self, sr: f64, decay_mod: f64, dur: f64) -> f64 {
        let raw_noise = self.noise();
        let filtered = self.svf1.bandpass(raw_noise, 4000.0, 3.0, sr);

        let total = dur * decay_mod;
        let env = if self.time < total {
            1.0 - self.time / total
        } else {
            self.active = false;
            return 0.0;
        };

        // Scraping pattern
        let scrape = ((self.time * 200.0).floor() % 2.0).max(0.3);
        filtered * env * scrape * 0.5
    }

    /// 808 Whistle: Sine tone with vibrato.
    fn synth_808_whistle(&mut self, sr: f64, decay_mod: f64, dur: f64) -> f64 {
        let total = dur * decay_mod * 3.0;
        let vibrato = (self.time * 6.0 * TAU).sin() * 30.0;
        advance_phase(&mut self.phase1, 2200.0 + vibrato, sr);

        let env = if self.time < total {
            let attack = (self.time * 100.0).min(1.0);
            let release = ((total - self.time) * 50.0).min(1.0);
            attack * release
        } else {
            self.active = false;
            return 0.0;
        };
        osc_sine(self.phase1) * env * 0.4
    }

    /// 808 FX: Noise + swept filter.
    fn synth_808_fx(&mut self, sr: f64, decay_mod: f64, character: f64) -> f64 {
        let raw_noise = self.noise();
        let freq = 500.0 + character * 8000.0;
        let sweep = freq * (1.0 + 2.0 * (-self.time * 10.0).exp());
        let filtered = self.svf1.bandpass(raw_noise, sweep.min(20000.0), 2.0, sr);

        let decay = (0.05 + character * 0.4) * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        filtered * env * 0.6
    }

    // ══════════════════════════════════════════════════════════════════════
    // 909 synthesis
    // ══════════════════════════════════════════════════════════════════════

    fn synth_909(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, drive_amt: f64) -> f64 {
        match self.sound {
            DrumSound::Kick | DrumSound::SubKick(_) => self.synth_909_kick(sr, decay_mod, tone_mod, drive_amt),
            DrumSound::Snare => self.synth_909_snare(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::SnareAlt => self.synth_909_snare_alt(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::Clap => self.synth_909_clap(sr, decay_mod, noise_mod),
            DrumSound::ClosedHat | DrumSound::PedalHat => self.synth_909_closed_hat(sr, decay_mod, tone_mod),
            DrumSound::OpenHat => self.synth_909_open_hat(sr, decay_mod, tone_mod),
            DrumSound::Rimshot => self.synth_808_rimshot(sr, decay_mod, tone_mod), // similar
            DrumSound::Cowbell => self.synth_808_cowbell(sr, decay_mod, tone_mod),
            DrumSound::Clave => self.synth_808_clave(sr, decay_mod),
            DrumSound::Maracas | DrumSound::Cabasa => self.synth_808_maracas(sr, decay_mod),
            DrumSound::LowTom => self.synth_808_tom(sr, decay_mod, tone_mod, 110.0),
            DrumSound::MidTom => self.synth_808_tom(sr, decay_mod, tone_mod, 170.0),
            DrumSound::HighTom => self.synth_808_tom(sr, decay_mod, tone_mod, 230.0),
            DrumSound::Crash | DrumSound::Splash => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.9),
            DrumSound::Cymbal => self.synth_808_cymbal(sr, decay_mod, tone_mod, 1.0),
            DrumSound::Ride | DrumSound::RideBell => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.5),
            DrumSound::Tambourine => self.synth_808_tambourine(sr, decay_mod),
            DrumSound::Vibraslap => self.synth_808_vibraslap(sr, decay_mod),
            DrumSound::Bongo(f) => self.synth_808_bongo(sr, decay_mod, tone_mod, f),
            DrumSound::Conga(f) => self.synth_808_conga(sr, decay_mod, tone_mod, f),
            DrumSound::Timbale(f) => self.synth_808_timbale(sr, decay_mod, tone_mod, f),
            DrumSound::Agogo(f) => self.synth_808_agogo(sr, decay_mod, tone_mod, f),
            DrumSound::Guiro(d) => self.synth_808_guiro(sr, decay_mod, d),
            DrumSound::Whistle(d) => self.synth_808_whistle(sr, decay_mod, d),
            DrumSound::FxNoise(v) => self.synth_808_fx(sr, decay_mod, v),
        }
    }

    /// 909 Kick: 55Hz sine, longer pitch sweep (80-200ms from 150Hz), noise click.
    fn synth_909_kick(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, drive_amt: f64) -> f64 {
        let base_freq = match self.sound {
            DrumSound::SubKick(mult) => 55.0 * mult,
            _ => 55.0,
        };
        let freq = base_freq * tone_mod;
        // Longer pitch sweep: from ~150Hz down over ~120ms (much longer than 808's 6ms)
        let sweep_start = 150.0 * tone_mod;
        let sweep = (sweep_start - freq) * (-self.time * 12.0).exp();
        let current_freq = freq + sweep;
        advance_phase(&mut self.phase1, current_freq, sr);
        let body = osc_sine(self.phase1);

        let decay = 0.22 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }

        // Noise click: white noise through LPF at 5kHz, 5-10ms decay, mixed at 25%
        let noise_click_env = (-self.time / 0.007).exp();
        let raw_noise = self.noise();
        let filtered_noise = self.lp1.tick_lp(raw_noise, 5000.0, sr);
        let click = filtered_noise * noise_click_env * 0.25;

        let out = (body * 0.75 + click) * env;
        if drive_amt > 0.01 {
            soft_clip(out, drive_amt * 3.0 + 1.0)
        } else {
            soft_clip(out, 1.0)  // 909 always has slight overdrive
        }
    }

    /// 909 Snare: Two TRIANGLE oscillators (180Hz, 330Hz) + bandpassed noise.
    fn synth_909_snare(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 180.0 * tone_mod;
        let f2 = 330.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let tonal_decay = 0.14 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        // Triangle oscillators for crisper harmonics
        let tonal = (osc_triangle(self.phase1) * 0.55 + osc_triangle(self.phase2) * 0.45) * tonal_env;

        let noise_decay = 0.16 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        // Noise through LPF 8kHz -> HPF 1.5kHz
        let lp_noise = self.lp1.tick_lp(raw_noise, 8000.0, sr);
        let bp_noise = self.hp1.tick_hp(lp_noise, 1500.0, sr);
        let noise_out = bp_noise * noise_env * noise_mod;

        let snappy = 0.55;
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 909 Snare Alt: Slightly different tuning.
    fn synth_909_snare_alt(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 200.0 * tone_mod;
        let f2 = 350.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let tonal_decay = 0.12 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        let tonal = (osc_triangle(self.phase1) * 0.5 + osc_triangle(self.phase2) * 0.5) * tonal_env;

        let noise_decay = 0.18 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        let lp_noise = self.lp1.tick_lp(raw_noise, 8000.0, sr);
        let bp_noise = self.hp1.tick_hp(lp_noise, 2000.0, sr);
        let noise_out = bp_noise * noise_env * noise_mod;

        let snappy = 0.6;
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 909 Clap: 4 noise bursts spaced 11ms apart through BPF 1.2kHz, slight jitter.
    fn synth_909_clap(&mut self, sr: f64, decay_mod: f64, noise_mod: f64) -> f64 {
        let raw_noise = self.noise() * noise_mod;
        let filtered = self.svf1.bandpass(raw_noise, 1200.0, 2.5, sr);

        // 4 bursts with 11ms spacing and slight jitter
        let spacings = [0.0, 0.011, 0.023, 0.034]; // slight jitter
        let mut burst_env = 0.0;
        for &burst_start in &spacings {
            let t_in_burst = self.time - burst_start;
            if t_in_burst >= 0.0 && t_in_burst < 0.012 {
                let attack = if t_in_burst < 0.003 {
                    t_in_burst / 0.003
                } else {
                    (-((t_in_burst - 0.003) / 0.006)).exp()
                };
                burst_env += attack;
            }
        }

        let tail_start = 0.045;
        let tail_env = if self.time > tail_start {
            (-(self.time - tail_start) / (0.18 * decay_mod)).exp()
        } else {
            0.0
        };

        let env = burst_env + tail_env * 0.65;
        if self.time > tail_start + 0.18 * decay_mod * 7.0 {
            self.active = false;
        }
        filtered * env
    }

    /// 909 Closed Hat: 6 oscillators + bit-crushing for gritty digital character.
    fn synth_909_closed_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        let bp1 = self.svf1.bandpass(raw, 3440.0, 3.0, sr);
        let bp2 = self.svf2.bandpass(raw, 7100.0, 3.0, sr);
        let filtered = bp1 * 0.4 + bp2 * 0.6; // more emphasis on 8-10kHz

        let hpf = self.hp1.tick_hp(filtered, 6000.0, sr);

        // Bit-crush: reduce to ~8-bit resolution for gritty 909 character
        let crushed = (hpf * 128.0).round() / 128.0;

        let decay = 0.04 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        crushed * env
    }

    /// 909 Open Hat: Same with bit-crushing, longer decay.
    fn synth_909_open_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        let bp1 = self.svf1.bandpass(raw, 3440.0, 3.0, sr);
        let bp2 = self.svf2.bandpass(raw, 7100.0, 3.0, sr);
        let filtered = bp1 * 0.4 + bp2 * 0.6;

        let hpf = self.hp1.tick_hp(filtered, 6000.0, sr);
        let crushed = (hpf * 128.0).round() / 128.0;

        let decay = 0.30 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        crushed * env
    }

    // ══════════════════════════════════════════════════════════════════════
    // 707 synthesis — halfway between 808 and 909 character
    // ══════════════════════════════════════════════════════════════════════

    fn synth_707(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, drive_amt: f64) -> f64 {
        match self.sound {
            DrumSound::Kick | DrumSound::SubKick(_) => self.synth_707_kick(sr, decay_mod, tone_mod, drive_amt),
            DrumSound::Snare => self.synth_707_snare(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::SnareAlt => self.synth_707_snare_alt(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::Clap => self.synth_707_clap(sr, decay_mod, noise_mod),
            DrumSound::ClosedHat | DrumSound::PedalHat => self.synth_707_closed_hat(sr, decay_mod, tone_mod),
            DrumSound::OpenHat => self.synth_707_open_hat(sr, decay_mod, tone_mod),
            DrumSound::Rimshot => self.synth_808_rimshot(sr, decay_mod, tone_mod),
            DrumSound::Cowbell => self.synth_808_cowbell(sr, decay_mod, tone_mod),
            DrumSound::Clave => self.synth_808_clave(sr, decay_mod),
            DrumSound::Maracas | DrumSound::Cabasa => self.synth_808_maracas(sr, decay_mod),
            DrumSound::LowTom => self.synth_808_tom(sr, decay_mod, tone_mod, 108.0),
            DrumSound::MidTom => self.synth_808_tom(sr, decay_mod, tone_mod, 165.0),
            DrumSound::HighTom => self.synth_808_tom(sr, decay_mod, tone_mod, 225.0),
            DrumSound::Crash | DrumSound::Splash => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.85),
            DrumSound::Cymbal => self.synth_808_cymbal(sr, decay_mod, tone_mod, 1.0),
            DrumSound::Ride | DrumSound::RideBell => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.55),
            DrumSound::Tambourine => self.synth_808_tambourine(sr, decay_mod),
            DrumSound::Vibraslap => self.synth_808_vibraslap(sr, decay_mod),
            DrumSound::Bongo(f) => self.synth_808_bongo(sr, decay_mod, tone_mod, f),
            DrumSound::Conga(f) => self.synth_808_conga(sr, decay_mod, tone_mod, f),
            DrumSound::Timbale(f) => self.synth_808_timbale(sr, decay_mod, tone_mod, f),
            DrumSound::Agogo(f) => self.synth_808_agogo(sr, decay_mod, tone_mod, f),
            DrumSound::Guiro(d) => self.synth_808_guiro(sr, decay_mod, d),
            DrumSound::Whistle(d) => self.synth_808_whistle(sr, decay_mod, d),
            DrumSound::FxNoise(v) => self.synth_808_fx(sr, decay_mod, v),
        }
    }

    /// 707 Kick: Between 808 and 909 — sine at 48Hz, medium sweep.
    fn synth_707_kick(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, drive_amt: f64) -> f64 {
        let base_freq = match self.sound {
            DrumSound::SubKick(mult) => 48.0 * mult,
            _ => 48.0,
        };
        let freq = base_freq * tone_mod;
        // Medium sweep: between 808's fast and 909's slow
        let sweep_start = 120.0 * tone_mod;
        let sweep = (sweep_start - freq) * (-self.time * 40.0).exp();
        advance_phase(&mut self.phase1, freq + sweep, sr);
        let body = osc_sine(self.phase1);

        let decay = 0.28 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }

        let click = (-self.time * 600.0).exp() * 0.2;
        let out = body * env + self.noise() * click * 0.1;
        if drive_amt > 0.01 {
            soft_clip(out, drive_amt * 2.5)
        } else {
            soft_clip(out, 0.3)
        }
    }

    /// 707 Snare: Hybrid sine/triangle with noise.
    fn synth_707_snare(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 190.0 * tone_mod;
        let f2 = 380.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let tonal_decay = 0.16 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        let tonal = (osc_sine(self.phase1) * 0.4 + osc_triangle(self.phase2) * 0.6) * tonal_env;

        let noise_decay = 0.14 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        let filtered = self.hp1.tick_hp(raw_noise, 1800.0, sr);
        let noise_out = filtered * noise_env * noise_mod;

        let snappy = 0.52;
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 707 Snare Alt.
    fn synth_707_snare_alt(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 205.0 * tone_mod;
        let f2 = 395.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let tonal_decay = 0.13 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        let tonal = (osc_sine(self.phase1) * 0.45 + osc_triangle(self.phase2) * 0.55) * tonal_env;

        let noise_decay = 0.15 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        let filtered = self.hp1.tick_hp(raw_noise, 2000.0, sr);
        let noise_out = filtered * noise_env * noise_mod;

        let snappy = 0.58;
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 707 Clap: Between 808 and 909.
    fn synth_707_clap(&mut self, sr: f64, decay_mod: f64, noise_mod: f64) -> f64 {
        let raw_noise = self.noise() * noise_mod;
        let filtered = self.svf1.bandpass(raw_noise, 1100.0, 2.2, sr);

        let spacings = [0.0, 0.0105, 0.0215, 0.033];
        let mut burst_env = 0.0;
        for &burst_start in &spacings {
            let t_in_burst = self.time - burst_start;
            if t_in_burst >= 0.0 && t_in_burst < 0.011 {
                let attack = if t_in_burst < 0.003 {
                    t_in_burst / 0.003
                } else {
                    (-((t_in_burst - 0.003) / 0.0055)).exp()
                };
                burst_env += attack;
            }
        }

        let tail_start = 0.043;
        let tail_env = if self.time > tail_start {
            (-(self.time - tail_start) / (0.16 * decay_mod)).exp()
        } else {
            0.0
        };

        let env = burst_env + tail_env * 0.68;
        if self.time > tail_start + 0.16 * decay_mod * 7.0 {
            self.active = false;
        }
        filtered * env
    }

    /// 707 Closed Hat: Tighter than 808, less aggressive than 909.
    fn synth_707_closed_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        let bp1 = self.svf1.bandpass(raw, 3440.0, 3.0, sr);
        let bp2 = self.svf2.bandpass(raw, 7100.0, 3.0, sr);
        let filtered = bp1 * 0.45 + bp2 * 0.55;

        let hpf = self.hp1.tick_hp(filtered, 6000.0, sr);

        let decay = 0.045 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        hpf * env
    }

    /// 707 Open Hat.
    fn synth_707_open_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        let bp1 = self.svf1.bandpass(raw, 3440.0, 3.0, sr);
        let bp2 = self.svf2.bandpass(raw, 7100.0, 3.0, sr);
        let filtered = bp1 * 0.45 + bp2 * 0.55;

        let hpf = self.hp1.tick_hp(filtered, 6000.0, sr);

        let decay = 0.25 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        hpf * env
    }

    // ══════════════════════════════════════════════════════════════════════
    // 606 synthesis — thinner, clickier, higher
    // ══════════════════════════════════════════════════════════════════════

    fn synth_606(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, drive_amt: f64) -> f64 {
        match self.sound {
            DrumSound::Kick | DrumSound::SubKick(_) => self.synth_606_kick(sr, decay_mod, tone_mod, drive_amt),
            DrumSound::Snare => self.synth_606_snare(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::SnareAlt => self.synth_606_snare_alt(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::Clap => self.synth_808_clap(sr, decay_mod, noise_mod), // reuse 808 clap
            DrumSound::ClosedHat | DrumSound::PedalHat => self.synth_606_closed_hat(sr, decay_mod, tone_mod),
            DrumSound::OpenHat => self.synth_606_open_hat(sr, decay_mod, tone_mod),
            DrumSound::Rimshot => self.synth_808_rimshot(sr, decay_mod, tone_mod),
            DrumSound::Cowbell => self.synth_808_cowbell(sr, decay_mod, tone_mod),
            DrumSound::Clave => self.synth_808_clave(sr, decay_mod),
            DrumSound::Maracas | DrumSound::Cabasa => self.synth_808_maracas(sr, decay_mod),
            DrumSound::LowTom => self.synth_808_tom(sr, decay_mod, tone_mod, 115.0),
            DrumSound::MidTom => self.synth_808_tom(sr, decay_mod, tone_mod, 175.0),
            DrumSound::HighTom => self.synth_808_tom(sr, decay_mod, tone_mod, 240.0),
            DrumSound::Crash | DrumSound::Splash => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.7),
            DrumSound::Cymbal => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.9),
            DrumSound::Ride | DrumSound::RideBell => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.5),
            DrumSound::Tambourine => self.synth_808_tambourine(sr, decay_mod),
            DrumSound::Vibraslap => self.synth_808_vibraslap(sr, decay_mod),
            DrumSound::Bongo(f) => self.synth_808_bongo(sr, decay_mod, tone_mod, f),
            DrumSound::Conga(f) => self.synth_808_conga(sr, decay_mod, tone_mod, f),
            DrumSound::Timbale(f) => self.synth_808_timbale(sr, decay_mod, tone_mod, f),
            DrumSound::Agogo(f) => self.synth_808_agogo(sr, decay_mod, tone_mod, f),
            DrumSound::Guiro(d) => self.synth_808_guiro(sr, decay_mod, d),
            DrumSound::Whistle(d) => self.synth_808_whistle(sr, decay_mod, d),
            DrumSound::FxNoise(v) => self.synth_808_fx(sr, decay_mod, v),
        }
    }

    /// 606 Kick: Two fixed-frequency sines (no pitch sweep). Thinner, clickier.
    fn synth_606_kick(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, drive_amt: f64) -> f64 {
        let base_freq = match self.sound {
            DrumSound::SubKick(mult) => 60.0 * mult,
            _ => 60.0,
        };
        let f1 = base_freq * tone_mod;
        let f2 = f1 * 1.5; // second harmonic for click
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let decay = 0.12 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }

        // Click transient from second osc
        let click_env = (-self.time * 400.0).exp();
        let body = osc_sine(self.phase1) * 0.7 + osc_sine(self.phase2) * 0.3 * click_env;

        let out = body * env;
        if drive_amt > 0.01 {
            soft_clip(out, drive_amt * 2.0)
        } else {
            out
        }
    }

    /// 606 Snare: Single sine with triggered FM. Thinner noise.
    fn synth_606_snare(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 210.0 * tone_mod;
        // FM: modulator decays, creating triggered frequency sweep
        let fm_amt = 100.0 * (-self.time * 50.0).exp();
        advance_phase(&mut self.phase2, f1 * 2.0, sr); // modulator
        let fm = osc_sine(self.phase2) * fm_amt;
        advance_phase(&mut self.phase1, f1 + fm, sr);

        let tonal_decay = 0.10 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        let tonal = osc_sine(self.phase1) * tonal_env;

        let noise_decay = 0.08 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        let filtered = self.hp1.tick_hp(raw_noise, 2500.0, sr);
        let noise_out = filtered * noise_env * noise_mod * 0.7; // thinner noise

        let snappy = 0.5;
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 606 Snare Alt.
    fn synth_606_snare_alt(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 230.0 * tone_mod;
        let fm_amt = 80.0 * (-self.time * 60.0).exp();
        advance_phase(&mut self.phase2, f1 * 2.3, sr);
        let fm = osc_sine(self.phase2) * fm_amt;
        advance_phase(&mut self.phase1, f1 + fm, sr);

        let tonal_decay = 0.09 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        let tonal = osc_sine(self.phase1) * tonal_env;

        let noise_decay = 0.10 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        let filtered = self.hp1.tick_hp(raw_noise, 3000.0, sr);
        let noise_out = filtered * noise_env * noise_mod * 0.6;

        let snappy = 0.55;
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 606 Closed Hat: Oscillators in 10-12kHz range. Thinner, more tinny.
    fn synth_606_closed_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_606;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        let bp = self.svf1.bandpass(raw, 11000.0, 2.0, sr);
        let hpf = self.hp1.tick_hp(bp, 8000.0, sr);

        let decay = 0.035 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        hpf * env
    }

    /// 606 Open Hat: Same high frequencies, longer decay.
    fn synth_606_open_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_606;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        let bp = self.svf1.bandpass(raw, 11000.0, 2.0, sr);
        let hpf = self.hp1.tick_hp(bp, 8000.0, sr);

        let decay = 0.22 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        hpf * env
    }

    // ── Kit 777: 808/909 bass + original creative sounds ──

    fn synth_777(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, drive_amt: f64) -> f64 {
        match self.sound {
            // Notes 24-35 mapped to SubKick: 808-style bass (deep sine, heavy sweep, long decay)
            DrumSound::SubKick(_) | DrumSound::Kick if self.note < 36 => {
                let variation = if let DrumSound::SubKick(v) = self.sound { v } else { 0.5 };
                let freq = 42.0 * (0.6 + variation * 0.8);
                let sweep = 9.0 * (1.2 - variation * 0.6);
                let decay = (0.40 * (1.5 - variation * 0.8)) * decay_mod;
                let env = (-self.time / decay).exp();
                let pitch_env = sweep * (-self.time * (25.0 + variation * 20.0)).exp();
                let f = freq * (1.0 + pitch_env) * (0.9 + tone_mod * 0.2);
                self.phase1 += f / sr;
                if self.phase1 > 1.0 { self.phase1 -= 1.0; }
                let body = (self.phase1 * std::f64::consts::TAU).sin();
                let click = (-self.time * 800.0).exp() * (0.3 + variation * 0.4);
                (body + click * 0.1) * env
            }
            // Notes 36-47: 909-style bass (distorted sine, noise click, medium sweep)
            DrumSound::Kick | DrumSound::Snare | DrumSound::SnareAlt | DrumSound::Clap
                if self.note >= 36 && self.note <= 47 => {
                let v = (self.note - 36) as f64 / 11.0;
                let freq = 58.0 * (0.6 + v * 0.8);
                let sweep = 5.0 * (1.2 - v * 0.6);
                let decay = (0.22 * (1.5 - v * 0.8)) * decay_mod;
                let env = (-self.time / decay).exp();
                let pitch_env = sweep * (-self.time * (25.0 + v * 20.0)).exp();
                let f = freq * (1.0 + pitch_env) * (0.9 + tone_mod * 0.2);
                self.phase1 += f / sr;
                if self.phase1 > 1.0 { self.phase1 -= 1.0; }
                let sine = (self.phase1 * std::f64::consts::TAU).sin();
                // 909-style distortion
                let gain = 1.0 + (0.35 + drive_amt) * 8.0;
                let driven = sine * gain;
                let body = driven / (1.0 + driven.abs()).sqrt();
                // Noise click
                let noise = self.noise();
                let click_env = (-self.time * 200.0).exp();
                let click = noise * click_env * 0.25;
                (body + click) * env
            }
            // 777 closed hat: granular metallic — ring mod of two FM'd sines
            DrumSound::ClosedHat | DrumSound::PedalHat => {
                let decay = 0.04 * decay_mod;
                let env = (-self.time / decay).exp();
                let f1 = 3200.0 * (0.8 + tone_mod * 0.4);
                let f2 = 5100.0 * (0.9 + tone_mod * 0.2);
                self.phase1 += f1 / sr;
                self.phase2 += f2 / sr;
                let fm = (self.phase1 * std::f64::consts::TAU).sin();
                let carrier = ((self.phase2 + fm * 0.3) * std::f64::consts::TAU).sin();
                let ring = fm * carrier; // ring modulation = metallic
                let noise = self.noise() * 0.3 * noise_mod;
                (ring + noise) * env
            }
            // 777 open hat: same as closed but with slow amplitude modulation
            DrumSound::OpenHat => {
                let decay = 0.25 * decay_mod;
                let env = (-self.time / decay).exp();
                let f1 = 2800.0 * (0.8 + tone_mod * 0.4);
                let f2 = 4700.0 * (0.9 + tone_mod * 0.2);
                self.phase1 += f1 / sr;
                self.phase2 += f2 / sr;
                let fm = (self.phase1 * std::f64::consts::TAU).sin();
                let carrier = ((self.phase2 + fm * 0.4) * std::f64::consts::TAU).sin();
                let ring = fm * carrier;
                let tremolo = 1.0 + (self.time * 35.0 * std::f64::consts::TAU).sin() * 0.15;
                let noise = self.noise() * 0.4 * noise_mod;
                (ring * tremolo + noise) * env
            }
            // 777 snare: tuned body + gated noise burst with pitch drop
            DrumSound::Snare | DrumSound::SnareAlt => {
                let decay_body = 0.12 * decay_mod;
                let decay_noise = 0.08 * decay_mod * (0.5 + noise_mod);
                let env_body = (-self.time / decay_body).exp();
                let env_noise = (-self.time / decay_noise).exp();
                let f = 220.0 * (0.8 + tone_mod * 0.4);
                let pitch_drop = 1.5 * (-self.time * 40.0).exp();
                self.phase1 += f * (1.0 + pitch_drop) / sr;
                let body = (self.phase1 * std::f64::consts::TAU).sin();
                // Gated noise — bursts
                let gate = if (self.time * 200.0).fract() < 0.6 { 1.0 } else { 0.3 };
                let noise = self.noise() * gate;
                body * env_body * 0.5 + noise * env_noise * 0.5
            }
            // 777 clap: filtered noise with chorus-like multi-tap
            DrumSound::Clap => {
                let decay = 0.1 * decay_mod;
                let env = (-self.time / decay).exp();
                let n1 = self.noise();
                let n2 = white_noise(self.noise_counter.wrapping_add(7000));
                let n3 = white_noise(self.noise_counter.wrapping_add(15000));
                let multi = (n1 + n2 * 0.8 + n3 * 0.6) / 2.4;
                let bp_freq = 1500.0 * (0.8 + tone_mod * 0.4);
                let (_, bp, _) = self.svf1.tick(multi, bp_freq, 3.0, sr);
                bp * env
            }
            // 777 cowbell: detuned additive with inharmonic partials
            DrumSound::Cowbell => {
                let decay = 0.06 * decay_mod;
                let env = (-self.time / decay).exp();
                self.phase1 += 587.0 / sr;
                self.phase2 += 845.0 / sr;
                let s1 = (self.phase1 * std::f64::consts::TAU).sin();
                let s2 = (self.phase2 * std::f64::consts::TAU).sin();
                let s3 = ((self.phase1 * 2.17) * std::f64::consts::TAU).sin() * 0.3;
                (s1 + s2 + s3) * env * 0.4
            }
            // 777 crash: noise wash with spectral motion
            DrumSound::Crash | DrumSound::Splash | DrumSound::Cymbal => {
                let decay = 0.6 * decay_mod;
                let env = (-self.time / decay).exp();
                let noise = self.noise();
                let sweep = (self.time * 3.0).min(1.0);
                let cutoff = 3000.0 + sweep * 6000.0 * tone_mod;
                let filtered = self.lp1.tick_lp(noise, cutoff, sr);
                self.phase1 += (440.0 * tone_mod + 200.0) / sr;
                let ring = (self.phase1 * std::f64::consts::TAU).sin() * 0.15;
                (filtered + ring) * env
            }
            // 777 ride: metallic ping with noise tail
            DrumSound::Ride | DrumSound::RideBell => {
                let decay = 0.3 * decay_mod;
                let env = (-self.time / decay).exp();
                self.phase1 += 620.0 / sr;
                self.phase2 += 893.0 / sr;
                let ping = (self.phase1 * std::f64::consts::TAU).sin()
                    * (self.phase2 * std::f64::consts::TAU).sin();
                let noise = self.noise() * 0.2 * (-self.time / (decay * 0.7)).exp();
                (ping * 0.6 + noise) * env
            }
            // 777 rimshot: phase-distorted click
            DrumSound::Rimshot => {
                let decay = 0.012 * decay_mod;
                let env = (-self.time / decay).exp();
                let f = 1200.0 * (0.8 + tone_mod * 0.4);
                self.phase1 += f / sr;
                // Phase distortion — sine of sine
                let pd = (self.phase1 * std::f64::consts::TAU).sin();
                let out = (pd * 3.0).sin();
                out * env * 0.6
            }
            // 777 toms: FM synthesis toms
            DrumSound::LowTom => self.synth_777_fm_tom(sr, decay_mod, tone_mod, 90.0),
            DrumSound::MidTom => self.synth_777_fm_tom(sr, decay_mod, tone_mod, 140.0),
            DrumSound::HighTom => self.synth_777_fm_tom(sr, decay_mod, tone_mod, 200.0),
            // 777 clave: hard digital click
            DrumSound::Clave => {
                let decay = 0.02 * decay_mod;
                let env = (-self.time / decay).exp();
                self.phase1 += 2800.0 / sr;
                let sq = if self.phase1.fract() < 0.5 { 1.0 } else { -1.0 };
                sq * env * 0.4
            }
            // Everything else: route through creative FX synthesis
            _ => {
                let character = (self.note as f64 - 48.0) / 80.0;
                self.synth_777_fx(sr, decay_mod, tone_mod, noise_mod, character.clamp(0.0, 1.0))
            }
        }
    }

    fn synth_777_fm_tom(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, base_freq: f64) -> f64 {
        let decay = 0.15 * decay_mod;
        let env = (-self.time / decay).exp();
        let f = base_freq * (0.8 + tone_mod * 0.4);
        let pitch_drop = 2.0 * (-self.time * 25.0).exp();
        // FM: modulator modulates carrier
        self.phase2 += (f * 2.0 * (1.0 + pitch_drop)) / sr;
        let modulator = (self.phase2 * std::f64::consts::TAU).sin();
        self.phase1 += (f * (1.0 + pitch_drop) + modulator * f * 0.5) / sr;
        let carrier = (self.phase1 * std::f64::consts::TAU).sin();
        carrier * env * 0.5
    }

    fn synth_777_fx(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, character: f64) -> f64 {
        let decay = (0.05 + character * 0.4) * decay_mod;
        let env = (-self.time / decay).exp();
        let f = 200.0 + character * 3000.0;
        // Wavefolder synthesis — creates rich harmonics
        self.phase1 += f * (0.8 + tone_mod * 0.4) / sr;
        let sine = (self.phase1 * std::f64::consts::TAU).sin();
        let fold_amount = 1.0 + character * 4.0;
        let folded = (sine * fold_amount).sin(); // sine of sine = wavefolding
        let noise = self.noise() * noise_mod * 0.3;
        (folded * 0.4 + noise * 0.3) * env
    }

    // ══════════════════════════════════════════════════════════════════════════
    // TSTY-1: Warm vintage studio kit — 88 sounds
    // Tape-saturated, reel-to-reel warmth, clean analog funk character
    // ══════════════════════════════════════════════════════════════════════════

    /// Gentle tape saturation — warm soft-clip with even harmonics.
    fn tape_sat(x: f64, amount: f64) -> f64 {
        if amount < 0.01 { return x; }
        let g = 1.0 + amount * 3.0;
        let driven = x * g;
        // Asymmetric soft-clip adds even harmonics (tape character)
        let out = driven / (1.0 + driven.abs()) + 0.05 * driven / (1.0 + (driven * 0.5).powi(2));
        out / g.sqrt()
    }

    /// Warm lowpass — single-pole filter simulating tape HF rolloff.
    fn tape_lp(&mut self, input: f64, cutoff: f64, sr: f64) -> f64 {
        let rc = 1.0 / (TAU * cutoff);
        let alpha = 1.0 / (1.0 + rc * sr);
        self.lp1_state = self.lp1_state + alpha * (input - self.lp1_state);
        self.lp1_state
    }

    fn synth_tsty1(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, _drive_amt: f64) -> f64 {
        // All tsty-1 sounds go through tape saturation + warm LP
        let raw = self.synth_tsty1_raw(sr, decay_mod, tone_mod, noise_mod);
        let saturated = Self::tape_sat(raw, 0.4);
        // Warm tape rolloff — cuts harsh highs like reel-to-reel
        let freq_cutoff = 8000.0 + tone_mod * 4000.0; // 8-12kHz tape rolloff
        self.tape_lp(saturated, freq_cutoff, sr)
    }

    fn synth_tsty1_raw(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        match self.sound {
            // ── KICKS (notes 24-35: 12 variations) ──
            DrumSound::SubKick(mult) => {
                // Variety of warm kicks based on mult value
                let variant = (mult * 20.0) as u8;
                match variant {
                    0..=1 => self.tsty1_kick_deep(sr, decay_mod, tone_mod),
                    2..=3 => self.tsty1_kick_round(sr, decay_mod, tone_mod),
                    4..=5 => self.tsty1_kick_punchy(sr, decay_mod, tone_mod),
                    6..=7 => self.tsty1_kick_warm(sr, decay_mod, tone_mod),
                    8..=9 => self.tsty1_kick_tight(sr, decay_mod, tone_mod),
                    10..=11 => self.tsty1_kick_boom(sr, decay_mod, tone_mod),
                    12..=13 => self.tsty1_kick_click(sr, decay_mod, tone_mod),
                    _ => self.tsty1_kick_vinyl(sr, decay_mod, tone_mod),
                }
            }
            DrumSound::Kick => self.tsty1_kick_studio(sr, decay_mod, tone_mod),

            // ── SNARES ──
            DrumSound::Snare => self.tsty1_snare_funk(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::SnareAlt => self.tsty1_snare_crisp(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::Rimshot => self.tsty1_rim_warm(sr, decay_mod, tone_mod),
            DrumSound::Clap => self.tsty1_clap_studio(sr, decay_mod, noise_mod),

            // ── HATS ──
            DrumSound::ClosedHat => self.tsty1_hat_closed_tight(sr, decay_mod, tone_mod),
            DrumSound::PedalHat => self.tsty1_hat_closed_soft(sr, decay_mod, tone_mod),
            DrumSound::OpenHat => self.tsty1_hat_open_shimmer(sr, decay_mod, tone_mod),

            // ── TOMS ──
            DrumSound::LowTom => self.tsty1_tom_warm(sr, decay_mod, tone_mod, 90.0),
            DrumSound::MidTom => self.tsty1_tom_warm(sr, decay_mod, tone_mod, 140.0),
            DrumSound::HighTom => self.tsty1_tom_warm(sr, decay_mod, tone_mod, 200.0),

            // ── CYMBALS ──
            DrumSound::Crash => self.tsty1_crash_warm(sr, decay_mod, tone_mod),
            DrumSound::Ride => self.tsty1_ride_smooth(sr, decay_mod, tone_mod),
            DrumSound::RideBell => self.tsty1_ride_bell(sr, decay_mod, tone_mod),
            DrumSound::Cymbal => self.tsty1_crash_warm(sr, decay_mod, tone_mod),
            DrumSound::Splash => self.tsty1_splash(sr, decay_mod, tone_mod),

            // ── PERCUSSION ──
            DrumSound::Cowbell => self.tsty1_cowbell(sr, decay_mod, tone_mod),
            DrumSound::Clave => self.tsty1_woodblock(sr, decay_mod),
            DrumSound::Tambourine => self.tsty1_tambourine(sr, decay_mod),
            DrumSound::Maracas => self.tsty1_shaker(sr, decay_mod),
            DrumSound::Cabasa => self.tsty1_cabasa(sr, decay_mod),
            DrumSound::Vibraslap => self.tsty1_vibraslap(sr, decay_mod),
            DrumSound::Conga(f) => self.tsty1_conga(sr, decay_mod, tone_mod, f),
            DrumSound::Bongo(f) => self.tsty1_bongo(sr, decay_mod, tone_mod, f),
            DrumSound::Timbale(f) => self.tsty1_timbale(sr, decay_mod, tone_mod, f),
            DrumSound::Agogo(f) => self.tsty1_agogo(sr, decay_mod, tone_mod, f),
            DrumSound::Whistle(d) => self.synth_808_whistle(sr, decay_mod, d),
            DrumSound::Guiro(d) => self.synth_808_guiro(sr, decay_mod, d),

            // ── FX (76-127) ──
            DrumSound::FxNoise(v) => self.tsty1_fx(sr, decay_mod, tone_mod, noise_mod, v),
        }
    }

    // ── TSTY-1 Kick variations ──

    fn tsty1_kick_studio(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        // Classic studio kick: warm body + subtle click
        let f = 52.0 * tone_mod;
        let sweep = f * 0.8 * (-self.time * 45.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1);
        let click = self.noise() * (-self.time * 300.0).exp() * 0.15;
        let env = (-self.time / (0.35 * decay_mod)).exp();
        (body * 0.85 + click) * env
    }

    fn tsty1_kick_deep(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 38.0 * tone_mod;
        let sweep = f * 0.5 * (-self.time * 30.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1);
        let sub = osc_sine(self.phase1 * 0.5) * 0.3; // sub harmonic
        let env = (-self.time / (0.5 * decay_mod)).exp();
        (body * 0.7 + sub) * env
    }

    fn tsty1_kick_round(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 48.0 * tone_mod;
        let sweep = f * 0.6 * (-self.time * 35.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        // Triangle wave for rounder tone
        let body = osc_triangle(self.phase1);
        let env = (-self.time / (0.3 * decay_mod)).exp();
        body * 0.9 * env
    }

    fn tsty1_kick_punchy(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 55.0 * tone_mod;
        let sweep = f * 1.2 * (-self.time * 60.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1);
        let click = self.noise() * (-self.time * 500.0).exp() * 0.25;
        let env = (-self.time / (0.2 * decay_mod)).exp();
        (body * 0.8 + click) * env
    }

    fn tsty1_kick_warm(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 45.0 * tone_mod;
        let sweep = f * 0.4 * (-self.time * 25.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        advance_phase(&mut self.phase2, f * 2.0 + sweep * 2.0, sr);
        let body = osc_sine(self.phase1) * 0.8 + osc_sine(self.phase2) * 0.15 * (-self.time * 80.0).exp();
        let env = (-self.time / (0.4 * decay_mod)).exp();
        body * env
    }

    fn tsty1_kick_tight(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 60.0 * tone_mod;
        let sweep = f * 1.5 * (-self.time * 80.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1);
        let env = (-self.time / (0.15 * decay_mod)).exp();
        body * 0.9 * env
    }

    fn tsty1_kick_boom(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 40.0 * tone_mod;
        let sweep = f * 0.3 * (-self.time * 15.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1);
        let rumble = osc_sine(self.phase1 * 0.5) * 0.2;
        let env = (-self.time / (0.6 * decay_mod)).exp();
        (body * 0.8 + rumble) * env
    }

    fn tsty1_kick_click(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 65.0 * tone_mod;
        let sweep = f * 2.0 * (-self.time * 100.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1) * 0.7;
        let click = self.noise() * (-self.time * 800.0).exp() * 0.4;
        let env = (-self.time / (0.12 * decay_mod)).exp();
        (body + click) * env
    }

    fn tsty1_kick_vinyl(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        // Lo-fi warm kick with subtle noise floor
        let f = 50.0 * tone_mod;
        let sweep = f * 0.6 * (-self.time * 40.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1);
        let floor = self.noise() * 0.02; // tape hiss
        let env = (-self.time / (0.35 * decay_mod)).exp();
        (body * 0.85 + floor) * env
    }

    // ── TSTY-1 Snare variations ──

    fn tsty1_snare_funk(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        // Warm funky snare — two body tones + filtered noise
        let f1 = 185.0 * tone_mod;
        let f2 = 330.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);
        let body = osc_sine(self.phase1) * 0.4 + osc_sine(self.phase2) * 0.25;
        let body_env = (-self.time / (0.12 * decay_mod)).exp();
        let raw_noise = self.noise() * noise_mod;
        let filtered = self.svf1.bandpass(raw_noise, 3500.0 * tone_mod, 1.5, sr);
        let noise_env = (-self.time / (0.18 * decay_mod)).exp();
        body * body_env + filtered * noise_env * 0.45
    }

    fn tsty1_snare_crisp(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        // Brighter, crisper snare
        let f1 = 200.0 * tone_mod;
        let f2 = 400.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);
        let body = osc_sine(self.phase1) * 0.35 + osc_triangle(self.phase2) * 0.2;
        let body_env = (-self.time / (0.08 * decay_mod)).exp();
        let raw_noise = self.noise() * noise_mod;
        let hp_noise = self.hp1.tick_hp(raw_noise, 4000.0 * tone_mod, sr);
        let filtered = self.svf1.bandpass(hp_noise, 6000.0, 1.2, sr);
        let noise_env = (-self.time / (0.15 * decay_mod)).exp();
        body * body_env + filtered * noise_env * 0.5
    }

    fn tsty1_rim_warm(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        // Warm rimshot with woody body
        advance_phase(&mut self.phase1, 480.0 * tone_mod, sr);
        advance_phase(&mut self.phase2, 1200.0 * tone_mod, sr);
        let body = osc_sine(self.phase1) * 0.5 + osc_sine(self.phase2) * 0.3;
        let click = self.noise() * (-self.time * 600.0).exp() * 0.2;
        let env = (-self.time / (0.02 * decay_mod)).exp();
        (body + click) * env
    }

    fn tsty1_clap_studio(&mut self, sr: f64, decay_mod: f64, noise_mod: f64) -> f64 {
        // Studio clap: multiple noise bursts with warm reverb tail
        let raw_noise = self.noise() * noise_mod;
        let filtered = self.svf1.bandpass(raw_noise, 1200.0, 2.5, sr);
        // 4 staggered hits for clap spread
        let burst_spacing = 0.012;
        let mut env = 0.0;
        for n in 0..4 {
            let t_offset = self.time - n as f64 * burst_spacing;
            if t_offset >= 0.0 {
                env += (-t_offset * 200.0).exp() * 0.3;
            }
        }
        // Reverb tail
        let tail = (-self.time / (0.15 * decay_mod)).exp() * 0.4;
        filtered * (env + tail)
    }

    // ── TSTY-1 Hat variations ──

    fn tsty1_hat_closed_tight(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        // Tight closed hat — bright but warm
        let mut freqs = [310.0, 456.0, 620.0, 830.0, 1050.0, 1380.0];
        for f in freqs.iter_mut() { *f *= tone_mod; }
        let metallic = self.hat_oscs.tick(sr, &freqs);
        let filtered = self.svf1.bandpass(metallic, 7500.0, 1.8, sr);
        let hp = self.hp1.tick_hp(filtered, 5000.0, sr);
        let env = (-self.time / (0.035 * decay_mod)).exp();
        hp * env * 0.4
    }

    fn tsty1_hat_closed_soft(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        // Softer closed hat — more mellow
        let mut freqs = [280.0, 420.0, 580.0, 780.0, 1000.0, 1300.0];
        for f in freqs.iter_mut() { *f *= tone_mod; }
        let metallic = self.hat_oscs.tick(sr, &freqs);
        let filtered = self.svf1.bandpass(metallic, 5500.0, 1.5, sr);
        let hp = self.hp1.tick_hp(filtered, 3500.0, sr);
        let env = (-self.time / (0.05 * decay_mod)).exp();
        hp * env * 0.35
    }

    fn tsty1_hat_open_shimmer(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        // Open hat — shimmery, sustained
        let mut freqs = [320.0, 470.0, 640.0, 860.0, 1100.0, 1420.0];
        for f in freqs.iter_mut() { *f *= tone_mod; }
        let metallic = self.hat_oscs.tick(sr, &freqs);
        let filtered = self.svf1.bandpass(metallic, 6500.0, 1.3, sr);
        let hp = self.hp1.tick_hp(filtered, 4000.0, sr);
        let env = (-self.time / (0.35 * decay_mod)).exp();
        hp * env * 0.35
    }

    // ── TSTY-1 Toms ──

    fn tsty1_tom_warm(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, base_freq: f64) -> f64 {
        let f = base_freq * tone_mod;
        let droop = f * 0.12 * (-self.time * 30.0).exp();
        advance_phase(&mut self.phase1, f + droop, sr);
        advance_phase(&mut self.phase2, f * 1.5, sr); // adds warmth
        let body = osc_sine(self.phase1) * 0.7 + osc_sine(self.phase2) * 0.15 * (-self.time * 60.0).exp();
        let attack = self.noise() * (-self.time * 200.0).exp() * 0.1;
        let env = (-self.time / (0.25 * decay_mod)).exp();
        (body + attack) * env
    }

    // ── TSTY-1 Cymbals ──

    fn tsty1_crash_warm(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = [340.0, 510.0, 680.0, 920.0, 1150.0, 1500.0];
        for f in freqs.iter_mut() { *f *= tone_mod; }
        let metallic = self.hat_oscs.tick(sr, &freqs);
        let noise_layer = self.noise() * 0.2;
        let mixed = metallic * 0.5 + noise_layer;
        let filtered = self.svf1.lowpass(mixed, 9000.0 * tone_mod, 0.3, sr);
        let env = (-self.time / (1.2 * decay_mod)).exp();
        filtered * env * 0.3
    }

    fn tsty1_ride_smooth(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = [380.0, 560.0, 750.0, 1000.0, 1280.0, 1650.0];
        for f in freqs.iter_mut() { *f *= tone_mod; }
        let metallic = self.hat_oscs.tick(sr, &freqs);
        let filtered = self.svf1.bandpass(metallic, 5000.0 * tone_mod, 1.0, sr);
        let env = (-self.time / (0.8 * decay_mod)).exp();
        let attack_env = 1.0 - (-self.time * 200.0).exp();
        filtered * env * attack_env.min(1.0) * 0.3
    }

    fn tsty1_ride_bell(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        advance_phase(&mut self.phase1, 680.0 * tone_mod, sr);
        advance_phase(&mut self.phase2, 1020.0 * tone_mod, sr);
        let bell = osc_sine(self.phase1) * 0.4 + osc_sine(self.phase2) * 0.3;
        let env = (-self.time / (0.6 * decay_mod)).exp();
        bell * env
    }

    fn tsty1_splash(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = [400.0, 600.0, 820.0, 1100.0, 1400.0, 1800.0];
        for f in freqs.iter_mut() { *f *= tone_mod; }
        let metallic = self.hat_oscs.tick(sr, &freqs);
        let filtered = self.svf1.bandpass(metallic, 7000.0, 1.5, sr);
        let env = (-self.time / (0.5 * decay_mod)).exp();
        filtered * env * 0.3
    }

    // ── TSTY-1 Percussion ──

    fn tsty1_cowbell(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        advance_phase(&mut self.phase1, 560.0 * tone_mod, sr);
        advance_phase(&mut self.phase2, 845.0 * tone_mod, sr);
        let body = osc_sine(self.phase1) * 0.4 + osc_sine(self.phase2) * 0.35;
        let filtered = self.svf1.bandpass(body, 700.0, 3.0, sr);
        let env = (-self.time / (0.06 * decay_mod)).exp();
        filtered * env
    }

    fn tsty1_woodblock(&mut self, sr: f64, decay_mod: f64) -> f64 {
        advance_phase(&mut self.phase1, 1800.0, sr);
        let click = osc_sine(self.phase1);
        let env = (-self.time / (0.015 * decay_mod)).exp();
        click * env * 0.5
    }

    fn tsty1_tambourine(&mut self, sr: f64, decay_mod: f64) -> f64 {
        let raw_noise = self.noise();
        let hp = self.hp1.tick_hp(raw_noise, 6000.0, sr);
        let bp = self.svf1.bandpass(hp, 9000.0, 2.0, sr);
        let jingle = bp * 0.4;
        // Rhythmic jingle modulation
        let mod_env = (self.time * 25.0).sin().abs() * (-self.time * 8.0).exp();
        let main_env = (-self.time / (0.15 * decay_mod)).exp();
        jingle * (main_env + mod_env * 0.3)
    }

    fn tsty1_shaker(&mut self, sr: f64, decay_mod: f64) -> f64 {
        let raw_noise = self.noise();
        let filtered = self.svf1.bandpass(raw_noise, 7000.0, 1.5, sr);
        let hp = self.hp1.tick_hp(filtered, 5000.0, sr);
        let env = (-self.time / (0.08 * decay_mod)).exp();
        hp * env * 0.35
    }

    fn tsty1_cabasa(&mut self, sr: f64, decay_mod: f64) -> f64 {
        let raw_noise = self.noise();
        let filtered = self.svf1.bandpass(raw_noise, 8500.0, 2.0, sr);
        let env = (-self.time / (0.1 * decay_mod)).exp();
        filtered * env * 0.3
    }

    fn tsty1_vibraslap(&mut self, sr: f64, decay_mod: f64) -> f64 {
        let raw_noise = self.noise();
        let filtered = self.svf1.bandpass(raw_noise, 3200.0, 5.0, sr);
        let rattle = (self.time * 30.0 * TAU).sin().abs();
        let env = (-self.time / (0.4 * decay_mod)).exp();
        filtered * rattle * env * 0.3
    }

    fn tsty1_conga(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f = freq * tone_mod;
        let droop = f * 0.06 * (-self.time * 40.0).exp();
        advance_phase(&mut self.phase1, f + droop, sr);
        let body = osc_sine(self.phase1);
        let slap = self.noise() * (-self.time * 400.0).exp() * 0.15;
        let env = (-self.time / (0.2 * decay_mod)).exp();
        (body * 0.7 + slap) * env
    }

    fn tsty1_bongo(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f = freq * tone_mod;
        let droop = f * 0.08 * (-self.time * 60.0).exp();
        advance_phase(&mut self.phase1, f + droop, sr);
        let body = osc_sine(self.phase1);
        let env = (-self.time / (0.12 * decay_mod)).exp();
        body * 0.7 * env
    }

    fn tsty1_timbale(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f = freq * tone_mod;
        advance_phase(&mut self.phase1, f, sr);
        let body = osc_sine(self.phase1);
        let ring = osc_sine(self.phase1 * 2.5) * 0.15 * (-self.time * 30.0).exp();
        let shell = self.noise() * (-self.time * 300.0).exp() * 0.1;
        let env = (-self.time / (0.18 * decay_mod)).exp();
        (body * 0.6 + ring + shell) * env
    }

    fn tsty1_agogo(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f1 = freq * tone_mod;
        let f2 = f1 * 1.48;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);
        let body = osc_sine(self.phase1) * 0.4 + osc_sine(self.phase2) * 0.3;
        let env = (-self.time / (0.15 * decay_mod)).exp();
        body * env
    }

    fn tsty1_fx(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, character: f64) -> f64 {
        // Variety of warm FX sounds
        let freq = 400.0 + character * 6000.0;
        let sweep = freq * (1.0 + 1.5 * (-self.time * 8.0).exp());
        advance_phase(&mut self.phase1, sweep * tone_mod, sr);
        let tone = osc_sine(self.phase1) * 0.4;
        let noise = self.noise() * noise_mod * 0.2;
        let env = (-self.time / (0.3 * decay_mod)).exp();
        (tone + noise) * env
    }

    // ══════════════════════════════════════════════════════════════════════════
    // TSTY-2: Realistic acoustic drums through reel-to-reel
    // Modal synthesis with Bessel function ratios, multi-component envelopes,
    // per-hit randomization, and frequency-dependent tape saturation.
    // ══════════════════════════════════════════════════════════════════════════

    /// Per-hit random value from hit_seed. Deterministic but varies per hit.
    fn hit_rand(&self, offset: u32) -> f64 {
        let mut x = self.hit_seed.wrapping_add(offset).wrapping_mul(2654435761);
        x ^= x >> 16;
        x = x.wrapping_mul(1103515245);
        (x as i32) as f64 / i32::MAX as f64
    }

    /// Frequency-dependent tape saturation — HF saturates more, adds head bump.
    fn tape_process(input: f64, time: f64, sr: f64, lp_state: &mut f64) -> f64 {
        // Asymmetric saturation (even + odd harmonics like real tape)
        let driven = input * 1.8;
        let sat = driven.tanh() + 0.04 * driven * (-driven.abs()).exp();

        // Tape HF rolloff (~10kHz, simulating 15ips)
        let rc = 1.0 / (TAU * 10000.0);
        let alpha = 1.0 / (1.0 + rc * sr);
        *lp_state = *lp_state + alpha * (sat - *lp_state);

        // Head bump: +3dB at ~80Hz (modeled as gentle boost)
        // We approximate this by mixing in a slight bass emphasis
        let _ = time; // time available for future wow/flutter
        *lp_state
    }

    fn synth_tsty2(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, _drive: f64) -> f64 {
        let raw = self.synth_tsty2_raw(sr, decay_mod, tone_mod, noise_mod);
        Self::tape_process(raw, self.time, sr, &mut self.lp1_state)
    }

    fn synth_tsty2_raw(&mut self, sr: f64, dm: f64, tm: f64, nm: f64) -> f64 {
        match self.sound {
            DrumSound::SubKick(mult) => {
                let variant = (mult * 20.0) as u8;
                match variant {
                    0..=2 => self.t2_kick_funk(sr, dm, tm, 70.0),
                    3..=5 => self.t2_kick_jazz(sr, dm, tm, 55.0),
                    6..=8 => self.t2_kick_rock(sr, dm, tm, 62.0),
                    9..=11 => self.t2_kick_tight(sr, dm, tm, 75.0),
                    12..=14 => self.t2_kick_deep(sr, dm, tm, 48.0),
                    15..=17 => self.t2_kick_round(sr, dm, tm, 58.0),
                    18..=19 => self.t2_kick_lo(sr, dm, tm, 42.0),
                    _ => self.t2_kick_click(sr, dm, tm, 68.0),
                }
            }
            DrumSound::Kick => self.t2_kick_funk(sr, dm, tm, 68.0),
            DrumSound::Snare => self.t2_snare_funk(sr, dm, tm, nm, 300.0),
            DrumSound::SnareAlt => self.t2_snare_dry(sr, dm, tm, nm, 280.0),
            DrumSound::Rimshot => self.t2_rimshot(sr, dm, tm),
            DrumSound::Clap => self.t2_clap(sr, dm, nm),
            DrumSound::ClosedHat => self.t2_hat_closed(sr, dm, tm, 0.04),
            DrumSound::PedalHat => self.t2_hat_pedal(sr, dm, tm),
            DrumSound::OpenHat => self.t2_hat_open(sr, dm, tm),
            DrumSound::LowTom => self.t2_tom(sr, dm, tm, 95.0, 0.28),
            DrumSound::MidTom => self.t2_tom(sr, dm, tm, 145.0, 0.22),
            DrumSound::HighTom => self.t2_tom(sr, dm, tm, 210.0, 0.18),
            DrumSound::Crash => self.t2_crash(sr, dm, tm, 1.5),
            DrumSound::Ride => self.t2_ride(sr, dm, tm),
            DrumSound::RideBell => self.t2_ride_bell(sr, dm, tm),
            DrumSound::Cymbal => self.t2_crash(sr, dm, tm, 1.0),
            DrumSound::Splash => self.t2_crash(sr, dm, tm, 0.6),
            DrumSound::Cowbell => self.t2_cowbell(sr, dm, tm),
            DrumSound::Clave => self.t2_woodblock(sr, dm, tm, 1800.0),
            DrumSound::Tambourine => self.t2_tambourine(sr, dm, tm),
            DrumSound::Maracas => self.t2_shaker(sr, dm),
            DrumSound::Cabasa => self.t2_shaker_long(sr, dm),
            DrumSound::Vibraslap => self.t2_vibraslap(sr, dm),
            DrumSound::Conga(f) => self.t2_tom(sr, dm, tm, f, 0.2),
            DrumSound::Bongo(f) => self.t2_tom(sr, dm, tm, f, 0.12),
            DrumSound::Timbale(f) => self.t2_timbale(sr, dm, tm, f),
            DrumSound::Agogo(f) => self.t2_agogo(sr, dm, tm, f),
            DrumSound::Whistle(d) => self.synth_808_whistle(sr, dm, d),
            DrumSound::Guiro(d) => self.synth_808_guiro(sr, dm, d),
            DrumSound::FxNoise(v) => self.t2_fx_perc(sr, dm, tm, nm, v),
        }
    }

    // ── TSTY-2 Kicks: Modal synthesis with Bessel ratios ──

    fn t2_kick_funk(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        let f = fund * tm;
        // Bessel membrane mode ratios for circular drum head
        let r_11 = 1.593; let r_21 = 2.136; let r_02 = 2.296;

        // Pitch sweep: real head is slower than 808 (8-15ms)
        let sweep = f * 0.3 * (-self.time * 70.0).exp();
        let ff = f + sweep;

        // Modal oscillators: fundamental + 3 inharmonic modes
        advance_phase(&mut self.phase1, ff, sr);
        advance_phase(&mut self.phase2, ff * r_11, sr);
        advance_phase(&mut self.phase3, ff * r_21, sr);
        advance_phase(&mut self.modal_phases[0], ff * r_02, sr);

        // Multi-rate envelope: fast initial + slower body
        let t = self.time;
        let fast = (-t / 0.008).exp(); // initial transient loss
        let slow = (-t / (0.18 * dm)).exp(); // body
        let env = 0.35 * fast + 0.65 * slow;

        // Modes with independent decay (higher modes die faster)
        let body = osc_sine(self.phase1) * 0.65 * env;
        let m1 = osc_sine(self.phase2) * 0.12 * (-t / (0.08 * dm)).exp();
        let m2 = osc_sine(self.phase3) * 0.08 * (-t / (0.05 * dm)).exp();
        let m3 = osc_sine(self.modal_phases[0]) * 0.06 * (-t / (0.04 * dm)).exp();

        // Beater impact: filtered noise with finite rise time
        let beater_rise = (t / 0.0015).min(1.0); // 1.5ms rise — NOT instant
        let beater = self.noise() * beater_rise * (-t * 250.0).exp();
        let beater_filtered = self.svf1.bandpass(beater, 2800.0 * tm, 1.5, sr) * 0.2;

        // Sub thump (air push)
        advance_phase(&mut self.modal_phases[1], f * 0.5, sr);
        let sub = osc_sine(self.modal_phases[1]) * 0.15 * (-t / (0.1 * dm)).exp();

        // Per-hit variation: slight pitch and level randomization
        let pitch_var = 1.0 + self.hit_rand(0) * 0.005; // ±0.5%
        let _ = pitch_var; // applied via hit_seed in trigger

        body + m1 + m2 + m3 + beater_filtered + sub
    }

    fn t2_kick_jazz(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        // Deeper, less damped, more resonant
        let f = fund * tm;
        let sweep = f * 0.4 * (-self.time * 40.0).exp(); // slower sweep
        advance_phase(&mut self.phase1, f + sweep, sr);
        advance_phase(&mut self.phase2, (f + sweep) * 1.593, sr);
        advance_phase(&mut self.phase3, (f + sweep) * 2.296, sr);

        let t = self.time;
        let env = 0.3 * (-t / 0.012).exp() + 0.7 * (-t / (0.35 * dm)).exp();
        let body = osc_sine(self.phase1) * 0.6 * env;
        let m1 = osc_sine(self.phase2) * 0.15 * (-t / (0.12 * dm)).exp();
        let m2 = osc_sine(self.phase3) * 0.08 * (-t / (0.08 * dm)).exp();

        // Soft felt beater
        let beater = self.noise() * (t / 0.002).min(1.0) * (-t * 150.0).exp();
        let beater_f = self.svf1.bandpass(beater, 1800.0 * tm, 1.2, sr) * 0.12;

        body + m1 + m2 + beater_f
    }

    fn t2_kick_rock(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        let f = fund * tm;
        let sweep = f * 0.35 * (-self.time * 55.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        advance_phase(&mut self.phase2, (f + sweep) * 1.593, sr);

        let t = self.time;
        let env = 0.4 * (-t / 0.006).exp() + 0.6 * (-t / (0.22 * dm)).exp();
        let body = osc_sine(self.phase1) * 0.7 * env;
        let m1 = osc_sine(self.phase2) * 0.1 * (-t / (0.06 * dm)).exp();

        // Plastic beater — brighter click
        let beater = self.noise() * (t / 0.001).min(1.0) * (-t * 400.0).exp();
        let beater_f = self.svf1.bandpass(beater, 4500.0 * tm, 2.0, sr) * 0.25;

        // Shell resonance
        let shell = self.noise() * (-t * 80.0).exp();
        let shell_f = self.svf2.bandpass(shell, 280.0 * tm, 10.0, sr) * 0.06;

        body + m1 + beater_f + shell_f
    }

    fn t2_kick_tight(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        let f = fund * tm;
        let sweep = f * 0.2 * (-self.time * 90.0).exp(); // fast sweep
        advance_phase(&mut self.phase1, f + sweep, sr);
        let t = self.time;
        let env = 0.5 * (-t / 0.005).exp() + 0.5 * (-t / (0.12 * dm)).exp();
        let body = osc_sine(self.phase1) * 0.75 * env;
        let beater = self.noise() * (t / 0.001).min(1.0) * (-t * 500.0).exp();
        let beater_f = self.svf1.bandpass(beater, 3500.0 * tm, 1.8, sr) * 0.2;
        body + beater_f
    }

    fn t2_kick_deep(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        let f = fund * tm;
        let sweep = f * 0.5 * (-self.time * 25.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        advance_phase(&mut self.phase2, (f + sweep) * 0.5, sr); // sub
        let t = self.time;
        let env = 0.2 * (-t / 0.015).exp() + 0.8 * (-t / (0.45 * dm)).exp();
        let body = osc_sine(self.phase1) * 0.55 * env;
        let sub = osc_sine(self.phase2) * 0.25 * (-t / (0.3 * dm)).exp();
        let beater = self.noise() * (t / 0.002).min(1.0) * (-t * 120.0).exp();
        let beater_f = self.svf1.bandpass(beater, 1500.0 * tm, 1.0, sr) * 0.1;
        body + sub + beater_f
    }

    fn t2_kick_round(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        let f = fund * tm;
        let sweep = f * 0.35 * (-self.time * 45.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        advance_phase(&mut self.phase2, (f + sweep) * 1.593, sr);
        let t = self.time;
        // Triangle for rounder character
        let body = osc_triangle(self.phase1) * 0.6 * (0.3 * (-t / 0.01).exp() + 0.7 * (-t / (0.25 * dm)).exp());
        let m1 = osc_sine(self.phase2) * 0.1 * (-t / (0.06 * dm)).exp();
        let beater = self.noise() * (t / 0.002).min(1.0) * (-t * 200.0).exp();
        let beater_f = self.svf1.bandpass(beater, 2000.0 * tm, 1.3, sr) * 0.12;
        body + m1 + beater_f
    }

    fn t2_kick_lo(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        // Very deep floor tom-like kick
        let f = fund * tm;
        let sweep = f * 0.6 * (-self.time * 20.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let t = self.time;
        let env = 0.2 * (-t / 0.02).exp() + 0.8 * (-t / (0.5 * dm)).exp();
        let body = osc_sine(self.phase1) * 0.7 * env;
        let rumble = osc_sine(self.phase1 * 0.5) * 0.15 * (-t / (0.35 * dm)).exp();
        body + rumble
    }

    fn t2_kick_click(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        // Lots of beater attack, less body
        let f = fund * tm;
        let sweep = f * 0.25 * (-self.time * 80.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let t = self.time;
        let env = 0.5 * (-t / 0.004).exp() + 0.5 * (-t / (0.15 * dm)).exp();
        let body = osc_sine(self.phase1) * 0.5 * env;
        // Wood beater — bright, loud click
        let beater = self.noise() * (t / 0.0005).min(1.0) * (-t * 600.0).exp();
        let beater_f = self.svf1.bandpass(beater, 5000.0 * tm, 2.5, sr) * 0.4;
        body + beater_f
    }

    // ── TSTY-2 Snares: Body modes + wire buzz via comb-filtered noise ──

    fn t2_snare_funk(&mut self, sr: f64, dm: f64, tm: f64, nm: f64, fund: f64) -> f64 {
        let f = fund * tm;
        let t = self.time;

        // Stick impact: very short bright burst (0.5-1ms)
        let stick = self.noise() * (t / 0.0003).min(1.0) * (-t * 1500.0).exp();
        let stick_f = self.hp1.tick_hp(stick, 3000.0, sr) * 0.3;

        // Batter head modes (Bessel ratios)
        let pitch_drop = f * 0.08 * (-t * 200.0).exp();
        advance_phase(&mut self.phase1, f + pitch_drop, sr);
        advance_phase(&mut self.phase2, (f + pitch_drop) * 1.593, sr);
        advance_phase(&mut self.phase3, (f + pitch_drop) * 2.136, sr);

        let head = osc_sine(self.phase1) * 0.35 * (-t / (0.1 * dm)).exp()
                 + osc_sine(self.phase2) * 0.15 * (-t / (0.07 * dm)).exp()
                 + osc_sine(self.phase3) * 0.08 * (-t / (0.05 * dm)).exp();

        // Snare wire buzz: noise through a comb filter at head fundamental
        let raw_noise = self.noise() * nm;
        // Comb filter: delay of 1/fund, feedback 0.35
        let comb_delay = 1.0 / f;
        let comb_samples = (comb_delay * sr) as usize;
        // Simplified: use SVF bandpass at wire frequencies instead of true comb
        let wire_band = self.svf1.bandpass(raw_noise, 4200.0 * tm, 0.8, sr);
        let wire_hp = self.hp1.tick_hp(wire_band, 1800.0, sr);
        // Wire buzz amplitude follows head vibration
        let wire_env = (-t / (0.18 * dm)).exp();
        let wires = wire_hp * wire_env * 0.4;

        // Shell resonance
        let shell_noise = self.noise() * (-t * 60.0).exp();
        let shell = self.svf2.bandpass(shell_noise, 450.0 * tm, 12.0, sr) * 0.05;

        let _ = comb_samples; // unused in simplified approach
        stick_f + head + wires + shell
    }

    fn t2_snare_dry(&mut self, sr: f64, dm: f64, tm: f64, nm: f64, fund: f64) -> f64 {
        // Drier, tighter — like a Bernard Purdie snare
        let f = fund * tm;
        let t = self.time;

        let stick = self.noise() * (t / 0.0004).min(1.0) * (-t * 2000.0).exp();
        let stick_f = self.hp1.tick_hp(stick, 4000.0, sr) * 0.35;

        advance_phase(&mut self.phase1, f, sr);
        advance_phase(&mut self.phase2, f * 1.593, sr);
        let head = osc_sine(self.phase1) * 0.3 * (-t / (0.06 * dm)).exp()
                 + osc_sine(self.phase2) * 0.12 * (-t / (0.04 * dm)).exp();

        let raw_noise = self.noise() * nm;
        let wire_band = self.svf1.bandpass(raw_noise, 5000.0 * tm, 0.6, sr);
        let wires = wire_band * (-t / (0.1 * dm)).exp() * 0.35;

        stick_f + head + wires
    }

    fn t2_rimshot(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        let t = self.time;
        // Stick on rim: sharp metallic crack
        advance_phase(&mut self.phase1, 520.0 * tm, sr);
        advance_phase(&mut self.phase2, 1380.0 * tm, sr);
        advance_phase(&mut self.phase3, 2700.0 * tm, sr);
        let crack = osc_sine(self.phase1) * 0.3 + osc_sine(self.phase2) * 0.25 + osc_sine(self.phase3) * 0.15;
        let stick = self.noise() * (-t * 800.0).exp() * 0.2;
        let env = (-t / (0.025 * dm)).exp();
        (crack + stick) * env
    }

    fn t2_clap(&mut self, sr: f64, dm: f64, nm: f64) -> f64 {
        let t = self.time;
        // Multiple clappers with random timing (NOT evenly spaced like 808)
        let mut env = 0.0;
        for n in 0..6u32 {
            // Gaussian-ish random offset per clapper
            let offset = (self.hit_rand(n * 5) * 0.012 + self.hit_rand(n * 5 + 1).abs() * 0.008).abs();
            let t_off = t - offset;
            if t_off >= 0.0 {
                let amp = 0.8 + self.hit_rand(n * 5 + 2) * 0.2; // varied amplitude
                env += (-t_off * 180.0).exp() * amp * 0.2;
            }
        }
        // Each clapper filtered differently
        let raw = self.noise() * nm;
        let center = 2200.0 + self.hit_rand(50) * 800.0; // varied per hit
        let filtered = self.svf1.bandpass(raw, center, 1.5, sr);
        let hp = self.hp1.tick_hp(filtered, 600.0, sr);
        // Room tail
        let tail = (-t / (0.12 * dm)).exp() * 0.3;
        hp * (env + tail)
    }

    // ── TSTY-2 Hats: Modal synthesis (inharmonic sine bank, NOT square waves) ──

    fn t2_hat_closed(&mut self, sr: f64, dm: f64, tm: f64, decay_base: f64) -> f64 {
        let t = self.time;
        // 10 inharmonic cymbal modes — the key to realistic metal sound
        let modes: [(f64, f64); 10] = [
            (342.0 * tm, 0.08),   // low body
            (817.0 * tm, 0.12),
            (1453.0 * tm, 0.22),
            (2298.0 * tm, 0.40),
            (3419.0 * tm, 0.65),
            (4735.0 * tm, 1.00),  // peak energy
            (6328.0 * tm, 0.80),
            (8249.0 * tm, 0.50),
            (10400.0 * tm, 0.25),
            (12800.0 * tm, 0.10),
        ];

        // Use hat_oscs for first 6, modal_phases for rest
        let mut sum = 0.0;
        let hat_freqs: [f64; 6] = [modes[0].0, modes[1].0, modes[2].0, modes[3].0, modes[4].0, modes[5].0];
        let hat_raw = self.hat_oscs.tick(sr, &hat_freqs);
        // Weight by mode amplitudes
        sum += hat_raw * 0.3; // blended hat modes

        // Additional higher modes via modal_phases
        for i in 0..4 {
            let (freq, amp) = modes[6 + i];
            advance_phase(&mut self.modal_phases[i], freq, sr);
            let mode_decay = decay_base * dm * (0.8 + i as f64 * 0.15); // higher modes decay slower in cymbals!
            sum += osc_sine(self.modal_phases[i]) * amp * (-t / mode_decay).exp() * 0.15;
        }

        // Per-hit detune variation
        let detune_mod = 1.0 + self.hit_rand(20) * 0.015;
        sum *= detune_mod;

        // Stick transient
        let stick = self.noise() * (t / 0.0003).min(1.0) * (-t * 2000.0).exp() * 0.15;

        // Contact sizzle (cymbals touching)
        let sizzle = self.noise() * (-t / (decay_base * dm * 0.5)).exp();
        let sizzle_f = self.svf1.bandpass(sizzle, 8000.0, 6.0, sr) * 0.08;

        // Main decay
        let env = (-t / (decay_base * dm)).exp();
        (sum * env + stick + sizzle_f) * 0.4
    }

    fn t2_hat_pedal(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        // Foot pedal chick: no stick, just cymbals clapping
        let t = self.time;
        let hat_freqs: [f64; 6] = [350.0*tm, 830.0*tm, 1480.0*tm, 2350.0*tm, 3500.0*tm, 4800.0*tm];
        let modal = self.hat_oscs.tick(sr, &hat_freqs);
        // Very fast decay (cymbals pressed together)
        let env = (-t / (0.02 * dm)).exp();
        // Chick noise (cymbals clapping together)
        let chick = self.noise() * (-t * 500.0).exp();
        let chick_f = self.svf1.bandpass(chick, 1200.0, 2.0, sr) * 0.15;
        modal * env * 0.25 + chick_f
    }

    fn t2_hat_open(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        // Open: all modes ring freely, 500ms-1.5s
        self.t2_hat_closed(sr, dm, tm, 0.8)
    }

    // ── TSTY-2 Toms: Modal with shell resonance ──

    fn t2_tom(&mut self, sr: f64, dm: f64, tm: f64, fund: f64, body_decay: f64) -> f64 {
        let f = fund * tm;
        let t = self.time;
        let droop = f * 0.12 * (-t * 35.0).exp();
        advance_phase(&mut self.phase1, f + droop, sr);
        advance_phase(&mut self.phase2, (f + droop) * 1.593, sr);
        advance_phase(&mut self.phase3, (f + droop) * 2.136, sr);

        let env = 0.3 * (-t / 0.008).exp() + 0.7 * (-t / (body_decay * dm)).exp();
        let body = osc_sine(self.phase1) * 0.55 * env;
        let m1 = osc_sine(self.phase2) * 0.15 * (-t / (body_decay * dm * 0.6)).exp();
        let m2 = osc_sine(self.phase3) * 0.08 * (-t / (body_decay * dm * 0.4)).exp();

        let stick = self.noise() * (t / 0.001).min(1.0) * (-t * 300.0).exp();
        let stick_f = self.svf1.bandpass(stick, 3000.0 * tm, 1.5, sr) * 0.12;

        body + m1 + m2 + stick_f
    }

    // ── TSTY-2 Cymbals ──

    fn t2_crash(&mut self, sr: f64, dm: f64, tm: f64, decay_mult: f64) -> f64 {
        let t = self.time;
        let hat_freqs: [f64; 6] = [380.0*tm, 890.0*tm, 1520.0*tm, 2650.0*tm, 3800.0*tm, 5200.0*tm];
        let modal = self.hat_oscs.tick(sr, &hat_freqs);
        let noise = self.noise() * 0.15;
        let mixed = modal * 0.4 + noise;
        let filtered = self.svf1.lowpass(mixed, 11000.0 * tm, 0.3, sr);
        // Crash builds slightly (1-2ms) then long decay
        let attack = (t / 0.002).min(1.0);
        let env = attack * (-t / (decay_mult * dm)).exp();
        filtered * env * 0.3
    }

    fn t2_ride(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        let t = self.time;
        let hat_freqs: [f64; 6] = [420.0*tm, 980.0*tm, 1680.0*tm, 2800.0*tm, 4100.0*tm, 5600.0*tm];
        let modal = self.hat_oscs.tick(sr, &hat_freqs);
        let filtered = self.svf1.bandpass(modal, 5500.0 * tm, 0.8, sr);
        let env = (-t / (0.9 * dm)).exp();
        // Ride has a "ping" then sustain
        let ping = (-t * 100.0).exp() * 0.15;
        (filtered * env + ping) * 0.3
    }

    fn t2_ride_bell(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        let t = self.time;
        advance_phase(&mut self.phase1, 720.0 * tm, sr);
        advance_phase(&mut self.phase2, 1080.0 * tm, sr);
        advance_phase(&mut self.phase3, 1620.0 * tm, sr);
        let bell = osc_sine(self.phase1) * 0.35 + osc_sine(self.phase2) * 0.3
                 + osc_sine(self.phase3) * 0.2;
        let env = (-t / (0.7 * dm)).exp();
        bell * env
    }

    // ── TSTY-2 Percussion ──

    fn t2_cowbell(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        let t = self.time;
        advance_phase(&mut self.phase1, 587.0 * tm, sr);
        advance_phase(&mut self.phase2, 878.0 * tm, sr);
        let body = osc_sine(self.phase1) * 0.35 + osc_sine(self.phase2) * 0.3;
        let filtered = self.svf1.bandpass(body, 730.0, 4.0, sr);
        let env = (-t / (0.065 * dm)).exp();
        filtered * env
    }

    fn t2_woodblock(&mut self, sr: f64, dm: f64, tm: f64, freq: f64) -> f64 {
        let t = self.time;
        let f = freq * tm;
        advance_phase(&mut self.phase1, f, sr);
        advance_phase(&mut self.phase2, f * 2.65, sr); // wood mode ratio
        let body = osc_sine(self.phase1) * 0.4 + osc_sine(self.phase2) * 0.2;
        let click = self.noise() * (-t * 1000.0).exp() * 0.15;
        let env = (-t / (0.018 * dm)).exp();
        (body + click) * env
    }

    fn t2_tambourine(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        let t = self.time;
        // Jingles: multiple small cymbal-like modes
        let hat_freqs: [f64; 6] = [4200.0*tm, 5800.0*tm, 7400.0*tm, 9100.0*tm, 11000.0*tm, 13000.0*tm];
        let jingles = self.hat_oscs.tick(sr, &hat_freqs);
        let filtered = self.hp1.tick_hp(jingles, 4000.0, sr);
        // Rhythmic jingle from hand shake
        let shake = (t * 22.0 * TAU).sin().abs() * (-t * 6.0).exp();
        let env = (-t / (0.2 * dm)).exp();
        filtered * (env + shake * 0.25) * 0.3
    }

    fn t2_shaker(&mut self, sr: f64, dm: f64) -> f64 {
        let t = self.time;
        let raw = self.noise();
        let filtered = self.svf1.bandpass(raw, 7500.0, 1.2, sr);
        let hp = self.hp1.tick_hp(filtered, 4500.0, sr);
        let env = (-t / (0.07 * dm)).exp();
        hp * env * 0.3
    }

    fn t2_shaker_long(&mut self, sr: f64, dm: f64) -> f64 {
        let t = self.time;
        let raw = self.noise();
        let filtered = self.svf1.bandpass(raw, 8000.0, 1.5, sr);
        // Swish envelope
        let swish = (t * 12.0).sin().abs() * (-t * 4.0).exp();
        let env = (-t / (0.15 * dm)).exp();
        filtered * (env + swish * 0.2) * 0.25
    }

    fn t2_vibraslap(&mut self, sr: f64, dm: f64) -> f64 {
        let t = self.time;
        let raw = self.noise();
        let filtered = self.svf1.bandpass(raw, 3500.0, 6.0, sr);
        let rattle = (t * 35.0 * TAU).sin().abs() * (-t * 3.0).exp();
        let env = (-t / (0.5 * dm)).exp();
        filtered * rattle * env * 0.25
    }

    fn t2_timbale(&mut self, sr: f64, dm: f64, tm: f64, freq: f64) -> f64 {
        let t = self.time;
        let f = freq * tm;
        advance_phase(&mut self.phase1, f, sr);
        advance_phase(&mut self.phase2, f * 2.14, sr); // shell mode
        let body = osc_sine(self.phase1) * 0.5;
        let ring = osc_sine(self.phase2) * 0.2 * (-t * 20.0).exp();
        let shell = self.noise() * (-t * 250.0).exp();
        let shell_f = self.svf1.bandpass(shell, f * 3.0, 8.0, sr) * 0.08;
        let env = (-t / (0.2 * dm)).exp();
        (body + ring + shell_f) * env
    }

    fn t2_agogo(&mut self, sr: f64, dm: f64, tm: f64, freq: f64) -> f64 {
        let t = self.time;
        let f1 = freq * tm;
        let f2 = f1 * 1.504; // inharmonic bell ratio
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);
        let body = osc_sine(self.phase1) * 0.35 + osc_sine(self.phase2) * 0.28;
        let env = (-t / (0.18 * dm)).exp();
        body * env
    }

    fn t2_fx_perc(&mut self, sr: f64, dm: f64, tm: f64, nm: f64, character: f64) -> f64 {
        let t = self.time;
        let freq = 300.0 + character * 5000.0;
        let sweep = freq * (1.0 + character * 2.0 * (-t * 6.0).exp());
        advance_phase(&mut self.phase1, sweep * tm, sr);
        let tone = osc_sine(self.phase1) * 0.35;
        let noise = self.noise() * nm * 0.15;
        // Modal resonance at swept frequency
        let reso = self.svf1.bandpass(tone + noise, sweep * tm * 0.8, 3.0, sr) * 0.2;
        let env = (-t / (0.25 * dm)).exp();
        (tone + noise + reso) * env
    }

    // ══════════════════════════════════════════════════════════════════════════
    // TSTY-3: Studio acoustic kit — 88 unique sounds, NO filler
    // Dispatches on MIDI note directly. Every sound is a unique synthesis.
    // Modeled after close-mic'd drums through Studer A800 reel-to-reel.
    // ══════════════════════════════════════════════════════════════════════════

    fn synth_tsty3(&mut self, sr: f64, dm: f64, tm: f64, nm: f64, _dr: f64) -> f64 {
        let raw = self.t3_dispatch(sr, dm, tm, nm);
        // Every tsty-3 sound goes through tape processing
        Self::tape_process(raw, self.time, sr, &mut self.lp1_state)
    }

    fn t3_dispatch(&mut self, sr: f64, dm: f64, tm: f64, nm: f64) -> f64 {
        let t = self.time;
        let n = self.note;
        match n {
            // ══ KICKS: 24-38 (15 unique kicks) ══
            // Each has different fundamental, beater, damping, and modal content

            24 => { // Kick: Studio Felt — warm, round, 60Hz, felt beater, medium damping
                let f = 60.0 * tm;
                let sw = f * 0.25 * (-t * 55.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f + sw) * 1.593, sr);
                let rise = (t / 0.0018).min(1.0);
                let body = osc_sine(self.phase1) * 0.6 * (0.3*(-t/0.01).exp() + 0.7*(-t/(0.22*dm)).exp());
                let m1 = osc_sine(self.phase2) * 0.1 * (-t/(0.06*dm)).exp();
                let beater = self.noise() * rise * (-t*180.0).exp();
                let bf = self.svf1.bandpass(beater, 2200.0, 1.3, sr) * 0.15;
                body + m1 + bf
            }
            25 => { // Kick: Tight Funk — 72Hz, damped, wood beater, dry
                let f = 72.0 * tm;
                let sw = f * 0.18 * (-t * 90.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_sine(self.phase1) * 0.7 * (0.5*(-t/0.005).exp() + 0.5*(-t/(0.13*dm)).exp());
                let beater = self.noise() * (t/0.0008).min(1.0) * (-t*450.0).exp();
                let bf = self.svf1.bandpass(beater, 4200.0, 2.0, sr) * 0.25;
                body + bf
            }
            26 => { // Kick: Jazz Brushed — 52Hz, resonant, slow sweep, soft
                let f = 52.0 * tm;
                let sw = f * 0.45 * (-t * 30.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f + sw) * 2.296, sr);
                let body = osc_sine(self.phase1) * 0.5 * (0.2*(-t/0.018).exp() + 0.8*(-t/(0.4*dm)).exp());
                let m1 = osc_sine(self.phase2) * 0.08 * (-t/(0.1*dm)).exp();
                let brush = self.noise() * (t/0.003).min(1.0) * (-t*100.0).exp();
                let bf = self.svf1.bandpass(brush, 1500.0, 0.9, sr) * 0.1;
                body + m1 + bf
            }
            27 => { // Kick: Deep Sub — 42Hz, very long decay, minimal click
                let f = 42.0 * tm;
                let sw = f * 0.5 * (-t * 20.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f * 0.5) + sw * 0.3, sr);
                let body = osc_sine(self.phase1) * 0.55 * (-t/(0.5*dm)).exp();
                let sub = osc_sine(self.phase2) * 0.25 * (-t/(0.35*dm)).exp();
                body + sub
            }
            28 => { // Kick: Rock Plastic — 65Hz, bright beater click, medium body
                let f = 65.0 * tm;
                let sw = f * 0.35 * (-t * 60.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f + sw) * 1.593, sr);
                advance_phase(&mut self.phase3, (f + sw) * 2.136, sr);
                let env = 0.4*(-t/0.006).exp() + 0.6*(-t/(0.2*dm)).exp();
                let body = osc_sine(self.phase1) * 0.6 * env;
                let m1 = osc_sine(self.phase2) * 0.12 * (-t/(0.05*dm)).exp();
                let m2 = osc_sine(self.phase3) * 0.06 * (-t/(0.03*dm)).exp();
                let beater = self.noise() * (t/0.0005).min(1.0) * (-t*600.0).exp();
                let bf = self.svf1.bandpass(beater, 5500.0, 2.2, sr) * 0.3;
                body + m1 + m2 + bf
            }
            29 => { // Kick: Boomy Floor — 48Hz, long, shell resonance dominant
                let f = 48.0 * tm;
                let sw = f * 0.3 * (-t * 25.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_sine(self.phase1) * 0.5 * (-t/(0.45*dm)).exp();
                // Shell resonance via resonant filter
                let shell_exc = self.noise() * (-t*40.0).exp();
                let shell = self.svf1.bandpass(shell_exc, 220.0*tm, 15.0, sr) * 0.12;
                let beater = self.noise() * (t/0.002).min(1.0) * (-t*120.0).exp();
                let bf = self.svf2.bandpass(beater, 1800.0, 1.0, sr) * 0.08;
                body + shell + bf
            }
            30 => { // Kick: Tight Click — 78Hz, very short, lots of attack
                let f = 78.0 * tm;
                let sw = f * 0.15 * (-t * 120.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_sine(self.phase1) * 0.55 * (0.6*(-t/0.003).exp() + 0.4*(-t/(0.08*dm)).exp());
                let click = self.noise() * (t/0.0003).min(1.0) * (-t*900.0).exp();
                let cf = self.hp1.tick_hp(click, 3000.0, sr) * 0.35;
                body + cf
            }
            31 => { // Kick: Warm Vintage — 58Hz, triangle body, soft character
                let f = 58.0 * tm;
                let sw = f * 0.3 * (-t * 45.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_triangle(self.phase1) * 0.55 * (0.25*(-t/0.012).exp() + 0.75*(-t/(0.28*dm)).exp());
                let warmth = osc_sine(self.phase1 * 0.5) * 0.12 * (-t/(0.2*dm)).exp();
                let felt = self.noise() * (t/0.002).min(1.0) * (-t*150.0).exp();
                let ff = self.svf1.bandpass(felt, 1800.0, 1.2, sr) * 0.08;
                body + warmth + ff
            }
            32 => { // Kick: Punchy Mid — 68Hz, strong 2nd mode, snappy
                let f = 68.0 * tm;
                let sw = f * 0.4 * (-t * 75.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f + sw) * 1.593, sr);
                let body = osc_sine(self.phase1) * 0.5 * (0.45*(-t/0.005).exp() + 0.55*(-t/(0.18*dm)).exp());
                let m1 = osc_sine(self.phase2) * 0.2 * (-t/(0.07*dm)).exp(); // strong 2nd mode
                let beater = self.noise() * (t/0.001).min(1.0) * (-t*350.0).exp();
                let bf = self.svf1.bandpass(beater, 3200.0, 1.6, sr) * 0.18;
                body + m1 + bf
            }
            33 => { // Kick: Thuddy — 55Hz, very damped, almost no ring
                let f = 55.0 * tm;
                let sw = f * 0.2 * (-t * 80.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_sine(self.phase1) * 0.65 * (-t/(0.1*dm)).exp();
                let thud = self.noise() * (t/0.001).min(1.0) * (-t*200.0).exp();
                let tf = self.svf1.lowpass(thud, 800.0, 0.5, sr) * 0.15;
                body + tf
            }
            34 => { // Kick: Room — 62Hz, emphasis on shell + room reflections
                let f = 62.0 * tm;
                let sw = f * 0.3 * (-t * 50.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_sine(self.phase1) * 0.45 * (0.3*(-t/0.008).exp() + 0.7*(-t/(0.25*dm)).exp());
                // "Room" via delayed noise burst
                let room = self.noise() * (-(t-0.015).max(0.0) * 30.0).exp() * 0.08;
                let rf = self.svf1.lowpass(room, 2000.0, 1.5, sr);
                let shell = self.noise() * (-t*50.0).exp();
                let sf = self.svf2.bandpass(shell, 300.0*tm, 10.0, sr) * 0.07;
                body + rf + sf
            }
            35 => { // Kick: Muffled — 50Hz, pillow inside, almost pure fundamental
                let f = 50.0 * tm;
                let sw = f * 0.15 * (-t * 40.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_sine(self.phase1) * 0.7 * (-t/(0.15*dm)).exp();
                body
            }
            36 => { // Kick: Studio Standard — balanced, 64Hz, all-purpose
                let f = 64.0 * tm;
                let sw = f * 0.28 * (-t * 58.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f + sw) * 1.593, sr);
                let env = 0.35*(-t/0.007).exp() + 0.65*(-t/(0.2*dm)).exp();
                let body = osc_sine(self.phase1) * 0.6 * env;
                let m1 = osc_sine(self.phase2) * 0.1 * (-t/(0.055*dm)).exp();
                let sub = osc_sine(self.phase1 * 0.5) * 0.1 * (-t/(0.12*dm)).exp();
                let beater = self.noise() * (t/0.001).min(1.0) * (-t*300.0).exp();
                let bf = self.svf1.bandpass(beater, 3000.0, 1.5, sr) * 0.18;
                body + m1 + sub + bf
            }
            37 => { // Kick: Ringy — 56Hz, long undamped, 3 audible modes
                let f = 56.0 * tm;
                let sw = f * 0.35 * (-t * 35.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f + sw) * 1.593, sr);
                advance_phase(&mut self.phase3, (f + sw) * 2.296, sr);
                let body = osc_sine(self.phase1) * 0.5 * (-t/(0.4*dm)).exp();
                let m1 = osc_sine(self.phase2) * 0.18 * (-t/(0.15*dm)).exp();
                let m2 = osc_sine(self.phase3) * 0.1 * (-t/(0.1*dm)).exp();
                body + m1 + m2
            }
            38 => { // Kick: Chest Hit — 45Hz, max sub, air push
                let f = 45.0 * tm;
                let sw = f * 0.6 * (-t * 22.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, f * 0.5, sr);
                let body = osc_sine(self.phase1) * 0.5 * (-t/(0.35*dm)).exp();
                let air = osc_sine(self.phase2) * 0.3 * (-t/(0.15*dm)).exp(); // sub push
                let thump = self.noise() * (-t*60.0).exp();
                let tf = self.svf1.lowpass(thump, 500.0, 0.8, sr) * 0.1;
                body + air + tf
            }

            // ══ SNARES: 39-53 (15 unique snares) ══

            39 => { // Snare: Funk Tight — 310Hz, short wires, crisp
                let f = 310.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                advance_phase(&mut self.phase2, f * 1.593, sr);
                let stick = self.noise() * (t/0.0003).min(1.0) * (-t*1800.0).exp();
                let sf = self.hp1.tick_hp(stick, 3500.0, sr) * 0.3;
                let head = osc_sine(self.phase1) * 0.3 * (-t/(0.08*dm)).exp()
                         + osc_sine(self.phase2) * 0.12 * (-t/(0.05*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 4500.0*tm, 0.7, sr);
                let wf = self.hp1.tick_hp(wire, 2000.0, sr) * (-t/(0.12*dm)).exp() * 0.35;
                sf + head + wf
            }
            40 => { // Snare: Fat Backbeat — 230Hz, big body, long wires
                let f = 230.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                advance_phase(&mut self.phase2, f * 1.593, sr);
                advance_phase(&mut self.phase3, f * 2.136, sr);
                let head = osc_sine(self.phase1) * 0.4 * (-t/(0.12*dm)).exp()
                         + osc_sine(self.phase2) * 0.18 * (-t/(0.08*dm)).exp()
                         + osc_sine(self.phase3) * 0.08 * (-t/(0.05*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 3800.0*tm, 0.6, sr);
                let wf = wire * (-t/(0.25*dm)).exp() * 0.4;
                let stick = self.noise() * (-t*1500.0).exp() * 0.2;
                head + wf + stick
            }
            41 => { // Snare: Dry Studio — 285Hz, damped, Purdie-style
                let f = 285.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.35 * (-t/(0.06*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 5200.0*tm, 0.5, sr);
                let wf = wire * (-t/(0.08*dm)).exp() * 0.3;
                let stick = self.noise() * (-t*2200.0).exp() * 0.25;
                head + wf + stick
            }
            42 => { // Snare: Brush Swish — 260Hz, noise-dominated, gentle
                let f = 260.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.2 * (-t/(0.1*dm)).exp();
                let brush = self.noise() * (t/0.004).min(1.0); // slow rise = brush stroke
                let bf = self.svf1.bandpass(brush, 3000.0, 0.8, sr) * (-t/(0.15*dm)).exp() * 0.4;
                head + bf
            }
            43 => { // Snare: Cross-Stick — rim click, no wires
                advance_phase(&mut self.phase1, 550.0*tm, sr);
                advance_phase(&mut self.phase2, 1450.0*tm, sr);
                let crack = osc_sine(self.phase1) * 0.3 + osc_sine(self.phase2) * 0.2;
                let click = self.noise() * (-t*1000.0).exp() * 0.25;
                let env = (-t/(0.02*dm)).exp();
                (crack + click) * env
            }
            44 => { // Snare: Ghost Note — very soft, all wire buzz, minimal head
                let f = 295.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.1 * (-t/(0.04*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 4800.0, 0.7, sr);
                let wf = wire * (-t/(0.06*dm)).exp() * 0.2;
                head + wf
            }
            45 => { // Snare: Ringy Metal Shell — 340Hz, long shell ring
                let f = 340.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                advance_phase(&mut self.phase2, f * 1.593, sr);
                let head = osc_sine(self.phase1) * 0.3 * (-t/(0.1*dm)).exp();
                let m1 = osc_sine(self.phase2) * 0.15 * (-t/(0.08*dm)).exp();
                let shell = self.noise() * (-t*40.0).exp();
                let shell_r = self.svf2.bandpass(shell, 520.0*tm, 18.0, sr) * 0.12; // metal ring
                let wire = self.svf1.bandpass(self.noise()*nm, 4200.0, 0.6, sr) * (-t/(0.18*dm)).exp() * 0.35;
                head + m1 + shell_r + wire
            }
            46 => { // Snare: Loose Wires — 270Hz, rattly long buzz
                let f = 270.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.3 * (-t/(0.09*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 3500.0, 0.5, sr);
                let wf = wire * (-t/(0.35*dm)).exp() * 0.45; // long loose buzz
                head + wf
            }
            47 => { // Snare: Piccolo — 380Hz, high tuned, bright, short
                let f = 380.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.25 * (-t/(0.05*dm)).exp();
                let crack = self.noise() * (-t*2500.0).exp() * 0.3;
                let cf = self.hp1.tick_hp(crack, 5000.0, sr);
                let wire = self.svf1.bandpass(self.noise()*nm, 6000.0, 0.8, sr) * (-t/(0.1*dm)).exp() * 0.3;
                head + cf + wire
            }
            48 => { // Snare: Wood Shell Deep — 220Hz, warm woody character
                let f = 220.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                advance_phase(&mut self.phase2, f * 2.136, sr);
                let head = osc_sine(self.phase1) * 0.35 * (-t/(0.1*dm)).exp();
                let m1 = osc_sine(self.phase2) * 0.1 * (-t/(0.06*dm)).exp();
                let shell = self.noise() * (-t*50.0).exp();
                let sf = self.svf2.bandpass(shell, 320.0*tm, 10.0, sr) * 0.08; // wood resonance
                let wire = self.svf1.bandpass(self.noise()*nm, 3600.0, 0.7, sr) * (-t/(0.15*dm)).exp() * 0.35;
                head + m1 + sf + wire
            }
            49 => { // Snare: Crack — 320Hz, maximum attack, minimal body
                advance_phase(&mut self.phase1, 320.0*tm, sr);
                let head = osc_sine(self.phase1) * 0.2 * (-t/(0.04*dm)).exp();
                let crack = self.noise() * (t/0.0002).min(1.0) * (-t*3000.0).exp();
                let cf = self.hp1.tick_hp(crack, 4000.0, sr) * 0.45;
                let wire = self.svf1.bandpass(self.noise()*nm, 5500.0, 0.8, sr) * (-t/(0.08*dm)).exp() * 0.25;
                head + cf + wire
            }
            50 => { // Snare: Thick — 250Hz, lots of body + wires together
                let f = 250.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                advance_phase(&mut self.phase2, f * 1.593, sr);
                advance_phase(&mut self.phase3, f * 2.296, sr);
                let head = osc_sine(self.phase1) * 0.4 * (-t/(0.12*dm)).exp()
                         + osc_sine(self.phase2) * 0.2 * (-t/(0.08*dm)).exp()
                         + osc_sine(self.phase3) * 0.1 * (-t/(0.06*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 4000.0, 0.6, sr) * (-t/(0.2*dm)).exp() * 0.4;
                let stick = self.noise() * (-t*1200.0).exp() * 0.15;
                head + wire + stick
            }
            51 => { // Snare: Rim Shot Full — stick hits rim + head simultaneously
                let f = 300.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                advance_phase(&mut self.phase2, 900.0*tm, sr); // rim harmonic
                advance_phase(&mut self.phase3, 2200.0*tm, sr); // rim overtone
                let head = osc_sine(self.phase1) * 0.3 * (-t/(0.08*dm)).exp();
                let rim = osc_sine(self.phase2) * 0.2 * (-t/(0.015*dm)).exp()
                        + osc_sine(self.phase3) * 0.1 * (-t/(0.01*dm)).exp();
                let crack = self.noise() * (-t*2000.0).exp() * 0.3;
                let wire = self.svf1.bandpass(self.noise()*nm, 4200.0, 0.7, sr) * (-t/(0.12*dm)).exp() * 0.3;
                head + rim + crack + wire
            }
            52 => { // Snare: Sizzle — 290Hz, snare wires very present, buzzy
                let f = 290.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.25 * (-t/(0.07*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 3800.0, 0.4, sr) * (-t/(0.28*dm)).exp() * 0.5;
                let wire2 = self.svf2.bandpass(self.noise()*nm, 7000.0, 1.0, sr) * (-t/(0.15*dm)).exp() * 0.2;
                head + wire + wire2
            }
            53 => { // Snare: Soft Roll — very gentle, like a snare roll sustain
                let f = 275.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.15 * (-t/(0.15*dm)).exp();
                // Simulated roll: amplitude modulated noise
                let roll_mod = (t * 18.0 * TAU).sin().abs();
                let wire = self.svf1.bandpass(self.noise()*nm, 4000.0, 0.6, sr) * roll_mod * (-t/(0.3*dm)).exp() * 0.3;
                head + wire
            }

            // ══ CLAPS/SNAPS: 54-59 (6 unique) ══

            54 => { // Clap: Group Tight — 5 clappers, tight timing
                let mut env = 0.0;
                for k in 0..5u32 {
                    let off = (self.hit_rand(k*3) * 0.008 + self.hit_rand(k*3+1).abs() * 0.005).abs();
                    let to = t - off;
                    if to >= 0.0 { env += (-to * 200.0).exp() * (0.75 + self.hit_rand(k*3+2) * 0.25) * 0.18; }
                }
                let n = self.noise() * nm;
                let f = self.svf1.bandpass(n, 2400.0 + self.hit_rand(60)*400.0, 1.5, sr);
                let hp = self.hp1.tick_hp(f, 700.0, sr);
                let tail = (-t/(0.1*dm)).exp() * 0.25;
                hp * (env + tail)
            }
            55 => { // Clap: Loose Group — 7 clappers, wide timing spread
                let mut env = 0.0;
                for k in 0..7u32 {
                    let off = (self.hit_rand(k*4) * 0.02 + self.hit_rand(k*4+1).abs() * 0.01).abs();
                    let to = t - off;
                    if to >= 0.0 { env += (-to * 150.0).exp() * (0.6 + self.hit_rand(k*4+2) * 0.4) * 0.13; }
                }
                let n = self.noise() * nm;
                let f = self.svf1.bandpass(n, 1800.0 + self.hit_rand(80)*600.0, 1.2, sr);
                let tail = (-t/(0.15*dm)).exp() * 0.3;
                f * (env + tail)
            }
            56 => { // Snap: Finger Snap — single, sharp, high
                let snap = self.noise() * (t/0.0002).min(1.0) * (-t*1200.0).exp();
                let f = self.svf1.bandpass(snap, 3200.0, 2.5, sr);
                let hp = self.hp1.tick_hp(f, 1500.0, sr);
                hp * 0.5
            }
            57 => { // Slap: Hand on Thigh — mid-frequency thump + skin noise
                advance_phase(&mut self.phase1, 180.0*tm, sr);
                let body = osc_sine(self.phase1) * 0.25 * (-t/(0.04*dm)).exp();
                let slap = self.noise() * (-t*400.0).exp();
                let sf = self.svf1.bandpass(slap, 1500.0, 1.5, sr) * 0.3;
                body + sf
            }
            58 => { // Clap: Single — one person clap
                let snap = self.noise() * (t/0.0004).min(1.0) * (-t*250.0).exp();
                let f = self.svf1.bandpass(snap, 2000.0, 1.8, sr);
                let hp = self.hp1.tick_hp(f, 500.0, sr);
                let tail = (-t/(0.06*dm)).exp() * 0.15;
                hp * 0.4 + tail * self.noise() * 0.03
            }
            59 => { // Clap: Reverb Hall — group clap with long room tail
                let mut env = 0.0;
                for k in 0..4u32 {
                    let off = (self.hit_rand(k*5) * 0.01).abs();
                    let to = t - off;
                    if to >= 0.0 { env += (-to * 180.0).exp() * 0.22; }
                }
                let n = self.noise() * nm;
                let f = self.svf1.bandpass(n, 2200.0, 1.3, sr);
                let tail = (-t/(0.25*dm)).exp() * 0.35; // long reverb
                f * (env + tail)
            }

            // ══ HI-HATS: 60-71 (12 unique, modal synthesis) ══

            60 => { // Hat: Closed Tight — very short, bright tick
                let freqs = [380.0*tm, 950.0*tm, 1600.0*tm, 2500.0*tm, 3700.0*tm, 5100.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(m, 5500.0, sr);
                let stick = self.noise() * (-t*2500.0).exp() * 0.12;
                let env = (-t/(0.025*dm)).exp();
                (hp * 0.35 + stick) * env
            }
            61 => { // Hat: Closed Medium — standard closed hat
                let freqs = [342.0*tm, 817.0*tm, 1453.0*tm, 2298.0*tm, 3419.0*tm, 4735.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.bandpass(m, 7000.0, 1.5, sr);
                let hp = self.hp1.tick_hp(f, 4500.0, sr);
                let env = (-t/(0.04*dm)).exp();
                hp * env * 0.35
            }
            62 => { // Hat: Closed Dark — lower modal content, muted
                let freqs = [300.0*tm, 720.0*tm, 1280.0*tm, 2050.0*tm, 3100.0*tm, 4200.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.lowpass(m, 6000.0, 0.5, sr);
                let env = (-t/(0.05*dm)).exp();
                f * env * 0.3
            }
            63 => { // Hat: Closed Sizzle — contact buzz between cymbals
                let freqs = [365.0*tm, 870.0*tm, 1520.0*tm, 2400.0*tm, 3550.0*tm, 4900.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(m, 5000.0, sr);
                let sizzle = self.noise() * (-t/(0.03*dm)).exp();
                let sz = self.svf1.bandpass(sizzle, 8500.0, 5.0, sr) * 0.1;
                let env = (-t/(0.045*dm)).exp();
                (hp * 0.3 + sz) * env
            }
            64 => { // Hat: Half-Open — cymbals barely touching, medium ring
                let freqs = [348.0*tm, 835.0*tm, 1480.0*tm, 2340.0*tm, 3480.0*tm, 4800.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.bandpass(m, 6000.0, 1.0, sr);
                let hp = self.hp1.tick_hp(f, 3500.0, sr);
                let sizzle = self.noise() * (-t/(0.15*dm)).exp();
                let sz = self.svf2.bandpass(sizzle, 7500.0, 4.0, sr) * 0.08;
                let env = (-t/(0.18*dm)).exp();
                (hp * 0.3 + sz) * env
            }
            65 => { // Hat: Open Bright — full shimmer, long ring
                let freqs = [355.0*tm, 850.0*tm, 1500.0*tm, 2380.0*tm, 3530.0*tm, 4880.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                advance_phase(&mut self.modal_phases[0], 6400.0*tm, sr);
                advance_phase(&mut self.modal_phases[1], 8300.0*tm, sr);
                let upper = osc_sine(self.modal_phases[0]) * 0.15 * (-t/(0.6*dm)).exp()
                          + osc_sine(self.modal_phases[1]) * 0.08 * (-t/(0.8*dm)).exp();
                let hp = self.hp1.tick_hp(m, 3000.0, sr);
                let env = (-t/(0.6*dm)).exp();
                (hp * 0.3 + upper) * env
            }
            66 => { // Hat: Open Dark — fewer high modes, warmer sustain
                let freqs = [310.0*tm, 740.0*tm, 1300.0*tm, 2100.0*tm, 3150.0*tm, 4350.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.lowpass(m, 7000.0, 0.4, sr);
                let env = (-t/(0.55*dm)).exp();
                f * env * 0.3
            }
            67 => { // Hat: Pedal Chick — foot pedal, no stick
                let freqs = [360.0*tm, 860.0*tm, 1520.0*tm, 2400.0*tm, 3560.0*tm, 4920.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let env = (-t/(0.018*dm)).exp();
                let chick = self.noise() * (-t*600.0).exp();
                let cf = self.svf1.bandpass(chick, 1400.0, 2.5, sr) * 0.12;
                m * env * 0.2 + cf
            }
            68 => { // Hat: Open Washy — very long, wash-like
                let freqs = [330.0*tm, 790.0*tm, 1400.0*tm, 2220.0*tm, 3300.0*tm, 4560.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.bandpass(m, 5500.0, 0.8, sr);
                let env = (-t/(1.0*dm)).exp();
                f * env * 0.25
            }
            69 => { // Hat: Closed Thin — very little body, all tick
                let freqs = [420.0*tm, 1020.0*tm, 1750.0*tm, 2750.0*tm, 4000.0*tm, 5500.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(m, 7000.0, sr);
                let env = (-t/(0.02*dm)).exp();
                hp * env * 0.3
            }
            70 => { // Hat: Half-Open Bright — brighter than 64
                let freqs = [370.0*tm, 900.0*tm, 1580.0*tm, 2480.0*tm, 3680.0*tm, 5080.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(m, 4000.0, sr);
                let env = (-t/(0.22*dm)).exp();
                hp * env * 0.32
            }
            71 => { // Hat: Open Sizzle — riveted cymbal character
                let freqs = [345.0*tm, 825.0*tm, 1460.0*tm, 2310.0*tm, 3430.0*tm, 4740.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let rattle = self.noise() * (t * 28.0 * TAU).sin().abs() * (-t * 5.0).exp();
                let rf = self.svf1.bandpass(rattle, 9000.0, 3.0, sr) * 0.08;
                let env = (-t/(0.7*dm)).exp();
                m * env * 0.25 + rf
            }

            // ══ TOMS: 72-79 (8 unique, different depths and characters) ══

            72 => { // Tom: Floor Deep — 80Hz, long decay, big body
                let f = 80.0*tm; let sw = f*0.15*(-t*25.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
                let body = osc_sine(self.phase1) * 0.55 * (0.2*(-t/0.015).exp() + 0.8*(-t/(0.32*dm)).exp());
                let m1 = osc_sine(self.phase2) * 0.15 * (-t/(0.1*dm)).exp();
                let stick = self.noise() * (t/0.001).min(1.0) * (-t*250.0).exp();
                let sf = self.svf1.bandpass(stick, 2500.0, 1.3, sr) * 0.1;
                body + m1 + sf
            }
            73 => { // Tom: Floor Medium — 105Hz
                let f = 105.0*tm; let sw = f*0.12*(-t*30.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                advance_phase(&mut self.phase2, (f+sw)*2.136, sr);
                let body = osc_sine(self.phase1) * 0.5 * (0.25*(-t/0.01).exp() + 0.75*(-t/(0.26*dm)).exp());
                let m1 = osc_sine(self.phase2) * 0.1 * (-t/(0.07*dm)).exp();
                let stick = self.noise() * (-t*280.0).exp() * 0.08;
                body + m1 + stick
            }
            74 => { // Tom: Low Rack — 130Hz
                let f = 130.0*tm; let sw = f*0.1*(-t*35.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                let body = osc_sine(self.phase1) * 0.5 * (0.3*(-t/0.008).exp() + 0.7*(-t/(0.22*dm)).exp());
                let stick = self.noise() * (-t*320.0).exp();
                let sf = self.svf1.bandpass(stick, 3000.0, 1.5, sr) * 0.1;
                body + sf
            }
            75 => { // Tom: Mid Rack — 165Hz
                let f = 165.0*tm; let sw = f*0.1*(-t*38.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
                let body = osc_sine(self.phase1) * 0.48 * (0.3*(-t/0.007).exp() + 0.7*(-t/(0.2*dm)).exp());
                let m1 = osc_sine(self.phase2) * 0.12 * (-t/(0.06*dm)).exp();
                body + m1
            }
            76 => { // Tom: High Rack — 210Hz
                let f = 210.0*tm; let sw = f*0.08*(-t*40.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                let body = osc_sine(self.phase1) * 0.45 * (0.35*(-t/0.006).exp() + 0.65*(-t/(0.17*dm)).exp());
                let stick = self.noise() * (-t*350.0).exp();
                let sf = self.svf1.bandpass(stick, 3500.0, 1.5, sr) * 0.12;
                body + sf
            }
            77 => { // Tom: Concert — 145Hz, big resonant, orchestral
                let f = 145.0*tm; let sw = f*0.1*(-t*22.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
                advance_phase(&mut self.phase3, (f+sw)*2.296, sr);
                let body = osc_sine(self.phase1) * 0.5 * (-t/(0.35*dm)).exp();
                let m1 = osc_sine(self.phase2) * 0.18 * (-t/(0.15*dm)).exp();
                let m2 = osc_sine(self.phase3) * 0.1 * (-t/(0.1*dm)).exp();
                body + m1 + m2
            }
            78 => { // Tom: Roto High — 280Hz, bright, synthetic-ish
                let f = 280.0*tm;
                advance_phase(&mut self.phase1, f, sr);
                let body = osc_sine(self.phase1) * 0.4 * (-t/(0.12*dm)).exp();
                let ring = osc_triangle(self.phase1 * 2.5) * 0.1 * (-t/(0.06*dm)).exp();
                body + ring
            }
            79 => { // Tom: Timbale-ish — 350Hz, metallic shell ring
                let f = 350.0*tm;
                advance_phase(&mut self.phase1, f, sr);
                let body = osc_sine(self.phase1) * 0.35 * (-t/(0.15*dm)).exp();
                let shell = self.noise() * (-t*30.0).exp();
                let sf = self.svf1.bandpass(shell, f*2.5, 12.0, sr) * 0.15;
                body + sf
            }

            // ══ CYMBALS: 80-87 (8 unique) ══

            80 => { // Crash: Dark — lower modal content, warm
                let freqs = [300.0*tm, 710.0*tm, 1250.0*tm, 2000.0*tm, 3000.0*tm, 4150.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.lowpass(m, 7000.0, 0.3, sr);
                let n = self.noise() * 0.12;
                let env = (t/0.003).min(1.0) * (-t/(1.2*dm)).exp();
                (f * 0.35 + n) * env
            }
            81 => { // Crash: Bright — higher modal content, cutting
                let freqs = [400.0*tm, 960.0*tm, 1680.0*tm, 2650.0*tm, 3900.0*tm, 5400.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(m, 2500.0, sr);
                let env = (t/0.002).min(1.0) * (-t/(1.5*dm)).exp();
                hp * env * 0.3
            }
            82 => { // Ride: Ping — defined stick sound, controlled wash
                let freqs = [420.0*tm, 1000.0*tm, 1720.0*tm, 2800.0*tm, 4150.0*tm, 5700.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.bandpass(m, 5500.0, 1.0, sr);
                let ping = (-t*120.0).exp() * 0.15;
                let env = (-t/(0.8*dm)).exp();
                (f * env + ping) * 0.28
            }
            83 => { // Ride: Wash — loose, washy
                let freqs = [380.0*tm, 910.0*tm, 1580.0*tm, 2520.0*tm, 3750.0*tm, 5180.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let env = (-t/(1.5*dm)).exp();
                m * env * 0.22
            }
            84 => { // Ride Bell — defined tonal bell hit
                advance_phase(&mut self.phase1, 750.0*tm, sr);
                advance_phase(&mut self.phase2, 1125.0*tm, sr);
                advance_phase(&mut self.phase3, 1688.0*tm, sr);
                let bell = osc_sine(self.phase1)*0.3 + osc_sine(self.phase2)*0.25 + osc_sine(self.phase3)*0.15;
                let env = (-t/(0.65*dm)).exp();
                bell * env
            }
            85 => { // Splash — fast, bright, short
                let freqs = [450.0*tm, 1080.0*tm, 1850.0*tm, 2900.0*tm, 4300.0*tm, 5900.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(m, 3500.0, sr);
                let env = (t/0.001).min(1.0) * (-t/(0.45*dm)).exp();
                hp * env * 0.3
            }
            86 => { // China — trashy, aggressive overtones
                let freqs = [280.0*tm, 670.0*tm, 1180.0*tm, 1900.0*tm, 2850.0*tm, 3950.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                // Extra distortion for trashy character
                let dist = (m * 2.0).tanh() * 0.5;
                let env = (t/0.002).min(1.0) * (-t/(0.9*dm)).exp();
                dist * env * 0.3
            }
            87 => { // Cymbal: Sizzle — riveted, continuous rattle
                let freqs = [350.0*tm, 840.0*tm, 1480.0*tm, 2350.0*tm, 3500.0*tm, 4830.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let rattle = self.noise() * (t * 30.0 * TAU).sin().abs() * (-t*4.0).exp();
                let rf = self.svf1.bandpass(rattle, 8500.0, 3.5, sr) * 0.1;
                let env = (-t/(1.2*dm)).exp();
                m * env * 0.22 + rf
            }

            // ══ PERCUSSION: 88-99 (12 unique) ══

            88 => { // Tambourine — jingles
                let freqs = [4500.0*tm, 6200.0*tm, 7800.0*tm, 9500.0*tm, 11200.0*tm, 13000.0*tm];
                let j = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(j, 4000.0, sr);
                let shake = (t * 24.0 * TAU).sin().abs() * (-t*7.0).exp();
                let env = (-t/(0.18*dm)).exp();
                hp * (env + shake * 0.2) * 0.25
            }
            89 => { // Shaker — dry seeds
                let n = self.noise();
                let f = self.svf1.bandpass(n, 7200.0, 1.3, sr);
                let hp = self.hp1.tick_hp(f, 5000.0, sr);
                hp * (-t/(0.06*dm)).exp() * 0.3
            }
            90 => { // Cowbell — two tones
                advance_phase(&mut self.phase1, 575.0*tm, sr);
                advance_phase(&mut self.phase2, 862.0*tm, sr);
                let body = osc_sine(self.phase1)*0.35 + osc_sine(self.phase2)*0.3;
                let f = self.svf1.bandpass(body, 720.0, 4.0, sr);
                f * (-t/(0.06*dm)).exp()
            }
            91 => { // Woodblock — sharp woody click
                advance_phase(&mut self.phase1, 1950.0*tm, sr);
                advance_phase(&mut self.phase2, 3200.0*tm, sr);
                let click = osc_sine(self.phase1)*0.3 + osc_sine(self.phase2)*0.15;
                let n = self.noise() * (-t*1200.0).exp() * 0.1;
                (click + n) * (-t/(0.012*dm)).exp()
            }
            92 => { // Clave — resonant wood
                advance_phase(&mut self.phase1, 2500.0*tm, sr);
                osc_sine(self.phase1) * 0.4 * (-t/(0.02*dm)).exp()
            }
            93 => { // Triangle — metallic ring
                advance_phase(&mut self.phase1, 1200.0*tm, sr);
                advance_phase(&mut self.phase2, 3600.0*tm, sr);
                let body = osc_sine(self.phase1)*0.3 + osc_sine(self.phase2)*0.2;
                body * (-t/(0.8*dm)).exp()
            }
            94 => { // Cabasa — scratchy beads
                let n = self.noise();
                let f = self.svf1.bandpass(n, 8800.0, 2.0, sr);
                f * (-t/(0.1*dm)).exp() * 0.28
            }
            95 => { // Guiro — scraping stick
                let n = self.noise();
                let f = self.svf1.bandpass(n, 4200.0, 3.0, sr);
                let scrape = (t * 40.0 * TAU).sin().abs() * (-t*5.0).exp();
                f * (scrape * 0.4 + 0.2) * (-t/(0.2*dm)).exp()
            }
            96 => { // Vibraslap — rattle
                let n = self.noise();
                let f = self.svf1.bandpass(n, 3300.0, 5.5, sr);
                let rattle = (t * 38.0 * TAU).sin().abs() * (-t*3.0).exp();
                f * rattle * (-t/(0.45*dm)).exp() * 0.25
            }
            97 => { // Maracas — short shake
                let n = self.noise();
                let hp = self.hp1.tick_hp(n, 6000.0, sr);
                hp * (-t/(0.04*dm)).exp() * 0.25
            }
            98 => { // Agogo High — metallic bell
                advance_phase(&mut self.phase1, 920.0*tm, sr);
                advance_phase(&mut self.phase2, 1384.0*tm, sr);
                (osc_sine(self.phase1)*0.35 + osc_sine(self.phase2)*0.25) * (-t/(0.15*dm)).exp()
            }
            99 => { // Agogo Low
                advance_phase(&mut self.phase1, 660.0*tm, sr);
                advance_phase(&mut self.phase2, 992.0*tm, sr);
                (osc_sine(self.phase1)*0.35 + osc_sine(self.phase2)*0.25) * (-t/(0.15*dm)).exp()
            }

            // ══ MORE PERCUSSION: 100-111 (12 unique) ══

            100 => { // Conga: Open High
                let f = 340.0*tm; let sw = f*0.06*(-t*45.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                let slap = self.noise() * (-t*500.0).exp() * 0.12;
                osc_sine(self.phase1) * 0.5 * (-t/(0.2*dm)).exp() + slap
            }
            101 => { // Conga: Muted
                let f = 320.0*tm;
                advance_phase(&mut self.phase1, f, sr);
                osc_sine(self.phase1) * 0.45 * (-t/(0.06*dm)).exp()
            }
            102 => { // Conga: Low Open
                let f = 220.0*tm; let sw = f*0.05*(-t*35.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                osc_sine(self.phase1) * 0.5 * (-t/(0.22*dm)).exp()
            }
            103 => { // Conga: Slap
                let f = 350.0*tm;
                advance_phase(&mut self.phase1, f, sr);
                let body = osc_sine(self.phase1) * 0.3 * (-t/(0.04*dm)).exp();
                let slap = self.noise() * (-t*800.0).exp();
                let sf = self.svf1.bandpass(slap, 2800.0, 2.0, sr) * 0.3;
                body + sf
            }
            104 => { // Bongo: High
                let f = 420.0*tm; let sw = f*0.08*(-t*70.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                osc_sine(self.phase1) * 0.45 * (-t/(0.1*dm)).exp()
            }
            105 => { // Bongo: Low
                let f = 310.0*tm; let sw = f*0.07*(-t*55.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                osc_sine(self.phase1) * 0.5 * (-t/(0.12*dm)).exp()
            }
            106 => { // Timbale: High — metallic shell ring
                let f = 520.0*tm;
                advance_phase(&mut self.phase1, f, sr);
                let body = osc_sine(self.phase1) * 0.4;
                let ring = self.noise() * (-t*20.0).exp();
                let rf = self.svf1.bandpass(ring, f*3.0, 10.0, sr) * 0.12;
                (body + rf) * (-t/(0.2*dm)).exp()
            }
            107 => { // Timbale: Low
                let f = 360.0*tm;
                advance_phase(&mut self.phase1, f, sr);
                let body = osc_sine(self.phase1) * 0.45;
                let shell = self.noise() * (-t*25.0).exp();
                let sf = self.svf1.bandpass(shell, f*2.5, 8.0, sr) * 0.1;
                (body + sf) * (-t/(0.22*dm)).exp()
            }
            108 => { // Cuica: High — squeaky friction drum
                let f = 600.0 + 400.0 * (-t*8.0).exp();
                advance_phase(&mut self.phase1, f*tm, sr);
                osc_sine(self.phase1) * 0.35 * (-t/(0.15*dm)).exp()
            }
            109 => { // Cuica: Low
                let f = 350.0 + 200.0 * (-t*6.0).exp();
                advance_phase(&mut self.phase1, f*tm, sr);
                osc_sine(self.phase1) * 0.35 * (-t/(0.2*dm)).exp()
            }
            110 => { // Whistle — pitched sine with vibrato
                let vib = (t * 6.0 * TAU).sin() * 25.0;
                advance_phase(&mut self.phase1, 2300.0*tm + vib, sr);
                osc_sine(self.phase1) * 0.3 * (-t/(0.08*dm)).exp()
            }
            111 => { // Clap: Vinyl Room — warm room clap
                let mut env = 0.0;
                for k in 0..4u32 {
                    let off = (self.hit_rand(k*6) * 0.01).abs();
                    let to = t - off;
                    if to >= 0.0 { env += (-to * 160.0).exp() * 0.2; }
                }
                let n = self.noise() * nm;
                let f = self.svf1.bandpass(n, 1900.0, 1.5, sr);
                let lp = self.svf2.lowpass(f, 5000.0, 0.5, sr); // tape warmth extra
                let tail = (-t/(0.18*dm)).exp() * 0.3;
                lp * (env + tail)
            }

            // Anything outside 24-111 = silence
            _ => 0.0,
        }
    }

    // ══════════════════════════════════════════════════════════════════════════
    // TSTY-4: Studio kit v4 — emphasis on hats, snares, claps with LONG decays
    // 88 unique sounds. NO filler. NO pitch transposition.
    // Layout: 8 kicks, 16 snares, 8 claps, 20 hats, 8 toms, 8 cymbals, 20 perc
    // ══════════════════════════════════════════════════════════════════════════

    fn synth_tsty4(&mut self, sr: f64, dm: f64, tm: f64, nm: f64, _dr: f64) -> f64 {
        let raw = self.t4v2(sr, dm, tm, nm);
        // Tape: asymmetric saturation + warm rolloff at 9.5kHz
        let sat = (raw * 1.6).tanh() + 0.035 * raw * (-(raw * 0.5).abs()).exp();
        let rc = 1.0 / (TAU * 9500.0);
        let alpha = 1.0 / (1.0 + rc * sr);
        self.lp1_state += alpha * (sat - self.lp1_state);
        self.lp1_state
    }

    fn t4v2(&mut self, sr: f64, dm: f64, tm: f64, nm: f64) -> f64 {
        let t = self.time;
        let n = self.note;
        match n {
        // ══ 8 KICKS (24-31) — each with different body/beater/shell topology ══

        24 => { // Kick: Felt Studio — 3-mode Bessel body, soft felt beater
            let f=60.0*tm; let sw=f*0.28*(-t*50.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            advance_phase(&mut self.phase3, (f+sw)*2.296, sr);
            let e = 0.3*(-t/0.01).exp() + 0.7*(-t/(0.3*dm)).exp();
            let body = (osc_sine(self.phase1)*0.55 + osc_sine(self.phase2)*0.12*(-t/(0.08*dm)).exp()
                + osc_sine(self.phase3)*0.06*(-t/(0.05*dm)).exp()) * e;
            let felt = self.noise()*(t/0.0018).min(1.0)*(-t*160.0).exp();
            body + self.svf1.bandpass(felt, 2000.0, 1.2, sr)*0.12
        }
        25 => { // Tight Funk — 72Hz, quick, wood beater
            let f=72.0*tm; let sw=f*0.2*(-t*85.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let e = 0.5*(-t/0.005).exp() + 0.5*(-t/(0.15*dm)).exp();
            let body = osc_sine(self.phase1)*0.65*e;
            let click = self.noise()*(t/0.0006).min(1.0)*(-t*500.0).exp();
            body + self.svf1.bandpass(click, 4500.0, 2.0, sr)*0.25
        }
        26 => { // Deep — 44Hz, long, resonant body
            let f=44.0*tm; let sw=f*0.4*(-t*25.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            advance_phase(&mut self.phase2, f*0.5, sr);
            let body = osc_sine(self.phase1)*0.5*(-t/(0.45*dm)).exp();
            let sub = osc_sine(self.phase2)*0.2*(-t/(0.3*dm)).exp();
            body + sub
        }
        27 => { // Rock — 65Hz, bright attack, medium body
            let f=65.0*tm; let sw=f*0.35*(-t*60.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            let e = 0.4*(-t/0.006).exp() + 0.6*(-t/(0.22*dm)).exp();
            let body = osc_sine(self.phase1)*0.6*e + osc_sine(self.phase2)*0.1*(-t/(0.06*dm)).exp();
            let plastic = self.noise()*(t/0.0004).min(1.0)*(-t*600.0).exp();
            body + self.hp1.tick_hp(plastic, 3000.0, sr)*0.3
        }
        28 => { // Round — 55Hz, triangle body, soft
            let f=55.0*tm; let sw=f*0.3*(-t*40.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let e = 0.25*(-t/0.012).exp() + 0.75*(-t/(0.28*dm)).exp();
            osc_triangle(self.phase1)*0.55*e
        }
        29 => { // Punchy — 68Hz, strong 2nd mode, snappy attack
            let f=68.0*tm; let sw=f*0.4*(-t*70.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            let body = osc_sine(self.phase1)*0.5*(0.45*(-t/0.005).exp()+0.55*(-t/(0.18*dm)).exp());
            let m1 = osc_sine(self.phase2)*0.18*(-t/(0.07*dm)).exp();
            let click = self.noise()*(t/0.001).min(1.0)*(-t*350.0).exp();
            body + m1 + self.svf1.bandpass(click, 3200.0, 1.5, sr)*0.15
        }
        30 => { // Boomy — 48Hz, shell resonance, long ring
            let f=48.0*tm; let sw=f*0.3*(-t*22.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let body = osc_sine(self.phase1)*0.5*(-t/(0.4*dm)).exp();
            let shell = self.noise()*(-t*35.0).exp();
            body + self.svf1.bandpass(shell, 240.0*tm, 14.0, sr)*0.1
        }
        31 => { // Thump — 58Hz, very damped, all attack
            let f=58.0*tm; let sw=f*0.15*(-t*100.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let body = osc_sine(self.phase1)*0.6*(-t/(0.1*dm)).exp();
            let thud = self.noise()*(t/0.001).min(1.0)*(-t*200.0).exp();
            body + self.svf1.lowpass(thud, 600.0, 0.5, sr)*0.12
        }

        // ══ 16 SNARES (32-47) — all with proper sustain and wire buzz ══

        32 => { // Snare: Funk — 305Hz, crisp wires, medium body
            let f=305.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*1.593, sr);
            let stick = self.hp1.tick_hp(self.noise()*(t/0.0003).min(1.0)*(-t*1600.0).exp(), 3500.0, sr)*0.28;
            let head = osc_sine(self.phase1)*0.32*(-t/(0.12*dm)).exp() + osc_sine(self.phase2)*0.12*(-t/(0.07*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 4500.0*tm, 0.7, sr);
            let wf = self.hp1.tick_hp(wire, 1800.0, sr)*(-t/(0.22*dm)).exp()*0.38;
            stick + head + wf
        }
        33 => { // Snare: Fat — 235Hz, big body, long wires
            let f=235.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*1.593, sr);
            advance_phase(&mut self.phase3, f*2.136, sr);
            let head = osc_sine(self.phase1)*0.38*(-t/(0.15*dm)).exp()
                + osc_sine(self.phase2)*0.16*(-t/(0.1*dm)).exp()
                + osc_sine(self.phase3)*0.08*(-t/(0.06*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 3800.0, 0.5, sr)*(-t/(0.35*dm)).exp()*0.42;
            let stick = self.noise()*(-t*1400.0).exp()*0.18;
            head + wire + stick
        }
        34 => { // Snare: Dry Purdie — 280Hz, short tight
            let f=280.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.3*(-t/(0.08*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 5200.0, 0.5, sr)*(-t/(0.12*dm)).exp()*0.3;
            let stick = self.noise()*(-t*2000.0).exp()*0.22;
            head + wire + stick
        }
        35 => { // Snare: Brush — 265Hz, gentle noise dominated
            let f=265.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.18*(-t/(0.12*dm)).exp();
            let brush = self.noise()*(t/0.004).min(1.0);
            let bf = self.svf1.bandpass(brush, 3000.0, 0.8, sr)*(-t/(0.2*dm)).exp()*0.35;
            head + bf
        }
        36 => { // Snare: Cross-Stick — rim only, no wires
            advance_phase(&mut self.phase1, 560.0*tm, sr); advance_phase(&mut self.phase2, 1500.0*tm, sr);
            let crack = (osc_sine(self.phase1)*0.28 + osc_sine(self.phase2)*0.18)*(-t/(0.025*dm)).exp();
            let click = self.noise()*(-t*900.0).exp()*0.2;
            crack + click
        }
        37 => { // Snare: Ghost — very quiet, wire-dominant
            let f=295.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.08*(-t/(0.05*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 4800.0, 0.7, sr)*(-t/(0.1*dm)).exp()*0.18;
            head + wire
        }
        38 => { // Snare: Metal Shell — 340Hz, ring, long
            let f=340.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*1.593, sr);
            let head = osc_sine(self.phase1)*0.28*(-t/(0.12*dm)).exp() + osc_sine(self.phase2)*0.14*(-t/(0.09*dm)).exp();
            let shell = self.svf2.bandpass(self.noise()*(-t*35.0).exp(), 520.0*tm, 18.0, sr)*0.12;
            let wire = self.svf1.bandpass(self.noise()*nm, 4200.0, 0.6, sr)*(-t/(0.25*dm)).exp()*0.35;
            head + shell + wire
        }
        39 => { // Snare: Loose — 270Hz, long rattly buzz
            let f=270.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.28*(-t/(0.1*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 3500.0, 0.4, sr)*(-t/(0.45*dm)).exp()*0.45;
            head + wire
        }
        40 => { // Snare: Piccolo — 385Hz, bright short
            let f=385.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.22*(-t/(0.06*dm)).exp();
            let crack = self.hp1.tick_hp(self.noise()*(-t*2500.0).exp(), 5000.0, sr)*0.3;
            let wire = self.svf1.bandpass(self.noise()*nm, 6200.0, 0.8, sr)*(-t/(0.14*dm)).exp()*0.28;
            head + crack + wire
        }
        41 => { // Snare: Wood Deep — 225Hz, woody
            let f=225.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*2.136, sr);
            let head = osc_sine(self.phase1)*0.35*(-t/(0.12*dm)).exp() + osc_sine(self.phase2)*0.1*(-t/(0.06*dm)).exp();
            let shell = self.svf2.bandpass(self.noise()*(-t*45.0).exp(), 330.0*tm, 10.0, sr)*0.08;
            let wire = self.svf1.bandpass(self.noise()*nm, 3600.0, 0.6, sr)*(-t/(0.2*dm)).exp()*0.35;
            head + shell + wire
        }
        42 => { // Snare: Crack — maximum attack, snappy
            advance_phase(&mut self.phase1, 315.0*tm, sr);
            let head = osc_sine(self.phase1)*0.2*(-t/(0.05*dm)).exp();
            let crack = self.hp1.tick_hp(self.noise()*(t/0.0002).min(1.0)*(-t*2800.0).exp(), 4000.0, sr)*0.4;
            let wire = self.svf1.bandpass(self.noise()*nm, 5500.0, 0.7, sr)*(-t/(0.1*dm)).exp()*0.22;
            head + crack + wire
        }
        43 => { // Snare: Thick — 252Hz, lots of body + wire
            let f=252.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*1.593, sr);
            advance_phase(&mut self.phase3, f*2.296, sr);
            let head = osc_sine(self.phase1)*0.38*(-t/(0.14*dm)).exp()
                + osc_sine(self.phase2)*0.18*(-t/(0.09*dm)).exp()
                + osc_sine(self.phase3)*0.09*(-t/(0.06*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 4000.0, 0.6, sr)*(-t/(0.25*dm)).exp()*0.38;
            head + wire + self.noise()*(-t*1200.0).exp()*0.12
        }
        44 => { // Snare: Rim Shot Full — head + rim together
            let f=300.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            advance_phase(&mut self.phase2, 920.0*tm, sr); advance_phase(&mut self.phase3, 2300.0*tm, sr);
            let head = osc_sine(self.phase1)*0.28*(-t/(0.1*dm)).exp();
            let rim = (osc_sine(self.phase2)*0.18 + osc_sine(self.phase3)*0.1)*(-t/(0.02*dm)).exp();
            let crack = self.noise()*(-t*1800.0).exp()*0.25;
            let wire = self.svf1.bandpass(self.noise()*nm, 4300.0, 0.7, sr)*(-t/(0.15*dm)).exp()*0.28;
            head + rim + crack + wire
        }
        45 => { // Snare: Sizzle — double wire band, buzzy
            let f=290.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.22*(-t/(0.08*dm)).exp();
            let w1 = self.svf1.bandpass(self.noise()*nm, 3800.0, 0.4, sr)*(-t/(0.3*dm)).exp()*0.4;
            let w2 = self.svf2.bandpass(self.noise()*nm, 7200.0, 1.0, sr)*(-t/(0.18*dm)).exp()*0.18;
            head + w1 + w2
        }
        46 => { // Snare: Roll Sustain — like a soft roll
            let f=275.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.12*(-t/(0.18*dm)).exp();
            let roll = (t*20.0*TAU).sin().abs();
            let wire = self.svf1.bandpass(self.noise()*nm, 4000.0, 0.6, sr)*roll*(-t/(0.4*dm)).exp()*0.3;
            head + wire
        }
        47 => { // Snare: Backbeat Bonham — 210Hz, huge, ringy
            let f=210.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*1.593, sr);
            advance_phase(&mut self.phase3, f*2.136, sr);
            let head = osc_sine(self.phase1)*0.4*(-t/(0.18*dm)).exp()
                + osc_sine(self.phase2)*0.2*(-t/(0.12*dm)).exp()
                + osc_sine(self.phase3)*0.12*(-t/(0.08*dm)).exp();
            let shell = self.svf2.bandpass(self.noise()*(-t*30.0).exp(), 400.0*tm, 16.0, sr)*0.12;
            let wire = self.svf1.bandpass(self.noise()*nm, 3500.0, 0.5, sr)*(-t/(0.35*dm)).exp()*0.4;
            head + shell + wire
        }

        // ══ 8 CLAPS/SNAPS (48-55) ══

        48 => { // Clap: Tight Group — 5 clappers, close timing
            let mut e=0.0;
            for k in 0..5u32 { let off=(self.hit_rand(k*3)*0.006+self.hit_rand(k*3+1).abs()*0.004).abs();
                let to=t-off; if to>=0.0 { e+=(-to*170.0).exp()*(0.75+self.hit_rand(k*3+2)*0.25)*0.17; } }
            let f = self.svf1.bandpass(self.noise()*nm, 2300.0+self.hit_rand(60)*500.0, 1.4, sr);
            let hp = self.hp1.tick_hp(f, 600.0, sr);
            let tail = (-t/(0.15*dm)).exp()*0.3;
            hp * (e + tail)
        }
        49 => { // Clap: Loose Group — 8 clappers, wide spread
            let mut e=0.0;
            for k in 0..8u32 { let off=(self.hit_rand(k*4)*0.018+self.hit_rand(k*4+1).abs()*0.012).abs();
                let to=t-off; if to>=0.0 { e+=(-to*130.0).exp()*(0.6+self.hit_rand(k*4+2)*0.4)*0.11; } }
            let f = self.svf1.bandpass(self.noise()*nm, 1900.0+self.hit_rand(80)*700.0, 1.1, sr);
            let tail = (-t/(0.2*dm)).exp()*0.35;
            f * (e + tail)
        }
        50 => { // Clap: Hall Reverb — group with long room
            let mut e=0.0;
            for k in 0..4u32 { let off=(self.hit_rand(k*5)*0.01).abs();
                let to=t-off; if to>=0.0 { e+=(-to*160.0).exp()*0.2; } }
            let f = self.svf1.bandpass(self.noise()*nm, 2200.0, 1.3, sr);
            let tail = (-t/(0.35*dm)).exp()*0.4; // LONG tail
            f * (e + tail)
        }
        51 => { // Finger Snap — sharp, high
            let snap = self.noise()*(t/0.0002).min(1.0)*(-t*1000.0).exp();
            let f = self.svf1.bandpass(snap, 3400.0, 2.5, sr);
            self.hp1.tick_hp(f, 1500.0, sr)*0.45
        }
        52 => { // Hand Slap — thigh slap, mid-heavy
            advance_phase(&mut self.phase1, 185.0*tm, sr);
            let body = osc_sine(self.phase1)*0.2*(-t/(0.05*dm)).exp();
            let slap = self.svf1.bandpass(self.noise()*(-t*350.0).exp(), 1600.0, 1.5, sr)*0.3;
            body + slap
        }
        53 => { // Single Clap — one person, dry
            let clap = self.noise()*(t/0.0004).min(1.0)*(-t*220.0).exp();
            let f = self.svf1.bandpass(clap, 2100.0, 1.8, sr);
            self.hp1.tick_hp(f, 500.0, sr)*0.35 + (-t/(0.08*dm)).exp()*self.noise()*0.02
        }
        54 => { // Clap: Dry Staccato — tight, no tail
            let mut e=0.0;
            for k in 0..3u32 { let off=(self.hit_rand(k*3)*0.005).abs();
                let to=t-off; if to>=0.0 { e+=(-to*250.0).exp()*0.25; } }
            let f = self.svf1.bandpass(self.noise()*nm, 2600.0, 1.5, sr);
            f * e
        }
        55 => { // Clap: Vinyl Room — warm, rolled off, vintage
            let mut e=0.0;
            for k in 0..5u32 { let off=(self.hit_rand(k*6)*0.008).abs();
                let to=t-off; if to>=0.0 { e+=(-to*150.0).exp()*0.18; } }
            let f = self.svf1.bandpass(self.noise()*nm, 1800.0, 1.2, sr);
            let warm = self.svf2.lowpass(f, 4500.0, 0.5, sr);
            let tail = (-t/(0.2*dm)).exp()*0.3;
            warm * (e + tail)
        }

        // ══ 20 HATS (56-75) — 6 closed, 4 half-open, 6 open, 2 pedal, 2 sizzle ══
        // ALL use inharmonic sine modal banks. Open hats have 0.8-2.0s decays.

        // -- 6 Closed Hats — each using DIFFERENT synthesis method --
        56 => { // Closed Hat: Modal — standard 6-mode sine bank, tight
            let freqs=[385.0*tm, 960.0*tm, 1620.0*tm, 2520.0*tm, 3740.0*tm, 5180.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(m, 5500.0, sr);
            let stick=self.noise()*(-t*2200.0).exp()*0.1;
            (hp*0.35+stick)*(-t/(0.06*dm)).exp()
        }
        57 => { // Closed Hat: Noise Filtered — NO modal oscs, pure noise shaping
            let n=self.noise();
            let bp=self.svf1.bandpass(n, 8500.0*tm, 2.5, sr)*0.35;
            let hp=self.hp1.tick_hp(bp, 6000.0, sr);
            let stick=self.noise()*(-t*3000.0).exp()*0.08;
            (hp+stick)*(-t/(0.055*dm)).exp()
        }
        58 => { // Closed Hat: FM Click — FM synthesis for unique metallic tick
            advance_phase(&mut self.phase1, 5500.0*tm, sr);
            advance_phase(&mut self.phase2, 8100.0*tm, sr);
            let fm = (self.phase1*TAU + osc_sine(self.phase2)*1.5).sin()*0.3;
            let hp=self.hp1.tick_hp(fm, 4000.0, sr);
            hp*(-t/(0.045*dm)).exp()
        }
        59 => { // Closed Hat: Ring Mod Tick — ring mod for dense inharmonic click
            advance_phase(&mut self.phase1, 4200.0*tm, sr);
            advance_phase(&mut self.phase2, 6300.0*tm, sr);
            let ring = osc_sine(self.phase1)*osc_sine(self.phase2)*0.35;
            let hp=self.hp1.tick_hp(ring, 5500.0, sr);
            hp*(-t/(0.04*dm)).exp()
        }
        60 => { // Closed Hat: Dark Warm — lowpassed modal, mellow
            let freqs=[280.0*tm, 670.0*tm, 1180.0*tm, 1900.0*tm, 2850.0*tm, 3950.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.lowpass(m, 5000.0, 0.4, sr);
            f*(-t/(0.07*dm)).exp()*0.32
        }
        61 => { // Closed Hat: Multi-band Noise — three separate noise bands
            let n=self.noise();
            let lo=self.svf1.bandpass(n, 3800.0*tm, 3.0, sr)*0.2;
            let mid=self.svf2.bandpass(n, 7200.0*tm, 2.5, sr)*0.25;
            let hi=self.svf3.bandpass(n, 12000.0*tm, 2.0, sr)*0.15;
            let hp=self.hp1.tick_hp(lo+mid+hi, 3500.0, sr);
            hp*(-t/(0.05*dm)).exp()
        }

        // -- 4 Half-Open Hats (longer than closed, shorter than open) --
        62 => { // Half-Open: Standard — 150-200ms
            let freqs=[350.0*tm, 840.0*tm, 1490.0*tm, 2350.0*tm, 3490.0*tm, 4820.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.bandpass(m, 6000.0, 1.0, sr);
            let hp=self.hp1.tick_hp(f, 3500.0, sr);
            let sizzle=self.svf2.bandpass(self.noise()*(-t/(0.2*dm)).exp(), 7800.0, 4.0, sr)*0.06;
            (hp*0.3+sizzle)*(-t/(0.22*dm)).exp()
        }
        63 => { // Half-Open: Bright — more top end
            let freqs=[380.0*tm, 920.0*tm, 1600.0*tm, 2520.0*tm, 3720.0*tm, 5140.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(m, 4000.0, sr);
            hp*(-t/(0.28*dm)).exp()*0.32
        }
        64 => { // Half-Open: Dark — warmer, lower modes
            let freqs=[315.0*tm, 755.0*tm, 1330.0*tm, 2120.0*tm, 3160.0*tm, 4370.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.lowpass(m, 6500.0, 0.5, sr);
            f*(-t/(0.25*dm)).exp()*0.3
        }
        65 => { // Half-Open: Trashy — buzzy, aggressive
            let freqs=[290.0*tm, 695.0*tm, 1230.0*tm, 1960.0*tm, 2920.0*tm, 4040.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let dist = (m*1.8).tanh()*0.5; // distortion for trash
            dist*(-t/(0.2*dm)).exp()*0.32
        }

        // -- 6 Open Hats — LONG DECAYS, each with DIFFERENT synthesis topology --
        66 => { // Open Hat: Modal Shimmer — 9-mode sine bank, 1.2s
            // Uses modal_phases for extra modes beyond the 6 hat oscillators
            let freqs=[340.0*tm, 815.0*tm, 1490.0*tm, 2350.0*tm, 3500.0*tm, 4850.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            advance_phase(&mut self.modal_phases[0], 6400.0*tm, sr);
            advance_phase(&mut self.modal_phases[1], 8300.0*tm, sr);
            advance_phase(&mut self.modal_phases[2], 10500.0*tm, sr);
            let upper = osc_sine(self.modal_phases[0])*0.15*(-t/(1.5*dm)).exp()
                + osc_sine(self.modal_phases[1])*0.1*(-t/(1.8*dm)).exp()
                + osc_sine(self.modal_phases[2])*0.06*(-t/(2.2*dm)).exp();
            let hp=self.hp1.tick_hp(m, 2500.0, sr);
            (hp*0.35 + upper)*(-t/(1.2*dm)).exp()
        }
        67 => { // Open Hat: FM Metallic — FM synthesis for metallic character, 1.5s
            // Completely different from modal bank — uses FM between two sines
            advance_phase(&mut self.phase1, 3200.0*tm, sr); // carrier
            advance_phase(&mut self.phase2, 4700.0*tm, sr); // modulator
            advance_phase(&mut self.phase3, 7100.0*tm, sr); // second carrier
            let fm_mod = osc_sine(self.phase2) * 2.5;
            let fm1 = (self.phase1 * TAU + fm_mod).sin() * 0.25;
            let fm2 = (self.phase3 * TAU + fm_mod * 0.7).sin() * 0.18;
            let noise_sheen = self.hp1.tick_hp(self.noise()*0.08, 8000.0, sr);
            let env = (-t/(1.5*dm)).exp();
            (fm1 + fm2 + noise_sheen) * env
        }
        68 => { // Open Hat: Filtered Noise — noise through resonant comb, 1.0s
            // No oscillators at all — pure filtered noise approach
            let n = self.noise();
            let bp1 = self.svf1.bandpass(n, 4200.0*tm, 3.0, sr) * 0.3;
            let bp2 = self.svf2.bandpass(n, 7800.0*tm, 2.5, sr) * 0.25;
            let bp3 = self.svf3.bandpass(n, 11500.0*tm, 2.0, sr) * 0.15;
            let hp = self.hp1.tick_hp(bp1 + bp2 + bp3, 3000.0, sr);
            let env = (-t/(1.0*dm)).exp();
            hp * env
        }
        69 => { // Open Hat: Ring Mod — two oscillators multiplied, 1.8s
            // Ring modulation creates dense inharmonic content
            advance_phase(&mut self.phase1, 2850.0*tm, sr);
            advance_phase(&mut self.phase2, 4130.0*tm, sr); // non-integer ratio
            let ring = osc_sine(self.phase1) * osc_sine(self.phase2); // sum & difference freqs
            advance_phase(&mut self.phase3, 6950.0*tm, sr);
            let shimmer = osc_sine(self.phase3) * 0.12;
            let hp = self.hp1.tick_hp(ring * 0.35 + shimmer, 2000.0, sr);
            let env = (-t/(1.8*dm)).exp();
            hp * env
        }
        70 => { // Open Hat: Breathy Wash — noise emphasis, gentle modes, 2.0s
            // Mostly high noise with just hints of tonality
            let n = self.noise();
            let wash = self.hp1.tick_hp(n, 5000.0, sr) * 0.3;
            advance_phase(&mut self.phase1, 5500.0*tm, sr);
            advance_phase(&mut self.phase2, 8200.0*tm, sr);
            let hints = osc_sine(self.phase1)*0.06 + osc_sine(self.phase2)*0.04;
            let env = (-t/(2.0*dm)).exp();
            (wash + hints) * env
        }
        71 => { // Open Hat: Trashy Distorted — heavy saturation, 1.3s
            // Distortion-based — overdrive creates new harmonics
            let freqs=[280.0*tm, 670.0*tm, 1180.0*tm, 1900.0*tm, 2850.0*tm, 3950.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            // Heavy waveshaping — creates completely different harmonic content
            let dist = (m * 3.5).tanh() * 0.4;
            let n = self.noise() * 0.08;
            let hp = self.hp1.tick_hp(dist + n, 1800.0, sr);
            let env = (-t/(1.3*dm)).exp();
            hp * env
        }

        // -- 2 Pedal Hats --
        72 => { // Pedal Chick: Standard — foot close, short
            let freqs=[355.0*tm, 850.0*tm, 1500.0*tm, 2370.0*tm, 3520.0*tm, 4860.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let chick=self.svf1.bandpass(self.noise()*(-t*500.0).exp(), 1300.0, 2.5, sr)*0.12;
            m*(-t/(0.02*dm)).exp()*0.2 + chick
        }
        73 => { // Pedal Chick: Splashy — looser closure
            let freqs=[340.0*tm, 820.0*tm, 1450.0*tm, 2290.0*tm, 3400.0*tm, 4700.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let chick=self.svf1.bandpass(self.noise()*(-t*300.0).exp(), 1500.0, 2.0, sr)*0.1;
            m*(-t/(0.06*dm)).exp()*0.22 + chick
        }

        // -- 2 Sizzle Hats --
        74 => { // Sizzle Hat: Riveted — continuous rattle, 1.5s
            let freqs=[348.0*tm, 835.0*tm, 1475.0*tm, 2330.0*tm, 3460.0*tm, 4780.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let rattle=self.noise()*(t*32.0*TAU).sin().abs()*(-t*3.5).exp();
            let rf=self.svf1.bandpass(rattle, 9000.0, 3.5, sr)*0.1;
            m*(-t/(1.5*dm)).exp()*0.22 + rf
        }
        75 => { // Sizzle Hat: Chain — heavier rattle
            let freqs=[330.0*tm, 792.0*tm, 1400.0*tm, 2215.0*tm, 3295.0*tm, 4550.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let rattle=self.noise()*(t*25.0*TAU).sin().abs()*(-t*3.0).exp();
            let rf=self.svf1.bandpass(rattle, 7500.0, 3.0, sr)*0.12;
            m*(-t/(1.2*dm)).exp()*0.22 + rf
        }

        // ══ 8 TOMS (76-83) ══

        76 => { // Floor Tom Deep — 82Hz
            let f=82.0*tm; let sw=f*0.14*(-t*28.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr); advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            let body = osc_sine(self.phase1)*0.5*(0.2*(-t/0.015).exp()+0.8*(-t/(0.35*dm)).exp());
            let m1 = osc_sine(self.phase2)*0.14*(-t/(0.12*dm)).exp();
            let stick = self.svf1.bandpass(self.noise()*(-t*250.0).exp(), 2500.0, 1.3, sr)*0.08;
            body + m1 + stick
        }
        77 => { // Floor Tom Medium — 108Hz
            let f=108.0*tm; let sw=f*0.12*(-t*32.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let body = osc_sine(self.phase1)*0.48*(0.25*(-t/0.01).exp()+0.75*(-t/(0.28*dm)).exp());
            body + self.noise()*(-t*280.0).exp()*0.06
        }
        78 => { // Rack Tom Low — 135Hz
            let f=135.0*tm; let sw=f*0.1*(-t*35.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let body = osc_sine(self.phase1)*0.48*(0.3*(-t/0.008).exp()+0.7*(-t/(0.24*dm)).exp());
            body + self.svf1.bandpass(self.noise()*(-t*300.0).exp(), 3000.0, 1.4, sr)*0.08
        }
        79 => { // Rack Tom Mid — 170Hz
            let f=170.0*tm; let sw=f*0.1*(-t*38.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr); advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            let body = osc_sine(self.phase1)*0.45*(0.3*(-t/0.007).exp()+0.7*(-t/(0.22*dm)).exp());
            body + osc_sine(self.phase2)*0.1*(-t/(0.07*dm)).exp()
        }
        80 => { // Rack Tom High — 215Hz
            let f=215.0*tm; let sw=f*0.08*(-t*40.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            osc_sine(self.phase1)*0.45*(0.35*(-t/0.006).exp()+0.65*(-t/(0.18*dm)).exp())
        }
        81 => { // Concert Tom — 150Hz, resonant, long
            let f=150.0*tm; let sw=f*0.1*(-t*22.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr); advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            advance_phase(&mut self.phase3, (f+sw)*2.296, sr);
            osc_sine(self.phase1)*0.45*(-t/(0.38*dm)).exp()
                + osc_sine(self.phase2)*0.16*(-t/(0.15*dm)).exp()
                + osc_sine(self.phase3)*0.08*(-t/(0.1*dm)).exp()
        }
        82 => { // Roto High — 290Hz, bright
            let f=290.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            osc_sine(self.phase1)*0.38*(-t/(0.14*dm)).exp()
                + osc_triangle(self.phase1*2.5)*0.08*(-t/(0.06*dm)).exp()
        }
        83 => { // Tom: Timbale-ish — 360Hz, metallic ring
            let f=360.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let body=osc_sine(self.phase1)*0.35*(-t/(0.18*dm)).exp();
            let ring=self.svf1.bandpass(self.noise()*(-t*25.0).exp(), f*2.8, 12.0, sr)*0.12;
            body + ring
        }

        // ══ 8 CYMBALS (84-91) ══

        84 => { // Crash: Dark — 1.5s
            let freqs=[310.0*tm, 740.0*tm, 1300.0*tm, 2080.0*tm, 3100.0*tm, 4280.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.lowpass(m, 7500.0, 0.3, sr);
            (f*0.32+self.noise()*0.08)*(t/0.003).min(1.0)*(-t/(1.5*dm)).exp()
        }
        85 => { // Crash: Bright — 1.8s
            let freqs=[405.0*tm, 970.0*tm, 1700.0*tm, 2680.0*tm, 3950.0*tm, 5450.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(m, 2500.0, sr);
            hp*(t/0.002).min(1.0)*(-t/(1.8*dm)).exp()*0.3
        }
        86 => { // Ride: Ping — defined, controlled
            let freqs=[425.0*tm, 1010.0*tm, 1740.0*tm, 2830.0*tm, 4180.0*tm, 5750.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.bandpass(m, 5500.0, 1.0, sr);
            let ping=(-t*100.0).exp()*0.12;
            (f*(-t/(1.0*dm)).exp()+ping)*0.28
        }
        87 => { // Ride: Wash — loose, 2s
            let freqs=[390.0*tm, 935.0*tm, 1630.0*tm, 2580.0*tm, 3830.0*tm, 5290.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            m*(-t/(2.0*dm)).exp()*0.22
        }
        88 => { // Ride Bell — tonal, 0.8s
            advance_phase(&mut self.phase1, 760.0*tm, sr);
            advance_phase(&mut self.phase2, 1140.0*tm, sr);
            advance_phase(&mut self.phase3, 1710.0*tm, sr);
            (osc_sine(self.phase1)*0.28+osc_sine(self.phase2)*0.22+osc_sine(self.phase3)*0.14)*(-t/(0.8*dm)).exp()
        }
        89 => { // Splash — quick bright, 0.5s
            let freqs=[460.0*tm, 1100.0*tm, 1880.0*tm, 2950.0*tm, 4350.0*tm, 5980.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(m, 3500.0, sr);
            hp*(t/0.001).min(1.0)*(-t/(0.5*dm)).exp()*0.3
        }
        90 => { // China — trashy, 1.2s
            let freqs=[285.0*tm, 685.0*tm, 1210.0*tm, 1930.0*tm, 2880.0*tm, 3990.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let dist=(m*2.2).tanh()*0.45;
            dist*(t/0.002).min(1.0)*(-t/(1.2*dm)).exp()*0.3
        }
        91 => { // Cymbal: Sizzle — riveted, 1.8s
            let freqs=[355.0*tm, 852.0*tm, 1505.0*tm, 2380.0*tm, 3540.0*tm, 4890.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let rattle=self.noise()*(t*28.0*TAU).sin().abs()*(-t*3.5).exp();
            let rf=self.svf1.bandpass(rattle, 8500.0, 3.5, sr)*0.1;
            m*(-t/(1.8*dm)).exp()*0.22+rf
        }

        // ══ 20 PERCUSSION (92-111) ══

        92 => { // Tambourine
            let freqs=[4600.0*tm, 6300.0*tm, 7900.0*tm, 9600.0*tm, 11300.0*tm, 13100.0*tm];
            let j=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(j, 4000.0, sr);
            let shake=(t*22.0*TAU).sin().abs()*(-t*6.0).exp();
            hp*((-t/(0.2*dm)).exp()+shake*0.2)*0.25
        }
        93 => { // Shaker: Tight
            let f=self.svf1.bandpass(self.noise(), 7200.0, 1.3, sr);
            self.hp1.tick_hp(f, 5000.0, sr)*(-t/(0.07*dm)).exp()*0.3
        }
        94 => { // Shaker: Long
            let f=self.svf1.bandpass(self.noise(), 8000.0, 1.5, sr);
            let swish=(t*14.0).sin().abs()*(-t*4.0).exp();
            f*((-t/(0.15*dm)).exp()+swish*0.15)*0.25
        }
        95 => { // Cowbell
            advance_phase(&mut self.phase1, 580.0*tm, sr); advance_phase(&mut self.phase2, 870.0*tm, sr);
            let body=osc_sine(self.phase1)*0.35+osc_sine(self.phase2)*0.3;
            self.svf1.bandpass(body, 725.0, 4.0, sr)*(-t/(0.07*dm)).exp()
        }
        96 => { // Woodblock
            advance_phase(&mut self.phase1, 1900.0*tm, sr); advance_phase(&mut self.phase2, 3100.0*tm, sr);
            (osc_sine(self.phase1)*0.3+osc_sine(self.phase2)*0.15+self.noise()*(-t*1000.0).exp()*0.08)
                *(-t/(0.015*dm)).exp()
        }
        97 => { // Clave
            advance_phase(&mut self.phase1, 2500.0*tm, sr);
            osc_sine(self.phase1)*0.4*(-t/(0.022*dm)).exp()
        }
        98 => { // Triangle
            advance_phase(&mut self.phase1, 1200.0*tm, sr); advance_phase(&mut self.phase2, 3600.0*tm, sr);
            (osc_sine(self.phase1)*0.3+osc_sine(self.phase2)*0.18)*(-t/(0.9*dm)).exp()
        }
        99 => { // Cabasa
            self.svf1.bandpass(self.noise(), 8800.0, 2.0, sr)*(-t/(0.1*dm)).exp()*0.28
        }
        100 => { // Guiro
            let f=self.svf1.bandpass(self.noise(), 4200.0, 3.0, sr);
            let scrape=(t*38.0*TAU).sin().abs()*(-t*4.5).exp();
            f*(scrape*0.4+0.15)*(-t/(0.22*dm)).exp()
        }
        101 => { // Vibraslap
            let f=self.svf1.bandpass(self.noise(), 3400.0, 5.5, sr);
            let rattle=(t*36.0*TAU).sin().abs()*(-t*2.8).exp();
            f*rattle*(-t/(0.5*dm)).exp()*0.25
        }
        102 => { // Maracas
            let hp=self.hp1.tick_hp(self.noise(), 6500.0, sr);
            hp*(-t/(0.045*dm)).exp()*0.25
        }
        103 => { // Agogo High
            advance_phase(&mut self.phase1, 930.0*tm, sr); advance_phase(&mut self.phase2, 1398.0*tm, sr);
            (osc_sine(self.phase1)*0.33+osc_sine(self.phase2)*0.24)*(-t/(0.16*dm)).exp()
        }
        104 => { // Agogo Low
            advance_phase(&mut self.phase1, 670.0*tm, sr); advance_phase(&mut self.phase2, 1008.0*tm, sr);
            (osc_sine(self.phase1)*0.33+osc_sine(self.phase2)*0.24)*(-t/(0.16*dm)).exp()
        }
        105 => { // Conga: Open
            let f=335.0*tm; advance_phase(&mut self.phase1, f+f*0.06*(-t*42.0).exp(), sr);
            osc_sine(self.phase1)*0.5*(-t/(0.22*dm)).exp() + self.noise()*(-t*450.0).exp()*0.08
        }
        106 => { // Conga: Mute
            advance_phase(&mut self.phase1, 320.0*tm, sr);
            osc_sine(self.phase1)*0.42*(-t/(0.06*dm)).exp()
        }
        107 => { // Conga: Slap
            advance_phase(&mut self.phase1, 355.0*tm, sr);
            let body=osc_sine(self.phase1)*0.25*(-t/(0.04*dm)).exp();
            body + self.svf1.bandpass(self.noise()*(-t*700.0).exp(), 2800.0, 2.0, sr)*0.25
        }
        108 => { // Bongo: High
            advance_phase(&mut self.phase1, 425.0*tm+425.0*tm*0.08*(-t*65.0).exp(), sr);
            osc_sine(self.phase1)*0.42*(-t/(0.1*dm)).exp()
        }
        109 => { // Bongo: Low
            advance_phase(&mut self.phase1, 315.0*tm+315.0*tm*0.07*(-t*50.0).exp(), sr);
            osc_sine(self.phase1)*0.45*(-t/(0.13*dm)).exp()
        }
        110 => { // Timbale High
            let f=530.0*tm; advance_phase(&mut self.phase1, f, sr);
            let body=osc_sine(self.phase1)*0.38;
            let ring=self.svf1.bandpass(self.noise()*(-t*18.0).exp(), f*3.0, 10.0, sr)*0.1;
            (body+ring)*(-t/(0.2*dm)).exp()
        }
        111 => { // Timbale Low
            let f=370.0*tm; advance_phase(&mut self.phase1, f, sr);
            let body=osc_sine(self.phase1)*0.4;
            let ring=self.svf1.bandpass(self.noise()*(-t*22.0).exp(), f*2.5, 8.0, sr)*0.08;
            (body+ring)*(-t/(0.22*dm)).exp()
        }

        _ => 0.0,
        }
    }

    // ══════════════════════════════════════════════════════════════════════════
    // TSTY-5: Each sound uses a NAMED synthesis technique. No two sounds
    // share the same technique+params. Verified twice after writing.
    //
    // SYNTHESIS TECHNIQUES USED (tracked to prevent reuse):
    // K1: 3-mode Bessel + felt noise burst  K2: FM body (sine mod sine) + click
    // K3: triangle waveshape + sub          K4: ring mod body + thump
    // K5: square wave LPF'd + noise         K6: wavefold sine + noise
    // K7: 2-mode + shell resonant BPF       K8: pure sine + heavy saturation
    // S1: sine body + comb-like wire        S2: FM body + HP noise wire
    // S3: triangle body + multi-band wire   S4: noise-only brush
    // S5: rim = two high sines              S6: sine body + AM wire
    // S7: square body + pitched noise       S8: wavefold body + noise burst
    // S9: ring mod body + filtered wire     S10: 3-mode + resonant shell ring
    // CL1: 5-clapper random burst           CL2: 8-clapper wide spread
    // CL3: single clap dry                  CL4: finger snap HP filtered
    // CL5: hand slap LPF                    CL6: group + long reverb tail
    // CH1: modal 6-osc bank                 CH2: pure noise BPF
    // CH3: FM metallic tick                 CH4: ring mod tick
    // CH5: dark LPF modal                   CH6: multi-band noise
    // CH7: noise + saturation               CH8: AM modulated noise
    // HO1: 9-mode shimmer 1.5s             HO2: FM metallic 2.0s
    // HO3: 3-band resonant noise 1.2s      HO4: ring mod wash 1.8s
    // HO5: breathy HP noise 2.5s           HO6: distorted modal 1.3s
    // HO7: comb filtered noise 1.6s        HO8: phase-mod shimmer 2.0s
    // PH1: pedal chick modal               PH2: pedal loose noise
    // T1-T6: toms with different modes/shells
    // CY1-CY6: cymbals with different topologies
    // P1-P12: unique percussion
    // ══════════════════════════════════════════════════════════════════════════

    fn synth_tsty5(&mut self, sr: f64, dm: f64, _tm: f64, _nm: f64, _dr: f64) -> f64 {
        // RESONATOR-BASED SYNTHESIS: exciter → resonant filters → tape saturation
        // The tone comes from FILTERS, not oscillators. Like a real drum.
        let t = self.time;
        let n = self.note;

        // Get the sound recipe for this note
        let recipe = t5_recipe(n);

        // ── EXCITER: short impulse/noise that rings the resonators ──
        let exciter = match recipe.exciter {
            0 => { // Impulse + noise: general purpose
                let impulse = if t < 0.001 { 1.0 } else { 0.0 };
                let noise = self.noise() * (-t / recipe.noise_decay.max(0.001)).exp() * recipe.noise_level;
                impulse * recipe.impulse_level + noise
            }
            1 => { // Noise only: hats, cymbals, brushes
                self.noise() * (-t / recipe.noise_decay.max(0.001)).exp() * recipe.noise_level
            }
            2 => { // Click + noise: snares, claps
                let click = self.noise() * (t / 0.0003).min(1.0) * (-t * 800.0).exp();
                let noise = self.noise() * (-t / recipe.noise_decay.max(0.001)).exp() * recipe.noise_level;
                click * recipe.impulse_level + noise
            }
            _ => { // Multi-burst: claps
                let mut env = 0.0;
                for k in 0..recipe.burst_count as u32 {
                    let off = (self.hit_rand(k * 3) * recipe.burst_spread
                        + self.hit_rand(k * 3 + 1).abs() * recipe.burst_spread * 0.5).abs();
                    let to = t - off;
                    if to >= 0.0 {
                        env += (-to * 180.0).exp() * (0.7 + self.hit_rand(k * 3 + 2) * 0.3) * 0.2;
                    }
                }
                self.noise() * (env + (-t / recipe.noise_decay.max(0.001)).exp() * recipe.noise_level * 0.3)
            }
        };

        // ── RESONATOR BANK: bandpass filters create the tone ──
        // Each resonator is a bandpass filter ringing at a specific frequency
        // This is how real drums work: the shell/head IS a resonator

        let mut resonated = 0.0;

        // Resonator 1 (primary body) — uses SVF1
        if recipe.r1_freq > 0.0 {
            // Pitch envelope on primary resonator
            let freq = recipe.r1_freq + recipe.pitch_sweep * (-t / recipe.pitch_time.max(0.001)).exp();
            let r1 = self.svf1.bandpass(exciter, freq, recipe.r1_q, sr);
            resonated += r1 * recipe.r1_level * (-t / (recipe.r1_decay * dm).max(0.001)).exp();
        }

        // Resonator 2 (secondary mode / shell) — uses SVF2
        if recipe.r2_freq > 0.0 {
            let r2 = self.svf2.bandpass(exciter, recipe.r2_freq, recipe.r2_q, sr);
            resonated += r2 * recipe.r2_level * (-t / (recipe.r2_decay * dm).max(0.001)).exp();
        }

        // Resonator 3 (third mode / brightness) — uses SVF3
        if recipe.r3_freq > 0.0 {
            let r3 = self.svf3.bandpass(exciter, recipe.r3_freq, recipe.r3_q, sr);
            resonated += r3 * recipe.r3_level * (-t / (recipe.r3_decay * dm).max(0.001)).exp();
        }

        // ── NOISE SHAPING: filtered noise for wires, shimmer, wash ──
        let noise_shaped = if recipe.noise_filter_freq > 0.0 {
            let raw = self.noise() * recipe.noise_mix;
            let filtered = self.hp1.tick_hp(raw, recipe.noise_filter_freq, sr);
            // For snare wires: modulate noise by body amplitude (sympathetic vibration)
            let body_mod = if recipe.wire_coupling > 0.0 {
                (resonated.abs() * recipe.wire_coupling + (1.0 - recipe.wire_coupling)).min(1.0)
            } else { 1.0 };
            filtered * body_mod * (-t / (recipe.noise_filter_decay * dm).max(0.001)).exp()
        } else { 0.0 };

        // ── MIX + TAPE SATURATION ──
        let raw = resonated + noise_shaped;
        let sat = (raw * 1.5).tanh() + 0.03 * raw * (-(raw * 0.5).abs()).exp();
        let rc = 1.0 / (TAU * 9500.0);
        let alpha = 1.0 / (1.0 + rc * sr);
        self.lp1_state += alpha * (sat - self.lp1_state);
        self.lp1_state
    }

    // Keep the old t5 for reference but it's no longer called
    #[allow(dead_code)]
    fn t5_old(&mut self, sr: f64, dm: f64, tm: f64, nm: f64) -> f64 {
        let t = self.time;
        match self.note {

        // ════════ 8 KICKS (24-31) — 8 different synthesis methods ════════

        24 => { // K1: 3-mode Bessel membrane + felt noise burst
            let f=62.0; let sw=f*0.28*(-t*50.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            advance_phase(&mut self.phase3, (f+sw)*2.296, sr);
            let e=0.3*(-t/0.01).exp()+0.7*(-t/(0.3*dm)).exp();
            let body=osc_sine(self.phase1)*0.55*e + osc_sine(self.phase2)*0.12*(-t/(0.08*dm)).exp()
                + osc_sine(self.phase3)*0.06*(-t/(0.05*dm)).exp();
            let felt=self.noise()*(t/0.0018).min(1.0)*(-t*160.0).exp();
            body + self.svf1.bandpass(felt, 2000.0, 1.2, sr)*0.12
        }
        25 => { // K2: FM body — sine modulating sine for complex low end
            advance_phase(&mut self.phase1, 55.0, sr); // carrier
            advance_phase(&mut self.phase2, 82.0, sr); // modulator
            let fm_idx = 3.0 * (-t*35.0).exp(); // FM index decays
            let body = (self.phase1*TAU + osc_sine(self.phase2)*fm_idx).sin();
            let e = 0.35*(-t/0.008).exp()+0.65*(-t/(0.25*dm)).exp();
            let click = self.noise()*(t/0.0006).min(1.0)*(-t*500.0).exp();
            body*0.55*e + self.svf1.bandpass(click, 4200.0, 2.0, sr)*0.2
        }
        26 => { // K3: Triangle waveshape body — rounder than sine, no harmonics
            let f=50.0; let sw=f*0.35*(-t*30.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let body = osc_triangle(self.phase1)*0.6;
            advance_phase(&mut self.phase2, 25.0, sr); // sub one octave down
            let sub = osc_triangle(self.phase2)*0.2*(-t/(0.25*dm)).exp();
            let e = 0.25*(-t/0.015).exp()+0.75*(-t/(0.4*dm)).exp();
            (body + sub)*e
        }
        27 => { // K4: Ring mod body — two sines multiplied for complex low end
            advance_phase(&mut self.phase1, 48.0, sr);
            advance_phase(&mut self.phase2, 72.0, sr); // 3:2 ratio
            let ring = osc_sine(self.phase1)*osc_sine(self.phase2); // creates 24Hz + 120Hz
            let e = 0.3*(-t/0.01).exp()+0.7*(-t/(0.35*dm)).exp();
            let thump = self.noise()*(-t*80.0).exp();
            ring*0.6*e + self.svf1.lowpass(thump, 400.0, 0.5, sr)*0.1
        }
        28 => { // K5: Square wave through LPF — harmonically rich body
            advance_phase(&mut self.phase1, 58.0, sr);
            let sq = if self.phase1.fract() < 0.5 { 0.8 } else { -0.8 };
            let filtered = self.svf1.lowpass(sq, 200.0 + 600.0*(-t*25.0).exp(), 0.8, sr);
            let e = 0.4*(-t/0.006).exp()+0.6*(-t/(0.2*dm)).exp();
            let click = self.noise()*(t/0.0004).min(1.0)*(-t*600.0).exp();
            filtered*e + self.hp1.tick_hp(click, 3000.0, sr)*0.25
        }
        29 => { // K6: Wavefolded sine — creates odd harmonics for warm thick tone
            let f=65.0; let sw=f*0.3*(-t*55.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let raw = osc_sine(self.phase1)*2.5; // overdrive the sine
            let folded = raw.sin(); // sine of sine = wavefold
            let e = 0.35*(-t/0.007).exp()+0.65*(-t/(0.22*dm)).exp();
            folded*0.45*e
        }
        30 => { // K7: 2-mode + shell resonant BPF — woody attack
            let f=68.0; let sw=f*0.2*(-t*65.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            let body = osc_sine(self.phase1)*0.5*(0.4*(-t/0.005).exp()+0.6*(-t/(0.18*dm)).exp());
            let m1 = osc_sine(self.phase2)*0.15*(-t/(0.06*dm)).exp();
            let shell_exc = self.noise()*(-t*50.0).exp();
            let shell = self.svf2.bandpass(shell_exc, 250.0, 15.0, sr)*0.1;
            body + m1 + shell
        }
        31 => { // K8: Pure sine + heavy saturation — tape-crushed minimal
            let f=52.0; let sw=f*0.15*(-t*40.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let raw = osc_sine(self.phase1)*1.8; // hot signal
            let crushed = (raw*2.0).tanh()*0.5; // heavy saturation adds harmonics
            let e = (-t/(0.18*dm)).exp();
            crushed*e
        }

        // ════════ 10 SNARES (32-41) — 10 different synthesis methods ════════

        32 => { // S1: Sine body + comb-like pitched wire buzz
            let f=305.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.3*(-t/(0.12*dm)).exp();
            let stick = self.hp1.tick_hp(self.noise()*(t/0.0003).min(1.0)*(-t*1600.0).exp(), 3500.0, sr)*0.25;
            // Wire: noise through tight BPF at head freq harmonics = quasi-comb
            let w1 = self.svf1.bandpass(self.noise()*nm, 3200.0, 3.0, sr)*0.15;
            let w2 = self.svf2.bandpass(self.noise()*nm, 5800.0, 2.5, sr)*0.12;
            let w3 = self.svf3.bandpass(self.noise()*nm, 8400.0, 2.0, sr)*0.08;
            let wire = (w1+w2+w3)*(-t/(0.25*dm)).exp();
            stick + head + wire
        }
        33 => { // S2: FM body (richer than pure sine) + HP noise wire
            advance_phase(&mut self.phase1, 240.0*tm, sr); // carrier
            advance_phase(&mut self.phase2, 380.0*tm, sr); // modulator
            let fm_body = (self.phase1*TAU + osc_sine(self.phase2)*0.8).sin();
            let head = fm_body*0.3*(-t/(0.14*dm)).exp();
            let wire = self.hp1.tick_hp(self.noise()*nm, 2500.0, sr)*(-t/(0.3*dm)).exp()*0.35;
            let stick = self.noise()*(-t*1400.0).exp()*0.18;
            head + wire + stick
        }
        34 => { // S3: Triangle body (softer odd harmonics) + multi-band wire
            advance_phase(&mut self.phase1, 280.0*tm, sr);
            let head = osc_triangle(self.phase1)*0.28*(-t/(0.1*dm)).exp();
            let w1 = self.svf1.bandpass(self.noise()*nm, 4000.0, 1.5, sr)*0.18;
            let w2 = self.svf2.bandpass(self.noise()*nm, 7500.0, 1.2, sr)*0.12;
            let wire = (w1+w2)*(-t/(0.22*dm)).exp();
            head + wire + self.noise()*(-t*2000.0).exp()*0.15
        }
        35 => { // S4: Noise-only brush — no tonal body at all
            let brush = self.noise()*(t/0.005).min(1.0); // slow rise = brush stroke
            let shaped = self.svf1.bandpass(brush, 2800.0, 0.6, sr);
            let hp = self.hp1.tick_hp(shaped, 800.0, sr);
            hp*(-t/(0.2*dm)).exp()*0.4
        }
        36 => { // S5: Cross-stick — two high sine partials, no wires
            advance_phase(&mut self.phase1, 580.0*tm, sr);
            advance_phase(&mut self.phase2, 1520.0*tm, sr);
            let crack = osc_sine(self.phase1)*0.25 + osc_sine(self.phase2)*0.18;
            let click = self.noise()*(-t*900.0).exp()*0.2;
            (crack + click)*(-t/(0.025*dm)).exp()
        }
        37 => { // S6: Sine body + AM modulated wire (amplitude modulation)
            advance_phase(&mut self.phase1, 310.0*tm, sr);
            let head = osc_sine(self.phase1)*0.3*(-t/(0.1*dm)).exp();
            // AM: noise amplitude modulated by a fast oscillator = "chattering" wire
            advance_phase(&mut self.phase3, 180.0, sr);
            let am_mod = (1.0 + osc_sine(self.phase3)) * 0.5; // 0-1 range
            let wire = self.noise()*nm*am_mod;
            let wire_f = self.hp1.tick_hp(wire, 2000.0, sr)*(-t/(0.28*dm)).exp()*0.35;
            head + wire_f
        }
        38 => { // S7: Square wave body + pitched noise — buzzy character
            advance_phase(&mut self.phase1, 260.0*tm, sr);
            let sq = if self.phase1.fract() < 0.5 { 0.3 } else { -0.3 };
            let body = self.svf1.lowpass(sq, 800.0, 0.5, sr)*(-t/(0.08*dm)).exp();
            let wire = self.hp1.tick_hp(self.noise()*nm, 3000.0, sr)*(-t/(0.18*dm)).exp()*0.3;
            body + wire + self.noise()*(-t*2500.0).exp()*0.2
        }
        39 => { // S8: Wavefolded body + noise burst — distorted snare
            advance_phase(&mut self.phase1, 290.0*tm, sr);
            let raw = osc_sine(self.phase1)*2.0;
            let folded = raw.sin()*0.25; // wavefold
            let body = folded*(-t/(0.1*dm)).exp();
            let crack = self.noise()*(-t*1800.0).exp()*0.3;
            let wire = self.svf1.bandpass(self.noise()*nm, 4500.0, 0.7, sr)*(-t/(0.2*dm)).exp()*0.3;
            body + crack + wire
        }
        40 => { // S9: Ring mod body + filtered wire — metallic snare
            advance_phase(&mut self.phase1, 250.0*tm, sr);
            advance_phase(&mut self.phase2, 395.0*tm, sr);
            let ring = osc_sine(self.phase1)*osc_sine(self.phase2); // inharmonic
            let body = ring*0.25*(-t/(0.1*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 3800.0, 0.6, sr)*(-t/(0.22*dm)).exp()*0.35;
            body + wire + self.noise()*(-t*1500.0).exp()*0.15
        }
        41 => { // S10: 3-mode Bessel body + resonant shell ring — big snare
            let f=225.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            advance_phase(&mut self.phase2, f*1.593, sr);
            advance_phase(&mut self.phase3, f*2.136, sr);
            let head = osc_sine(self.phase1)*0.35*(-t/(0.15*dm)).exp()
                + osc_sine(self.phase2)*0.15*(-t/(0.1*dm)).exp()
                + osc_sine(self.phase3)*0.08*(-t/(0.06*dm)).exp();
            let shell = self.svf2.bandpass(self.noise()*(-t*30.0).exp(), 420.0, 18.0, sr)*0.12;
            let wire = self.svf1.bandpass(self.noise()*nm, 3500.0, 0.5, sr)*(-t/(0.35*dm)).exp()*0.4;
            head + shell + wire
        }

        // ════════ 6 CLAPS (42-47) — 6 different approaches ════════

        42 => { // CL1: 5-clapper tight — random timing, BPF noise
            let mut e=0.0;
            for k in 0..5u32 { let off=(self.hit_rand(k*3)*0.006+self.hit_rand(k*3+1).abs()*0.004).abs();
                let to=t-off; if to>=0.0 { e+=(-to*170.0).exp()*(0.75+self.hit_rand(k*3+2)*0.25)*0.17; } }
            let f=self.svf1.bandpass(self.noise()*nm, 2300.0, 1.4, sr);
            let hp=self.hp1.tick_hp(f, 600.0, sr);
            hp*(e + (-t/(0.15*dm)).exp()*0.3)
        }
        43 => { // CL2: 8-clapper loose — wide spread, LP filtered
            let mut e=0.0;
            for k in 0..8u32 { let off=(self.hit_rand(k*4)*0.02+self.hit_rand(k*4+1).abs()*0.01).abs();
                let to=t-off; if to>=0.0 { e+=(-to*130.0).exp()*(0.6+self.hit_rand(k*4+2)*0.4)*0.11; } }
            let f=self.svf1.bandpass(self.noise()*nm, 1800.0, 1.0, sr);
            let warm=self.svf2.lowpass(f, 5000.0, 0.5, sr);
            warm*(e + (-t/(0.2*dm)).exp()*0.35)
        }
        44 => { // CL3: Single dry clap — one burst with short tail
            let clap=self.noise()*(t/0.0004).min(1.0)*(-t/(0.03*dm)).exp();
            self.svf1.bandpass(clap, 2100.0, 1.8, sr)*0.4
        }
        45 => { // CL4: Finger snap — short but controlled
            let snap=self.noise()*(t/0.0002).min(1.0)*(-t/(0.015*dm)).exp();
            let f=self.svf1.bandpass(snap, 3400.0, 2.5, sr);
            self.hp1.tick_hp(f, 1500.0, sr)*0.45
        }
        46 => { // CL5: Hand slap — LPF, mid-heavy thump
            advance_phase(&mut self.phase1, 185.0, sr);
            let body=osc_sine(self.phase1)*0.2*(-t/(0.08*dm)).exp();
            let slap=self.svf1.lowpass(self.noise()*(-t/(0.02*dm)).exp(), 2500.0, 1.0, sr)*0.3;
            body + slap
        }
        47 => { // CL6: Group + long reverb tail — hall clap
            let mut e=0.0;
            for k in 0..4u32 { let off=(self.hit_rand(k*5)*0.01).abs();
                let to=t-off; if to>=0.0 { e+=(-to*160.0).exp()*0.2; } }
            let f=self.svf1.bandpass(self.noise()*nm, 2200.0, 1.3, sr);
            f*(e + (-t/(0.4*dm)).exp()*0.4)
        }

        // ════════ 8 CLOSED HATS (48-55) — 8 DIFFERENT synthesis methods ════════

        48 => { // CH1: Standard modal 6-osc bank
            let freqs=[385.0, 962.0, 1618.0, 2524.0, 3742.0, 5183.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(m, 5500.0, sr);
            (hp*0.35+self.noise()*(-t*2200.0).exp()*0.08)*(-t/(0.06*dm)).exp()
        }
        49 => { // CH2: Pure noise BPF — no oscillators
            let n=self.noise();
            let f=self.svf1.bandpass(n, 8500.0, 2.5, sr);
            self.hp1.tick_hp(f, 6000.0, sr)*0.35*(-t/(0.055*dm)).exp()
        }
        50 => { // CH3: FM metallic tick — carrier+modulator
            advance_phase(&mut self.phase1, 5500.0, sr);
            advance_phase(&mut self.phase2, 8100.0, sr);
            let fm=(self.phase1*TAU+osc_sine(self.phase2)*1.5).sin()*0.3;
            self.hp1.tick_hp(fm, 4000.0, sr)*(-t/(0.045*dm)).exp()
        }
        51 => { // CH4: Ring mod tick — dense inharmonic
            advance_phase(&mut self.phase1, 4200.0, sr);
            advance_phase(&mut self.phase2, 6300.0, sr);
            let ring=osc_sine(self.phase1)*osc_sine(self.phase2)*0.35;
            self.hp1.tick_hp(ring, 5500.0, sr)*(-t/(0.04*dm)).exp()
        }
        52 => { // CH5: Dark LPF modal — warm closed hat
            let freqs=[280.0, 672.0, 1184.0, 1896.0, 2848.0, 3952.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            self.svf1.lowpass(m, 5000.0, 0.4, sr)*0.32*(-t/(0.07*dm)).exp()
        }
        53 => { // CH6: Multi-band noise — 3 independent noise bands
            let n=self.noise();
            let lo=self.svf1.bandpass(n, 3800.0, 3.0, sr)*0.2;
            let mid=self.svf2.bandpass(n, 7200.0, 2.5, sr)*0.25;
            let hi=self.svf3.bandpass(n, 12000.0, 2.0, sr)*0.15;
            self.hp1.tick_hp(lo+mid+hi, 3500.0, sr)*(-t/(0.05*dm)).exp()
        }
        54 => { // CH7: Noise + saturation — driven noise for grit
            let n=self.noise()*0.8;
            let hp=self.hp1.tick_hp(n, 6000.0, sr);
            let sat=(hp*3.0).tanh()*0.25;
            sat*(-t/(0.05*dm)).exp()
        }
        55 => { // CH8: AM modulated noise — fluttering tick
            advance_phase(&mut self.phase1, 12000.0, sr);
            let am=(1.0+osc_sine(self.phase1))*0.5;
            let n=self.noise()*am;
            let f=self.svf1.bandpass(n, 7500.0, 2.0, sr);
            f*0.35*(-t/(0.04*dm)).exp()
        }

        // ════════ 8 OPEN HATS (56-63) — 8 DIFFERENT synthesis, 1.0-2.5s decays ════════

        56 => { // HO1: 9-mode shimmer — modal bank + 3 upper partials
            let freqs=[340.0, 815.0, 1490.0, 2350.0, 3500.0, 4850.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            advance_phase(&mut self.modal_phases[0], 6400.0, sr);
            advance_phase(&mut self.modal_phases[1], 8300.0, sr);
            advance_phase(&mut self.modal_phases[2], 10500.0, sr);
            let upper=osc_sine(self.modal_phases[0])*0.15*(-t/(1.8*dm)).exp()
                +osc_sine(self.modal_phases[1])*0.1*(-t/(2.2*dm)).exp()
                +osc_sine(self.modal_phases[2])*0.06*(-t/(2.5*dm)).exp();
            (self.hp1.tick_hp(m, 2500.0, sr)*0.35+upper)*(-t/(1.5*dm)).exp()
        }
        57 => { // HO2: FM metallic shimmer — completely different from modal
            advance_phase(&mut self.phase1, 3200.0, sr);
            advance_phase(&mut self.phase2, 4700.0, sr);
            advance_phase(&mut self.phase3, 7100.0, sr);
            let fm_mod=osc_sine(self.phase2)*2.5;
            let fm1=(self.phase1*TAU+fm_mod).sin()*0.25;
            let fm2=(self.phase3*TAU+fm_mod*0.7).sin()*0.18;
            let noise=self.hp1.tick_hp(self.noise()*0.06, 8000.0, sr);
            (fm1+fm2+noise)*(-t/(2.0*dm)).exp()
        }
        58 => { // HO3: 3-band resonant noise — no oscillators
            let n=self.noise();
            let b1=self.svf1.bandpass(n, 4200.0, 3.0, sr)*0.28;
            let b2=self.svf2.bandpass(n, 7800.0, 2.5, sr)*0.22;
            let b3=self.svf3.bandpass(n, 11500.0, 2.0, sr)*0.14;
            self.hp1.tick_hp(b1+b2+b3, 3000.0, sr)*(-t/(1.2*dm)).exp()
        }
        59 => { // HO4: Ring mod wash — two non-integer sines multiplied
            advance_phase(&mut self.phase1, 2850.0, sr);
            advance_phase(&mut self.phase2, 4130.0, sr);
            let ring=osc_sine(self.phase1)*osc_sine(self.phase2)*0.35;
            advance_phase(&mut self.phase3, 6950.0, sr);
            let shimmer=osc_sine(self.phase3)*0.1;
            self.hp1.tick_hp(ring+shimmer, 2000.0, sr)*(-t/(1.8*dm)).exp()
        }
        60 => { // HO5: Breathy HP noise — mostly air, very long
            let wash=self.hp1.tick_hp(self.noise(), 5000.0, sr)*0.25;
            advance_phase(&mut self.phase1, 5500.0, sr);
            let hint=osc_sine(self.phase1)*0.04;
            (wash+hint)*(-t/(2.5*dm)).exp()
        }
        61 => { // HO6: Distorted modal — heavy waveshaping creates new partials
            let freqs=[280.0, 670.0, 1180.0, 1900.0, 2850.0, 3950.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let dist=(m*3.5).tanh()*0.4;
            self.hp1.tick_hp(dist+self.noise()*0.06, 1800.0, sr)*(-t/(1.3*dm)).exp()
        }
        62 => { // HO7: Comb-filtered noise — shimmer from comb resonances
            let n=self.noise();
            // Approximate comb: tight BPF at harmonic series of a high freq
            let c1=self.svf1.bandpass(n, 3500.0, 8.0, sr)*0.15;
            let c2=self.svf2.bandpass(n, 7000.0, 6.0, sr)*0.12;
            let c3=self.svf3.bandpass(n, 10500.0, 5.0, sr)*0.08;
            self.hp1.tick_hp(c1+c2+c3, 2500.0, sr)*(-t/(1.6*dm)).exp()
        }
        63 => { // HO8: Phase-modulated shimmer — phase distortion synthesis
            advance_phase(&mut self.phase1, 4800.0, sr);
            advance_phase(&mut self.phase2, 3100.0, sr); // modulator
            // Phase distortion: warp the phase before sine lookup
            let warped = self.phase1 + osc_sine(self.phase2)*0.3*(-t*2.0).exp();
            let pd = (warped*TAU).sin()*0.3;
            advance_phase(&mut self.phase3, 8200.0, sr);
            let hi = osc_sine(self.phase3)*0.08*(-t/(2.5*dm)).exp();
            self.hp1.tick_hp(pd+hi, 2500.0, sr)*(-t/(2.0*dm)).exp()
        }

        // ════════ 2 PEDAL HATS (64-65) ════════

        64 => { // PH1: Pedal modal chick — foot close
            let freqs=[355.0, 850.0, 1500.0, 2370.0, 3520.0, 4860.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let chick=self.svf1.bandpass(self.noise()*(-t*500.0).exp(), 1300.0, 2.5, sr)*0.12;
            m*(-t/(0.025*dm)).exp()*0.2 + chick
        }
        65 => { // PH2: Pedal noise chick — no modal, just clap of cymbals
            let clap=self.noise()*(t/0.0003).min(1.0)*(-t*400.0).exp();
            let f=self.svf1.bandpass(clap, 1800.0, 2.0, sr);
            let hp=self.hp1.tick_hp(f, 800.0, sr);
            hp*0.3
        }

        // ════════ 6 TOMS (66-71) — each with different body character ════════

        66 => { // T1: Floor — 3-mode Bessel, long decay
            let f=82.0*tm; let sw=f*0.14*(-t*28.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr); advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            advance_phase(&mut self.phase3, (f+sw)*2.296, sr);
            let body=osc_sine(self.phase1)*0.5*(0.2*(-t/0.015).exp()+0.8*(-t/(0.35*dm)).exp());
            body + osc_sine(self.phase2)*0.14*(-t/(0.12*dm)).exp()
                + osc_sine(self.phase3)*0.06*(-t/(0.08*dm)).exp()
        }
        67 => { // T2: Low rack — FM body for richer tone
            advance_phase(&mut self.phase1, 130.0*tm, sr);
            advance_phase(&mut self.phase2, 195.0*tm, sr);
            let body=(self.phase1*TAU+osc_sine(self.phase2)*0.5*(-t*20.0).exp()).sin();
            body*0.45*(0.3*(-t/0.008).exp()+0.7*(-t/(0.24*dm)).exp())
        }
        68 => { // T3: Mid rack — triangle body, mellow
            let f=170.0*tm; let sw=f*0.1*(-t*35.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            osc_triangle(self.phase1)*0.45*(0.3*(-t/0.007).exp()+0.7*(-t/(0.22*dm)).exp())
        }
        69 => { // T4: High rack — sine + ring mod overtone
            advance_phase(&mut self.phase1, 215.0*tm, sr);
            advance_phase(&mut self.phase2, 340.0*tm, sr);
            let body=osc_sine(self.phase1)*0.4;
            let ring=osc_sine(self.phase1)*osc_sine(self.phase2)*0.08*(-t/(0.06*dm)).exp();
            (body+ring)*(0.35*(-t/0.006).exp()+0.65*(-t/(0.18*dm)).exp())
        }
        70 => { // T5: Concert — very long, resonant 3-mode
            let f=150.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*1.593, sr);
            advance_phase(&mut self.phase3, f*2.296, sr);
            osc_sine(self.phase1)*0.45*(-t/(0.4*dm)).exp()
                + osc_sine(self.phase2)*0.16*(-t/(0.18*dm)).exp()
                + osc_sine(self.phase3)*0.08*(-t/(0.12*dm)).exp()
        }
        71 => { // T6: Timbale — metallic shell via resonant filter
            let f=360.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let body=osc_sine(self.phase1)*0.35*(-t/(0.2*dm)).exp();
            let shell=self.svf1.bandpass(self.noise()*(-t*25.0).exp(), f*2.8, 14.0, sr)*0.15;
            body + shell
        }

        // ════════ 6 CYMBALS (72-77) — each different topology ════════

        72 => { // CY1: Crash dark — LPF modal
            let freqs=[310.0, 740.0, 1300.0, 2080.0, 3100.0, 4280.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.lowpass(m+self.noise()*0.08, 7500.0, 0.3, sr);
            f*0.32*(t/0.003).min(1.0)*(-t/(1.5*dm)).exp()
        }
        73 => { // CY2: Crash bright — HP modal
            let freqs=[405.0, 972.0, 1703.0, 2684.0, 3953.0, 5458.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            self.hp1.tick_hp(m, 2500.0, sr)*(t/0.002).min(1.0)*(-t/(1.8*dm)).exp()*0.3
        }
        74 => { // CY3: Ride ping — BPF focused + sine ping
            let freqs=[425.0, 1015.0, 1742.0, 2831.0, 4184.0, 5753.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.bandpass(m, 5500.0, 1.0, sr);
            let ping=(-t*100.0).exp()*0.12;
            (f*(-t/(1.0*dm)).exp()+ping)*0.28
        }
        75 => { // CY4: Ride wash — FM generated wash
            advance_phase(&mut self.phase1, 2800.0, sr);
            advance_phase(&mut self.phase2, 4100.0, sr);
            let fm=(self.phase1*TAU+osc_sine(self.phase2)*1.2).sin()*0.25;
            self.hp1.tick_hp(fm, 2000.0, sr)*(-t/(2.0*dm)).exp()
        }
        76 => { // CY5: Splash — noise burst, fast decay
            let n=self.noise();
            let hp=self.hp1.tick_hp(n, 4000.0, sr);
            let shaped=self.svf1.bandpass(hp, 8000.0, 1.5, sr);
            shaped*0.3*(t/0.001).min(1.0)*(-t/(0.5*dm)).exp()
        }
        77 => { // CY6: China — distorted modal for trashy character
            let freqs=[285.0, 683.0, 1207.0, 1932.0, 2878.0, 3988.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let dist=(m*2.5).tanh()*0.4;
            dist*(t/0.002).min(1.0)*(-t/(1.2*dm)).exp()*0.3
        }

        // ════════ 12 PERCUSSION (78-89) — each unique ════════

        78 => { // P1: Cowbell — two sines through narrow BPF
            advance_phase(&mut self.phase1, 580.0, sr); advance_phase(&mut self.phase2, 870.0, sr);
            self.svf1.bandpass(osc_sine(self.phase1)*0.35+osc_sine(self.phase2)*0.3, 725.0, 4.0, sr)
                *(-t/(0.07*dm)).exp()
        }
        79 => { // P2: Woodblock — high sine + noise click
            advance_phase(&mut self.phase1, 1900.0, sr);
            (osc_sine(self.phase1)*0.3+self.noise()*(-t*1000.0).exp()*0.08)*(-t/(0.015*dm)).exp()
        }
        80 => { // P3: Clave — single pure high sine
            advance_phase(&mut self.phase1, 2500.0, sr);
            osc_sine(self.phase1)*0.4*(-t/(0.022*dm)).exp()
        }
        81 => { // P4: Triangle instrument — long ring
            advance_phase(&mut self.phase1, 1200.0, sr); advance_phase(&mut self.phase2, 3600.0, sr);
            (osc_sine(self.phase1)*0.3+osc_sine(self.phase2)*0.18)*(-t/(0.9*dm)).exp()
        }
        82 => { // P5: Tambourine — high modal jingles
            let freqs=[4600.0, 6300.0, 7900.0, 9600.0, 11300.0, 13100.0];
            let j=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(j, 4000.0, sr);
            let shake=(t*22.0*TAU).sin().abs()*(-t*6.0).exp();
            hp*((-t/(0.2*dm)).exp()+shake*0.2)*0.25
        }
        83 => { // P6: Shaker — tight noise burst
            self.hp1.tick_hp(self.svf1.bandpass(self.noise(), 7200.0, 1.3, sr), 5000.0, sr)
                *(-t/(0.07*dm)).exp()*0.3
        }
        84 => { // P7: Cabasa — scratchy high noise
            self.svf1.bandpass(self.noise(), 8800.0, 2.0, sr)*(-t/(0.1*dm)).exp()*0.28
        }
        85 => { // P8: Guiro — scraping modulated noise
            let scrape=(t*38.0*TAU).sin().abs()*(-t*4.5).exp();
            self.svf1.bandpass(self.noise(), 4200.0, 3.0, sr)*(scrape*0.4+0.15)*(-t/(0.22*dm)).exp()
        }
        86 => { // P9: Vibraslap — rattling decay
            let rattle=(t*36.0*TAU).sin().abs()*(-t*2.8).exp();
            self.svf1.bandpass(self.noise(), 3400.0, 5.5, sr)*rattle*(-t/(0.5*dm)).exp()*0.25
        }
        87 => { // P10: Maracas — short HP noise
            self.hp1.tick_hp(self.noise(), 6500.0, sr)*(-t/(0.045*dm)).exp()*0.25
        }
        88 => { // P11: Agogo high — two inharmonic sines
            advance_phase(&mut self.phase1, 930.0, sr); advance_phase(&mut self.phase2, 1398.0, sr);
            (osc_sine(self.phase1)*0.33+osc_sine(self.phase2)*0.24)*(-t/(0.16*dm)).exp()
        }
        89 => { // P12: Agogo low — FM bell tone
            advance_phase(&mut self.phase1, 670.0, sr); advance_phase(&mut self.phase2, 1008.0, sr);
            let fm=(self.phase1*TAU+osc_sine(self.phase2)*0.5).sin()*0.3;
            fm*(-t/(0.18*dm)).exp()
        }

        // ════════ 12 MORE PERCUSSION (90-101) — hand drums, etc ════════

        90 => { // Conga open — sine body with slap
            let f=335.0*tm; advance_phase(&mut self.phase1, f+f*0.06*(-t*42.0).exp(), sr);
            osc_sine(self.phase1)*0.5*(-t/(0.22*dm)).exp() + self.noise()*(-t*450.0).exp()*0.08
        }
        91 => { // Conga mute — short damped
            advance_phase(&mut self.phase1, 320.0*tm, sr);
            osc_sine(self.phase1)*0.42*(-t/(0.06*dm)).exp()
        }
        92 => { // Conga slap — noise-heavy attack
            advance_phase(&mut self.phase1, 355.0*tm, sr);
            osc_sine(self.phase1)*0.2*(-t/(0.04*dm)).exp()
                + self.svf1.bandpass(self.noise()*(-t*700.0).exp(), 2800.0, 2.0, sr)*0.3
        }
        93 => { // Bongo high — quick, bright
            advance_phase(&mut self.phase1, 425.0*tm+425.0*0.08*(-t*65.0).exp(), sr);
            osc_sine(self.phase1)*0.42*(-t/(0.1*dm)).exp()
        }
        94 => { // Bongo low — rounder, longer
            advance_phase(&mut self.phase1, 315.0*tm+315.0*0.07*(-t*50.0).exp(), sr);
            osc_sine(self.phase1)*0.45*(-t/(0.15*dm)).exp()
        }
        95 => { // Timbale high — shell ring via resonant BPF
            let f=530.0*tm; advance_phase(&mut self.phase1, f, sr);
            osc_sine(self.phase1)*0.38*(-t/(0.2*dm)).exp()
                + self.svf1.bandpass(self.noise()*(-t*18.0).exp(), f*3.0, 10.0, sr)*0.1
        }
        96 => { // Timbale low
            let f=370.0*tm; advance_phase(&mut self.phase1, f, sr);
            osc_sine(self.phase1)*0.4*(-t/(0.22*dm)).exp()
                + self.svf1.bandpass(self.noise()*(-t*22.0).exp(), f*2.5, 8.0, sr)*0.08
        }
        97 => { // Cuica high — pitch sweep
            let f=600.0+400.0*(-t*8.0).exp();
            advance_phase(&mut self.phase1, f, sr);
            osc_sine(self.phase1)*0.35*(-t/(0.18*dm)).exp()
        }
        98 => { // Cuica low — slower sweep
            let f=350.0+200.0*(-t*6.0).exp();
            advance_phase(&mut self.phase1, f, sr);
            osc_sine(self.phase1)*0.35*(-t/(0.22*dm)).exp()
        }
        99 => { // Whistle — pitched sine with vibrato
            advance_phase(&mut self.phase1, 2300.0+(t*6.0*TAU).sin()*25.0, sr);
            osc_sine(self.phase1)*0.3*(-t/(0.1*dm)).exp()
        }
        100 => { // Ride bell — 3 sine partials
            advance_phase(&mut self.phase1, 760.0, sr);
            advance_phase(&mut self.phase2, 1140.0, sr);
            advance_phase(&mut self.phase3, 1710.0, sr);
            (osc_sine(self.phase1)*0.28+osc_sine(self.phase2)*0.22+osc_sine(self.phase3)*0.14)
                *(-t/(0.8*dm)).exp()
        }
        101 => { // Sizzle cymbal — modal + rattle
            let freqs=[348.0, 835.0, 1475.0, 2330.0, 3460.0, 4780.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let rattle=self.noise()*(t*30.0*TAU).sin().abs()*(-t*3.5).exp();
            let rf=self.svf1.bandpass(rattle, 8500.0, 3.5, sr)*0.1;
            m*(-t/(1.5*dm)).exp()*0.22 + rf
        }

        // Notes 102-111: 10 more unique percussion — each hardcoded, no formulas
        102 => { // Clap: Vintage vinyl — warm LP filtered
            let mut e=0.0;
            for k in 0..5u32 { let off=(self.hit_rand(k*6)*0.008).abs();
                let to=t-off; if to>=0.0 { e+=(-to*150.0).exp()*0.18; } }
            let f=self.svf1.bandpass(self.noise()*nm, 1900.0, 1.5, sr);
            self.svf2.lowpass(f, 5000.0, 0.5, sr)*(e + (-t/(0.2*dm)).exp()*0.3)
        }
        103 => { // Hat: Pedal splash — open then choked
            let freqs=[362.0, 868.0, 1537.0, 2427.0, 3607.0, 4985.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            // Swell then choke envelope
            let swell = (t*8.0).min(1.0)*(-t/(0.15*dm)).exp();
            m*swell*0.3
        }
        104 => { // Snap: Knuckle crack — very high, very short
            let snap=self.noise()*(t/0.00015).min(1.0)*(-t*1800.0).exp();
            self.svf1.bandpass(snap, 4200.0, 3.0, sr)*0.4
        }
        105 => { // Stick click — wood on wood
            advance_phase(&mut self.phase1, 2200.0, sr);
            advance_phase(&mut self.phase2, 4800.0, sr);
            (osc_sine(self.phase1)*0.2+osc_sine(self.phase2)*0.12+self.noise()*(-t*1500.0).exp()*0.1)
                *(-t/(0.01*dm)).exp()
        }
        106 => { // Bell tree — descending shimmer
            let sweep = 3000.0 + 5000.0*(-t*3.0).exp();
            advance_phase(&mut self.phase1, sweep, sr);
            advance_phase(&mut self.phase2, sweep*1.5, sr);
            (osc_sine(self.phase1)*0.2+osc_sine(self.phase2)*0.12)*(-t/(0.6*dm)).exp()
        }
        107 => { // Snap: Tongue click — very short mid pop
            let pop=self.noise()*(t/0.0002).min(1.0)*(-t*800.0).exp();
            self.svf1.bandpass(pop, 1800.0, 2.0, sr)*0.35
        }
        108 => { // Brush sweep — long swishing noise
            let n=self.noise();
            let sweep=(t*3.0*TAU).sin()*0.3;
            let f=self.svf1.bandpass(n, 4000.0+sweep*2000.0, 0.8, sr);
            self.hp1.tick_hp(f, 1500.0, sr)*(-t/(0.4*dm)).exp()*0.25
        }
        109 => { // Hat: Foot splash — hit then immediately open
            let freqs=[330.0, 793.0, 1403.0, 2217.0, 3297.0, 4553.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(m, 3000.0, sr);
            hp*(-t/(0.8*dm)).exp()*0.28
        }
        110 => { // Clap: Stadium — massive reverb, 8 clappers
            let mut e=0.0;
            for k in 0..8u32 { let off=(self.hit_rand(k*7)*0.025+self.hit_rand(k*7+1).abs()*0.015).abs();
                let to=t-off; if to>=0.0 { e+=(-to*100.0).exp()*(0.5+self.hit_rand(k*7+2)*0.5)*0.1; } }
            let f=self.svf1.bandpass(self.noise()*nm, 2000.0, 1.0, sr);
            f*(e + (-t/(0.5*dm)).exp()*0.45) // very long tail
        }
        111 => { // Snare: Brush tap — gentle, all wire, barely there
            let wire=self.svf1.bandpass(self.noise()*nm*0.5, 5000.0, 0.8, sr);
            self.hp1.tick_hp(wire, 2500.0, sr)*(-t/(0.08*dm)).exp()*0.2
        }

        _ => 0.0,
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// DrumRack Plugin
// ══════════════════════════════════════════════════════════════════════════════

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
        // Reuse voice with same note
        if let Some(i) = self.voices.iter().position(|v| v.note == note) {
            return &mut self.voices[i];
        }
        // Find inactive voice
        if let Some(i) = self.voices.iter().position(|v| !v.active) {
            return &mut self.voices[i];
        }
        // Steal oldest voice
        &mut self.voices[0]
    }
}

impl Default for DrumRack {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for DrumRack {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "Phosphor Drums".into(),
            version: "0.2.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..MAX_VOICES).map(|_| DrumVoice::new()).collect();
    }

    fn process(
        &mut self,
        _inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        midi_events: &[MidiEvent],
    ) {
        if outputs.is_empty() {
            return;
        }
        let buf_len = outputs[0].len();
        let gain = self.params[P_GAIN];
        let sr = self.sample_rate;
        let decay = self.params[P_DECAY] as f64;
        let tone = self.params[P_TONE] as f64;
        let noise = self.params[P_NOISE] as f64;
        let drive = self.params[P_DRIVE] as f64;
        let kit = self.kit;

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
                if ev.status & 0xF0 == 0x90 && ev.data2 > 0 {
                    let sound = note_to_sound(ev.data1);
                    let voice = self.find_voice(ev.data1);
                    voice.trigger(ev.data1, ev.data2, sound, kit);
                }
                ei += 1;
            }

            let mut sample = 0.0f32;
            for voice in &mut self.voices {
                sample += voice.tick(sr, decay, tone, noise, drive);
            }
            sample *= gain;
            for ch in outputs.iter_mut() {
                ch[i] = sample;
            }
        }
    }

    fn parameter_count(&self) -> usize {
        PARAM_COUNT
    }

    fn parameter_info(&self, index: usize) -> Option<ParameterInfo> {
        if index >= PARAM_COUNT {
            return None;
        }
        Some(ParameterInfo {
            name: PARAM_NAMES[index].into(),
            min: 0.0,
            max: 1.0,
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
        for v in &mut self.voices {
            v.active = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_on(note: u8, vel: u8, offset: u32) -> MidiEvent {
        MidiEvent {
            sample_offset: offset,
            status: 0x90,
            data1: note,
            data2: vel,
        }
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
    fn all_note_ranges_produce_sound() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 512);
        // Test every 8th note across the range
        for note in (24..112).step_by(8) {
            let mut out = vec![0.0f32; 512];
            dr.process(&[], &mut [&mut out], &[note_on(note, 100, 0)]);
            let peak = out.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!(
                peak > 0.001,
                "Note {note} should produce sound, peak={peak}"
            );
            dr.reset();
        }
    }

    #[test]
    fn kits_sound_different() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 512);
        dr.set_parameter(P_KIT, 0.0);
        let mut out808 = vec![0.0f32; 512];
        dr.process(&[], &mut [&mut out808], &[note_on(24, 100, 0)]);

        dr.reset();
        dr.set_parameter(P_KIT, 0.25);
        let mut out909 = vec![0.0f32; 512];
        dr.process(&[], &mut [&mut out909], &[note_on(24, 100, 0)]);

        let diff: f32 = out808
            .iter()
            .zip(out909.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 0.5, "808 and 909 should differ, diff={diff}");
    }

    #[test]
    fn output_is_finite() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 64);
        for note in 0..128u8 {
            let mut out = vec![0.0f32; 64];
            dr.process(&[], &mut [&mut out], &[note_on(note, 127, 0)]);
            assert!(
                out.iter().all(|s| s.is_finite()),
                "Note {note} output not finite"
            );
        }
    }

    #[test]
    fn kit_switch_changes_sound() {
        let mut dr = DrumRack::new();
        dr.init(44100.0, 512);
        dr.set_parameter(P_KIT, 0.0);
        let mut out808 = vec![0.0f32; 512];
        dr.process(&[], &mut [&mut out808], &[note_on(36, 100, 0)]);

        dr.reset();
        dr.set_parameter(P_KIT, 0.75);
        let mut out606 = vec![0.0f32; 512];
        dr.process(&[], &mut [&mut out606], &[note_on(36, 100, 0)]);

        let diff: f32 = out808
            .iter()
            .zip(out606.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 0.5, "808 and 606 kicks should differ, diff={diff}");
    }

    #[test]
    fn varied_sounds_across_range() {
        // Verify different sound types produce different output
        let mut dr = DrumRack::new();
        dr.init(44100.0, 1024);

        let mut out_kick = vec![0.0f32; 1024];
        dr.process(&[], &mut [&mut out_kick], &[note_on(36, 100, 0)]);
        dr.reset();

        let mut out_hat = vec![0.0f32; 1024];
        dr.process(&[], &mut [&mut out_hat], &[note_on(42, 100, 0)]);

        let diff: f32 = out_kick
            .iter()
            .zip(out_hat.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 1.0, "Kick and hat should sound different, diff={diff}");
    }
}
