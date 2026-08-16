//! 727 kit synthesis.
//!
//! The TR-727 is the TR-707's Latin sibling: the same case, the same panel,
//! the same board and the same converter, with a different mask in the ROM.
//! Everything [`kit_707`](super::kit_707) says about the machine around the
//! sample applies here unchanged — eight bits at 25 kHz, six for the longest
//! sound, held with only the output filter after it, and analog envelope
//! generators on the far side of the converter for the sounds that need one.
//!
//! **These are not Roland's samples**, for the same reason they are not in the
//! 707: the ROM is Roland's. What is modelled is the shape of the machine
//! around each recording.
//!
//! What is different is the sound set, and the difference is total. The
//! fifteen are hi and low bongo, mute hi conga, open hi conga, low conga, hi
//! and low timbale, hi and low agogô, cabasa, maracas, short and long whistle,
//! quijada and star chime. **There is no bass drum, no snare, no hi-hat and no
//! cymbal anywhere in the machine.** That is not a gap to be filled from
//! another kit — it is what a TR-727 is — so a kick is played on the low
//! conga, a snare on the low timbale, hi-hats on the maracas and the cabasa,
//! and every cymbal on the star chime, each at the fader of the voice it lands
//! on. See [`voice_727`].
//!
//! The panel is a fader per instrument and the accent bus, as the 707's is,
//! and the four faders with no voice behind them are dead.

use super::super::*;

/// The output filter after the hold — the same three poles at the same corner
/// as the 707, because it is the same board.
const RECON_HZ: f64 = 11_000.0;

/// The star chime's envelope generator, the one sound on the machine long
/// enough to need one after the converter.
const CHIME_SECONDS: f64 = 1.400;

/// What this machine's voices ring for, in seconds to −20 dB.
///
/// A TR-727 has no decay control, so these are fixed: the decay written into
/// the data, or for the star chime the analog envelope after the converter.
/// The bass-drum and hi-hat strips report nothing, because there is no such
/// voice in the machine.
pub(crate) fn decay_seconds(index: usize) -> Option<f64> {
    Some(match index {
        P_LT_DECAY => 0.300,
        P_MT_DECAY => 0.220,
        P_HT_DECAY => 0.090,
        // The chime's envelope generator runs 1.4 s and the cascade in front
        // of it puts the loudest bell 130 ms in, so what is heard is 1.53. The
        // rendered figure is the one printed.
        P_CY_DECAY => 1.530,
        _ => return None,
    })
}

/// The fifteen, in front-panel order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Voice727 {
    HiBongo,
    LowBongo,
    MuteHiConga,
    OpenHiConga,
    LowConga,
    HiTimbale,
    LowTimbale,
    HiAgogo,
    LowAgogo,
    Cabasa,
    Maracas,
    ShortWhistle,
    LongWhistle,
    Quijada,
    StarChime,
}

impl Voice727 {
    /// The panel strip this sound is played from.
    ///
    /// The hand drums sort onto the three tom strips by pitch, the agogôs onto
    /// the cowbell strip because that is where a bell goes on this panel, the
    /// shakers and blown sounds onto the strip the 808 shares between its
    /// maracas and its hand clap, and the star chime onto the cymbal.
    pub(crate) fn strip(self) -> Instrument {
        match self {
            Self::LowConga => Instrument::LowTom,
            Self::OpenHiConga | Self::MuteHiConga | Self::LowBongo | Self::LowTimbale => {
                Instrument::MidTom
            }
            Self::HiBongo | Self::HiTimbale => Instrument::HighTom,
            Self::HiAgogo | Self::LowAgogo => Instrument::Cowbell,
            Self::Cabasa
            | Self::Maracas
            | Self::Quijada
            | Self::ShortWhistle
            | Self::LongWhistle => Instrument::Clap,
            Self::StarChime => Instrument::Cymbal,
        }
    }

    /// The word length this sound is stored at. The star chime is the longest
    /// thing in the ROM and takes the six-bit treatment the 707 gives its
    /// crash and its ride, for the same reason: something had to give, and a
    /// sound with an envelope generator after the converter can afford it.
    fn bits(self) -> u32 {
        match self {
            Self::StarChime => PCM_707_CYMBAL_BITS,
            _ => PCM_707_BITS,
        }
    }

