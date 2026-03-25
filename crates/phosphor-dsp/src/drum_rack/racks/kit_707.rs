//! 707 kit synthesis.

use super::super::*;

impl DrumVoice {
    // 707 synthesis — halfway between 808 and 909 character
    // ══════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_707(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, drive_amt: f64) -> f64 {
        match self.sound {
            DrumSound::Kick | DrumSound::SubKick(_) => self.synth_707_kick(sr, decay_mod, tone_mod, drive_amt),
            DrumSound::Snare => self.synth_707_snare(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::SnareAlt => self.synth_707_snare_alt(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::Clap => self.synth_707_clap(sr, decay_mod, noise_mod),
            DrumSound::ClosedHat | DrumSound::PedalHat => self.synth_707_closed_hat(sr, decay_mod, tone_mod),
            DrumSound::OpenHat => self.synth_707_open_hat(sr, decay_mod, tone_mod),
            DrumSound::Rimshot => self.synth_808_rimshot(sr, decay_mod, tone_mod),
            DrumSound::Cowbell => self.synth_808_cowbell(sr, decay_mod, tone_mod),
            DrumSound::Clave => self.synth_808_clave(sr, decay_mod),
            DrumSound::Maracas | DrumSound::Cabasa => self.synth_808_maracas(sr, decay_mod),
            DrumSound::LowTom => self.synth_808_tom(sr, decay_mod, tone_mod, 108.0),
            DrumSound::MidTom => self.synth_808_tom(sr, decay_mod, tone_mod, 165.0),
            DrumSound::HighTom => self.synth_808_tom(sr, decay_mod, tone_mod, 225.0),
            DrumSound::Crash | DrumSound::Splash => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.85),
            DrumSound::Cymbal => self.synth_808_cymbal(sr, decay_mod, tone_mod, 1.0),
            DrumSound::Ride | DrumSound::RideBell => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.55),
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

    /// 707 Kick: Between 808 and 909 — sine at 48Hz, medium sweep.
    pub(crate) fn synth_707_kick(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, drive_amt: f64) -> f64 {
        let base_freq = match self.sound {
            DrumSound::SubKick(mult) => 48.0 * mult,
            _ => 48.0,
        };
        let freq = base_freq * tone_mod;
        // Medium sweep: between 808's fast and 909's slow
        let sweep_start = 120.0 * tone_mod;
        let sweep = (sweep_start - freq) * (-self.time * 40.0).exp();
        advance_phase(&mut self.phase1, freq + sweep, sr);
        let body = osc_sine(self.phase1);

        let decay = 0.28 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }

        let click = (-self.time * 600.0).exp() * 0.2;
        let out = body * env + self.noise() * click * 0.1;
        if drive_amt > 0.01 {
            soft_clip(out, drive_amt * 2.5)
        } else {
            soft_clip(out, 0.3)
        }
    }

    /// 707 Snare: Hybrid sine/triangle with noise.
    pub(crate) fn synth_707_snare(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 190.0 * tone_mod;
        let f2 = 380.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let tonal_decay = 0.16 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        let tonal = (osc_sine(self.phase1) * 0.4 + osc_triangle(self.phase2) * 0.6) * tonal_env;

        let noise_decay = 0.14 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        let filtered = self.hp1.tick_hp(raw_noise, 1800.0, sr);
        let noise_out = filtered * noise_env * noise_mod;

        let snappy = 0.52;
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 707 Snare Alt.
    pub(crate) fn synth_707_snare_alt(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 205.0 * tone_mod;
        let f2 = 395.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let tonal_decay = 0.13 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        let tonal = (osc_sine(self.phase1) * 0.45 + osc_triangle(self.phase2) * 0.55) * tonal_env;

        let noise_decay = 0.15 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        let filtered = self.hp1.tick_hp(raw_noise, 2000.0, sr);
        let noise_out = filtered * noise_env * noise_mod;

        let snappy = 0.58;
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 707 Clap: Between 808 and 909.
    pub(crate) fn synth_707_clap(&mut self, sr: f64, decay_mod: f64, noise_mod: f64) -> f64 {
        let raw_noise = self.noise() * noise_mod;
        let filtered = self.svf1.bandpass(raw_noise, 1100.0, 2.2, sr);

        let spacings = [0.0, 0.0105, 0.0215, 0.033];
        let mut burst_env = 0.0;
        for &burst_start in &spacings {
            let t_in_burst = self.time - burst_start;
            if t_in_burst >= 0.0 && t_in_burst < 0.011 {
                let attack = if t_in_burst < 0.003 {
                    t_in_burst / 0.003
                } else {
                    (-((t_in_burst - 0.003) / 0.0055)).exp()
                };
                burst_env += attack;
            }
        }

        let tail_start = 0.043;
        let tail_env = if self.time > tail_start {
            (-(self.time - tail_start) / (0.16 * decay_mod)).exp()
        } else {
            0.0
        };

        let env = burst_env + tail_env * 0.68;
        if self.time > tail_start + 0.16 * decay_mod * 7.0 {
            self.active = false;
        }
        filtered * env
    }

    /// 707 Closed Hat: Tighter than 808, less aggressive than 909.
    pub(crate) fn synth_707_closed_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        let bp1 = self.svf1.bandpass(raw, 3440.0, 3.0, sr);
        let bp2 = self.svf2.bandpass(raw, 7100.0, 3.0, sr);
        let filtered = bp1 * 0.45 + bp2 * 0.55;

        let hpf = self.hp1.tick_hp(filtered, 6000.0, sr);

        let decay = 0.045 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        hpf * env
    }

    /// 707 Open Hat.
    pub(crate) fn synth_707_open_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        let bp1 = self.svf1.bandpass(raw, 3440.0, 3.0, sr);
        let bp2 = self.svf2.bandpass(raw, 7100.0, 3.0, sr);
        let filtered = bp1 * 0.45 + bp2 * 0.55;

        let hpf = self.hp1.tick_hp(filtered, 6000.0, sr);

        let decay = 0.25 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        hpf * env
    }

    // ══════════════════════════════════════════════════════════════════════
}
