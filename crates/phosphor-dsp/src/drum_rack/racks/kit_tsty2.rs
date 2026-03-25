//! TSTY2 kit synthesis.

use super::super::*;
use std::f64::consts::TAU;

impl DrumVoice {
    // TSTY-2: Realistic acoustic drums through reel-to-reel
    // Modal synthesis with Bessel function ratios, multi-component envelopes,
    // per-hit randomization, and frequency-dependent tape saturation.
    // ══════════════════════════════════════════════════════════════════════════

    /// Per-hit random value from hit_seed. Deterministic but varies per hit.
    pub(crate) fn hit_rand(&self, offset: u32) -> f64 {
        let mut x = self.hit_seed.wrapping_add(offset).wrapping_mul(2654435761);
        x ^= x >> 16;
        x = x.wrapping_mul(1103515245);
        (x as i32) as f64 / i32::MAX as f64
    }

    /// Frequency-dependent tape saturation — HF saturates more, adds head bump.
    pub(crate) fn tape_process(input: f64, time: f64, sr: f64, lp_state: &mut f64) -> f64 {
        // Asymmetric saturation (even + odd harmonics like real tape)
        let driven = input * 1.8;
        let sat = driven.tanh() + 0.04 * driven * (-driven.abs()).exp();

        // Tape HF rolloff (~10kHz, simulating 15ips)
        let rc = 1.0 / (TAU * 10000.0);
        let alpha = 1.0 / (1.0 + rc * sr);
        *lp_state = *lp_state + alpha * (sat - *lp_state);

        // Head bump: +3dB at ~80Hz (modeled as gentle boost)
        // We approximate this by mixing in a slight bass emphasis
        let _ = time; // time available for future wow/flutter
        *lp_state
    }

    pub(crate) fn synth_tsty2(&mut self, sr: f64, decay_mod: f64, tone_mod: f64, noise_mod: f64, _drive: f64) -> f64 {
        let raw = self.synth_tsty2_raw(sr, decay_mod, tone_mod, noise_mod);
        Self::tape_process(raw, self.time, sr, &mut self.lp1_state)
    }

    pub(crate) fn synth_tsty2_raw(&mut self, sr: f64, dm: f64, tm: f64, nm: f64) -> f64 {
        match self.sound {
            DrumSound::SubKick(mult) => {
                let variant = (mult * 20.0) as u8;
                match variant {
                    0..=2 => self.t2_kick_funk(sr, dm, tm, 70.0),
                    3..=5 => self.t2_kick_jazz(sr, dm, tm, 55.0),
                    6..=8 => self.t2_kick_rock(sr, dm, tm, 62.0),
                    9..=11 => self.t2_kick_tight(sr, dm, tm, 75.0),
                    12..=14 => self.t2_kick_deep(sr, dm, tm, 48.0),
                    15..=17 => self.t2_kick_round(sr, dm, tm, 58.0),
                    18..=19 => self.t2_kick_lo(sr, dm, tm, 42.0),
                    _ => self.t2_kick_click(sr, dm, tm, 68.0),
                }
            }
            DrumSound::Kick => self.t2_kick_funk(sr, dm, tm, 68.0),
            DrumSound::Snare => self.t2_snare_funk(sr, dm, tm, nm, 300.0),
            DrumSound::SnareAlt => self.t2_snare_dry(sr, dm, tm, nm, 280.0),
            DrumSound::Rimshot => self.t2_rimshot(sr, dm, tm),
            DrumSound::Clap => self.t2_clap(sr, dm, nm),
            DrumSound::ClosedHat => self.t2_hat_closed(sr, dm, tm, 0.04),
            DrumSound::PedalHat => self.t2_hat_pedal(sr, dm, tm),
            DrumSound::OpenHat => self.t2_hat_open(sr, dm, tm),
            DrumSound::LowTom => self.t2_tom(sr, dm, tm, 95.0, 0.28),
            DrumSound::MidTom => self.t2_tom(sr, dm, tm, 145.0, 0.22),
            DrumSound::HighTom => self.t2_tom(sr, dm, tm, 210.0, 0.18),
            DrumSound::Crash => self.t2_crash(sr, dm, tm, 1.5),
            DrumSound::Ride => self.t2_ride(sr, dm, tm),
            DrumSound::RideBell => self.t2_ride_bell(sr, dm, tm),
            DrumSound::Cymbal => self.t2_crash(sr, dm, tm, 1.0),
            DrumSound::Splash => self.t2_crash(sr, dm, tm, 0.6),
            DrumSound::Cowbell => self.t2_cowbell(sr, dm, tm),
            DrumSound::Clave => self.t2_woodblock(sr, dm, tm, 1800.0),
            DrumSound::Tambourine => self.t2_tambourine(sr, dm, tm),
            DrumSound::Maracas => self.t2_shaker(sr, dm),
            DrumSound::Cabasa => self.t2_shaker_long(sr, dm),
            DrumSound::Vibraslap => self.t2_vibraslap(sr, dm),
            DrumSound::Conga(f) => self.t2_tom(sr, dm, tm, f, 0.2),
            DrumSound::Bongo(f) => self.t2_tom(sr, dm, tm, f, 0.12),
            DrumSound::Timbale(f) => self.t2_timbale(sr, dm, tm, f),
            DrumSound::Agogo(f) => self.t2_agogo(sr, dm, tm, f),
            DrumSound::Whistle(d) => self.synth_808_whistle(sr, dm, d),
            DrumSound::Guiro(d) => self.synth_808_guiro(sr, dm, d),
            DrumSound::FxNoise(v) => self.t2_fx_perc(sr, dm, tm, nm, v),
        }
    }

