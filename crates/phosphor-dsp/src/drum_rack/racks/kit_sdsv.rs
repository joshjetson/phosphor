//! Simmons SDS-V kit synthesis.
//!
//! 1981, and the only machine in this rack that is a synthesizer rather than a
//! drum machine. There is no ROM in it and no bridged-T either: each voice is
//! a module in a 19" rack, and each module is
//!
//! * a **triangle VCO** for the body, tuned by a TONE PITCH pot the manual
//!   describes as running "from an 8" tom tom to a large timpani", with a
//!   **BEND** control that drops the pitch as the note decays;
//! * a **transistor noise source** for the attack, through a **four-pole
//!   SSM2044 low-pass** whose corner is the NOISE PITCH pot — the manual calls
//!   it the brightness of the stick strike — and whose resonance resistor is
//!   set to none on the bass drum and the toms and used on the snare;
//! * a **CLICK**, which is a short boost of that same noise mixed in ahead of
//!   the first VCA, so it is filtered with the noise and the NOISE PITCH knob
//!   is what decides how sharp it is;
//! * a fixed **LFO**, triangle, on the VCO's pitch and the filter's corner;
//! * **OTA VCAs** driven by ramp generators off CD4528 monostables — so the
//!   envelopes are straight lines, not exponentials, and a Simmons drum stops
//!   rather than fading. That is modelled directly; it is a good part of why
//!   these sound the way they do.
//!
//! The standard machine is five modules — bass drum, snare and three toms —
//! and that is what is here. Optional hi-hat and cymbal modules existed, but
//! they were EPROM playback at eight linear bits rather than a sixth of this
//! circuit, so building an analog hat here would be building a machine Simmons
//! did not sell. Everything metal, shaken or clapped is played on the snare
//! instead, which is the machine's only noise voice — see [`module_sdsv`].
//!
//! # What the Roland panel does and does not have room for
//!
//! Each module carries six knobs and this panel names four of them. The
//! mapping, stated plainly:
//!
//! | module knob        | strip                                    |
//! |--------------------|------------------------------------------|
//! | TONE PITCH         | TUNE, on all five modules                |
//! | NOISE PITCH        | TONE, on the bass drum and the snare      |
//! | DECAY              | DECAY, on the bass drum and the three toms|
//! | CLICK-DRUM         | ATTACK, on the bass drum                 |
//! | NOISE-TONE         | SNAPPY, on the snare                     |
//! | BEND               | nowhere                                  |
//!
//! Three of them therefore have no knob on this panel and sit at the value the
//! module was voiced at: BEND everywhere, the snare's DECAY (this panel's
//! snare strip has no decay control on it), and the toms' NOISE PITCH,
//! NOISE-TONE and CLICK. BEND is the one that hurts, because the falling tom
//! is what this machine is remembered for — so it is voiced deep and left
//! there rather than wired to a knob belonging to another instrument.

use super::super::*;

/// How far TONE PITCH swings either side of the module's own tuning. The
/// manual's range — an 8" tom to a large timpani — is about 45 Hz to 220 Hz,
/// so a little over two octaves of travel, centred.
const TUNE_SPAN: f64 = 2.2;

/// How far DECAY swings either side of the module's own ramp.
const DECAY_SPAN: f64 = 2.6;

/// How far NOISE PITCH swings the SSM2044's corner either side of the
/// module's own. The manual gives this control no range, so the figure is
/// chosen: a little over a octave either way.
///
/// It was two octaves either way first, and that is where the rack's headroom
/// sweep found this kit. White noise through a low-pass carries power in
/// proportion to the corner, so opening the filter four times over makes the
/// module twice as loud — with the snare's five folded parts landing together
/// that put the whole kit 0.6 dB past the master limiter's ceiling. Narrowing
/// the control is the fix; trimming the rack for one knob position on one kit
/// is not.
const NOISE_PITCH_SPAN: f64 = 2.2;

/// The LFO. Triangle, and fixed on the instrument — the speed control was
/// only ever on the expanded board — so this rate is chosen rather than
/// derived: fast enough to put a flutter on a 300 ms drum, slow enough not to
/// become frequency modulation.
const LFO_HZ: f64 = 14.0;

/// How far the SSM2044's corner falls over the note. The manual describes the
/// noise as becoming duller as the sound dies away, and this is that.
const FILTER_FALL: f64 = 0.35;
const FILTER_FALL_TAU: f64 = 0.060;

