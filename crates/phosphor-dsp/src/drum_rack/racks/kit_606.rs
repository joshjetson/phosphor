//! 606 kit synthesis.
//!
//! The TR-606 is fully analog and deliberately small. Its service notes list
//! the sound sources on the first page and there are seven of them: bass drum,
//! snare drum, low tom, high tom, cymbal, open hi-hat and closed hi-hat, with
//! accent as the eighth track. There is no clap, no conga, no cowbell, no
//! rimshot, no maracas and no claves anywhere in the machine, and the whole of
//! its front panel is six mix knobs — accent, bass drum, snare drum, one for
//! both toms, cymbal, one for both hi-hats — and nothing else. No tuning, no
//! tone, no decay, on any voice.
//!
//! This file was previously seven-eighths of an 808: five voices of its own
//! and twenty borrowed, including a cowbell, congas and a rimshot the
//! instrument does not have. What a note with no voice does now is [an
//! explicit table](voice_606) rather than a silent borrow — see the comment
//! there for the reasoning.
//!
//! The numbers are worked out of the service-notes schematic (Jan 6 1982,
//! main board OP3126-050 and sub board OP3126-060) the same way the 808's
//! are. Where a value is not derivable from the printed one, the comment says
//! so rather than dressing a choice up as a measurement.

use super::super::*;

// ── Bass drum ──
//
// Two op-amp resonators in cascade, IC5A into IC5B, which is one more stage
// than the 808's bass drum has and is most of why the 606's is thinner.

/// IC5A, the body: R59 680k across R58 3.3k with C19 and C20 at 0.056 µF,
/// which puts the resonance at 60.0 Hz. The 808's equivalent stage is 49.4 Hz,
/// so the 606's kick sits about a third above it — audibly a smaller drum.
const BD_HZ: f64 = 60.0;
/// IC5B, the knock: R61 560k across R66 12k with C23 and C24 at 0.015 µF, fed
/// from IC5A through R56 10k, which resonates at 192 Hz.
///
/// The stage's midband gain works out at 28 — R61 over twice R56 — which the
/// rest of the board plainly does not pass, so what is kept here is the
/// emphasis and not the 29 dB. This is the second resonance that a 606 kick
/// has and an 808 kick does not.
const BD_KNOCK_HZ: f64 = 192.0;
const BD_KNOCK_Q: f64 = 3.5;
const BD_KNOCK_MIX: f64 = 0.7;
/// The strike starts the ring above its resting pitch, as any bridged-T does.
const BD_SWEEP: f64 = 0.55;
const BD_SWEEP_TAU: f64 = 0.007;
/// Ring time. The schematic gives the envelope network but not the discharge
/// path that sets its rate, so this is chosen and not derived: 250 ms to
/// −20 dB, a third of the 808's centre detent, which is the length a machine
/// with no decay control has to be to stay out of the way.
/// Measured on the body, past the strike: 235 ms to −20 dB from 15 ms in.
const BD_TAU: f64 = 0.250 / DECAY_REFERENCE;
/// The trigger pulse. It is what sets the second stage ringing, so its area
/// is what decides how much knock the drum has — a longer, softer pulse than
/// the 808's rectangular one, because a rectangle here is a spike on the
/// output rather than a knock in it.
const BD_CLICK_TAU: f64 = 0.0016;
const BD_CLICK_GAIN: f64 = 1.1;
/// Where the two stages together leave the drum against the rest of the
/// machine. A 606 kick is a small sound and this is not the place to argue
/// with that, but the second resonator costs it more than the board does.
const BD_OUT: f64 = 1.15;

// ── Snare drum ──

/// One oscillator, not two: IC10A with R112 820k across R107 330 and C32/C33
/// at 0.027 µF, which resonates at 358 Hz. The 808's snare board carries two
/// bridged-Ts an octave apart at 238 Hz and 476 Hz; the 606 has the one, half
/// an octave above the 808's lower, and that is the whole tonal difference
/// between the two snares before the noise is added.
const SD_HZ: f64 = 358.0;
/// The noise path's high-pass: C52 and C58 at 0.0018 µF with R153 22k and
/// R152 100k, about 1.9 kHz, against the 808's 1.2 kHz. Thinner by design.
const SD_NOISE_HP: f64 = 1900.0;
/// Chosen, as above: 120 ms of drum under 190 ms of wire.
const SD_TONE_TAU: f64 = 0.120 / DECAY_REFERENCE;
const SD_NOISE_TAU: f64 = 0.190 / DECAY_REFERENCE;
/// The ATTACK stage ahead of the oscillator (D17 on the block diagram), which
/// is a click into the resonator rather than a sound of its own.
const SD_CLICK_SECONDS: f64 = 0.0004;
const SD_CLICK_GAIN: f64 = 0.35;
const SD_NOISE_MIX: f64 = 0.62;

