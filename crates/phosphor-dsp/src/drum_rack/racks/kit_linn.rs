//! LinnDrum kit synthesis.
//!
//! Roger Linn's second machine, 1982. It is a sampler, and it is the machine
//! most of what people mean by "eighties drums" came out of. It is often
//! called the LM-2; Linn never did — the LM-1 was the first machine and this
//! one is the LinnDrum.
//!
//! **These are not Linn's samples.** They cannot be: the ROM holds recordings
//! of a real kit and they are not ours to ship. What is modelled here is the
//! shape of the machine around the recording, the same way the 707 is done —
//! and the shape is a different one, because the converter is different:
//!
//! 1. **A companded word length.** Eight bits, but through an AM6070, which is
//!    a µ-255 law converter: sign, three-bit chord, four-bit step. Its step
//!    size follows the signal instead of being fixed, which is why a LinnDrum
//!    tail stays smooth where a linear eight-bit tail grows hash under it —
//!    see [`compand`], which carries the arithmetic and the trade.
//! 2. **A clock, and tuning as a change to it.** 28 kHz to 35 kHz. The TUNING
//!    section covers the snare, the sidestick, the three toms and the two
//!    congas, about an octave of travel, and it works by reading the ROM
//!    faster or slower. Pitch and length therefore move *together*: a tuned-up
//!    LinnDrum tom is shorter as well as higher, which no analog tuning knob
//!    anywhere else in this rack does.
//! 3. **An analog envelope on the hi-hat only.** The closed hi-hat has a decay
//!    knob on the front panel "to simulate different pressures on the pedal",
//!    and it is the one contour on the machine that is not in the data.
//! 4. **A recording of an acoustic kit.** The 707's sounds are dry mid-eighties
//!    electronic drums; these are close-miked studio drums with a little room
//!    on the snare and the clap. Character, not circuits.
//!
//! Which sound sits at 28 kHz and which at 35 is not published. What is
//! published is that the LM-1's sounds ran at 28 kHz and that the LinnDrum
//! raised the rate; the split taken here is drums at 35 and everything metal
//! or shaken at 28, on the grounds that the long sounds are the ones that had
//! to fit in the ROM. It is stated as a choice rather than dressed up as a
//! measurement.

use super::super::*;

/// The two hi-hat envelope generators. The closed one is the panel knob's,
/// 25 ms to 210 ms; the open one is fixed.
const CH_SECONDS: [f64; 2] = [0.025, 0.210];
const OH_SECONDS: f64 = 0.520;

/// How far the TUNING knobs move a read clock: an octave of travel, so a
/// semitone under six either side of the recording's own rate.
const TUNE_OCTAVES: f64 = 1.0;

/// The read clock as a multiple of the recording's own, from a TUNING knob.
fn tune_rate(knob: f64) -> f64 {
    ((knob.clamp(0.0, 1.0) - 0.5) * TUNE_OCTAVES).exp2()
}

/// What this machine's decay knobs render, in seconds to −20 dB.
///
/// Only one of them is a knob. The rest are the decays written into the data,
/// which do not move — except with the TUNING knobs, which shorten the sounds
/// they raise; what is printed here is the untuned length.
pub(crate) fn decay_seconds(index: usize, knob: f64) -> Option<f64> {
    Some(match index {
        P_BD_DECAY => 0.240,
        P_LT_DECAY => 0.360,
        P_MT_DECAY => 0.300,
        P_HT_DECAY => 0.255,
        // The crash's envelope runs 1.7 s in the data; what is heard is 1.54,
        // because the stick on the front of it is the loudest part and the
        // −20 dB point is measured from there. The rendered figure is the one
        // printed.
        P_CY_DECAY => 1.540,
        P_OH_DECAY => OH_SECONDS,
        P_CH_DECAY => geometric(CH_SECONDS[0], CH_SECONDS[1], knob),
        _ => return None,
    })
}