/// One voice module, at the settings it was shipped at.
#[derive(Debug, Clone, Copy)]
struct Voicing {
    /// TONE PITCH at the centre detent.
    hz: f64,
    /// BEND: how far above its resting pitch the VCO starts, as a multiple,
    /// and how long it takes to get down.
    bend: f64,
    bend_tau: f64,
    /// DECAY at the centre detent: the length of the tone VCA's ramp.
    ramp: f64,
    /// How much of that ramp the noise VCA runs for.
    noise_ramp: f64,
    /// NOISE PITCH at the centre detent: the SSM2044's corner.
    cutoff: f64,
    /// The filter's resonance. 0.707 is the resistor set to none, which is
    /// the bass drum and the toms; the snare is the module that uses it.
    q: f64,
    /// NOISE-TONE at the centre detent.
    noise_mix: f64,
    /// CLICK-DRUM at the centre detent.
    click: f64,
    /// How deep the LFO runs into the VCO on this module.
    lfo_depth: f64,
    /// Where the module sits against the rest of the machine.
    out: f64,
}

/// The five standard modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModuleSdsV {
    Bass,
    Snare,
    LowTom,
    MidTom,
    HighTom,
}

impl ModuleSdsV {
    /// The panel strip this module is played from.
    pub(crate) fn strip(self) -> Instrument {
        match self {
            Self::Bass => Instrument::Bd,
            Self::Snare => Instrument::Sd,
            Self::LowTom => Instrument::LowTom,
            Self::MidTom => Instrument::MidTom,
            Self::HighTom => Instrument::HighTom,
        }
    }

    /// How the module left the factory.
    ///
    /// The boards are the same circuit with the pots set for the drum each is
    /// meant to be; the toms carry the deep bend the machine is known by and
    /// the bass drum a much shallower one, because a bass drum that swept an
    /// octave would be a tom.
    fn voicing(self) -> Voicing {
        match self {
            Self::Bass => Voicing {
                hz: 62.0,
                bend: 0.45,
                bend_tau: 0.028,
                ramp: 0.400,
                noise_ramp: 0.10,
                cutoff: 850.0,
                q: 0.707,
                noise_mix: 0.20,
                click: 0.45,
                lfo_depth: 0.010,
                out: 1.05,
            },
            Self::Snare => Voicing {
                hz: 190.0,
                bend: 0.28,
                bend_tau: 0.018,
                ramp: 0.290,
                noise_ramp: 0.95,
                cutoff: 2600.0,
                // The one module with the resonance resistor fitted.
                q: 2.6,
                noise_mix: 0.68,
                click: 0.38,
                lfo_depth: 0.035,
                out: 0.95,
            },
            Self::LowTom => Voicing {
                hz: 92.0,
                bend: 1.10,
                bend_tau: 0.110,
                ramp: 0.680,
                noise_ramp: 0.09,
                cutoff: 1200.0,
                q: 0.707,
                noise_mix: 0.18,
                click: 0.40,
                lfo_depth: 0.022,
                out: 1.0,
            },
            Self::MidTom => Voicing {
                hz: 128.0,
                bend: 1.05,
                bend_tau: 0.095,
                ramp: 0.580,
                noise_ramp: 0.09,
                cutoff: 1500.0,
                q: 0.707,
                noise_mix: 0.18,
                click: 0.40,
                lfo_depth: 0.022,
                out: 1.0,
            },
            Self::HighTom => Voicing {
                hz: 178.0,
                bend: 1.00,
                bend_tau: 0.080,
                ramp: 0.490,
                noise_ramp: 0.09,
                cutoff: 1900.0,
                q: 0.707,
                noise_mix: 0.18,
                click: 0.40,
                lfo_depth: 0.022,
                out: 1.0,
            },
        }
    }
}

/// Which module a note is played on.
///
/// The rule is the 606's: a note the machine has no voice for is folded onto
/// the nearest voice it does have, at that voice's fader, rather than silenced
/// or filled in from another machine's circuit. With five modules the folds go
/// a long way — every hi-hat, cymbal, shaker and clap in the note map lands on
/// the snare, because the snare's noise source is the only one in the rack.
pub(crate) fn module_sdsv(sound: DrumSound) -> ModuleSdsV {
    use DrumSound as S;
    match sound {
        S::Kick | S::SubKick(_) => ModuleSdsV::Bass,
        S::LowTom => ModuleSdsV::LowTom,
        S::MidTom => ModuleSdsV::MidTom,
        S::HighTom => ModuleSdsV::HighTom,
        // Hand drums sort onto the three toms by pitch, as they do everywhere
        // else in this rack.
        S::Conga(f) | S::Bongo(f) | S::Timbale(f) => {
            if f < 260.0 {
                ModuleSdsV::LowTom
            } else if f < 360.0 {
                ModuleSdsV::MidTom
            } else {
                ModuleSdsV::HighTom
            }
        }
        // Pitched percussion with no board of its own goes on the high tom,
        // which with its click up is the closest this machine has to a bell.
        S::Cowbell | S::Agogo(_) => ModuleSdsV::HighTom,
        // Everything else — the snare, the rimshot, both hats, every cymbal,
        // the clap and every shaker — is the noise module.
        _ => ModuleSdsV::Snare,
    }
}