// ── Toms ──

/// Low and high tom, an octave apart.
///
/// The two boards are identical but for the bridged-T's capacitors and the
/// resistor at their junction — 0.027 µF with 180 Ω on the low tom, 0.018 µF
/// with 100 Ω on the high — and that pair of ratios puts them exactly an
/// octave apart. The absolute tuning is the one number in this machine that
/// the scan does not settle, because the junction resistor is switched by the
/// ATTACK transistor and the printed value is not the effective one; the pair
/// is anchored here so the low tom sits a fifth above the 808's, which is the
/// difference the two machines are described by. The output filter is the
/// upper bound on it: 405 Hz, below, means the high tom cannot be much above
/// 300 Hz and still be a tom.
const TOM_HZ: [f64; 2] = [150.0, 300.0];
/// Chosen: 260 ms and 190 ms to −20 dB, the higher drum the shorter.
const TOM_TAU: [f64; 2] = [0.260 / DECAY_REFERENCE, 0.190 / DECAY_REFERENCE];
/// Both toms share one low-pass and one VCA on the sub board: R346 10k and
/// R342 22k with C314 0.039 µF and C315 0.018 µF, a 405 Hz Sallen-Key. This
/// is what makes a 606 tom a thud with a tick on it rather than a ring.
const TOM_LP: f64 = 405.0;
/// Both toms share the sub board's output stage with everything else on it,
/// and this is where that stage leaves them against the rest of the machine.
const TOM_OUT: f64 = 0.75;
/// The ATTACK transistor ahead of each tom's oscillator. Short and hard: it
/// is the percussive front of the sound and it is inside the low-pass, so it
/// reads as a knock rather than as a click.
const TOM_CLICK_SECONDS: f64 = 0.0015;
const TOM_CLICK_GAIN: f64 = 0.5;
const TOM_SWEEP: f64 = 0.22;
const TOM_SWEEP_TAU: f64 = 0.010;

// ── Metal section ──
//
// Six Schmitt-trigger oscillators — HAT_FREQS_606, derived in the parent
// module — summed into two multiple-feedback band-passes, then a VCA and a
// high-pass on each path. Exactly the 808's architecture with the 606's
// components, which is why the cymbal and both hats share one bank here as
// they do there, and why a closed hat cuts an open one off.

/// IC15B: R220 82k, R222 560, C90/C91 0.0068 µF.
const METAL_BP_LOW: f64 = 3500.0;
/// IC15A: R212 82k, R223 560, C94/C95 0.0033 µF.
const METAL_BP_HIGH: f64 = 7200.0;
/// The cymbal's two output high-passes: Q40 with C59/C60 0.0022 µF against
/// R159 22k and R195 100k, and IC12B with C63/C73 0.001 µF against R207 and
/// R208 39k.
const CY_HP_BODY: f64 = 2200.0;
const CY_HP_STRIKE: f64 = 4100.0;
/// The hi-hat's: C44 and C67 0.0022 µF with R172 12k and R170 33k.
const HAT_HP: f64 = 3600.0;
/// Chosen: 800 ms of body under a 250 ms strike, which is a cymbal short
/// enough to belong to a machine with no decay knob. The 808's runs 350 ms to
/// 1.2 s and has one.
const CY_BODY_TAU: f64 = 0.800 / DECAY_REFERENCE;
const CY_STRIKE_TAU: f64 = 0.250 / DECAY_REFERENCE;
/// Chosen: 300 ms open, 60 ms closed. The 808 is 90-600 ms and a fixed 50 ms.
const OH_TAU: f64 = 0.300 / DECAY_REFERENCE;
const CH_TAU: f64 = 0.060 / DECAY_REFERENCE;

/// The seven voices a TR-606 has.
///
/// One table, two readers: [`voice_606`] answers both "what does this note
/// sound like" for the synthesis below and "which fader is it under" for
/// [`instrument_of`], so the two cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Voice606 {
    Bd,
    Sd,
    LowTom,
    HighTom,
    Cymbal,
    OpenHat,
    ClosedHat,
}

