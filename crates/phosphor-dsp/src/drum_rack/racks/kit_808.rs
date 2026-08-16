//! 808 kit synthesis.
//!
//! Voice by voice, this is the TR-808's own architecture: bridged-T
//! resonators for the drums, a bank of six free-running square oscillators for
//! everything metal, and a noise generator gated four times over for the clap.
//! The numbers in the constants below are the published ones — the service
//! notes, and the circuit analyses that work the component values back into
//! frequencies — rather than values picked to sound about right.
//!
//! Decay figures for this machine are quoted as times rather than as time
//! constants, and the one that can be cross-checked (the bass drum's "50 to
//! 800 ms, 300 ms at centre") lines up when read as the −20 dB point, so
//! `DECAY_REFERENCE` in the parent module is how they are converted.

use super::super::*;
use std::f64::consts::TAU;

// ── Bass drum ──

/// Bridged-T resonance. The service notes label the filter 56 Hz; working the
/// component values in the same notes through the transfer function gives
/// 49.4 Hz, and real machines measure anywhere from 48 Hz up. The computed
/// value is the one used here.
const BD_FREQ: f64 = 49.4;
/// The resonator starts its ring high and settles within a few milliseconds:
/// about 130 Hz at the first cycle. Too brief to hear as a pitch, which is
/// why it reads as snap rather than as a sweep.
const BD_ATTACK_HZ: f64 = 130.0;
const BD_SWEEP_TAU: f64 = 0.006;
/// The trigger pulse, as it reaches the output through the tone control.
const BD_CLICK_SECONDS: f64 = 0.0003;
const BD_CLICK_GAIN: f64 = 0.5;
/// The tone control is a passive low-pass on the way out. It barely touches a
/// 50 Hz ring; what it does is round the click off.
const BD_TONE_HZ: [f64; 2] = [320.0, 5200.0];

// ── Snare drum ──

/// The snare's two bridged-T oscillators, from page 14 of the service notes:
/// 476 Hz and 238 Hz, an octave apart.
const SD_LOW_HZ: f64 = 238.0;
const SD_HIGH_HZ: f64 = 476.0;
/// Body decays. Not published; sized so the whole drum runs about 250 ms,
/// which is where a recording of the instrument puts it.
const SD_LOW_TAU: f64 = 0.055;
const SD_HIGH_TAU: f64 = 0.040;
const SD_NOISE_TAU: f64 = 0.070;
/// The noise path is high-passed in the circuit. The low-pass is not in the
/// schematic but the buffer that follows it behaves like one, and without
/// something up there the noise is far brighter than the instrument.
const SD_NOISE_HP: f64 = 1200.0;
const SD_NOISE_LP: f64 = 7500.0;

// ── Metal section ──

/// The two band-pass filters the six oscillators are split by, shared between
/// the cymbal and both hats.
const METAL_BP_LOW: f64 = 3440.0;
const METAL_BP_HIGH: f64 = 7100.0;
/// Each of the three metal VCAs runs into its own Sallen-Key high-pass. The
/// corners are not published; these are set so the oscillators' own 205-800 Hz
/// fundamentals stay out of the output, which is what that filter is for.
const HAT_HP: f64 = 6000.0;
const CYMBAL_HP: f64 = 1200.0;
/// The cymbal's DECAY knob is on the 3440 Hz path only. The 7100 Hz path,
/// which is the strike rather than the body, is fixed and short.
const CY_HIGH_TAU: f64 = 0.250 / DECAY_REFERENCE;

// ── Cowbell ──

/// The cowbell takes two of the six oscillators — the 540 Hz and 800 Hz pair,
/// the two with trimpots — into an asymmetrical band-pass that cuts below its
/// centre harder than above it, so the squares keep their harmonics.
const CB_BP_HZ: f64 = 1000.0;
const CB_BP_Q: f64 = 1.0;
const CB_HP_HZ: f64 = 350.0;
/// Two-stage envelope: a fast transient snap over a longer clanging tail.
const CB_FAST_TAU: f64 = 0.012;
const CB_SLOW_TAU: f64 = 0.105;
const CB_FAST_MIX: f64 = 0.55;

