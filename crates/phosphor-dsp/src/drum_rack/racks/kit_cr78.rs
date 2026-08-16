//! CR-78 kit synthesis.
//!
//! The CompuRhythm CR-78, 1978, and the ancestor of everything else Roland in
//! this rack — four years before the 808 and built in an older idiom. Fully
//! analog, and three things about it are not true of any later machine here:
//!
//! * **Its snare has no oscillator in it.** The 808's snare is two bridged-T
//!   resonators with noise across them and the 606's is one; the CR-78's is
//!   white noise and nothing else, with its body shaped by a filter rather
//!   than rung by a circuit. That is most of why it reads as a brush rather
//!   than as a crack.
//! * **Its filters use inductors.** The metal section runs through an LC
//!   band-pass — 47 mH against 15 nF, which is 5.99 kHz by 1/(2π√LC), and the
//!   figure quoted for the machine is 5.59 kHz, so the printed inductor value
//!   is nominal. One band-pass for all of the metal, where the 808 splits its
//!   oscillators into two and high-passes each path hard. That is the whole of
//!   why a CR-78 hi-hat is soft and woody and an 808's is a hiss.
//! * **It has a METALLIC BEAT.** Three filtered square waves, on their own
//!   button, added to a pattern to give it a chime tick. There is nothing like
//!   it on any later Roland, and it is reachable here from the note map's
//!   effects range — see [`voice_cr78`].
//!
//! Fourteen voices: bass drum, snare, rim shot, hi-hat, cymbal, high and low
//! bongo, high and low conga, tambourine, maracas, claves, cowbell and guiro.
//! There is **one** cymbal and **one** hi-hat — no ride, no crash and no open
//! hat — so those parts are played on the cymbal, which is the long metal
//! voice the machine has.
//!
//! Its front panel is not a mixer. Each instrument has a CANCEL VOICE button
//! rather than a fader, and the only level control that shapes the sound is
//! one balance slider tilting the whole machine between the bass drum and the
//! high percussion. A fader is the finer-grained version of the button, so
//! that is what this panel gives; the balance slider is global and has no
//! strip here.
//!
//! Where a value could not be worked out of what is published, the comment
//! says it was chosen — the same rule [`kit_606`](super::kit_606) follows.

use super::super::*;

/// The six oscillators of the metal section.
///
/// Chosen, not derived: Roland built this section the way it built the 808's —
/// a bank of square waves summed into a band-pass — but the component values
/// are not in anything available here. What is set is the character the
/// machine is described by. They run below the 808's 205-800 Hz and are
/// spaced more closely, so the comb they make inside the band-pass is denser
/// and lower, which is a duller sound out of the same architecture.
pub(crate) const HAT_FREQS_CR78: [f64; 6] = [188.0, 264.5, 311.7, 402.9, 476.2, 621.5];

/// The LC band-pass every metal voice runs through.
///
/// The figure quoted for the machine is 5.59 kHz. The printed components — a
/// 47 mH inductor against 15 nF — give 5.99 kHz through 1/(2π√LC), so the
/// coil's nominal value is a few percent off its real one. The machine's own
/// figure is the one used.
const METAL_LC_HZ: f64 = 5594.0;
/// Its Q. An LC tank's is set by the coil's own resistance and is a *high* one
/// at audio — this is a tuned circuit, not the broad multiple-feedback section
/// the 808 uses. That is the other half of why this machine's metal is soft:
/// a narrow band at 6 kHz is a tick, where the 808's pair of wider bands at
/// 3.4 and 7.1 kHz high-passed together at 6 is a hiss.
///
/// It was written at 1.5 first, on the assumption that "soft" meant "broad",
/// and measured *twice as bright* above 8 kHz as the 808's cymbal — because a
/// low-Q band at 6 kHz has a skirt reaching well past 10.
const METAL_LC_Q: f64 = 4.0;
/// The gentle high-pass after it, which is nothing like the 808's 6 kHz
/// Sallen-Key on the hats: enough to keep the oscillators' fundamentals out
/// and no more.
const METAL_HP_HZ: f64 = 2400.0;