impl Voice606 {
    /// The panel strip this voice is played from.
    pub(crate) fn strip(self) -> Instrument {
        match self {
            Self::Bd => Instrument::Bd,
            Self::Sd => Instrument::Sd,
            Self::LowTom => Instrument::LowTom,
            Self::HighTom => Instrument::HighTom,
            Self::Cymbal => Instrument::Cymbal,
            Self::OpenHat => Instrument::OpenHat,
            Self::ClosedHat => Instrument::ClosedHat,
        }
    }
}

/// Which of the seven a note is played on.
///
/// **The decision this table is:** a note the machine has no voice for is
/// folded onto the nearest voice it does have, rather than silenced and rather
/// than filled in with a circuit from another machine.
///
/// Silence was the alternative, and it is the more literal answer: a TR-606
/// asked for a conga produces nothing, because there is no conga in it. It
/// was rejected because of what this rack is — one instrument on a track,
/// switchable between ten kits, playing a General MIDI part. Under silence,
/// switching a finished part to the 606 deletes half of it with no indication
/// of why, and the fix the player wants is the one a player with a 606 in
/// front of them makes anyway: play the part on the voices the machine has.
/// Folding does that for them, and it is what the machine leaves you.
///
/// What it must not do — and what it did before — is fold quietly onto
/// *another machine's* circuit. Every fold below lands on one of the 606's own
/// seven, at that voice's fader, so the panel reads as the instrument does.
pub(crate) fn voice_606(sound: DrumSound) -> Voice606 {
    use DrumSound as S;
    match sound {
        S::Kick | S::SubKick(_) => Voice606::Bd,
        // The snare is the only voice with a crack in it, so the rimshot, the
        // claves and the hand clap are played on it.
        S::Snare | S::SnareAlt | S::Rimshot | S::Clave | S::Clap => Voice606::Sd,
        // Two toms, not three: the mid tom's notes are the lower half of the
        // General MIDI set, so they go to the low tom.
        S::LowTom | S::MidTom => Voice606::LowTom,
        S::HighTom => Voice606::HighTom,
        // Congas, bongos and timbales sort onto the two toms by pitch, at the
        // boundary between them. Cowbells and agogos are pitched percussion
        // with no board of their own; they go on the high tom.
        S::Conga(f) | S::Bongo(f) | S::Timbale(f) => {
            if f < 300.0 { Voice606::LowTom } else { Voice606::HighTom }
        }
        S::Cowbell | S::Agogo(_) => Voice606::HighTom,
        S::ClosedHat | S::PedalHat => Voice606::ClosedHat,
        S::OpenHat => Voice606::OpenHat,
        // One cymbal, so the crash, the splash and the ride are all it.
        S::Crash | S::Splash | S::Cymbal | S::Ride | S::RideBell => Voice606::Cymbal,
        // Everything shaken, scraped or blown is a short bright noise, and the
        // closed hat is the machine's short bright noise.
        S::Maracas
        | S::Cabasa
        | S::Tambourine
        | S::Vibraslap
        | S::Guiro(_)
        | S::Whistle(_)
        | S::FxNoise(_) => Voice606::ClosedHat,
    }
}

