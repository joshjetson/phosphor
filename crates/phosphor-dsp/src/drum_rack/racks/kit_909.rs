//! 909 kit synthesis.

use super::super::*;

impl DrumVoice {
    // 909 synthesis
    // ══════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_909(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, drive_amt: f64) -> f64 {
        match self.sound {
            DrumSound::Kick | DrumSound::SubKick(_) => self.synth_909_kick(sr, decay_mod, tone_mod, drive_amt),
            DrumSound::Snare => self.synth_909_snare(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::SnareAlt => self.synth_909_snare_alt(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::Clap => self.synth_909_clap(sr, decay_mod, noise_mod),
            DrumSound::ClosedHat | DrumSound::PedalHat => self.synth_909_closed_hat(sr, decay_mod, tone_mod),
            DrumSound::OpenHat => self.synth_909_open_hat(sr, decay_mod, tone_mod),
            DrumSound::Rimshot => self.synth_808_rimshot(sr, decay_mod, tone_mod), // similar
            DrumSound::Cowbell => self.synth_808_cowbell(sr, decay_mod, tone_mod),
            DrumSound::Clave => self.synth_808_clave(sr, decay_mod),
            DrumSound::Maracas | DrumSound::Cabasa => self.synth_808_maracas(sr, decay_mod),
            DrumSound::LowTom => self.synth_808_tom(sr, decay_mod, tone_mod, 110.0),
            DrumSound::MidTom => self.synth_808_tom(sr, decay_mod, tone_mod, 170.0),
            DrumSound::HighTom => self.synth_808_tom(sr, decay_mod, tone_mod, 230.0),
            DrumSound::Crash | DrumSound::Splash => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.9),
            DrumSound::Cymbal => self.synth_808_cymbal(sr, decay_mod, tone_mod, 1.0),
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

    /// 909 Kick: 55Hz sine, longer pitch sweep (80-200ms from 150Hz), noise click.
    pub(crate) fn synth_909_kick(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, drive_amt: f64) -> f64 {
        let base_freq = match self.sound {
            DrumSound::SubKick(mult) => 55.0 * mult,
            _ => 55.0,
        };
        let freq = base_freq * tone_mod;
        // Longer pitch sweep: from ~150Hz down over ~120ms (much longer than 808's 6ms)
        let sweep_start = 150.0 * tone_mod;
        let sweep = (sweep_start - freq) * (-self.time * 12.0).exp();
        let current_freq = freq + sweep;
        advance_phase(&mut self.phase1, current_freq, sr);
        let body = osc_sine(self.phase1);

        let decay = 0.22 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }

        // Noise click: white noise through LPF at 5kHz, 5-10ms decay, mixed at 25%
        let noise_click_env = (-self.time / 0.007).exp();
        let raw_noise = self.noise();
        let filtered_noise = self.lp1.tick_lp(raw_noise, 5000.0, sr);
        let click = filtered_noise * noise_click_env * 0.25;

        let out = (body * 0.75 + click) * env;
        if drive_amt > 0.01 {
            soft_clip(out, drive_amt * 3.0 + 1.0)
        } else {
            soft_clip(out, 1.0)  // 909 always has slight overdrive
        }
    }

    /// 909 Snare: Two TRIANGLE oscillators (180Hz, 330Hz) + bandpassed noise.
    pub(crate) fn synth_909_snare(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 180.0 * tone_mod;
        let f2 = 330.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let tonal_decay = 0.14 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        // Triangle oscillators for crisper harmonics
        let tonal = (osc_triangle(self.phase1) * 0.55 + osc_triangle(self.phase2) * 0.45) * tonal_env;

        let noise_decay = 0.16 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        // Noise through LPF 8kHz -> HPF 1.5kHz
        let lp_noise = self.lp1.tick_lp(raw_noise, 8000.0, sr);
        let bp_noise = self.hp1.tick_hp(lp_noise, 1500.0, sr);
        let noise_out = bp_noise * noise_env * noise_mod;

        let snappy = 0.55;
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 909 Snare Alt: Slightly different tuning.
    pub(crate) fn synth_909_snare_alt(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 200.0 * tone_mod;
        let f2 = 350.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let tonal_decay = 0.12 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        let tonal = (osc_triangle(self.phase1) * 0.5 + osc_triangle(self.phase2) * 0.5) * tonal_env;

        let noise_decay = 0.18 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        let lp_noise = self.lp1.tick_lp(raw_noise, 8000.0, sr);
        let bp_noise = self.hp1.tick_hp(lp_noise, 2000.0, sr);
        let noise_out = bp_noise * noise_env * noise_mod;

        let snappy = 0.6;
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 909 Clap: 4 noise bursts spaced 11ms apart through BPF 1.2kHz, slight jitter.
    pub(crate) fn synth_909_clap(&mut self, sr: f64, decay_mod: f64, noise_mod: f64) -> f64 {
        let raw_noise = self.noise() * noise_mod;
        let filtered = self.svf1.bandpass(raw_noise, 1200.0, 2.5, sr);

        // 4 bursts with 11ms spacing and slight jitter
        let spacings = [0.0, 0.011, 0.023, 0.034]; // slight jitter
        let mut burst_env = 0.0;
        for &burst_start in &spacings {
            let t_in_burst = self.time - burst_start;
            if t_in_burst >= 0.0 && t_in_burst < 0.012 {
                let attack = if t_in_burst < 0.003 {
                    t_in_burst / 0.003
                } else {
                    (-((t_in_burst - 0.003) / 0.006)).exp()
                };
                burst_env += attack;
            }
        }

        let tail_start = 0.045;
        let tail_env = if self.time > tail_start {
            (-(self.time - tail_start) / (0.18 * decay_mod)).exp()
        } else {
            0.0
        };

        let env = burst_env + tail_env * 0.65;
        if self.time > tail_start + 0.18 * decay_mod * 7.0 {
            self.active = false;
        }
        filtered * env
    }

    /// 909 Closed Hat: 6 oscillators + bit-crushing for gritty digital character.
    pub(crate) fn synth_909_closed_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        let bp1 = self.svf1.bandpass(raw, 3440.0, 3.0, sr);
        let bp2 = self.svf2.bandpass(raw, 7100.0, 3.0, sr);
        let filtered = bp1 * 0.4 + bp2 * 0.6; // more emphasis on 8-10kHz

        let hpf = self.hp1.tick_hp(filtered, 6000.0, sr);

        // Bit-crush: reduce to ~8-bit resolution for gritty 909 character
        let crushed = (hpf * 128.0).round() / 128.0;

        let decay = 0.04 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        crushed * env
    }

    /// 909 Open Hat: Same with bit-crushing, longer decay.
    pub(crate) fn synth_909_open_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        let bp1 = self.svf1.bandpass(raw, 3440.0, 3.0, sr);
        let bp2 = self.svf2.bandpass(raw, 7100.0, 3.0, sr);
        let filtered = bp1 * 0.4 + bp2 * 0.6;

        let hpf = self.hp1.tick_hp(filtered, 6000.0, sr);
        let crushed = (hpf * 128.0).round() / 128.0;

        let decay = 0.30 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        crushed * env
    }

    // ══════════════════════════════════════════════════════════════════════
}