// ── Ring times ──
//
// The CR-78 has no decay control anywhere on it, so every one of these is
// fixed. None of them is published; they are chosen to the descriptions of the
// machine — softer and shorter than the 808 throughout, and markedly so on the
// metal, where the 808's cymbal knob alone runs to 1.2 s.

const BD_SECONDS: f64 = 0.200;
const SD_SECONDS: f64 = 0.150;
const RIM_SECONDS: f64 = 0.028;
const HAT_SECONDS: f64 = 0.055;
const CY_BODY_SECONDS: f64 = 0.900;
const CY_STRIKE_SECONDS: f64 = 0.220;
const METALLIC_SECONDS: f64 = 0.070;

/// What this machine's voices ring for, in seconds to −20 dB. Fixed, because
/// the instrument has no decay control. The open-hat and ride strips report
/// nothing: there is no such voice in the machine.
///
/// Rendered times rather than envelope constants — the low-passes on the kick
/// and the two-stage cymbal each move the number — so the panel prints the
/// drum and not the capacitor.
pub(crate) fn decay_seconds(index: usize) -> Option<f64> {
    Some(match index {
        P_BD_DECAY => 0.200,
        P_LT_DECAY => 0.232,
        P_MT_DECAY => 0.180,
        P_HT_DECAY => 0.065,
        P_CY_DECAY => 0.760,
        P_CH_DECAY => 0.060,
        _ => return None,
    })
}

/// The fourteen voices, plus the metallic beat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoiceCr78 {
    Bd,
    Sd,
    Rimshot,
    HiHat,
    Cymbal,
    HiBongo,
    LowBongo,
    HiConga,
    LowConga,
    Tambourine,
    Maracas,
    Claves,
    Cowbell,
    Guiro,
    Metallic,
}

impl VoiceCr78 {
    /// The panel strip this voice is played from.
    pub(crate) fn strip(self) -> Instrument {
        match self {
            Self::Bd => Instrument::Bd,
            Self::Sd => Instrument::Sd,
            Self::Rimshot | Self::Claves => Instrument::Rim,
            Self::HiHat => Instrument::ClosedHat,
            Self::Cymbal => Instrument::Cymbal,
            Self::LowConga => Instrument::LowTom,
            Self::HiConga | Self::LowBongo => Instrument::MidTom,
            Self::HiBongo => Instrument::HighTom,
            Self::Cowbell | Self::Metallic => Instrument::Cowbell,
            Self::Tambourine | Self::Maracas | Self::Guiro => Instrument::Clap,
        }
    }
}

/// Which of the voices a note is played on.
///
/// The rule is the 606's: a note the machine has no voice for is folded onto
/// the nearest voice it does have, at that voice's fader. The CR-78's gaps are
/// a ride, a crash, an open hi-hat, a hand clap and a set of toms — so every
/// long metal part goes to the one cymbal, a clap goes to the snare, which is
/// the machine's noise burst, and the toms are played on the congas and the
/// bongos, which is what a machine built in 1978 leaves you.
///
/// The METALLIC BEAT is on its own button on the instrument rather than in any
/// note map, so the effects range is where it is reachable from here.
pub(crate) fn voice_cr78(sound: DrumSound) -> VoiceCr78 {
    use DrumSound as S;
    match sound {
        S::Kick | S::SubKick(_) => VoiceCr78::Bd,
        // No clap circuit; the snare is the machine's noise burst.
        S::Snare | S::SnareAlt | S::Clap => VoiceCr78::Sd,
        S::Rimshot => VoiceCr78::Rimshot,
        S::Clave => VoiceCr78::Claves,
        S::LowTom => VoiceCr78::LowConga,
        S::MidTom => VoiceCr78::HiConga,
        S::HighTom => VoiceCr78::HiBongo,
        S::Conga(f) => {
            if f < 260.0 { VoiceCr78::LowConga } else { VoiceCr78::HiConga }
        }
        S::Bongo(f) => {
            if f < 350.0 { VoiceCr78::LowBongo } else { VoiceCr78::HiBongo }
        }
        // No timbale either; the bongos are the machine's bright hand drums.
        S::Timbale(f) => {
            if f < 420.0 { VoiceCr78::LowBongo } else { VoiceCr78::HiBongo }
        }
        S::Cowbell | S::Agogo(_) => VoiceCr78::Cowbell,
        S::ClosedHat | S::PedalHat => VoiceCr78::HiHat,
        // One hi-hat and one cymbal: an open hat, a crash and a ride are all
        // played on the cymbal.
        S::OpenHat | S::Crash | S::Splash | S::Cymbal | S::Ride | S::RideBell => VoiceCr78::Cymbal,
        S::Maracas | S::Cabasa => VoiceCr78::Maracas,
        S::Tambourine => VoiceCr78::Tambourine,
        // Everything scraped, rattled or blown is the guiro, which is the
        // machine's one long noise voice.
        S::Guiro(_) | S::Vibraslap | S::Whistle(_) => VoiceCr78::Guiro,
        S::FxNoise(_) => VoiceCr78::Metallic,
    }
}

