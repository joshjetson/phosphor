//! 777 kit synthesis.

use super::super::*;

impl DrumVoice {
    // ── Kit 777: 808/909 bass + original creative sounds ──

    pub(crate) fn synth_777(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, drive_amt: f64) -> f64 {
        match self.sound {
            // Notes 24-35 mapped to SubKick: 808-style bass (deep sine, heavy sweep, long decay)
            DrumSound::SubKick(_) | DrumSound::Kick if self.note < 36 => {
                let variation = if let DrumSound::SubKick(v) = self.sound { v } else { 0.5 };
                let freq = 42.0 * (0.6 + variation * 0.8);
                let sweep = 9.0 * (1.2 - variation * 0.6);
                let decay = (0.40 * (1.5 - variation * 0.8)) * decay_mod;
                let env = (-self.time / decay).exp();
                let pitch_env = sweep * (-self.time * (25.0 + variation * 20.0)).exp();
                let f = freq * (1.0 + pitch_env) * (0.9 + tone_mod * 0.2);
                self.phase1 += f / sr;
                if self.phase1 > 1.0 { self.phase1 -= 1.0; }
                let body = (self.phase1 * std::f64::consts::TAU).sin();
                let click = (-self.time * 800.0).exp() * (0.3 + variation * 0.4);
                (body + click * 0.1) * env
            }
            // Notes 36-47: 909-style bass (distorted sine, noise click, medium sweep)
            DrumSound::Kick | DrumSound::Snare | DrumSound::SnareAlt | DrumSound::Clap
                if self.note >= 36 && self.note <= 47 => {
                let v = (self.note - 36) as f64 / 11.0;
                let freq = 58.0 * (0.6 + v * 0.8);
                let sweep = 5.0 * (1.2 - v * 0.6);
                let decay = (0.22 * (1.5 - v * 0.8)) * decay_mod;
                let env = (-self.time / decay).exp();
                let pitch_env = sweep * (-self.time * (25.0 + v * 20.0)).exp();
                let f = freq * (1.0 + pitch_env) * (0.9 + tone_mod * 0.2);
                self.phase1 += f / sr;
                if self.phase1 > 1.0 { self.phase1 -= 1.0; }
                let sine = (self.phase1 * std::f64::consts::TAU).sin();
                // 909-style distortion
                let gain = 1.0 + (0.35 + drive_amt) * 8.0;
                let driven = sine * gain;
                let body = driven / (1.0 + driven.abs()).sqrt();
                // Noise click
                let noise = self.noise();
                let click_env = (-self.time * 200.0).exp();
                let click = noise * click_env * 0.25;
                (body + click) * env
            }
            // 777 closed hat: granular metallic — ring mod of two FM'd sines
            DrumSound::ClosedHat | DrumSound::PedalHat => {
                let decay = 0.04 * decay_mod;
                let env = (-self.time / decay).exp();
                let f1 = 3200.0 * (0.8 + tone_mod * 0.4);
                let f2 = 5100.0 * (0.9 + tone_mod * 0.2);
                self.phase1 += f1 / sr;
                self.phase2 += f2 / sr;
                let fm = (self.phase1 * std::f64::consts::TAU).sin();
                let carrier = ((self.phase2 + fm * 0.3) * std::f64::consts::TAU).sin();
                let ring = fm * carrier; // ring modulation = metallic
                let noise = self.noise() * 0.3 * noise_mod;
                (ring + noise) * env
            }
            // 777 open hat: same as closed but with slow amplitude modulation
            DrumSound::OpenHat => {
                let decay = 0.25 * decay_mod;
                let env = (-self.time / decay).exp();
                let f1 = 2800.0 * (0.8 + tone_mod * 0.4);
                let f2 = 4700.0 * (0.9 + tone_mod * 0.2);
                self.phase1 += f1 / sr;
                self.phase2 += f2 / sr;
                let fm = (self.phase1 * std::f64::consts::TAU).sin();
                let carrier = ((self.phase2 + fm * 0.4) * std::f64::consts::TAU).sin();
                let ring = fm * carrier;
                let tremolo = 1.0 + (self.time * 35.0 * std::f64::consts::TAU).sin() * 0.15;
                let noise = self.noise() * 0.4 * noise_mod;
                (ring * tremolo + noise) * env
            }
            // 777 snare: tuned body + gated noise burst with pitch drop
            DrumSound::Snare | DrumSound::SnareAlt => {
                let decay_body = 0.12 * decay_mod;
                let decay_noise = 0.08 * decay_mod * (0.5 + noise_mod);
                let env_body = (-self.time / decay_body).exp();
                let env_noise = (-self.time / decay_noise).exp();
                let f = 220.0 * (0.8 + tone_mod * 0.4);
                let pitch_drop = 1.5 * (-self.time * 40.0).exp();
                self.phase1 += f * (1.0 + pitch_drop) / sr;
                let body = (self.phase1 * std::f64::consts::TAU).sin();
                // Gated noise — bursts
                let gate = if (self.time * 200.0).fract() < 0.6 { 1.0 } else { 0.3 };
                let noise = self.noise() * gate;
                body * env_body * 0.5 + noise * env_noise * 0.5
            }
            // 777 clap: filtered noise with chorus-like multi-tap
            DrumSound::Clap => {
                let decay = 0.1 * decay_mod;
                let env = (-self.time / decay).exp();
                let n1 = self.noise();
                let n2 = white_noise(self.noise_counter.wrapping_add(7000));
                let n3 = white_noise(self.noise_counter.wrapping_add(15000));
                let multi = (n1 + n2 * 0.8 + n3 * 0.6) / 2.4;
                let bp_freq = 1500.0 * (0.8 + tone_mod * 0.4);
                let (_, bp, _) = self.svf1.tick(multi, bp_freq, 3.0, sr);
                bp * env
            }
            // 777 cowbell: detuned additive with inharmonic partials
            DrumSound::Cowbell => {
                let decay = 0.06 * decay_mod;
                let env = (-self.time / decay).exp();
                self.phase1 += 587.0 / sr;
                self.phase2 += 845.0 / sr;
                let s1 = (self.phase1 * std::f64::consts::TAU).sin();
                let s2 = (self.phase2 * std::f64::consts::TAU).sin();
                let s3 = ((self.phase1 * 2.17) * std::f64::consts::TAU).sin() * 0.3;
                (s1 + s2 + s3) * env * 0.4
            }
            // 777 crash: noise wash with spectral motion
            DrumSound::Crash | DrumSound::Splash | DrumSound::Cymbal => {
                let decay = 0.6 * decay_mod;
                let env = (-self.time / decay).exp();
                let noise = self.noise();
                let sweep = (self.time * 3.0).min(1.0);
                let cutoff = 3000.0 + sweep * 6000.0 * tone_mod;
                let filtered = self.lp1.tick_lp(noise, cutoff, sr);
                self.phase1 += (440.0 * tone_mod + 200.0) / sr;
                let ring = (self.phase1 * std::f64::consts::TAU).sin() * 0.15;
                (filtered + ring) * env
            }
            // 777 ride: metallic ping with noise tail
            DrumSound::Ride | DrumSound::RideBell => {
                let decay = 0.3 * decay_mod;
                let env = (-self.time / decay).exp();
                self.phase1 += 620.0 / sr;
                self.phase2 += 893.0 / sr;
                let ping = (self.phase1 * std::f64::consts::TAU).sin()
                    * (self.phase2 * std::f64::consts::TAU).sin();
                let noise = self.noise() * 0.2 * (-self.time / (decay * 0.7)).exp();
                (ping * 0.6 + noise) * env
            }
            // 777 rimshot: phase-distorted click
            DrumSound::Rimshot => {
                let decay = 0.012 * decay_mod;
                let env = (-self.time / decay).exp();
                let f = 1200.0 * (0.8 + tone_mod * 0.4);
                self.phase1 += f / sr;
                // Phase distortion — sine of sine
                let pd = (self.phase1 * std::f64::consts::TAU).sin();
                let out = (pd * 3.0).sin();
                out * env * 0.6
            }
            // 777 toms: FM synthesis toms
            DrumSound::LowTom => self.synth_777_fm_tom(sr, decay_mod, tone_mod, 90.0),
            DrumSound::MidTom => self.synth_777_fm_tom(sr, decay_mod, tone_mod, 140.0),
            DrumSound::HighTom => self.synth_777_fm_tom(sr, decay_mod, tone_mod, 200.0),
            // 777 clave: hard digital click
            DrumSound::Clave => {
                let decay = 0.02 * decay_mod;
                let env = (-self.time / decay).exp();
                self.phase1 += 2800.0 / sr;
                let sq = if self.phase1.fract() < 0.5 { 1.0 } else { -1.0 };
                sq * env * 0.4
            }
            // Everything else: route through creative FX synthesis
            _ => {
                let character = (self.note as f64 - 48.0) / 80.0;
                self.synth_777_fx(sr, decay_mod, tone_mod, noise_mod, character.clamp(0.0, 1.0))
            }
        }
    }

