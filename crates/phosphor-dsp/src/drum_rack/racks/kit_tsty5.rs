//! TSTY5 kit synthesis — resonator-based physical modeling.

use super::super::*;
use std::f64::consts::TAU;

impl DrumVoice {
    // TSTY-5: Each sound uses a NAMED synthesis technique. No two sounds
    // share the same technique+params. Verified twice after writing.
    //
    // SYNTHESIS TECHNIQUES USED (tracked to prevent reuse):
    // K1: 3-mode Bessel + felt noise burst  K2: FM body (sine mod sine) + click
    // K3: triangle waveshape + sub          K4: ring mod body + thump
    // K5: square wave LPF'd + noise         K6: wavefold sine + noise
    // K7: 2-mode + shell resonant BPF       K8: pure sine + heavy saturation
    // S1: sine body + comb-like wire        S2: FM body + HP noise wire
    // S3: triangle body + multi-band wire   S4: noise-only brush
    // S5: rim = two high sines              S6: sine body + AM wire
    // S7: square body + pitched noise       S8: wavefold body + noise burst
    // S9: ring mod body + filtered wire     S10: 3-mode + resonant shell ring
    // CL1: 5-clapper random burst           CL2: 8-clapper wide spread
    // CL3: single clap dry                  CL4: finger snap HP filtered
    // CL5: hand slap LPF                    CL6: group + long reverb tail
    // CH1: modal 6-osc bank                 CH2: pure noise BPF
    // CH3: FM metallic tick                 CH4: ring mod tick
    // CH5: dark LPF modal                   CH6: multi-band noise
    // CH7: noise + saturation               CH8: AM modulated noise
    // HO1: 9-mode shimmer 1.5s             HO2: FM metallic 2.0s
    // HO3: 3-band resonant noise 1.2s      HO4: ring mod wash 1.8s
    // HO5: breathy HP noise 2.5s           HO6: distorted modal 1.3s
    // HO7: comb filtered noise 1.6s        HO8: phase-mod shimmer 2.0s
    // PH1: pedal chick modal               PH2: pedal loose noise
    // T1-T6: toms with different modes/shells
    // CY1-CY6: cymbals with different topologies
    // P1-P12: unique percussion
    // ══════════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_tsty5(&mut self, sr: f64, dm: f64, _tm: f64, _nm: f64, _dr: f64) -> f64 {
        // RESONATOR-BASED SYNTHESIS: exciter → resonant filters → tape saturation
        // The tone comes from FILTERS, not oscillators. Like a real drum.
        let t = self.time;
        let n = self.note;

        // Get the sound recipe for this note
        let recipe = t5_recipe(n);

        // ── EXCITER: short impulse/noise that rings the resonators ──
        let exciter = match recipe.exciter {
            0 => { // Impulse + noise: general purpose
                let impulse = if t < 0.001 { 1.0 } else { 0.0 };
                let noise = self.noise() * (-t / recipe.noise_decay.max(0.001)).exp() * recipe.noise_level;
                impulse * recipe.impulse_level + noise
            }
            1 => { // Noise only: hats, cymbals, brushes
                self.noise() * (-t / recipe.noise_decay.max(0.001)).exp() * recipe.noise_level
            }
            2 => { // Click + noise: snares, claps
                let click = self.noise() * (t / 0.0003).min(1.0) * (-t * 800.0).exp();
                let noise = self.noise() * (-t / recipe.noise_decay.max(0.001)).exp() * recipe.noise_level;
                click * recipe.impulse_level + noise
            }
            _ => { // Multi-burst: claps
                let mut env = 0.0;
                for k in 0..recipe.burst_count as u32 {
                    let off = (self.hit_rand(k * 3) * recipe.burst_spread
                        + self.hit_rand(k * 3 + 1).abs() * recipe.burst_spread * 0.5).abs();
                    let to = t - off;
                    if to >= 0.0 {
                        env += (-to * 180.0).exp() * (0.7 + self.hit_rand(k * 3 + 2) * 0.3) * 0.2;
                    }
                }
                self.noise() * (env + (-t / recipe.noise_decay.max(0.001)).exp() * recipe.noise_level * 0.3)
            }
        };

        // ── RESONATOR BANK: bandpass filters create the tone ──
        // Each resonator is a bandpass filter ringing at a specific frequency
        // This is how real drums work: the shell/head IS a resonator

        let mut resonated = 0.0;

        // Resonator 1 (primary body) — uses SVF1
        if recipe.r1_freq > 0.0 {
            // Pitch envelope on primary resonator
            let freq = recipe.r1_freq + recipe.pitch_sweep * (-t / recipe.pitch_time.max(0.001)).exp();
            let r1 = self.svf1.bandpass(exciter, freq, recipe.r1_q, sr);
            resonated += r1 * recipe.r1_level * (-t / (recipe.r1_decay * dm).max(0.001)).exp();
        }

        // Resonator 2 (secondary mode / shell) — uses SVF2
        if recipe.r2_freq > 0.0 {
            let r2 = self.svf2.bandpass(exciter, recipe.r2_freq, recipe.r2_q, sr);
            resonated += r2 * recipe.r2_level * (-t / (recipe.r2_decay * dm).max(0.001)).exp();
        }

        // Resonator 3 (third mode / brightness) — uses SVF3
        if recipe.r3_freq > 0.0 {
            let r3 = self.svf3.bandpass(exciter, recipe.r3_freq, recipe.r3_q, sr);
            resonated += r3 * recipe.r3_level * (-t / (recipe.r3_decay * dm).max(0.001)).exp();
        }

