//! TSTY4 kit synthesis.

use super::super::*;
use std::f64::consts::TAU;

impl DrumVoice {
    // TSTY-4: Studio kit v4 — emphasis on hats, snares, claps with LONG decays
    // 88 unique sounds. NO filler. NO pitch transposition.
    // Layout: 8 kicks, 16 snares, 8 claps, 20 hats, 8 toms, 8 cymbals, 20 perc
    // ══════════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_tsty4(&mut self, sr: f64, dm: f64, tm: f64, nm: f64, _dr: f64) -> f64 {
        let raw = self.t4v2(sr, dm, tm, nm);
        // Tape: asymmetric saturation + warm rolloff at 9.5kHz
        let sat = (raw * 1.6).tanh() + 0.035 * raw * (-(raw * 0.5).abs()).exp();
        let rc = 1.0 / (TAU * 9500.0);
        let alpha = 1.0 / (1.0 + rc * sr);
        self.lp1_state += alpha * (sat - self.lp1_state);
        self.lp1_state
    }

    pub(crate) fn t4v2(&mut self, sr: f64, dm: f64, tm: f64, nm: f64) -> f64 {
        let t = self.time;
        let n = self.note;
        match n {
        // ══ 8 KICKS (24-31) — each with different body/beater/shell topology ══

        24 => { // Kick: Felt Studio — 3-mode Bessel body, soft felt beater
            let f=60.0*tm; let sw=f*0.28*(-t*50.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            advance_phase(&mut self.phase3, (f+sw)*2.296, sr);
            let e = 0.3*(-t/0.01).exp() + 0.7*(-t/(0.3*dm)).exp();
            let body = (osc_sine(self.phase1)*0.55 + osc_sine(self.phase2)*0.12*(-t/(0.08*dm)).exp()
                + osc_sine(self.phase3)*0.06*(-t/(0.05*dm)).exp()) * e;
            let felt = self.noise()*(t/0.0018).min(1.0)*(-t*160.0).exp();
            body + self.svf1.bandpass(felt, 2000.0, 1.2, sr)*0.12
        }
        25 => { // Tight Funk — 72Hz, quick, wood beater
            let f=72.0*tm; let sw=f*0.2*(-t*85.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let e = 0.5*(-t/0.005).exp() + 0.5*(-t/(0.15*dm)).exp();
            let body = osc_sine(self.phase1)*0.65*e;
            let click = self.noise()*(t/0.0006).min(1.0)*(-t*500.0).exp();
            body + self.svf1.bandpass(click, 4500.0, 2.0, sr)*0.25
        }
        26 => { // Deep — 44Hz, long, resonant body
            let f=44.0*tm; let sw=f*0.4*(-t*25.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            advance_phase(&mut self.phase2, f*0.5, sr);
            let body = osc_sine(self.phase1)*0.5*(-t/(0.45*dm)).exp();
            let sub = osc_sine(self.phase2)*0.2*(-t/(0.3*dm)).exp();
            body + sub
        }
        27 => { // Rock — 65Hz, bright attack, medium body
            let f=65.0*tm; let sw=f*0.35*(-t*60.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            let e = 0.4*(-t/0.006).exp() + 0.6*(-t/(0.22*dm)).exp();
            let body = osc_sine(self.phase1)*0.6*e + osc_sine(self.phase2)*0.1*(-t/(0.06*dm)).exp();
            let plastic = self.noise()*(t/0.0004).min(1.0)*(-t*600.0).exp();
            body + self.hp1.tick_hp(plastic, 3000.0, sr)*0.3
        }
        28 => { // Round — 55Hz, triangle body, soft
            let f=55.0*tm; let sw=f*0.3*(-t*40.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let e = 0.25*(-t/0.012).exp() + 0.75*(-t/(0.28*dm)).exp();
            osc_triangle(self.phase1)*0.55*e
        }
        29 => { // Punchy — 68Hz, strong 2nd mode, snappy attack
            let f=68.0*tm; let sw=f*0.4*(-t*70.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            let body = osc_sine(self.phase1)*0.5*(0.45*(-t/0.005).exp()+0.55*(-t/(0.18*dm)).exp());
            let m1 = osc_sine(self.phase2)*0.18*(-t/(0.07*dm)).exp();
            let click = self.noise()*(t/0.001).min(1.0)*(-t*350.0).exp();
            body + m1 + self.svf1.bandpass(click, 3200.0, 1.5, sr)*0.15
        }
        30 => { // Boomy — 48Hz, shell resonance, long ring
            let f=48.0*tm; let sw=f*0.3*(-t*22.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let body = osc_sine(self.phase1)*0.5*(-t/(0.4*dm)).exp();
            let shell = self.noise()*(-t*35.0).exp();
            body + self.svf1.bandpass(shell, 240.0*tm, 14.0, sr)*0.1
        }
        31 => { // Thump — 58Hz, very damped, all attack
            let f=58.0*tm; let sw=f*0.15*(-t*100.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let body = osc_sine(self.phase1)*0.6*(-t/(0.1*dm)).exp();
            let thud = self.noise()*(t/0.001).min(1.0)*(-t*200.0).exp();
            body + self.svf1.lowpass(thud, 600.0, 0.5, sr)*0.12
        }

        // ══ 16 SNARES (32-47) — all with proper sustain and wire buzz ══

        32 => { // Snare: Funk — 305Hz, crisp wires, medium body
            let f=305.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*1.593, sr);
            let stick = self.hp1.tick_hp(self.noise()*(t/0.0003).min(1.0)*(-t*1600.0).exp(), 3500.0, sr)*0.28;
            let head = osc_sine(self.phase1)*0.32*(-t/(0.12*dm)).exp() + osc_sine(self.phase2)*0.12*(-t/(0.07*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 4500.0*tm, 0.7, sr);
            let wf = self.hp1.tick_hp(wire, 1800.0, sr)*(-t/(0.22*dm)).exp()*0.38;
            stick + head + wf
        }
        33 => { // Snare: Fat — 235Hz, big body, long wires
            let f=235.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*1.593, sr);
            advance_phase(&mut self.phase3, f*2.136, sr);
            let head = osc_sine(self.phase1)*0.38*(-t/(0.15*dm)).exp()
                + osc_sine(self.phase2)*0.16*(-t/(0.1*dm)).exp()
                + osc_sine(self.phase3)*0.08*(-t/(0.06*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 3800.0, 0.5, sr)*(-t/(0.35*dm)).exp()*0.42;
            let stick = self.noise()*(-t*1400.0).exp()*0.18;
            head + wire + stick
        }
        34 => { // Snare: Dry Purdie — 280Hz, short tight
            let f=280.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.3*(-t/(0.08*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 5200.0, 0.5, sr)*(-t/(0.12*dm)).exp()*0.3;
            let stick = self.noise()*(-t*2000.0).exp()*0.22;
            head + wire + stick
        }
        35 => { // Snare: Brush — 265Hz, gentle noise dominated
            let f=265.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.18*(-t/(0.12*dm)).exp();
            let brush = self.noise()*(t/0.004).min(1.0);
            let bf = self.svf1.bandpass(brush, 3000.0, 0.8, sr)*(-t/(0.2*dm)).exp()*0.35;
            head + bf
        }
        36 => { // Snare: Cross-Stick — rim only, no wires
            advance_phase(&mut self.phase1, 560.0*tm, sr); advance_phase(&mut self.phase2, 1500.0*tm, sr);
            let crack = (osc_sine(self.phase1)*0.28 + osc_sine(self.phase2)*0.18)*(-t/(0.025*dm)).exp();
            let click = self.noise()*(-t*900.0).exp()*0.2;
            crack + click
        }
        37 => { // Snare: Ghost — very quiet, wire-dominant
            let f=295.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.08*(-t/(0.05*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 4800.0, 0.7, sr)*(-t/(0.1*dm)).exp()*0.18;
            head + wire
        }
        38 => { // Snare: Metal Shell — 340Hz, ring, long
            let f=340.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*1.593, sr);
            let head = osc_sine(self.phase1)*0.28*(-t/(0.12*dm)).exp() + osc_sine(self.phase2)*0.14*(-t/(0.09*dm)).exp();
            let shell = self.svf2.bandpass(self.noise()*(-t*35.0).exp(), 520.0*tm, 18.0, sr)*0.12;
            let wire = self.svf1.bandpass(self.noise()*nm, 4200.0, 0.6, sr)*(-t/(0.25*dm)).exp()*0.35;
            head + shell + wire
        }
        39 => { // Snare: Loose — 270Hz, long rattly buzz
            let f=270.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.28*(-t/(0.1*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 3500.0, 0.4, sr)*(-t/(0.45*dm)).exp()*0.45;
            head + wire
        }
        40 => { // Snare: Piccolo — 385Hz, bright short
            let f=385.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.22*(-t/(0.06*dm)).exp();
            let crack = self.hp1.tick_hp(self.noise()*(-t*2500.0).exp(), 5000.0, sr)*0.3;
            let wire = self.svf1.bandpass(self.noise()*nm, 6200.0, 0.8, sr)*(-t/(0.14*dm)).exp()*0.28;
            head + crack + wire
        }
        41 => { // Snare: Wood Deep — 225Hz, woody
            let f=225.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*2.136, sr);
            let head = osc_sine(self.phase1)*0.35*(-t/(0.12*dm)).exp() + osc_sine(self.phase2)*0.1*(-t/(0.06*dm)).exp();
            let shell = self.svf2.bandpass(self.noise()*(-t*45.0).exp(), 330.0*tm, 10.0, sr)*0.08;
            let wire = self.svf1.bandpass(self.noise()*nm, 3600.0, 0.6, sr)*(-t/(0.2*dm)).exp()*0.35;
            head + shell + wire
        }
        42 => { // Snare: Crack — maximum attack, snappy
            advance_phase(&mut self.phase1, 315.0*tm, sr);
            let head = osc_sine(self.phase1)*0.2*(-t/(0.05*dm)).exp();
            let crack = self.hp1.tick_hp(self.noise()*(t/0.0002).min(1.0)*(-t*2800.0).exp(), 4000.0, sr)*0.4;
            let wire = self.svf1.bandpass(self.noise()*nm, 5500.0, 0.7, sr)*(-t/(0.1*dm)).exp()*0.22;
            head + crack + wire
        }
        43 => { // Snare: Thick — 252Hz, lots of body + wire
            let f=252.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*1.593, sr);
            advance_phase(&mut self.phase3, f*2.296, sr);
            let head = osc_sine(self.phase1)*0.38*(-t/(0.14*dm)).exp()
                + osc_sine(self.phase2)*0.18*(-t/(0.09*dm)).exp()
                + osc_sine(self.phase3)*0.09*(-t/(0.06*dm)).exp();
            let wire = self.svf1.bandpass(self.noise()*nm, 4000.0, 0.6, sr)*(-t/(0.25*dm)).exp()*0.38;
            head + wire + self.noise()*(-t*1200.0).exp()*0.12
        }
        44 => { // Snare: Rim Shot Full — head + rim together
            let f=300.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            advance_phase(&mut self.phase2, 920.0*tm, sr); advance_phase(&mut self.phase3, 2300.0*tm, sr);
            let head = osc_sine(self.phase1)*0.28*(-t/(0.1*dm)).exp();
            let rim = (osc_sine(self.phase2)*0.18 + osc_sine(self.phase3)*0.1)*(-t/(0.02*dm)).exp();
            let crack = self.noise()*(-t*1800.0).exp()*0.25;
            let wire = self.svf1.bandpass(self.noise()*nm, 4300.0, 0.7, sr)*(-t/(0.15*dm)).exp()*0.28;
            head + rim + crack + wire
        }
        45 => { // Snare: Sizzle — double wire band, buzzy
            let f=290.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.22*(-t/(0.08*dm)).exp();
            let w1 = self.svf1.bandpass(self.noise()*nm, 3800.0, 0.4, sr)*(-t/(0.3*dm)).exp()*0.4;
            let w2 = self.svf2.bandpass(self.noise()*nm, 7200.0, 1.0, sr)*(-t/(0.18*dm)).exp()*0.18;
            head + w1 + w2
        }
        46 => { // Snare: Roll Sustain — like a soft roll
            let f=275.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let head = osc_sine(self.phase1)*0.12*(-t/(0.18*dm)).exp();
            let roll = (t*20.0*TAU).sin().abs();
            let wire = self.svf1.bandpass(self.noise()*nm, 4000.0, 0.6, sr)*roll*(-t/(0.4*dm)).exp()*0.3;
            head + wire
        }
        47 => { // Snare: Backbeat Bonham — 210Hz, huge, ringy
            let f=210.0*tm;
            advance_phase(&mut self.phase1, f, sr); advance_phase(&mut self.phase2, f*1.593, sr);
            advance_phase(&mut self.phase3, f*2.136, sr);
            let head = osc_sine(self.phase1)*0.4*(-t/(0.18*dm)).exp()
                + osc_sine(self.phase2)*0.2*(-t/(0.12*dm)).exp()
                + osc_sine(self.phase3)*0.12*(-t/(0.08*dm)).exp();
            let shell = self.svf2.bandpass(self.noise()*(-t*30.0).exp(), 400.0*tm, 16.0, sr)*0.12;
            let wire = self.svf1.bandpass(self.noise()*nm, 3500.0, 0.5, sr)*(-t/(0.35*dm)).exp()*0.4;
            head + shell + wire
        }

        // ══ 8 CLAPS/SNAPS (48-55) ══

        48 => { // Clap: Tight Group — 5 clappers, close timing
            let mut e=0.0;
            for k in 0..5u32 { let off=(self.hit_rand(k*3)*0.006+self.hit_rand(k*3+1).abs()*0.004).abs();
                let to=t-off; if to>=0.0 { e+=(-to*170.0).exp()*(0.75+self.hit_rand(k*3+2)*0.25)*0.17; } }
            let f = self.svf1.bandpass(self.noise()*nm, 2300.0+self.hit_rand(60)*500.0, 1.4, sr);
            let hp = self.hp1.tick_hp(f, 600.0, sr);
            let tail = (-t/(0.15*dm)).exp()*0.3;
            hp * (e + tail)
        }
        49 => { // Clap: Loose Group — 8 clappers, wide spread
            let mut e=0.0;
            for k in 0..8u32 { let off=(self.hit_rand(k*4)*0.018+self.hit_rand(k*4+1).abs()*0.012).abs();
                let to=t-off; if to>=0.0 { e+=(-to*130.0).exp()*(0.6+self.hit_rand(k*4+2)*0.4)*0.11; } }
            let f = self.svf1.bandpass(self.noise()*nm, 1900.0+self.hit_rand(80)*700.0, 1.1, sr);
            let tail = (-t/(0.2*dm)).exp()*0.35;
            f * (e + tail)
        }
        50 => { // Clap: Hall Reverb — group with long room
            let mut e=0.0;
            for k in 0..4u32 { let off=(self.hit_rand(k*5)*0.01).abs();
                let to=t-off; if to>=0.0 { e+=(-to*160.0).exp()*0.2; } }
            let f = self.svf1.bandpass(self.noise()*nm, 2200.0, 1.3, sr);
            let tail = (-t/(0.35*dm)).exp()*0.4; // LONG tail
            f * (e + tail)
        }
        51 => { // Finger Snap — sharp, high
            let snap = self.noise()*(t/0.0002).min(1.0)*(-t*1000.0).exp();
            let f = self.svf1.bandpass(snap, 3400.0, 2.5, sr);
            self.hp1.tick_hp(f, 1500.0, sr)*0.45
        }
        52 => { // Hand Slap — thigh slap, mid-heavy
            advance_phase(&mut self.phase1, 185.0*tm, sr);
            let body = osc_sine(self.phase1)*0.2*(-t/(0.05*dm)).exp();
            let slap = self.svf1.bandpass(self.noise()*(-t*350.0).exp(), 1600.0, 1.5, sr)*0.3;
            body + slap
        }
        53 => { // Single Clap — one person, dry
            let clap = self.noise()*(t/0.0004).min(1.0)*(-t*220.0).exp();
            let f = self.svf1.bandpass(clap, 2100.0, 1.8, sr);
            self.hp1.tick_hp(f, 500.0, sr)*0.35 + (-t/(0.08*dm)).exp()*self.noise()*0.02
        }
        54 => { // Clap: Dry Staccato — tight, no tail
            let mut e=0.0;
            for k in 0..3u32 { let off=(self.hit_rand(k*3)*0.005).abs();
                let to=t-off; if to>=0.0 { e+=(-to*250.0).exp()*0.25; } }
            let f = self.svf1.bandpass(self.noise()*nm, 2600.0, 1.5, sr);
            f * e
        }
        55 => { // Clap: Vinyl Room — warm, rolled off, vintage
            let mut e=0.0;
            for k in 0..5u32 { let off=(self.hit_rand(k*6)*0.008).abs();
                let to=t-off; if to>=0.0 { e+=(-to*150.0).exp()*0.18; } }
            let f = self.svf1.bandpass(self.noise()*nm, 1800.0, 1.2, sr);
            let warm = self.svf2.lowpass(f, 4500.0, 0.5, sr);
            let tail = (-t/(0.2*dm)).exp()*0.3;
            warm * (e + tail)
        }

        // ══ 20 HATS (56-75) — 6 closed, 4 half-open, 6 open, 2 pedal, 2 sizzle ══
        // ALL use inharmonic sine modal banks. Open hats have 0.8-2.0s decays.

        // -- 6 Closed Hats — each using DIFFERENT synthesis method --
        56 => { // Closed Hat: Modal — standard 6-mode sine bank, tight
            let freqs=[385.0*tm, 960.0*tm, 1620.0*tm, 2520.0*tm, 3740.0*tm, 5180.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(m, 5500.0, sr);
            let stick=self.noise()*(-t*2200.0).exp()*0.1;
            (hp*0.35+stick)*(-t/(0.06*dm)).exp()
        }
        57 => { // Closed Hat: Noise Filtered — NO modal oscs, pure noise shaping
            let n=self.noise();
            let bp=self.svf1.bandpass(n, 8500.0*tm, 2.5, sr)*0.35;
            let hp=self.hp1.tick_hp(bp, 6000.0, sr);
            let stick=self.noise()*(-t*3000.0).exp()*0.08;
            (hp+stick)*(-t/(0.055*dm)).exp()
        }
        58 => { // Closed Hat: FM Click — FM synthesis for unique metallic tick
            advance_phase(&mut self.phase1, 5500.0*tm, sr);
            advance_phase(&mut self.phase2, 8100.0*tm, sr);
            let fm = (self.phase1*TAU + osc_sine(self.phase2)*1.5).sin()*0.3;
            let hp=self.hp1.tick_hp(fm, 4000.0, sr);
            hp*(-t/(0.045*dm)).exp()
        }
        59 => { // Closed Hat: Ring Mod Tick — ring mod for dense inharmonic click
            advance_phase(&mut self.phase1, 4200.0*tm, sr);
            advance_phase(&mut self.phase2, 6300.0*tm, sr);
            let ring = osc_sine(self.phase1)*osc_sine(self.phase2)*0.35;
            let hp=self.hp1.tick_hp(ring, 5500.0, sr);
            hp*(-t/(0.04*dm)).exp()
        }
        60 => { // Closed Hat: Dark Warm — lowpassed modal, mellow
            let freqs=[280.0*tm, 670.0*tm, 1180.0*tm, 1900.0*tm, 2850.0*tm, 3950.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.lowpass(m, 5000.0, 0.4, sr);
            f*(-t/(0.07*dm)).exp()*0.32
        }
        61 => { // Closed Hat: Multi-band Noise — three separate noise bands
            let n=self.noise();
            let lo=self.svf1.bandpass(n, 3800.0*tm, 3.0, sr)*0.2;
            let mid=self.svf2.bandpass(n, 7200.0*tm, 2.5, sr)*0.25;
            let hi=self.svf3.bandpass(n, 12000.0*tm, 2.0, sr)*0.15;
            let hp=self.hp1.tick_hp(lo+mid+hi, 3500.0, sr);
            hp*(-t/(0.05*dm)).exp()
        }

        // -- 4 Half-Open Hats (longer than closed, shorter than open) --
        62 => { // Half-Open: Standard — 150-200ms
            let freqs=[350.0*tm, 840.0*tm, 1490.0*tm, 2350.0*tm, 3490.0*tm, 4820.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.bandpass(m, 6000.0, 1.0, sr);
            let hp=self.hp1.tick_hp(f, 3500.0, sr);
            let sizzle=self.svf2.bandpass(self.noise()*(-t/(0.2*dm)).exp(), 7800.0, 4.0, sr)*0.06;
            (hp*0.3+sizzle)*(-t/(0.22*dm)).exp()
        }
        63 => { // Half-Open: Bright — more top end
            let freqs=[380.0*tm, 920.0*tm, 1600.0*tm, 2520.0*tm, 3720.0*tm, 5140.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(m, 4000.0, sr);
            hp*(-t/(0.28*dm)).exp()*0.32
        }
        64 => { // Half-Open: Dark — warmer, lower modes
            let freqs=[315.0*tm, 755.0*tm, 1330.0*tm, 2120.0*tm, 3160.0*tm, 4370.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.lowpass(m, 6500.0, 0.5, sr);
            f*(-t/(0.25*dm)).exp()*0.3
        }
        65 => { // Half-Open: Trashy — buzzy, aggressive
            let freqs=[290.0*tm, 695.0*tm, 1230.0*tm, 1960.0*tm, 2920.0*tm, 4040.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let dist = (m*1.8).tanh()*0.5; // distortion for trash
            dist*(-t/(0.2*dm)).exp()*0.32
        }

        // -- 6 Open Hats — LONG DECAYS, each with DIFFERENT synthesis topology --
        66 => { // Open Hat: Modal Shimmer — 9-mode sine bank, 1.2s
            // Uses modal_phases for extra modes beyond the 6 hat oscillators
            let freqs=[340.0*tm, 815.0*tm, 1490.0*tm, 2350.0*tm, 3500.0*tm, 4850.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            advance_phase(&mut self.modal_phases[0], 6400.0*tm, sr);
            advance_phase(&mut self.modal_phases[1], 8300.0*tm, sr);
            advance_phase(&mut self.modal_phases[2], 10500.0*tm, sr);
            let upper = osc_sine(self.modal_phases[0])*0.15*(-t/(1.5*dm)).exp()
                + osc_sine(self.modal_phases[1])*0.1*(-t/(1.8*dm)).exp()
                + osc_sine(self.modal_phases[2])*0.06*(-t/(2.2*dm)).exp();
            let hp=self.hp1.tick_hp(m, 2500.0, sr);
            (hp*0.35 + upper)*(-t/(1.2*dm)).exp()
        }
        67 => { // Open Hat: FM Metallic — FM synthesis for metallic character, 1.5s
            // Completely different from modal bank — uses FM between two sines
            advance_phase(&mut self.phase1, 3200.0*tm, sr); // carrier
            advance_phase(&mut self.phase2, 4700.0*tm, sr); // modulator
            advance_phase(&mut self.phase3, 7100.0*tm, sr); // second carrier
            let fm_mod = osc_sine(self.phase2) * 2.5;
            let fm1 = (self.phase1 * TAU + fm_mod).sin() * 0.25;
            let fm2 = (self.phase3 * TAU + fm_mod * 0.7).sin() * 0.18;
            let noise_sheen = self.hp1.tick_hp(self.noise()*0.08, 8000.0, sr);
            let env = (-t/(1.5*dm)).exp();
            (fm1 + fm2 + noise_sheen) * env
        }
        68 => { // Open Hat: Filtered Noise — noise through resonant comb, 1.0s
            // No oscillators at all — pure filtered noise approach
            let n = self.noise();
            let bp1 = self.svf1.bandpass(n, 4200.0*tm, 3.0, sr) * 0.3;
            let bp2 = self.svf2.bandpass(n, 7800.0*tm, 2.5, sr) * 0.25;
            let bp3 = self.svf3.bandpass(n, 11500.0*tm, 2.0, sr) * 0.15;
            let hp = self.hp1.tick_hp(bp1 + bp2 + bp3, 3000.0, sr);
            let env = (-t/(1.0*dm)).exp();
            hp * env
        }
        69 => { // Open Hat: Ring Mod — two oscillators multiplied, 1.8s
            // Ring modulation creates dense inharmonic content
            advance_phase(&mut self.phase1, 2850.0*tm, sr);
            advance_phase(&mut self.phase2, 4130.0*tm, sr); // non-integer ratio
            let ring = osc_sine(self.phase1) * osc_sine(self.phase2); // sum & difference freqs
            advance_phase(&mut self.phase3, 6950.0*tm, sr);
            let shimmer = osc_sine(self.phase3) * 0.12;
            let hp = self.hp1.tick_hp(ring * 0.35 + shimmer, 2000.0, sr);
            let env = (-t/(1.8*dm)).exp();
            hp * env
        }
        70 => { // Open Hat: Breathy Wash — noise emphasis, gentle modes, 2.0s
            // Mostly high noise with just hints of tonality
            let n = self.noise();
            let wash = self.hp1.tick_hp(n, 5000.0, sr) * 0.3;
            advance_phase(&mut self.phase1, 5500.0*tm, sr);
            advance_phase(&mut self.phase2, 8200.0*tm, sr);
            let hints = osc_sine(self.phase1)*0.06 + osc_sine(self.phase2)*0.04;
            let env = (-t/(2.0*dm)).exp();
            (wash + hints) * env
        }
        71 => { // Open Hat: Trashy Distorted — heavy saturation, 1.3s
            // Distortion-based — overdrive creates new harmonics
            let freqs=[280.0*tm, 670.0*tm, 1180.0*tm, 1900.0*tm, 2850.0*tm, 3950.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            // Heavy waveshaping — creates completely different harmonic content
            let dist = (m * 3.5).tanh() * 0.4;
            let n = self.noise() * 0.08;
            let hp = self.hp1.tick_hp(dist + n, 1800.0, sr);
            let env = (-t/(1.3*dm)).exp();
            hp * env
        }

        // -- 2 Pedal Hats --
        72 => { // Pedal Chick: Standard — foot close, short
            let freqs=[355.0*tm, 850.0*tm, 1500.0*tm, 2370.0*tm, 3520.0*tm, 4860.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let chick=self.svf1.bandpass(self.noise()*(-t*500.0).exp(), 1300.0, 2.5, sr)*0.12;
            m*(-t/(0.02*dm)).exp()*0.2 + chick
        }
        73 => { // Pedal Chick: Splashy — looser closure
            let freqs=[340.0*tm, 820.0*tm, 1450.0*tm, 2290.0*tm, 3400.0*tm, 4700.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let chick=self.svf1.bandpass(self.noise()*(-t*300.0).exp(), 1500.0, 2.0, sr)*0.1;
            m*(-t/(0.06*dm)).exp()*0.22 + chick
        }

        // -- 2 Sizzle Hats --
        74 => { // Sizzle Hat: Riveted — continuous rattle, 1.5s
            let freqs=[348.0*tm, 835.0*tm, 1475.0*tm, 2330.0*tm, 3460.0*tm, 4780.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let rattle=self.noise()*(t*32.0*TAU).sin().abs()*(-t*3.5).exp();
            let rf=self.svf1.bandpass(rattle, 9000.0, 3.5, sr)*0.1;
            m*(-t/(1.5*dm)).exp()*0.22 + rf
        }
        75 => { // Sizzle Hat: Chain — heavier rattle
            let freqs=[330.0*tm, 792.0*tm, 1400.0*tm, 2215.0*tm, 3295.0*tm, 4550.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let rattle=self.noise()*(t*25.0*TAU).sin().abs()*(-t*3.0).exp();
            let rf=self.svf1.bandpass(rattle, 7500.0, 3.0, sr)*0.12;
            m*(-t/(1.2*dm)).exp()*0.22 + rf
        }

        // ══ 8 TOMS (76-83) ══

        76 => { // Floor Tom Deep — 82Hz
            let f=82.0*tm; let sw=f*0.14*(-t*28.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr); advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            let body = osc_sine(self.phase1)*0.5*(0.2*(-t/0.015).exp()+0.8*(-t/(0.35*dm)).exp());
            let m1 = osc_sine(self.phase2)*0.14*(-t/(0.12*dm)).exp();
            let stick = self.svf1.bandpass(self.noise()*(-t*250.0).exp(), 2500.0, 1.3, sr)*0.08;
            body + m1 + stick
        }
        77 => { // Floor Tom Medium — 108Hz
            let f=108.0*tm; let sw=f*0.12*(-t*32.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let body = osc_sine(self.phase1)*0.48*(0.25*(-t/0.01).exp()+0.75*(-t/(0.28*dm)).exp());
            body + self.noise()*(-t*280.0).exp()*0.06
        }
        78 => { // Rack Tom Low — 135Hz
            let f=135.0*tm; let sw=f*0.1*(-t*35.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            let body = osc_sine(self.phase1)*0.48*(0.3*(-t/0.008).exp()+0.7*(-t/(0.24*dm)).exp());
            body + self.svf1.bandpass(self.noise()*(-t*300.0).exp(), 3000.0, 1.4, sr)*0.08
        }
        79 => { // Rack Tom Mid — 170Hz
            let f=170.0*tm; let sw=f*0.1*(-t*38.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr); advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            let body = osc_sine(self.phase1)*0.45*(0.3*(-t/0.007).exp()+0.7*(-t/(0.22*dm)).exp());
            body + osc_sine(self.phase2)*0.1*(-t/(0.07*dm)).exp()
        }
        80 => { // Rack Tom High — 215Hz
            let f=215.0*tm; let sw=f*0.08*(-t*40.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr);
            osc_sine(self.phase1)*0.45*(0.35*(-t/0.006).exp()+0.65*(-t/(0.18*dm)).exp())
        }
        81 => { // Concert Tom — 150Hz, resonant, long
            let f=150.0*tm; let sw=f*0.1*(-t*22.0).exp();
            advance_phase(&mut self.phase1, f+sw, sr); advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
            advance_phase(&mut self.phase3, (f+sw)*2.296, sr);
            osc_sine(self.phase1)*0.45*(-t/(0.38*dm)).exp()
                + osc_sine(self.phase2)*0.16*(-t/(0.15*dm)).exp()
                + osc_sine(self.phase3)*0.08*(-t/(0.1*dm)).exp()
        }
        82 => { // Roto High — 290Hz, bright
            let f=290.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            osc_sine(self.phase1)*0.38*(-t/(0.14*dm)).exp()
                + osc_triangle(self.phase1*2.5)*0.08*(-t/(0.06*dm)).exp()
        }
        83 => { // Tom: Timbale-ish — 360Hz, metallic ring
            let f=360.0*tm;
            advance_phase(&mut self.phase1, f, sr);
            let body=osc_sine(self.phase1)*0.35*(-t/(0.18*dm)).exp();
            let ring=self.svf1.bandpass(self.noise()*(-t*25.0).exp(), f*2.8, 12.0, sr)*0.12;
            body + ring
        }

        // ══ 8 CYMBALS (84-91) ══

        84 => { // Crash: Dark — 1.5s
            let freqs=[310.0*tm, 740.0*tm, 1300.0*tm, 2080.0*tm, 3100.0*tm, 4280.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.lowpass(m, 7500.0, 0.3, sr);
            (f*0.32+self.noise()*0.08)*(t/0.003).min(1.0)*(-t/(1.5*dm)).exp()
        }
        85 => { // Crash: Bright — 1.8s
            let freqs=[405.0*tm, 970.0*tm, 1700.0*tm, 2680.0*tm, 3950.0*tm, 5450.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(m, 2500.0, sr);
            hp*(t/0.002).min(1.0)*(-t/(1.8*dm)).exp()*0.3
        }
        86 => { // Ride: Ping — defined, controlled
            let freqs=[425.0*tm, 1010.0*tm, 1740.0*tm, 2830.0*tm, 4180.0*tm, 5750.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let f=self.svf1.bandpass(m, 5500.0, 1.0, sr);
            let ping=(-t*100.0).exp()*0.12;
            (f*(-t/(1.0*dm)).exp()+ping)*0.28
        }
        87 => { // Ride: Wash — loose, 2s
            let freqs=[390.0*tm, 935.0*tm, 1630.0*tm, 2580.0*tm, 3830.0*tm, 5290.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            m*(-t/(2.0*dm)).exp()*0.22
        }
        88 => { // Ride Bell — tonal, 0.8s
            advance_phase(&mut self.phase1, 760.0*tm, sr);
            advance_phase(&mut self.phase2, 1140.0*tm, sr);
            advance_phase(&mut self.phase3, 1710.0*tm, sr);
            (osc_sine(self.phase1)*0.28+osc_sine(self.phase2)*0.22+osc_sine(self.phase3)*0.14)*(-t/(0.8*dm)).exp()
        }
        89 => { // Splash — quick bright, 0.5s
            let freqs=[460.0*tm, 1100.0*tm, 1880.0*tm, 2950.0*tm, 4350.0*tm, 5980.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(m, 3500.0, sr);
            hp*(t/0.001).min(1.0)*(-t/(0.5*dm)).exp()*0.3
        }
        90 => { // China — trashy, 1.2s
            let freqs=[285.0*tm, 685.0*tm, 1210.0*tm, 1930.0*tm, 2880.0*tm, 3990.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let dist=(m*2.2).tanh()*0.45;
            dist*(t/0.002).min(1.0)*(-t/(1.2*dm)).exp()*0.3
        }
        91 => { // Cymbal: Sizzle — riveted, 1.8s
            let freqs=[355.0*tm, 852.0*tm, 1505.0*tm, 2380.0*tm, 3540.0*tm, 4890.0*tm];
            let m=self.hat_oscs.tick(sr, &freqs);
            let rattle=self.noise()*(t*28.0*TAU).sin().abs()*(-t*3.5).exp();
            let rf=self.svf1.bandpass(rattle, 8500.0, 3.5, sr)*0.1;
            m*(-t/(1.8*dm)).exp()*0.22+rf
        }

        // ══ 20 PERCUSSION (92-111) ══

        92 => { // Tambourine
            let freqs=[4600.0*tm, 6300.0*tm, 7900.0*tm, 9600.0*tm, 11300.0*tm, 13100.0*tm];
            let j=self.hat_oscs.tick(sr, &freqs);
            let hp=self.hp1.tick_hp(j, 4000.0, sr);
            let shake=(t*22.0*TAU).sin().abs()*(-t*6.0).exp();
            hp*((-t/(0.2*dm)).exp()+shake*0.2)*0.25
        }
        93 => { // Shaker: Tight
            let f=self.svf1.bandpass(self.noise(), 7200.0, 1.3, sr);
            self.hp1.tick_hp(f, 5000.0, sr)*(-t/(0.07*dm)).exp()*0.3
        }
        94 => { // Shaker: Long
            let f=self.svf1.bandpass(self.noise(), 8000.0, 1.5, sr);
            let swish=(t*14.0).sin().abs()*(-t*4.0).exp();
            f*((-t/(0.15*dm)).exp()+swish*0.15)*0.25
        }
        95 => { // Cowbell
            advance_phase(&mut self.phase1, 580.0*tm, sr); advance_phase(&mut self.phase2, 870.0*tm, sr);
            let body=osc_sine(self.phase1)*0.35+osc_sine(self.phase2)*0.3;
            self.svf1.bandpass(body, 725.0, 4.0, sr)*(-t/(0.07*dm)).exp()
        }
        96 => { // Woodblock
            advance_phase(&mut self.phase1, 1900.0*tm, sr); advance_phase(&mut self.phase2, 3100.0*tm, sr);
            (osc_sine(self.phase1)*0.3+osc_sine(self.phase2)*0.15+self.noise()*(-t*1000.0).exp()*0.08)
                *(-t/(0.015*dm)).exp()
        }
        97 => { // Clave
            advance_phase(&mut self.phase1, 2500.0*tm, sr);
            osc_sine(self.phase1)*0.4*(-t/(0.022*dm)).exp()
        }
        98 => { // Triangle
            advance_phase(&mut self.phase1, 1200.0*tm, sr); advance_phase(&mut self.phase2, 3600.0*tm, sr);
            (osc_sine(self.phase1)*0.3+osc_sine(self.phase2)*0.18)*(-t/(0.9*dm)).exp()
        }
        99 => { // Cabasa
            self.svf1.bandpass(self.noise(), 8800.0, 2.0, sr)*(-t/(0.1*dm)).exp()*0.28
        }
        100 => { // Guiro
            let f=self.svf1.bandpass(self.noise(), 4200.0, 3.0, sr);
            let scrape=(t*38.0*TAU).sin().abs()*(-t*4.5).exp();
            f*(scrape*0.4+0.15)*(-t/(0.22*dm)).exp()
        }
        101 => { // Vibraslap
            let f=self.svf1.bandpass(self.noise(), 3400.0, 5.5, sr);
            let rattle=(t*36.0*TAU).sin().abs()*(-t*2.8).exp();
            f*rattle*(-t/(0.5*dm)).exp()*0.25
        }
        102 => { // Maracas
            let hp=self.hp1.tick_hp(self.noise(), 6500.0, sr);
            hp*(-t/(0.045*dm)).exp()*0.25
        }
        103 => { // Agogo High
            advance_phase(&mut self.phase1, 930.0*tm, sr); advance_phase(&mut self.phase2, 1398.0*tm, sr);
            (osc_sine(self.phase1)*0.33+osc_sine(self.phase2)*0.24)*(-t/(0.16*dm)).exp()
        }
        104 => { // Agogo Low
            advance_phase(&mut self.phase1, 670.0*tm, sr); advance_phase(&mut self.phase2, 1008.0*tm, sr);
            (osc_sine(self.phase1)*0.33+osc_sine(self.phase2)*0.24)*(-t/(0.16*dm)).exp()
        }
        105 => { // Conga: Open
            let f=335.0*tm; advance_phase(&mut self.phase1, f+f*0.06*(-t*42.0).exp(), sr);
            osc_sine(self.phase1)*0.5*(-t/(0.22*dm)).exp() + self.noise()*(-t*450.0).exp()*0.08
        }
        106 => { // Conga: Mute
            advance_phase(&mut self.phase1, 320.0*tm, sr);
            osc_sine(self.phase1)*0.42*(-t/(0.06*dm)).exp()
        }
        107 => { // Conga: Slap
            advance_phase(&mut self.phase1, 355.0*tm, sr);
            let body=osc_sine(self.phase1)*0.25*(-t/(0.04*dm)).exp();
            body + self.svf1.bandpass(self.noise()*(-t*700.0).exp(), 2800.0, 2.0, sr)*0.25
        }
        108 => { // Bongo: High
            advance_phase(&mut self.phase1, 425.0*tm+425.0*tm*0.08*(-t*65.0).exp(), sr);
            osc_sine(self.phase1)*0.42*(-t/(0.1*dm)).exp()
        }
        109 => { // Bongo: Low
            advance_phase(&mut self.phase1, 315.0*tm+315.0*tm*0.07*(-t*50.0).exp(), sr);
            osc_sine(self.phase1)*0.45*(-t/(0.13*dm)).exp()
        }
        110 => { // Timbale High
            let f=530.0*tm; advance_phase(&mut self.phase1, f, sr);
            let body=osc_sine(self.phase1)*0.38;
            let ring=self.svf1.bandpass(self.noise()*(-t*18.0).exp(), f*3.0, 10.0, sr)*0.1;
            (body+ring)*(-t/(0.2*dm)).exp()
        }
        111 => { // Timbale Low
            let f=370.0*tm; advance_phase(&mut self.phase1, f, sr);
            let body=osc_sine(self.phase1)*0.4;
            let ring=self.svf1.bandpass(self.noise()*(-t*22.0).exp(), f*2.5, 8.0, sr)*0.08;
            (body+ring)*(-t/(0.22*dm)).exp()
        }

        _ => 0.0,
        }
    }

    // ══════════════════════════════════════════════════════════════════════════
}
