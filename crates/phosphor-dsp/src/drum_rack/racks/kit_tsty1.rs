//! TSTY1 kit synthesis.

use super::super::*;
use std::f64::consts::TAU;

impl DrumVoice {
    // TSTY-1: Warm vintage studio kit — 88 sounds
    // Tape-saturated, reel-to-reel warmth, clean analog funk character
    // ══════════════════════════════════════════════════════════════════════════

    /// Gentle tape saturation — warm soft-clip with even harmonics.
    pub(crate) fn tape_sat(x: f64, amount: f64) -> f64 {
        if amount < 0.01 { return x; }
        let g = 1.0 + amount * 3.0;
        let driven = x * g;
        // Asymmetric soft-clip adds even harmonics (tape character)
        let out = driven / (1.0 + driven.abs()) + 0.05 * driven / (1.0 + (driven * 0.5).powi(2));
        out / g.sqrt()
    }

    /// Warm lowpass — single-pole filter simulating tape HF rolloff.
    pub(crate) fn tape_lp(&mut self, input: f64, cutoff: f64, sr: f64) -> f64 {
        let rc = 1.0 / (TAU * cutoff);
        let alpha = 1.0 / (1.0 + rc * sr);
        self.lp1_state = self.lp1_state + alpha * (input - self.lp1_state);
        self.lp1_state
    }

    pub(crate) fn synth_tsty1(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, _drive_amt: f64) -> f64 {
        // All tsty-1 sounds go through tape saturation + warm LP
        let raw = self.synth_tsty1_raw(sr, decay_mod, tone_mod, noise_mod);
        let saturated = Self::tape_sat(raw, 0.4);
        // Warm tape rolloff — cuts harsh highs like reel-to-reel
        let freq_cutoff = 8000.0 + tone_mod * 4000.0; // 8-12kHz tape rolloff
        self.tape_lp(saturated, freq_cutoff, sr)
    }

