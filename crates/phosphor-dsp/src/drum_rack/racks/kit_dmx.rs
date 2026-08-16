//! Oberheim DMX kit synthesis.
//!
//! 1981, eight voice cards, one voice per card, and the machine most of early
//! hip-hop was built on. It shares a converter family with the LinnDrum — eight
//! bits companded through a µ-255 DAC — and almost nothing else: where a
//! LinnDrum is a close-miked studio kit with a little room on it, a DMX is
//! hard, dry and short.
//!
//! **These are not Oberheim's samples.** The ROM is a set of recordings and
//! they are not ours to ship; what is modelled is the machine around them, the
//! same way the 707 and the LinnDrum are done.
//!
//! # Twenty-four sounds out of eleven recordings
//!
//! This is the machine's defining trick and it is worth stating exactly,
//! because it is usually described as "tuning". The owner's manual lists eight
//! buttons with three variations each:
//!
//! | card   | its three                                   | recordings |
//! |--------|---------------------------------------------|------------|
//! | BASS   | three volume levels                         | 1 |
//! | SNARE  | three volume levels                         | 1 |
//! | HIHAT  | closed, accented closed, open               | 1 |
//! | TOM 1  | three pitches                               | 1 |
//! | TOM 2  | three pitches, lower than TOM 1              | 1 |
//! | CYMBAL | ride, accented ride, crash                  | 2 |
//! | PERC 1 | tambourine, accented tambourine, rimshot    | 2 |
//! | PERC 2 | shaker, accented shaker, handclap           | 2 |
//!
//! Eleven recordings, twenty-four sounds. Only the toms' variations are pitch;
//! the rest are level or envelope length. What *is* a pitch change is a change
//! of read clock, so a DMX tom that is a fourth higher is also a fourth
//! shorter — see [`VoiceDmx::rate`], where the five drum pitches this rack
//! reaches come out of the two tom recordings.
//!
//! # The panel
//!
//! The DMX's front panel is faders and nothing else. Its tuning is a trimpot
//! on the top rear of each voice card, half an octave up or down, with a CV
//! input wired in parallel with it — a control of the machine, just behind the
//! lid, so it is live here. One difference worth stating: the pot is per
//! *card*, so on the instrument tuning TOM 1 moves all three of its pitches at
//! once. This panel gives a TUNE knob per strip instead, which is finer than
//! the original rather than coarser, and is the same trade the level faders
//! make.
//!
//! There is no cowbell card in a factory DMX, so cowbell parts are played on
//! the rimshot and answer that fader — as they do on a 909, and for the same
//! reason.

use super::super::*;

/// Half an octave up or down, which is what the card pot gives.
fn tune_rate(knob: f64) -> f64 {
    (knob.clamp(0.0, 1.0) - 0.5).exp2()
}

/// The two hi-hat envelope generators after the converter.
const CH_SECONDS: f64 = 0.052;
const OH_SECONDS: f64 = 0.330;

/// What this machine's voices ring for, in seconds to −20 dB.
///
/// A DMX has no decay control at all — its panel is faders — so these are the
/// lengths written into the data, and the two hi-hat envelopes. The TUNE knobs
/// shorten what they raise; the untuned length is what is printed.
pub(crate) fn decay_seconds(index: usize) -> Option<f64> {
    Some(match index {
        P_BD_DECAY => 0.165,
        P_LT_DECAY => 0.290,
        P_MT_DECAY => 0.230,
        P_HT_DECAY => 0.185,
        P_CY_DECAY => 1.400,
        P_OH_DECAY => OH_SECONDS,
        P_CH_DECAY => CH_SECONDS,
        _ => return None,
    })
}

/// The eleven recordings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SampleDmx {
    Bass,
    Snare,
    Hat,
    Tom1,
    Tom2,
    Ride,
    Crash,
    Tambourine,
    Rimshot,
    Shaker,
    Clap,
}

/// The sounds this rack reaches, which is fifteen of the twenty-four: the
/// nine the machine makes by level or accent alone are one sound here, because
/// the accent bus is already how this rack plays a drum harder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoiceDmx {
    Bass,
    Snare,
    ClosedHat,
    OpenHat,
    Crash,
    Ride,
    HighTom,
    MidTom,
    LowTom,
    HiConga,
    LowConga,
    Rimshot,
    Tambourine,
    Shaker,
    Clap,
}