/// The fifteen recordings, with the hi-hat counted twice because the two hats
/// are one recording with two envelope generators, as they are on a 707.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoiceLinn {
    Bass,
    Snare,
    Sidestick,
    ClosedHat,
    OpenHat,
    Crash,
    Ride,
    LowTom,
    MidTom,
    HighTom,
    Cabasa,
    Tambourine,
    LowConga,
    HiConga,
    Cowbell,
    Clap,
}

impl VoiceLinn {
    /// The panel strip this sound is played from.
    pub(crate) fn strip(self) -> Instrument {
        match self {
            Self::Bass => Instrument::Bd,
            Self::Snare => Instrument::Sd,
            Self::Sidestick => Instrument::Rim,
            Self::ClosedHat => Instrument::ClosedHat,
            Self::OpenHat => Instrument::OpenHat,
            Self::Crash => Instrument::Cymbal,
            Self::Ride => Instrument::Ride,
            // The congas have their own tuning knobs on the instrument, and on
            // this panel that is the tuning knob of the tom strip they sit at.
            Self::LowTom | Self::LowConga => Instrument::LowTom,
            Self::MidTom | Self::HiConga => Instrument::MidTom,
            Self::HighTom => Instrument::HighTom,
            Self::Cowbell => Instrument::Cowbell,
            Self::Cabasa | Self::Tambourine | Self::Clap => Instrument::Clap,
        }
    }

    /// The recording's own clock.
    fn rate(self) -> f64 {
        match self {
            Self::ClosedHat
            | Self::OpenHat
            | Self::Crash
            | Self::Ride
            | Self::Cabasa
            | Self::Tambourine
            | Self::Cowbell => PCM_LINN_METAL_RATE,
            _ => PCM_LINN_DRUM_RATE,
        }
    }

    /// Whether the TUNING section reaches this sound. Seven of the fifteen:
    /// snare, sidestick, three toms, two congas.
    fn tuned(self) -> bool {
        matches!(
            self,
            Self::Snare
                | Self::Sidestick
                | Self::LowTom
                | Self::MidTom
                | Self::HighTom
                | Self::LowConga
                | Self::HiConga
        )
    }

    /// The time constant of the analog envelope after the converter, or `None`
    /// for the thirteen sounds whose decay is in the data.
    fn post_tau(self, c: &Controls) -> Option<f64> {
        Some(match self {
            Self::ClosedHat => geometric(CH_SECONDS[0], CH_SECONDS[1], c.decay) / DECAY_REFERENCE,
            Self::OpenHat => OH_SECONDS / DECAY_REFERENCE,
            _ => return None,
        })
    }

    /// A tag to index this sound's ROM by, so no two read the same words.
    fn tag(self) -> u64 {
        self as u64 + 1
    }
}

/// Which of the fifteen a note plays.
///
/// The folds are onto the nearest recording the machine has, never onto
/// another machine's circuit — the same rule the 606 and the 707 follow. A
/// LinnDrum has no maracas, no bongo, no timbale, no agogo, no guiro and no
/// whistle: shakers go to the cabasa, rattles and blown sounds to the
/// tambourine, pitched bells to the cowbell, and the hand drums sort onto the
/// two congas by pitch.
pub(crate) fn voice_linn(sound: DrumSound) -> VoiceLinn {
    use DrumSound as S;
    match sound {
        S::Kick | S::SubKick(_) => VoiceLinn::Bass,
        S::Snare | S::SnareAlt => VoiceLinn::Snare,
        S::Rimshot | S::Clave => VoiceLinn::Sidestick,
        S::LowTom => VoiceLinn::LowTom,
        S::MidTom => VoiceLinn::MidTom,
        S::HighTom => VoiceLinn::HighTom,
        S::Conga(f) | S::Bongo(f) | S::Timbale(f) => {
            if f < 260.0 { VoiceLinn::LowConga } else { VoiceLinn::HiConga }
        }
        S::ClosedHat | S::PedalHat => VoiceLinn::ClosedHat,
        S::OpenHat => VoiceLinn::OpenHat,
        S::Crash | S::Splash | S::Cymbal => VoiceLinn::Crash,
        S::Ride | S::RideBell => VoiceLinn::Ride,
        S::Cowbell | S::Agogo(_) => VoiceLinn::Cowbell,
        S::Clap => VoiceLinn::Clap,
        S::Cabasa | S::Maracas => VoiceLinn::Cabasa,
        S::Tambourine | S::Vibraslap | S::Guiro(_) | S::Whistle(_) | S::FxNoise(_) => {
            VoiceLinn::Tambourine
        }
    }
}