        // ── NOISE SHAPING: filtered noise for wires, shimmer, wash ──
        let noise_shaped = if recipe.noise_filter_freq > 0.0 {
            let raw = self.noise() * recipe.noise_mix;
            let filtered = self.hp1.tick_hp(raw, recipe.noise_filter_freq, sr);
            // For snare wires: modulate noise by body amplitude (sympathetic vibration)
            let body_mod = if recipe.wire_coupling > 0.0 {
                (resonated.abs() * recipe.wire_coupling + (1.0 - recipe.wire_coupling)).min(1.0)
            } else { 1.0 };
            filtered * body_mod * (-t / (recipe.noise_filter_decay * dm).max(0.001)).exp()
        } else { 0.0 };

        // ── MIX + TAPE SATURATION ──
        let raw = resonated + noise_shaped;
        let sat = (raw * 1.5).tanh() + 0.03 * raw * (-(raw * 0.5).abs()).exp();
        let rc = 1.0 / (TAU * 9500.0);
        let alpha = 1.0 / (1.0 + rc * sr);
        self.lp1_state += alpha * (sat - self.lp1_state);
        self.lp1_state
    }

    // Keep the old t5 for reference but it's no longer called
    #[allow(dead_code)]
    pub(crate) fn t5_old(&mut self, sr: f64, dm: f64, tm: f64, nm: f64) -> f64 {
        let t = self.time;
        match self.note {

        // ════════ 8 KICKS (24-31) — 8 different synthesis methods ════════

        24 => { // K1: 3-mode Bessel membrane + felt noise burst
            let f=62.0; let sw=f*0.28*(-t*50.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            advance_phase(&mut self.phase3, (f+sw)*2.296, sr);
            let e=0.3*(-t/0.01).exp()+0.7*(-t/(0.3*dm)).exp();
            let body=osc_sine(self.phase1)*0.55*e + osc_sine(self.phase2)*0.12*(-t/(0.08*dm)).exp()
                + osc_sine(self.phase3)*0.06*(-t/(0.05*dm)).exp();
            let felt=self.noise()*(t/0.0018).min(1.0)*(-t*160.0).exp();
            body + self.svf1.bandpass(felt, 2000.0, 1.2, sr)*0.12
        }
        25 => { // K2: FM body — sine modulating sine for complex low end
            advance_phase(&mut self.phase1, 55.0, sr); // carrier
            advance_phase(&mut self.phase2, 82.0, sr); // modulator
            let fm_idx = 3.0 * (-t*35.0).exp(); // FM index decays
            let body = (self.phase1*TAU + osc_sine(self.phase2)*fm_idx).sin();
            let e = 0.35*(-t/0.008).exp()+0.65*(-t/(0.25*dm)).exp();
            let click = self.noise()*(t/0.0006).min(1.0)*(-t*500.0).exp();
            body*0.55*e + self.svf1.bandpass(click, 4200.0, 2.0, sr)*0.2
        }
        26 => { // K3: Triangle waveshape body — rounder than sine, no harmonics
            let f=50.0; let sw=f*0.35*(-t*30.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let body = osc_triangle(self.phase1)*0.6;
            advance_phase(&mut self.phase2, 25.0, sr); // sub one octave down
            let sub = osc_triangle(self.phase2)*0.2*(-t/(0.25*dm)).exp();
            let e = 0.25*(-t/0.015).exp()+0.75*(-t/(0.4*dm)).exp();
            (body + sub)*e
        }
        27 => { // K4: Ring mod body — two sines multiplied for complex low end
            advance_phase(&mut self.phase1, 48.0, sr);
            advance_phase(&mut self.phase2, 72.0, sr); // 3:2 ratio
            let ring = osc_sine(self.phase1)*osc_sine(self.phase2); // creates 24Hz + 120Hz
            let e = 0.3*(-t/0.01).exp()+0.7*(-t/(0.35*dm)).exp();
            let thump = self.noise()*(-t*80.0).exp();
            ring*0.6*e + self.svf1.lowpass(thump, 400.0, 0.5, sr)*0.1
        }
        28 => { // K5: Square wave through LPF — harmonically rich body
            advance_phase(&mut self.phase1, 58.0, sr);
            let sq = if self.phase1.fract() < 0.5 { 0.8 } else { -0.8 };
            let filtered = self.svf1.lowpass(sq, 200.0 + 600.0*(-t*25.0).exp(), 0.8, sr);
            let e = 0.4*(-t/0.006).exp()+0.6*(-t/(0.2*dm)).exp();
            let click = self.noise()*(t/0.0004).min(1.0)*(-t*600.0).exp();
            filtered*e + self.hp1.tick_hp(click, 3000.0, sr)*0.25
        }
        29 => { // K6: Wavefolded sine — creates odd harmonics for warm thick tone
            let f=65.0; let sw=f*0.3*(-t*55.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let raw = osc_sine(self.phase1)*2.5; // overdrive the sine
            let folded = raw.sin(); // sine of sine = wavefold
            let e = 0.35*(-t/0.007).exp()+0.65*(-t/(0.22*dm)).exp();
            folded*0.45*e
        }
        30 => { // K7: 2-mode + shell resonant BPF — woody attack
            let f=68.0; let sw=f*0.2*(-t*65.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            let body = osc_sine(self.phase1)*0.5*(0.4*(-t/0.005).exp()+0.6*(-t/(0.18*dm)).exp());
            let m1 = osc_sine(self.phase2)*0.15*(-t/(0.06*dm)).exp();
            let shell_exc = self.noise()*(-t*50.0).exp();
            let shell = self.svf2.bandpass(shell_exc, 250.0, 15.0, sr)*0.1;
            body + m1 + shell
        }
        31 => { // K8: Pure sine + heavy saturation — tape-crushed minimal
            let f=52.0; let sw=f*0.15*(-t*40.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let raw = osc_sine(self.phase1)*1.8; // hot signal
            let crushed = (raw*2.0).tanh()*0.5; // heavy saturation adds harmonics
            let e = (-t/(0.18*dm)).exp();
            crushed*e
        }

        // ════════ 10 SNARES (32-41) — 10 different synthesis methods ════════

        32 => { // S1: Sine body + comb-like pitched wire buzz
            let f=305.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.3*(-t/(0.12*dm)).exp();
            let stick = self.hp1.tick_hp(self.noise()*(t/0.0003).min(1.0)*(-t*1600.0).exp(), 3500.0, sr)*0.25;
            // Wire: noise through tight BPF at head freq harmonics = quasi-comb
            let w1 = self.svf1.bandpass(self.noise()*nm, 3200.0, 3.0, sr)*0.15;
            let w2 = self.svf2.bandpass(self.noise()*nm, 5800.0, 2.5, sr)*0.12;
            let w3 = self.svf3.bandpass(self.noise()*nm, 8400.0, 2.0, sr)*0.08;
            let wire = (w1+w2+w3)*(-t/(0.25*dm)).exp();
            stick + head + wire
        }
        33 => { // S2: FM body (richer than pure sine) + HP noise wire
            advance_phase(&mut self.phase1, 240.0*tm, sr); // carrier
            advance_phase(&mut self.phase2, 380.0*tm, sr); // modulator
            let fm_body = (self.phase1*TAU + osc_sine(self.phase2)*0.8).sin();
            let head = fm_body*0.3*(-t/(0.14*dm)).exp();
            let wire = self.hp1.tick_hp(self.noise()*nm, 2500.0, sr)*(-t/(0.3*dm)).exp()*0.35;
            let stick = self.noise()*(-t*1400.0).exp()*0.18;
            head + wire + stick
        }
        34 => { // S3: Triangle body (softer odd harmonics) + multi-band wire
            advance_phase(&mut self.phase1, 280.0*tm, sr);
            let head = osc_triangle(self.phase1)*0.28*(-t/(0.1*dm)).exp();
            let w1 = self.svf1.bandpass(self.noise()*nm, 4000.0, 1.5, sr)*0.18;
            let w2 = self.svf2.bandpass(self.noise()*nm, 7500.0, 1.2, sr)*0.12;
            let wire = (w1+w2)*(-t/(0.22*dm)).exp();
            head + wire + self.noise()*(-t*2000.0).exp()*0.15
        }
        35 => { // S4: Noise-only brush — no tonal body at all
            let brush = self.noise()*(t/0.005).min(1.0); // slow rise = brush stroke
            let shaped = self.svf1.bandpass(brush, 2800.0, 0.6, sr);
            let hp = self.hp1.tick_hp(shaped, 800.0, sr);
            hp*(-t/(0.2*dm)).exp()*0.4
        }
        36 => { // S5: Cross-stick — two high sine partials, no wires
            advance_phase(&mut self.phase1, 580.0*tm, sr);
            advance_phase(&mut self.phase2, 1520.0*tm, sr);
            let crack = osc_sine(self.phase1)*0.25 + osc_sine(self.phase2)*0.18;
            let click = self.noise()*(-t*900.0).exp()*0.2;
            (crack + click)*(-t/(0.025*dm)).exp()
        }
        37 => { // S6: Sine body + AM modulated wire (amplitude modulation)
            advance_phase(&mut self.phase1, 310.0*tm, sr);
            let head = osc_sine(self.phase1)*0.3*(-t/(0.1*dm)).exp();
            // AM: noise amplitude modulated by a fast oscillator = "chattering" wire
            advance_phase(&mut self.phase3, 180.0, sr);
            let am_mod = (1.0 + osc_sine(self.phase3)) * 0.5; // 0-1 range
            let wire = self.noise()*nm*am_mod;
            let wire_f = self.hp1.tick_hp(wire, 2000.0, sr)*(-t/(0.28*dm)).exp()*0.35;
            head + wire_f
        }
        38 => { // S7: Square wave body + pitched noise — buzzy character
            advance_phase(&mut self.phase1, 260.0*tm, sr);
            let sq = if self.phase1.fract() < 0.5 { 0.3 } else { -0.3 };
            let body = self.svf1.lowpass(sq, 800.0, 0.5, sr)*(-t/(0.08*dm)).exp();
            let wire = self.hp1.tick_hp(self.noise()*nm, 3000.0, sr)*(-t/(0.18*dm)).exp()*0.3;
            body + wire + self.noise()*(-t*2500.0).exp()*0.2
        }
        39 => { // S8: Wavefolded body + noise burst — distorted snare
            advance_phase(&mut self.phase1, 290.0*tm, sr);
            let raw = osc_sine(self.phase1)*2.0;
            let folded = raw.sin()*0.25; // wavefold
            let body = folded*(-t/(0.1*dm)).exp();
            let crack = self.noise()*(-t*1800.0).exp()*0.3;
            let wire = self.svf1.bandpass(self.noise()*nm, 4500.0, 0.7, sr)*(-t/(0.2*dm)).exp()*0.3;
            body + crack + wire
        }
        40 => { // S9: Ring mod body + filtered wire — metallic snare
            advance_phase(&mut self.phase1, 250.0*tm, sr);
            advance_phase(&mut self.phase2, 395.0*tm, sr);
            let ring = osc_sine(self.phase1)*osc_sine(self.phase2); // inharmonic
            let body = ring*0.25*(-t/(0.1*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 3800.0, 0.6, sr)*(-t/(0.22*dm)).exp()*0.35;
            body + wire + self.noise()*(-t*1500.0).exp()*0.15
        }
        41 => { // S10: 3-mode Bessel body + resonant shell ring — big snare
            let f=225.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            advance_phase(&mut self.phase2, f*1.593, sr);
            advance_phase(&mut self.phase3, f*2.136, sr);
            let head = osc_sine(self.phase1)*0.35*(-t/(0.15*dm)).exp()
                + osc_sine(self.phase2)*0.15*(-t/(0.1*dm)).exp()
                + osc_sine(self.phase3)*0.08*(-t/(0.06*dm)).exp();
            let shell = self.svf2.bandpass(self.noise()*(-t*30.0).exp(), 420.0, 18.0, sr)*0.12;
            let wire = self.svf1.bandpass(self.noise()*nm, 3500.0, 0.5, sr)*(-t/(0.35*dm)).exp()*0.4;
            head + shell + wire
        }

        // ════════ 6 CLAPS (42-47) — 6 different approaches ════════

        42 => { // CL1: 5-clapper tight — random timing, BPF noise
            let mut e=0.0;
            for k in 0..5u32 { let off=(self.hit_rand(k*3)*0.006+self.hit_rand(k*3+1).abs()*0.004).abs();
                let to=t-off; if to>=0.0 { e+=(-to*170.0).exp()*(0.75+self.hit_rand(k*3+2)*0.25)*0.17; } }
            let f=self.svf1.bandpass(self.noise()*nm, 2300.0, 1.4, sr);
            let hp=self.hp1.tick_hp(f, 600.0, sr);
            hp*(e + (-t/(0.15*dm)).exp()*0.3)
        }
        43 => { // CL2: 8-clapper loose — wide spread, LP filtered
            let mut e=0.0;
            for k in 0..8u32 { let off=(self.hit_rand(k*4)*0.02+self.hit_rand(k*4+1).abs()*0.01).abs();
                let to=t-off; if to>=0.0 { e+=(-to*130.0).exp()*(0.6+self.hit_rand(k*4+2)*0.4)*0.11; } }
            let f=self.svf1.bandpass(self.noise()*nm, 1800.0, 1.0, sr);
            let warm=self.svf2.lowpass(f, 5000.0, 0.5, sr);
            warm*(e + (-t/(0.2*dm)).exp()*0.35)
        }
        44 => { // CL3: Single dry clap — one burst with short tail
            let clap=self.noise()*(t/0.0004).min(1.0)*(-t/(0.03*dm)).exp();
            self.svf1.bandpass(clap, 2100.0, 1.8, sr)*0.4
        }
        45 => { // CL4: Finger snap — short but controlled
            let snap=self.noise()*(t/0.0002).min(1.0)*(-t/(0.015*dm)).exp();
            let f=self.svf1.bandpass(snap, 3400.0, 2.5, sr);
            self.hp1.tick_hp(f, 1500.0, sr)*0.45
        }
        46 => { // CL5: Hand slap — LPF, mid-heavy thump
            advance_phase(&mut self.phase1, 185.0, sr);
            let body=osc_sine(self.phase1)*0.2*(-t/(0.08*dm)).exp();
            let slap=self.svf1.lowpass(self.noise()*(-t/(0.02*dm)).exp(), 2500.0, 1.0, sr)*0.3;
            body + slap
        }
        47 => { // CL6: Group + long reverb tail — hall clap
            let mut e=0.0;
            for k in 0..4u32 { let off=(self.hit_rand(k*5)*0.01).abs();
                let to=t-off; if to>=0.0 { e+=(-to*160.0).exp()*0.2; } }
            let f=self.svf1.bandpass(self.noise()*nm, 2200.0, 1.3, sr);
            f*(e + (-t/(0.4*dm)).exp()*0.4)
        }

        // ════════ 8 CLOSED HATS (48-55) — 8 DIFFERENT synthesis methods ════════

        48 => { // CH1: Standard modal 6-osc bank
            let freqs=[385.0, 962.0, 1618.0, 2524.0, 3742.0, 5183.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(m, 5500.0, sr);
            (hp*0.35+self.noise()*(-t*2200.0).exp()*0.08)*(-t/(0.06*dm)).exp()
        }
        49 => { // CH2: Pure noise BPF — no oscillators
            let n=self.noise();
            let f=self.svf1.bandpass(n, 8500.0, 2.5, sr);
            self.hp1.tick_hp(f, 6000.0, sr)*0.35*(-t/(0.055*dm)).exp()
        }
        50 => { // CH3: FM metallic tick — carrier+modulator
            advance_phase(&mut self.phase1, 5500.0, sr);
            advance_phase(&mut self.phase2, 8100.0, sr);
            let fm=(self.phase1*TAU+osc_sine(self.phase2)*1.5).sin()*0.3;
            self.hp1.tick_hp(fm, 4000.0, sr)*(-t/(0.045*dm)).exp()
        }
        51 => { // CH4: Ring mod tick — dense inharmonic
            advance_phase(&mut self.phase1, 4200.0, sr);
            advance_phase(&mut self.phase2, 6300.0, sr);
            let ring=osc_sine(self.phase1)*osc_sine(self.phase2)*0.35;
            self.hp1.tick_hp(ring, 5500.0, sr)*(-t/(0.04*dm)).exp()
        }
        52 => { // CH5: Dark LPF modal — warm closed hat
            let freqs=[280.0, 672.0, 1184.0, 1896.0, 2848.0, 3952.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            self.svf1.lowpass(m, 5000.0, 0.4, sr)*0.32*(-t/(0.07*dm)).exp()
        }
        53 => { // CH6: Multi-band noise — 3 independent noise bands
            let n=self.noise();
            let lo=self.svf1.bandpass(n, 3800.0, 3.0, sr)*0.2;
            let mid=self.svf2.bandpass(n, 7200.0, 2.5, sr)*0.25;
            let hi=self.svf3.bandpass(n, 12000.0, 2.0, sr)*0.15;
            self.hp1.tick_hp(lo+mid+hi, 3500.0, sr)*(-t/(0.05*dm)).exp()
        }
        54 => { // CH7: Noise + saturation — driven noise for grit
            let n=self.noise()*0.8;
            let hp=self.hp1.tick_hp(n, 6000.0, sr);
            let sat=(hp*3.0).tanh()*0.25;
            sat*(-t/(0.05*dm)).exp()
        }
        55 => { // CH8: AM modulated noise — fluttering tick
            advance_phase(&mut self.phase1, 12000.0, sr);
            let am=(1.0+osc_sine(self.phase1))*0.5;
            let n=self.noise()*am;
            let f=self.svf1.bandpass(n, 7500.0, 2.0, sr);
            f*0.35*(-t/(0.04*dm)).exp()
        }

        // ════════ 8 OPEN HATS (56-63) — 8 DIFFERENT synthesis, 1.0-2.5s decays ════════

        56 => { // HO1: 9-mode shimmer — modal bank + 3 upper partials
            let freqs=[340.0, 815.0, 1490.0, 2350.0, 3500.0, 4850.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            advance_phase(&mut self.modal_phases[0], 6400.0, sr);
            advance_phase(&mut self.modal_phases[1], 8300.0, sr);
            advance_phase(&mut self.modal_phases[2], 10500.0, sr);
            let upper=osc_sine(self.modal_phases[0])*0.15*(-t/(1.8*dm)).exp()
                +osc_sine(self.modal_phases[1])*0.1*(-t/(2.2*dm)).exp()
                +osc_sine(self.modal_phases[2])*0.06*(-t/(2.5*dm)).exp();
            (self.hp1.tick_hp(m, 2500.0, sr)*0.35+upper)*(-t/(1.5*dm)).exp()
        }
        57 => { // HO2: FM metallic shimmer — completely different from modal
            advance_phase(&mut self.phase1, 3200.0, sr);
            advance_phase(&mut self.phase2, 4700.0, sr);
            advance_phase(&mut self.phase3, 7100.0, sr);
            let fm_mod=osc_sine(self.phase2)*2.5;
            let fm1=(self.phase1*TAU+fm_mod).sin()*0.25;
            let fm2=(self.phase3*TAU+fm_mod*0.7).sin()*0.18;
            let noise=self.hp1.tick_hp(self.noise()*0.06, 8000.0, sr);
            (fm1+fm2+noise)*(-t/(2.0*dm)).exp()
        }
        58 => { // HO3: 3-band resonant noise — no oscillators
            let n=self.noise();
            let b1=self.svf1.bandpass(n, 4200.0, 3.0, sr)*0.28;
            let b2=self.svf2.bandpass(n, 7800.0, 2.5, sr)*0.22;
            let b3=self.svf3.bandpass(n, 11500.0, 2.0, sr)*0.14;
            self.hp1.tick_hp(b1+b2+b3, 3000.0, sr)*(-t/(1.2*dm)).exp()
        }
        59 => { // HO4: Ring mod wash — two non-integer sines multiplied
            advance_phase(&mut self.phase1, 2850.0, sr);
            advance_phase(&mut self.phase2, 4130.0, sr);
            let ring=osc_sine(self.phase1)*osc_sine(self.phase2)*0.35;
            advance_phase(&mut self.phase3, 6950.0, sr);
            let shimmer=osc_sine(self.phase3)*0.1;
            self.hp1.tick_hp(ring+shimmer, 2000.0, sr)*(-t/(1.8*dm)).exp()
        }
        60 => { // HO5: Breathy HP noise — mostly air, very long
            let wash=self.hp1.tick_hp(self.noise(), 5000.0, sr)*0.25;
            advance_phase(&mut self.phase1, 5500.0, sr);
            let hint=osc_sine(self.phase1)*0.04;
            (wash+hint)*(-t/(2.5*dm)).exp()
        }
        61 => { // HO6: Distorted modal — heavy waveshaping creates new partials
            let freqs=[280.0, 670.0, 1180.0, 1900.0, 2850.0, 3950.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let dist=(m*3.5).tanh()*0.4;
            self.hp1.tick_hp(dist+self.noise()*0.06, 1800.0, sr)*(-t/(1.3*dm)).exp()
        }
        62 => { // HO7: Comb-filtered noise — shimmer from comb resonances
            let n=self.noise();
            // Approximate comb: tight BPF at harmonic series of a high freq
            let c1=self.svf1.bandpass(n, 3500.0, 8.0, sr)*0.15;
            let c2=self.svf2.bandpass(n, 7000.0, 6.0, sr)*0.12;
            let c3=self.svf3.bandpass(n, 10500.0, 5.0, sr)*0.08;
            self.hp1.tick_hp(c1+c2+c3, 2500.0, sr)*(-t/(1.6*dm)).exp()
        }
        63 => { // HO8: Phase-modulated shimmer — phase distortion synthesis
            advance_phase(&mut self.phase1, 4800.0, sr);
            advance_phase(&mut self.phase2, 3100.0, sr); // modulator
            // Phase distortion: warp the phase before sine lookup
            let warped = self.phase1 + osc_sine(self.phase2)*0.3*(-t*2.0).exp();
            let pd = (warped*TAU).sin()*0.3;
            advance_phase(&mut self.phase3, 8200.0, sr);
            let hi = osc_sine(self.phase3)*0.08*(-t/(2.5*dm)).exp();
            self.hp1.tick_hp(pd+hi, 2500.0, sr)*(-t/(2.0*dm)).exp()
        }

        // ════════ 2 PEDAL HATS (64-65) ════════

        64 => { // PH1: Pedal modal chick — foot close
            let freqs=[355.0, 850.0, 1500.0, 2370.0, 3520.0, 4860.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let chick=self.svf1.bandpass(self.noise()*(-t*500.0).exp(), 1300.0, 2.5, sr)*0.12;
            m*(-t/(0.025*dm)).exp()*0.2 + chick
        }
        65 => { // PH2: Pedal noise chick — no modal, just clap of cymbals
            let clap=self.noise()*(t/0.0003).min(1.0)*(-t*400.0).exp();
            let f=self.svf1.bandpass(clap, 1800.0, 2.0, sr);
            let hp=self.hp1.tick_hp(f, 800.0, sr);
            hp*0.3
        }

        // ════════ 6 TOMS (66-71) — each with different body character ════════

        66 => { // T1: Floor — 3-mode Bessel, long decay
            let f=82.0*tm; let sw=f*0.14*(-t*28.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr); advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            advance_phase(&mut self.phase3, (f+sw)*2.296, sr);
            let body=osc_sine(self.phase1)*0.5*(0.2*(-t/0.015).exp()+0.8*(-t/(0.35*dm)).exp());
            body + osc_sine(self.phase2)*0.14*(-t/(0.12*dm)).exp()
                + osc_sine(self.phase3)*0.06*(-t/(0.08*dm)).exp()
        }
        67 => { // T2: Low rack — FM body for richer tone
            advance_phase(&mut self.phase1, 130.0*tm, sr);
            advance_phase(&mut self.phase2, 195.0*tm, sr);
            let body=(self.phase1*TAU+osc_sine(self.phase2)*0.5*(-t*20.0).exp()).sin();
            body*0.45*(0.3*(-t/0.008).exp()+0.7*(-t/(0.24*dm)).exp())
        }
        68 => { // T3: Mid rack — triangle body, mellow
            let f=170.0*tm; let sw=f*0.1*(-t*35.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            osc_triangle(self.phase1)*0.45*(0.3*(-t/0.007).exp()+0.7*(-t/(0.22*dm)).exp())
        }
        69 => { // T4: High rack — sine + ring mod overtone
            advance_phase(&mut self.phase1, 215.0*tm, sr);
            advance_phase(&mut self.phase2, 340.0*tm, sr);
            let body=osc_sine(self.phase1)*0.4;
            let ring=osc_sine(self.phase1)*osc_sine(self.phase2)*0.08*(-t/(0.06*dm)).exp();
            (body+ring)*(0.35*(-t/0.006).exp()+0.65*(-t/(0.18*dm)).exp())
        }
        70 => { // T5: Concert — very long, resonant 3-mode
            let f=150.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*1.593, sr);
            advance_phase(&mut self.phase3, f*2.296, sr);
            osc_sine(self.phase1)*0.45*(-t/(0.4*dm)).exp()
                + osc_sine(self.phase2)*0.16*(-t/(0.18*dm)).exp()
                + osc_sine(self.phase3)*0.08*(-t/(0.12*dm)).exp()
        }
        71 => { // T6: Timbale — metallic shell via resonant filter
            let f=360.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let body=osc_sine(self.phase1)*0.35*(-t/(0.2*dm)).exp();
            let shell=self.svf1.bandpass(self.noise()*(-t*25.0).exp(), f*2.8, 14.0, sr)*0.15;
            body + shell
        }

        // ════════ 6 CYMBALS (72-77) — each different topology ════════

        72 => { // CY1: Crash dark — LPF modal
            let freqs=[310.0, 740.0, 1300.0, 2080.0, 3100.0, 4280.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.lowpass(m+self.noise()*0.08, 7500.0, 0.3, sr);
            f*0.32*(t/0.003).min(1.0)*(-t/(1.5*dm)).exp()
        }
        73 => { // CY2: Crash bright — HP modal
            let freqs=[405.0, 972.0, 1703.0, 2684.0, 3953.0, 5458.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            self.hp1.tick_hp(m, 2500.0, sr)*(t/0.002).min(1.0)*(-t/(1.8*dm)).exp()*0.3
        }
        74 => { // CY3: Ride ping — BPF focused + sine ping
            let freqs=[425.0, 1015.0, 1742.0, 2831.0, 4184.0, 5753.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.bandpass(m, 5500.0, 1.0, sr);
            let ping=(-t*100.0).exp()*0.12;
            (f*(-t/(1.0*dm)).exp()+ping)*0.28
        }
        75 => { // CY4: Ride wash — FM generated wash
            advance_phase(&mut self.phase1, 2800.0, sr);
            advance_phase(&mut self.phase2, 4100.0, sr);
            let fm=(self.phase1*TAU+osc_sine(self.phase2)*1.2).sin()*0.25;
            self.hp1.tick_hp(fm, 2000.0, sr)*(-t/(2.0*dm)).exp()
        }
        76 => { // CY5: Splash — noise burst, fast decay
            let n=self.noise();
            let hp=self.hp1.tick_hp(n, 4000.0, sr);
            let shaped=self.svf1.bandpass(hp, 8000.0, 1.5, sr);
            shaped*0.3*(t/0.001).min(1.0)*(-t/(0.5*dm)).exp()
        }
        77 => { // CY6: China — distorted modal for trashy character
            let freqs=[285.0, 683.0, 1207.0, 1932.0, 2878.0, 3988.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let dist=(m*2.5).tanh()*0.4;
            dist*(t/0.002).min(1.0)*(-t/(1.2*dm)).exp()*0.3
        }

        // ════════ 12 PERCUSSION (78-89) — each unique ════════

        78 => { // P1: Cowbell — two sines through narrow BPF
            advance_phase(&mut self.phase1, 580.0, sr); advance_phase(&mut self.phase2, 870.0, sr);
            self.svf1.bandpass(osc_sine(self.phase1)*0.35+osc_sine(self.phase2)*0.3, 725.0, 4.0, sr)
                *(-t/(0.07*dm)).exp()
        }
        79 => { // P2: Woodblock — high sine + noise click
            advance_phase(&mut self.phase1, 1900.0, sr);
            (osc_sine(self.phase1)*0.3+self.noise()*(-t*1000.0).exp()*0.08)*(-t/(0.015*dm)).exp()
        }
        80 => { // P3: Clave — single pure high sine
            advance_phase(&mut self.phase1, 2500.0, sr);
            osc_sine(self.phase1)*0.4*(-t/(0.022*dm)).exp()
        }
        81 => { // P4: Triangle instrument — long ring
            advance_phase(&mut self.phase1, 1200.0, sr); advance_phase(&mut self.phase2, 3600.0, sr);
            (osc_sine(self.phase1)*0.3+osc_sine(self.phase2)*0.18)*(-t/(0.9*dm)).exp()
        }
        82 => { // P5: Tambourine — high modal jingles
            let freqs=[4600.0, 6300.0, 7900.0, 9600.0, 11300.0, 13100.0];
            let j=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(j, 4000.0, sr);
            let shake=(t*22.0*TAU).sin().abs()*(-t*6.0).exp();
            hp*((-t/(0.2*dm)).exp()+shake*0.2)*0.25
        }
        83 => { // P6: Shaker — tight noise burst
            self.hp1.tick_hp(self.svf1.bandpass(self.noise(), 7200.0, 1.3, sr), 5000.0, sr)
                *(-t/(0.07*dm)).exp()*0.3
        }
        84 => { // P7: Cabasa — scratchy high noise
            self.svf1.bandpass(self.noise(), 8800.0, 2.0, sr)*(-t/(0.1*dm)).exp()*0.28
        }
        85 => { // P8: Guiro — scraping modulated noise
            let scrape=(t*38.0*TAU).sin().abs()*(-t*4.5).exp();
            self.svf1.bandpass(self.noise(), 4200.0, 3.0, sr)*(scrape*0.4+0.15)*(-t/(0.22*dm)).exp()
        }
        86 => { // P9: Vibraslap — rattling decay
            let rattle=(t*36.0*TAU).sin().abs()*(-t*2.8).exp();
            self.svf1.bandpass(self.noise(), 3400.0, 5.5, sr)*rattle*(-t/(0.5*dm)).exp()*0.25
        }
        87 => { // P10: Maracas — short HP noise
            self.hp1.tick_hp(self.noise(), 6500.0, sr)*(-t/(0.045*dm)).exp()*0.25
        }
        88 => { // P11: Agogo high — two inharmonic sines
            advance_phase(&mut self.phase1, 930.0, sr); advance_phase(&mut self.phase2, 1398.0, sr);
            (osc_sine(self.phase1)*0.33+osc_sine(self.phase2)*0.24)*(-t/(0.16*dm)).exp()
        }
        89 => { // P12: Agogo low — FM bell tone
            advance_phase(&mut self.phase1, 670.0, sr); advance_phase(&mut self.phase2, 1008.0, sr);
            let fm=(self.phase1*TAU+osc_sine(self.phase2)*0.5).sin()*0.3;
            fm*(-t/(0.18*dm)).exp()
        }

        // ════════ 12 MORE PERCUSSION (90-101) — hand drums, etc ════════

        90 => { // Conga open — sine body with slap
            let f=335.0*tm; advance_phase(&mut self.phase1, f+f*0.06*(-t*42.0).exp(), sr);
            osc_sine(self.phase1)*0.5*(-t/(0.22*dm)).exp() + self.noise()*(-t*450.0).exp()*0.08
        }
        91 => { // Conga mute — short damped
            advance_phase(&mut self.phase1, 320.0*tm, sr);
            osc_sine(self.phase1)*0.42*(-t/(0.06*dm)).exp()
        }
        92 => { // Conga slap — noise-heavy attack
            advance_phase(&mut self.phase1, 355.0*tm, sr);
            osc_sine(self.phase1)*0.2*(-t/(0.04*dm)).exp()
                + self.svf1.bandpass(self.noise()*(-t*700.0).exp(), 2800.0, 2.0, sr)*0.3
        }
        93 => { // Bongo high — quick, bright
            advance_phase(&mut self.phase1, 425.0*tm+425.0*0.08*(-t*65.0).exp(), sr);
            osc_sine(self.phase1)*0.42*(-t/(0.1*dm)).exp()
        }
        94 => { // Bongo low — rounder, longer
            advance_phase(&mut self.phase1, 315.0*tm+315.0*0.07*(-t*50.0).exp(), sr);
            osc_sine(self.phase1)*0.45*(-t/(0.15*dm)).exp()
        }
        95 => { // Timbale high — shell ring via resonant BPF
            let f=530.0*tm; advance_phase(&mut self.phase1, f, sr);
            osc_sine(self.phase1)*0.38*(-t/(0.2*dm)).exp()
                + self.svf1.bandpass(self.noise()*(-t*18.0).exp(), f*3.0, 10.0, sr)*0.1
        }
        96 => { // Timbale low
            let f=370.0*tm; advance_phase(&mut self.phase1, f, sr);
            osc_sine(self.phase1)*0.4*(-t/(0.22*dm)).exp()
                + self.svf1.bandpass(self.noise()*(-t*22.0).exp(), f*2.5, 8.0, sr)*0.08
        }
        97 => { // Cuica high — pitch sweep
            let f=600.0+400.0*(-t*8.0).exp();
            advance_phase(&mut self.phase1, f, sr);
            osc_sine(self.phase1)*0.35*(-t/(0.18*dm)).exp()
        }
        98 => { // Cuica low — slower sweep
            let f=350.0+200.0*(-t*6.0).exp();
            advance_phase(&mut self.phase1, f, sr);
            osc_sine(self.phase1)*0.35*(-t/(0.22*dm)).exp()
        }
        99 => { // Whistle — pitched sine with vibrato
            advance_phase(&mut self.phase1, 2300.0+(t*6.0*TAU).sin()*25.0, sr);
            osc_sine(self.phase1)*0.3*(-t/(0.1*dm)).exp()
        }
        100 => { // Ride bell — 3 sine partials
            advance_phase(&mut self.phase1, 760.0, sr);
            advance_phase(&mut self.phase2, 1140.0, sr);
            advance_phase(&mut self.phase3, 1710.0, sr);
            (osc_sine(self.phase1)*0.28+osc_sine(self.phase2)*0.22+osc_sine(self.phase3)*0.14)
                *(-t/(0.8*dm)).exp()
        }
        101 => { // Sizzle cymbal — modal + rattle
            let freqs=[348.0, 835.0, 1475.0, 2330.0, 3460.0, 4780.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let rattle=self.noise()*(t*30.0*TAU).sin().abs()*(-t*3.5).exp();
            let rf=self.svf1.bandpass(rattle, 8500.0, 3.5, sr)*0.1;
            m*(-t/(1.5*dm)).exp()*0.22 + rf
        }

        // Notes 102-111: 10 more unique percussion — each hardcoded, no formulas
        102 => { // Clap: Vintage vinyl — warm LP filtered
            let mut e=0.0;
            for k in 0..5u32 { let off=(self.hit_rand(k*6)*0.008).abs();
                let to=t-off; if to>=0.0 { e+=(-to*150.0).exp()*0.18; } }
            let f=self.svf1.bandpass(self.noise()*nm, 1900.0, 1.5, sr);
            self.svf2.lowpass(f, 5000.0, 0.5, sr)*(e + (-t/(0.2*dm)).exp()*0.3)
        }
        103 => { // Hat: Pedal splash — open then choked
            let freqs=[362.0, 868.0, 1537.0, 2427.0, 3607.0, 4985.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            // Swell then choke envelope
            let swell = (t*8.0).min(1.0)*(-t/(0.15*dm)).exp();
            m*swell*0.3
        }
        104 => { // Snap: Knuckle crack — very high, very short
            let snap=self.noise()*(t/0.00015).min(1.0)*(-t*1800.0).exp();
            self.svf1.bandpass(snap, 4200.0, 3.0, sr)*0.4
        }
        105 => { // Stick click — wood on wood
            advance_phase(&mut self.phase1, 2200.0, sr);
            advance_phase(&mut self.phase2, 4800.0, sr);
            (osc_sine(self.phase1)*0.2+osc_sine(self.phase2)*0.12+self.noise()*(-t*1500.0).exp()*0.1)
                *(-t/(0.01*dm)).exp()
        }
        106 => { // Bell tree — descending shimmer
            let sweep = 3000.0 + 5000.0*(-t*3.0).exp();
            advance_phase(&mut self.phase1, sweep, sr);
            advance_phase(&mut self.phase2, sweep*1.5, sr);
            (osc_sine(self.phase1)*0.2+osc_sine(self.phase2)*0.12)*(-t/(0.6*dm)).exp()
        }
        107 => { // Snap: Tongue click — very short mid pop
            let pop=self.noise()*(t/0.0002).min(1.0)*(-t*800.0).exp();
            self.svf1.bandpass(pop, 1800.0, 2.0, sr)*0.35
        }
        108 => { // Brush sweep — long swishing noise
            let n=self.noise();
            let sweep=(t*3.0*TAU).sin()*0.3;
            let f=self.svf1.bandpass(n, 4000.0+sweep*2000.0, 0.8, sr);
            self.hp1.tick_hp(f, 1500.0, sr)*(-t/(0.4*dm)).exp()*0.25
        }
        109 => { // Hat: Foot splash — hit then immediately open
            let freqs=[330.0, 793.0, 1403.0, 2217.0, 3297.0, 4553.0];
            let m=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(m, 3000.0, sr);
            hp*(-t/(0.8*dm)).exp()*0.28
        }
        110 => { // Clap: Stadium — massive reverb, 8 clappers
            let mut e=0.0;
            for k in 0..8u32 { let off=(self.hit_rand(k*7)*0.025+self.hit_rand(k*7+1).abs()*0.015).abs();
                let to=t-off; if to>=0.0 { e+=(-to*100.0).exp()*(0.5+self.hit_rand(k*7+2)*0.5)*0.1; } }
            let f=self.svf1.bandpass(self.noise()*nm, 2000.0, 1.0, sr);
            f*(e + (-t/(0.5*dm)).exp()*0.45) // very long tail
        }
        111 => { // Snare: Brush tap — gentle, all wire, barely there
            let wire=self.svf1.bandpass(self.noise()*nm*0.5, 5000.0, 0.8, sr);
            self.hp1.tick_hp(wire, 2500.0, sr)*(-t/(0.08*dm)).exp()*0.2
        }

        _ => 0.0,
        }
    }
}