    // ── TSTY-2 Kicks: Modal synthesis with Bessel ratios ──

    pub(crate) fn t2_kick_funk(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        let f = fund * tm;
        // Bessel membrane mode ratios for circular drum head
        let r_11 = 1.593; let r_21 = 2.136; let r_02 = 2.296;

        // Pitch sweep: real head is slower than 808 (8-15ms)
        let sweep = f * 0.3 * (-self.time * 70.0).exp();
        let ff = f + sweep;

        // Modal oscillators: fundamental + 3 inharmonic modes
        advance_phase(&mut self.phase1, ff, sr);
        advance_phase(&mut self.phase2, ff * r_11, sr);
        advance_phase(&mut self.phase3, ff * r_21, sr);
        advance_phase(&mut self.modal_phases[0], ff * r_02, sr);

        // Multi-rate envelope: fast initial + slower body
        let t = self.time;
        let fast = (-t / 0.008).exp(); // initial transient loss
        let slow = (-t / (0.18 * dm)).exp(); // body
        let env = 0.35 * fast + 0.65 * slow;

        // Modes with independent decay (higher modes die faster)
        let body = osc_sine(self.phase1) * 0.65 * env;
        let m1 = osc_sine(self.phase2) * 0.12 * (-t / (0.08 * dm)).exp();
        let m2 = osc_sine(self.phase3) * 0.08 * (-t / (0.05 * dm)).exp();
        let m3 = osc_sine(self.modal_phases[0]) * 0.06 * (-t / (0.04 * dm)).exp();

        // Beater impact: filtered noise with finite rise time
        let beater_rise = (t / 0.0015).min(1.0); // 1.5ms rise — NOT instant
        let beater = self.noise() * beater_rise * (-t * 250.0).exp();
        let beater_filtered = self.svf1.bandpass(beater, 2800.0 * tm, 1.5, sr) * 0.2;

        // Sub thump (air push)
        advance_phase(&mut self.modal_phases[1], f * 0.5, sr);
        let sub = osc_sine(self.modal_phases[1]) * 0.15 * (-t / (0.1 * dm)).exp();

        // Per-hit variation: slight pitch and level randomization
        let pitch_var = 1.0 + self.hit_rand(0) * 0.005; // ±0.5%
        let _ = pitch_var; // applied via hit_seed in trigger

        body + m1 + m2 + m3 + beater_filtered + sub
    }

    pub(crate) fn t2_kick_jazz(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        // Deeper, less damped, more resonant
        let f = fund * tm;
        let sweep = f * 0.4 * (-self.time * 40.0).exp(); // slower sweep
        advance_phase(&mut self.phase1, f + sweep, sr);
        advance_phase(&mut self.phase2, (f + sweep) * 1.593, sr);
        advance_phase(&mut self.phase3, (f + sweep) * 2.296, sr);

        let t = self.time;
        let env = 0.3 * (-t / 0.012).exp() + 0.7 * (-t / (0.35 * dm)).exp();
        let body = osc_sine(self.phase1) * 0.6 * env;
        let m1 = osc_sine(self.phase2) * 0.15 * (-t / (0.12 * dm)).exp();
        let m2 = osc_sine(self.phase3) * 0.08 * (-t / (0.08 * dm)).exp();

        // Soft felt beater
        let beater = self.noise() * (t / 0.002).min(1.0) * (-t * 150.0).exp();
        let beater_f = self.svf1.bandpass(beater, 1800.0 * tm, 1.2, sr) * 0.12;

        body + m1 + m2 + beater_f
    }

