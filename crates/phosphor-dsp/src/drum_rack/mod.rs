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
pub(crate) fn white_noise(seed: u64) -> f64 {
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
pub(crate) fn advance_phase(phase: &mut f64, freq: f64, sr: f64) {
    *phase += freq / sr;
    *phase -= (*phase).floor();
}

/// Sine oscillator.
#[inline]
pub(crate) fn osc_sine(phase: f64) -> f64 {
    (phase * TAU).sin()
}

/// Square wave oscillator (band-limited via first few harmonics approximation).
#[inline]
pub(crate) fn osc_square(phase: f64) -> f64 {
    if phase < 0.5 { 1.0 } else { -1.0 }
}

/// Triangle wave oscillator.
#[inline]
pub(crate) fn osc_triangle(phase: f64) -> f64 {
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
pub(crate) fn soft_clip(x: f64, drive: f64) -> f64 {
    let gained = x * (1.0 + drive * 8.0);
    gained / (1.0 + gained.abs()).sqrt()
}

/// Simple one-pole low-pass filter state.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OnePole {
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
pub(crate) struct Svf {
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
pub(crate) enum DrumSound {
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
pub(crate) struct HatOscillators {
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
pub(crate) const HAT_FREQS_808: [f64; 6] = [205.0, 304.0, 370.0, 523.0, 540.0, 800.0];

/// 606 hat oscillator frequencies (higher, thinner).
pub(crate) const HAT_FREQS_606: [f64; 6] = [10200.0, 10800.0, 11300.0, 11800.0, 12100.0, 12500.0];

#[derive(Debug)]
pub(crate) struct DrumVoice {
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
pub(crate) struct T5Recipe {
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

pub(crate) const T5_DEFAULT: T5Recipe = T5Recipe {
    exciter: 0, impulse_level: 1.0, noise_level: 0.5, noise_decay: 0.01,
    burst_count: 0, burst_spread: 0.0,
    r1_freq: 0.0, r1_q: 5.0, r1_level: 0.5, r1_decay: 0.15, pitch_sweep: 0.0, pitch_time: 0.02,
    r2_freq: 0.0, r2_q: 3.0, r2_level: 0.3, r2_decay: 0.1,
    r3_freq: 0.0, r3_q: 2.0, r3_level: 0.2, r3_decay: 0.08,
    noise_filter_freq: 0.0, noise_mix: 0.0, noise_filter_decay: 0.15, wire_coupling: 0.0,
};

pub(crate) fn t5_recipe(note: u8) -> T5Recipe {
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

}

// Per-kit synthesis methods live in separate files under racks/
mod racks;

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