impl VoiceDmx {
    /// Which of the eleven this sound reads.
    pub(crate) fn sample(self) -> SampleDmx {
        match self {
            Self::Bass => SampleDmx::Bass,
            Self::Snare => SampleDmx::Snare,
            Self::ClosedHat | Self::OpenHat => SampleDmx::Hat,
            Self::Crash => SampleDmx::Crash,
            Self::Ride => SampleDmx::Ride,
            // The five drum pitches out of two recordings, which is the
            // machine's whole trick.
            Self::HiConga | Self::HighTom | Self::MidTom => SampleDmx::Tom1,
            Self::LowTom | Self::LowConga => SampleDmx::Tom2,
            Self::Rimshot => SampleDmx::Rimshot,
            Self::Tambourine => SampleDmx::Tambourine,
            Self::Shaker => SampleDmx::Shaker,
            Self::Clap => SampleDmx::Clap,
        }
    }

    /// The read clock as a multiple of the recording's own, before the card
    /// pot. This is where the extra pitches come from, and because it is a
    /// clock it takes the length with it.
    pub(crate) fn rate(self) -> f64 {
        match self {
            // TOM 1's three: a major third up, its own pitch, a major third
            // down. TOM 2's two: its own pitch and a major third down.
            Self::HiConga => 1.260,
            Self::HighTom => 1.000,
            Self::MidTom => 0.794,
            Self::LowTom => 1.000,
            Self::LowConga => 0.794,
            _ => 1.000,
        }
    }

    /// The panel strip this sound is played from.
    pub(crate) fn strip(self) -> Instrument {
        match self {
            Self::Bass => Instrument::Bd,
            Self::Snare => Instrument::Sd,
            Self::ClosedHat => Instrument::ClosedHat,
            Self::OpenHat => Instrument::OpenHat,
            Self::Crash => Instrument::Cymbal,
            Self::Ride => Instrument::Ride,
            Self::HighTom | Self::HiConga => Instrument::HighTom,
            Self::MidTom => Instrument::MidTom,
            Self::LowTom | Self::LowConga => Instrument::LowTom,
            // No cowbell card, so the rimshot carries those parts too.
            Self::Rimshot => Instrument::Rim,
            Self::Tambourine | Self::Shaker | Self::Clap => Instrument::Clap,
        }
    }

    /// Whether the card's tuning pot reaches this sound. The drum cards have
    /// one; the cymbal and percussion cards are left where they were cut.
    fn tuned(self) -> bool {
        matches!(
            self,
            Self::Bass
                | Self::Snare
                | Self::HighTom
                | Self::MidTom
                | Self::LowTom
                | Self::HiConga
                | Self::LowConga
        )
    }

    /// The envelope generator after the converter, for the two sounds that
    /// have one. Everything else decays inside the data.
    fn post_tau(self) -> Option<f64> {
        Some(match self {
            Self::ClosedHat => CH_SECONDS / DECAY_REFERENCE,
            Self::OpenHat => OH_SECONDS / DECAY_REFERENCE,
            _ => return None,
        })
    }
}

/// Which sound a note plays.
///
/// Folds are onto the nearest card the machine has. A factory DMX has no
/// cowbell, no conga and no maracas: bells go to the rimshot, hand drums to
/// the two tom recordings at their other pitches, and everything shaken to the
/// shaker or the tambourine.
pub(crate) fn voice_dmx(sound: DrumSound) -> VoiceDmx {
    use DrumSound as S;
    match sound {
        S::Kick | S::SubKick(_) => VoiceDmx::Bass,
        S::Snare | S::SnareAlt => VoiceDmx::Snare,
        S::Rimshot | S::Clave | S::Cowbell | S::Agogo(_) => VoiceDmx::Rimshot,
        S::LowTom => VoiceDmx::LowTom,
        S::MidTom => VoiceDmx::MidTom,
        S::HighTom => VoiceDmx::HighTom,
        S::Conga(f) | S::Bongo(f) | S::Timbale(f) => {
            if f < 260.0 {
                VoiceDmx::LowConga
            } else if f < 360.0 {
                VoiceDmx::LowTom
            } else {
                VoiceDmx::HiConga
            }
        }
        S::ClosedHat | S::PedalHat => VoiceDmx::ClosedHat,
        S::OpenHat => VoiceDmx::OpenHat,
        S::Crash | S::Splash | S::Cymbal => VoiceDmx::Crash,
        S::Ride | S::RideBell => VoiceDmx::Ride,
        S::Clap => VoiceDmx::Clap,
        S::Maracas | S::Cabasa => VoiceDmx::Shaker,
        S::Tambourine | S::Vibraslap | S::Guiro(_) | S::Whistle(_) | S::FxNoise(_) => {
            VoiceDmx::Tambourine
        }
    }
}