    /// The analog envelope after the converter, or `None` for the fourteen
    /// whose decay is in the data.
    fn post_tau(self) -> Option<f64> {
        match self {
            Self::StarChime => Some(CHIME_SECONDS / DECAY_REFERENCE),
            _ => None,
        }
    }

    /// A tag to index this sound's ROM by.
    fn tag(self) -> u64 {
        // Offset past the 707's tags: different mask, different words.
        self as u64 + 41
    }
}

/// Which of the fifteen a note plays.
///
/// **The decision this table is:** the four instrument families a TR-727 does
/// not have — bass drum, snare, hi-hat and cymbal — are played on the nearest
/// Latin voice rather than borrowed from the 707 next to it in this rack.
/// Borrowing would be easy and it would be a lie about the instrument: the
/// whole reason to have a 727 is that it does not have those sounds.
///
/// So a kick goes on the low conga, which is the deepest voice in the machine;
/// a snare on the low timbale and a rimshot on the high one, which are the two
/// voices with a rim crack in them; a closed hat on the maracas and an open
/// one on the cabasa, which are its short and long shakes; a clap, a guiro and
/// a vibraslap on the quijada, which is the rattle the vibraslap was invented
/// to imitate; and every cymbal on the star chime.
pub(crate) fn voice_727(sound: DrumSound) -> Voice727 {
    use DrumSound as S;
    match sound {
        S::Kick | S::SubKick(_) | S::LowTom => Voice727::LowConga,
        S::Snare | S::SnareAlt => Voice727::LowTimbale,
        S::Rimshot | S::Clave => Voice727::HiTimbale,
        S::MidTom => Voice727::OpenHiConga,
        S::HighTom => Voice727::HiBongo,
        // The note map's three congas are this machine's three congas, so they
        // sort by the frequencies it gives them rather than by the generic
        // tom boundaries.
        S::Conga(f) => {
            if f >= 340.0 {
                Voice727::MuteHiConga
            } else if f >= 260.0 {
                Voice727::OpenHiConga
            } else {
                Voice727::LowConga
            }
        }
        S::Bongo(f) => {
            if f >= 350.0 { Voice727::HiBongo } else { Voice727::LowBongo }
        }
        S::Timbale(f) => {
            if f >= 420.0 { Voice727::HiTimbale } else { Voice727::LowTimbale }
        }
        // The agogô is the only bell on the machine, so a cowbell part is
        // played on the higher of the two.
        S::Cowbell => Voice727::HiAgogo,
        S::Agogo(f) => {
            if f >= 750.0 { Voice727::HiAgogo } else { Voice727::LowAgogo }
        }
        S::ClosedHat | S::PedalHat => Voice727::Maracas,
        S::OpenHat | S::Tambourine | S::Cabasa => Voice727::Cabasa,
        S::Maracas => Voice727::Maracas,
        S::Crash | S::Splash | S::Cymbal | S::Ride | S::RideBell => Voice727::StarChime,
        S::Whistle(d) => {
            if d < 0.2 { Voice727::ShortWhistle } else { Voice727::LongWhistle }
        }
        S::Clap | S::Vibraslap | S::Guiro(_) | S::FxNoise(_) => Voice727::Quijada,
    }
}

impl DrumVoice {
    // 727 synthesis — the 707's converter with a Latin mask in it
    // ══════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_727(&mut self, sr: f64, c: &Controls) -> f64 {
        let voice = voice_727(self.sound);

        if self.convert(sr, PCM_727_RATE) {
            let word = self.rom_727(voice);
            self.dac_hold = Self::quantize(word, voice.bits());
        }

        let smoothed = self.svf3.lowpass(self.dac_hold, RECON_HZ, 0.707, sr);
        let filtered = self.lp1.tick_lp(smoothed, RECON_HZ, sr);

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