/// What this machine's decay knobs render, in seconds to −20 dB.
///
/// The envelope is a straight line rather than an exponential, and a line
/// reaches a tenth of its height nine tenths of the way along, so a ramp of
/// `t` seconds reads as 0.9·t here. Reporting the ramp length itself would put
/// this machine's numbers on a different footing from every other kit's.
///
/// The snare has a DECAY knob on the instrument and no strip for it on this
/// panel, so there is nothing to report at `P_SD_*`; the cymbal and hi-hat
/// strips have no module behind them at all.
pub(crate) fn decay_seconds(index: usize, knob: f64) -> Option<f64> {
    let module = match index {
        P_BD_DECAY => ModuleSdsV::Bass,
        P_LT_DECAY => ModuleSdsV::LowTom,
        P_MT_DECAY => ModuleSdsV::MidTom,
        P_HT_DECAY => ModuleSdsV::HighTom,
        _ => return None,
    };
    Some(module.voicing().ramp * geometric(1.0 / DECAY_SPAN, DECAY_SPAN, knob) * 0.9)
}

/// A knob into a ratio either side of unity, unity at the centre detent.
fn span(amount: f64, knob: f64) -> f64 {
    geometric(1.0 / amount, amount, knob)
}

impl DrumVoice {
    // SDS-V synthesis — VCO, noise, SSM2044, OTA VCA
    // ══════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_sdsv(&mut self, sr: f64, c: &Controls) -> f64 {
        let module = module_sdsv(self.sound);
        let v = module.voicing();
        let stretch = self.accent_stretch();

        // ── The ramp generators. Straight lines, which is what a CD4528 into
        // an OTA gives you: a Simmons drum ends, it does not fade out.
        let ramp = v.ramp * span(DECAY_SPAN, c.decay) * stretch;
        let tone_env = (1.0 - self.time / ramp).max(0.0);
        let noise_env = (1.0 - self.time / (ramp * v.noise_ramp)).max(0.0);
        if tone_env <= 0.0 && noise_env <= 0.0 {
            self.active = false;
            return 0.0;
        }

        // ── The LFO, one triangle, into both the VCO and the filter.
        advance_phase(&mut self.phase3, LFO_HZ, sr);
        let lfo = osc_triangle(self.phase3);

        // ── The VCO. TONE PITCH sets where it rests and BEND is how far above
        // that the note starts; on the toms that is more than an octave, and
        // it is the descending fill the machine is remembered for.
        let base = match self.sound {
            DrumSound::SubKick(mult) => v.hz * mult,
            _ => v.hz,
        };
        let rest = base * span(TUNE_SPAN, c.tune);
        let bend = 1.0 + v.bend * (-self.time / v.bend_tau).exp();
        advance_phase(&mut self.phase1, rest * bend * (1.0 + v.lfo_depth * lfo), sr);
        let tone = osc_triangle(self.phase1) * tone_env;

        // ── The noise path. The click is a short boost of the same source
        // mixed in ahead of the VCA, so it goes through the filter with it and
        // NOISE PITCH is what decides how sharp it reads.
        //
        // The boost is three times rather than the six it was first voiced at.
        // Six put the snare 0.6 dB over the master limiter's ceiling with the
        // whole panel at its top — a resonant four-pole struck by a burst that
        // large rings past everything else in the rack — and the fix belongs
        // in this module's own gain staging rather than in the rack's trim.
        let click_amount = (v.click * (0.2 + 1.6 * c.attack)).clamp(0.0, 1.6);
        let click = 1.0 + click_amount * 3.0 * (-self.time / 0.0022).exp();
        let raw = self.noise() * click;

        // The SSM2044, four poles of it. Its corner falls over the note —
        // "duller as the sound dies away" — with the LFO rippling it.
        let corner = v.cutoff
            * span(NOISE_PITCH_SPAN, c.tone)
            * (1.0 - FILTER_FALL * (1.0 - (-self.time / FILTER_FALL_TAU).exp()))
            * (1.0 + 0.06 * lfo);
        let corner = corner.clamp(60.0, sr * 0.45);
        let stage1 = self.svf1.lowpass(raw, corner, v.q, sr);
        let noise = self.svf2.lowpass(stage1, corner, 0.707, sr) * noise_env;

        // ── NOISE-TONE, the balance between the two VCAs.
        let mix = (v.noise_mix + (c.snappy - 0.5) * 0.8).clamp(0.0, 1.0);
        let out = (tone * (1.0 - mix) * 1.20 + noise * mix * 1.10) * v.out;

        if c.drive > 0.01 { drive_stage(out, c.drive * 2.0) } else { out }
    }
}