impl DrumVoice {
    // DMX synthesis
    // ══════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_dmx(&mut self, sr: f64, c: &Controls) -> f64 {
        let voice = voice_dmx(self.sound);
        let clock = PCM_DMX_RATE
            * voice.rate()
            * if voice.tuned() { tune_rate(c.tune) } else { 1.0 };

        // Every word, not one — see [`DrumVoice::convert_words`]. A DMX card's
        // pitch trimmer and its three tom pitches multiply, so the read clock
        // reaches 28 kHz times 1.78, and a host at 22.05 kHz would drop words
        // rather than play them.
        let words = self.convert_words(sr, clock);
        if words > 0 {
            let mut sum = 0.0;
            for _ in 0..words {
                self.dac_address = self.dac_address.wrapping_add(1);
                sum += compand(self.rom_dmx(voice.sample()));
            }
            self.dac_hold = sum / f64::from(words);
        }

        const RECON: f64 = PCM_DMX_RATE * 0.42;
        let smoothed = self.svf3.lowpass(self.dac_hold, RECON, 0.707, sr);
        let filtered = self.lp1.tick_lp(smoothed, RECON, sr);

        let out = match voice.post_tau() {
            Some(tau) => {
                let env = (-self.time / (tau * self.accent_stretch())).exp();
                if env < 0.001 {
                    self.active = false;
                    return 0.0;
                }
                filtered * env
            }
            None => filtered,
        };

