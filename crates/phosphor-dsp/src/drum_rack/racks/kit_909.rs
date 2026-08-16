//! 909 kit synthesis.
//!
//! The TR-909 is a hybrid and the split is the instrument: **bass drum, snare,
//! toms, rimshot and hand clap are analog circuits**, and **the hi-hat, the
//! ride and the crash — those three and nothing else — are 6-bit samples
//! clocked at 18 kHz** into an otherwise analog signal path. Roland went
//! digital for the cymbals because the analog ones were not good enough before
//! the deadline, and the quantisation is not incidental damage: it is most of
//! why a 909 cymbal sounds like a 909 cymbal.
//!
//! What this file used to be: six voices of its own and nineteen taken from
//! the 808, including the crash, the ride and the cowbell. Its hats were the
//! 808's six square oscillators with a bit-crusher after the envelope, which
//! is the one place the crushing must not go — see [`DrumVoice::quantize`].
//!
//! Two things about this machine that are easy to get backwards, both from
//! Robin Whittle's circuit notes on modifying it:
//!
//! * **The bass drum's TUNE control is not a pitch control.** It sets how long
//!   the oscillator runs above its resting frequency at the start of the note
//!   — the decay time of the pitch envelope. Turning it up lengthens the sweep
//!   rather than raising the drum.
//! * **The snare's TONE control is not a filter.** It sets the *length* of the
//!   noise, and SNAPPY sets its level. The two oscillators go to the output at
//!   full level whatever either knob says.
//!
//! And one about its noise: the 808 amplifies a noisy transistor, the 909
//! clocks a pair of 18-stage shift registers. That is a random binary
//! sequence, not an analog hiss — see [`DrumVoice::digital_noise`].
//!
//! The machine has no cowbell circuit, so cowbell notes are played on the
//! rimshot, at the rimshot's level knob — see [`instrument_of`].

use super::super::*;

// ── Bass drum ──

/// Resting frequency of the bridged-T. Measured on the instrument at about
/// 55 Hz, against the 808's 49.4 Hz.
const BD_HZ: f64 = 55.0;
/// How far above the resting pitch the note starts, as a multiple of it: the
/// oscillator opens at 220 Hz and sweeps down to 55. The 808's equivalent
/// starts at 2.6 times its resting pitch and is gone inside 6 ms; this one is
/// four times and lasts long enough to hear as a sweep rather than a click.
const BD_SWEEP: f64 = 3.0;
/// What TUNE moves: the pitch envelope's time constant, 12 ms to 150 ms.
const BD_SWEEP_TAU: [f64; 2] = [0.012, 0.150];
/// What DECAY moves. The 808's equivalent knob stops at 800 ms and this one
/// reaches a second and a half, which is the difference the two machines'
/// kicks are known by: a 909's decay genuinely extends the ring.
///
/// These are the envelope's own end points; what comes out measures 100 ms
/// and 1.5 s to −20 dB, because the overdrive across the drum holds the front
/// of the note up and stretches both.
const BD_DECAY_SECONDS: [f64; 2] = [0.080, 1.200];
/// What ATTACK moves: how much of the trigger pulse reaches the output. At
/// zero the drum is the resonator alone.
const BD_CLICK_SECONDS: f64 = 0.0026;
const BD_CLICK_GAIN: f64 = 1.3;
/// Where the drum sits against the rest of the machine, after the overdrive.
/// The 909's kick is the loudest voice on its own front panel and it is left
/// a little above the other kits' here, but not by the 6 dB the soft clipper
/// hands it if nothing takes it back.
const BD_OUT: f64 = 0.7;

// ── Snare drum ──

/// The two oscillators. Both are triangle waves rounded towards sines by
/// back-to-back diodes, both take the TUNE knob, and both are struck by a
/// start-of-note pitch pulse that is the same for every hit whatever the
/// accent bus is doing.
pub(crate) const SD_LOW_HZ: f64 = 185.0;
const SD_HIGH_HZ: f64 = 330.0;
const SD_PITCH_PULSE: f64 = 0.55;
const SD_PITCH_TAU: f64 = 0.006;
/// Their VCAs' envelopes, which are not the noise's.
const SD_LOW_SECONDS: f64 = 0.100;
const SD_HIGH_SECONDS: f64 = 0.062;
/// What TONE moves: the length of the snare noise, 40 ms to 400 ms.
const SD_NOISE_SECONDS: [f64; 2] = [0.040, 0.400];
/// The noise is high-passed on its way to the Snappy pot.
const SD_NOISE_HP: f64 = 1400.0;
/// The second, briefer noise component, which is accent-independent and is
/// most of the snare's front edge.
const SD_TRANSIENT_TAU: f64 = 0.004;
const SD_TRANSIENT_HP: f64 = 5200.0;

