//! 707 kit synthesis.
//!
//! The TR-707 is not an analog drum machine, so modelling it with analog
//! circuits — which is what this file used to do, with nineteen of its
//! twenty-five sounds taken from the 808's boards — gets it wrong at the first
//! step. It is fifteen recordings in four mask ROMs, read out by an address
//! counter into a converter, and then *contoured by analog envelope generators
//! after that converter*.
//!
//! **These are not Roland's samples.** They cannot be: the ROM is Roland's
//! and copyrighted. What is modelled here is the shape of the machine around
//! the sample — the two artefacts of its converter and the analog stage after
//! it — with a synthesised source standing in for each recording:
//!
//! 1. **A word length.** Eight bits, and six for the crash and the ride. The
//!    quantisation is applied to the source before the envelope, because that
//!    is where the hardware has it — see [`DrumVoice::quantize`].
//! 2. **A clock ceiling.** 25 kHz, held between conversions with nothing but
//!    the output filter after it. Everything above 12.5 kHz that the machine
//!    could have stored simply is not there, and the hold is what puts the
//!    hardness into the top.
//! 3. **An abrupt start.** A recording begins at the sample it begins at.
//!    There is no attack ramp anywhere in this file.
//! 4. **The analog envelope after the DAC.** On the crash, the ride and the
//!    hi-hats the stored waveform does not decay; the decay is a VCA on the
//!    other side of the converter. On the drums the decay is in the data. The
//!    difference is audible and it is the reason the machine was built this
//!    way: an envelope after the converter takes the quantisation noise down
//!    with the note instead of leaving it under the tail.
//!
//! The front panel has a fader per instrument and an accent, and nothing else
//! — no tuning, no tone, no decay. `DrumKit::is_live` says so.

use super::super::*;

/// The analog output filter, after the hold. The converter runs at 25 kHz and
/// the first image sits at 25 kHz minus the highest thing in the sample, so
/// this is what keeps them out — three poles of it, because two is not enough
/// behind a zero-order hold.
const RECON_HZ: f64 = 11_000.0;

// ── The fifteen ──
//
// Two bass drums, two snares, a rimshot, three toms, closed and open hi-hat,
// crash, ride, hand clap, cowbell and tambourine. The machine has no conga,
// no bongo, no clave and no maracas — the TR-727 is the one with those — so
// those parts fold onto the nearest of the fifteen, at that sound's fader.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Voice707 {
    Bd1,
    Bd2,
    Sd1,
    Sd2,
    Rim,
    LowTom,
    MidTom,
    HighTom,
    ClosedHat,
    OpenHat,
    Crash,
    Ride,
    Clap,
    Cowbell,
    Tambourine,
}

impl Voice707 {
    /// The word length this sound is stored at. The crash and the ride are
    /// six bits where the rest are eight — they are the longest sounds in the
    /// ROM and something had to give.
    ///
    /// The sources disagree about this: the machine is documented both as
    /// eight bits with six-bit cymbals and as six bits throughout. The split
    /// is the one taken here because it is the one that explains the machine's
    /// own behaviour — the cymbals are the sounds that got the analog envelope
    /// generators, and an envelope after the converter is what you add when
    /// the quantisation noise is loud enough to need taking down.
    fn bits(self) -> u32 {
        match self {
            Self::Crash | Self::Ride => PCM_707_CYMBAL_BITS,
            _ => PCM_707_BITS,
        }
    }

    /// The time constant of the analog envelope generator after the converter,
    /// or `None` for the sounds whose decay is in the stored data.
    fn post_tau(self) -> Option<f64> {
        Some(match self {
            Self::ClosedHat => 0.055 / DECAY_REFERENCE,
            Self::OpenHat => 0.420 / DECAY_REFERENCE,
            Self::Crash => 1.300 / DECAY_REFERENCE,
            Self::Ride => 0.900 / DECAY_REFERENCE,
            _ => return None,
        })
    }

    /// A tag to index this sound's ROM by, so no two of them read the same
    /// words.
    fn tag(self) -> u64 {
        self as u64 + 1
    }
}

