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
        };

        // Auto-deactivate if very quiet
        if self.time > 0.01 && sample.abs() < 0.0001 {
            self.active = false;
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

        let mut events: Vec<&MidiEvent> = midi_events.iter().collect();
        events.sort_by_key(|e| e.sample_offset);
        let mut ei = 0;

        for i in 0..buf_len {
            while ei < events.len() && events[ei].sample_offset as usize <= i {
                let ev = events[ei];
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