        if c.drive > 0.01 && matches!(voice, VoiceDmx::Bass) {
            soft_clip(out, c.drive * 2.0)
        } else {
            out
        }
    }

    /// One word out of ROM. Everything here runs at the recording's own clock,
    /// which is what makes reading it faster read it higher.
    fn rom_dmx(&mut self, sample: SampleDmx) -> f64 {
        const RATE: f64 = PCM_DMX_RATE;
        let t = self.dac_address as f64 / RATE;
        let tag = sample as u64 + 1;
        match sample {
            // ── Bass drum: short, hard, and mostly beater. A DMX kick stops
            // where a LinnDrum's is still going.
            SampleDmx::Bass => {
                let f = 57.0 + 52.0 * (-t / 0.011).exp();
                advance_phase(&mut self.phase1, f, RATE);
                let beater =
                    self.svf1.bandpass(self.rom_noise(tag), 3100.0, 1.0, RATE) * (-t / 0.0028).exp();
                let env = (-t / (0.165 / DECAY_REFERENCE)).exp();
                (osc_sine(self.phase1) * 0.92 + beater * 0.62) * env
            }
            // ── Snare: bright, tight and dead. No room on it at all, which is
            // most of the difference from the LinnDrum's.
            SampleDmx::Snare => {
                advance_phase(&mut self.phase1, 224.0, RATE);
                advance_phase(&mut self.phase2, 358.0, RATE);
                let head = (osc_sine(self.phase1) * 0.6 + osc_sine(self.phase2) * 0.4)
                    * (-t / (0.060 / DECAY_REFERENCE)).exp();
                let raw = self.rom_noise(tag);
                let wires = self.hp1.tick_hp(raw, 2300.0, RATE) * (-t / (0.160 / DECAY_REFERENCE)).exp();
                let crack = self.svf1.bandpass(raw, 4800.0, 1.1, RATE) * (-t / 0.0035).exp();
                head * 0.50 + wires * 0.92 + crack * 0.35
            }
            // ── Hi-hat: one recording, dark and dry, non-decaying in the data
            // so the envelope after the converter is what is heard.
            SampleDmx::Hat => {
                let n = self.rom_noise(tag);
                let body = self.svf1.bandpass(n, 5900.0, 0.8, RATE);
                let edge = self.svf2.bandpass(n, 9600.0, 1.3, RATE);
                (body * 0.80 + edge * 0.45) * 0.82
            }
            // ── The two tom recordings. TOM 2 is the lower drum; both are
            // read at other clocks to make the machine's other pitches.
            SampleDmx::Tom1 => self.rom_dmx_tom(tag, 168.0, 0.185),
            SampleDmx::Tom2 => self.rom_dmx_tom(tag, 106.0, 0.290),
            // ── Ride: the biggest sample in the machine, and the one with a
            // real stick on it.
            SampleDmx::Ride => {
                let n = self.rom_noise(tag);
                advance_phase(&mut self.phase1, 1320.0, RATE);
                advance_phase(&mut self.phase2, 2960.0, RATE);
                let ping = (osc_sine(self.phase1) * 0.58 + osc_sine(self.phase2) * 0.30)
                    * (0.28 + 0.72 * (-t / 0.035).exp());
                let wash = self.svf1.bandpass(n, 5200.0, 0.65, RATE);
                let env = (-t / (0.950 / DECAY_REFERENCE)).exp();
                (ping * 0.75 + wash * 0.48) * env * 0.9
            }
            // ── Crash: wide, and shorter than a LinnDrum's.
            SampleDmx::Crash => {
                let n = self.rom_noise(tag);
                let wash = self.svf1.bandpass(n, 4100.0, 0.45, RATE);
                let air = self.svf2.bandpass(n, 8600.0, 0.7, RATE);
                let env = (-t / (1.400 / DECAY_REFERENCE)).exp();
                (wash * 0.95 + air * 0.55) * (0.85 + 0.15 * (-t / 0.010).exp()) * env * 0.85
            }
            // ── Tambourine: PERC 1's first two sounds.
            SampleDmx::Tambourine => {
                let n = self.rom_noise(tag);
                let jingle = self.svf1.bandpass(n, 8400.0, 1.2, RATE);
                let rattle = 0.62 + 0.38 * (t * 240.0 * std::f64::consts::TAU).sin().abs();
                jingle * rattle * (-t / (0.140 / DECAY_REFERENCE)).exp() * 1.15
            }
            // ── Rimshot: PERC 1's third, and the only pitched click on the
            // machine, so the cowbell parts land here as well.
            SampleDmx::Rimshot => {
                advance_phase(&mut self.phase1, 810.0, RATE);
                advance_phase(&mut self.phase2, 2450.0, RATE);
                let crack =
                    self.svf1.bandpass(self.rom_noise(tag), 3900.0, 1.6, RATE) * (-t / 0.0012).exp();
                let env = (-t / (0.036 / DECAY_REFERENCE)).exp();
                (osc_sine(self.phase1) * 0.45 + osc_sine(self.phase2) * 0.30 + crack * 0.9) * env
            }
            // ── Shaker: PERC 2's first two.
            SampleDmx::Shaker => {
                let n = self.rom_noise(tag);
                let beads = self.hp1.tick_hp(n, 6400.0, RATE);
                let rattle = 0.5 + 0.5 * (t * 380.0 * std::f64::consts::TAU).sin().abs();
                beads * rattle * (-t / (0.070 / DECAY_REFERENCE)).exp() * 1.05
            }
            // ── Handclap: PERC 2's third. Short and tight, three bursts and
            // very little behind them.
            SampleDmx::Clap => {
                let n = self.svf1.bandpass(self.rom_noise(tag), 1500.0, 1.7, RATE);
                let bursts = if t < 0.024 {
                    let within = t % 0.0078;
                    (-within / 0.0022).exp()
                } else {
                    0.0
                };
                let room = (-t / (0.070 / DECAY_REFERENCE)).exp() * 0.28;
                n * (bursts + room)
            }
        }
    }

    /// One tom recording: head, shell, and a drop in pitch as the skin
    /// settles. Dry — there is no room in a DMX.
    fn rom_dmx_tom(&mut self, tag: u64, freq: f64, seconds: f64) -> f64 {
        const RATE: f64 = PCM_DMX_RATE;
        let t = self.dac_address as f64 / RATE;
        let f = freq * (1.0 + 0.20 * (-t / 0.012).exp());
        advance_phase(&mut self.phase1, f, RATE);
        let shell = self.hp1.tick_hp(self.rom_noise(tag), 2000.0, RATE) * (-t / 0.004).exp();
        let env = (-t / (seconds / DECAY_REFERENCE)).exp();
        (osc_sine(self.phase1) * 0.95 + shell * 0.38) * env
    }
}