/// What this machine's voices ring for, in seconds to −20 dB.
///
/// A TR-707 has no decay control anywhere on it, so none of these move: they
/// are the analog envelope generators after the converter for the four sounds
/// that have one, and the decay written into the data for the rest. Printing
/// them turns the panel into the machine's own spec sheet, which is more use
/// than a percentage on a knob that does nothing.
pub(crate) fn decay_seconds(index: usize) -> Option<f64> {
    Some(match index {
        P_BD_DECAY => 0.230,
        P_LT_DECAY => 0.300,
        P_MT_DECAY => 0.260,
        P_HT_DECAY => 0.220,
        P_CY_DECAY => 1.300,
        P_OH_DECAY => 0.420,
        P_CH_DECAY => 0.055,
        _ => return None,
    })
}

/// Which of the fifteen a note plays.
///
/// The folds are onto the nearest sound in the same ROM — congas and bongos
/// onto the toms they sit between, claves onto the rimshot, shakers and
/// whistles onto the tambourine, agogos onto the cowbell — never onto another
/// machine's circuit.
pub(crate) fn voice_707(sound: DrumSound) -> Voice707 {
    use DrumSound as S;
    match sound {
        S::Kick => Voice707::Bd1,
        S::SubKick(_) => Voice707::Bd2,
        S::Snare => Voice707::Sd1,
        S::SnareAlt => Voice707::Sd2,
        S::Rimshot | S::Clave => Voice707::Rim,
        S::LowTom => Voice707::LowTom,
        S::MidTom => Voice707::MidTom,
        S::HighTom => Voice707::HighTom,
        S::Conga(f) | S::Bongo(f) | S::Timbale(f) => {
            if f < 260.0 {
                Voice707::LowTom
            } else if f < 360.0 {
                Voice707::MidTom
            } else {
                Voice707::HighTom
            }
        }
        S::ClosedHat | S::PedalHat => Voice707::ClosedHat,
        S::OpenHat => Voice707::OpenHat,
        S::Crash | S::Splash | S::Cymbal => Voice707::Crash,
        S::Ride | S::RideBell => Voice707::Ride,
        S::Clap => Voice707::Clap,
        S::Cowbell | S::Agogo(_) => Voice707::Cowbell,
        S::Tambourine
        | S::Maracas
        | S::Cabasa
        | S::Vibraslap
        | S::Guiro(_)
        | S::Whistle(_)
        | S::FxNoise(_) => Voice707::Tambourine,
    }
}

impl DrumVoice {
    // 707 synthesis — a converter and what is on each side of it
    // ══════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_707(&mut self, sr: f64, c: &Controls) -> f64 {
        let voice = voice_707(self.sound);

        // The digital side: read a word, quantise it, hold it. Everything
        // inside here runs at the playback clock, not at the sample rate.
        if self.convert(sr, PCM_707_RATE) {
            let word = self.rom_707(voice);
            self.dac_hold = Self::quantize(word, voice.bits());
        }

        // The analog side: the output filter, then the envelope generator for
        // the four sounds that have one.
        let smoothed = self.svf3.lowpass(self.dac_hold, RECON_HZ, 0.707, sr);
        let filtered = self.lp1.tick_lp(smoothed, RECON_HZ, sr);
        let out = match voice.post_tau() {
            Some(tau) => {
                // Accent reaches these four, because their envelope is a
                // capacitor the trigger charges, as on the analog machines.
                let env = (-self.time / (tau * self.accent_stretch())).exp();
                if env < 0.001 {
                    self.active = false;
                    return 0.0;
                }
                filtered * env
            }
            // Decay in the data, so accent is level and only level: a
            // recording cannot be made longer by hitting the pad harder. The
            // voice is freed by the rack's peak follower, which is what that
            // follower is for.
            None => filtered,
        };