impl DrumVoice {
    // CR-78 synthesis
    // ══════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_cr78(&mut self, sr: f64, c: &Controls, metal: &MetalBank) -> f64 {
        match voice_cr78(self.sound) {
            VoiceCr78::Bd => self.synth_cr78_kick(sr, c),
            VoiceCr78::Sd => self.synth_cr78_snare(sr),
            VoiceCr78::Rimshot => self.synth_cr78_rimshot(sr),
            VoiceCr78::HiHat => self.synth_cr78_hat(sr, metal),
            VoiceCr78::Cymbal => self.synth_cr78_cymbal(sr, metal),
            VoiceCr78::HiBongo => self.synth_cr78_drum(sr, 492.0, 0.070, 0.30),
            VoiceCr78::LowBongo => self.synth_cr78_drum(sr, 372.0, 0.088, 0.28),
            VoiceCr78::HiConga => self.synth_cr78_drum(sr, 268.0, 0.190, 0.20),
            VoiceCr78::LowConga => self.synth_cr78_drum(sr, 186.0, 0.240, 0.18),
            VoiceCr78::Tambourine => self.synth_cr78_tambourine(sr),
            VoiceCr78::Maracas => self.synth_cr78_maracas(sr),
            VoiceCr78::Claves => self.synth_cr78_claves(sr),
            VoiceCr78::Cowbell => self.synth_cr78_cowbell(sr, metal),
            VoiceCr78::Guiro => self.synth_cr78_guiro(sr),
            VoiceCr78::Metallic => self.synth_cr78_metallic(sr, metal),
        }
    }

    /// CR-78 Bass drum: a bridged-T ring with almost nothing on the front of
    /// it. Where the 808 gives you a click through a tone control and the 606
    /// a knock from a second resonator, this one is round and quiet — the
    /// balance slider is there because the kick needs help to be heard.
    fn synth_cr78_kick(&mut self, sr: f64, c: &Controls) -> f64 {
        let base = match self.sound {
            DrumSound::SubKick(mult) => 63.0 * mult,
            _ => 63.0,
        };
        // A shallow sweep: the strike lifts the ring by a fifth for a few
        // milliseconds and no more.
        let sweep = base * 0.50 * (-self.time / 0.008).exp();
        advance_phase(&mut self.phase1, base + sweep, sr);

        let env = (-self.time / (BD_SECONDS / DECAY_REFERENCE * self.accent_stretch())).exp();
        if env < 0.0005 {
            self.active = false;
            return 0.0;
        }

        let pulse = if self.time < 0.0006 { 0.30 } else { 0.0 };
        // A soft low-pass on the way out, which is where the woodiness comes
        // from: there is no bright edge on this drum at all.
        let out = self.lp1.tick_lp(osc_sine(self.phase1) * env + pulse, 420.0, sr) * 1.15;

        if c.drive > 0.01 { soft_clip(out, c.drive * 2.0) } else { out }
    }

    /// CR-78 Snare: white noise, and only white noise.
    ///
    /// There is no bridged-T in this circuit. The body is the same noise
    /// through a resonant low band rather than an oscillator ringing under it,
    /// which is what makes this snare a brush rather than a crack and is the
    /// clearest single difference between this machine and the 808.
    fn synth_cr78_snare(&mut self, sr: f64) -> f64 {
        let stretch = self.accent_stretch();
        let body_env = (-self.time / (0.075 / DECAY_REFERENCE * stretch)).exp();
        let wire_env = (-self.time / (SD_SECONDS / DECAY_REFERENCE * stretch)).exp();
        if body_env < 0.001 && wire_env < 0.001 {
            self.active = false;
            return 0.0;
        }

        let raw = self.noise();
        let body = self.svf1.bandpass(raw, 245.0, 1.3, sr) * body_env;
        let wires = self.hp1.tick_hp(raw, 1450.0, sr);
        // One more pole on top, because the CR-78's noise path is not bright:
        // the 808's snare noise runs to 7.5 kHz and this one is well under it.
        let shaped = self.lp1.tick_lp(wires, 4200.0, sr) * wire_env;
        body * 1.35 + shaped * 0.85
    }

    /// CR-78 Rim shot: a woodblock knock rather than a metal crack — a short
    /// pitched click with a band of noise on it.
    fn synth_cr78_rimshot(&mut self, sr: f64) -> f64 {
        let env = (-self.time / (RIM_SECONDS / DECAY_REFERENCE * self.accent_stretch())).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        advance_phase(&mut self.phase1, 424.0, sr);
        advance_phase(&mut self.phase2, 1265.0, sr);
        let knock = self.svf1.bandpass(self.noise(), 1900.0, 1.3, sr) * (-self.time / 0.0016).exp();
        let raw = osc_sine(self.phase1) * 0.42 + osc_sine(self.phase2) * 0.34 + knock * 0.55;
        self.hp1.tick_hp(raw, 300.0, sr) * env
    }

    /// CR-78 Hi-hat: the oscillator bank through the one LC band-pass. Short,
    /// and much softer than an 808's — one broad band at 6 kHz instead of a
    /// 3.4 kHz body and a 7.1 kHz strike high-passed together at 6.
    fn synth_cr78_hat(&mut self, sr: f64, metal: &MetalBank) -> f64 {
        let env = (-self.time / (HAT_SECONDS / DECAY_REFERENCE * self.accent_stretch())).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        let band = self.svf1.bandpass(metal.hash(), METAL_LC_HZ, METAL_LC_Q, sr);
        self.svf3.highpass(band, METAL_HP_HZ, 0.707, sr) * env * 1.6
    }

    /// CR-78 Cymbal: the LC band is the strike and the coil's low skirt is the
    /// body, which is the arrangement every metal circuit of the period uses —
    /// the top of a struck cymbal dies first and the wash under it is what is
    /// left. It is also the 808's arrangement, at a much lower pair of bands.
    ///
    /// This was written the other way round twice: first with the body on the
    /// LC band and a *higher* strike over it, then with the body still on the
    /// LC band. Both measured brighter than the 808's cymbal — one of them
    /// with a spectral centroid of 8.4 kHz against the 808's 3.1 — which is
    /// the opposite of what this machine is. A long envelope on the highest
    /// band in a voice is what makes a sound bright, whatever the filters are
    /// centred on.
    fn synth_cr78_cymbal(&mut self, sr: f64, metal: &MetalBank) -> f64 {
        let stretch = self.accent_stretch();
        let body_env = (-self.time / (CY_BODY_SECONDS / DECAY_REFERENCE * stretch)).exp();
        let strike_env = (-self.time / (CY_STRIKE_SECONDS / DECAY_REFERENCE * stretch)).exp();
        if body_env < 0.001 && strike_env < 0.001 {
            self.active = false;
            return 0.0;
        }
        let raw = metal.hash();
        let strike = self.svf1.bandpass(raw, METAL_LC_HZ, METAL_LC_Q, sr);
        let body = self.svf2.bandpass(raw, METAL_LC_HZ * 0.42, 1.4, sr);
        self.svf3.highpass(
            body * body_env * 1.15 + strike * strike_env * 0.55,
            METAL_HP_HZ * 0.5,
            0.707,
            sr,
        ) * 1.5
    }

    /// CR-78 Bongo and conga: one resonator per drum with a hand on the front
    /// of it. No tuning control; these are where the circuits sit.
    fn synth_cr78_drum(&mut self, sr: f64, freq: f64, seconds: f64, slap: f64) -> f64 {
        let sweep = freq * 0.18 * (-self.time / 0.008).exp();
        advance_phase(&mut self.phase1, freq + sweep, sr);

        let env = (-self.time / (seconds / DECAY_REFERENCE * self.accent_stretch())).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        let palm = self.hp1.tick_hp(self.noise(), 1600.0, sr) * (-self.time / 0.0035).exp();
        (osc_sine(self.phase1) + palm * slap) * env
    }

    /// CR-78 Tambourine: a narrower and lower jingle than the 808's, with the
    /// rattle in the envelope rather than in the filter.
    fn synth_cr78_tambourine(&mut self, sr: f64) -> f64 {
        let env = (-self.time / (0.130 / DECAY_REFERENCE * self.accent_stretch())).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        let jingle = self.svf1.bandpass(self.noise(), 6800.0, 1.4, sr);
        let rattle = 0.62 + 0.38 * (self.time * 170.0 * std::f64::consts::TAU).sin().abs();
        jingle * rattle * env * 1.1
    }

    /// CR-78 Maracas: a short shake, and a soft one. The 808's is noise above
    /// 5 kHz with nothing over it; this one is rolled off on top, which is the
    /// same difference the rest of the machine has.
    fn synth_cr78_maracas(&mut self, sr: f64) -> f64 {
        let env = (-self.time / (0.045 / DECAY_REFERENCE * self.accent_stretch())).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        let hp = self.hp1.tick_hp(self.noise(), 4300.0, sr);
        self.lp1.tick_lp(hp, 9000.0, sr) * env * 1.1
    }

    /// CR-78 Claves: a woodblock, so two partials rather than the one pure
    /// sine the 808 uses.
    fn synth_cr78_claves(&mut self, sr: f64) -> f64 {
        let env = (-self.time / (0.022 / DECAY_REFERENCE * self.accent_stretch())).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        advance_phase(&mut self.phase1, 2280.0, sr);
        advance_phase(&mut self.phase2, 3540.0, sr);
        (osc_sine(self.phase1) * 0.78 + osc_sine(self.phase2) * 0.22) * env
    }

    /// CR-78 Cowbell: two of the six oscillators, as on the 808 — but into a
    /// band-pass an octave lower, which is a duller bell.
    fn synth_cr78_cowbell(&mut self, sr: f64, metal: &MetalBank) -> f64 {
        let stretch = self.accent_stretch();
        let env = 0.5 * (-self.time / (0.014 * stretch)).exp()
            + 0.5 * (-self.time / (0.120 / DECAY_REFERENCE * stretch)).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        let bp = self.svf1.bandpass(metal.cowbell(), 840.0, 1.1, sr);
        self.hp1.tick_hp(bp, 300.0, sr) * env * 1.15
    }

    /// CR-78 Guiro: a gourd scraped with a stick, so a run of ticks under one
    /// falling envelope.
    fn synth_cr78_guiro(&mut self, sr: f64) -> f64 {
        let total = 0.180 * self.accent_stretch();
        if self.time > total {
            self.active = false;
            return 0.0;
        }
        let band = self.svf1.bandpass(self.noise(), 3100.0, 2.2, sr);
        // The teeth of the scraper, at a fixed spacing.
        let within = (self.time * 62.0).fract();
        let tick = (-within / 0.22).exp();
        band * tick * (1.0 - self.time / total) * 1.2
    }

    /// CR-78 Metallic beat: three of the six oscillators through a narrow
    /// band-pass and a fast envelope. There is nothing else like this on any
    /// later Roland, and three filtered square waves is exactly what it is.
    fn synth_cr78_metallic(&mut self, sr: f64, metal: &MetalBank) -> f64 {
        let env = (-self.time / (METALLIC_SECONDS / DECAY_REFERENCE * self.accent_stretch())).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        let band = self.svf1.bandpass(metal.chime(), 3600.0, 3.0, sr);
        self.svf3.highpass(band, 1800.0, 0.707, sr) * env * 1.4
    }
}