// ── Hand clap ──

/// Noise through a band-pass at 1 kHz, gated four times in rapid succession by
/// a 100 Hz retrigger oscillator, with a second VCA on the same noise carrying
/// a long "reverb" envelope for the tail.
const CP_BP_HZ: f64 = 1000.0;
const CP_BP_Q: f64 = 2.0;
const CP_GATE_COUNT: usize = 4;
const CP_GATE_SPACING: f64 = 0.010;
const CP_GATE_ATTACK: f64 = 0.0004;
const CP_GATE_TAU: f64 = 0.0035;
const CP_TAIL_TAU: f64 = 0.080;
const CP_TAIL_MIX: f64 = 0.30;

// ── Toms and congas ──

/// Centre tunings of the three tom boards: the same circuits as the congas,
/// switched an octave down, which lands them inside the 80-100 Hz and
/// 165-220 Hz ranges quoted for the low and high toms. The congas keep the
/// tunings the note map gives them, which are within a semitone or two of the
/// published centre detents of 185, 280 and 400 Hz.
const TOM_HZ: [f64; 3] = [92.0, 140.0, 200.0];
/// The toms carry a little filtered noise for a shell rattle; the congas are
/// the same circuit with that part left out.
const TOM_NOISE: f64 = 0.12;
/// The bridged-T is struck hard enough to start above its resting pitch.
const DRUM_SWEEP: f64 = 0.20;
const DRUM_SWEEP_TAU: f64 = 0.012;