        // DRIVE is the rack's control rather than Roland's. On a machine with
        // no bass drum it goes across the deepest voice there is.
        if c.drive > 0.01 && matches!(voice, Voice727::LowConga) {
            soft_clip(out, c.drive * 2.0)
        } else {
            out
        }
    }

    /// One word out of ROM, at the recording's own clock.
    fn rom_727(&mut self, voice: Voice727) -> f64 {
        const RATE: f64 = PCM_727_RATE;
        let t = self.time;
        let tag = voice.tag();
        match voice {
            // ── Bongos: small, tight, hard-struck heads with the shell right
            // behind them.
            Voice727::HiBongo => self.rom_727_hand_drum(tag, 438.0, 1310.0, 0.090, 0.30),
            Voice727::LowBongo => self.rom_727_hand_drum(tag, 322.0, 962.0, 0.115, 0.28),
            // ── Congas: the mute is the same drum with a palm on it, so it is
            // the shortest sound in this group by a long way.
            Voice727::MuteHiConga => self.rom_727_hand_drum(tag, 378.0, 1130.0, 0.055, 0.34),
            Voice727::OpenHiConga => self.rom_727_hand_drum(tag, 302.0, 905.0, 0.220, 0.22),
            Voice727::LowConga => self.rom_727_hand_drum(tag, 188.0, 562.0, 0.300, 0.18),
            // ── Timbales: a metal shell, so a bright ring over the head and a
            // rim crack on the front of it.
            Voice727::HiTimbale => self.rom_727_timbale(tag, 494.0, 1480.0, 0.185),
            Voice727::LowTimbale => self.rom_727_timbale(tag, 348.0, 1044.0, 0.240),
            // ── Agogôs: two struck bells about a fourth apart, and the
            // machine's only pitched metal.
            Voice727::HiAgogo => self.rom_727_agogo(788.0, 0.155),
            Voice727::LowAgogo => self.rom_727_agogo(628.0, 0.180),
            // ── Cabasa: steel beads on a steel cylinder, so a long bright
            // shhh with the rattle in it.
            Voice727::Cabasa => {
                let n = self.rom_noise(tag);
                let beads = self.hp1.tick_hp(n, 5400.0, RATE);
                let rattle = 0.55 + 0.45 * (t * 290.0 * std::f64::consts::TAU).sin().abs();
                beads * rattle * (-t / (0.115 / DECAY_REFERENCE)).exp() * 1.1
            }
            // ── Maracas: seeds in a gourd, so shorter, drier and lower than
            // the cabasa, with one hard front edge.
            Voice727::Maracas => {
                let n = self.rom_noise(tag);
                let seeds = self.svf1.bandpass(n, 7200.0, 0.8, RATE);
                let front = 1.0 + 2.2 * (-t / 0.0022).exp();
                seeds * front * (-t / (0.055 / DECAY_REFERENCE)).exp() * 0.85
            }
            // ── Whistles: the same instrument twice, held for two lengths.
            Voice727::ShortWhistle => self.rom_727_whistle(tag, 0.085),
            Voice727::LongWhistle => self.rom_727_whistle(tag, 0.330),
            // ── Quijada: a donkey jawbone struck so the teeth rattle in their
            // sockets. A hard crack and then a rattle that slows as it dies —
            // this is the instrument a vibraslap imitates.
            Voice727::Quijada => {
                let n = self.rom_noise(tag);
                let crack = self.svf1.bandpass(n, 1900.0, 1.2, RATE) * (-t / 0.0035).exp() * 2.4;
                let teeth = self.svf2.bandpass(n, 3800.0, 2.2, RATE);
                // The rattle slows as the jaw settles.
                let rate_now = 95.0 - 55.0 * (1.0 - (-t / 0.180).exp());
                let rattle = (t * rate_now * std::f64::consts::TAU).sin().abs().powf(1.6);
                (crack + teeth * rattle * 1.5) * (-t / (0.300 / DECAY_REFERENCE)).exp() * 0.6
            }
            // ── Star chime: a handful of small bells set going one after the
            // other. The cascade is in the data; the length is the envelope
            // generator on the far side of the converter, which is why this is
            // the one sound on the machine that has one.
            Voice727::StarChime => {
                advance_phase(&mut self.phase1, 2840.0, RATE);
                advance_phase(&mut self.phase2, 3970.0, RATE);
                advance_phase(&mut self.phase3, 5310.0, RATE);
                let entry = |at: f64| if t > at { 1.0 - (-(t - at) / 0.004).exp() } else { 0.0 };
                let bells = osc_sine(self.phase1) * 0.42 * entry(0.000)
                    + osc_sine(self.phase2) * 0.34 * entry(0.055)
                    + osc_sine(self.phase3) * 0.26 * entry(0.130);
                let shimmer = self.svf1.bandpass(self.rom_noise(tag), 7400.0, 0.9, RATE) * 0.30;
                (bells + shimmer) * 0.95
            }
        }
    }

    /// A bongo or a conga: one head with the shell above it and a hand on the
    /// front rather than a stick, so the attack is a slap and not a crack.
    fn rom_727_hand_drum(
        &mut self,
        tag: u64,
        head: f64,
        shell: f64,
        seconds: f64,
        slap: f64,
    ) -> f64 {
        const RATE: f64 = PCM_727_RATE;
        let t = self.time;
        let f = head * (1.0 + 0.16 * (-t / 0.009).exp());
        advance_phase(&mut self.phase1, f, RATE);
        advance_phase(&mut self.phase2, shell, RATE);
        let palm = self.svf1.bandpass(self.rom_noise(tag), 2800.0, 1.0, RATE) * (-t / 0.0030).exp();
        let env = (-t / (seconds / DECAY_REFERENCE)).exp();
        (osc_sine(self.phase1) * 0.90
            + osc_sine(self.phase2) * 0.14 * (-t / 0.020).exp()
            + palm * slap)
            * env
    }

    /// A timbale: a metal shell, so the ring above the head is much stronger
    /// than a conga's and the stick on the rim is a crack rather than a slap.
    fn rom_727_timbale(&mut self, tag: u64, head: f64, ring: f64, seconds: f64) -> f64 {
        const RATE: f64 = PCM_727_RATE;
        let t = self.time;
        advance_phase(&mut self.phase1, head, RATE);
        advance_phase(&mut self.phase2, ring, RATE);
        let rim = self.svf1.bandpass(self.rom_noise(tag), 4400.0, 1.4, RATE) * (-t / 0.0018).exp();
        let env = (-t / (seconds / DECAY_REFERENCE)).exp();
        (osc_sine(self.phase1) * 0.72
            + osc_sine(self.phase2) * 0.30 * (-t / 0.045).exp()
            + rim * 0.85)
            * env
    }

    /// An agogô bell: struck metal, two partials, and no noise on it at all.
    fn rom_727_agogo(&mut self, hz: f64, seconds: f64) -> f64 {
        const RATE: f64 = PCM_727_RATE;
        let t = self.time;
        advance_phase(&mut self.phase1, hz, RATE);
        advance_phase(&mut self.phase2, hz * 1.504, RATE);
        let raw = osc_sine(self.phase1) * 0.62 + osc_sine(self.phase2) * 0.38;
        let env = 0.35 * (-t / 0.006).exp() + 0.65 * (-t / (seconds / DECAY_REFERENCE)).exp();
        raw * env
    }

    /// A samba whistle: two pipes a hair apart so they beat against each
    /// other, with the player's breath under them.
    fn rom_727_whistle(&mut self, tag: u64, seconds: f64) -> f64 {
        const RATE: f64 = PCM_727_RATE;
        let t = self.time;
        let vibrato = (t * 7.0 * std::f64::consts::TAU).sin() * 26.0;
        advance_phase(&mut self.phase1, 2455.0 + vibrato, RATE);
        advance_phase(&mut self.phase2, 2487.0 + vibrato, RATE);
        let breath = self.svf1.bandpass(self.rom_noise(tag), 3200.0, 0.9, RATE);
        let attack = (t / 0.004).min(1.0);
        // The recording stops when the player does, so the release is short
        // and in the data.
        let env = if t < seconds {
            attack
        } else {
            attack * (-(t - seconds) / 0.012).exp()
        };
        (osc_sine(self.phase1) * 0.44 + osc_sine(self.phase2) * 0.30 + breath * 0.22) * env
    }
}