impl DrumVoice {
    // LinnDrum synthesis — a companding converter and what is on each side
    // ══════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_linn(&mut self, sr: f64, c: &Controls) -> f64 {
        let voice = voice_linn(self.sound);
        let base = voice.rate();
        // TUNING is the read clock. Everything in the recording moves with it,
        // its length included, because that is what reading a recording faster
        // does — see the module docs.
        let clock = if voice.tuned() { base * tune_rate(c.tune) } else { base };

        // Every word, not one: the TUNING knob can put the read clock above
        // the host's sample rate, and words that land inside one output sample
        // are averaged rather than dropped — see [`DrumVoice::convert_words`].
        let words = self.convert_words(sr, clock);
        if words > 0 {
            let mut sum = 0.0;
            for _ in 0..words {
                self.dac_address = self.dac_address.wrapping_add(1);
                sum += compand(self.rom_linn(voice));
            }
            self.dac_hold = sum / f64::from(words);
        }

        // The output filter after the hold. Three poles, as on the 707: two is
        // not enough behind a zero-order hold, and the first image sits at the
        // clock minus the highest thing in the data.
        let recon = base * 0.42;
        let smoothed = self.svf3.lowpass(self.dac_hold, recon, 0.707, sr);
        let filtered = self.lp1.tick_lp(smoothed, recon, sr);

        let out = match voice.post_tau(c) {
            // The hi-hat's envelope is a capacitor the trigger charges, so the
            // accent bus lengthens it as it does on the analog machines.
            Some(tau) => {
                let env = (-self.time / (tau * self.accent_stretch())).exp();
                if env < 0.001 {
                    self.active = false;
                    return 0.0;
                }
                filtered * env
            }
            // Decay in the data, so accent is level and only level. On the
            // instrument that is literally true: the sounds with dynamics are
            // stored two or three times at two or three levels.
            None => filtered,
        };