    pub(crate) fn synth_tsty1_raw(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        match self.sound {
            // ── KICKS (notes 24-35: 12 variations) ──
            DrumSound::SubKick(mult) => {
                // Variety of warm kicks based on mult value
                let variant = (mult * 20.0) as u8;
                match variant {
                    0..=1 => self.tsty1_kick_deep(sr, decay_mod, tone_mod),
                    2..=3 => self.tsty1_kick_round(sr, decay_mod, tone_mod),
                    4..=5 => self.tsty1_kick_punchy(sr, decay_mod, tone_mod),
                    6..=7 => self.tsty1_kick_warm(sr, decay_mod, tone_mod),
                    8..=9 => self.tsty1_kick_tight(sr, decay_mod, tone_mod),
                    10..=11 => self.tsty1_kick_boom(sr, decay_mod, tone_mod),
                    12..=13 => self.tsty1_kick_click(sr, decay_mod, tone_mod),
                    _ => self.tsty1_kick_vinyl(sr, decay_mod, tone_mod),
                }
            }
            DrumSound::Kick => self.tsty1_kick_studio(sr, decay_mod, tone_mod),

            // ── SNARES ──
            DrumSound::Snare => self.tsty1_snare_funk(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::SnareAlt => self.tsty1_snare_crisp(sr, decay_mod, tone_mod, noise_mod),
            DrumSound::Rimshot => self.tsty1_rim_warm(sr, decay_mod, tone_mod),
            DrumSound::Clap => self.tsty1_clap_studio(sr, decay_mod, noise_mod),

            // ── HATS ──
            DrumSound::ClosedHat => self.tsty1_hat_closed_tight(sr, decay_mod, tone_mod),
            DrumSound::PedalHat => self.tsty1_hat_closed_soft(sr, decay_mod, tone_mod),
            DrumSound::OpenHat => self.tsty1_hat_open_shimmer(sr, decay_mod, tone_mod),

            // ── TOMS ──
            DrumSound::LowTom => self.tsty1_tom_warm(sr, decay_mod, tone_mod, 90.0),
            DrumSound::MidTom => self.tsty1_tom_warm(sr, decay_mod, tone_mod, 140.0),
            DrumSound::HighTom => self.tsty1_tom_warm(sr, decay_mod, tone_mod, 200.0),

            // ── CYMBALS ──
            DrumSound::Crash => self.tsty1_crash_warm(sr, decay_mod, tone_mod),
            DrumSound::Ride => self.tsty1_ride_smooth(sr, decay_mod, tone_mod),
            DrumSound::RideBell => self.tsty1_ride_bell(sr, decay_mod, tone_mod),
            DrumSound::Cymbal => self.tsty1_crash_warm(sr, decay_mod, tone_mod),
            DrumSound::Splash => self.tsty1_splash(sr, decay_mod, tone_mod),

            // ── PERCUSSION ──
            DrumSound::Cowbell => self.tsty1_cowbell(sr, decay_mod, tone_mod),
            DrumSound::Clave => self.tsty1_woodblock(sr, decay_mod),
            DrumSound::Tambourine => self.tsty1_tambourine(sr, decay_mod),
            DrumSound::Maracas => self.tsty1_shaker(sr, decay_mod),
            DrumSound::Cabasa => self.tsty1_cabasa(sr, decay_mod),
            DrumSound::Vibraslap => self.tsty1_vibraslap(sr, decay_mod),
            DrumSound::Conga(f) => self.tsty1_conga(sr, decay_mod, tone_mod, f),
            DrumSound::Bongo(f) => self.tsty1_bongo(sr, decay_mod, tone_mod, f),
            DrumSound::Timbale(f) => self.tsty1_timbale(sr, decay_mod, tone_mod, f),
            DrumSound::Agogo(f) => self.tsty1_agogo(sr, decay_mod, tone_mod, f),
            DrumSound::Whistle(d) => self.synth_808_whistle(sr, decay_mod, d),
            DrumSound::Guiro(d) => self.synth_808_guiro(sr, decay_mod, d),

            // ── FX (76-127) ──
            DrumSound::FxNoise(v) => self.tsty1_fx(sr, decay_mod, tone_mod, noise_mod, v),
        }
    }

    // ── TSTY-1 Kick variations ──

    pub(crate) fn tsty1_kick_studio(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        // Classic studio kick: warm body + subtle click
        let f = 52.0 * tone_mod;
        let sweep = f * 0.8 * (-self.time * 45.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1);
        let click = self.noise() * (-self.time * 300.0).exp() * 0.15;
        let env = (-self.time / (0.35 * decay_mod)).exp();
        (body * 0.85 + click) * env
    }

    pub(crate) fn tsty1_kick_deep(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 38.0 * tone_mod;
        let sweep = f * 0.5 * (-self.time * 30.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1);
        let sub = osc_sine(self.phase1 * 0.5) * 0.3; // sub harmonic
        let env = (-self.time / (0.5 * decay_mod)).exp();
        (body * 0.7 + sub) * env
    }

    pub(crate) fn tsty1_kick_round(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 48.0 * tone_mod;
        let sweep = f * 0.6 * (-self.time * 35.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        // Triangle wave for rounder tone
        let body = osc_triangle(self.phase1);
        let env = (-self.time / (0.3 * decay_mod)).exp();
        body * 0.9 * env
    }

    pub(crate) fn tsty1_kick_punchy(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 55.0 * tone_mod;
        let sweep = f * 1.2 * (-self.time * 60.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1);
        let click = self.noise() * (-self.time * 500.0).exp() * 0.25;
        let env = (-self.time / (0.2 * decay_mod)).exp();
        (body * 0.8 + click) * env
    }

    pub(crate) fn tsty1_kick_warm(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 45.0 * tone_mod;
        let sweep = f * 0.4 * (-self.time * 25.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        advance_phase(&mut self.phase2, f * 2.0 + sweep * 2.0, sr);
        let body = osc_sine(self.phase1) * 0.8 + osc_sine(self.phase2) * 0.15 * (-self.time * 80.0).exp();
        let env = (-self.time / (0.4 * decay_mod)).exp();
        body * env
    }

    pub(crate) fn tsty1_kick_tight(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 60.0 * tone_mod;
        let sweep = f * 1.5 * (-self.time * 80.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1);
        let env = (-self.time / (0.15 * decay_mod)).exp();
        body * 0.9 * env
    }

    pub(crate) fn tsty1_kick_boom(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 40.0 * tone_mod;
        let sweep = f * 0.3 * (-self.time * 15.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1);
        let rumble = osc_sine(self.phase1 * 0.5) * 0.2;
        let env = (-self.time / (0.6 * decay_mod)).exp();
        (body * 0.8 + rumble) * env
    }

    pub(crate) fn tsty1_kick_click(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let f = 65.0 * tone_mod;
        let sweep = f * 2.0 * (-self.time * 100.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1) * 0.7;
        let click = self.noise() * (-self.time * 800.0).exp() * 0.4;
        let env = (-self.time / (0.12 * decay_mod)).exp();
        (body + click) * env
    }

    pub(crate) fn tsty1_kick_vinyl(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        // Lo-fi warm kick with subtle noise floor
        let f = 50.0 * tone_mod;
        let sweep = f * 0.6 * (-self.time * 40.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let body = osc_sine(self.phase1);
        let floor = self.noise() * 0.02; // tape hiss
        let env = (-self.time / (0.35 * decay_mod)).exp();
        (body * 0.85 + floor) * env
    }

    // ── TSTY-1 Snare variations ──

    pub(crate) fn tsty1_snare_funk(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        // Warm funky snare — two body tones + filtered noise
        let f1 = 185.0 * tone_mod;
        let f2 = 330.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);
        let body = osc_sine(self.phase1) * 0.4 + osc_sine(self.phase2) * 0.25;
        let body_env = (-self.time / (0.12 * decay_mod)).exp();
        let raw_noise = self.noise() * noise_mod;
        let filtered = self.svf1.bandpass(raw_noise, 3500.0 * tone_mod, 1.5, sr);
        let noise_env = (-self.time / (0.18 * decay_mod)).exp();
        body * body_env + filtered * noise_env * 0.45
    }

    pub(crate) fn tsty1_snare_crisp(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64) -> f64 {
        // Brighter, crisper snare
        let f1 = 200.0 * tone_mod;
        let f2 = 400.0 * tone_mod;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);
        let body = osc_sine(self.phase1) * 0.35 + osc_triangle(self.phase2) * 0.2;
        let body_env = (-self.time / (0.08 * decay_mod)).exp();
        let raw_noise = self.noise() * noise_mod;
        let hp_noise = self.hp1.tick_hp(raw_noise, 4000.0 * tone_mod, sr);
        let filtered = self.svf1.bandpass(hp_noise, 6000.0, 1.2, sr);
        let noise_env = (-self.time / (0.15 * decay_mod)).exp();
        body * body_env + filtered * noise_env * 0.5
    }

    pub(crate) fn tsty1_rim_warm(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        // Warm rimshot with woody body
        advance_phase(&mut self.phase1, 480.0 * tone_mod, sr);
        advance_phase(&mut self.phase2, 1200.0 * tone_mod, sr);
        let body = osc_sine(self.phase1) * 0.5 + osc_sine(self.phase2) * 0.3;
        let click = self.noise() * (-self.time * 600.0).exp() * 0.2;
        let env = (-self.time / (0.02 * decay_mod)).exp();
        (body + click) * env
    }

    pub(crate) fn tsty1_clap_studio(&mut self, sr: f64, decay_mod: f64, noise_mod: f64) -> f64 {
        // Studio clap: multiple noise bursts with warm reverb tail
        let raw_noise = self.noise() * noise_mod;
        let filtered = self.svf1.bandpass(raw_noise, 1200.0, 2.5, sr);
        // 4 staggered hits for clap spread
        let burst_spacing = 0.012;
        let mut env = 0.0;
        for n in 0..4 {
            let t_offset = self.time - n as f64 * burst_spacing;
            if t_offset >= 0.0 {
                env += (-t_offset * 200.0).exp() * 0.3;
            }
        }
        // Reverb tail
        let tail = (-self.time / (0.15 * decay_mod)).exp() * 0.4;
        filtered * (env + tail)
    }

    // ── TSTY-1 Hat variations ──

    pub(crate) fn tsty1_hat_closed_tight(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        // Tight closed hat — bright but warm
        let mut freqs = [310.0, 456.0, 620.0, 830.0, 1050.0, 1380.0];
        for f in freqs.iter_mut() { *f *= tone_mod; }
        let metallic = self.hat_oscs.tick(sr, &freqs);
        let filtered = self.svf1.bandpass(metallic, 7500.0, 1.8, sr);
        let hp = self.hp1.tick_hp(filtered, 5000.0, sr);
        let env = (-self.time / (0.035 * decay_mod)).exp();
        hp * env * 0.4
    }

    pub(crate) fn tsty1_hat_closed_soft(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        // Softer closed hat — more mellow
        let mut freqs = [280.0, 420.0, 580.0, 780.0, 1000.0, 1300.0];
        for f in freqs.iter_mut() { *f *= tone_mod; }
        let metallic = self.hat_oscs.tick(sr, &freqs);
        let filtered = self.svf1.bandpass(metallic, 5500.0, 1.5, sr);
        let hp = self.hp1.tick_hp(filtered, 3500.0, sr);
        let env = (-self.time / (0.05 * decay_mod)).exp();
        hp * env * 0.35
    }

    pub(crate) fn tsty1_hat_open_shimmer(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        // Open hat — shimmery, sustained
        let mut freqs = [320.0, 470.0, 640.0, 860.0, 1100.0, 1420.0];
        for f in freqs.iter_mut() { *f *= tone_mod; }
        let metallic = self.hat_oscs.tick(sr, &freqs);
        let filtered = self.svf1.bandpass(metallic, 6500.0, 1.3, sr);
        let hp = self.hp1.tick_hp(filtered, 4000.0, sr);
        let env = (-self.time / (0.35 * decay_mod)).exp();
        hp * env * 0.35
    }

    // ── TSTY-1 Toms ──

    pub(crate) fn tsty1_tom_warm(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, base_freq: f64) -> f64 {
        let f = base_freq * tone_mod;
        let droop = f * 0.12 * (-self.time * 30.0).exp();
        advance_phase(&mut self.phase1, f + droop, sr);
        advance_phase(&mut self.phase2, f * 1.5, sr); // adds warmth
        let body = osc_sine(self.phase1) * 0.7 + osc_sine(self.phase2) * 0.15 * (-self.time * 60.0).exp();
        let attack = self.noise() * (-self.time * 200.0).exp() * 0.1;
        let env = (-self.time / (0.25 * decay_mod)).exp();
        (body + attack) * env
    }

    // ── TSTY-1 Cymbals ──

    pub(crate) fn tsty1_crash_warm(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = [340.0, 510.0, 680.0, 920.0, 1150.0, 1500.0];
        for f in freqs.iter_mut() { *f *= tone_mod; }
        let metallic = self.hat_oscs.tick(sr, &freqs);
        let noise_layer = self.noise() * 0.2;
        let mixed = metallic * 0.5 + noise_layer;
        let filtered = self.svf1.lowpass(mixed, 9000.0 * tone_mod, 0.3, sr);
        let env = (-self.time / (1.2 * decay_mod)).exp();
        filtered * env * 0.3
    }

    pub(crate) fn tsty1_ride_smooth(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = [380.0, 560.0, 750.0, 1000.0, 1280.0, 1650.0];
        for f in freqs.iter_mut() { *f *= tone_mod; }
        let metallic = self.hat_oscs.tick(sr, &freqs);
        let filtered = self.svf1.bandpass(metallic, 5000.0 * tone_mod, 1.0, sr);
        let env = (-self.time / (0.8 * decay_mod)).exp();
        let attack_env = 1.0 - (-self.time * 200.0).exp();
        filtered * env * attack_env.min(1.0) * 0.3
    }

    pub(crate) fn tsty1_ride_bell(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        advance_phase(&mut self.phase1, 680.0 * tone_mod, sr);
        advance_phase(&mut self.phase2, 1020.0 * tone_mod, sr);
        let bell = osc_sine(self.phase1) * 0.4 + osc_sine(self.phase2) * 0.3;
        let env = (-self.time / (0.6 * decay_mod)).exp();
        bell * env
    }

    pub(crate) fn tsty1_splash(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        let mut freqs = [400.0, 600.0, 820.0, 1100.0, 1400.0, 1800.0];
        for f in freqs.iter_mut() { *f *= tone_mod; }
        let metallic = self.hat_oscs.tick(sr, &freqs);
        let filtered = self.svf1.bandpass(metallic, 7000.0, 1.5, sr);
        let env = (-self.time / (0.5 * decay_mod)).exp();
        filtered * env * 0.3
    }

    // ── TSTY-1 Percussion ──

    pub(crate) fn tsty1_cowbell(&mut self, sr: f64, decay_mod: f64, tone_mod: f64) -> f64 {
        advance_phase(&mut self.phase1, 560.0 * tone_mod, sr);
        advance_phase(&mut self.phase2, 845.0 * tone_mod, sr);
        let body = osc_sine(self.phase1) * 0.4 + osc_sine(self.phase2) * 0.35;
        let filtered = self.svf1.bandpass(body, 700.0, 3.0, sr);
        let env = (-self.time / (0.06 * decay_mod)).exp();
        filtered * env
    }

    pub(crate) fn tsty1_woodblock(&mut self, sr: f64, decay_mod: f64) -> f64 {
        advance_phase(&mut self.phase1, 1800.0, sr);
        let click = osc_sine(self.phase1);
        let env = (-self.time / (0.015 * decay_mod)).exp();
        click * env * 0.5
    }

    pub(crate) fn tsty1_tambourine(&mut self, sr: f64, decay_mod: f64) -> f64 {
        let raw_noise = self.noise();
        let hp = self.hp1.tick_hp(raw_noise, 6000.0, sr);
        let bp = self.svf1.bandpass(hp, 9000.0, 2.0, sr);
        let jingle = bp * 0.4;
        // Rhythmic jingle modulation
        let mod_env = (self.time * 25.0).sin().abs() * (-self.time * 8.0).exp();
        let main_env = (-self.time / (0.15 * decay_mod)).exp();
        jingle * (main_env + mod_env * 0.3)
    }

    pub(crate) fn tsty1_shaker(&mut self, sr: f64, decay_mod: f64) -> f64 {
        let raw_noise = self.noise();
        let filtered = self.svf1.bandpass(raw_noise, 7000.0, 1.5, sr);
        let hp = self.hp1.tick_hp(filtered, 5000.0, sr);
        let env = (-self.time / (0.08 * decay_mod)).exp();
        hp * env * 0.35
    }

    pub(crate) fn tsty1_cabasa(&mut self, sr: f64, decay_mod: f64) -> f64 {
        let raw_noise = self.noise();
        let filtered = self.svf1.bandpass(raw_noise, 8500.0, 2.0, sr);
        let env = (-self.time / (0.1 * decay_mod)).exp();
        filtered * env * 0.3
    }

    pub(crate) fn tsty1_vibraslap(&mut self, sr: f64, decay_mod: f64) -> f64 {
        let raw_noise = self.noise();
        let filtered = self.svf1.bandpass(raw_noise, 3200.0, 5.0, sr);
        let rattle = (self.time * 30.0 * TAU).sin().abs();
        let env = (-self.time / (0.4 * decay_mod)).exp();
        filtered * rattle * env * 0.3
    }

    pub(crate) fn tsty1_conga(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f = freq * tone_mod;
        let droop = f * 0.06 * (-self.time * 40.0).exp();
        advance_phase(&mut self.phase1, f + droop, sr);
        let body = osc_sine(self.phase1);
        let slap = self.noise() * (-self.time * 400.0).exp() * 0.15;
        let env = (-self.time / (0.2 * decay_mod)).exp();
        (body * 0.7 + slap) * env
    }

    pub(crate) fn tsty1_bongo(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f = freq * tone_mod;
        let droop = f * 0.08 * (-self.time * 60.0).exp();
        advance_phase(&mut self.phase1, f + droop, sr);
        let body = osc_sine(self.phase1);
        let env = (-self.time / (0.12 * decay_mod)).exp();
        body * 0.7 * env
    }

    pub(crate) fn tsty1_timbale(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f = freq * tone_mod;
        advance_phase(&mut self.phase1, f, sr);
        let body = osc_sine(self.phase1);
        let ring = osc_sine(self.phase1 * 2.5) * 0.15 * (-self.time * 30.0).exp();
        let shell = self.noise() * (-self.time * 300.0).exp() * 0.1;
        let env = (-self.time / (0.18 * decay_mod)).exp();
        (body * 0.6 + ring + shell) * env
    }

    pub(crate) fn tsty1_agogo(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, freq: f64) -> f64 {
        let f1 = freq * tone_mod;
        let f2 = f1 * 1.48;
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);
        let body = osc_sine(self.phase1) * 0.4 + osc_sine(self.phase2) * 0.3;
        let env = (-self.time / (0.15 * decay_mod)).exp();
        body * env
    }

    pub(crate) fn tsty1_fx(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, character: f64) -> f64 {
        // Variety of warm FX sounds
        let freq = 400.0 + character * 6000.0;
        let sweep = freq * (1.0 + 1.5 * (-self.time * 8.0).exp());
        advance_phase(&mut self.phase1, sweep * tone_mod, sr);
        let tone = osc_sine(self.phase1) * 0.4;
        let noise = self.noise() * noise_mod * 0.2;
        let env = (-self.time / (0.3 * decay_mod)).exp();
        (tone + noise) * env
    }

    // ══════════════════════════════════════════════════════════════════════════
}