// ── Toms ──

/// Centre tunings of the three tom circuits.
const TOM_HZ: [f64; 3] = [80.0, 113.0, 160.0];
/// Their ring times at the centre detent of each DECAY knob, which the 808
/// does not have at all.
const TOM_SECONDS: [f64; 3] = [0.450, 0.380, 0.320];
/// The pitch envelope, which is deeper and four times longer than the 808's
/// — a 909 tom bends, an 808 tom is struck.
const TOM_SWEEP: f64 = 0.45;
const TOM_SWEEP_TAU: f64 = 0.045;
/// The noise layer under the tone, which the 808's toms do not have: theirs
/// is a pure sine with a rattle on the attack, this is a band of noise that
/// rings with the drum.
const TOM_NOISE_LP: f64 = 2500.0;
const TOM_NOISE_MIX: f64 = 0.22;

// ── Rimshot and clap ──

const RS_BODY_HZ: f64 = 470.0;
const RS_CRACK_HZ: f64 = 1720.0;
const RS_SECONDS: f64 = 0.028;
const CP_BP_HZ: f64 = 1050.0;
const CP_BP_Q: f64 = 1.8;
/// The clap's VCA is driven by a few cycles of a sawtooth rather than by four
/// square gates, so its bursts run into each other at the end instead of
/// stopping — that is what the density trimpot sets.
const CP_PULSE_HZ: f64 = 95.0;
const CP_PULSE_SPAN: f64 = 0.036;
const CP_TAIL_SECONDS: f64 = 0.130;
const CP_TAIL_MIX: f64 = 0.32;

// ── The sampled three ──

/// The output filter after the converter. Nine kilohertz is the ceiling the
/// 18 kHz clock puts on the data, so this is where the images start — the
/// first of them sits at 18 kHz minus the highest thing in the sample, which
/// is why a zero-order hold needs a filter with more than two poles behind it
/// and why this one is three.
const RECON_HZ: f64 = 8600.0;
/// Ring times of the two envelope generators the sampled voices have of their
/// own. The hats' come from the panel.
const CRASH_SECONDS: f64 = 1.600;
const RIDE_SECONDS: f64 = 1.100;
const CH_SECONDS: f64 = 0.045;
const OH_SECONDS: f64 = 0.380;

/// How much longer the drum is heard for than its envelope generator runs.
///
/// The bass drum is soft-clipped across the whole voice, so the front of the
/// note is held up against the knee and the tail comes out of it later than
/// the exponential alone would put it. Measured against the rendered −20 dB
/// time at both ends of the knob; the panel prints what the machine does, not
/// what the capacitor does.
const BD_DECAY_STRETCH: f64 = 1.25;

/// What this machine's decay knobs render, in seconds to −20 dB. See
/// [`param_seconds`].
pub(crate) fn decay_seconds(index: usize, knob: f64) -> Option<f64> {
    Some(match index {
        P_BD_DECAY => {
            geometric(BD_DECAY_SECONDS[0], BD_DECAY_SECONDS[1], knob) * BD_DECAY_STRETCH
        }
        P_LT_DECAY => TOM_SECONDS[0] * decay_scale(knob),
        P_MT_DECAY => TOM_SECONDS[1] * decay_scale(knob),
        P_HT_DECAY => TOM_SECONDS[2] * decay_scale(knob),
        // No cymbal decay control on this machine — the crash is a sample and
        // its envelope generator is fixed — so this is that fixed time.
        P_CY_DECAY => CRASH_SECONDS,
        P_OH_DECAY => OH_SECONDS * decay_scale(knob),
        P_CH_DECAY => CH_SECONDS * decay_scale(knob),
        _ => return None,
    })
}

