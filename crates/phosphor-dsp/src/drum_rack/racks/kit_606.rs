//! 606 kit synthesis.

use super::super::*;

impl DrumVoice {
    // 606 synthesis — thinner, clickier, higher
    // ══════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_606(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, drive_amt: f64) -> f64 {
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
    pub(crate) fn synth_606_kick(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, drive_amt: f64) -> f64 {
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
    pub(crate) fn synth_606_snare(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
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
    pub(crate) fn synth_606_snare_alt(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
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
    pub(crate) fn synth_606_closed_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
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
    pub(crate) fn synth_606_open_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
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