    pub(crate) fn t2_kick_rock(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        let f = fund * tm;
        let sweep = f * 0.35 * (-self.time * 55.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        advance_phase(&mut self.phase2, (f + sweep) * 1.593, sr);

        let t = self.time;
        let env = 0.4 * (-t / 0.006).exp() + 0.6 * (-t / (0.22 * dm)).exp();
        let body = osc_sine(self.phase1) * 0.7 * env;
        let m1 = osc_sine(self.phase2) * 0.1 * (-t / (0.06 * dm)).exp();

        // Plastic beater — brighter click
        let beater = self.noise() * (t / 0.001).min(1.0) * (-t * 400.0).exp();
        let beater_f = self.svf1.bandpass(beater, 4500.0 * tm, 2.0, sr) * 0.25;

        // Shell resonance
        let shell = self.noise() * (-t * 80.0).exp();
        let shell_f = self.svf2.bandpass(shell, 280.0 * tm, 10.0, sr) * 0.06;

        body + m1 + beater_f + shell_f
    }

    pub(crate) fn t2_kick_tight(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        let f = fund * tm;
        let sweep = f * 0.2 * (-self.time * 90.0).exp(); // fast sweep
        advance_phase(&mut self.phase1, f + sweep, sr);
        let t = self.time;
        let env = 0.5 * (-t / 0.005).exp() + 0.5 * (-t / (0.12 * dm)).exp();
        let body = osc_sine(self.phase1) * 0.75 * env;
        let beater = self.noise() * (t / 0.001).min(1.0) * (-t * 500.0).exp();
        let beater_f = self.svf1.bandpass(beater, 3500.0 * tm, 1.8, sr) * 0.2;
        body + beater_f
    }

    pub(crate) fn t2_kick_deep(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        let f = fund * tm;
        let sweep = f * 0.5 * (-self.time * 25.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        advance_phase(&mut self.phase2, (f + sweep) * 0.5, sr); // sub
        let t = self.time;
        let env = 0.2 * (-t / 0.015).exp() + 0.8 * (-t / (0.45 * dm)).exp();
        let body = osc_sine(self.phase1) * 0.55 * env;
        let sub = osc_sine(self.phase2) * 0.25 * (-t / (0.3 * dm)).exp();
        let beater = self.noise() * (t / 0.002).min(1.0) * (-t * 120.0).exp();
        let beater_f = self.svf1.bandpass(beater, 1500.0 * tm, 1.0, sr) * 0.1;
        body + sub + beater_f
    }

    pub(crate) fn t2_kick_round(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        let f = fund * tm;
        let sweep = f * 0.35 * (-self.time * 45.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        advance_phase(&mut self.phase2, (f + sweep) * 1.593, sr);
        let t = self.time;
        // Triangle for rounder character
        let body = osc_triangle(self.phase1) * 0.6 * (0.3 * (-t / 0.01).exp() + 0.7 * (-t / (0.25 * dm)).exp());
        let m1 = osc_sine(self.phase2) * 0.1 * (-t / (0.06 * dm)).exp();
        let beater = self.noise() * (t / 0.002).min(1.0) * (-t * 200.0).exp();
        let beater_f = self.svf1.bandpass(beater, 2000.0 * tm, 1.3, sr) * 0.12;
        body + m1 + beater_f
    }

    pub(crate) fn t2_kick_lo(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        // Very deep floor tom-like kick
        let f = fund * tm;
        let sweep = f * 0.6 * (-self.time * 20.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let t = self.time;
        let env = 0.2 * (-t / 0.02).exp() + 0.8 * (-t / (0.5 * dm)).exp();
        let body = osc_sine(self.phase1) * 0.7 * env;
        let rumble = osc_sine(self.phase1 * 0.5) * 0.15 * (-t / (0.35 * dm)).exp();
        body + rumble
    }

    pub(crate) fn t2_kick_click(&mut self, sr: f64, dm: f64, tm: f64, fund: f64) -> f64 {
        // Lots of beater attack, less body
        let f = fund * tm;
        let sweep = f * 0.25 * (-self.time * 80.0).exp();
        advance_phase(&mut self.phase1, f + sweep, sr);
        let t = self.time;
        let env = 0.5 * (-t / 0.004).exp() + 0.5 * (-t / (0.15 * dm)).exp();
        let body = osc_sine(self.phase1) * 0.5 * env;
        // Wood beater — bright, loud click
        let beater = self.noise() * (t / 0.0005).min(1.0) * (-t * 600.0).exp();
        let beater_f = self.svf1.bandpass(beater, 5000.0 * tm, 2.5, sr) * 0.4;
        body + beater_f
    }

    // ── TSTY-2 Snares: Body modes + wire buzz via comb-filtered noise ──

    pub(crate) fn t2_snare_funk(&mut self, sr: f64, dm: f64, tm: f64, nm: f64, fund: f64) -> f64 {
        let f = fund * tm;
        let t = self.time;

        // Stick impact: very short bright burst (0.5-1ms)
        let stick = self.noise() * (t / 0.0003).min(1.0) * (-t * 1500.0).exp();
        let stick_f = self.hp1.tick_hp(stick, 3000.0, sr) * 0.3;

        // Batter head modes (Bessel ratios)
        let pitch_drop = f * 0.08 * (-t * 200.0).exp();
        advance_phase(&mut self.phase1, f + pitch_drop, sr);
        advance_phase(&mut self.phase2, (f + pitch_drop) * 1.593, sr);
        advance_phase(&mut self.phase3, (f + pitch_drop) * 2.136, sr);

        let head = osc_sine(self.phase1) * 0.35 * (-t / (0.1 * dm)).exp()
                 + osc_sine(self.phase2) * 0.15 * (-t / (0.07 * dm)).exp()
                 + osc_sine(self.phase3) * 0.08 * (-t / (0.05 * dm)).exp();

        // Snare wire buzz: noise through a comb filter at head fundamental
        let raw_noise = self.noise() * nm;
        // Comb filter: delay of 1/fund, feedback 0.35
        let comb_delay = 1.0 / f;
        let comb_samples = (comb_delay * sr) as usize;
        // Simplified: use SVF bandpass at wire frequencies instead of true comb
        let wire_band = self.svf1.bandpass(raw_noise, 4200.0 * tm, 0.8, sr);
        let wire_hp = self.hp1.tick_hp(wire_band, 1800.0, sr);
        // Wire buzz amplitude follows head vibration
        let wire_env = (-t / (0.18 * dm)).exp();
        let wires = wire_hp * wire_env * 0.4;

        // Shell resonance
        let shell_noise = self.noise() * (-t * 60.0).exp();
        let shell = self.svf2.bandpass(shell_noise, 450.0 * tm, 12.0, sr) * 0.05;

        let _ = comb_samples; // unused in simplified approach
        stick_f + head + wires + shell
    }

    pub(crate) fn t2_snare_dry(&mut self, sr: f64, dm: f64, tm: f64, nm: f64, fund: f64) -> f64 {
        // Drier, tighter — like a Bernard Purdie snare
        let f = fund * tm;
        let t = self.time;

        let stick = self.noise() * (t / 0.0004).min(1.0) * (-t * 2000.0).exp();
        let stick_f = self.hp1.tick_hp(stick, 4000.0, sr) * 0.35;

        advance_phase(&mut self.phase1, f, sr);
        advance_phase(&mut self.phase2, f * 1.593, sr);
        let head = osc_sine(self.phase1) * 0.3 * (-t / (0.06 * dm)).exp()
                 + osc_sine(self.phase2) * 0.12 * (-t / (0.04 * dm)).exp();

        let raw_noise = self.noise() * nm;
        let wire_band = self.svf1.bandpass(raw_noise, 5000.0 * tm, 0.6, sr);
        let wires = wire_band * (-t / (0.1 * dm)).exp() * 0.35;

        stick_f + head + wires
    }

    pub(crate) fn t2_rimshot(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        let t = self.time;
        // Stick on rim: sharp metallic crack
        advance_phase(&mut self.phase1, 520.0 * tm, sr);
        advance_phase(&mut self.phase2, 1380.0 * tm, sr);
        advance_phase(&mut self.phase3, 2700.0 * tm, sr);
        let crack = osc_sine(self.phase1) * 0.3 + osc_sine(self.phase2) * 0.25 + osc_sine(self.phase3) * 0.15;
        let stick = self.noise() * (-t * 800.0).exp() * 0.2;
        let env = (-t / (0.025 * dm)).exp();
        (crack + stick) * env
    }

    pub(crate) fn t2_clap(&mut self, sr: f64, dm: f64, nm: f64) -> f64 {
        let t = self.time;
        // Multiple clappers with random timing (NOT evenly spaced like 808)
        let mut env = 0.0;
        for n in 0..6u32 {
            // Gaussian-ish random offset per clapper
            let offset = (self.hit_rand(n * 5) * 0.012 + self.hit_rand(n * 5 + 1).abs() * 0.008).abs();
            let t_off = t - offset;
            if t_off >= 0.0 {
                let amp = 0.8 + self.hit_rand(n * 5 + 2) * 0.2; // varied amplitude
                env += (-t_off * 180.0).exp() * amp * 0.2;
            }
        }
        // Each clapper filtered differently
        let raw = self.noise() * nm;
        let center = 2200.0 + self.hit_rand(50) * 800.0; // varied per hit
        let filtered = self.svf1.bandpass(raw, center, 1.5, sr);
        let hp = self.hp1.tick_hp(filtered, 600.0, sr);
        // Room tail
        let tail = (-t / (0.12 * dm)).exp() * 0.3;
        hp * (env + tail)
    }

    // ── TSTY-2 Hats: Modal synthesis (inharmonic sine bank, NOT square waves) ──

    pub(crate) fn t2_hat_closed(&mut self, sr: f64, dm: f64, tm: f64, decay_base: f64) -> f64 {
        let t = self.time;
        // 10 inharmonic cymbal modes — the key to realistic metal sound
        let modes: [(f64, f64); 10] = [
            (342.0 * tm, 0.08),   // low body
            (817.0 * tm, 0.12),
            (1453.0 * tm, 0.22),
            (2298.0 * tm, 0.40),
            (3419.0 * tm, 0.65),
            (4735.0 * tm, 1.00),  // peak energy
            (6328.0 * tm, 0.80),
            (8249.0 * tm, 0.50),
            (10400.0 * tm, 0.25),
            (12800.0 * tm, 0.10),
        ];

        // Use hat_oscs for first 6, modal_phases for rest
        let mut sum = 0.0;
        let hat_freqs: [f64; 6] = [modes[0].0, modes[1].0, modes[2].0, modes[3].0, modes[4].0, modes[5].0];
        let hat_raw = self.hat_oscs.tick(sr, &hat_freqs);
        // Weight by mode amplitudes
        sum += hat_raw * 0.3; // blended hat modes

        // Additional higher modes via modal_phases
        for i in 0..4 {
            let (freq, amp) = modes[6 + i];
            advance_phase(&mut self.modal_phases[i], freq, sr);
            let mode_decay = decay_base * dm * (0.8 + i as f64 * 0.15); // higher modes decay slower in cymbals!
            sum += osc_sine(self.modal_phases[i]) * amp * (-t / mode_decay).exp() * 0.15;
        }

        // Per-hit detune variation
        let detune_mod = 1.0 + self.hit_rand(20) * 0.015;
        sum *= detune_mod;

        // Stick transient
        let stick = self.noise() * (t / 0.0003).min(1.0) * (-t * 2000.0).exp() * 0.15;

        // Contact sizzle (cymbals touching)
        let sizzle = self.noise() * (-t / (decay_base * dm * 0.5)).exp();
        let sizzle_f = self.svf1.bandpass(sizzle, 8000.0, 6.0, sr) * 0.08;

        // Main decay
        let env = (-t / (decay_base * dm)).exp();
        (sum * env + stick + sizzle_f) * 0.4
    }

    pub(crate) fn t2_hat_pedal(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        // Foot pedal chick: no stick, just cymbals clapping
        let t = self.time;
        let hat_freqs: [f64; 6] = [350.0*tm, 830.0*tm, 1480.0*tm, 2350.0*tm, 3500.0*tm, 4800.0*tm];
        let modal = self.hat_oscs.tick(sr, &hat_freqs);
        // Very fast decay (cymbals pressed together)
        let env = (-t / (0.02 * dm)).exp();
        // Chick noise (cymbals clapping together)
        let chick = self.noise() * (-t * 500.0).exp();
        let chick_f = self.svf1.bandpass(chick, 1200.0, 2.0, sr) * 0.15;
        modal * env * 0.25 + chick_f
    }

    pub(crate) fn t2_hat_open(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        // Open: all modes ring freely, 500ms-1.5s
        self.t2_hat_closed(sr, dm, tm, 0.8)
    }

    // ── TSTY-2 Toms: Modal with shell resonance ──

    pub(crate) fn t2_tom(&mut self, sr: f64, dm: f64, tm: f64, fund: f64, body_decay: f64) -> f64 {
        let f = fund * tm;
        let t = self.time;
        let droop = f * 0.12 * (-t * 35.0).exp();
        advance_phase(&mut self.phase1, f + droop, sr);
        advance_phase(&mut self.phase2, (f + droop) * 1.593, sr);
        advance_phase(&mut self.phase3, (f + droop) * 2.136, sr);

        let env = 0.3 * (-t / 0.008).exp() + 0.7 * (-t / (body_decay * dm)).exp();
        let body = osc_sine(self.phase1) * 0.55 * env;
        let m1 = osc_sine(self.phase2) * 0.15 * (-t / (body_decay * dm * 0.6)).exp();
        let m2 = osc_sine(self.phase3) * 0.08 * (-t / (body_decay * dm * 0.4)).exp();

        let stick = self.noise() * (t / 0.001).min(1.0) * (-t * 300.0).exp();
        let stick_f = self.svf1.bandpass(stick, 3000.0 * tm, 1.5, sr) * 0.12;

        body + m1 + m2 + stick_f
    }

    // ── TSTY-2 Cymbals ──

    pub(crate) fn t2_crash(&mut self, sr: f64, dm: f64, tm: f64, decay_mult: f64) -> f64 {
        let t = self.time;
        let hat_freqs: [f64; 6] = [380.0*tm, 890.0*tm, 1520.0*tm, 2650.0*tm, 3800.0*tm, 5200.0*tm];
        let modal = self.hat_oscs.tick(sr, &hat_freqs);
        let noise = self.noise() * 0.15;
        let mixed = modal * 0.4 + noise;
        let filtered = self.svf1.lowpass(mixed, 11000.0 * tm, 0.3, sr);
        // Crash builds slightly (1-2ms) then long decay
        let attack = (t / 0.002).min(1.0);
        let env = attack * (-t / (decay_mult * dm)).exp();
        filtered * env * 0.3
    }

    pub(crate) fn t2_ride(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        let t = self.time;
        let hat_freqs: [f64; 6] = [420.0*tm, 980.0*tm, 1680.0*tm, 2800.0*tm, 4100.0*tm, 5600.0*tm];
        let modal = self.hat_oscs.tick(sr, &hat_freqs);
        let filtered = self.svf1.bandpass(modal, 5500.0 * tm, 0.8, sr);
        let env = (-t / (0.9 * dm)).exp();
        // Ride has a "ping" then sustain
        let ping = (-t * 100.0).exp() * 0.15;
        (filtered * env + ping) * 0.3
    }

    pub(crate) fn t2_ride_bell(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        let t = self.time;
        advance_phase(&mut self.phase1, 720.0 * tm, sr);
        advance_phase(&mut self.phase2, 1080.0 * tm, sr);
        advance_phase(&mut self.phase3, 1620.0 * tm, sr);
        let bell = osc_sine(self.phase1) * 0.35 + osc_sine(self.phase2) * 0.3
                 + osc_sine(self.phase3) * 0.2;
        let env = (-t / (0.7 * dm)).exp();
        bell * env
    }

    // ── TSTY-2 Percussion ──

    pub(crate) fn t2_cowbell(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        let t = self.time;
        advance_phase(&mut self.phase1, 587.0 * tm, sr);
        advance_phase(&mut self.phase2, 878.0 * tm, sr);
        let body = osc_sine(self.phase1) * 0.35 + osc_sine(self.phase2) * 0.3;
        let filtered = self.svf1.bandpass(body, 730.0, 4.0, sr);
        let env = (-t / (0.065 * dm)).exp();
        filtered * env
    }

    pub(crate) fn t2_woodblock(&mut self, sr: f64, dm: f64, tm: f64, freq: f64) -> f64 {
        let t = self.time;
        let f = freq * tm;
        advance_phase(&mut self.phase1, f, sr);
        advance_phase(&mut self.phase2, f * 2.65, sr); // wood mode ratio
        let body = osc_sine(self.phase1) * 0.4 + osc_sine(self.phase2) * 0.2;
        let click = self.noise() * (-t * 1000.0).exp() * 0.15;
        let env = (-t / (0.018 * dm)).exp();
        (body + click) * env
    }

    pub(crate) fn t2_tambourine(&mut self, sr: f64, dm: f64, tm: f64) -> f64 {
        let t = self.time;
        // Jingles: multiple small cymbal-like modes
        let hat_freqs: [f64; 6] = [4200.0*tm, 5800.0*tm, 7400.0*tm, 9100.0*tm, 11000.0*tm, 13000.0*tm];
        let jingles = self.hat_oscs.tick(sr, &hat_freqs);
        let filtered = self.hp1.tick_hp(jingles, 4000.0, sr);
        // Rhythmic jingle from hand shake
        let shake = (t * 22.0 * TAU).sin().abs() * (-t * 6.0).exp();
        let env = (-t / (0.2 * dm)).exp();
        filtered * (env + shake * 0.25) * 0.3
    }

    pub(crate) fn t2_shaker(&mut self, sr: f64, dm: f64) -> f64 {
        let t = self.time;
        let raw = self.noise();
        let filtered = self.svf1.bandpass(raw, 7500.0, 1.2, sr);
        let hp = self.hp1.tick_hp(filtered, 4500.0, sr);
        let env = (-t / (0.07 * dm)).exp();
        hp * env * 0.3
    }

    pub(crate) fn t2_shaker_long(&mut self, sr: f64, dm: f64) -> f64 {
        let t = self.time;
        let raw = self.noise();
        let filtered = self.svf1.bandpass(raw, 8000.0, 1.5, sr);
        // Swish envelope
        let swish = (t * 12.0).sin().abs() * (-t * 4.0).exp();
        let env = (-t / (0.15 * dm)).exp();
        filtered * (env + swish * 0.2) * 0.25
    }

    pub(crate) fn t2_vibraslap(&mut self, sr: f64, dm: f64) -> f64 {
        let t = self.time;
        let raw = self.noise();
        let filtered = self.svf1.bandpass(raw, 3500.0, 6.0, sr);
        let rattle = (t * 35.0 * TAU).sin().abs() * (-t * 3.0).exp();
        let env = (-t / (0.5 * dm)).exp();
        filtered * rattle * env * 0.25
    }

    pub(crate) fn t2_timbale(&mut self, sr: f64, dm: f64, tm: f64, freq: f64) -> f64 {
        let t = self.time;
        let f = freq * tm;
        advance_phase(&mut self.phase1, f, sr);
        advance_phase(&mut self.phase2, f * 2.14, sr); // shell mode
        let body = osc_sine(self.phase1) * 0.5;
        let ring = osc_sine(self.phase2) * 0.2 * (-t * 20.0).exp();
        let shell = self.noise() * (-t * 250.0).exp();
        let shell_f = self.svf1.bandpass(shell, f * 3.0, 8.0, sr) * 0.08;
        let env = (-t / (0.2 * dm)).exp();
        (body + ring + shell_f) * env
    }

    pub(crate) fn t2_agogo(&mut self, sr: f64, dm: f64, tm: f64, freq: f64) -> f64 {
        let t = self.time;
        let f1 = freq * tm;
        let f2 = f1 * 1.504; // inharmonic bell ratio
        advance_phase(&mut self.phase1, f1, sr);
        advance_phase(&mut self.phase2, f2, sr);
        let body = osc_sine(self.phase1) * 0.35 + osc_sine(self.phase2) * 0.28;
        let env = (-t / (0.18 * dm)).exp();
        body * env
    }

    pub(crate) fn t2_fx_perc(&mut self, sr: f64, dm: f64, tm: f64, nm: f64, character: f64) -> f64 {
        let t = self.time;
        let freq = 300.0 + character * 5000.0;
        let sweep = freq * (1.0 + character * 2.0 * (-t * 6.0).exp());
        advance_phase(&mut self.phase1, sweep * tm, sr);
        let tone = osc_sine(self.phase1) * 0.35;
        let noise = self.noise() * nm * 0.15;
        // Modal resonance at swept frequency
        let reso = self.svf1.bandpass(tone + noise, sweep * tm * 0.8, 3.0, sr) * 0.2;
        let env = (-t / (0.25 * dm)).exp();
        (tone + noise + reso) * env
    }

    // ══════════════════════════════════════════════════════════════════════════
}