        // DRIVE is the rack's control rather than Roland's, and it goes where
        // it goes on the other kits: across the bass drum.
        if c.drive > 0.01 && matches!(voice, Voice707::Bd1 | Voice707::Bd2) {
            soft_clip(out, c.drive * 2.0)
        } else {
            out
        }
    }

    /// One word out of ROM.
    ///
    /// Called at 25 kHz, so every phase and filter in here is advanced at
    /// `PCM_707_RATE`: these are the recordings, and a recording's own clock
    /// is the one it was made at.
    fn rom_707(&mut self, voice: Voice707) -> f64 {
        const RATE: f64 = PCM_707_RATE;
        let t = self.time;
        let tag = voice.tag();
        match voice {
            // ── Bass drums: short, dry, no ring. Two of them, the second
            // tighter and higher, which is what the two BD pads are for.
            Voice707::Bd1 => {
                let f = 52.0 + 42.0 * (-t / 0.018).exp();
                advance_phase(&mut self.phase1, f, RATE);
                let beater = self.svf1.bandpass(self.rom_noise(tag), 2400.0, 1.1, RATE)
                    * (-t / 0.0035).exp();
                let env = (-t / (0.230 / DECAY_REFERENCE)).exp();
                (osc_sine(self.phase1) * 0.95 + beater * 0.45) * env
            }
            Voice707::Bd2 => {
                let mult = match self.sound {
                    DrumSound::SubKick(m) => m,
                    _ => 1.0,
                };
                let f = (62.0 + 50.0 * (-t / 0.012).exp()) * mult;
                advance_phase(&mut self.phase1, f, RATE);
                let beater = self.svf1.bandpass(self.rom_noise(tag), 3200.0, 1.0, RATE)
                    * (-t / 0.0025).exp();
                let env = (-t / (0.150 / DECAY_REFERENCE)).exp();
                (osc_sine(self.phase1) * 0.9 + beater * 0.55) * env
            }
            // ── Snares: two shell modes and a wire band, dry and mid-heavy.
            Voice707::Sd1 => self.rom_707_snare(tag, 208.0, 337.0, 0.170, 2000.0),
            Voice707::Sd2 => self.rom_707_snare(tag, 246.0, 396.0, 0.130, 2600.0),
            // ── Rimshot: 25 ms of stick and shell, and nothing after it.
            Voice707::Rim => {
                advance_phase(&mut self.phase1, 415.0, RATE);
                advance_phase(&mut self.phase2, 1720.0, RATE);
                let crack =
                    self.svf1.bandpass(self.rom_noise(tag), 3600.0, 1.4, RATE) * (-t / 0.0015).exp();
                let env = (-t / (0.030 / DECAY_REFERENCE)).exp();
                (osc_sine(self.phase1) * 0.45 + osc_sine(self.phase2) * 0.35 + crack * 0.8) * env
            }
            // ── Toms: a recorded drum, so a little shell noise under the
            // head and a short drop in pitch.
            Voice707::LowTom => self.rom_707_tom(tag, 98.0, 0.300),
            Voice707::MidTom => self.rom_707_tom(tag, 142.0, 0.260),
            Voice707::HighTom => self.rom_707_tom(tag, 196.0, 0.220),
            // ── Hi-hats: the stored waveform does not decay. Both hats are
            // one instrument recorded once — the ROM holds them as two
            // entries, but they are the same cymbal — so they read the same
            // words and differ in the envelope after the converter and in how
            // much of the top survives the closed one's shorter one.
            Voice707::ClosedHat | Voice707::OpenHat => {
                let n = self.rom_noise(Voice707::ClosedHat.tag());
                let body = self.svf1.bandpass(n, 7600.0, 0.8, RATE);
                let edge = self.svf2.bandpass(n, 10_500.0, 1.2, RATE);
                (body * 0.8 + edge * 0.58) * 0.85
            }
            // ── Crash: a wash, six bits of it, non-decaying in the data.
            Voice707::Crash => {
                let n = self.rom_noise(tag);
                let wash = self.svf1.bandpass(n, 5200.0, 0.5, RATE);
                let air = self.svf2.bandpass(n, 9400.0, 0.7, RATE);
                (wash * 0.95 + air * 0.7) * 0.8
            }
            // ── Ride: the same six bits, but a stick on a bell rather than a
            // wash — the ping is a narrow pair of partials over it.
            Voice707::Ride => {
                let n = self.rom_noise(tag);
                advance_phase(&mut self.phase1, 1180.0, RATE);
                advance_phase(&mut self.phase2, 2650.0, RATE);
                let ping = (osc_sine(self.phase1) * 0.5 + osc_sine(self.phase2) * 0.35)
                    * (0.35 + 0.65 * (-t / 0.045).exp());
                let wash = self.svf1.bandpass(n, 6400.0, 0.6, RATE);
                (ping * 0.68 + wash * 0.75) * 0.8
            }
            // ── Hand clap: a recording of a clap, so its four bursts are in
            // the data and every hit has them in the same places — which is
            // exactly what the 808's clap, retriggered by an oscillator, does
            // not do.
            Voice707::Clap => {
                let n = self.svf1.bandpass(self.rom_noise(tag), 1350.0, 1.6, RATE);
                let bursts = if t < 0.034 {
                    let within = t % 0.0085;
                    (-within / 0.0028).exp()
                } else {
                    0.0
                };
                let room = (-t / (0.075 / DECAY_REFERENCE)).exp() * 0.35;
                n * (bursts + room)
            }
            // ── Cowbell: two partials, struck and gone.
            Voice707::Cowbell => {
                advance_phase(&mut self.phase1, 545.0, RATE);
                advance_phase(&mut self.phase2, 812.0, RATE);
                let raw = osc_square(self.phase1) * 0.5 + osc_square(self.phase2) * 0.5;
                let body = self.svf1.bandpass(raw, 950.0, 1.1, RATE);
                let env = 0.5 * (-t / 0.008).exp() + 0.5 * (-t / (0.130 / DECAY_REFERENCE)).exp();
                body * env * 1.2
            }
            // ── Tambourine: jingles, so a bright band with a fast rattle in
            // it and a short tail.
            Voice707::Tambourine => {
                let n = self.rom_noise(tag);
                let jingle = self.svf1.bandpass(n, 8800.0, 1.0, RATE);
                let rattle = 0.65 + 0.35 * (t * 190.0 * std::f64::consts::TAU).sin().abs();
                let env = (-t / (0.150 / DECAY_REFERENCE)).exp();
                jingle * rattle * env * 1.15
            }
        }
    }

    /// The two snares, which differ only in tuning, wire brightness and
    /// length. Both are dry: a 707 snare has no room on it.
    fn rom_707_snare(&mut self, tag: u64, low: f64, high: f64, seconds: f64, wire_hp: f64) -> f64 {
        const RATE: f64 = PCM_707_RATE;
        let t = self.time;
        advance_phase(&mut self.phase1, low, RATE);
        advance_phase(&mut self.phase2, high, RATE);
        let head = (osc_sine(self.phase1) * 0.6 + osc_sine(self.phase2) * 0.4)
            * (-t / (0.070 / DECAY_REFERENCE)).exp();
        let wires = self.hp1.tick_hp(self.rom_noise(tag), wire_hp, RATE)
            * (-t / (seconds / DECAY_REFERENCE)).exp();
        head * 0.6 + wires * 0.95
    }

    /// One tom: head, a drop in pitch over the first few milliseconds, and
    /// the shell under it.
    fn rom_707_tom(&mut self, tag: u64, freq: f64, seconds: f64) -> f64 {
        const RATE: f64 = PCM_707_RATE;
        let t = self.time;
        let f = freq * (1.0 + 0.18 * (-t / 0.014).exp());
        advance_phase(&mut self.phase1, f, RATE);
        let shell = self.hp1.tick_hp(self.rom_noise(tag), 1800.0, RATE) * (-t / 0.005).exp();
        let env = (-t / (seconds / DECAY_REFERENCE)).exp();
        (osc_sine(self.phase1) * 0.95 + shell * 0.3) * env
    }
}