impl DrumVoice {
    // 808 synthesis
    // ══════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_808(&mut self, sr: f64, c: &Controls, metal: &MetalBank) -> f64 {
        match self.sound {
            DrumSound::Kick | DrumSound::SubKick(_) => self.synth_808_kick(sr, c),
            DrumSound::Snare => self.synth_808_snare(sr, c),
            DrumSound::SnareAlt => self.synth_808_snare_alt(sr, c),
            DrumSound::Clap => self.synth_808_hand_clap(sr),
            DrumSound::ClosedHat | DrumSound::PedalHat | DrumSound::OpenHat => {
                self.synth_808_hat(sr, c, metal)
            }
            DrumSound::Rimshot => self.synth_808_rimshot(sr, c.decay_mult, c.tune_ratio),
            DrumSound::Cowbell => self.synth_808_bell(sr, metal),
            DrumSound::Clave => self.synth_808_clave(sr, c.decay_mult),
            DrumSound::Maracas | DrumSound::Cabasa => self.synth_808_maracas(sr, c.decay_mult),
            DrumSound::LowTom => self.synth_808_drum(sr, c, TOM_HZ[0], TOM_TAU[0], TOM_NOISE),
            DrumSound::MidTom => self.synth_808_drum(sr, c, TOM_HZ[1], TOM_TAU[1], TOM_NOISE),
            DrumSound::HighTom => self.synth_808_drum(sr, c, TOM_HZ[2], TOM_TAU[2], TOM_NOISE),
            // The 808 has one cymbal, and this rack's note map has five. They
            // are the same circuit at different ring times, which is what a
            // player reaching for a ride on an 808 is doing anyway.
            DrumSound::Crash | DrumSound::Splash => self.synth_808_cymbal_circuit(sr, c, metal, 0.8),
            DrumSound::Cymbal => self.synth_808_cymbal_circuit(sr, c, metal, 1.0),
            DrumSound::Ride | DrumSound::RideBell => self.synth_808_cymbal_circuit(sr, c, metal, 0.6),
            DrumSound::Tambourine => self.synth_808_tambourine(sr, c.decay_mult),
            DrumSound::Vibraslap => self.synth_808_vibraslap(sr, c.decay_mult),
            // Bongos, congas and timbales are the tom board at other tunings.
            DrumSound::Bongo(freq) => self.synth_808_drum(sr, c, freq, conga_tau(freq), 0.0),
            DrumSound::Conga(freq) => self.synth_808_drum(sr, c, freq, conga_tau(freq), 0.0),
            DrumSound::Timbale(freq) => self.synth_808_timbale(sr, c.decay_mult, c.tune_ratio, freq),
            DrumSound::Agogo(freq) => self.synth_808_agogo(sr, c.decay_mult, c.tune_ratio, freq),
            DrumSound::Guiro(dec) => self.synth_808_guiro(sr, c.decay_mult, dec),
            DrumSound::Whistle(dec) => self.synth_808_whistle(sr, c.decay_mult, dec),
            DrumSound::FxNoise(v) => self.synth_808_fx(sr, c.decay_mult, v),
        }
    }

    /// 808 Bass drum: a bridged-T ringing at 49.4 Hz whose DECAY knob varies
    /// the feedback around it, so what the knob sets is the ring time.
    ///
    /// TONE is a passive low-pass on the way out. It used to be wired to the
    /// resonator's frequency here, which detuned the whole drum by ±20% —
    /// an 808's bass drum has no tuning control at all.
    pub(crate) fn synth_808_kick(&mut self, sr: f64, c: &Controls) -> f64 {
        let base_freq = match self.sound {
            DrumSound::SubKick(mult) => BD_FREQ * mult,
            _ => BD_FREQ,
        };
        // Inert on the 808 and live on the 909, which does have a tune knob.
        let freq = base_freq * c.tune_ratio;
        // The ring starts near 130 Hz and settles inside 6 ms. This used to
        // start at nine times the resting pitch — 378 Hz for a 42 Hz drum,
        // which is a synthesizer's kick, not this one's.
        let sweep = (BD_ATTACK_HZ - BD_FREQ) * (base_freq / BD_FREQ) * (-self.time / BD_SWEEP_TAU).exp();
        advance_phase(&mut self.phase1, freq + sweep, sr);
        let body = osc_sine(self.phase1);

        let tau = c.tau * self.accent_stretch();
        let env = (-self.time / tau).exp();
        if env < 0.0005 {
            self.active = false;
            return 0.0;
        }

        // The click is the trigger pulse itself, not a harmonic of the body.
        let pulse = if self.time < BD_CLICK_SECONDS { BD_CLICK_GAIN } else { 0.0 };
        let cut = BD_TONE_HZ[0] + c.tone * c.tone * (BD_TONE_HZ[1] - BD_TONE_HZ[0]);
        let out = self.lp1.tick_lp(body * env + pulse, cut, sr);

        if c.drive > 0.01 { drive_stage(out, c.drive * 2.0) } else { out }
    }

    /// 808 Snare: two bridged-T oscillators an octave apart at 238 Hz and
    /// 476 Hz, TONE across the pair, plus high-passed noise that SNAPPY sets
    /// against them.
    pub(crate) fn synth_808_snare(&mut self, sr: f64, c: &Controls) -> f64 {
        self.snare_voice(sr, c, SD_LOW_HZ, SD_HIGH_HZ, 1.0)
    }

    /// 808 Alternate snare: the same circuit struck off-centre — a little
    /// higher, a little drier, which is what note 40 is for.
    pub(crate) fn synth_808_snare_alt(&mut self, sr: f64, c: &Controls) -> f64 {
        self.snare_voice(sr, c, SD_LOW_HZ * 1.12, SD_HIGH_HZ * 1.12, 0.8)
    }

    fn snare_voice(&mut self, sr: f64, c: &Controls, lo_hz: f64, hi_hz: f64, body: f64) -> f64 {
        let tune = c.tune_ratio;
        advance_phase(&mut self.phase1, lo_hz * tune, sr);
        advance_phase(&mut self.phase2, hi_hz * tune, sr);

        // TONE is a divider across the two oscillators' outputs. The pair sums
        // to a constant, so the knob moves the balance without moving the
        // level. It used to scale both frequencies instead, which tuned the
        // drum rather than voicing it.
        let hi_mix = 0.3 + c.tone * 0.7;
        let lo_mix = 1.3 - hi_mix;
        let stretch = self.accent_stretch();
        let lo_env = (-self.time / (SD_LOW_TAU * stretch)).exp();
        let hi_env = (-self.time / (SD_HIGH_TAU * stretch)).exp();
        let tonal =
            (osc_sine(self.phase1) * lo_mix * lo_env + osc_sine(self.phase2) * hi_mix * hi_env) / 1.3;

        let noise_env = (-self.time / (SD_NOISE_TAU * stretch)).exp();
        let hp = self.hp1.tick_hp(self.noise(), SD_NOISE_HP, sr);
        let noise = self.lp1.tick_lp(hp, SD_NOISE_LP, sr) * noise_env;

        if lo_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        tonal * body * (1.0 - 0.45 * c.snappy) + noise * (0.35 + 1.05 * c.snappy)
    }

    /// 808 Hi-hat, open and closed: the six oscillators through both
    /// band-passes and a high-pass, with the ring time the strip gives it.
    ///
    /// One circuit and two envelope generators, which is why a closed hat cuts
    /// an open one off — see `DrumVoice::choke`. The closed side is fixed at
    /// 50 ms on this machine and the open side runs 90 ms to 600 ms; the
    /// closed-hat decay knob is the 909's, and is inert here.
    pub(crate) fn synth_808_hat(&mut self, sr: f64, c: &Controls, metal: &MetalBank) -> f64 {
        let tau = c.tau * self.accent_stretch();
        self.metal_voice(sr, metal, HAT_HP, tau, tau, 0.5)
    }

    /// 808 Cymbal: the low band carries the body and takes the DECAY knob, the
    /// high band is the strike and is fixed, and TONE mixes the two.
    pub(crate) fn synth_808_cymbal_circuit(
        &mut self,
        sr: f64,
        c: &Controls,
        metal: &MetalBank,
        size: f64,
    ) -> f64 {
        let stretch = self.accent_stretch() * size;
        let low_tau = c.tau * stretch;
        let high_tau = CY_HIGH_TAU * stretch;
        self.metal_voice(sr, metal, CYMBAL_HP, low_tau, high_tau, c.tone)
    }

    /// The shared body of the three metal voices: the oscillator hash split by
    /// the two band-passes, each with its own envelope, mixed by `tone`, then
    /// through the voice's high-pass.
    fn metal_voice(
        &mut self,
        sr: f64,
        metal: &MetalBank,
        hp_hz: f64,
        low_tau: f64,
        high_tau: f64,
        tone: f64,
    ) -> f64 {
        let raw = metal.hash();
        let low = self.svf1.bandpass(raw, METAL_BP_LOW, 3.0, sr);
        let high = self.svf2.bandpass(raw, METAL_BP_HIGH, 3.0, sr);

        let low_env = (-self.time / low_tau).exp();
        let high_env = (-self.time / high_tau).exp();
        if low_env < 0.001 && high_env < 0.001 {
            self.active = false;
            return 0.0;
        }

        let mixed = low * low_env * (1.0 - tone * 0.7) + high * high_env * (0.3 + tone * 0.7);
        // Each VCA runs into a Sallen-Key high-pass on the instrument, which
        // is what keeps the oscillators' own 205-800 Hz fundamentals out of
        // the output. The cymbal had no high-pass here at all and the hats had
        // a one-pole, which is not steep enough against six square waves:
        // measured through a 24 dB/octave filter, 15% of the cymbal's energy
        // sat below 1 kHz and it is 5% now.
        self.svf3.highpass(mixed, hp_hz, 0.707, sr)
    }

    /// 808 Cowbell: the 540 Hz and 800 Hz oscillators of the same bank, into
    /// the asymmetrical band-pass, with the circuit's two-stage envelope.
    pub(crate) fn synth_808_bell(&mut self, sr: f64, metal: &MetalBank) -> f64 {
        let raw = metal.cowbell();
        let bp = self.svf1.bandpass(raw, CB_BP_HZ, CB_BP_Q, sr);
        let out = self.hp1.tick_hp(bp, CB_HP_HZ, sr);

        let stretch = self.accent_stretch();
        let env = CB_FAST_MIX * (-self.time / (CB_FAST_TAU * stretch)).exp()
            + (1.0 - CB_FAST_MIX) * (-self.time / (CB_SLOW_TAU * stretch)).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        out * env
    }

    /// 808 Hand clap: one noise source, two VCAs. One is gated four times by
    /// the 100 Hz retrigger oscillator; the other carries the long envelope
    /// that stands in for a room.
    pub(crate) fn synth_808_hand_clap(&mut self, sr: f64) -> f64 {
        let filtered = self.svf1.bandpass(self.noise(), CP_BP_HZ, CP_BP_Q, sr);

        let gate_span = CP_GATE_SPACING * CP_GATE_COUNT as f64;
        let gates = if self.time < gate_span {
            let within = self.time % CP_GATE_SPACING;
            if within < CP_GATE_ATTACK {
                within / CP_GATE_ATTACK
            } else {
                (-(within - CP_GATE_ATTACK) / CP_GATE_TAU).exp()
            }
        } else {
            0.0
        };

        let stretch = self.accent_stretch();
        let tail = (-self.time / (CP_TAIL_TAU * stretch)).exp();
        if self.time > gate_span && tail < 0.001 {
            self.active = false;
            return 0.0;
        }
        filtered * (gates + tail * CP_TAIL_MIX)
    }

    /// 808 Tom, conga and bongo: one bridged-T with a pitch envelope, and for
    /// the toms a little noise for the shell.
    pub(crate) fn synth_808_drum(
        &mut self,
        sr: f64,
        c: &Controls,
        base_freq: f64,
        tau: f64,
        noise_amount: f64,
    ) -> f64 {
        let freq = base_freq * c.tune_ratio;
        let sweep = freq * DRUM_SWEEP * (-self.time / DRUM_SWEEP_TAU).exp();
        advance_phase(&mut self.phase1, freq + sweep, sr);

        let env = (-self.time / (tau * c.decay_mult * self.accent_stretch())).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }

        let body = osc_sine(self.phase1);
        let rattle = if noise_amount > 0.0 {
            self.hp1.tick_hp(self.noise(), 1200.0, sr) * (-self.time / 0.004).exp() * noise_amount
        } else {
            0.0
        };
        (body + rattle) * env
    }

    // ── The 808's smaller voices ─────────────────────────────────────────
    //
    // These keep the modifier-triple signature from when nine kits shared
    // them. Six more sat here — a clap, a cowbell, a tom, a cymbal, a bongo
    // and a conga — and they were what the 606, the 707 and the 909 played
    // for nineteen of their twenty-five sounds each. Nothing calls them now
    // that those three machines have their own, so they are gone; what is
    // left is called by the 808 itself, and `synth_808_guiro` and
    // `synth_808_whistle` by two of the tsty kits.

    /// 808 Rimshot: Two sines at 455 Hz and 1667 Hz, HPF, soft-clip.
    pub(crate) fn synth_808_rimshot(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
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
    pub(crate) fn synth_808_clave(&mut self, sr: f64, decay_mod: f64) -> f64 {
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
    pub(crate) fn synth_808_maracas(&mut self, sr: f64, decay_mod: f64) -> f64 {
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

    /// 808 Tambourine: noise through high BPF with rattling envelope.
    pub(crate) fn synth_808_tambourine(&mut self, sr: f64, decay_mod: f64) -> f64 {
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
    pub(crate) fn synth_808_vibraslap(&mut self, sr: f64, decay_mod: f64) -> f64 {
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

    /// 808 Timbale: Bright sine with HPF.
    pub(crate) fn synth_808_timbale(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
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
    pub(crate) fn synth_808_agogo(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
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
    pub(crate) fn synth_808_guiro(&mut self, sr: f64, decay_mod: f64, dur: f64) -> f64 {
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
    pub(crate) fn synth_808_whistle(&mut self, sr: f64, decay_mod: f64, dur: f64) -> f64 {
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
    pub(crate) fn synth_808_fx(&mut self, sr: f64, decay_mod: f64, character: f64) -> f64 {
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
}

/// Ring time of the conga board at a given tuning: the three published decay
/// times, 180 ms low, 100 ms middle and 80 ms high.
fn conga_tau(freq: f64) -> f64 {
    if freq < 260.0 {
        CONGA_TAU[0]
    } else if freq < 360.0 {
        CONGA_TAU[1]
    } else {
        CONGA_TAU[2]
    }
}
