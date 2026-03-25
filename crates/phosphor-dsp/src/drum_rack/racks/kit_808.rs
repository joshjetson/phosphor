//! 808 kit synthesis.

use super::super::*;
use std::f64::consts::TAU;

impl DrumVoice {
    // 808 synthesis
    // ══════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_808(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, drive_amt: f64) -> f64 {
        match self.sound {
            DrumSound::Kick | DrumSound::SubKick(_) => self.synth_808_kick(sr, decay_mod, tone_mod, drive_amt),
            DrumSound::Snare => self.synth_808_snare(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::SnareAlt => {
                // Alternate snare: more noise, less body
                self.synth_808_snare_alt(sr, decay_mod, tone_mod, noise_mod)
            }
            DrumSound::Clap => self.synth_808_clap(sr, decay_mod, noise_mod),
            DrumSound::ClosedHat | DrumSound::PedalHat => self.synth_808_closed_hat(sr, decay_mod, tone_mod),
            DrumSound::OpenHat => self.synth_808_open_hat(sr, decay_mod, tone_mod),
            DrumSound::Rimshot => self.synth_808_rimshot(sr, decay_mod, tone_mod),
            DrumSound::Cowbell => self.synth_808_cowbell(sr, decay_mod, tone_mod),
            DrumSound::Clave => self.synth_808_clave(sr, decay_mod),
            DrumSound::Maracas | DrumSound::Cabasa => self.synth_808_maracas(sr, decay_mod),
            DrumSound::LowTom => self.synth_808_tom(sr, decay_mod, tone_mod, 105.0),
            DrumSound::MidTom => self.synth_808_tom(sr, decay_mod, tone_mod, 160.0),
            DrumSound::HighTom => self.synth_808_tom(sr, decay_mod, tone_mod, 220.0),
            DrumSound::Crash | DrumSound::Splash => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.8),
            DrumSound::Cymbal => self.synth_808_cymbal(sr, decay_mod, tone_mod, 1.0),
            DrumSound::Ride | DrumSound::RideBell => self.synth_808_cymbal(sr, decay_mod, tone_mod, 0.6),
            DrumSound::Tambourine => self.synth_808_tambourine(sr, decay_mod),
            DrumSound::Vibraslap => self.synth_808_vibraslap(sr, decay_mod),
            DrumSound::Bongo(freq) => self.synth_808_bongo(sr, decay_mod, tone_mod, freq),
            DrumSound::Conga(freq) => self.synth_808_conga(sr, decay_mod, tone_mod, freq),
            DrumSound::Timbale(freq) => self.synth_808_timbale(sr, decay_mod, tone_mod, freq),
            DrumSound::Agogo(freq) => self.synth_808_agogo(sr, decay_mod, tone_mod, freq),
            DrumSound::Guiro(dec) => self.synth_808_guiro(sr, decay_mod, dec),
            DrumSound::Whistle(dec) => self.synth_808_whistle(sr, decay_mod, dec),
            DrumSound::FxNoise(v) => self.synth_808_fx(sr, decay_mod, v),
        }
    }

    /// 808 Kick: 42Hz sine with heavy pitch sweep from ~340Hz down over ~6ms.
    pub(crate) fn synth_808_kick(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, drive_amt: f64) -> f64 {
        let base_freq = match self.sound {
            DrumSound::SubKick(mult) => 42.0 * mult,
            _ => 42.0,
        };
        let freq = base_freq * tone_mod;
        let sweep = freq * 8.0 * (-self.time * 160.0).exp(); // fast sweep ~6ms
        let current_freq = freq + sweep;
        advance_phase(&mut self.phase1, current_freq, sr);
        let body = osc_sine(self.phase1);

        let decay = 0.40 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.0005 {
            self.active = false;
            return 0.0;
        }

        // Click transient
        let click = (-self.time * 800.0).exp() * osc_sine(self.phase1 * 3.0) * 0.3;

        let out = (body + click) * env;
        if drive_amt > 0.01 {
            soft_clip(out, drive_amt * 2.0)
        } else {
            out
        }
    }

    /// 808 Snare: Two sines (180Hz + 424Hz) + HPF noise.
    pub(crate) fn synth_808_snare(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 180.0 * tone_mod;
        let f2 = 424.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let tonal_decay = 0.20 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        let tonal = (osc_sine(self.phase1) * 0.6 + osc_sine(self.phase2) * 0.4) * tonal_env;

        let noise_decay = 0.18 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        let filtered_noise = self.hp1.tick_hp(raw_noise, 1500.0, sr);
        let noise_out = filtered_noise * noise_env * noise_mod;

        let snappy = 0.5; // balance tonal vs noise
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 808 Alternate Snare: More noise emphasis.
    pub(crate) fn synth_808_snare_alt(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        let f1 = 200.0 * tone_mod;
        let f2 = 440.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);

        let tonal_decay = 0.15 * decay_mod;
        let tonal_env = (-self.time / tonal_decay).exp();
        let tonal = (osc_sine(self.phase1) * 0.5 + osc_sine(self.phase2) * 0.5) * tonal_env;

        let noise_decay = 0.22 * decay_mod;
        let noise_env = (-self.time / noise_decay).exp();
        let raw_noise = self.noise();
        let filtered_noise = self.hp1.tick_hp(raw_noise, 2000.0, sr);
        let noise_out = filtered_noise * noise_env * noise_mod;

        let snappy = 0.65;
        let out = tonal * (1.0 - snappy) + noise_out * snappy;

        if tonal_env < 0.001 && noise_env < 0.001 {
            self.active = false;
        }
        out
    }

    /// 808 Closed Hi-Hat: 6 square oscillators -> two parallel BPFs -> HPF.
    pub(crate) fn synth_808_closed_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        // Two parallel bandpass filters
        let bp1 = self.svf1.bandpass(raw, 3440.0, 3.0, sr);
        let bp2 = self.svf2.bandpass(raw, 7100.0, 3.0, sr);
        let filtered = bp1 * 0.5 + bp2 * 0.5;

        // High-pass at 6kHz
        let hpf = self.hp1.tick_hp(filtered, 6000.0, sr);

        // Short exponential decay ~50ms
        let decay = 0.05 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        hpf * env
    }

    /// 808 Open Hi-Hat: Same 6 oscillators + filters, longer decay.
    pub(crate) fn synth_808_open_hat(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        let bp1 = self.svf1.bandpass(raw, 3440.0, 3.0, sr);
        let bp2 = self.svf2.bandpass(raw, 7100.0, 3.0, sr);
        let filtered = bp1 * 0.5 + bp2 * 0.5;

        let hpf = self.hp1.tick_hp(filtered, 6000.0, sr);

        // Longer decay 200-800ms
        let decay = 0.35 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        hpf * env
    }

    /// 808 Clap: 4 rapid noise bursts + reverb tail.
    pub(crate) fn synth_808_clap(&mut self, sr: f64, decay_mod: f64, noise_mod: f64) -> f64 {
        let raw_noise = self.noise() * noise_mod;
        let filtered = self.svf1.bandpass(raw_noise, 1000.0, 2.0, sr);

        // 4 bursts spaced 10ms apart, each ~3ms attack, ~7ms decay
        let mut burst_env = 0.0;
        for burst in 0..4 {
            let burst_start = burst as f64 * 0.010;
            let t_in_burst = self.time - burst_start;
            if t_in_burst >= 0.0 && t_in_burst < 0.010 {
                let attack = if t_in_burst < 0.003 {
                    t_in_burst / 0.003
                } else {
                    (-((t_in_burst - 0.003) / 0.005)).exp()
                };
                burst_env += attack;
            }
        }

        // Reverb tail starting after bursts
        let tail_start = 0.040;
        let tail_env = if self.time > tail_start {
            (-(self.time - tail_start) / (0.15 * decay_mod)).exp()
        } else {
            0.0
        };

        let env = burst_env + tail_env * 0.7;
        if self.time > tail_start + 0.15 * decay_mod * 7.0 {
            self.active = false;
        }
        filtered * env
    }

    /// 808 Cowbell: Two square waves at 540 Hz and 800 Hz through BPF.
    pub(crate) fn synth_808_cowbell(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        advance_phase(&mut self.phase1, 540.0 * tone_mod, sr);
        advance_phase(&mut self.phase2, 800.0 * tone_mod, sr);

        let raw = osc_square(self.phase1) * 0.5 + osc_square(self.phase2) * 0.5;
        let filtered = self.svf1.bandpass(raw, 700.0, 3.0, sr);

        let decay = 0.065 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        filtered * env
    }

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

    /// 808 Tom: Sine with pitch droop + small noise.
    pub(crate) fn synth_808_tom(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, base_freq: f64) -> f64 {
        let freq = base_freq * tone_mod;
        // Subtle pitch droop: starts 15% higher
        let droop = freq * 0.15 * (-self.time * 40.0).exp();
        advance_phase(&mut self.phase1, freq + droop, sr);

        let decay = match base_freq as u32 {
            0..=120 => 0.18,
            121..=180 => 0.12,
            _ => 0.08,
        } * decay_mod;

        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }

        let body = osc_sine(self.phase1);
        // Small noise click at attack
        let noise_env = (-self.time * 200.0).exp();
        let noise_click = self.noise() * noise_env * 0.15;

        (body + noise_click) * env
    }

    /// 808 Cymbal: 6 oscillators, dual envelope for spectral shift.
    pub(crate) fn synth_808_cymbal(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, size: f64) -> f64 {
        let mut freqs = HAT_FREQS_808;
        for f in freqs.iter_mut() {
            *f *= tone_mod;
        }
        let raw = self.hat_oscs.tick(sr, &freqs);

        // Lower BPF at 3440Hz with longer decay
        let bp_low = self.svf1.bandpass(raw, 3440.0, 2.5, sr);
        let low_decay = (0.6 + size * 0.6) * decay_mod;
        let low_env = (-self.time / low_decay).exp();

        // Upper BPF at 7100Hz with shorter decay
        let bp_high = self.svf2.bandpass(raw, 7100.0, 2.5, sr);
        let high_decay = (0.3 + size * 0.2) * decay_mod;
        let high_env = (-self.time / high_decay).exp();

        if low_env < 0.001 && high_env < 0.001 {
            self.active = false;
            return 0.0;
        }

        bp_low * low_env * 0.5 + bp_high * high_env * 0.5
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

    /// 808 Bongo: Sine with slight pitch droop, fast decay.
    pub(crate) fn synth_808_bongo(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f = freq * tone_mod;
        let droop = f * 0.1 * (-self.time * 80.0).exp();
        advance_phase(&mut self.phase1, f + droop, sr);

        let decay = 0.06 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        osc_sine(self.phase1) * env
    }

    /// 808 Conga: Sine body, no noise component.
    pub(crate) fn synth_808_conga(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f = freq * tone_mod;
        let droop = f * 0.08 * (-self.time * 50.0).exp();
        advance_phase(&mut self.phase1, f + droop, sr);

        let decay = 0.10 * decay_mod;
        let env = (-self.time / decay).exp();
        if env < 0.001 {
            self.active = false;
            return 0.0;
        }
        osc_sine(self.phase1) * env
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