        if c.drive > 0.01 && matches!(voice, VoiceLinn::Bass) {
            drive_stage(out, c.drive * 2.0)
        } else {
            out
        }
    }

    /// One word out of ROM.
    ///
    /// Called at the read clock, so every phase and filter in here is advanced
    /// at the recording's own rate: what the ROM holds is fixed, and the clock
    /// is what makes it higher or lower.
    fn rom_linn(&mut self, voice: VoiceLinn) -> f64 {
        let rate = voice.rate();
        // Position in the recording rather than wall-clock seconds, so a
        // tuned-up sound reaches its end sooner.
        let t = self.dac_address as f64 / rate;
        let tag = voice.tag();
        match voice {
            // ── Bass drum: deep, round, damped. A LinnDrum kick is a felt
            // beater on a muffled 22", and there is almost no ring in it.
            VoiceLinn::Bass => {
                let f = 48.0 + 38.0 * (-t / 0.020).exp();
                advance_phase(&mut self.phase1, f, rate);
                let beater = self.svf1.bandpass(self.rom_noise(tag), 2200.0, 1.2, rate)
                    * (-t / 0.0045).exp();
                let env = (-t / (0.240 / DECAY_REFERENCE)).exp();
                (osc_sine(self.phase1) * 1.0 + beater * 0.40) * env
            }
            // ── Snare: the sound the machine is known by. A tuned shell with
            // a wide band of wire over it and a short plate of room behind
            // that, which is what separates it from the 707's dead one.
            VoiceLinn::Snare => {
                advance_phase(&mut self.phase1, 188.0, rate);
                advance_phase(&mut self.phase2, 297.0, rate);
                let head = (osc_sine(self.phase1) * 0.62 + osc_sine(self.phase2) * 0.38)
                    * (-t / (0.095 / DECAY_REFERENCE)).exp();
                let raw = self.rom_noise(tag);
                let wires = self.hp1.tick_hp(raw, 1650.0, rate) * (-t / (0.230 / DECAY_REFERENCE)).exp();
                let room = self.svf1.bandpass(raw, 900.0, 0.7, rate)
                    * (-t / (0.330 / DECAY_REFERENCE)).exp()
                    * 0.22;
                head * 0.55 + wires * 0.85 + room
            }
            // ── Sidestick: rim and stick, gone in thirty milliseconds.
            VoiceLinn::Sidestick => {
                advance_phase(&mut self.phase1, 745.0, rate);
                advance_phase(&mut self.phase2, 2280.0, rate);
                let stick =
                    self.svf1.bandpass(self.rom_noise(tag), 4200.0, 1.5, rate) * (-t / 0.0012).exp();
                let env = (-t / (0.032 / DECAY_REFERENCE)).exp();
                (osc_sine(self.phase1) * 0.40 + osc_sine(self.phase2) * 0.30 + stick * 0.85) * env
            }
            // ── Hi-hat: one recording for both. Non-decaying in the data, so
            // what is heard is the envelope generator after the converter and
            // the decay knob that sets it.
            VoiceLinn::ClosedHat | VoiceLinn::OpenHat => {
                let n = self.rom_noise(VoiceLinn::ClosedHat.tag());
                let body = self.svf1.bandpass(n, 6800.0, 0.75, rate);
                let edge = self.svf2.bandpass(n, 10_800.0, 1.2, rate);
                (body * 0.78 + edge * 0.60) * 0.82
            }
            // ── Crash: a wide wash with the stick on the front of it.
            VoiceLinn::Crash => {
                let n = self.rom_noise(tag);
                let wash = self.svf1.bandpass(n, 4600.0, 0.45, rate);
                let air = self.svf2.bandpass(n, 9200.0, 0.65, rate);
                let env = (-t / (1.700 / DECAY_REFERENCE)).exp();
                (wash * 0.95 + air * 0.62) * (0.82 + 0.18 * (-t / 0.012).exp()) * env * 0.85
            }
            // ── Ride: a stick on a bell, which is a pair of partials over a
            // much quieter wash than the crash's.
            VoiceLinn::Ride => {
                let n = self.rom_noise(tag);
                advance_phase(&mut self.phase1, 1240.0, rate);
                advance_phase(&mut self.phase2, 2810.0, rate);
                let ping = (osc_sine(self.phase1) * 0.55 + osc_sine(self.phase2) * 0.32)
                    * (0.30 + 0.70 * (-t / 0.040).exp());
                let wash = self.svf1.bandpass(n, 5600.0, 0.6, rate);
                let env = (-t / (1.150 / DECAY_REFERENCE)).exp();
                (ping * 0.72 + wash * 0.55) * env * 0.9
            }
            // ── Toms: recorded drums with the head, the shell under it and a
            // short drop in pitch as the head settles.
            VoiceLinn::LowTom => self.rom_linn_tom(tag, rate, 93.0, 0.360),
            VoiceLinn::MidTom => self.rom_linn_tom(tag, rate, 132.0, 0.300),
            VoiceLinn::HighTom => self.rom_linn_tom(tag, rate, 187.0, 0.255),
            // ── Congas: hand on skin, so no stick and a tighter head.
            VoiceLinn::LowConga => self.rom_linn_conga(tag, rate, 198.0, 0.290),
            VoiceLinn::HiConga => self.rom_linn_conga(tag, rate, 328.0, 0.215),
            // ── Cabasa: beads on steel, a burst of bright noise with the
            // rattle in it and nothing behind it.
            VoiceLinn::Cabasa => {
                let n = self.rom_noise(tag);
                let beads = self.hp1.tick_hp(n, 5200.0, rate);
                let rattle = 0.55 + 0.45 * (t * 320.0 * std::f64::consts::TAU).sin().abs();
                beads * rattle * (-t / (0.095 / DECAY_REFERENCE)).exp() * 1.05
            }
            // ── Tambourine: jingles, so a narrower and higher band than the
            // cabasa and a longer tail on it.
            VoiceLinn::Tambourine => {
                let n = self.rom_noise(tag);
                let jingle = self.svf1.bandpass(n, 9100.0, 1.1, rate);
                let rattle = 0.60 + 0.40 * (t * 210.0 * std::f64::consts::TAU).sin().abs();
                jingle * rattle * (-t / (0.200 / DECAY_REFERENCE)).exp() * 1.15
            }
            // ── Cowbell: struck metal, two partials and a clang under them.
            VoiceLinn::Cowbell => {
                advance_phase(&mut self.phase1, 562.0, rate);
                advance_phase(&mut self.phase2, 843.0, rate);
                let raw = osc_square(self.phase1) * 0.5 + osc_square(self.phase2) * 0.5;
                let body = self.svf1.bandpass(raw, 1050.0, 1.2, rate);
                let env = 0.45 * (-t / 0.009).exp() + 0.55 * (-t / (0.165 / DECAY_REFERENCE)).exp();
                body * env * 1.2
            }
            // ── Hand claps: a recording of several people, so the bursts are
            // in the data and land in the same places on every hit — and there
            // is a room behind them, which the 707's does not have.
            VoiceLinn::Clap => {
                let n = self.svf1.bandpass(self.rom_noise(tag), 1250.0, 1.5, rate);
                let bursts = if t < 0.040 {
                    let within = t % 0.0092;
                    (-within / 0.0030).exp()
                } else {
                    0.0
                };
                let room = (-t / (0.200 / DECAY_REFERENCE)).exp() * 0.42;
                n * (bursts + room)
            }
        }
    }

    /// One tom: head, shell and the pitch drop of a struck skin.
    fn rom_linn_tom(&mut self, tag: u64, rate: f64, freq: f64, seconds: f64) -> f64 {
        let t = self.dac_address as f64 / rate;
        let f = freq * (1.0 + 0.22 * (-t / 0.016).exp());
        advance_phase(&mut self.phase1, f, rate);
        advance_phase(&mut self.phase2, f * 2.31, rate);
        let shell = self.hp1.tick_hp(self.rom_noise(tag), 1600.0, rate) * (-t / 0.006).exp();
        let env = (-t / (seconds / DECAY_REFERENCE)).exp();
        (osc_sine(self.phase1) * 0.95 + osc_sine(self.phase2) * 0.12 + shell * 0.35) * env
    }

    /// One conga: the same drum without a stick on it, so the front is a slap
    /// rather than a crack and the head is tighter.
    fn rom_linn_conga(&mut self, tag: u64, rate: f64, freq: f64, seconds: f64) -> f64 {
        let t = self.dac_address as f64 / rate;
        let f = freq * (1.0 + 0.14 * (-t / 0.010).exp());
        advance_phase(&mut self.phase1, f, rate);
        let slap = self.svf1.bandpass(self.rom_noise(tag), 2600.0, 1.0, rate) * (-t / 0.0035).exp();
        let env = (-t / (seconds / DECAY_REFERENCE)).exp();
        (osc_sine(self.phase1) * 0.92 + slap * 0.42) * env
    }
}