    pub(crate) fn synth_777_fm_tom(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, base_freq: f64) -> f64 {
        let decay = 0.15 * decay_mod;
        let env = (-self.time / decay).exp();
        let f = base_freq * (0.8 + tone_mod * 0.4);
        let pitch_drop = 2.0 * (-self.time * 25.0).exp();
        // FM: modulator modulates carrier
        self.phase2 += (f * 2.0 * (1.0 + pitch_drop)) / sr;
        let modulator = (self.phase2 * std::f64::consts::TAU).sin();
        self.phase1 += (f * (1.0 + pitch_drop) + modulator * f * 0.5) / sr;
        let carrier = (self.phase1 * std::f64::consts::TAU).sin();
        carrier * env * 0.5
    }

    pub(crate) fn synth_777_fx(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, character: f64) -> f64 {
        let decay = (0.05 + character * 0.4) * decay_mod;
        let env = (-self.time / decay).exp();
        let f = 200.0 + character * 3000.0;
        // Wavefolder synthesis — creates rich harmonics
        self.phase1 += f * (0.8 + tone_mod * 0.4) / sr;
        let sine = (self.phase1 * std::f64::consts::TAU).sin();
        let fold_amount = 1.0 + character * 4.0;
        let folded = (sine * fold_amount).sin(); // sine of sine = wavefolding
        let noise = self.noise() * noise_mod * 0.3;
        (folded * 0.4 + noise * 0.3) * env
    }

    // ══════════════════════════════════════════════════════════════════════════
}