/// Which sampled voice a note reads, and how it is tuned.
///
/// The two hats read the *same* ROM — one recording, two envelope generators
/// — which is why the closed hat cuts the open one off and why they sound
/// like one instrument played two ways. The ride and the crash have their own
/// data and their own TUNE knob, and tuning a sampled voice means clocking it
/// faster — so TUNE moves the pitch and the brightness of the crash and the
/// ride, and does not move their length, because their length is an analog
/// envelope on the far side of the converter and it does not know how fast
/// the ROM is being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sampled909 {
    Hat,
    Ride,
    Crash,
}

impl DrumVoice {
    // 909 synthesis
    // ══════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_909(&mut self, sr: f64, c: &Controls) -> f64 {
        match self.sound {
            DrumSound::Kick | DrumSound::SubKick(_) => self.synth_909_kick(sr, c),
            DrumSound::Snare => self.synth_909_snare(sr, c, SD_LOW_HZ, SD_HIGH_HZ),
            // The second snare pad is the same circuit struck harder and
            // tuned up, which is what the 909's two snare sounds are.
            DrumSound::SnareAlt => {
                self.synth_909_snare(sr, c, SD_LOW_HZ * 1.15, SD_HIGH_HZ * 1.15)
            }
            DrumSound::LowTom => self.synth_909_tom(sr, c, 0),
            DrumSound::MidTom => self.synth_909_tom(sr, c, 1),
            DrumSound::HighTom => self.synth_909_tom(sr, c, 2),
            // Congas, bongos and timbales are played on the toms, which is
            // what a machine with no congas leaves you. They sort by pitch on
            // the same boundaries the strips do.
            DrumSound::Conga(f) | DrumSound::Bongo(f) | DrumSound::Timbale(f) => {
                let which = if f < 260.0 {
                    0
                } else if f < 360.0 {
                    1
                } else {
                    2
                };
                self.synth_909_tom(sr, c, which)
            }
            // No cowbell circuit on this machine: the rimshot is the only
            // pitched click it has, so that is where those parts go.
            DrumSound::Rimshot | DrumSound::Clave | DrumSound::Cowbell | DrumSound::Agogo(_) => {
                self.synth_909_rimshot(sr)
            }
            // Likewise no shaker of any kind, and the clap is the machine's
            // one noise burst.
            DrumSound::Clap
            | DrumSound::Maracas
            | DrumSound::Cabasa
            | DrumSound::Tambourine
            | DrumSound::Vibraslap
            | DrumSound::Guiro(_)
            | DrumSound::Whistle(_)
            | DrumSound::FxNoise(_) => self.synth_909_clap(sr),
            DrumSound::ClosedHat | DrumSound::PedalHat => {
                let tau = CH_SECONDS / DECAY_REFERENCE * c.decay_mult;
                self.synth_909_sampled(sr, Sampled909::Hat, 1.0, tau)
            }
            DrumSound::OpenHat => {
                let tau = OH_SECONDS / DECAY_REFERENCE * c.decay_mult;
                self.synth_909_sampled(sr, Sampled909::Hat, 1.0, tau)
            }
            DrumSound::Crash | DrumSound::Splash | DrumSound::Cymbal => {
                let tau = CRASH_SECONDS / DECAY_REFERENCE;
                self.synth_909_sampled(sr, Sampled909::Crash, c.tune_ratio, tau)
            }
            DrumSound::Ride | DrumSound::RideBell => {
                let tau = RIDE_SECONDS / DECAY_REFERENCE;
                self.synth_909_sampled(sr, Sampled909::Ride, c.tune_ratio, tau)
            }
        }
    }

    /// A triangle rounded towards a sine, which is what the back-to-back
    /// diodes across the 909's oscillators do to them. Not a sine: the corners
    /// survive, and they are why this machine's kick and toms cut through
    /// where the 808's sit under everything.
    fn rounded_triangle(phase: f64) -> f64 {
        osc_sine(phase) * 0.78 + osc_triangle(phase) * 0.22
    }

    /// 909 Bass drum.
    ///
    /// TUNE is the length of the pitch sweep, ATTACK is how much of the
    /// trigger pulse gets out, DECAY is the ring. All three are on the front
    /// panel of this machine and none of them is on the 808's.
    fn synth_909_kick(&mut self, sr: f64, c: &Controls) -> f64 {
        let base = match self.sound {
            DrumSound::SubKick(mult) => BD_HZ * mult,
            _ => BD_HZ,
        };
        let sweep_tau = geometric(BD_SWEEP_TAU[0], BD_SWEEP_TAU[1], c.tune);
        let freq = base * (1.0 + BD_SWEEP * (-self.time / sweep_tau).exp());
        advance_phase(&mut self.phase1, freq, sr);

        let tau = geometric(BD_DECAY_SECONDS[0], BD_DECAY_SECONDS[1], c.decay) / DECAY_REFERENCE;
        let env = (-self.time / (tau * self.accent_stretch())).exp();
        if env < 0.0005 {
            self.active = false;
            return 0.0;
        }

        // The attack is the trigger pulse and a scrape of noise with it, both
        // gone in under two milliseconds.
        let click = if self.time < BD_CLICK_SECONDS {
            let shape = 1.0 - self.time / BD_CLICK_SECONDS;
            (0.35 + 0.65 * self.digital_noise()) * shape * BD_CLICK_GAIN * c.attack
        } else {
            0.0
        };

        let body = Self::rounded_triangle(self.phase1) * env;
        // A 909 kick is always a little overdriven; the DRIVE knob is on top
        // of that rather than instead of it.
        soft_clip(body + click, 0.35 + c.drive * 3.0) * BD_OUT
    }

    /// 909 Snare: two rounded triangles with their own envelopes, a brief
    /// bright transient, and the long noise the TONE knob sets the length of.
    fn synth_909_snare(&mut self, sr: f64, c: &Controls, low_hz: f64, high_hz: f64) -> f64 {
        // The start-of-note pitch pulse is common to both oscillators and is
        // the same for every note, accented or not.
        let bend = 1.0 + SD_PITCH_PULSE * (-self.time / SD_PITCH_TAU).exp();
        advance_phase(&mut self.phase1, low_hz * c.tune_ratio * bend, sr);
        advance_phase(&mut self.phase2, high_hz * c.tune_ratio * bend, sr);

        let stretch = self.accent_stretch();
        let low_env = (-self.time / (SD_LOW_SECONDS / DECAY_REFERENCE * stretch)).exp();
        let high_env = (-self.time / (SD_HIGH_SECONDS / DECAY_REFERENCE * stretch)).exp();
        let noise_tau =
            geometric(SD_NOISE_SECONDS[0], SD_NOISE_SECONDS[1], c.tone) / DECAY_REFERENCE;
        let noise_env = (-self.time / (noise_tau * stretch)).exp();
        if low_env < 0.001 && noise_env < 0.001 {
            self.active = false;
            return 0.0;
        }

        let drum = Self::rounded_triangle(self.phase1) * low_env * 0.6
            + Self::rounded_triangle(self.phase2) * high_env * 0.4;

        let raw = self.digital_noise();
        let snares = self.hp1.tick_hp(raw, SD_NOISE_HP, sr) * noise_env;
        let transient =
            self.hp2.tick_hp(raw, SD_TRANSIENT_HP, sr) * (-self.time / SD_TRANSIENT_TAU).exp();

        // SNAPPY is a level control on the noise alone: the oscillators reach
        // the output at the same level whatever it says.
        drum * 0.55 + (snares * (0.15 + 1.1 * c.snappy) + transient * 0.5) * 0.6
    }

    /// 909 Tom: a rounded triangle with a long pitch envelope and a band of
    /// noise ringing under it, with TUNE and DECAY on each of the three.
    fn synth_909_tom(&mut self, sr: f64, c: &Controls, which: usize) -> f64 {
        let base = TOM_HZ[which] * c.tune_ratio;
        let freq = base * (1.0 + TOM_SWEEP * (-self.time / TOM_SWEEP_TAU).exp());
        advance_phase(&mut self.phase1, freq, sr);

        let tau = TOM_SECONDS[which] / DECAY_REFERENCE * c.decay_mult;
        let env = (-self.time / (tau * self.accent_stretch())).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }

        let skin = self.lp1.tick_lp(self.digital_noise(), TOM_NOISE_LP, sr);
        (Self::rounded_triangle(self.phase1) + skin * TOM_NOISE_MIX) * env
    }

    /// 909 Rimshot: a short pitched knock with a crack across the top of it.
    fn synth_909_rimshot(&mut self, sr: f64) -> f64 {
        let tau = RS_SECONDS / DECAY_REFERENCE * self.accent_stretch();
        let env = (-self.time / tau).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }

        advance_phase(&mut self.phase1, RS_BODY_HZ, sr);
        let crack = self.svf1.bandpass(self.digital_noise(), RS_CRACK_HZ, 1.6, sr);
        let out = (osc_sine(self.phase1) * 0.55 + crack * 0.8) * env;
        self.hp1.tick_hp(out, 320.0, sr)
    }

    /// 909 Hand clap: noise through a band-pass, gated by a few cycles of a
    /// sawtooth, over the pseudo-reverb the 808 has as well.
    fn synth_909_clap(&mut self, sr: f64) -> f64 {
        let filtered = self.svf1.bandpass(self.digital_noise(), CP_BP_HZ, CP_BP_Q, sr);

        let pulses = if self.time < CP_PULSE_SPAN {
            // Sawtooth, not a gate: each burst arrives hard and slopes away,
            // and the whole train fades over the span.
            let within = (self.time * CP_PULSE_HZ).fract();
            (1.0 - within) * (1.0 - self.time / CP_PULSE_SPAN)
        } else {
            0.0
        };

        let tail_tau = CP_TAIL_SECONDS / DECAY_REFERENCE * self.accent_stretch();
        let tail = (-self.time / tail_tau).exp();
        if self.time > CP_PULSE_SPAN && tail < 0.001 {
            self.active = false;
            return 0.0;
        }
        filtered * (pulses + tail * CP_TAIL_MIX)
    }

    /// The three sampled voices: 6-bit words at 18 kHz, held, filtered, and
    /// shaped by an analog envelope generator on the far side of the
    /// converter.
    ///
    /// `rate_ratio` is the TUNE knob, which on a sampled voice is the clock:
    /// the crash and the ride get shorter as they get higher, because that is
    /// what reading the same words faster does.
    fn synth_909_sampled(
        &mut self,
        sr: f64,
        voice: Sampled909,
        rate_ratio: f64,
        tau: f64,
    ) -> f64 {
        if self.convert(sr, PCM_909_RATE * rate_ratio) {
            let word = self.rom_909(voice);
            self.dac_hold = Self::quantize(word, PCM_909_BITS);
        }

        let env = (-self.time / (tau * self.accent_stretch())).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        let smoothed = self.svf3.lowpass(self.dac_hold, RECON_HZ, 0.707, sr);
        self.lp1.tick_lp(smoothed, RECON_HZ, sr) * env
    }

    /// One 6-bit word.
    ///
    /// Everything in here runs at the recording's own clock, so nothing in it
    /// can be brighter than 9 kHz — the ceiling that makes a 909 hi-hat the
    /// short, dark, dense thing it is rather than the airy one an analog
    /// six-oscillator hat gives you.
    fn rom_909(&mut self, voice: Sampled909) -> f64 {
        const RATE: f64 = PCM_909_RATE;
        let tag = voice as u64 + 1;
        // Position in the recording rather than wall-clock seconds: features
        // sit at addresses, so they arrive sooner when the clock runs faster.
        let t = self.dac_address as f64 / RATE;
        let n = self.rom_noise(tag);
        match voice {
            // One recording for both hats. Dense, mid-heavy, and with the
            // stick's edge in the first few milliseconds.
            Sampled909::Hat => {
                let body = self.svf1.bandpass(n, 5600.0, 0.9, RATE);
                let edge = self.svf2.bandpass(n, 8200.0, 1.3, RATE);
                (body * 0.52 + edge * 0.42) * (0.85 + 0.15 * (-t / 0.004).exp())
            }
            // The ride's ping is a pair of partials over the wash, and the
            // wash is what is left after 9 kHz has been taken off the top.
            Sampled909::Ride => {
                advance_phase(&mut self.phase1, 1150.0, RATE);
                advance_phase(&mut self.phase2, 2450.0, RATE);
                let ping = (osc_sine(self.phase1) * 0.5 + osc_sine(self.phase2) * 0.3)
                    * (0.3 + 0.7 * (-t / 0.060).exp());
                let wash = self.svf1.bandpass(n, 5400.0, 0.6, RATE);
                ping * 0.6 + wash * 0.55
            }
            // The crash is the same source with no ping and everything
            // spread: the widest band the converter can hold.
            Sampled909::Crash => {
                let low = self.svf1.bandpass(n, 3600.0, 0.5, RATE);
                let high = self.svf2.bandpass(n, 7400.0, 0.7, RATE);
                (low * 0.7 + high * 0.7) * (0.8 + 0.2 * (-t / 0.010).exp())
            }
        }
    }
}