impl DrumVoice {
    // 606 synthesis
    // ══════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_606(&mut self, sr: f64, c: &Controls, metal: &MetalBank) -> f64 {
        match voice_606(self.sound) {
            Voice606::Bd => self.synth_606_kick(sr, c),
            Voice606::Sd => self.synth_606_snare(sr),
            Voice606::LowTom => self.synth_606_tom(sr, TOM_HZ[0], TOM_TAU[0]),
            Voice606::HighTom => self.synth_606_tom(sr, TOM_HZ[1], TOM_TAU[1]),
            Voice606::Cymbal => self.synth_606_cymbal(sr, metal),
            Voice606::OpenHat => self.synth_606_hat(sr, metal, OH_TAU),
            Voice606::ClosedHat => self.synth_606_hat(sr, metal, CH_TAU),
        }
    }

    /// 606 Bass drum: a 60 Hz resonator into a 192 Hz one.
    ///
    /// No tuning knob and no decay knob, because the machine has neither. The
    /// rack's own DRIVE control is the exception — it is ours, not Roland's.
    fn synth_606_kick(&mut self, sr: f64, c: &Controls) -> f64 {
        let base = match self.sound {
            DrumSound::SubKick(mult) => BD_HZ * mult,
            _ => BD_HZ,
        };
        let sweep = base * BD_SWEEP * (-self.time / BD_SWEEP_TAU).exp();
        advance_phase(&mut self.phase1, base + sweep, sr);

        let env = (-self.time / (BD_TAU * self.accent_stretch())).exp();
        if env < 0.0005 {
            self.active = false;
            return 0.0;
        }

        let pulse = BD_CLICK_GAIN * (-self.time / BD_CLICK_TAU).exp();
        let first = osc_sine(self.phase1) * env + pulse;
        let knock = self.svf1.bandpass(first, BD_KNOCK_HZ, BD_KNOCK_Q, sr);
        let out = (first * (1.0 - BD_KNOCK_MIX) + knock * BD_KNOCK_MIX) * BD_OUT;

        if c.drive > 0.01 { soft_clip(out, c.drive * 2.0) } else { out }
    }

    /// 606 Snare: one 358 Hz resonator, a click into it, and noise above
    /// 1.9 kHz. Two thirds of what is heard is the noise.
    fn synth_606_snare(&mut self, sr: f64) -> f64 {
        let stretch = self.accent_stretch();
        let sweep = SD_HZ * 0.10 * (-self.time / 0.004).exp();
        advance_phase(&mut self.phase1, SD_HZ + sweep, sr);

        let tone_env = (-self.time / (SD_TONE_TAU * stretch)).exp();
        let noise_env = (-self.time / (SD_NOISE_TAU * stretch)).exp();
        if tone_env < 0.001 && noise_env < 0.001 {
            self.active = false;
            return 0.0;
        }

        let pulse = if self.time < SD_CLICK_SECONDS { SD_CLICK_GAIN } else { 0.0 };
        let tonal = (osc_sine(self.phase1) + pulse) * tone_env;
        let noise = self.hp1.tick_hp(self.noise(), SD_NOISE_HP, sr) * noise_env;
        tonal * (1.0 - SD_NOISE_MIX) + noise * SD_NOISE_MIX
    }

    /// 606 Tom: bridged-T with the ATTACK transistor's knock in front of it,
    /// through the 405 Hz low-pass both toms share.
    fn synth_606_tom(&mut self, sr: f64, freq: f64, tau: f64) -> f64 {
        let sweep = freq * TOM_SWEEP * (-self.time / TOM_SWEEP_TAU).exp();
        advance_phase(&mut self.phase1, freq + sweep, sr);

        let env = (-self.time / (tau * self.accent_stretch())).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }

        let knock = if self.time < TOM_CLICK_SECONDS { TOM_CLICK_GAIN } else { 0.0 };
        self.lp1.tick_lp(osc_sine(self.phase1) * env + knock, TOM_LP, sr) * TOM_OUT
    }

    /// 606 Cymbal: the six oscillators through both band-passes, each path
    /// with its own envelope and its own high-pass.
    fn synth_606_cymbal(&mut self, sr: f64, metal: &MetalBank) -> f64 {
        let stretch = self.accent_stretch();
        let body_env = (-self.time / (CY_BODY_TAU * stretch)).exp();
        let strike_env = (-self.time / (CY_STRIKE_TAU * stretch)).exp();
        if body_env < 0.001 && strike_env < 0.001 {
            self.active = false;
            return 0.0;
        }

        let raw = metal.hash();
        let low = self.svf1.bandpass(raw, METAL_BP_LOW, 3.0, sr) * body_env;
        let high = self.svf2.bandpass(raw, METAL_BP_HIGH, 3.0, sr) * strike_env;
        // Two high-passes, as the instrument has, though the upper one is a
        // single pole here: the 7.2 kHz band-pass ahead of it has already
        // taken the oscillators' own 145-370 Hz out.
        self.svf3.highpass(low, CY_HP_BODY, 0.707, sr) * 0.95
            + self.hp1.tick_hp(high, CY_HP_STRIKE, sr) * 1.1
    }

    /// 606 Hi-hat: the same bank and the same band-passes as the cymbal,
    /// weighted towards the upper one, with the fixed ring time of whichever
    /// of the two envelope generators fired. One VCA between them, so a closed
    /// hat cuts an open one off — the "envelope shut off" on the block
    /// diagram, handled rack-wide by [`DrumVoice::choke`].
    fn synth_606_hat(&mut self, sr: f64, metal: &MetalBank, tau: f64) -> f64 {
        let env = (-self.time / (tau * self.accent_stretch())).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }

        let raw = metal.hash();
        let low = self.svf1.bandpass(raw, METAL_BP_LOW, 3.0, sr);
        let high = self.svf2.bandpass(raw, METAL_BP_HIGH, 3.0, sr);
        let mixed = low * 0.5 + high * 0.92;
        self.svf3.highpass(mixed, HAT_HP, 0.707, sr) * env
    }
}
